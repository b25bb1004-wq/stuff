//! CLI tool for calliope: linear-time regex grep and scripted multi-cursor editing.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

use calliope::{Buffer, Cursor, Editor, Regex};

const VERSION_STR: &str = "calliope 0.1.0";

fn usage() -> &'static str {
    "\
Usage:
  calliope <subcommand> [args]
  calliope --help
  calliope --version

Subcommands:
  grep REGEX FILE...
      Search FILE(s) for REGEX and print 'file:line:col: text'.
      Exits 0 on match, 1 on no match or I/O error, 2 on bad regex or usage.
      Example: calliope grep \"fn [a-z]+\" src/main.rs

  edit SCRIPT FILE
      Apply edit SCRIPT lines to FILE and print the resulting buffer.
      Script commands: goto LINE COL, insert TEXT, delete N, undo, redo,
      cursor add LINE COL, save.
      Example: calliope edit script.txt notes.txt"
}

enum CliError {
    Usage(String),
    Op(String),
    SilentFail,
}

fn read_input(path: &str) -> Result<String, CliError> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::Op(format!("failed to read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read_to_string(path).map_err(|e| CliError::Op(format!("failed to read '{path}': {e}")))
    }
}

fn cmd_grep(args: &[String]) -> Result<(), CliError> {
    if args.len() < 2 {
        return Err(CliError::Usage("grep expects REGEX FILE...".to_string()));
    }
    let pattern = &args[0];
    let regex = Regex::new(pattern)
        .map_err(|e| CliError::Usage(format!("invalid regex at {}: {}", e.pos, e.msg)))?;

    let mut matched_any = false;
    let mut out = io::stdout().lock();
    for file in &args[1..] {
        let text = read_input(file)?;
        let mut lines: Vec<&str> = text.split('\n').collect();
        if text.ends_with('\n') {
            lines.pop();
        }
        for (idx, raw_line) in lines.into_iter().enumerate() {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            if let Some(groups) = regex.find_at(line, 0) {
                if let Some(Some((start, _end))) = groups.first().copied() {
                    let line_no = idx + 1;
                    let col = line[..start].chars().count() + 1;
                    writeln!(out, "{file}:{line_no}:{col}: {line}")
                        .map_err(|e| CliError::Op(format!("failed to write stdout: {e}")))?;
                    matched_any = true;
                }
            }
        }
    }
    if matched_any {
        Ok(())
    } else {
        Err(CliError::SilentFail)
    }
}

fn line_col_to_offset(buf: &Buffer, line_1: usize, col_1: usize) -> Result<usize, CliError> {
    if line_1 == 0 || col_1 == 0 {
        return Err(CliError::Op("line and column must be >= 1".to_string()));
    }
    let line_idx = line_1 - 1;
    let start = buf
        .line_start(line_idx)
        .ok_or_else(|| CliError::Op(format!("line {line_1} out of range")))?;
    let next_start = buf.line_start(line_idx + 1).unwrap_or(buf.len());
    let mut end = next_start;
    if next_start > start && buf.slice(next_start - 1, next_start) == "\n" {
        end = next_start - 1;
    }
    let line_str = buf.slice(start, end);
    let mut byte_in_line = line_str.len();
    for (ci, (bi, _ch)) in line_str.char_indices().enumerate() {
        if ci + 1 == col_1 {
            byte_in_line = bi;
            break;
        }
    }
    Ok(start + byte_in_line)
}

fn forward_n_chars(buf: &Buffer, from: usize, n: usize) -> usize {
    if n == 0 || from >= buf.len() {
        return from.min(buf.len());
    }
    let tail = buf.slice(from, buf.len());
    let mut count = 0usize;
    for (bi, ch) in tail.char_indices() {
        count += 1;
        if count == n {
            return from + bi + ch.len_utf8();
        }
    }
    buf.len()
}

fn unescape_insert(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn parse_two_usize(rest: &str, line_no: usize, cmd_name: &str) -> Result<(usize, usize), CliError> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(CliError::Op(format!(
            "script line {line_no}: {cmd_name} expects LINE COL"
        )));
    }
    let a = parts[0].parse::<usize>().map_err(|_| {
        CliError::Op(format!(
            "script line {line_no}: invalid line '{}'",
            parts[0]
        ))
    })?;
    let b = parts[1]
        .parse::<usize>()
        .map_err(|_| CliError::Op(format!("script line {line_no}: invalid col '{}'", parts[1])))?;
    Ok((a, b))
}

fn cmd_edit(args: &[String]) -> Result<(), CliError> {
    if args.len() != 2 {
        return Err(CliError::Usage("edit expects SCRIPT FILE".to_string()));
    }
    let script_path = &args[0];
    let file_path = &args[1];
    if script_path == "-" && file_path == "-" {
        return Err(CliError::Usage(
            "SCRIPT and FILE cannot both be '-'".to_string(),
        ));
    }
    let script = read_input(script_path)?;
    let initial = read_input(file_path)?;
    let mut editor = Editor::new(&initial);

    for (idx, raw_line) in script.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("goto ") {
            let (l, c) = parse_two_usize(rest, line_no, "goto")?;
            let off = line_col_to_offset(&editor.buffer, l, c)?;
            editor.set_cursors(vec![Cursor {
                anchor: off,
                head: off,
            }]);
        } else if let Some(rest) = trimmed.strip_prefix("cursor add ") {
            let (l, c) = parse_two_usize(rest, line_no, "cursor add")?;
            let off = line_col_to_offset(&editor.buffer, l, c)?;
            let mut cs = editor.cursors().to_vec();
            cs.push(Cursor {
                anchor: off,
                head: off,
            });
            editor.set_cursors(cs);
        } else if let Some(rest) = line.strip_prefix("insert ") {
            let text = unescape_insert(rest);
            editor.type_text(&text);
        } else if trimmed == "insert" {
            editor.type_text("");
        } else if let Some(rest) = trimmed.strip_prefix("delete ") {
            let n = rest.trim().parse::<usize>().map_err(|_| {
                CliError::Op(format!("script line {line_no}: invalid delete count"))
            })?;
            if n > 0 {
                let sels: Vec<Cursor> = editor
                    .cursors()
                    .iter()
                    .map(|c| {
                        let start = c.anchor.min(c.head);
                        let end = forward_n_chars(&editor.buffer, start, n);
                        Cursor {
                            anchor: start,
                            head: end,
                        }
                    })
                    .collect();
                editor.set_cursors(sels);
                editor.backspace();
            }
        } else if trimmed == "undo" {
            editor.undo();
        } else if trimmed == "redo" {
            editor.redo();
        } else if trimmed == "save" {
            if file_path == "-" {
                return Err(CliError::Op("cannot save when FILE is '-'".to_string()));
            }
            fs::write(file_path, editor.buffer.text())
                .map_err(|e| CliError::Op(format!("failed to save '{file_path}': {e}")))?;
        } else {
            return Err(CliError::Op(format!(
                "script line {line_no}: unknown command '{trimmed}'"
            )));
        }
    }

    let mut out = io::stdout().lock();
    out.write_all(editor.buffer.text().as_bytes())
        .map_err(|e| CliError::Op(format!("failed to write stdout: {e}")))?;
    Ok(())
}

fn run() -> Result<(), CliError> {
    let args: Vec<String> = env::args().skip(1).collect();
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
            match sub {
                "grep" => cmd_grep(rest),
                "edit" => cmd_edit(rest),
                other => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
            }
        }
    }
}

fn main() {
    if let Err(err) = run() {
        match err {
            CliError::Usage(msg) => {
                eprintln!("calliope: {msg}");
                process::exit(2);
            }
            CliError::Op(msg) => {
                eprintln!("calliope: {msg}");
                process::exit(1);
            }
            CliError::SilentFail => {
                process::exit(1);
            }
        }
    }
}
