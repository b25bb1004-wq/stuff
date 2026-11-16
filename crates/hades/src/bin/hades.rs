//! Command-line tool for inspecting and manipulating `hades` filesystem images.
//!
//! Text mode prints the same lines as before. `--describe` prints the manifest, and `--json` prints one
//! ACI envelope (docs/ai-computer/ACI.md). Both modes share the `do_*` functions below.
#![forbid(unsafe_code)]

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use aci::envelope::{self, Code, DEFAULT_LIMIT, MAX_LIMIT};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
use hades::{fsck, mkfs, mkfs_min_blocks, Error, Fs, Kind, MemDisk, MkfsOptions, BLOCK};

const VERSION: &str = "hades 0.1.0";
const DEFAULT_BLOCKS: u64 = 1024;
const MAX_CLI_BLOCKS: u64 = 262_144;
const CHUNK_BYTES: usize = 8 * BLOCK;

const USAGE: &str = "\
hades 0.1.0 - Monolith filesystem image tool

Usage:
  hades mkfs IMG [--blocks N]    Create an empty hades image (default 1024 blocks)
  hades ls IMG PATH              List directory entries at PATH
  hades cat IMG PATH             Write file contents at PATH to stdout
  hades put IMG SRC PATH         Copy host file SRC (or - for stdin) to PATH
  hades rm IMG PATH              Remove the file at PATH
  hades mkdir IMG PATH           Create a directory at PATH
  hades fsck IMG                 Check filesystem consistency
  hades --help                   Print this help message
  hades --version                Print version

Examples:
  hades mkfs disk.img --blocks 256
  hades mkdir disk.img /notes
  hades put disk.img hello.txt /notes/hello.txt
  hades ls disk.img /notes
  hades cat disk.img /notes/hello.txt
  hades rm disk.img /notes/hello.txt
  hades fsck disk.img
";

const fn req(name: &'static str, kind: ArgKind) -> Arg {
    Arg {
        name,
        kind,
        required: true,
        default: None,
        choices: &[],
    }
}

const fn opt(name: &'static str, kind: ArgKind, default: &'static str) -> Arg {
    Arg {
        name,
        kind,
        required: false,
        default: Some(default),
        choices: &[],
    }
}

const IMG_PATH: &[Arg] = &[req("IMG", ArgKind::Path), req("PATH", ArgKind::Text)];
const MKFS_ARGS: &[Arg] = &[
    req("IMG", ArgKind::Path),
    opt("--blocks", ArgKind::Int, "1024"),
];
const LS_ARGS: &[Arg] = &[
    req("IMG", ArgKind::Path),
    req("PATH", ArgKind::Text),
    opt("--limit", ArgKind::Int, "100"),
    opt("--offset", ArgKind::Int, "0"),
];
const CAT_ARGS: &[Arg] = &[
    req("IMG", ArgKind::Path),
    req("PATH", ArgKind::Text),
    Arg {
        name: "--out",
        kind: ArgKind::Path,
        required: false,
        default: None,
        choices: &[],
    },
];
const PUT_ARGS: &[Arg] = &[
    req("IMG", ArgKind::Path),
    req("SRC", ArgKind::Path),
    req("PATH", ArgKind::Text),
];
const FSCK_ARGS: &[Arg] = &[req("IMG", ArgKind::Path)];

const COMMANDS: &[Command] = &[
    Command {
        name: "mkfs",
        summary: "Create an empty hades image IMG with --blocks N blocks (default 1024)",
        effect: Effect::Write,
        args: MKFS_ARGS,
        returns: "path (the image file), blocks",
        example: "hades --json mkfs disk.img --blocks 256",
    },
    Command {
        name: "ls",
        summary: "List directory entries at PATH; --limit and --offset page them (--json only)",
        effect: Effect::Read,
        args: LS_ARGS,
        returns: "items: [{name, kind, size}], total, more, next_offset",
        example: "hades --json ls disk.img /",
    },
    Command {
        name: "cat",
        summary: "Write file contents at PATH to stdout; --json returns capped text, --out FILE writes all bytes",
        effect: Effect::Write,
        args: CAT_ARGS,
        returns: "path, text (capped, with truncated, bytes, lossy when they apply); with --out: path (host file), bytes",
        example: "hades --json cat disk.img /notes/hello.txt",
    },
    Command {
        name: "put",
        summary: "Copy host file SRC (or - for stdin) to PATH inside the image",
        effect: Effect::Write,
        args: PUT_ARGS,
        returns: "path, bytes",
        example: "hades --json put disk.img hello.txt /hello.txt",
    },
    Command {
        name: "rm",
        summary: "Remove the file at PATH",
        effect: Effect::Write,
        args: IMG_PATH,
        returns: "path",
        example: "hades --json rm disk.img /hello.txt",
    },
    Command {
        name: "mkdir",
        summary: "Create a directory at PATH",
        effect: Effect::Write,
        args: IMG_PATH,
        returns: "path",
        example: "hades --json mkdir disk.img /notes",
    },
    Command {
        name: "fsck",
        summary: "Check filesystem consistency; exit 1 when problems are found",
        effect: Effect::Read,
        args: FSCK_ARGS,
        returns: "clean, problems: [text], count",
        example: "hades --json fsck disk.img",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "hades",
    version: env!("CARGO_PKG_VERSION"),
    summary:
        "Monolith filesystem image tool; image paths start with /; mkfs needs at least 68 blocks",
    commands: COMMANDS,
};

/// A failed run: the code and message for the envelope, the hint, and the exit code the text path uses.
struct Fail {
    code: Code,
    message: String,
    hint: Option<String>,
    exit: u8,
}

impl Fail {
    fn usage(message: impl Into<String>) -> Fail {
        Fail {
            code: Code::Usage,
            message: message.into(),
            hint: None,
            exit: 2,
        }
    }

    fn op(code: Code, message: impl Into<String>) -> Fail {
        Fail {
            code,
            message: message.into(),
            hint: None,
            exit: 1,
        }
    }
}

enum Action {
    Help,
    Version,
    Mkfs {
        img: PathBuf,
        blocks: u64,
    },
    Ls {
        img: PathBuf,
        path: String,
    },
    Cat {
        img: PathBuf,
        path: String,
    },
    Put {
        img: PathBuf,
        src: OsString,
        path: String,
    },
    Rm {
        img: PathBuf,
        path: String,
    },
    Mkdir {
        img: PathBuf,
        path: String,
    },
    Fsck {
        img: PathBuf,
    },
}

/// Options that exist only under `--json`: paging for `ls`, the host file for `cat --out`.
#[derive(Default)]
struct JsonOpts {
    limit: Option<usize>,
    offset: Option<usize>,
    out: Option<PathBuf>,
}

fn main() -> ExitCode {
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut args);
    if envelope::take_describe_flag(&args) {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    if json {
        return run_json(&args);
    }
    match run_text(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(fail) => {
            eprintln!("hades: {}", fail.message);
            ExitCode::from(fail.exit)
        }
    }
}

fn run_text(args: &[OsString]) -> Result<(), Fail> {
    match parse_args(args)? {
        Action::Help => {
            print!("{USAGE}");
            Ok(())
        }
        Action::Version => {
            println!("{VERSION}");
            Ok(())
        }
        Action::Mkfs { img, blocks } => {
            do_mkfs(&img, blocks)?;
            println!("mkfs {} ({blocks} blocks)", img.display());
            Ok(())
        }
        Action::Ls { img, path } => {
            for entry in do_ls(&img, &path)? {
                println!("{}\t{}\t{}", kind_str(entry.kind), entry.inode, entry.name);
            }
            Ok(())
        }
        Action::Cat { img, path } => {
            let mut stdout = io::stdout().lock();
            do_cat(&img, &path, &mut |bytes| {
                stdout
                    .write_all(bytes)
                    .map_err(|e| Fail::op(Code::Io, format!("stdout: {e}")))
            })?;
            stdout
                .flush()
                .map_err(|e| Fail::op(Code::Io, format!("stdout: {e}")))?;
            Ok(())
        }
        Action::Put { img, src, path } => do_put(&img, &src, &path).map(|_| ()),
        Action::Rm { img, path } => do_rm(&img, &path),
        Action::Mkdir { img, path } => do_mkdir(&img, &path),
        Action::Fsck { img } => {
            let problems = do_fsck(&img)?;
            if problems.is_empty() {
                println!("fsck: ok");
                return Ok(());
            }
            for problem in &problems {
                eprintln!("hades: {problem}");
            }
            Err(Fail::op(
                Code::Invalid,
                format!("fsck found {} error(s)", problems.len()),
            ))
        }
    }
}

/// `hades --json ...`: one envelope on stdout; stderr keeps the `hades: message` line.
fn run_json(args: &[OsString]) -> ExitCode {
    let cmd = known_command(args.first());
    match json_command(args) {
        Ok((data, exit)) => {
            print!("{}", envelope::ok("hades", cmd, data));
            ExitCode::from(exit)
        }
        Err(fail) => {
            eprintln!("hades: {}", fail.message);
            let hint = match (fail.hint, fail.code) {
                (Some(hint), _) => Some(hint),
                (None, Code::Usage) => Some(String::from("run: hades --describe")),
                (None, _) => None,
            };
            print!(
                "{}",
                envelope::err("hades", cmd, fail.code, &fail.message, hint.as_deref())
            );
            ExitCode::from(fail.exit)
        }
    }
}

/// The subcommand name for the envelope, or the empty string when the name is not one of ours.
fn known_command(first: Option<&OsString>) -> &'static str {
    const NAMES: [&str; 9] = [
        "mkfs",
        "ls",
        "cat",
        "put",
        "rm",
        "mkdir",
        "fsck",
        "--help",
        "--version",
    ];
    first
        .and_then(|a| a.to_str())
        .and_then(|s| NAMES.iter().copied().find(|n| *n == s))
        .unwrap_or("")
}

/// Run one command under `--json`. Returns the data and the exit code for a successful run.
fn json_command(args: &[OsString]) -> Result<(Json, u8), Fail> {
    let mut args = args.to_vec();
    let name = known_command(args.first());
    let opts = take_json_opts(name, &mut args)?;
    match parse_args(&args)? {
        Action::Help => Ok((Json::obj([("usage", Json::Str(USAGE.into()))]), 0)),
        Action::Version => Ok((Json::obj([("version", Json::Str(VERSION.into()))]), 0)),
        Action::Mkfs { img, blocks } => {
            do_mkfs(&img, blocks)?;
            let data = Json::obj([("path", Json::path(&img)), ("blocks", int(blocks))]);
            Ok((data, 0))
        }
        Action::Ls { img, path } => {
            let entries = do_ls(&img, &path).map_err(|f| git_bash_hint(f, &path))?;
            let limit = opts.limit.unwrap_or(DEFAULT_LIMIT);
            let offset = opts.offset.unwrap_or(0);
            let total = entries.len();
            let items = entries
                .iter()
                .skip(offset)
                .take(limit)
                .map(entry_json)
                .collect();
            Ok((envelope::page(items, total, offset, limit), 0))
        }
        Action::Cat { img, path } => {
            let mut bytes = Vec::new();
            do_cat(&img, &path, &mut |chunk| {
                bytes.extend_from_slice(chunk);
                Ok(())
            })
            .map_err(|f| git_bash_hint(f, &path))?;
            match opts.out {
                Some(out) => {
                    fs::write(&out, &bytes).map_err(|e| {
                        Fail::op(Code::Io, format!("cannot write '{}': {e}", out.display()))
                    })?;
                    let data = Json::obj([("path", Json::path(&out)), ("bytes", int(bytes.len()))]);
                    Ok((data, 0))
                }
                None => {
                    let mut pairs = vec![("path".to_string(), Json::Str(path))];
                    pairs.extend(envelope::capped(&bytes));
                    Ok((Json::Obj(pairs), 0))
                }
            }
        }
        Action::Put { img, src, path } => {
            let count = do_put(&img, &src, &path).map_err(|f| git_bash_hint(f, &path))?;
            let data = Json::obj([("path", Json::Str(path)), ("bytes", int(count))]);
            Ok((data, 0))
        }
        Action::Rm { img, path } => {
            do_rm(&img, &path).map_err(|f| git_bash_hint(f, &path))?;
            Ok((Json::obj([("path", Json::Str(path))]), 0))
        }
        Action::Mkdir { img, path } => {
            do_mkdir(&img, &path).map_err(|f| git_bash_hint(f, &path))?;
            Ok((Json::obj([("path", Json::Str(path))]), 0))
        }
        Action::Fsck { img } => {
            let problems = do_fsck(&img)?;
            for problem in &problems {
                eprintln!("hades: {problem}");
            }
            let clean = problems.is_empty();
            let count = problems.len();
            let list = problems.into_iter().map(Json::Str).collect();
            let data = Json::obj([
                ("clean", Json::Bool(clean)),
                ("problems", Json::Arr(list)),
                ("count", int(count)),
            ]);
            // Problems found: ok stays true (the answer is "not clean"), exit 1 as the text path.
            Ok((data, if clean { 0 } else { 1 }))
        }
    }
}

/// Pull `--limit`, `--offset` (ls) and `--out` (cat) out of the arguments. They are JSON-only, so in
/// text mode the same words stay in place and fail as usage errors, as before.
fn take_json_opts(name: &str, args: &mut Vec<OsString>) -> Result<JsonOpts, Fail> {
    let mut opts = JsonOpts::default();
    let mut kept = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].to_str().unwrap_or("");
        let takes_value = i > 0
            && matches!(
                (name, flag),
                ("ls", "--limit" | "--offset") | ("cat", "--out")
            );
        if !takes_value {
            kept.push(args[i].clone());
            i += 1;
            continue;
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| Fail::usage(format!("{flag} requires a value")))?;
        match flag {
            "--out" => opts.out = Some(PathBuf::from(value)),
            _ => {
                let text = os_to_utf8(value, flag)?;
                let number = text
                    .parse::<usize>()
                    .map_err(|_| Fail::usage(format!("invalid value '{text}' for {flag}")))?;
                if flag == "--limit" {
                    if !(1..=MAX_LIMIT).contains(&number) {
                        return Err(Fail::usage(format!(
                            "--limit must be between 1 and {MAX_LIMIT}"
                        )));
                    }
                    opts.limit = Some(number);
                } else {
                    opts.offset = Some(number);
                }
            }
        }
        i += 2;
    }
    *args = kept;
    Ok(opts)
}

fn int<T: TryInto<i64>>(n: T) -> Json {
    Json::Int(n.try_into().unwrap_or(i64::MAX))
}

fn kind_str(kind: Kind) -> &'static str {
    match kind {
        Kind::Dir => "dir",
        Kind::File => "file",
    }
}

fn entry_json(entry: &Entry) -> Json {
    Json::obj([
        ("name", Json::Str(entry.name.clone())),
        ("kind", Json::Str(kind_str(entry.kind).into())),
        ("size", int(entry.size)),
    ])
}

fn os_to_utf8(arg: &OsStr, label: &str) -> Result<String, Fail> {
    arg.to_str()
        .map(String::from)
        .ok_or_else(|| Fail::usage(format!("{label} must be valid UTF-8")))
}

fn parse_args(args: &[OsString]) -> Result<Action, Fail> {
    let Some(first) = args.first() else {
        return Err(Fail::usage("missing subcommand (try --help)"));
    };
    let cmd = os_to_utf8(first, "subcommand")?;
    let rest = &args[1..];
    match cmd.as_str() {
        "--help" | "-h" => {
            if !rest.is_empty() {
                return Err(Fail::usage("unexpected arguments after --help"));
            }
            Ok(Action::Help)
        }
        "--version" | "-V" => {
            if !rest.is_empty() {
                return Err(Fail::usage("unexpected arguments after --version"));
            }
            Ok(Action::Version)
        }
        "mkfs" => parse_mkfs(rest),
        "ls" => {
            let (img, path) = parse_img_path(rest, "ls")?;
            Ok(Action::Ls { img, path })
        }
        "cat" => {
            let (img, path) = parse_img_path(rest, "cat")?;
            Ok(Action::Cat { img, path })
        }
        "put" => parse_put(rest),
        "rm" => {
            let (img, path) = parse_img_path(rest, "rm")?;
            Ok(Action::Rm { img, path })
        }
        "mkdir" => {
            let (img, path) = parse_img_path(rest, "mkdir")?;
            Ok(Action::Mkdir { img, path })
        }
        "fsck" => {
            if rest.len() != 1 {
                return Err(Fail::usage("usage: hades fsck IMG"));
            }
            Ok(Action::Fsck {
                img: PathBuf::from(&rest[0]),
            })
        }
        other => Err(Fail::usage(format!("unknown subcommand '{other}'"))),
    }
}

fn parse_mkfs(args: &[OsString]) -> Result<Action, Fail> {
    let mut img: Option<PathBuf> = None;
    let mut blocks: Option<u64> = None;
    let mut i = 0;
    while i < args.len() {
        if let Some(s) = args[i].to_str() {
            if s == "--blocks" {
                if blocks.is_some() {
                    return Err(Fail::usage("duplicate --blocks option"));
                }
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| Fail::usage("--blocks requires a value"))?;
                let val_str = os_to_utf8(val, "--blocks")?;
                let parsed = val_str
                    .parse::<u64>()
                    .map_err(|_| Fail::usage(format!("invalid block count '{val_str}'")))?;
                blocks = Some(parsed);
                i += 1;
                continue;
            }
            if let Some(val_str) = s.strip_prefix("--blocks=") {
                if blocks.is_some() {
                    return Err(Fail::usage("duplicate --blocks option"));
                }
                let parsed = val_str
                    .parse::<u64>()
                    .map_err(|_| Fail::usage(format!("invalid block count '{val_str}'")))?;
                blocks = Some(parsed);
                i += 1;
                continue;
            }
            if s.starts_with('-') && s != "-" {
                return Err(Fail::usage(format!("unknown option '{s}'")));
            }
        }
        if img.is_some() {
            return Err(Fail::usage("usage: hades mkfs IMG [--blocks N]"));
        }
        img = Some(PathBuf::from(&args[i]));
        i += 1;
    }
    let img = img.ok_or_else(|| Fail::usage("usage: hades mkfs IMG [--blocks N]"))?;
    Ok(Action::Mkfs {
        img,
        blocks: blocks.unwrap_or(DEFAULT_BLOCKS),
    })
}

fn parse_img_path(args: &[OsString], name: &str) -> Result<(PathBuf, String), Fail> {
    if args.len() != 2 {
        return Err(Fail::usage(format!("usage: hades {name} IMG PATH")));
    }
    let img = PathBuf::from(&args[0]);
    let path = os_to_utf8(&args[1], "PATH")?;
    Ok((img, path))
}

fn parse_put(args: &[OsString]) -> Result<Action, Fail> {
    if args.len() != 3 {
        return Err(Fail::usage("usage: hades put IMG SRC PATH"));
    }
    let img = PathBuf::from(&args[0]);
    let src = args[1].clone();
    let path = os_to_utf8(&args[2], "PATH")?;
    Ok(Action::Put { img, src, path })
}

fn fmt_fs_err(err: Error) -> &'static str {
    match err {
        Error::Io => "i/o error",
        Error::Corrupt => "corrupt filesystem image",
        Error::NotFound => "no such file or directory",
        Error::Exists => "file or directory already exists",
        Error::NotDir => "not a directory",
        Error::IsDir => "is a directory",
        Error::NotEmpty => "directory not empty",
        Error::NoSpace => "no space left on device",
        Error::NameTooLong => "name too long",
        Error::BadPath => "invalid path",
        Error::TooLarge => "file or transaction too large",
    }
}

/// The ACI code for a filesystem error.
fn fs_code(err: Error) -> Code {
    match err {
        Error::NotFound => Code::NotFound,
        Error::Exists => Code::Exists,
        Error::Io | Error::NoSpace => Code::Io,
        Error::Corrupt
        | Error::NotDir
        | Error::IsDir
        | Error::NotEmpty
        | Error::NameTooLong
        | Error::BadPath
        | Error::TooLarge => Code::Invalid,
    }
}

/// Git Bash rewrites an image path like `/docs` into `C:/Program Files/Git/docs`; hint that on the JSON error.
fn git_bash_hint(mut fail: Fail, path: &str) -> Fail {
    let windows = path.as_bytes().get(1) == Some(&b':') || path.contains("Program Files/Git");
    if windows && matches!(fail.code, Code::Invalid) && fail.message.ends_with("invalid path") {
        fail.hint = Some(String::from(
            "Git Bash rewrote the image path; set MSYS_NO_PATHCONV=1 or write it as //docs",
        ));
    }
    fail
}

fn fs_err(context: &str, err: Error) -> Fail {
    Fail::op(fs_code(err), format!("{context}: {}", fmt_fs_err(err)))
}

fn io_code(err: &io::Error) -> Code {
    if err.kind() == io::ErrorKind::NotFound {
        Code::NotFound
    } else {
        Code::Io
    }
}

fn read_image(path: &Path) -> Result<MemDisk, Fail> {
    let bytes = fs::read(path).map_err(|e| {
        Fail::op(
            io_code(&e),
            format!("cannot read '{}': {e}", path.display()),
        )
    })?;
    if bytes.is_empty() || bytes.len() % BLOCK != 0 {
        return Err(Fail::op(
            Code::Invalid,
            format!(
                "image '{}' size ({} bytes) is not a non-zero multiple of {BLOCK}",
                path.display(),
                bytes.len()
            ),
        ));
    }
    let blocks = (bytes.len() / BLOCK) as u64;
    let mut disk = MemDisk::new(blocks);
    if disk.bytes().len() != bytes.len() {
        return Err(Fail::op(Code::Io, "image size exceeds memory limit"));
    }
    disk.bytes_mut().copy_from_slice(&bytes);
    Ok(disk)
}

fn write_image_atomic(path: &Path, bytes: &[u8]) -> Result<(), Fail> {
    let file_name = path.file_name().ok_or_else(|| {
        Fail::op(
            Code::Invalid,
            format!("invalid image path '{}'", path.display()),
        )
    })?;
    let pid = std::process::id();
    let mut tmp_file = None;
    let mut tmp_path = PathBuf::new();

    for attempt in 0u32..32 {
        let mut tmp_name = OsString::from(".");
        tmp_name.push(file_name);
        tmp_name.push(format!(".tmp.{pid}.{attempt}"));
        let candidate = path.with_file_name(tmp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => {
                tmp_path = candidate;
                tmp_file = Some(f);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(Fail::op(
                    Code::Io,
                    format!("cannot create temp image '{}': {e}", candidate.display()),
                ));
            }
        }
    }

    let mut file = tmp_file.ok_or_else(|| {
        Fail::op(
            Code::Io,
            format!("could not allocate temp file for '{}'", path.display()),
        )
    })?;

    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&tmp_path);
        return Err(Fail::op(
            Code::Io,
            format!("cannot write '{}': {e}", tmp_path.display()),
        ));
    }
    drop(file);

    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(Fail::op(
            Code::Io,
            format!("cannot commit image '{}': {e}", path.display()),
        ));
    }
    Ok(())
}

fn do_mkfs(img: &Path, blocks: u64) -> Result<(), Fail> {
    let min = mkfs_min_blocks(MkfsOptions::default());
    if blocks < min {
        // Usage-style failure: exit 2 on the text path, code invalid under --json.
        return Err(Fail {
            code: Code::Invalid,
            message: format!("mkfs: need at least {min} blocks"),
            hint: Some(format!("use --blocks {min} or more")),
            exit: 2,
        });
    }
    if blocks > MAX_CLI_BLOCKS {
        return Err(Fail::op(
            Code::Invalid,
            format!("block count {blocks} exceeds maximum {MAX_CLI_BLOCKS}"),
        ));
    }
    let mut disk = MemDisk::new(blocks);
    if disk.bytes().len() != (blocks as usize).saturating_mul(BLOCK) {
        return Err(Fail::op(Code::Io, "image size exceeds memory limit"));
    }
    mkfs(&mut disk, MkfsOptions::default()).map_err(|e| fs_err("mkfs", e))?;
    write_image_atomic(img, disk.bytes())
}

/// One directory entry, with the size of the file it names.
struct Entry {
    name: String,
    kind: Kind,
    inode: u32,
    size: u64,
}

fn child_path(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn do_ls(img: &Path, path: &str) -> Result<Vec<Entry>, Fail> {
    let disk = read_image(img)?;
    let mut fs = Fs::mount(disk).map_err(|e| fs_err("mount", e))?;
    let listed = fs.list(path).map_err(|e| fs_err(path, e))?;
    let mut entries = Vec::with_capacity(listed.len());
    for entry in listed {
        let child = child_path(path, &entry.name);
        let size = fs.stat(&child).map_err(|e| fs_err(&child, e))?.size;
        entries.push(Entry {
            name: entry.name,
            kind: entry.kind,
            inode: entry.inode,
            size,
        });
    }
    Ok(entries)
}

/// Read the file at `path` in chunks and pass each chunk to `sink`.
fn do_cat(
    img: &Path,
    path: &str,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), Fail>,
) -> Result<(), Fail> {
    let disk = read_image(img)?;
    let mut fs = Fs::mount(disk).map_err(|e| fs_err("mount", e))?;
    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut offset = 0u64;
    loop {
        let n = fs
            .read(path, offset, &mut buf)
            .map_err(|e| fs_err(path, e))?;
        if n == 0 {
            break;
        }
        sink(&buf[..n])?;
        offset = offset
            .checked_add(n as u64)
            .ok_or_else(|| Fail::op(Code::Io, "offset overflow"))?;
    }
    Ok(())
}

fn read_source(src: &OsStr) -> Result<Vec<u8>, Fail> {
    if src == OsStr::new("-") {
        let mut data = Vec::new();
        io::stdin()
            .lock()
            .read_to_end(&mut data)
            .map_err(|e| Fail::op(Code::Io, format!("cannot read stdin: {e}")))?;
        Ok(data)
    } else {
        let src_path = Path::new(src);
        fs::read(src_path).map_err(|e| {
            Fail::op(
                io_code(&e),
                format!("cannot read '{}': {e}", src_path.display()),
            )
        })
    }
}

/// Copy `src` into the image at `path`. Returns the number of bytes written.
fn do_put(img: &Path, src: &OsStr, path: &str) -> Result<usize, Fail> {
    let data = read_source(src)?;
    let disk = read_image(img)?;
    let mut fs = Fs::mount(disk).map_err(|e| fs_err("mount", e))?;

    match fs.stat(path) {
        Ok(st) => {
            if st.kind == Kind::Dir {
                return Err(fs_err(path, Error::IsDir));
            }
            fs.truncate(path, 0).map_err(|e| fs_err(path, e))?;
        }
        Err(Error::NotFound) => {
            fs.create(path).map_err(|e| fs_err(path, e))?;
        }
        Err(e) => return Err(fs_err(path, e)),
    }

    let mut offset = 0u64;
    for chunk in data.chunks(CHUNK_BYTES) {
        let written = fs.write(path, offset, chunk).map_err(|e| fs_err(path, e))?;
        if written != chunk.len() {
            return Err(Fail::op(Code::Io, format!("{path}: short write")));
        }
        offset = offset
            .checked_add(written as u64)
            .ok_or_else(|| Fail::op(Code::Io, "offset overflow"))?;
    }

    let disk = fs.unmount().map_err(|e| fs_err("unmount", e))?;
    write_image_atomic(img, disk.bytes())?;
    Ok(data.len())
}

fn do_rm(img: &Path, path: &str) -> Result<(), Fail> {
    let disk = read_image(img)?;
    let mut fs = Fs::mount(disk).map_err(|e| fs_err("mount", e))?;
    fs.remove(path).map_err(|e| fs_err(path, e))?;
    let disk = fs.unmount().map_err(|e| fs_err("unmount", e))?;
    write_image_atomic(img, disk.bytes())
}

fn do_mkdir(img: &Path, path: &str) -> Result<(), Fail> {
    let disk = read_image(img)?;
    let mut fs = Fs::mount(disk).map_err(|e| fs_err("mount", e))?;
    fs.mkdir(path).map_err(|e| fs_err(path, e))?;
    let disk = fs.unmount().map_err(|e| fs_err("unmount", e))?;
    write_image_atomic(img, disk.bytes())
}

/// The problems fsck found; empty when the image is clean.
fn do_fsck(img: &Path) -> Result<Vec<String>, Fail> {
    let mut disk = read_image(img)?;
    let report = fsck(&mut disk, false).map_err(|e| fs_err("fsck", e))?;
    Ok(report.errors)
}
