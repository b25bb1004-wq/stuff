//! Chronos SHA-256 content-addressed VCS demo.

use std::fmt::Write as _;

use chronos::{decode_commit, hash_object, to_hex, Id, Kind, MemStore, Repo, Signature};

use super::{check_input, Demo, Output};

const MAX_LINES: usize = 256;
const BASE_TIME: i64 = 1_700_000_000;

const DEFAULT_SCRIPT: &str = "\
write README.md Monolith Chronos VCS
write src/main.lg fn main() {}
commit initial commit
branch feat
checkout feat
write src/util.lg fn helper() -> int { 1 }
commit add helper
log
checkout main";

/// Registered Chronos version control demo.
pub const DEMO: Demo = Demo {
    id: "chronos",
    title: "Chronos VCS",
    default_input: DEFAULT_SCRIPT,
    run,
};

/// Execute `write`, `commit`, `branch`, `checkout`, and `log` commands on an in-memory `Repo`.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let mut repo = Repo::init(MemStore::new());
    let mut out = String::new();
    let mut commit_seq = 0i64;
    let mut ran_any = false;
    let mut line_count = 0usize;

    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        line_count += 1;
        if line_count > MAX_LINES {
            return Err(format!("script exceeds {MAX_LINES} lines"));
        }
        ran_any = true;
        exec_line(&mut repo, line_no, line, &mut commit_seq, &mut out)?;
    }

    if !ran_any {
        return Err(String::from("empty chronos script"));
    }
    Ok(Output::Text(out))
}

fn exec_line(
    repo: &mut Repo<MemStore>,
    line_no: usize,
    line: &str,
    commit_seq: &mut i64,
    out: &mut String,
) -> Result<(), String> {
    let (cmd, rest) = split_first_word(line);
    match cmd {
        "write" => {
            let (path, text) = split_first_word(rest);
            if path.is_empty() {
                return Err(format!("line {line_no}: write requires PATH [TEXT]"));
            }
            repo.add(path, text.as_bytes(), false)
                .map_err(|e| format!("line {line_no}: write {path}: {e}"))?;
            let blob_id = hash_object(Kind::Blob, text.as_bytes());
            let _ = writeln!(
                out,
                "write {path} -> blob {} ({} bytes)",
                short_id(&blob_id),
                text.len()
            );
        }
        "commit" => {
            let msg = rest.trim();
            if msg.is_empty() {
                return Err(format!("line {line_no}: commit requires MSG"));
            }
            *commit_seq = commit_seq.saturating_add(1);
            let sig = Signature {
                name: String::from("Monolith Demo"),
                email: String::from("demo@monolith.local"),
                time: BASE_TIME.saturating_add(*commit_seq),
                tz_minutes: 0,
            };
            let cid = repo
                .commit(msg, &sig)
                .map_err(|e| format!("line {line_no}: commit: {e}"))?;
            let (kind, bytes) = repo
                .get(&cid)
                .map_err(|e| format!("line {line_no}: read commit: {e}"))?;
            if kind != Kind::Commit {
                return Err(format!("line {line_no}: unexpected object kind"));
            }
            let commit =
                decode_commit(&bytes).map_err(|e| format!("line {line_no}: decode commit: {e}"))?;
            let files = repo
                .files(&cid)
                .map_err(|e| format!("line {line_no}: read tree: {e}"))?;
            let branch = repo
                .current_branch()
                .unwrap_or_else(|| String::from("DETACHED"));
            let _ = writeln!(
                out,
                "commit [{branch}] {} tree {} \"{msg}\"",
                short_id(&cid),
                short_id(&commit.tree)
            );
            for (path, (content, _)) in &files {
                let bid = hash_object(Kind::Blob, content);
                let _ = writeln!(
                    out,
                    "  tree: {path} (blob {}, {} bytes)",
                    short_id(&bid),
                    content.len()
                );
            }
        }
        "branch" => {
            let name = require_single_arg(line_no, "branch", rest)?;
            let tip = repo
                .head()
                .ok_or_else(|| format!("line {line_no}: branch {name}: no commits yet"))?;
            repo.branch(name, &tip)
                .map_err(|e| format!("line {line_no}: branch {name}: {e}"))?;
            let _ = writeln!(out, "branch {name} -> {}", short_id(&tip));
        }
        "checkout" => {
            let target = require_single_arg(line_no, "checkout", rest)?;
            let files = repo
                .checkout(target)
                .map_err(|e| format!("line {line_no}: checkout {target}: {e}"))?;
            let _ = writeln!(out, "checkout {target} ({} files):", files.len());
            for (path, (content, _)) in &files {
                let bid = hash_object(Kind::Blob, content);
                let _ = writeln!(
                    out,
                    "  {path} (blob {}, {} bytes)",
                    short_id(&bid),
                    content.len()
                );
            }
        }
        "log" => {
            if !rest.trim().is_empty() {
                return Err(format!("line {line_no}: log takes no arguments"));
            }
            let Some(tip) = repo.head() else {
                return Err(format!("line {line_no}: log: no commits yet"));
            };
            let ids = repo
                .log(&tip)
                .map_err(|e| format!("line {line_no}: log: {e}"))?;
            let _ = writeln!(out, "log ({} commits):", ids.len());
            for id in ids {
                let (_, raw) = repo
                    .get(&id)
                    .map_err(|e| format!("line {line_no}: log object: {e}"))?;
                let c =
                    decode_commit(&raw).map_err(|e| format!("line {line_no}: log decode: {e}"))?;
                let _ = writeln!(
                    out,
                    "  {} tree {} {}",
                    short_id(&id),
                    short_id(&c.tree),
                    c.message
                );
            }
        }
        _ => {
            return Err(format!("line {line_no}: unknown command `{cmd}`"));
        }
    }
    Ok(())
}

fn short_id(id: &Id) -> String {
    let hex = to_hex(id);
    hex[..12].to_owned()
}

fn split_first_word(s: &str) -> (&str, &str) {
    let trimmed = s.trim_start();
    match trimmed.find(|c: char| c.is_ascii_whitespace()) {
        Some(idx) => {
            let head = &trimmed[..idx];
            let tail = trimmed[idx..].trim_start();
            (head, tail)
        }
        None => (trimmed, ""),
    }
}

fn require_single_arg<'a>(line_no: usize, cmd: &str, rest: &'a str) -> Result<&'a str, String> {
    let arg = rest.trim();
    if arg.is_empty() || arg.contains(|c: char| c.is_ascii_whitespace()) {
        return Err(format!("line {line_no}: {cmd} expects one argument"));
    }
    Ok(arg)
}
