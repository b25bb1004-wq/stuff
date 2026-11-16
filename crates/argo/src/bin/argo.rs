//! CLI tool for argo: dependency solving, lock files, local installation, and registry serving.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

use argo::client;
use argo::lock;
use argo::manifest::Manifest;
use argo::pkg;
use argo::registry::Registry;
use argo::solve::{self, Index};
use argo::version::{Req, Version};
use mercury::{parse_response, write_request, Limits, Request};

const VERSION_STR: &str = "argo 0.1.0";

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

enum CliError {
    Usage(String),
    Op(String),
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

fn write_output(path: &str, content: &str) -> Result<(), CliError> {
    if path == "-" {
        let mut out = io::stdout().lock();
        out.write_all(content.as_bytes())
            .map_err(|e| CliError::Op(format!("failed to write stdout: {e}")))?;
        out.flush()
            .map_err(|e| CliError::Op(format!("failed to flush stdout: {e}")))?;
        Ok(())
    } else {
        fs::write(path, content).map_err(|e| CliError::Op(format!("failed to write '{path}': {e}")))
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
        let name = parts
            .next()
            .ok_or_else(|| CliError::Op(format!("index line {line_no}: missing package name")))?;
        if !is_valid_pkg_name(name) {
            return Err(CliError::Op(format!(
                "index line {line_no}: invalid package name '{name}'"
            )));
        }
        let ver_str = parts
            .next()
            .ok_or_else(|| CliError::Op(format!("index line {line_no}: missing version")))?;
        let ver = Version::parse(ver_str)
            .map_err(|e| CliError::Op(format!("index line {line_no}: {e}")))?;
        let mut deps = Vec::new();
        while let Some(dep) = parts.next() {
            if !is_valid_pkg_name(dep) {
                return Err(CliError::Op(format!(
                    "index line {line_no}: invalid dependency name '{dep}'"
                )));
            }
            let req_str = parts.next().ok_or_else(|| {
                CliError::Op(format!(
                    "index line {line_no}: missing requirement for '{dep}'"
                ))
            })?;
            let req = Req::parse(req_str)
                .map_err(|e| CliError::Op(format!("index line {line_no}: {e}")))?;
            deps.push((dep.to_string(), req));
        }
        index.add(name, ver, deps);
    }
    Ok(index)
}

fn solve_from_paths(index_path: &str, manifest_path: &str) -> Result<solve::Solution, CliError> {
    if index_path == "-" && manifest_path == "-" {
        return Err(CliError::Usage(
            "INDEX and MANIFEST cannot both be '-'".to_string(),
        ));
    }
    let index_text = read_input(index_path)?;
    let manifest_text = read_input(manifest_path)?;
    let index = parse_index_text(&index_text)?;
    let manifest = Manifest::parse(&manifest_text)
        .map_err(|e| CliError::Op(format!("manifest error: {e}")))?;
    solve::solve(&index, &manifest.dependencies, None).map_err(|e| CliError::Op(e.to_string()))
}

fn cmd_solve(args: &[String]) -> Result<(), CliError> {
    if args.len() != 2 {
        return Err(CliError::Usage("solve expects INDEX MANIFEST".to_string()));
    }
    let sol = solve_from_paths(&args[0], &args[1])?;
    let mut out = io::stdout().lock();
    for (name, ver) in &sol {
        writeln!(out, "{name} {ver}")
            .map_err(|e| CliError::Op(format!("failed to write stdout: {e}")))?;
    }
    Ok(())
}

fn cmd_lock(args: &[String]) -> Result<(), CliError> {
    let mut pos: Vec<&str> = Vec::new();
    let mut out_path: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "-o" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| CliError::Usage("-o requires a path".to_string()))?;
            if out_path.is_some() {
                return Err(CliError::Usage("duplicate -o flag".to_string()));
            }
            out_path = Some(val.as_str());
        } else if let Some(val) = arg.strip_prefix("-o=") {
            if out_path.is_some() {
                return Err(CliError::Usage("duplicate -o flag".to_string()));
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
        return Err(CliError::Usage(
            "lock expects INDEX MANIFEST -o LOCK".to_string(),
        ));
    }
    let lock_out = out_path.ok_or_else(|| CliError::Usage("lock requires -o LOCK".to_string()))?;
    let sol = solve_from_paths(pos[0], pos[1])?;
    let formatted = lock::write(&sol);
    write_output(lock_out, &formatted)
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
    let raw = fs::read(path)
        .map_err(|e| CliError::Op(format!("failed to read key file '{}': {e}", path.display())))?;
    if raw.len() == 32 {
        let mut k = [0u8; 32];
        k.copy_from_slice(&raw);
        return Ok(vec![k]);
    }
    let text = std::str::from_utf8(&raw)
        .map_err(|_| CliError::Op(format!("key file '{}' is not valid UTF-8", path.display())))?;
    let mut keys = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let key = decode_hex_key(trimmed).ok_or_else(|| {
            CliError::Op(format!(
                "key file '{}' line {}: expected 64 hex chars",
                path.display(),
                idx + 1
            ))
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
        return Err(CliError::Op(format!(
            "package directory '{}' does not exist or is not a directory",
            dir.display()
        )));
    }
    let mut entries: Vec<PathBuf> = Vec::new();
    let rd = fs::read_dir(dir)
        .map_err(|e| CliError::Op(format!("failed to read '{}': {e}", dir.display())))?;
    for entry_res in rd {
        let entry = entry_res.map_err(|e| CliError::Op(format!("failed to read entry: {e}")))?;
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
        let bytes = fs::read(path)
            .map_err(|e| CliError::Op(format!("failed to read '{}': {e}", path.display())))?;
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
            .map_err(|e| CliError::Op(format!("package '{}': {e}", path.display())))?;
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
        .map_err(|e| CliError::Op(format!("failed to connect to registry: {e}")))?;
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
        .map_err(|e| CliError::Op(format!("failed to upload package: {e}")))?;
    stream
        .flush()
        .map_err(|e| CliError::Op(format!("failed to flush upload: {e}")))?;

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .map_err(|e| CliError::Op(format!("failed to read upload response: {e}")))?;
    let (resp, _) = parse_response(&buf, &Limits::default(), false)
        .map_err(|e| CliError::Op(format!("bad registry response: {e:?}")))?;
    if !(200..300).contains(&resp.status) {
        let body = String::from_utf8_lossy(&resp.body);
        return Err(CliError::Op(format!(
            "registry rejected {} {}: HTTP {} {} ({})",
            pkg.name, pkg.version, resp.status, resp.reason, body
        )));
    }
    Ok(())
}

fn cmd_install(args: &[String]) -> Result<(), CliError> {
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
                .ok_or_else(|| CliError::Usage("--from requires a directory".to_string()))?;
            if from_dir.is_some() {
                return Err(CliError::Usage("duplicate --from flag".to_string()));
            }
            from_dir = Some(val.as_str());
        } else if let Some(val) = arg.strip_prefix("--from=") {
            if from_dir.is_some() {
                return Err(CliError::Usage("duplicate --from flag".to_string()));
            }
            from_dir = Some(val);
        } else if arg == "--key" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| CliError::Usage("--key requires a hex key or path".to_string()))?;
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
        return Err(CliError::Usage(
            "install expects LOCK STORE_DIR --from DIR".to_string(),
        ));
    }
    let dir = from_dir.ok_or_else(|| CliError::Usage("install requires --from DIR".to_string()))?;
    let lock_text = read_input(pos[0])?;
    let locked = lock::read(&lock_text).map_err(|e| CliError::Op(format!("lock error: {e}")))?;

    let mut deps: Vec<(String, Req)> = Vec::with_capacity(locked.len());
    for (name, ver) in &locked {
        let req = Req::parse(&format!("={ver}"))
            .map_err(|e| CliError::Op(format!("invalid locked version for '{name}': {e}")))?;
        deps.push((name.clone(), req));
    }

    let (pkgs, trusted) = load_dir_packages(Path::new(dir), &explicit_keys)?;
    let reg = Registry::start("127.0.0.1:0", trusted.clone())
        .map_err(|e| CliError::Op(format!("failed to start local registry: {e}")))?;
    let addr = reg.addr();

    let res = (|| -> Result<solve::Solution, CliError> {
        for p in &pkgs {
            http_put_pkg(addr, p)?;
        }
        client::install(addr, &deps, Path::new(pos[1]), &trusted)
            .map_err(|e| CliError::Op(e.to_string()))
    })();

    reg.stop();
    let sol = res?;
    let mut out = io::stdout().lock();
    for (name, ver) in &sol {
        writeln!(out, "{name} {ver}")
            .map_err(|e| CliError::Op(format!("failed to write stdout: {e}")))?;
    }
    Ok(())
}

fn cmd_serve(args: &[String]) -> Result<(), CliError> {
    let mut pos: Vec<&str> = Vec::new();
    let mut listen = "127.0.0.1:7500";
    let mut explicit_keys: Vec<[u8; 32]> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--listen" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| CliError::Usage("--listen requires an address".to_string()))?;
            listen = val.as_str();
        } else if let Some(val) = arg.strip_prefix("--listen=") {
            listen = val;
        } else if arg == "--key" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| CliError::Usage("--key requires a hex key or path".to_string()))?;
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
        return Err(CliError::Usage(
            "serve expects INDEX_DIR [--listen 127.0.0.1:7500]".to_string(),
        ));
    }
    let (pkgs, trusted) = load_dir_packages(Path::new(pos[0]), &explicit_keys)?;
    let reg = Registry::start(listen, trusted)
        .map_err(|e| CliError::Op(format!("failed to listen on '{listen}': {e}")))?;
    let addr = reg.addr();
    for p in &pkgs {
        if let Err(e) = http_put_pkg(addr, p) {
            reg.stop();
            return Err(e);
        }
    }
    {
        let mut out = io::stdout().lock();
        let _ = writeln!(out, "listening on {addr}");
        let _ = out.flush();
    }
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
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
                "solve" => cmd_solve(rest),
                "lock" => cmd_lock(rest),
                "install" => cmd_install(rest),
                "serve" => cmd_serve(rest),
                other => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
            }
        }
    }
}

fn main() {
    if let Err(err) = run() {
        match err {
            CliError::Usage(msg) => {
                eprintln!("argo: {msg}");
                process::exit(2);
            }
            CliError::Op(msg) => {
                eprintln!("argo: {msg}");
                process::exit(1);
            }
        }
    }
}
