//! Command-line tool for inspecting and manipulating `hades` filesystem images.
#![forbid(unsafe_code)]

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

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

enum CliError {
    Usage(String),
    Op(String),
}

impl CliError {
    fn exit_code(&self) -> ExitCode {
        match self {
            CliError::Usage(_) => ExitCode::from(2),
            CliError::Op(_) => ExitCode::from(1),
        }
    }

    fn message(&self) -> &str {
        match self {
            CliError::Usage(msg) | CliError::Op(msg) => msg,
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

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("hades: {}", err.message());
            err.exit_code()
        }
    }
}

fn run() -> Result<(), CliError> {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match parse_args(&args)? {
        Action::Help => {
            print!("{USAGE}");
            Ok(())
        }
        Action::Version => {
            println!("{VERSION}");
            Ok(())
        }
        Action::Mkfs { img, blocks } => cmd_mkfs(&img, blocks),
        Action::Ls { img, path } => cmd_ls(&img, &path),
        Action::Cat { img, path } => cmd_cat(&img, &path),
        Action::Put { img, src, path } => cmd_put(&img, &src, &path),
        Action::Rm { img, path } => cmd_rm(&img, &path),
        Action::Mkdir { img, path } => cmd_mkdir(&img, &path),
        Action::Fsck { img } => cmd_fsck(&img),
    }
}

fn os_to_utf8(arg: &OsStr, label: &str) -> Result<String, CliError> {
    arg.to_str()
        .map(String::from)
        .ok_or_else(|| CliError::Usage(format!("{label} must be valid UTF-8")))
}

fn parse_args(args: &[OsString]) -> Result<Action, CliError> {
    let Some(first) = args.first() else {
        return Err(CliError::Usage(String::from(
            "missing subcommand (try --help)",
        )));
    };
    let cmd = os_to_utf8(first, "subcommand")?;
    let rest = &args[1..];
    match cmd.as_str() {
        "--help" | "-h" => {
            if !rest.is_empty() {
                return Err(CliError::Usage(String::from(
                    "unexpected arguments after --help",
                )));
            }
            Ok(Action::Help)
        }
        "--version" | "-V" => {
            if !rest.is_empty() {
                return Err(CliError::Usage(String::from(
                    "unexpected arguments after --version",
                )));
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
                return Err(CliError::Usage(String::from("usage: hades fsck IMG")));
            }
            Ok(Action::Fsck {
                img: PathBuf::from(&rest[0]),
            })
        }
        other => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
    }
}

fn parse_mkfs(args: &[OsString]) -> Result<Action, CliError> {
    let mut img: Option<PathBuf> = None;
    let mut blocks: Option<u64> = None;
    let mut i = 0;
    while i < args.len() {
        if let Some(s) = args[i].to_str() {
            if s == "--blocks" {
                if blocks.is_some() {
                    return Err(CliError::Usage(String::from("duplicate --blocks option")));
                }
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| CliError::Usage(String::from("--blocks requires a value")))?;
                let val_str = os_to_utf8(val, "--blocks")?;
                let parsed = val_str
                    .parse::<u64>()
                    .map_err(|_| CliError::Usage(format!("invalid block count '{val_str}'")))?;
                blocks = Some(parsed);
                i += 1;
                continue;
            }
            if let Some(val_str) = s.strip_prefix("--blocks=") {
                if blocks.is_some() {
                    return Err(CliError::Usage(String::from("duplicate --blocks option")));
                }
                let parsed = val_str
                    .parse::<u64>()
                    .map_err(|_| CliError::Usage(format!("invalid block count '{val_str}'")))?;
                blocks = Some(parsed);
                i += 1;
                continue;
            }
            if s.starts_with('-') && s != "-" {
                return Err(CliError::Usage(format!("unknown option '{s}'")));
            }
        }
        if img.is_some() {
            return Err(CliError::Usage(String::from(
                "usage: hades mkfs IMG [--blocks N]",
            )));
        }
        img = Some(PathBuf::from(&args[i]));
        i += 1;
    }
    let img =
        img.ok_or_else(|| CliError::Usage(String::from("usage: hades mkfs IMG [--blocks N]")))?;
    Ok(Action::Mkfs {
        img,
        blocks: blocks.unwrap_or(DEFAULT_BLOCKS),
    })
}

fn parse_img_path(args: &[OsString], name: &str) -> Result<(PathBuf, String), CliError> {
    if args.len() != 2 {
        return Err(CliError::Usage(format!("usage: hades {name} IMG PATH")));
    }
    let img = PathBuf::from(&args[0]);
    let path = os_to_utf8(&args[1], "PATH")?;
    Ok((img, path))
}

fn parse_put(args: &[OsString]) -> Result<Action, CliError> {
    if args.len() != 3 {
        return Err(CliError::Usage(String::from(
            "usage: hades put IMG SRC PATH",
        )));
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

fn fs_err(context: &str, err: Error) -> CliError {
    CliError::Op(format!("{context}: {}", fmt_fs_err(err)))
}

fn read_image(path: &Path) -> Result<MemDisk, CliError> {
    let bytes = fs::read(path)
        .map_err(|e| CliError::Op(format!("cannot read '{}': {e}", path.display())))?;
    if bytes.is_empty() || bytes.len() % BLOCK != 0 {
        return Err(CliError::Op(format!(
            "image '{}' size ({} bytes) is not a non-zero multiple of {BLOCK}",
            path.display(),
            bytes.len()
        )));
    }
    let blocks = (bytes.len() / BLOCK) as u64;
    let mut disk = MemDisk::new(blocks);
    if disk.bytes().len() != bytes.len() {
        return Err(CliError::Op(String::from(
            "image size exceeds memory limit",
        )));
    }
    disk.bytes_mut().copy_from_slice(&bytes);
    Ok(disk)
}

fn write_image_atomic(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let file_name = path
        .file_name()
        .ok_or_else(|| CliError::Op(format!("invalid image path '{}'", path.display())))?;
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
                return Err(CliError::Op(format!(
                    "cannot create temp image '{}': {e}",
                    candidate.display()
                )));
            }
        }
    }

    let mut file = tmp_file.ok_or_else(|| {
        CliError::Op(format!(
            "could not allocate temp file for '{}'",
            path.display()
        ))
    })?;

    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&tmp_path);
        return Err(CliError::Op(format!(
            "cannot write '{}': {e}",
            tmp_path.display()
        )));
    }
    drop(file);

    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(CliError::Op(format!(
            "cannot commit image '{}': {e}",
            path.display()
        )));
    }
    Ok(())
}

fn cmd_mkfs(img: &Path, blocks: u64) -> Result<(), CliError> {
    let min = mkfs_min_blocks(MkfsOptions::default());
    if blocks < min {
        return Err(CliError::Usage(format!("mkfs: need at least {min} blocks")));
    }
    if blocks > MAX_CLI_BLOCKS {
        return Err(CliError::Op(format!(
            "block count {blocks} exceeds maximum {MAX_CLI_BLOCKS}"
        )));
    }
    let mut disk = MemDisk::new(blocks);
    if disk.bytes().len() != (blocks as usize).saturating_mul(BLOCK) {
        return Err(CliError::Op(String::from(
            "image size exceeds memory limit",
        )));
    }
    mkfs(&mut disk, MkfsOptions::default()).map_err(|e| fs_err("mkfs", e))?;
    write_image_atomic(img, disk.bytes())?;
    println!("mkfs {} ({blocks} blocks)", img.display());
    Ok(())
}

fn cmd_ls(img: &Path, path: &str) -> Result<(), CliError> {
    let disk = read_image(img)?;
    let mut fs = Fs::mount(disk).map_err(|e| fs_err("mount", e))?;
    let entries = fs.list(path).map_err(|e| fs_err(path, e))?;
    for entry in entries {
        let kind = match entry.kind {
            Kind::Dir => "dir",
            Kind::File => "file",
        };
        println!("{kind}\t{}\t{}", entry.inode, entry.name);
    }
    Ok(())
}

fn cmd_cat(img: &Path, path: &str) -> Result<(), CliError> {
    let disk = read_image(img)?;
    let mut fs = Fs::mount(disk).map_err(|e| fs_err("mount", e))?;
    let mut stdout = io::stdout().lock();
    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut offset = 0u64;
    loop {
        let n = fs
            .read(path, offset, &mut buf)
            .map_err(|e| fs_err(path, e))?;
        if n == 0 {
            break;
        }
        stdout
            .write_all(&buf[..n])
            .map_err(|e| CliError::Op(format!("stdout: {e}")))?;
        offset = offset
            .checked_add(n as u64)
            .ok_or_else(|| CliError::Op(String::from("offset overflow")))?;
    }
    stdout
        .flush()
        .map_err(|e| CliError::Op(format!("stdout: {e}")))?;
    Ok(())
}

fn read_source(src: &OsStr) -> Result<Vec<u8>, CliError> {
    if src == OsStr::new("-") {
        let mut data = Vec::new();
        io::stdin()
            .lock()
            .read_to_end(&mut data)
            .map_err(|e| CliError::Op(format!("cannot read stdin: {e}")))?;
        Ok(data)
    } else {
        let src_path = Path::new(src);
        fs::read(src_path)
            .map_err(|e| CliError::Op(format!("cannot read '{}': {e}", src_path.display())))
    }
}

fn cmd_put(img: &Path, src: &OsStr, path: &str) -> Result<(), CliError> {
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
            return Err(CliError::Op(format!("{path}: short write")));
        }
        offset = offset
            .checked_add(written as u64)
            .ok_or_else(|| CliError::Op(String::from("offset overflow")))?;
    }

    let disk = fs.unmount().map_err(|e| fs_err("unmount", e))?;
    write_image_atomic(img, disk.bytes())
}

fn cmd_rm(img: &Path, path: &str) -> Result<(), CliError> {
    let disk = read_image(img)?;
    let mut fs = Fs::mount(disk).map_err(|e| fs_err("mount", e))?;
    fs.remove(path).map_err(|e| fs_err(path, e))?;
    let disk = fs.unmount().map_err(|e| fs_err("unmount", e))?;
    write_image_atomic(img, disk.bytes())
}

fn cmd_mkdir(img: &Path, path: &str) -> Result<(), CliError> {
    let disk = read_image(img)?;
    let mut fs = Fs::mount(disk).map_err(|e| fs_err("mount", e))?;
    fs.mkdir(path).map_err(|e| fs_err(path, e))?;
    let disk = fs.unmount().map_err(|e| fs_err("unmount", e))?;
    write_image_atomic(img, disk.bytes())
}

fn cmd_fsck(img: &Path) -> Result<(), CliError> {
    let mut disk = read_image(img)?;
    let report = fsck(&mut disk, false).map_err(|e| fs_err("fsck", e))?;
    if report.errors.is_empty() {
        println!("fsck: ok");
        return Ok(());
    }
    for err in &report.errors {
        eprintln!("hades: {err}");
    }
    Err(CliError::Op(format!(
        "fsck found {} error(s)",
        report.errors.len()
    )))
}
