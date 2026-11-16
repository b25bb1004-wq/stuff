//! calliope: the terminal editor (`calliope [FILE]`) and a CLI for linear-time regex grep and
//! scripted multi-cursor editing (`calliope grep`, `calliope edit`).
#![deny(unsafe_code)]

#[path = "calliope/tty.rs"]
#[allow(unsafe_code)]
mod tty;

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

use aci::envelope::{self, Code, DEFAULT_LIMIT, MAX_LIMIT};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};
use calliope::{ansi_changed, file, Buffer, Cursor, Editor, Regex, Screen, Session};

const VERSION_STR: &str = "calliope 0.1.0";

/// Matched text in a `grep` item is cut at this many bytes.
const GREP_TEXT_CAP: usize = 400;

const GREP_ARGS: &[Arg] = &[
    Arg {
        name: "REGEX",
        kind: Kind::Text,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "FILE...",
        kind: Kind::Path,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "--limit",
        kind: Kind::Int,
        required: false,
        default: Some("100"),
        choices: &[],
    },
    Arg {
        name: "--offset",
        kind: Kind::Int,
        required: false,
        default: Some("0"),
        choices: &[],
    },
];

const EDIT_ARGS: &[Arg] = &[
    Arg {
        name: "SCRIPT",
        kind: Kind::Path,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "FILE",
        kind: Kind::Path,
        required: true,
        default: None,
        choices: &[],
    },
];

const FILE_ARGS: &[Arg] = &[Arg {
    name: "FILE",
    kind: Kind::Path,
    required: true,
    default: None,
    choices: &[],
}];

const COMMANDS: &[Command] = &[
    Command {
        name: "grep",
        summary: "Search FILE(s) for REGEX, one item per matching line",
        effect: Effect::Read,
        args: GREP_ARGS,
        returns: "items: [{path, line, col, text}], total, more, next_offset",
        example: "calliope --json grep \"fn [a-z]+\" src/main.rs",
    },
    Command {
        name: "edit",
        summary: "Apply an edit SCRIPT to FILE, one script line per command (blank and # lines are skipped):\ngoto LINE COL (1-based)\ninsert TEXT (rest of the line; \\n \\t \\r \\\\ expand, other backslashes stay)\nnewline (line break at the cursor, in the file's line ending)\ndelete N (N characters forward; a line break counts as one)\nundo\nredo\ncursor add LINE COL (1-based)\nsave (writes FILE; without save nothing is written and saved is false)",
        effect: Effect::Write,
        args: EDIT_ARGS,
        returns: "path, changed, saved, lines_before, lines_after, text (capped)",
        example: "calliope --json edit script.txt notes.txt",
    },
    Command {
        name: "FILE",
        summary: "Open the terminal editor on FILE; refused under --json",
        effect: Effect::Write,
        args: FILE_ARGS,
        returns: "nothing: always code unsupported",
        example: "calliope --json notes.txt",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "calliope",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Terminal editor, regex grep and scripted edits",
    commands: COMMANDS,
};

fn usage() -> &'static str {
    "\
Usage:
  calliope FILE                (open the editor; needs a terminal)
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
      One script line per command (blank and '#' lines are skipped):
        goto LINE COL       move the cursor (1-based)
        insert TEXT         type the rest of the line; \\n \\t \\r \\\\ expand, other backslashes stay
        newline             line break at the cursor, in the file's line ending
        delete N            delete N characters forward (a line break counts as one)
        undo
        redo
        cursor add LINE COL add a cursor (1-based)
        save                write FILE; without save nothing is written and saved is false
      Example: calliope edit script.txt notes.txt"
}

enum CliError {
    Usage(String),
    Op(String),
    SilentFail,
    /// stdout was closed by the reader (`calliope grep ... | head -1`): stop quietly with exit 0.
    Closed,
    /// An operation failed (exit 1) with the ACI code it maps to under `--json`.
    Coded(Code, String),
}

/// A failed stdout write: a closed pipe ends the run quietly, anything else is an error.
fn stdout_err(e: io::Error) -> CliError {
    // Windows reports a closed pipe as os error 232 (ERROR_NO_DATA).
    if e.kind() == io::ErrorKind::BrokenPipe || e.raw_os_error() == Some(232) {
        CliError::Closed
    } else {
        CliError::Op(format!("failed to write stdout: {e}"))
    }
}

fn read_input(path: &str) -> Result<String, CliError> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::Op(format!("failed to read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read_to_string(path).map_err(|e| {
            let code = if e.kind() == io::ErrorKind::NotFound {
                Code::NotFound
            } else {
                Code::Io
            };
            CliError::Coded(code, format!("failed to read '{path}': {e}"))
        })
    }
}

fn compile_regex(pattern: &str) -> Result<Regex, CliError> {
    Regex::new(pattern)
        .map_err(|e| CliError::Usage(format!("invalid regex at {}: {}", e.pos, e.msg)))
}

/// Matching lines of `text` as (1-based line, 1-based column, line text without its line ending).
fn hits<'a>(regex: &Regex, text: &'a str) -> Vec<(usize, usize, &'a str)> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    let mut found = Vec::new();
    for (idx, raw_line) in lines.into_iter().enumerate() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if let Some(groups) = regex.find_at(line, 0) {
            if let Some(Some((start, _end))) = groups.first().copied() {
                let col = line[..start].chars().count() + 1;
                found.push((idx + 1, col, line));
            }
        }
    }
    found
}

fn cmd_grep(args: &[String]) -> Result<(), CliError> {
    if args.len() < 2 {
        return Err(CliError::Usage("grep expects REGEX FILE...".to_string()));
    }
    let regex = compile_regex(&args[0])?;

    let mut matched_any = false;
    let mut out = io::stdout().lock();
    for file in &args[1..] {
        let text = read_input(file)?;
        for (line_no, col, line) in hits(&regex, &text) {
            writeln!(out, "{file}:{line_no}:{col}: {line}").map_err(stdout_err)?;
            matched_any = true;
        }
    }
    if matched_any {
        Ok(())
    } else {
        Err(CliError::SilentFail)
    }
}

fn line_col_to_offset(
    buf: &Buffer,
    line_no: usize,
    line_1: usize,
    col_1: usize,
) -> Result<usize, CliError> {
    if line_1 == 0 || col_1 == 0 {
        return Err(CliError::Coded(
            Code::Invalid,
            format!("script line {line_no}: line and column must be >= 1"),
        ));
    }
    let line_idx = line_1 - 1;
    let start = buf.line_start(line_idx).ok_or_else(|| {
        CliError::Coded(
            Code::Invalid,
            format!("script line {line_no}: line {line_1} out of range"),
        )
    })?;
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
        return Err(CliError::Coded(
            Code::Invalid,
            format!("script line {line_no}: {cmd_name} expects LINE COL"),
        ));
    }
    let a = parts[0].parse::<usize>().map_err(|_| {
        CliError::Coded(
            Code::Invalid,
            format!("script line {line_no}: invalid line '{}'", parts[0]),
        )
    })?;
    let b = parts[1].parse::<usize>().map_err(|_| {
        CliError::Coded(
            Code::Invalid,
            format!("script line {line_no}: invalid col '{}'", parts[1]),
        )
    })?;
    Ok((a, b))
}

/// What a script run produced: the file text before, the buffer after, and the text of the last save.
struct EditOutcome {
    before: String,
    buffer: String,
    saved: Option<String>,
}

fn cmd_edit(args: &[String]) -> Result<(), CliError> {
    let out = run_edit(args)?;
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(out.buffer.as_bytes())
        .map_err(stdout_err)?;
    Ok(())
}

fn run_edit(args: &[String]) -> Result<EditOutcome, CliError> {
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
    // Inserted newlines take the file's own style: CRLF for a CRLF file, LF otherwise.
    let newline = file::newline_for(&initial);
    let mut editor = Editor::new(&initial);
    let mut saved: Option<String> = None;

    for (idx, raw_line) in script.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("goto ") {
            let (l, c) = parse_two_usize(rest, line_no, "goto")?;
            let off = line_col_to_offset(&editor.buffer, line_no, l, c)?;
            editor.set_cursors(vec![Cursor {
                anchor: off,
                head: off,
            }]);
        } else if let Some(rest) = trimmed.strip_prefix("cursor add ") {
            let (l, c) = parse_two_usize(rest, line_no, "cursor add")?;
            let off = line_col_to_offset(&editor.buffer, line_no, l, c)?;
            let mut cs = editor.cursors().to_vec();
            cs.push(Cursor {
                anchor: off,
                head: off,
            });
            editor.set_cursors(cs);
        } else if let Some(rest) = line.strip_prefix("insert ") {
            let text = file::style_inserted(&unescape_insert(rest), newline);
            editor.type_text(&text);
        } else if trimmed == "insert" {
            editor.type_text("");
        } else if trimmed == "newline" {
            editor.type_text(&file::style_inserted("\n", newline));
        } else if let Some(rest) = trimmed.strip_prefix("delete ") {
            let n = rest.trim().parse::<usize>().map_err(|_| {
                CliError::Coded(
                    Code::Invalid,
                    format!("script line {line_no}: invalid delete count"),
                )
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
                return Err(CliError::Coded(
                    Code::Invalid,
                    format!("script line {line_no}: cannot save when FILE is '-'"),
                ));
            }
            let text = editor.buffer.text();
            fs::write(file_path, &text).map_err(|e| {
                CliError::Coded(
                    Code::Io,
                    format!("script line {line_no}: failed to save '{file_path}': {e}"),
                )
            })?;
            saved = Some(text);
        } else {
            return Err(CliError::Coded(
                Code::Invalid,
                format!("script line {line_no}: unknown command '{trimmed}'"),
            ));
        }
    }

    Ok(EditOutcome {
        before: initial,
        buffer: editor.buffer.text(),
        saved,
    })
}

fn run() -> Result<(), CliError> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(first) = args.first() else {
        // No arguments: the editor on a new buffer when run from a terminal.
        return if tty::is_terminal() {
            run_editor(&[])
        } else {
            Err(CliError::Usage(
                "missing subcommand (try --help)".to_string(),
            ))
        };
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
                // A FILE name opens the editor, but only from a terminal: a script that asks
                // for an unknown subcommand keeps getting the usage error.
                other if !other.starts_with('-') && tty::is_terminal() => run_editor(&args),
                other if !other.starts_with('-') && looks_like_file(other) => Err(CliError::Usage(
                    "the editor needs a terminal (stdin is not a terminal)".to_string(),
                )),
                other => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
            }
        }
    }
}

/// True for an existing path, or a word with a path separator or a dot, such as `notes.txt`.
fn looks_like_file(word: &str) -> bool {
    std::path::Path::new(word).exists() || word.contains(['/', '\\', '.'])
}

/// Open the terminal editor on the first of `files` (a new buffer when it does not exist yet).
fn run_editor(files: &[String]) -> Result<(), CliError> {
    let path = files.first().map(PathBuf::from);
    if files.len() > 1 {
        eprintln!(
            "calliope: this version edits one file; ignoring {}",
            files[1..].join(" ")
        );
    }
    let bytes = match &path {
        Some(p) if p.exists() => fs::read(p)
            .map_err(|e| CliError::Op(format!("failed to read '{}': {e}", p.display())))?,
        _ => Vec::new(),
    };
    let (width, height) = tty::size();
    let mut session = Session::from_bytes(path, &bytes, width, height)
        .map_err(|e| CliError::Op(e.to_string()))?;
    let _terminal =
        tty::enter().map_err(|e| CliError::Op(format!("cannot start the terminal editor: {e}")))?;
    let mut previous: Option<Screen> = None;
    loop {
        // Checked before every draw, so a resize that woke the last read repaints at once.
        let size = tty::size();
        if size != session.size() {
            session.resize(size.0, size.1);
            previous = None;
        }
        let frame = session.render();
        tty::write(&ansi_changed(previous.as_ref(), &frame))
            .map_err(|e| CliError::Op(format!("failed to write to the terminal: {e}")))?;
        previous = Some(frame);
        if session.quit_requested() {
            break;
        }
        match tty::read_events() {
            None => break,
            Some(events) => {
                for ev in events {
                    session.handle(ev);
                }
            }
        }
    }
    Ok(())
}

/// A failed `--json` run: the envelope code, its message and hint, and the process exit code.
struct Failure {
    code: Code,
    message: String,
    hint: Option<String>,
    exit: i32,
}

const HINT_DESCRIBE: &str = "run: calliope --describe";

fn failure(e: CliError) -> Failure {
    match e {
        CliError::Usage(message) => Failure {
            code: Code::Usage,
            message,
            hint: Some(HINT_DESCRIBE.into()),
            exit: 2,
        },
        CliError::Coded(code, message) => Failure {
            code,
            message,
            hint: None,
            exit: 1,
        },
        CliError::Op(message) => Failure {
            code: Code::Io,
            message,
            hint: None,
            exit: 1,
        },
        CliError::SilentFail | CliError::Closed => Failure {
            code: Code::Internal,
            message: "unexpected end of run".into(),
            hint: None,
            exit: 1,
        },
    }
}

fn editor_unsupported() -> Failure {
    Failure {
        code: Code::Unsupported,
        message: "the terminal editor is not available under --json".into(),
        hint: Some(
            "run: calliope --json edit SCRIPT FILE or calliope --json grep REGEX FILE".into(),
        ),
        exit: 1,
    }
}

/// Split `--limit N` and `--offset N` out of a grep's arguments.
fn paging_flags(args: &[String]) -> Result<(usize, usize, Vec<String>), CliError> {
    let (mut limit, mut offset, mut rest) = (DEFAULT_LIMIT, 0usize, Vec::new());
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--limit" | "--offset" => {
                let value = it
                    .next()
                    .and_then(|v| v.parse::<usize>().ok())
                    .ok_or_else(|| CliError::Usage(format!("{arg} expects a number")))?;
                if arg == "--limit" {
                    if value == 0 || value > MAX_LIMIT {
                        return Err(CliError::Usage(format!("--limit must be 1 to {MAX_LIMIT}")));
                    }
                    limit = value;
                } else {
                    offset = value;
                }
            }
            _ => rest.push(arg.clone()),
        }
    }
    Ok((limit, offset, rest))
}

/// `calliope --json grep`: one page of matches over every FILE. No match is ok with total 0 and exit 1.
fn json_grep(args: &[String]) -> Result<(Json, i32), Failure> {
    let (limit, offset, rest) = paging_flags(args).map_err(failure)?;
    if rest.len() < 2 {
        return Err(failure(CliError::Usage(
            "grep expects REGEX FILE...".into(),
        )));
    }
    let regex = Regex::new(&rest[0]).map_err(|e| Failure {
        code: Code::Invalid,
        message: format!("invalid regex at {}: {}", e.pos, e.msg),
        hint: Some(HINT_DESCRIBE.into()),
        exit: 2,
    })?;
    let mut all: Vec<(&str, usize, usize, &str)> = Vec::new();
    let mut texts = Vec::new();
    for file in &rest[1..] {
        texts.push((file.as_str(), read_input(file).map_err(failure)?));
    }
    for (file, text) in &texts {
        for (line, col, line_text) in hits(&regex, text) {
            all.push((file, line, col, line_text));
        }
    }
    let total = all.len();
    let items: Vec<Json> = all
        .iter()
        .skip(offset)
        .take(limit)
        .map(|&(file, line, col, line_text)| {
            let (text, cut) = cut_text(line_text, GREP_TEXT_CAP);
            let mut pairs = vec![
                ("path".to_string(), Json::path(Path::new(file))),
                ("line".to_string(), Json::Int(line as i64)),
                ("col".to_string(), Json::Int(col as i64)),
                ("text".to_string(), Json::Str(text.into())),
            ];
            if cut {
                pairs.push(("truncated".to_string(), Json::Bool(true)));
            }
            Json::Obj(pairs)
        })
        .collect();
    let exit = if total == 0 { 1 } else { 0 };
    Ok((envelope::page(items, total, offset, limit), exit))
}

/// The longest prefix of `s` within `cap` bytes that ends on a character boundary, and whether it was cut.
fn cut_text(s: &str, cap: usize) -> (&str, bool) {
    if s.len() <= cap {
        return (s, false);
    }
    let mut end = cap;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

/// Lines in `text`, counted the way `lines()` does.
fn line_count(text: &str) -> i64 {
    text.lines().count() as i64
}

/// `calliope --json edit SCRIPT FILE`: the same run as the text path, reported as fields.
fn json_edit(args: &[String]) -> Result<(Json, i32), Failure> {
    let out = run_edit(args).map_err(failure)?;
    let changed = out
        .saved
        .as_deref()
        .is_some_and(|saved| saved != out.before);
    let mut pairs = vec![
        ("path".to_string(), Json::path(Path::new(&args[1]))),
        ("changed".to_string(), Json::Bool(changed)),
        ("saved".to_string(), Json::Bool(out.saved.is_some())),
        (
            "lines_before".to_string(),
            Json::Int(line_count(&out.before)),
        ),
        (
            "lines_after".to_string(),
            Json::Int(line_count(&out.buffer)),
        ),
    ];
    pairs.extend(envelope::capped(out.buffer.as_bytes()));
    Ok((Json::Obj(pairs), 0))
}

/// Run `calliope --json ARGS...` (`args` are the arguments after `--json`): one envelope on stdout.
fn run_json(args: &[String]) -> i32 {
    let (cmd, result) = match args.first().map(String::as_str) {
        None => ("FILE", Err(editor_unsupported())),
        Some("grep") => ("grep", json_grep(&args[1..])),
        Some("edit") => ("edit", json_edit(&args[1..])),
        Some(word) if !word.starts_with('-') && looks_like_file(word) => {
            ("FILE", Err(editor_unsupported()))
        }
        Some(word) => (
            "",
            Err(failure(CliError::Usage(format!(
                "unknown subcommand '{word}'"
            )))),
        ),
    };
    let exit = match result {
        Ok((data, exit)) => {
            print!("{}", envelope::ok("calliope", cmd, data));
            exit
        }
        Err(f) => {
            eprintln!("calliope: {}", f.message);
            print!(
                "{}",
                envelope::err("calliope", cmd, f.code, &f.message, f.hint.as_deref())
            );
            f.exit
        }
    };
    let _ = io::stdout().flush();
    exit
}

fn main() {
    let raw: Vec<OsString> = env::args_os().skip(1).collect();
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return;
    }
    if raw.first().and_then(|a| a.to_str()) == Some("--json") {
        let rest: Vec<String> = raw[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        process::exit(run_json(&rest));
    }
    if let Err(err) = run() {
        match err {
            CliError::Usage(msg) => {
                eprintln!("calliope: {msg}");
                process::exit(2);
            }
            CliError::Op(msg) | CliError::Coded(_, msg) => {
                eprintln!("calliope: {msg}");
                process::exit(1);
            }
            CliError::SilentFail => {
                process::exit(1);
            }
            CliError::Closed => process::exit(0),
        }
    }
}
