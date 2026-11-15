//! CLI tool for chronos: git-like SHA-256 repository management under `.chronos/`.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chronos::{
    decode_commit, decode_tree, from_hex, to_hex, validate_entry_name, Change, DirStore, Mode,
    Repo, Signature,
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

  add PATH...
      Stage one or more files into the repository index.
      Example: chronos add README.md src/main.rs

  commit -m MSG
      Record a new commit from the staged index.
      Example: chronos commit -m \"initial commit\"

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
      Show staged changes relative to HEAD.
      Example: chronos status

  serve [--addr HOST:PORT] [--push-token-file PATH]
      Serve /repo over HTTP. Fetch is open; POST /push needs the token in
      the file (Authorization: Bearer TOKEN) and returns 403 without one.
      Example: chronos serve --push-token-file ~/.chronos-push-token"
}

enum CliError {
    Usage(String),
    Op(String),
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

fn rel_repo_path(
    repo_root: &Path,
    cwd: &Path,
    target: &Path,
) -> Result<(PathBuf, String), CliError> {
    let canon_root = fs::canonicalize(repo_root)
        .map_err(|e| CliError::Op(format!("cannot resolve repo root: {e}")))?;
    let joined = if target.is_absolute() {
        target.to_path_buf()
    } else {
        cwd.join(target)
    };
    let canon_file = fs::canonicalize(&joined)
        .map_err(|e| CliError::Op(format!("cannot resolve '{}': {e}", target.display())))?;
    if !canon_file.is_file() {
        return Err(CliError::Op(format!(
            "'{}' is not a regular file",
            target.display()
        )));
    }
    let rel = canon_file
        .strip_prefix(&canon_root)
        .map_err(|_| CliError::Op(format!("'{}' is outside the repository", target.display())))?;
    let mut parts = Vec::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(s) => {
                let s_str = s
                    .to_str()
                    .ok_or_else(|| CliError::Op("non-UTF-8 path component".to_string()))?;
                parts.push(s_str);
            }
            _ => {
                return Err(CliError::Op(format!("invalid path '{}'", target.display())));
            }
        }
    }
    if parts.first().copied() == Some(".chronos") || parts.is_empty() {
        return Err(CliError::Op(format!("cannot add '{}'", target.display())));
    }
    let rel_path = parts.join("/");
    Ok((canon_file, rel_path))
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

fn cmd_add(args: &[String], base_dir: Option<&Path>) -> Result<(), CliError> {
    if args.is_empty() {
        return Err(CliError::Usage("add expects at least one PATH".to_string()));
    }
    let cwd = match base_dir {
        Some(b) => b.to_path_buf(),
        None => env::current_dir()
            .map_err(|e| CliError::Op(format!("cannot get current directory: {e}")))?,
    };
    let (root, mut repo) = open_repo(Some(&cwd))?;
    for arg in args {
        let (disk_path, rel_path) = rel_repo_path(&root, &cwd, Path::new(arg))?;
        let bytes = fs::read(&disk_path)
            .map_err(|e| CliError::Op(format!("failed to read '{arg}': {e}")))?;
        let meta = fs::metadata(&disk_path)
            .map_err(|e| CliError::Op(format!("failed to stat '{arg}': {e}")))?;
        let exec = is_executable(&meta);
        repo.add(&rel_path, &bytes, exec)
            .map_err(|e| CliError::Op(format!("failed to stage '{arg}': {e}")))?;
    }
    Ok(())
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
    let msg = message.ok_or_else(|| CliError::Usage("commit requires -m MSG".to_string()))?;
    if msg.is_empty() {
        return Err(CliError::Op("commit message cannot be empty".to_string()));
    }
    let (_root, mut repo) = open_repo(base_dir)?;
    if repo.status().is_empty() {
        return Err(CliError::Op("nothing to commit".to_string()));
    }
    let sig = Signature {
        name: "Monolith".to_string(),
        email: "monolith@local".to_string(),
        time: current_unix_secs(),
        tz_minutes: 0,
    };
    let id = repo
        .commit(msg, &sig)
        .map_err(|e| CliError::Op(format!("commit failed: {e}")))?;
    println!("{}", to_hex(&id));
    Ok(())
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
    for id in ids {
        let (_, raw) = repo
            .get(&id)
            .map_err(|e| CliError::Op(format!("failed to read commit: {e}")))?;
        let commit =
            decode_commit(&raw).map_err(|e| CliError::Op(format!("corrupt commit: {e}")))?;
        let subject = commit.message.lines().next().unwrap_or("");
        println!("{} {subject}", to_hex(&id));
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
    let old_files = repo.staged();
    let new_files = repo
        .checkout(name)
        .map_err(|e| CliError::Op(format!("checkout '{name}' failed: {e}")))?;

    let mut targets = Vec::with_capacity(new_files.len());
    for (rel, (bytes, exec)) in &new_files {
        targets.push((disk_path_for_rel(&root, rel)?, bytes, *exec));
    }
    for old_rel in old_files.keys() {
        if !new_files.contains_key(old_rel) {
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
    let (_root, repo) = open_repo(base_dir)?;
    if let Some(branch) = repo.current_branch() {
        println!("on branch {branch}");
    } else if let Some(head) = repo.head() {
        println!("HEAD detached at {}", to_hex(&head));
    } else {
        println!("on branch main (no commits)");
    }
    for ch in repo.status() {
        match ch {
            Change::Added(p) => println!("A {p}"),
            Change::Modified(p) => println!("M {p}"),
            Change::Deleted(p) => println!("D {p}"),
        }
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
                "checkout" => cmd_checkout(rest, base_ref),
                "cat-file" => cmd_cat_file(rest, base_ref),
                "status" => cmd_status(rest, base_ref),
                "serve" => cmd_serve(rest, base_ref),
                other => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
            }
        }
    }
}

fn main() {
    if let Err(err) = run() {
        match err {
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
