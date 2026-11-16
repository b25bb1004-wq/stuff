//! CLI tool for argo: dependency solving, lock files, local installation, and registry serving.
//!
//! Text mode prints the same lines as before. `--describe` prints the manifest, and `--json` prints one
//! ACI envelope (docs/ai-computer/ACI.md). Each command builds its data from the same values it prints.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, Instant};

use aci::envelope::{self, Code, DEFAULT_LIMIT, MAX_LIMIT};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest as ToolManifest};
use argo::client::{self, InstallError};
use argo::lock;
use argo::manifest::Manifest;
use argo::pkg;
use argo::registry::Registry;
use argo::solve::{self, Index, Solution};
use argo::version::{Req, Version};
use mercury::{parse_response, write_request, Limits, Request};

const VERSION_STR: &str = "argo 0.1.0";
const DEFAULT_LISTEN: &str = "127.0.0.1:7500";

fn usage() -> &'static str {
    "\
argo - dependency solving, lock files, local installation, and registry serving
Usage:
  argo <subcommand> [options] [args]
  argo --help
  argo --version

Subcommands:
  solve INDEX MANIFEST
      Resolve dependencies and print 'name version' lines.
      Example: argo solve index.txt Cargo.argo

  lock INDEX MANIFEST -o LOCK
      Resolve dependencies and write a lock file.
      Example: argo lock index.txt Cargo.argo -o argo.lock

  install LOCK STORE_DIR --from DIR [--key HEX_OR_FILE]
      Install packages locked in LOCK into STORE_DIR from local package DIR.
      Example: argo install argo.lock ./store --from ./pkgs

  serve INDEX_DIR [--listen ADDR] [--key HEX_OR_FILE]
      Serve signed packages from INDEX_DIR over HTTP (default 127.0.0.1:7500).
      Example: argo serve ./pkgs --listen 127.0.0.1:7500"
}

/// A failed command: a usage error (exit 2) or an operation failure (exit 1) with its ACI code.
enum CliError {
    Usage(String),
    Op {
        code: Code,
        message: String,
        hint: Option<String>,
    },
}

fn op(code: Code, message: String) -> CliError {
    CliError::Op {
        code,
        message,
        hint: None,
    }
}

fn usage_err(message: &str) -> CliError {
    CliError::Usage(message.to_string())
}

fn io_code(e: &io::Error) -> Code {
    if e.kind() == io::ErrorKind::NotFound {
        Code::NotFound
    } else {
        Code::Io
    }
}

const fn req(name: &'static str, kind: ArgKind) -> Arg {
    Arg {
        name,
        kind,
        required: true,
        default: None,
        choices: &[],
    }
}

const fn opt(name: &'static str, kind: ArgKind, default: Option<&'static str>) -> Arg {
    Arg {
        name,
        kind,
        required: false,
        default,
        choices: &[],
    }
}

const SOLVE_ARGS: &[Arg] = &[
    req("INDEX", ArgKind::Path),
    req("MANIFEST", ArgKind::Path),
    opt("--limit", ArgKind::Int, Some("100")),
    opt("--offset", ArgKind::Int, Some("0")),
];
const LOCK_ARGS: &[Arg] = &[
    req("INDEX", ArgKind::Path),
    req("MANIFEST", ArgKind::Path),
    req("-o", ArgKind::Path),
];
const INSTALL_ARGS: &[Arg] = &[
    req("LOCK", ArgKind::Path),
    req("STORE_DIR", ArgKind::Path),
    req("--from", ArgKind::Path),
    opt("--key", ArgKind::Text, None),
    opt("--limit", ArgKind::Int, Some("100")),
    opt("--offset", ArgKind::Int, Some("0")),
];
const SERVE_ARGS: &[Arg] = &[
    req("INDEX_DIR", ArgKind::Path),
    opt("--listen", ArgKind::Text, Some(DEFAULT_LISTEN)),
    opt("--key", ArgKind::Text, None),
];

const COMMANDS: &[Command] = &[
    Command {
        name: "solve",
        summary: "Resolve dependencies from INDEX and MANIFEST; --limit and --offset page the result (--json only)",
        effect: Effect::Read,
        args: SOLVE_ARGS,
        returns: "items: [{name, version}], total, more, next_offset",
        example: "argo --json solve index.txt Cargo.argo",
    },
    Command {
        name: "lock",
        summary: "Resolve INDEX and MANIFEST and write a lock file to -o FILE",
        effect: Effect::Write,
        args: LOCK_ARGS,
        returns: "path (the lock file), bytes, count (packages locked)",
        example: "argo --json lock index.txt Cargo.argo -o argo.lock",
    },
    Command {
        name: "install",
        summary: "Install the packages in LOCK into STORE_DIR from the signed packages in --from DIR",
        effect: Effect::Net,
        args: INSTALL_ARGS,
        returns: "items: [{name, version}], total, more, next_offset, path (the store folder), bytes (package bytes read), duration_ms",
        example: "argo --json install argo.lock ./store --from ./pkgs",
    },
    Command {
        name: "serve",
        summary: "Serve signed packages from INDEX_DIR on --listen; prints one envelope once bound, then runs",
        effect: Effect::Net,
        args: SERVE_ARGS,
        returns: "listen (the bound address), path (the served folder), count (packages served)",
        example: "argo --json serve ./pkgs --listen 127.0.0.1:0",
    },
];

const MANIFEST: ToolManifest = ToolManifest {
    tool: "argo",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Package manager: solving, lock files, local install and registry serving",
    commands: COMMANDS,
};

/// The paging flags that exist only under `--json`.
#[derive(Default)]
struct Page {
    limit: Option<usize>,
    offset: Option<usize>,
}

/// Pull `--limit N` and `--offset N` out of the arguments under `--json`. In text mode they stay and fail as before.
fn take_page(args: &mut Vec<String>, json: bool) -> Result<Page, CliError> {
    let mut page = Page::default();
    if !json {
        return Ok(page);
    }
    let mut kept = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        if flag != "--limit" && flag != "--offset" {
            kept.push(args[i].clone());
            i += 1;
            continue;
        }
        let Some(value) = args.get(i + 1) else {
            return Err(CliError::Usage(format!("{flag} requires a value")));
        };
        let number: usize = value
            .parse()
            .map_err(|_| CliError::Usage(format!("invalid value '{value}' for {flag}")))?;
        if flag == "--limit" {
            if !(1..=MAX_LIMIT).contains(&number) {
                return Err(CliError::Usage(format!(
                    "--limit must be between 1 and {MAX_LIMIT}"
                )));
            }
            page.limit = Some(number);
        } else {
            page.offset = Some(number);
        }
        i += 2;
    }
    *args = kept;
    Ok(page)
}

/// One page of a solution as `{items, total, more, next_offset}`.
fn page_data(sol: &Solution, page: &Page) -> Json {
    let limit = page.limit.unwrap_or(DEFAULT_LIMIT);
    let offset = page.offset.unwrap_or(0);
    let items = sol
        .iter()
        .skip(offset)
        .take(limit)
        .map(|(name, ver)| {
            Json::obj([
                ("name", Json::Str(name.clone())),
                ("version", Json::Str(ver.to_string())),
            ])
        })
        .collect();
    envelope::page(items, sol.len(), offset, limit)
}

fn read_input(path: &str) -> Result<String, CliError> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| op(Code::Io, format!("failed to read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read_to_string(path)
            .map_err(|e| op(io_code(&e), format!("failed to read '{path}': {e}")))
    }
}

fn write_output(path: &str, content: &str) -> Result<(), CliError> {
    if path == "-" {
        let mut out = io::stdout().lock();
        out.write_all(content.as_bytes())
            .map_err(|e| op(Code::Io, format!("failed to write stdout: {e}")))?;
        out.flush()
            .map_err(|e| op(Code::Io, format!("failed to flush stdout: {e}")))?;
        Ok(())
    } else {
        fs::write(path, content)
            .map_err(|e| op(io_code(&e), format!("failed to write '{path}': {e}")))
    }
}

fn is_valid_pkg_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn parse_index_text(text: &str) -> Result<Index, CliError> {
    let mut index = Index::new();
    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let name = parts.next().ok_or_else(|| {
            op(
                Code::Invalid,
                format!("index line {line_no}: missing package name"),
            )
        })?;
        if !is_valid_pkg_name(name) {
            return Err(op(
                Code::Invalid,
                format!("index line {line_no}: invalid package name '{name}'"),
            ));
        }
        let ver_str = parts.next().ok_or_else(|| {
            op(
                Code::Invalid,
                format!("index line {line_no}: missing version"),
            )
        })?;
        let ver = Version::parse(ver_str)
            .map_err(|e| op(Code::Invalid, format!("index line {line_no}: {e}")))?;
        let mut deps = Vec::new();
        while let Some(dep) = parts.next() {
            if !is_valid_pkg_name(dep) {
                return Err(op(
                    Code::Invalid,
                    format!("index line {line_no}: invalid dependency name '{dep}'"),
                ));
            }
            let req_str = parts.next().ok_or_else(|| {
                op(
                    Code::Invalid,
                    format!("index line {line_no}: missing requirement for '{dep}'"),
                )
            })?;
            let req = Req::parse(req_str)
                .map_err(|e| op(Code::Invalid, format!("index line {line_no}: {e}")))?;
            deps.push((dep.to_string(), req));
        }
        index.add(name, ver, deps);
    }
    Ok(index)
}

fn solve_from_paths(index_path: &str, manifest_path: &str) -> Result<Solution, CliError> {
    if index_path == "-" && manifest_path == "-" {
        return Err(usage_err("INDEX and MANIFEST cannot both be '-'"));
    }
    let index_text = read_input(index_path)?;
    let manifest_text = read_input(manifest_path)?;
    let index = parse_index_text(&index_text)?;
    let manifest = Manifest::parse(&manifest_text)
        .map_err(|e| op(Code::Invalid, format!("manifest error: {e}")))?;
    solve::solve(&index, &manifest.dependencies, None).map_err(|e| CliError::Op {
        code: Code::Conflict,
        message: e.to_string(),
        hint: Some(
            "no version satisfies every requirement; relax a requirement in MANIFEST or add a version to INDEX"
                .to_string(),
        ),
    })
}

/// Text mode prints the lines and returns None. JSON mode returns the page as data.
fn cmd_solve(args: &[String], json: bool) -> Result<Option<Json>, CliError> {
    let mut args = args.to_vec();
    let page = take_page(&mut args, json)?;
    if args.len() != 2 {
        return Err(usage_err("solve expects INDEX MANIFEST"));
    }
    let sol = solve_from_paths(&args[0], &args[1])?;
    if json {
        return Ok(Some(page_data(&sol, &page)));
    }
    let mut out = io::stdout().lock();
    for (name, ver) in &sol {
        writeln!(out, "{name} {ver}")
            .map_err(|e| op(Code::Io, format!("failed to write stdout: {e}")))?;
    }
    Ok(None)
}

fn cmd_lock(args: &[String], json: bool) -> Result<Option<Json>, CliError> {
    let mut pos: Vec<&str> = Vec::new();
    let mut out_path: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "-o" {
            i += 1;
            let val = args.get(i).ok_or_else(|| usage_err("-o requires a path"))?;
            if out_path.is_some() {
                return Err(usage_err("duplicate -o flag"));
            }
            out_path = Some(val.as_str());
        } else if let Some(val) = arg.strip_prefix("-o=") {
            if out_path.is_some() {
                return Err(usage_err("duplicate -o flag"));
            }
            out_path = Some(val);
        } else if arg.starts_with('-') && arg != "-" {
            return Err(CliError::Usage(format!("unknown flag '{arg}'")));
        } else {
            pos.push(arg);
        }
        i += 1;
    }
    if pos.len() != 2 {
        return Err(usage_err("lock expects INDEX MANIFEST -o LOCK"));
    }
    let lock_out = out_path.ok_or_else(|| usage_err("lock requires -o LOCK"))?;
    if json && lock_out == "-" {
        return Err(usage_err(
            "lock --json writes a file; give -o FILE instead of -o -",
        ));
    }
    let sol = solve_from_paths(pos[0], pos[1])?;
    let formatted = lock::write(&sol);
    write_output(lock_out, &formatted)?;
    if json {
        return Ok(Some(Json::obj([
            ("path", Json::path(Path::new(lock_out))),
            ("bytes", int(formatted.len())),
            ("count", int(sol.len())),
        ])));
    }
    Ok(None)
}

fn int(n: usize) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn decode_hex_key(s: &str) -> Option<[u8; 32]> {
    let bytes = s.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_nibble(bytes[i * 2])?;
        let lo = hex_nibble(bytes[i * 2 + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_key_file(path: &Path) -> Result<Vec<[u8; 32]>, CliError> {
    let raw = fs::read(path).map_err(|e| {
        op(
            io_code(&e),
            format!("failed to read key file '{}': {e}", path.display()),
        )
    })?;
    if raw.len() == 32 {
        let mut k = [0u8; 32];
        k.copy_from_slice(&raw);
        return Ok(vec![k]);
    }
    let text = std::str::from_utf8(&raw).map_err(|_| {
        op(
            Code::Invalid,
            format!("key file '{}' is not valid UTF-8", path.display()),
        )
    })?;
    let mut keys = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let key = decode_hex_key(trimmed).ok_or_else(|| {
            op(
                Code::Invalid,
                format!(
                    "key file '{}' line {}: expected 64 hex chars",
                    path.display(),
                    idx + 1
                ),
            )
        })?;
        keys.push(key);
    }
    Ok(keys)
}

fn load_key_arg(spec: &str) -> Result<Vec<[u8; 32]>, CliError> {
    if let Some(k) = decode_hex_key(spec.trim()) {
        return Ok(vec![k]);
    }
    parse_key_file(Path::new(spec))
}

struct LoadedPkg {
    name: String,
    version: Version,
    blob: Vec<u8>,
}

fn load_dir_packages(
    dir: &Path,
    explicit_keys: &[[u8; 32]],
) -> Result<(Vec<LoadedPkg>, Vec<[u8; 32]>), CliError> {
    if !dir.is_dir() {
        return Err(op(
            Code::NotFound,
            format!(
                "package directory '{}' does not exist or is not a directory",
                dir.display()
            ),
        ));
    }
    let mut entries: Vec<PathBuf> = Vec::new();
    let rd = fs::read_dir(dir).map_err(|e| {
        op(
            io_code(&e),
            format!("failed to read '{}': {e}", dir.display()),
        )
    })?;
    for entry_res in rd {
        let entry = entry_res.map_err(|e| op(Code::Io, format!("failed to read entry: {e}")))?;
        let path = entry.path();
        if path.is_file() {
            entries.push(path);
        }
    }
    entries.sort();

    let mut keys: BTreeSet<[u8; 32]> = explicit_keys.iter().copied().collect();
    if keys.is_empty() {
        for path in &entries {
            let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if fname.ends_with(".pub") || fname == "trusted.keys" {
                for k in parse_key_file(path)? {
                    keys.insert(k);
                }
            }
        }
    }

    let mut raw_blobs: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for path in &entries {
        let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if fname.ends_with(".pub") || fname == "trusted.keys" {
            continue;
        }
        let is_pkg_ext = fname.ends_with(".pkg");
        let bytes = fs::read(path).map_err(|e| {
            op(
                io_code(&e),
                format!("failed to read '{}': {e}", path.display()),
            )
        })?;
        if is_pkg_ext || bytes.starts_with(b"CAIRNSIG1\n") {
            raw_blobs.push((path.clone(), bytes));
        }
    }

    if keys.is_empty() {
        for (_, blob) in &raw_blobs {
            if blob.len() >= 106 && blob.starts_with(b"CAIRNSIG1\n") {
                let mut pk = [0u8; 32];
                pk.copy_from_slice(&blob[10..42]);
                keys.insert(pk);
            }
        }
    }

    let trusted: Vec<[u8; 32]> = keys.into_iter().collect();
    let mut pkgs = Vec::with_capacity(raw_blobs.len());
    for (path, blob) in raw_blobs {
        let p = pkg::open(&blob, &trusted)
            .map_err(|e| op(Code::Invalid, format!("package '{}': {e}", path.display())))?;
        pkgs.push(LoadedPkg {
            name: p.manifest.name,
            version: p.manifest.version,
            blob,
        });
    }
    Ok((pkgs, trusted))
}

fn http_put_pkg(addr: SocketAddr, pkg: &LoadedPkg) -> Result<(), CliError> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| op(Code::Io, format!("failed to connect to registry: {e}")))?;
    let timeout = Duration::from_secs(10);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let target = format!("/pkg/{}/{}", pkg.name, pkg.version);
    let req = Request {
        method: "PUT".to_string(),
        target,
        minor: 1,
        headers: vec![
            ("Host".to_string(), addr.to_string().into_bytes()),
            ("Connection".to_string(), b"close".to_vec()),
            (
                "Content-Length".to_string(),
                pkg.blob.len().to_string().into_bytes(),
            ),
        ],
        body: pkg.blob.clone(),
    };
    let wire = write_request(&req);
    stream
        .write_all(&wire)
        .map_err(|e| op(Code::Io, format!("failed to upload package: {e}")))?;
    stream
        .flush()
        .map_err(|e| op(Code::Io, format!("failed to flush upload: {e}")))?;

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .map_err(|e| op(Code::Io, format!("failed to read upload response: {e}")))?;
    let (resp, _) = parse_response(&buf, &Limits::default(), false)
        .map_err(|e| op(Code::Io, format!("bad registry response: {e:?}")))?;
    if !(200..300).contains(&resp.status) {
        let body = String::from_utf8_lossy(&resp.body);
        return Err(op(
            Code::Denied,
            format!(
                "registry rejected {} {}: HTTP {} {} ({})",
                pkg.name, pkg.version, resp.status, resp.reason, body
            ),
        ));
    }
    Ok(())
}

/// The ACI code for a failed install step.
fn install_code(e: &InstallError) -> Code {
    match e {
        InstallError::Http(_) | InstallError::Io(_) => Code::Io,
        InstallError::Solve(_) => Code::Conflict,
        InstallError::Package(..) | InstallError::NotInstalled(_) | InstallError::InUse(_) => {
            Code::Invalid
        }
    }
}

fn cmd_install(args: &[String], json: bool) -> Result<Option<Json>, CliError> {
    let mut args = args.to_vec();
    let page = take_page(&mut args, json)?;
    let args = &args[..];
    let mut pos: Vec<&str> = Vec::new();
    let mut from_dir: Option<&str> = None;
    let mut explicit_keys: Vec<[u8; 32]> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--from" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| usage_err("--from requires a directory"))?;
            if from_dir.is_some() {
                return Err(usage_err("duplicate --from flag"));
            }
            from_dir = Some(val.as_str());
        } else if let Some(val) = arg.strip_prefix("--from=") {
            if from_dir.is_some() {
                return Err(usage_err("duplicate --from flag"));
            }
            from_dir = Some(val);
        } else if arg == "--key" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| usage_err("--key requires a hex key or path"))?;
            explicit_keys.extend(load_key_arg(val)?);
        } else if let Some(val) = arg.strip_prefix("--key=") {
            explicit_keys.extend(load_key_arg(val)?);
        } else if arg.starts_with('-') && arg != "-" {
            return Err(CliError::Usage(format!("unknown flag '{arg}'")));
        } else {
            pos.push(arg);
        }
        i += 1;
    }

    if pos.len() != 2 {
        return Err(usage_err("install expects LOCK STORE_DIR --from DIR"));
    }
    let dir = from_dir.ok_or_else(|| usage_err("install requires --from DIR"))?;
    let lock_text = read_input(pos[0])?;
    let locked =
        lock::read(&lock_text).map_err(|e| op(Code::Invalid, format!("lock error: {e}")))?;

    let mut deps: Vec<(String, Req)> = Vec::with_capacity(locked.len());
    for (name, ver) in &locked {
        let req = Req::parse(&format!("={ver}")).map_err(|e| {
            op(
                Code::Invalid,
                format!("invalid locked version for '{name}': {e}"),
            )
        })?;
        deps.push((name.clone(), req));
    }

    let (pkgs, trusted) = load_dir_packages(Path::new(dir), &explicit_keys)?;
    let package_bytes: usize = pkgs.iter().map(|p| p.blob.len()).sum();
    let reg = Registry::start("127.0.0.1:0", trusted.clone())
        .map_err(|e| op(Code::Io, format!("failed to start local registry: {e}")))?;
    let addr = reg.addr();

    let started = Instant::now();
    let res = (|| -> Result<Solution, CliError> {
        for p in &pkgs {
            http_put_pkg(addr, p)?;
        }
        client::install(addr, &deps, Path::new(pos[1]), &trusted).map_err(|e| CliError::Op {
            code: install_code(&e),
            message: e.to_string(),
            hint: None,
        })
    })();
    let elapsed = started.elapsed();

    reg.stop();
    let sol = res?;
    if json {
        let mut data = match page_data(&sol, &page) {
            Json::Obj(fields) => fields,
            other => return Ok(Some(other)),
        };
        data.push(("path".to_string(), Json::path(Path::new(pos[1]))));
        data.push(("bytes".to_string(), int(package_bytes)));
        data.push(("duration_ms".to_string(), int_u128(elapsed.as_millis())));
        return Ok(Some(Json::Obj(data)));
    }
    let mut out = io::stdout().lock();
    for (name, ver) in &sol {
        writeln!(out, "{name} {ver}")
            .map_err(|e| op(Code::Io, format!("failed to write stdout: {e}")))?;
    }
    Ok(None)
}

fn int_u128(n: u128) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn cmd_serve(args: &[String], json: bool) -> Result<Option<Json>, CliError> {
    let mut pos: Vec<&str> = Vec::new();
    let mut listen = DEFAULT_LISTEN;
    let mut explicit_keys: Vec<[u8; 32]> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--listen" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| usage_err("--listen requires an address"))?;
            listen = val.as_str();
        } else if let Some(val) = arg.strip_prefix("--listen=") {
            listen = val;
        } else if arg == "--key" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| usage_err("--key requires a hex key or path"))?;
            explicit_keys.extend(load_key_arg(val)?);
        } else if let Some(val) = arg.strip_prefix("--key=") {
            explicit_keys.extend(load_key_arg(val)?);
        } else if arg.starts_with('-') {
            return Err(CliError::Usage(format!("unknown flag '{arg}'")));
        } else {
            pos.push(arg);
        }
        i += 1;
    }

    if pos.len() != 1 {
        return Err(usage_err(
            "serve expects INDEX_DIR [--listen 127.0.0.1:7500]",
        ));
    }
    let (pkgs, trusted) = load_dir_packages(Path::new(pos[0]), &explicit_keys)?;
    let reg = Registry::start(listen, trusted).map_err(|e| CliError::Op {
        code: Code::Io,
        message: format!("failed to listen on '{listen}': {e}"),
        hint: Some(format!(
            "nothing is bound to {listen}; choose a free address such as 127.0.0.1:0"
        )),
    })?;
    let addr = reg.addr();
    for p in &pkgs {
        if let Err(e) = http_put_pkg(addr, p) {
            reg.stop();
            return Err(e);
        }
    }
    if json {
        let data = Json::obj([
            ("listen", Json::Str(addr.to_string())),
            ("path", Json::path(Path::new(pos[0]))),
            ("count", int(pkgs.len())),
        ]);
        print!("{}", envelope::ok("argo", "serve", data));
        let _ = io::stdout().flush();
    } else {
        let mut out = io::stdout().lock();
        let _ = writeln!(out, "listening on {addr}");
        let _ = out.flush();
    }
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

/// Help and version text (text mode prints; JSON mode returns it as data).
fn help_out(json: bool) -> Result<Option<Json>, CliError> {
    if json {
        Ok(Some(Json::obj([("usage", Json::Str(usage().into()))])))
    } else {
        println!("{}", usage());
        Ok(None)
    }
}

fn run(args: &[String], json: bool) -> Result<Option<Json>, CliError> {
    let Some(first) = args.first() else {
        return Err(usage_err("missing subcommand (try --help)"));
    };
    match first.as_str() {
        "--help" | "-h" | "help" => help_out(json),
        "--version" | "-V" => {
            if json {
                Ok(Some(Json::obj([(
                    "version",
                    Json::Str(VERSION_STR.into()),
                )])))
            } else {
                println!("{VERSION_STR}");
                Ok(None)
            }
        }
        sub => {
            let rest = &args[1..];
            if rest.iter().any(|a| a == "--help" || a == "-h") {
                return help_out(json);
            }
            match sub {
                "solve" => cmd_solve(rest, json),
                "lock" => cmd_lock(rest, json),
                "install" => cmd_install(rest, json),
                "serve" => cmd_serve(rest, json),
                other => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
            }
        }
    }
}

/// The subcommand name for the envelope, or the empty string when the name is not one of ours.
fn known_command(first: Option<&String>) -> &'static str {
    const NAMES: [&str; 7] = [
        "solve",
        "lock",
        "install",
        "serve",
        "help",
        "--help",
        "--version",
    ];
    first
        .and_then(|a| NAMES.iter().copied().find(|n| *n == a.as_str()))
        .unwrap_or("")
}

/// `argo --json ...`: one envelope on stdout; stderr keeps the `argo: message` line.
fn run_json(args: &[String]) -> u8 {
    let cmd = known_command(args.first());
    match run(args, true) {
        Ok(data) => {
            print!("{}", envelope::ok("argo", cmd, data.unwrap_or(Json::Null)));
            0
        }
        Err(CliError::Usage(msg)) => {
            eprintln!("argo: {msg}");
            let hint = Some("run: argo --describe");
            print!("{}", envelope::err("argo", cmd, Code::Usage, &msg, hint));
            2
        }
        Err(CliError::Op {
            code,
            message,
            hint,
        }) => {
            eprintln!("argo: {message}");
            print!(
                "{}",
                envelope::err("argo", cmd, code, &message, hint.as_deref())
            );
            1
        }
    }
}

fn main() {
    let mut args: Vec<String> = env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if args.len() == 1 && args[0] == "--describe" {
        println!("{}", MANIFEST.to_json().write());
        return;
    }
    let json = args.first().map(String::as_str) == Some("--json");
    if json {
        args.remove(0);
        process::exit(i32::from(run_json(&args)));
    }
    if let Err(err) = run(&args, false) {
        match err {
            CliError::Usage(msg) => {
                eprintln!("argo: {msg}");
                process::exit(2);
            }
            CliError::Op { message, .. } => {
                eprintln!("argo: {message}");
                process::exit(1);
            }
        }
    }
}
