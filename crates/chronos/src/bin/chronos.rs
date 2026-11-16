//! CLI tool for chronos: git-like SHA-256 repository management under `.chronos/`.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Component, Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chronos::git;
use chronos::ignore::Rules;
use chronos::{
    decode_commit, decode_tree, from_hex, to_hex, unified, validate_entry_name, Change, DirStore,
    Error, Mode, Repo, Signature,
};

const VERSION_STR: &str = "chronos 0.1.0";

fn usage() -> &'static str {
    "\
Usage:
  chronos [-C DIR] <subcommand> [options] [args]
  chronos --help
  chronos --version

Subcommands:
  init [DIR]
      Initialize a new repository under DIR/.chronos/ (default '.').
      Example: chronos init myrepo

  add [-f] [-A] [PATH...]
      Stage files, or whole directories (recursively), into the index. Honours .gitignore
      (root and nested files); a tracked path is never ignored. Deleted tracked paths are
      staged as removals. -A/--all with no PATH stages the whole work tree. -f/--force adds
      ignored paths that are named explicitly.
      Example: chronos add README.md src/main.rs
      Example: chronos add -A

  commit [-m MSG]
      Record a new commit from the staged index. Without -m the message comes from
      $EDITOR (or $VISUAL, else calliope in a terminal); with neither and no terminal
      the commit fails at once. Author is CHRONOS_NAME and CHRONOS_EMAIL, else git's
      user.name and user.email, else Monolith.
      Example: chronos commit -m \"initial commit\"

  diff [--staged|--cached]
      Show the unified diff of unstaged edits (working tree against the index),
      or with --staged/--cached the staged changes (index against HEAD).
      Example: chronos diff

  switch NAME
      Same as checkout NAME. Local changes are carried over when no changed path differs
      between HEAD and NAME; otherwise the switch is refused and the paths are named.
      Example: chronos switch main

  import GIT_DIR
      Import all branches and tags of the git repository GIT_DIR into the empty
      directory (default '.') as a chronos repository, and check out HEAD there.
      Example: chronos -C ~/work/strata-chronos import C:/Users/ADMIN/strata

  export GIT_DIR
      Append chronos commits that git does not have yet to the git repository
      GIT_DIR (via git fast-import). Repeat runs only send new work.
      Example: chronos export C:/Users/ADMIN/strata

  log
      Show commits reachable from HEAD, newest first.
      Example: chronos log

  branch [NAME]
      List branches or create a new branch at HEAD.
      Example: chronos branch feature

  checkout NAME
      Switch to branch or revision NAME and update working files.
      Example: chronos checkout feature

  cat-file ID
      Print the contents of object ID (blob, tree, or commit).
      Example: chronos cat-file HEAD

  status
      Show staged changes (against HEAD), unstaged edits (against the index), and
      untracked, non-ignored files ('? PATH'; an untracked folder is shown once as 'dir/').
      Example: chronos status

  restore --staged PATH...
      Unstage: reset the index entries for PATH to HEAD's (removed if HEAD lacks them).
      The work tree is not touched.
      Example: chronos restore --staged src/main.rs

  reset
      Unstage everything: the whole index goes back to HEAD. The work tree is not touched.
      Example: chronos reset

  rm [-f] [--cached] PATH...
      Remove tracked paths from the index and the work tree. Refuses when a path has
      uncommitted changes unless -f. --cached keeps the file on disk.
      Example: chronos rm --cached notes.txt

  mv SRC DST
      Rename a tracked file in the work tree and the index. If DST is a directory, SRC
      moves into it.
      Example: chronos mv old.txt new.txt

  serve [--addr HOST:PORT] [--push-token-file PATH]
      Serve /repo over HTTP. Fetch is open; POST /push needs the token in
      the file (Authorization: Bearer TOKEN) and returns 403 without one.
      Example: chronos serve --push-token-file ~/.chronos-push-token"
}

enum CliError {
    Usage(String),
    Op(String),
    /// stdout was closed by the reader (`chronos log | head`): stop quietly with exit 0.
    Closed,
}

fn stdout_err(e: io::Error) -> CliError {
    if e.kind() == io::ErrorKind::BrokenPipe {
        CliError::Closed
    } else {
        CliError::Op(format!("failed to write stdout: {e}"))
    }
}

fn find_repo_root(start: &Path) -> Result<PathBuf, CliError> {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join(".chronos").join("HEAD").is_file() {
            return Ok(cur);
        }
        if !cur.pop() {
            return Err(CliError::Op(
                "not a chronos repository (missing .chronos/)".to_string(),
            ));
        }
    }
}

fn open_repo(work_dir: Option<&Path>) -> Result<(PathBuf, Repo<DirStore>), CliError> {
    let start = match work_dir {
        Some(d) => d.to_path_buf(),
        None => env::current_dir()
            .map_err(|e| CliError::Op(format!("cannot get current directory: {e}")))?,
    };
    let root = find_repo_root(&start)?;
    let store = DirStore::new(root.join(".chronos"));
    let repo = Repo::open(store).map_err(|e| CliError::Op(format!("cannot open repo: {e}")))?;
    Ok((root, repo))
}

fn current_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Repo-relative path of `arg` ("" for the root), resolved from `cwd`. Lexical below the
/// canonical repo root, so the path may already be gone from disk (a deleted file can still be
/// removed from the index). Rejects paths outside the repository.
fn repo_rel(root: &Path, cwd: &Path, arg: &str) -> Result<String, CliError> {
    let outside = || CliError::Op(format!("'{arg}' is outside the repository"));
    let canon_root = fs::canonicalize(root)
        .map_err(|e| CliError::Op(format!("cannot resolve repo root: {e}")))?;
    let p = Path::new(arg);
    let (start, rest) = if p.is_absolute() {
        let abs = fs::canonicalize(p)
            .map_err(|e| CliError::Op(format!("cannot resolve '{arg}': {e}")))?;
        let rel = abs.strip_prefix(&canon_root).map_err(|_| outside())?;
        (rel.to_path_buf(), PathBuf::new())
    } else {
        let canon_cwd =
            fs::canonicalize(cwd).map_err(|e| CliError::Op(format!("cannot resolve cwd: {e}")))?;
        let rel = canon_cwd.strip_prefix(&canon_root).map_err(|_| outside())?;
        (rel.to_path_buf(), p.to_path_buf())
    };
    let mut parts: Vec<String> = Vec::new();
    for comp in start.components().chain(rest.components()) {
        match comp {
            Component::Normal(s) => parts.push(
                s.to_str()
                    .ok_or_else(|| CliError::Op("non-UTF-8 path component".to_string()))?
                    .to_string(),
            ),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop().ok_or_else(outside)?;
            }
            _ => return Err(CliError::Op(format!("invalid path '{arg}'"))),
        }
    }
    if matches!(parts.first().map(String::as_str), Some(".chronos" | ".git")) {
        return Err(CliError::Op(format!("cannot add '{arg}'")));
    }
    Ok(parts.join("/"))
}

/// True if `path` is `dir` or lies under it. The empty `dir` is the whole tree.
fn under(path: &str, dir: &str) -> bool {
    dir.is_empty() || path == dir || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

/// True if some indexed path lies strictly under `dir`.
fn tracked_under(tracked: &BTreeSet<String>, dir: &str) -> bool {
    if dir.is_empty() {
        return !tracked.is_empty();
    }
    let prefix = format!("{dir}/");
    tracked
        .range(prefix.clone()..)
        .next()
        .is_some_and(|k| k.starts_with(&prefix))
}

/// Where the work tree walk starts and which ignore rules apply.
struct Walk<'a> {
    root: &'a Path,
    /// Apply .gitignore rules (off for `add -f`).
    honour: bool,
    /// Indexed paths: never ignored, and an ignored directory holding one is still entered.
    tracked: &'a BTreeSet<String>,
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    (meta.permissions().mode() & 0o111) != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn set_executable(path: &Path, exec: bool) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        let mode = if exec {
            perms.mode() | 0o755
        } else {
            perms.mode() & !0o111
        };
        perms.set_mode(mode);
        let _ = fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _exec: bool) {}

fn cmd_init(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    if args.len() > 1 {
        return Err(CliError::Usage("init expects at most one DIR".to_string()));
    }
    let target = match (args.first(), base_dir) {
        (Some(d), Some(b)) => {
            let p = Path::new(d);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                b.join(p)
            }
        }
        (Some(d), None) => PathBuf::from(d),
        (None, Some(b)) => b.to_path_buf(),
        (None, None) => env::current_dir()
            .map_err(|e| CliError::Op(format!("cannot get current directory: {e}")))?,
    };
    if target.is_file() {
        return Err(CliError::Op(format!(
            "'{}' is a file, not a directory",
            target.display()
        )));
    }
    let chronos_dir = target.join(".chronos");
    fs::create_dir_all(&chronos_dir)
        .map_err(|e| CliError::Op(format!("failed to create '{}': {e}", chronos_dir.display())))?;
    let _repo = Repo::init(DirStore::new(&chronos_dir));
    println!(
        "initialized chronos repository in {}",
        chronos_dir.display()
    );
    Ok(())
}

fn cwd_or_current(base_dir: Option<&Path>) -> Result<PathBuf, CliError> {
    match base_dir {
        Some(b) => Ok(b.to_path_buf()),
        None => env::current_dir()
            .map_err(|e| CliError::Op(format!("cannot get current directory: {e}"))),
    }
}

/// Split `args` into flags (`-f`/`--force`, `-A`/`--all`, `--cached`, `--staged`) and paths.
fn split_flags<'a>(
    args: &'a [String],
    flags: &[&str],
) -> Result<(Vec<&'a str>, Vec<&'a str>), CliError> {
    let mut seen = Vec::new();
    let mut paths = Vec::new();
    for arg in args {
        if flags.contains(&arg.as_str()) {
            seen.push(arg.as_str());
        } else if arg.starts_with('-') && arg != "-" {
            return Err(CliError::Usage(format!("unknown option '{arg}'")));
        } else {
            paths.push(arg.as_str());
        }
    }
    Ok((seen, paths))
}

fn cmd_add(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    let (flags, paths) = split_flags(args, &["-f", "--force", "-A", "--all"])?;
    let force = flags.iter().any(|f| matches!(*f, "-f" | "--force"));
    let all = flags.iter().any(|f| matches!(*f, "-A" | "--all"));
    if paths.is_empty() && !all {
        return Err(CliError::Usage("add expects at least one PATH".to_string()));
    }
    let cwd = cwd_or_current(base_dir)?;
    let (root, mut repo) = open_repo(Some(&cwd))?;
    let tracked: BTreeSet<String> = repo.staged().into_keys().collect();

    // Resolve every argument before touching the index, so a bad one stages nothing.
    let targets: Vec<(&str, String)> = if paths.is_empty() {
        vec![(".", String::new())]
    } else {
        let mut v = Vec::new();
        for p in paths {
            v.push((p, repo_rel(&root, &cwd, p)?));
        }
        v
    };
    let mut stage: BTreeSet<String> = BTreeSet::new();
    let mut remove: BTreeSet<String> = BTreeSet::new();
    for (shown, rel) in targets {
        let meta = if rel.is_empty() {
            fs::metadata(&root).ok()
        } else {
            disk_path_for_rel(&root, &rel)
                .ok()
                .and_then(|p| fs::metadata(p).ok())
        };
        let mut rules = Rules::new();
        if !force {
            preload_rules(&root, &rel, &mut rules)?;
        }
        let walk = Walk {
            root: &root,
            honour: !force,
            tracked: &tracked,
        };
        match meta {
            Some(m) if m.is_dir() => {
                let ignored = !force
                    && !rel.is_empty()
                    && rules.is_ignored(&rel, true)
                    && !tracked_under(&tracked, &rel);
                if ignored {
                    return Err(ignored_error(shown));
                }
                let mut found = Vec::new();
                walk_dir(&walk, &rel, &mut rules, &mut found)?;
                stage.extend(found);
            }
            Some(_) => {
                let ignored = !force && rules.is_ignored(&rel, false) && !tracked.contains(&rel);
                if ignored {
                    return Err(ignored_error(shown));
                }
                stage.insert(rel.clone());
            }
            None => {
                if !tracked.iter().any(|k| under(k, &rel)) {
                    return Err(CliError::Op(format!(
                        "cannot stat '{shown}': no such file or directory"
                    )));
                }
            }
        }
        // Tracked paths under the target that are gone from disk are staged as removals.
        for k in tracked.iter().filter(|k| under(k, &rel)) {
            if !disk_path_for_rel(&root, k).is_ok_and(|p| p.is_file()) {
                remove.insert(k.clone());
            }
        }
    }

    for k in &remove {
        repo.rm(k)
            .map_err(|e| CliError::Op(format!("failed to unstage '{k}': {e}")))?;
    }
    for rel in &stage {
        let disk = disk_path_for_rel(&root, rel)?;
        let bytes =
            fs::read(&disk).map_err(|e| CliError::Op(format!("failed to read '{rel}': {e}")))?;
        let meta = fs::metadata(&disk)
            .map_err(|e| CliError::Op(format!("failed to stat '{rel}': {e}")))?;
        repo.add(rel, &bytes, is_executable(&meta))
            .map_err(|e| CliError::Op(format!("failed to stage '{rel}': {e}")))?;
    }
    Ok(())
}

fn ignored_error(shown: &str) -> CliError {
    CliError::Op(format!(
        "paths are ignored by a .gitignore file: {shown}; use -f to add them"
    ))
}

/// Load the .gitignore files above `rel`: the root's and each parent's. `walk_dir` loads `rel`'s.
fn preload_rules(root: &Path, rel: &str, rules: &mut Rules) -> Result<(), CliError> {
    if rel.is_empty() {
        return Ok(());
    }
    load_gitignore(root, "", rules)?;
    let comps: Vec<&str> = rel.split('/').collect();
    let mut dir = String::new();
    for comp in &comps[..comps.len() - 1] {
        dir = if dir.is_empty() {
            comp.to_string()
        } else {
            format!("{dir}/{comp}")
        };
        load_gitignore(root, &dir, rules)?;
    }
    Ok(())
}

fn load_gitignore(root: &Path, rel: &str, rules: &mut Rules) -> Result<(), CliError> {
    let dir = if rel.is_empty() {
        root.to_path_buf()
    } else {
        disk_path_for_rel(root, rel)?
    };
    if let Ok(bytes) = fs::read(dir.join(".gitignore")) {
        rules.add(rel, &String::from_utf8_lossy(&bytes));
    }
    Ok(())
}

/// Collect the repo-relative files under `rel` ("" for the root) that `add` may stage: not
/// ignored, or tracked. Skips `.chronos` and `.git`, and does not enter ignored directories
/// unless they hold a tracked path.
fn walk_dir(w: &Walk, rel: &str, rules: &mut Rules, out: &mut Vec<String>) -> Result<(), CliError> {
    let dir = if rel.is_empty() {
        w.root.to_path_buf()
    } else {
        disk_path_for_rel(w.root, rel)?
    };
    if w.honour {
        load_gitignore(w.root, rel, rules)?;
    }
    let mut names = Vec::new();
    let entries = fs::read_dir(&dir)
        .map_err(|e| CliError::Op(format!("cannot read '{}': {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| CliError::Op(format!("cannot read entry: {e}")))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| CliError::Op("non-UTF-8 path component".to_string()))?;
        names.push(name);
    }
    names.sort();
    for name in names {
        if name == ".chronos" || name == ".git" {
            continue;
        }
        let child = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let meta = fs::metadata(dir.join(&name))
            .map_err(|e| CliError::Op(format!("cannot stat '{child}': {e}")))?;
        if meta.is_dir() {
            if w.honour && rules.is_ignored(&child, true) && !tracked_under(w.tracked, &child) {
                continue;
            }
            walk_dir(w, &child, rules, out)?;
        } else if !w.honour || !rules.is_ignored(&child, false) || w.tracked.contains(&child) {
            out.push(child);
        }
    }
    Ok(())
}

/// Author for new commits: CHRONOS_NAME/CHRONOS_EMAIL, then git's user settings, then Monolith.
fn default_identity(root: &Path) -> Signature {
    let git_value = |key: &str| -> Option<String> {
        let out = process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["config", "--get", key])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let value = String::from_utf8(out.stdout).ok()?.trim().to_string();
        (!value.is_empty()).then_some(value)
    };
    let name = env::var("CHRONOS_NAME")
        .ok()
        .or_else(|| git_value("user.name"))
        .unwrap_or_else(|| "Monolith".to_string());
    let email = env::var("CHRONOS_EMAIL")
        .ok()
        .or_else(|| git_value("user.email"))
        .unwrap_or_else(|| "monolith@local".to_string());
    Signature {
        name,
        email,
        time: current_unix_secs(),
        tz_minutes: 0,
    }
}

fn cmd_commit(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    let mut message: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "-m" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| CliError::Usage("-m requires a commit message".to_string()))?;
            if message.is_some() {
                return Err(CliError::Usage("duplicate -m flag".to_string()));
            }
            message = Some(val.as_str());
        } else if let Some(val) = arg.strip_prefix("-m=") {
            if message.is_some() {
                return Err(CliError::Usage("duplicate -m flag".to_string()));
            }
            message = Some(val);
        } else {
            return Err(CliError::Usage(format!("unexpected argument '{arg}'")));
        }
        i += 1;
    }
    if message == Some("") {
        return Err(CliError::Op("commit message cannot be empty".to_string()));
    }
    let (root, mut repo) = open_repo(base_dir)?;
    if repo.status().is_empty() {
        return Err(CliError::Op("nothing to commit".to_string()));
    }
    // Without -m the message comes from $EDITOR, as in git (checked after "nothing to commit").
    let edited;
    let msg = match message {
        Some(m) => m,
        None => {
            edited = edit_message(&root)?;
            edited.as_str()
        }
    };
    let sig = default_identity(&root);
    match repo.commit(msg, &sig) {
        Ok(id) => println!("{}", to_hex(&id)),
        Err(e) => {
            if message.is_none() {
                let _ = fs::remove_file(edit_path(&root));
            }
            return Err(CliError::Op(format!("commit failed: {e}")));
        }
    }
    Ok(())
}

fn edit_path(root: &Path) -> PathBuf {
    root.join(".chronos").join("COMMIT_EDITMSG")
}

/// Ask `$EDITOR`, `$VISUAL` or (in a terminal) `calliope` for a commit message in
/// `.chronos/COMMIT_EDITMSG`. Without an editor and without a terminal it fails before writing
/// anything. Lines that start with `#` are dropped, as git does; an empty result aborts the
/// commit, and so does any failure, which removes COMMIT_EDITMSG.
fn edit_message(root: &Path) -> Result<String, CliError> {
    let editor = ["EDITOR", "VISUAL"]
        .iter()
        .find_map(|k| env::var(k).ok().filter(|v| !v.trim().is_empty()));
    let editor = match editor {
        Some(e) => e,
        None if io::stdin().is_terminal() => "calliope".to_string(),
        None => {
            return Err(CliError::Op(
                "no commit message: use -m MSG or set EDITOR".to_string(),
            ))
        }
    };
    let path = edit_path(root);
    let template = "\n# Write the commit message above. Lines starting with # are ignored.\n# An empty message aborts the commit.\n";
    let result = fs::write(&path, template)
        .map_err(|e| CliError::Op(format!("cannot write {}: {e}", path.display())))
        .and_then(|()| run_editor(&editor, &path));
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result
}

fn run_editor(editor: &str, path: &Path) -> Result<String, CliError> {
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("calliope");
    let status = process::Command::new(program)
        .args(parts)
        .arg(path)
        .status()
        .map_err(|e| CliError::Op(format!("cannot start editor '{editor}': {e}")))?;
    if !status.success() {
        return Err(CliError::Op(format!(
            "editor '{editor}' exited with {status}; commit aborted"
        )));
    }
    let text = fs::read_to_string(path)
        .map_err(|e| CliError::Op(format!("cannot read {}: {e}", path.display())))?;
    let msg = text
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let msg = msg.trim().to_string();
    if msg.is_empty() {
        return Err(CliError::Op(
            "empty commit message: commit aborted".to_string(),
        ));
    }
    Ok(msg)
}

fn cmd_log(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    if !args.is_empty() {
        return Err(CliError::Usage("log takes no arguments".to_string()));
    }
    let (_root, repo) = open_repo(base_dir)?;
    let head = repo
        .head()
        .ok_or_else(|| CliError::Op("no commits yet".to_string()))?;
    let ids = repo
        .log(&head)
        .map_err(|e| CliError::Op(format!("log failed: {e}")))?;
    let mut out = io::stdout().lock();
    for id in ids {
        let (_, raw) = repo
            .get(&id)
            .map_err(|e| CliError::Op(format!("failed to read commit: {e}")))?;
        let commit =
            decode_commit(&raw).map_err(|e| CliError::Op(format!("corrupt commit: {e}")))?;
        let subject = commit.message.lines().next().unwrap_or("");
        writeln!(out, "{} {subject}", to_hex(&id)).map_err(stdout_err)?;
    }
    Ok(())
}

fn cmd_branch(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    if args.len() > 1 {
        return Err(CliError::Usage(
            "branch expects zero or one NAME".to_string(),
        ));
    }
    let (_root, mut repo) = open_repo(base_dir)?;
    if let Some(name) = args.first() {
        let head = repo
            .head()
            .ok_or_else(|| CliError::Op("cannot create branch: no commits yet".to_string()))?;
        repo.branch(name, &head)
            .map_err(|e| CliError::Op(format!("branch '{name}' failed: {e}")))?;
        println!("created branch {name} at {}", to_hex(&head));
    } else {
        let cur = repo.current_branch();
        for (name, _) in repo.branches() {
            if cur.as_deref() == Some(name.as_str()) {
                println!("* {name}");
            } else {
                println!("  {name}");
            }
        }
    }
    Ok(())
}

fn disk_path_for_rel(root: &Path, rel: &str) -> Result<PathBuf, CliError> {
    let mut p = root.to_path_buf();
    for comp in rel.split('/') {
        validate_entry_name(comp)
            .map_err(|_| CliError::Op(format!("refusing unsafe path '{rel}'")))?;
        p.push(comp);
    }
    let inside = p
        .strip_prefix(root)
        .is_ok_and(|rest| rest.components().all(|c| matches!(c, Component::Normal(_))));
    if !inside {
        return Err(CliError::Op(format!(
            "refusing path '{rel}' outside the work tree"
        )));
    }
    Ok(p)
}

fn prune_empty_parents(root: &Path, file_path: &Path) {
    let mut cur = file_path.parent();
    while let Some(dir) = cur {
        if dir == root {
            break;
        }
        if fs::remove_dir(dir).is_err() {
            break;
        }
        cur = dir.parent();
    }
}

fn cmd_checkout(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    if args.len() != 1 {
        return Err(CliError::Usage("checkout expects NAME".to_string()));
    }
    let name = &args[0];
    let (root, mut repo) = open_repo(base_dir)?;
    let dirty = dirty_paths(&root, &repo)?;
    let conflicts = repo
        .switch_conflicts(name, &dirty)
        .map_err(|e| CliError::Op(format!("checkout '{name}' failed: {e}")))?;
    if !conflicts.is_empty() {
        return Err(CliError::Op(format!(
            "cannot switch to '{name}': local changes to {} would be overwritten; commit or restore them first",
            conflicts.join(", ")
        )));
    }
    let old_files = repo.staged();
    let new_files = repo
        .checkout_keep(name, &dirty)
        .map_err(|e| CliError::Op(format!("checkout '{name}' failed: {e}")))?;

    let mut targets = Vec::with_capacity(new_files.len());
    for (rel, (bytes, exec)) in &new_files {
        targets.push((disk_path_for_rel(&root, rel)?, bytes, *exec));
    }
    for old_rel in old_files.keys() {
        if !new_files.contains_key(old_rel) && !dirty.contains(old_rel) {
            let p = disk_path_for_rel(&root, old_rel)?;
            if p.exists() {
                let _ = fs::remove_file(&p);
                prune_empty_parents(&root, &p);
            }
        }
    }
    for (p, bytes, exec) in &targets {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CliError::Op(format!("failed to create '{}': {e}", parent.display()))
            })?;
        }
        fs::write(p, bytes)
            .map_err(|e| CliError::Op(format!("failed to write '{}': {e}", p.display())))?;
        set_executable(p, *exec);
    }
    println!("checked out {name}");
    Ok(())
}

/// Paths with staged or unstaged changes: the local state a switch must not lose.
fn dirty_paths(root: &Path, repo: &Repo<DirStore>) -> Result<BTreeSet<String>, CliError> {
    let mut out: BTreeSet<String> = repo
        .status()
        .iter()
        .map(|c| path_of(c).to_string())
        .collect();
    for (change, _, _) in unstaged(root, repo)? {
        out.insert(path_of(&change).to_string());
    }
    Ok(out)
}

fn cmd_restore(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    let (flags, paths) = split_flags(args, &["--staged"])?;
    if flags.is_empty() {
        return Err(CliError::Usage(
            "restore only supports --staged: restore --staged PATH...".to_string(),
        ));
    }
    if paths.is_empty() {
        return Err(CliError::Usage(
            "restore --staged expects at least one PATH".to_string(),
        ));
    }
    let cwd = cwd_or_current(base_dir)?;
    let (root, mut repo) = open_repo(Some(&cwd))?;
    for arg in paths {
        let rel = repo_rel(&root, &cwd, arg)?;
        repo.unstage(&rel).map_err(|e| match e {
            Error::NotFound => CliError::Op(format!("pathspec '{arg}' did not match any file")),
            other => CliError::Op(format!("restore '{arg}' failed: {other}")),
        })?;
    }
    Ok(())
}

fn cmd_reset(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    if !args.is_empty() {
        return Err(CliError::Usage(
            "reset takes no arguments; use restore --staged PATH to unstage paths".to_string(),
        ));
    }
    let (_root, mut repo) = open_repo(base_dir)?;
    repo.unstage("")
        .map_err(|e| CliError::Op(format!("reset failed: {e}")))
}

fn cmd_rm(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    let (flags, paths) = split_flags(args, &["-f", "--force", "--cached"])?;
    let cached = flags.contains(&"--cached");
    let force = flags.iter().any(|f| matches!(*f, "-f" | "--force"));
    if paths.is_empty() {
        return Err(CliError::Usage("rm expects at least one PATH".to_string()));
    }
    let cwd = cwd_or_current(base_dir)?;
    let (root, mut repo) = open_repo(Some(&cwd))?;
    let dirty = if cached || force {
        BTreeSet::new()
    } else {
        dirty_paths(&root, &repo)?
    };
    // Check every argument before removing anything.
    let mut doomed: BTreeSet<String> = BTreeSet::new();
    for arg in paths {
        let rel = repo_rel(&root, &cwd, arg)?;
        if rel.is_empty() {
            return Err(CliError::Op(
                "refusing to remove the whole repository; name the paths".to_string(),
            ));
        }
        let matched: Vec<String> = repo
            .staged()
            .into_keys()
            .filter(|k| under(k, &rel))
            .collect();
        if matched.is_empty() {
            return Err(CliError::Op(format!(
                "pathspec '{arg}' did not match any tracked file"
            )));
        }
        for k in matched {
            if dirty.contains(&k) {
                return Err(CliError::Op(format!(
                    "'{k}' has uncommitted changes; use --cached to keep the file or -f to remove it"
                )));
            }
            doomed.insert(k);
        }
    }
    let mut out = io::stdout().lock();
    for k in &doomed {
        if !cached {
            let p = disk_path_for_rel(&root, k)?;
            if p.is_file() {
                fs::remove_file(&p)
                    .map_err(|e| CliError::Op(format!("failed to remove '{k}': {e}")))?;
                prune_empty_parents(&root, &p);
            }
        }
        repo.rm(k)
            .map_err(|e| CliError::Op(format!("failed to unstage '{k}': {e}")))?;
        writeln!(out, "rm '{k}'").map_err(stdout_err)?;
    }
    Ok(())
}

fn cmd_mv(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    let [src, dst] = args else {
        return Err(CliError::Usage("mv expects SRC DST".to_string()));
    };
    let cwd = cwd_or_current(base_dir)?;
    let (root, mut repo) = open_repo(Some(&cwd))?;
    let src_rel = repo_rel(&root, &cwd, src)?;
    let mut dst_rel = repo_rel(&root, &cwd, dst)?;
    let staged = repo.staged();
    let Some((bytes, exec)) = staged.get(&src_rel).cloned() else {
        return Err(CliError::Op(format!(
            "bad source '{src}': not a tracked file"
        )));
    };
    let src_disk = disk_path_for_rel(&root, &src_rel)?;
    if !src_disk.is_file() {
        return Err(CliError::Op(format!(
            "bad source '{src}': no such file in the work tree"
        )));
    }
    if disk_path_for_rel(&root, &dst_rel)?.is_dir() {
        let base = src_rel.rsplit('/').next().unwrap_or("");
        dst_rel = format!("{dst_rel}/{base}");
    }
    let dst_disk = disk_path_for_rel(&root, &dst_rel)?;
    if dst_disk.exists() || staged.contains_key(&dst_rel) {
        return Err(CliError::Op(format!("destination '{dst}' already exists")));
    }
    if let Some(parent) = dst_disk.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CliError::Op(format!("failed to create '{}': {e}", parent.display())))?;
    }
    fs::rename(&src_disk, &dst_disk)
        .map_err(|e| CliError::Op(format!("cannot move '{src}' to '{dst}': {e}")))?;
    if let Err(e) = repo.add(&dst_rel, &bytes, exec) {
        let _ = fs::rename(&dst_disk, &src_disk);
        return Err(CliError::Op(format!("failed to stage '{dst}': {e}")));
    }
    repo.rm(&src_rel)
        .map_err(|e| CliError::Op(format!("failed to unstage '{src}': {e}")))?;
    prune_empty_parents(&root, &src_disk);
    Ok(())
}

fn cmd_diff(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    let staged = match args {
        [] => false,
        [flag] if flag == "--staged" || flag == "--cached" => true,
        _ => {
            return Err(CliError::Usage(
                "diff takes no arguments or --staged".to_string(),
            ))
        }
    };
    let (root, repo) = open_repo(base_dir)?;
    let mut out = io::stdout().lock();
    if !staged {
        for (change, old, new) in unstaged(&root, &repo)? {
            write_diff(&mut out, path_of(&change), &old, &new).map_err(stdout_err)?;
        }
        return Ok(());
    }
    let head_files = match repo.head() {
        Some(head) => repo
            .files(&head)
            .map_err(|e| CliError::Op(format!("diff failed: {e}")))?,
        None => Default::default(),
    };
    let index = repo.staged();
    for change in repo.status() {
        let path = match change {
            Change::Added(p) | Change::Modified(p) | Change::Deleted(p) => p,
        };
        let old: &[u8] = head_files
            .get(&path)
            .map(|(b, _)| b.as_slice())
            .unwrap_or(&[]);
        let new: &[u8] = index.get(&path).map(|(b, _)| b.as_slice()).unwrap_or(&[]);
        write_diff(&mut out, &path, old, new).map_err(stdout_err)?;
    }
    Ok(())
}

fn write_diff(out: &mut impl Write, path: &str, old: &[u8], new: &[u8]) -> io::Result<()> {
    if old.contains(&0) || new.contains(&0) {
        writeln!(out, "Binary files a/{path} and b/{path} differ")
    } else {
        out.write_all(&unified(path, old, new, 3))
    }
}

fn path_of(change: &Change) -> &str {
    match change {
        Change::Added(p) | Change::Modified(p) | Change::Deleted(p) => p,
    }
}

/// (change, index bytes, disk bytes); disk bytes are empty when the file is deleted.
type Unstaged = Vec<(Change, Vec<u8>, Vec<u8>)>;

/// Index entries whose working-tree file differs (Modified) or is gone (Deleted).
fn unstaged(root: &Path, repo: &Repo<DirStore>) -> Result<Unstaged, CliError> {
    let mut out = Vec::new();
    for (path, (index_bytes, _)) in repo.staged() {
        let disk = disk_path_for_rel(root, &path)?;
        match fs::read(&disk) {
            Ok(now) if now == index_bytes => {}
            Ok(now) => out.push((Change::Modified(path), index_bytes, now)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                out.push((Change::Deleted(path), index_bytes, Vec::new()));
            }
            Err(e) => return Err(CliError::Op(format!("failed to read '{path}': {e}"))),
        }
    }
    Ok(out)
}

/// Write checked-out files under `root`. Used after an import, into an empty directory.
fn write_worktree(root: &Path, files: &chronos::Files) -> Result<(), CliError> {
    for (rel, (bytes, exec)) in files {
        let p = disk_path_for_rel(root, rel)?;
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CliError::Op(format!("failed to create '{}': {e}", parent.display()))
            })?;
        }
        fs::write(&p, bytes)
            .map_err(|e| CliError::Op(format!("failed to write '{}': {e}", p.display())))?;
        set_executable(&p, *exec);
    }
    Ok(())
}

fn print_warnings(summary: &git::Summary) {
    for line in &summary.warnings {
        eprintln!("chronos: warning: {line}");
    }
}

fn cmd_import(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    let [git_dir] = args else {
        return Err(CliError::Usage("import expects one GIT_DIR".to_string()));
    };
    let target = match base_dir {
        Some(b) => b.to_path_buf(),
        None => env::current_dir()
            .map_err(|e| CliError::Op(format!("cannot get current directory: {e}")))?,
    };
    if !target.is_dir() {
        return Err(CliError::Op(format!(
            "import target '{}' is not a directory",
            target.display()
        )));
    }
    let mut entries = fs::read_dir(&target)
        .map_err(|e| CliError::Op(format!("cannot read '{}': {e}", target.display())))?;
    if entries.next().is_some() {
        return Err(CliError::Op(format!(
            "import needs an empty directory; '{}' is not empty",
            target.display()
        )));
    }
    let chronos_dir = target.join(".chronos");
    fs::create_dir(&chronos_dir)
        .map_err(|e| CliError::Op(format!("failed to create '{}': {e}", chronos_dir.display())))?;
    let mut repo = Repo::init(DirStore::new(&chronos_dir));
    let summary = match git::import(&mut repo, Path::new(git_dir)) {
        Ok(summary) => summary,
        Err(e) => {
            let _ = fs::remove_dir_all(&chronos_dir);
            return Err(CliError::Op(format!("import failed: {e}")));
        }
    };
    if let Some(head) = repo.head() {
        let files = repo
            .files(&head)
            .map_err(|e| CliError::Op(format!("cannot read HEAD: {e}")))?;
        write_worktree(&target, &files)?;
    }
    print_warnings(&summary);
    println!(
        "imported {} commits and {} refs from {git_dir} into {}",
        summary.commits,
        summary.refs,
        chronos_dir.display()
    );
    Ok(())
}

fn cmd_export(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    let [git_dir] = args else {
        return Err(CliError::Usage("export expects one GIT_DIR".to_string()));
    };
    let (_root, mut repo) = open_repo(base_dir)?;
    let summary = git::export(&mut repo, Path::new(git_dir))
        .map_err(|e| CliError::Op(format!("export failed: {e}")))?;
    print_warnings(&summary);
    if summary.commits == 0 && summary.refs == 0 {
        println!("nothing to export to {git_dir}");
    } else {
        println!(
            "exported {} commits and {} refs to {git_dir}",
            summary.commits, summary.refs
        );
    }
    Ok(())
}

fn cmd_serve(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    let mut addr = "127.0.0.1:8080".to_string();
    let mut token_file: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--addr" => {
                addr = it
                    .next()
                    .ok_or_else(|| CliError::Usage("--addr requires HOST:PORT".to_string()))?
                    .clone();
            }
            "--push-token-file" => {
                let path = it.next().ok_or_else(|| {
                    CliError::Usage("--push-token-file requires PATH".to_string())
                })?;
                token_file = Some(PathBuf::from(path));
            }
            other => return Err(CliError::Usage(format!("serve: unknown option '{other}'"))),
        }
    }
    // Pushes stay disabled (403) unless a token file is given; the token is never taken from argv.
    let token = match token_file {
        Some(path) => {
            let raw = fs::read_to_string(&path).map_err(|e| {
                CliError::Usage(format!(
                    "cannot read push token file '{}': {e}",
                    path.display()
                ))
            })?;
            let token = raw.trim().to_string();
            if token.is_empty() {
                return Err(CliError::Usage("push token file is empty".to_string()));
            }
            Some(token)
        }
        None => None,
    };
    let (_root, repo) = open_repo(base_dir)?;
    let mut router = mercury::server::Router::new();
    chronos::remote::serve(
        Arc::new(Mutex::new(repo)),
        &mut router,
        "/repo",
        token.as_deref(),
    );
    let config = mercury::server::Config {
        addr,
        ..mercury::server::Config::default()
    };
    let server = mercury::server::Server::start(config, router)
        .map_err(|e| CliError::Op(format!("serve failed: {e}")))?;
    println!(
        "serving /repo on {} (pushes {})",
        server.local_addr(),
        if token.is_some() {
            "enabled"
        } else {
            "disabled"
        }
    );
    loop {
        std::thread::park();
    }
}

fn cmd_cat_file(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    if args.len() != 1 {
        return Err(CliError::Usage("cat-file expects ID".to_string()));
    }
    let rev = &args[0];
    let (_root, repo) = open_repo(base_dir)?;
    let id = from_hex(rev)
        .or_else(|| repo.resolve(rev))
        .ok_or_else(|| CliError::Op(format!("object '{rev}' not found")))?;
    let (kind, data) = repo
        .get(&id)
        .map_err(|e| CliError::Op(format!("cat-file '{rev}' failed: {e}")))?;
    match kind {
        chronos::Kind::Blob | chronos::Kind::Commit => {
            let mut out = io::stdout().lock();
            out.write_all(&data)
                .map_err(|e| CliError::Op(format!("failed to write stdout: {e}")))?;
        }
        chronos::Kind::Tree => {
            let entries = decode_tree(&data)
                .map_err(|e| CliError::Op(format!("corrupt tree object: {e}")))?;
            for entry in entries {
                let (mode_s, kind_s) = match entry.mode {
                    Mode::File => ("100644", "blob"),
                    Mode::Exec => ("100755", "blob"),
                    Mode::Dir => ("040000", "tree"),
                };
                println!("{mode_s} {kind_s} {}\t{}", to_hex(&entry.id), entry.name);
            }
        }
    }
    Ok(())
}

fn cmd_status(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    if !args.is_empty() {
        return Err(CliError::Usage("status takes no arguments".to_string()));
    }
    let (root, repo) = open_repo(base_dir)?;
    let mut out = io::stdout().lock();
    let head_line = if let Some(branch) = repo.current_branch() {
        format!("on branch {branch}")
    } else if let Some(head) = repo.head() {
        format!("HEAD detached at {}", to_hex(&head))
    } else {
        "on branch main (no commits)".to_string()
    };
    writeln!(out, "{head_line}").map_err(stdout_err)?;
    let staged = repo.status();
    if !staged.is_empty() {
        writeln!(out, "Changes to be committed:").map_err(stdout_err)?;
        write_changes(&mut out, staged.iter()).map_err(stdout_err)?;
    }
    let dirty = unstaged(&root, &repo)?;
    if !dirty.is_empty() {
        writeln!(out, "Changes not staged for commit:").map_err(stdout_err)?;
        write_changes(&mut out, dirty.iter().map(|(c, _, _)| c)).map_err(stdout_err)?;
    }
    let tracked: BTreeSet<String> = repo.staged().into_keys().collect();
    let untracked = untracked_paths(&root, &tracked)?;
    if !untracked.is_empty() {
        writeln!(out, "Untracked files:").map_err(stdout_err)?;
        for path in &untracked {
            writeln!(out, "? {path}").map_err(stdout_err)?;
        }
    }
    Ok(())
}

/// Files on disk that are neither indexed nor ignored. A directory with no indexed path inside
/// is shown once as `dir/`, as git does.
fn untracked_paths(root: &Path, tracked: &BTreeSet<String>) -> Result<Vec<String>, CliError> {
    let walk = Walk {
        root,
        honour: true,
        tracked,
    };
    let mut files = Vec::new();
    walk_dir(&walk, "", &mut Rules::new(), &mut files)?;
    let mut shown = BTreeSet::new();
    for file in files.iter().filter(|f| !tracked.contains(*f)) {
        let parts: Vec<&str> = file.split('/').collect();
        let collapsed = (1..parts.len())
            .map(|end| parts[..end].join("/"))
            .find(|dir| !tracked_under(tracked, dir));
        shown.insert(match collapsed {
            Some(dir) => format!("{dir}/"),
            None => file.clone(),
        });
    }
    Ok(shown.into_iter().collect())
}

fn write_changes<'a>(
    out: &mut impl Write,
    changes: impl Iterator<Item = &'a Change>,
) -> io::Result<()> {
    for ch in changes {
        let tag = match ch {
            Change::Added(_) => "A",
            Change::Modified(_) => "M",
            Change::Deleted(_) => "D",
        };
        writeln!(out, "{tag} {}", path_of(ch))?;
    }
    Ok(())
}

fn run() -> Result<(), CliError> {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    let mut base_dir: Option<PathBuf> = None;
    let mut idx = 0;
    if raw_args.first().map(String::as_str) == Some("-C") {
        let dir = raw_args
            .get(1)
            .ok_or_else(|| CliError::Usage("-C requires a directory".to_string()))?;
        base_dir = Some(PathBuf::from(dir));
        idx = 2;
    }
    let args = &raw_args[idx..];
    let Some(first) = args.first() else {
        return Err(CliError::Usage(
            "missing subcommand (try --help)".to_string(),
        ));
    };
    match first.as_str() {
        "--help" | "-h" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        "--version" | "-V" => {
            println!("{VERSION_STR}");
            Ok(())
        }
        sub => {
            let rest = &args[1..];
            if rest.iter().any(|a| a == "--help" || a == "-h") {
                println!("{}", usage());
                return Ok(());
            }
            let base_ref = base_dir.as_deref();
            match sub {
                "init" => cmd_init(rest, base_ref),
                "add" => cmd_add(rest, base_ref),
                "commit" => cmd_commit(rest, base_ref),
                "log" => cmd_log(rest, base_ref),
                "branch" => cmd_branch(rest, base_ref),
                "checkout" | "switch" => cmd_checkout(rest, base_ref),
                "cat-file" => cmd_cat_file(rest, base_ref),
                "status" => cmd_status(rest, base_ref),
                "restore" => cmd_restore(rest, base_ref),
                "reset" => cmd_reset(rest, base_ref),
                "rm" => cmd_rm(rest, base_ref),
                "mv" => cmd_mv(rest, base_ref),
                "diff" => cmd_diff(rest, base_ref),
                "import" => cmd_import(rest, base_ref),
                "export" => cmd_export(rest, base_ref),
                "serve" => cmd_serve(rest, base_ref),
                other => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
            }
        }
    }
}

fn main() {
    if let Err(err) = run() {
        match err {
            CliError::Closed => process::exit(0),
            CliError::Usage(msg) => {
                eprintln!("chronos: {msg}");
                process::exit(2);
            }
            CliError::Op(msg) => {
                eprintln!("chronos: {msg}");
                process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::disk_path_for_rel;
    use std::path::Path;

    #[test]
    fn checkout_path_rejects_windows_unsafe_names() {
        let root = Path::new("work");
        for rel in [
            "a\\b",
            "C:x",
            "dir/C:x",
            "..\\..\\evil.bat",
            "x:stream",
            "a/../b",
            "./x",
        ] {
            assert!(disk_path_for_rel(root, rel).is_err(), "{rel}");
        }
        let ok = disk_path_for_rel(root, "src/main.rs");
        assert!(ok.is_ok_and(|p| p.starts_with(root)));
    }
}
