//! Hades journaled filesystem demo.

use std::fmt::Write as _;

use hades::{fsck, mkfs, Fs, Kind, MemDisk, MkfsOptions};

use super::{check_input, Demo, Output};

const DISK_BLOCKS: u64 = 256;
const MAX_LINES: usize = 512;
const MAX_READ_BYTES: usize = 64 * 1024;

const DEFAULT_SCRIPT: &str = "\
mkdir /notes
write /notes/hello.txt Monolith Hades FS
ls /
ls /notes
cat /notes/hello.txt
write /notes/temp.txt scratch
rm /notes/temp.txt
ls /notes";

/// Registered Hades filesystem demo.
pub const DEMO: Demo = Demo {
    id: "hades",
    title: "Hades FS",
    default_input: DEFAULT_SCRIPT,
    run,
};

/// Format a 256-block `MemDisk`, run a script of filesystem commands, and run `fsck`.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let mut disk = MemDisk::new(DISK_BLOCKS);
    mkfs(&mut disk, MkfsOptions::default()).map_err(|e| format!("mkfs failed: {e:?}"))?;
    let mut fs = Fs::mount(disk).map_err(|e| format!("mount failed: {e:?}"))?;

    let mut out = String::new();
    let mut ran_any = false;
    let mut line_count = 0usize;

    for (idx, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        line_count += 1;
        if line_count > MAX_LINES {
            return Err(format!("script exceeds {MAX_LINES} lines"));
        }
        ran_any = true;
        exec_line(&mut fs, idx + 1, line, &mut out)?;
    }

    if !ran_any {
        return Err(String::from("empty filesystem script"));
    }

    let mut disk = fs.unmount().map_err(|e| format!("unmount failed: {e:?}"))?;
    let report = fsck(&mut disk, false).map_err(|e| format!("fsck failed: {e:?}"))?;
    let _ = writeln!(
        out,
        "fsck: {} errors, {} fixed",
        report.errors.len(),
        report.fixed
    );
    for err in &report.errors {
        let _ = writeln!(out, "fsck error: {err}");
    }
    Ok(Output::Text(out))
}

fn exec_line(
    fs: &mut Fs<MemDisk>,
    line_no: usize,
    line: &str,
    out: &mut String,
) -> Result<(), String> {
    let (cmd, rest) = split_first_word(line);
    match cmd {
        "mkdir" => {
            let path = rest.trim();
            require_single_arg(line_no, cmd, path)?;
            fs.mkdir(path)
                .map_err(|e| format!("line {line_no}: mkdir {path}: {e:?}"))?;
            let _ = writeln!(out, "mkdir {path}");
        }
        "write" => {
            let (path, text) = split_first_word(rest);
            if path.is_empty() {
                return Err(format!("line {line_no}: write requires PATH [TEXT]"));
            }
            match fs.create(path) {
                Ok(()) | Err(hades::Error::Exists) => {}
                Err(e) => return Err(format!("line {line_no}: create {path}: {e:?}")),
            }
            fs.truncate(path, 0)
                .map_err(|e| format!("line {line_no}: truncate {path}: {e:?}"))?;
            let written = fs
                .write(path, 0, text.as_bytes())
                .map_err(|e| format!("line {line_no}: write {path}: {e:?}"))?;
            let _ = writeln!(out, "write {path} ({written} bytes)");
        }
        "cat" => {
            let path = rest.trim();
            require_single_arg(line_no, cmd, path)?;
            let st = fs
                .stat(path)
                .map_err(|e| format!("line {line_no}: stat {path}: {e:?}"))?;
            if st.kind != Kind::File {
                return Err(format!("line {line_no}: cat {path}: not a file"));
            }
            let to_read = usize::try_from(st.size)
                .unwrap_or(MAX_READ_BYTES)
                .min(MAX_READ_BYTES);
            let mut buf = vec![0u8; to_read];
            let n = fs
                .read(path, 0, &mut buf)
                .map_err(|e| format!("line {line_no}: read {path}: {e:?}"))?;
            let text = String::from_utf8_lossy(&buf[..n]);
            let _ = writeln!(out, "cat {path}: {text}");
        }
        "ls" => {
            let path = if rest.trim().is_empty() {
                "/"
            } else {
                rest.trim()
            };
            let entries = fs
                .list(path)
                .map_err(|e| format!("line {line_no}: ls {path}: {e:?}"))?;
            let _ = writeln!(out, "ls {path}:");
            if entries.is_empty() {
                out.push_str("  (empty)\n");
            } else {
                for entry in entries {
                    let kind = match entry.kind {
                        Kind::Dir => "dir ",
                        Kind::File => "file",
                    };
                    let _ = writeln!(out, "  {kind} {:>4} {}", entry.inode, entry.name);
                }
            }
        }
        "rm" => {
            let path = rest.trim();
            require_single_arg(line_no, cmd, path)?;
            fs.remove(path)
                .map_err(|e| format!("line {line_no}: rm {path}: {e:?}"))?;
            let _ = writeln!(out, "rm {path}");
        }
        "rmdir" => {
            let path = rest.trim();
            require_single_arg(line_no, cmd, path)?;
            fs.rmdir(path)
                .map_err(|e| format!("line {line_no}: rmdir {path}: {e:?}"))?;
            let _ = writeln!(out, "rmdir {path}");
        }
        _ => {
            return Err(format!("line {line_no}: unknown command `{cmd}`"));
        }
    }
    Ok(())
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

fn require_single_arg(line_no: usize, cmd: &str, arg: &str) -> Result<(), String> {
    if arg.is_empty() || arg.contains(|c: char| c.is_ascii_whitespace()) {
        return Err(format!("line {line_no}: {cmd} expects one path argument"));
    }
    Ok(())
}
