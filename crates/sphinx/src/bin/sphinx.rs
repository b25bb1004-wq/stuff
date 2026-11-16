//! Command-line interface for sphinx cryptographic primitives.
//!
//! Text mode is unchanged. `--describe` prints the manifest; `--json` prints one ACI envelope
//! (docs/ai-computer/ACI.md). Each command computes its value once and either prints it or puts it in the envelope.
#![forbid(unsafe_code)]

use sphinx::aead;
use sphinx::ed25519;
use sphinx::hmac::hmac_sha256;
use sphinx::sha256::Sha256;
use sphinx::sha512::Sha512;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use aci::envelope::{self, Code, TEXT_CAP};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};

const VERSION: &str = "sphinx 0.1.0";

const USAGE: &str = "\
Usage: sphinx <subcommand> [args...]

Subcommands:
  hash ALG FILE               Hash FILE ('-' for stdin) with sha256 or sha512
  hmac KEYHEX FILE            Compute HMAC-SHA-256 of FILE with hex key
  keygen -o KEYFILE           Write an Ed25519 seed file (mode 600 on Unix), print the public key hex
  sign KEYFILE FILE           Sign FILE with 32-byte Ed25519 seed file
  verify PUBHEX SIGHEX FILE   Verify Ed25519 signature (exit 0 ok, 1 failed)
  seal KEYFILE FILE [-o OUT]  Encrypt FILE with 32-byte key file (ChaCha20-Poly1305)
  open KEYFILE FILE [-o OUT]  Decrypt FILE with 32-byte key file (ChaCha20-Poly1305)

Options:
  --help, -h                  Print this help message
  --version, -V               Print version

Examples:
  sphinx hash sha256 notes.txt
  sphinx hmac 4a656665 msg.bin
  sphinx keygen -o id.key
  sphinx sign id.key msg.txt
  sphinx verify <pubhex> <sighex> msg.txt
  sphinx seal box.key secret.txt -o secret.enc
  sphinx open box.key secret.enc";

const fn req(name: &'static str, kind: ArgKind) -> Arg {
    Arg {
        name,
        kind,
        required: true,
        default: None,
        choices: &[],
    }
}

const fn out_flag(required: bool) -> Arg {
    Arg {
        name: "-o",
        kind: ArgKind::Path,
        required,
        default: None,
        choices: &[],
    }
}

const HASH_ARGS: &[Arg] = &[
    Arg {
        name: "ALG",
        kind: ArgKind::Choice,
        required: true,
        default: None,
        choices: &["sha256", "sha512"],
    },
    req("FILE", ArgKind::Path),
];
const HMAC_ARGS: &[Arg] = &[req("KEYHEX", ArgKind::Text), req("FILE", ArgKind::Path)];
const KEYGEN_ARGS: &[Arg] = &[out_flag(true)];
const SIGN_ARGS: &[Arg] = &[req("KEYFILE", ArgKind::Path), req("FILE", ArgKind::Path)];
const VERIFY_ARGS: &[Arg] = &[
    req("PUBHEX", ArgKind::Text),
    req("SIGHEX", ArgKind::Text),
    req("FILE", ArgKind::Path),
];
const SEAL_ARGS: &[Arg] = &[
    req("KEYFILE", ArgKind::Path),
    req("FILE", ArgKind::Path),
    out_flag(false),
];

const COMMANDS: &[Command] = &[
    Command {
        name: "hash",
        summary: "Hash FILE ('-' for stdin) with sha256 or sha512",
        effect: Effect::Read,
        args: HASH_ARGS,
        returns: "algorithm, hash (lowercase hex)",
        example: "sphinx --json hash sha256 notes.txt",
    },
    Command {
        name: "hmac",
        summary: "HMAC-SHA-256 of FILE with the hex key KEYHEX",
        effect: Effect::Read,
        args: HMAC_ARGS,
        returns: "mac (lowercase hex)",
        example: "sphinx --json hmac 4a656665 msg.bin",
    },
    Command {
        name: "keygen",
        summary: "Write a new Ed25519 seed file (mode 600 on Unix); --json cannot write the seed to stdout",
        effect: Effect::Write,
        args: KEYGEN_ARGS,
        returns: "path (the seed file), public_key (lowercase hex)",
        example: "sphinx --json keygen -o id.key",
    },
    Command {
        name: "sign",
        summary: "Sign FILE with the 32-byte Ed25519 seed file KEYFILE",
        effect: Effect::Read,
        args: SIGN_ARGS,
        returns: "signature (lowercase hex)",
        example: "sphinx --json sign id.key msg.txt",
    },
    Command {
        name: "verify",
        summary: "Check an Ed25519 signature; a wrong signature is valid false with exit 1",
        effect: Effect::Read,
        args: VERIFY_ARGS,
        returns: "valid (bool)",
        example: "sphinx --json verify <pubhex> <sighex> msg.txt",
    },
    Command {
        name: "seal",
        summary: "Encrypt FILE with the 32-byte key file KEYFILE (ChaCha20-Poly1305); --json needs -o",
        effect: Effect::Write,
        args: SEAL_ARGS,
        returns: "path, bytes (the output is a random 12-byte nonce then the ciphertext)",
        example: "sphinx --json seal box.key secret.txt -o secret.enc",
    },
    Command {
        name: "open",
        summary: "Decrypt FILE with the key file KEYFILE; without -o, --json returns the plaintext",
        effect: Effect::Write,
        args: SEAL_ARGS,
        returns: "without -o: plaintext (UTF-8) or plaintext_hex, capped; with -o: path, bytes; failed authentication is code invalid",
        example: "sphinx --json open box.key secret.enc",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "sphinx",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Cryptographic primitives: hashes, MACs, Ed25519 signatures and ChaCha20-Poly1305",
    commands: COMMANDS,
};

/// A failed run: the ACI code, the message, and whether it is a usage error (exit 2) or an operation error (exit 1).
enum CliError {
    Op(Code, String),
    Usage(String),
}

impl CliError {
    fn code(&self) -> Code {
        match self {
            CliError::Op(code, _) => *code,
            CliError::Usage(_) => Code::Usage,
        }
    }

    fn message(&self) -> &str {
        match self {
            CliError::Op(_, msg) | CliError::Usage(msg) => msg,
        }
    }

    fn exit(&self) -> u8 {
        match self {
            CliError::Op(..) => 1,
            CliError::Usage(_) => 2,
        }
    }
}

/// What a command produced: the JSON data and the exit code for a successful run.
type Reply = Result<(Json, u8), CliError>;

fn io_code(err: &io::Error) -> Code {
    if err.kind() == io::ErrorKind::NotFound {
        Code::NotFound
    } else {
        Code::Io
    }
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0f)]));
    }
    out
}

fn decode_hex_vec(s: &str) -> Result<Vec<u8>, CliError> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(CliError::Op(
            Code::Invalid,
            String::from("hex string has odd length"),
        ));
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for &[hi_b, lo_b] in bytes.as_chunks::<2>().0 {
        let hi = hex_nibble(hi_b)?;
        let lo = hex_nibble(lo_b)?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn decode_hex_array<const N: usize>(s: &str, label: &str) -> Result<[u8; N], CliError> {
    let v = decode_hex_vec(s)
        .map_err(|_| CliError::Op(Code::Invalid, format!("invalid {label} hex")))?;
    let len = v.len();
    v.try_into().map_err(|_| {
        CliError::Op(
            Code::Invalid,
            format!("invalid {label} length: expected {N} bytes, got {len}"),
        )
    })
}

fn hex_nibble(b: u8) -> Result<u8, CliError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(CliError::Op(
            Code::Invalid,
            String::from("invalid hex character"),
        )),
    }
}

fn read_input(path: &str) -> Result<Vec<u8>, CliError> {
    let mut buf = Vec::new();
    if path == "-" {
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| CliError::Op(Code::Io, format!("failed to read stdin: {e}")))?;
    } else {
        let mut f = File::open(path)
            .map_err(|e| CliError::Op(io_code(&e), format!("cannot open '{path}': {e}")))?;
        f.read_to_end(&mut buf)
            .map_err(|e| CliError::Op(io_code(&e), format!("failed to read '{path}': {e}")))?;
    }
    Ok(buf)
}

fn read_key32(path: &str) -> Result<[u8; 32], CliError> {
    let data = read_input(path)?;
    let len = data.len();
    data.try_into().map_err(|_| {
        CliError::Op(
            Code::Invalid,
            format!("key file '{path}' must be 32 bytes, got {len}"),
        )
    })
}

fn write_output(out_path: Option<&str>, data: &[u8]) -> Result<(), CliError> {
    match out_path {
        None | Some("-") => io::stdout()
            .write_all(data)
            .map_err(|e| CliError::Op(Code::Io, format!("failed to write stdout: {e}"))),
        Some(path) => fs::write(path, data)
            .map_err(|e| CliError::Op(io_code(&e), format!("cannot write '{path}': {e}"))),
    }
}

fn fill_random(buf: &mut [u8]) -> Result<(), CliError> {
    entropy::fill(buf).map_err(|e| CliError::Op(Code::Internal, e.to_string()))
}

fn write_secret_key32(path: &str, key: &[u8; 32]) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut f = opts
            .open(path)
            .map_err(|e| CliError::Op(io_code(&e), format!("cannot write '{path}': {e}")))?;
        f.write_all(key)
            .map_err(|e| CliError::Op(Code::Io, format!("failed to write '{path}': {e}")))?;
        f.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| {
                CliError::Op(Code::Io, format!("cannot set permissions on '{path}': {e}"))
            })?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        let mut f = opts
            .open(path)
            .map_err(|e| CliError::Op(io_code(&e), format!("cannot write '{path}': {e}")))?;
        f.write_all(key)
            .map_err(|e| CliError::Op(Code::Io, format!("failed to write '{path}': {e}")))?;
        Ok(())
    }
}

fn hash_reader<F: FnMut(&[u8])>(path: &str, mut update: F) -> Result<(), CliError> {
    let mut buf = [0u8; 8192];
    if path == "-" {
        let mut stdin = io::stdin().lock();
        loop {
            let n = stdin
                .read(&mut buf)
                .map_err(|e| CliError::Op(Code::Io, format!("failed to read stdin: {e}")))?;
            if n == 0 {
                break;
            }
            update(&buf[..n]);
        }
    } else {
        let mut f = File::open(path)
            .map_err(|e| CliError::Op(io_code(&e), format!("cannot open '{path}': {e}")))?;
        loop {
            let n = f
                .read(&mut buf)
                .map_err(|e| CliError::Op(Code::Io, format!("failed to read '{path}': {e}")))?;
            if n == 0 {
                break;
            }
            update(&buf[..n]);
        }
    }
    Ok(())
}

/// The lowercase hex digest of FILE with `alg`.
fn digest_hex(alg: &str, path: &str) -> Result<String, CliError> {
    match alg {
        "sha256" => {
            let mut h = Sha256::new();
            hash_reader(path, |chunk| h.update(chunk))?;
            Ok(to_hex(&h.finalize()))
        }
        "sha512" => {
            let mut h = Sha512::new();
            hash_reader(path, |chunk| h.update(chunk))?;
            Ok(to_hex(&h.finalize()))
        }
        other => Err(CliError::Op(
            Code::Invalid,
            format!("unsupported hash algorithm '{other}' (expected sha256 or sha512)"),
        )),
    }
}

/// Print one hex value (text) or return it under `field` (JSON).
fn hex_reply(json: bool, field: &str, hex: String) -> Reply {
    if json {
        Ok((Json::obj([(field, Json::Str(hex))]), 0))
    } else {
        println!("{hex}");
        Ok((Json::Null, 0))
    }
}

fn usage(message: &str) -> CliError {
    CliError::Usage(message.to_string())
}

fn cmd_hash(args: &[String], json: bool) -> Reply {
    let [alg, path] = args else {
        return Err(usage("usage: sphinx hash ALG FILE"));
    };
    let hex = digest_hex(alg, path)?;
    if json {
        return Ok((
            Json::obj([
                ("algorithm", Json::Str(alg.clone())),
                ("hash", Json::Str(hex)),
            ]),
            0,
        ));
    }
    hex_reply(false, "hash", hex)
}

fn cmd_hmac(args: &[String], json: bool) -> Reply {
    let [key_hex, path] = args else {
        return Err(usage("usage: sphinx hmac KEYHEX FILE"));
    };
    let key = decode_hex_vec(key_hex)
        .map_err(|_| CliError::Op(Code::Invalid, String::from("invalid key hex")))?;
    let data = read_input(path)?;
    let tag = hmac_sha256(&key, &data);
    hex_reply(json, "mac", to_hex(&tag))
}

fn cmd_keygen(args: &[String], json: bool) -> Reply {
    let [flag, path] = args else {
        return Err(usage("usage: sphinx keygen -o KEYFILE"));
    };
    if flag != "-o" {
        return Err(usage("usage: sphinx keygen -o KEYFILE"));
    }
    if json && path == "-" {
        return Err(CliError::Op(
            Code::Unsupported,
            String::from(
                "keygen -o - writes the seed to stdout, which --json cannot carry; use -o KEYFILE",
            ),
        ));
    }
    let mut seed = [0u8; 32];
    fill_random(&mut seed)?;
    if path == "-" {
        io::stdout()
            .write_all(&seed)
            .map_err(|e| CliError::Op(Code::Io, format!("failed to write stdout: {e}")))?;
        return Ok((Json::Null, 0));
    }
    write_secret_key32(path, &seed)?;
    let pub_key = to_hex(&ed25519::public_key(&seed));
    if json {
        return Ok((
            Json::obj([
                ("path", Json::path(Path::new(path))),
                ("public_key", Json::Str(pub_key)),
            ]),
            0,
        ));
    }
    println!("{pub_key}");
    Ok((Json::Null, 0))
}

fn cmd_sign(args: &[String], json: bool) -> Reply {
    let [key_path, msg_path] = args else {
        return Err(usage("usage: sphinx sign KEYFILE FILE"));
    };
    let seed = read_key32(key_path)?;
    let msg = read_input(msg_path)?;
    let sig = ed25519::sign(&seed, &msg);
    hex_reply(json, "signature", to_hex(&sig))
}

fn cmd_verify(args: &[String], json: bool) -> Reply {
    let [pub_hex, sig_hex, msg_path] = args else {
        return Err(usage("usage: sphinx verify PUBHEX SIGHEX FILE"));
    };
    let pub_key: [u8; 32] = decode_hex_array(pub_hex, "public key")?;
    let sig: [u8; 64] = decode_hex_array(sig_hex, "signature")?;
    let msg = read_input(msg_path)?;
    let valid = ed25519::verify(&pub_key, &msg, &sig).is_ok();
    if json {
        // A wrong signature is an answer, not a failure: ok stays true, the exit code stays 1.
        return Ok((
            Json::obj([("valid", Json::Bool(valid))]),
            if valid { 0 } else { 1 },
        ));
    }
    if valid {
        println!("ok");
        Ok((Json::Null, 0))
    } else {
        Err(CliError::Op(
            Code::Invalid,
            String::from("verification failed"),
        ))
    }
}

type SealArgs<'a> = (&'a str, &'a str, Option<&'a str>);

fn parse_seal_open_args<'a>(args: &'a [String], verb: &str) -> Result<SealArgs<'a>, CliError> {
    match args {
        [key, input] => Ok((key.as_str(), input.as_str(), None)),
        [key, input, flag, out] if flag == "-o" => {
            Ok((key.as_str(), input.as_str(), Some(out.as_str())))
        }
        _ => Err(CliError::Usage(format!(
            "usage: sphinx {verb} KEYFILE FILE [-o OUT]"
        ))),
    }
}

/// `--json` cannot carry binary on stdout: seal and open need a file for it.
fn json_out_file<'a>(out: Option<&'a str>, verb: &str) -> Result<Option<&'a str>, CliError> {
    match out {
        Some("-") => Err(usage(
            "binary output to stdout is not available under --json; use -o FILE",
        )),
        None if verb == "seal" => Err(usage("seal writes binary output: add -o FILE under --json")),
        other => Ok(other),
    }
}

fn written(path: &str, bytes: usize) -> Json {
    Json::obj([
        ("path", Json::path(Path::new(path))),
        ("bytes", Json::Int(i64::try_from(bytes).unwrap_or(i64::MAX))),
    ])
}

fn cmd_seal(args: &[String], json: bool) -> Reply {
    let (key_path, in_path, out_path) = parse_seal_open_args(args, "seal")?;
    let out_path = if json {
        json_out_file(out_path, "seal")?
    } else {
        out_path
    };
    let key = read_key32(key_path)?;
    let plaintext = read_input(in_path)?;
    let mut nonce = [0u8; 12];
    fill_random(&mut nonce)?;
    let body = aead::seal(&key, &nonce, b"", &plaintext);
    let mut out = Vec::with_capacity(nonce.len() + body.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    write_output(out_path, &out)?;
    match (json, out_path) {
        (true, Some(path)) => Ok((written(path, out.len()), 0)),
        _ => Ok((Json::Null, 0)),
    }
}

/// The plaintext as text when it is UTF-8, otherwise as hex; cut at the text cap like other text values.
fn plaintext_fields(bytes: &[u8]) -> Vec<(String, Json)> {
    let (key, mut text) = match std::str::from_utf8(bytes) {
        Ok(s) => ("plaintext", s.to_string()),
        Err(_) => ("plaintext_hex", to_hex(bytes)),
    };
    let truncated = text.len() > TEXT_CAP;
    if truncated {
        let mut cut = TEXT_CAP;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    let mut fields = vec![(key.to_string(), Json::Str(text))];
    if truncated {
        fields.push(("truncated".to_string(), Json::Bool(true)));
        fields.push((
            "bytes".to_string(),
            Json::Int(i64::try_from(bytes.len()).unwrap_or(i64::MAX)),
        ));
    }
    fields
}

fn cmd_open(args: &[String], json: bool) -> Reply {
    let (key_path, in_path, out_path) = parse_seal_open_args(args, "open")?;
    if json {
        json_out_file(out_path, "open")?;
    }
    let key = read_key32(key_path)?;
    let sealed = read_input(in_path)?;
    let Some((nonce, body)) = sealed.split_first_chunk::<12>() else {
        return Err(CliError::Op(
            Code::Invalid,
            String::from("sealed input is too short"),
        ));
    };
    let plaintext = aead::open(&key, nonce, b"", body)
        .map_err(|_| CliError::Op(Code::Invalid, String::from("decryption failed")))?;
    if !json {
        write_output(out_path, &plaintext)?;
        return Ok((Json::Null, 0));
    }
    match out_path {
        Some(path) => {
            write_output(Some(path), &plaintext)?;
            Ok((written(path, plaintext.len()), 0))
        }
        None => Ok((Json::Obj(plaintext_fields(&plaintext)), 0)),
    }
}

fn run(args: &[String], json: bool) -> Reply {
    let Some(cmd) = args.first().map(String::as_str) else {
        return Err(usage("missing subcommand (try --help)"));
    };
    let rest = &args[1..];
    match cmd {
        "--help" | "-h" => {
            if json {
                return Ok((Json::obj([("usage", Json::Str(USAGE.into()))]), 0));
            }
            println!("{USAGE}");
            Ok((Json::Null, 0))
        }
        "--version" | "-V" => {
            if json {
                return Ok((Json::obj([("version", Json::Str(VERSION.into()))]), 0));
            }
            println!("{VERSION}");
            Ok((Json::Null, 0))
        }
        "hash" => cmd_hash(rest, json),
        "hmac" => cmd_hmac(rest, json),
        "keygen" => cmd_keygen(rest, json),
        "sign" => cmd_sign(rest, json),
        "verify" => cmd_verify(rest, json),
        "seal" => cmd_seal(rest, json),
        "open" => cmd_open(rest, json),
        other => Err(CliError::Usage(format!(
            "unknown subcommand '{other}' (try --help)"
        ))),
    }
}

/// The command name for the envelope, or the empty string when it is not one of ours.
fn known_command(first: Option<&String>) -> &'static str {
    const NAMES: [&str; 11] = [
        "hash",
        "hmac",
        "keygen",
        "sign",
        "verify",
        "seal",
        "open",
        "--help",
        "-h",
        "--version",
        "-V",
    ];
    first
        .and_then(|s| NAMES.iter().copied().find(|n| *n == s.as_str()))
        .unwrap_or("")
}

fn main() -> ExitCode {
    let mut raw: Vec<OsString> = env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut raw);
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    let args: Vec<String> = raw
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if json {
        let cmd = known_command(args.first());
        return match run(&args, true) {
            Ok((data, exit)) => {
                print!("{}", envelope::ok("sphinx", cmd, data));
                ExitCode::from(exit)
            }
            Err(err) => {
                eprintln!("sphinx: {}", err.message());
                let hint = matches!(err, CliError::Usage(_)).then_some("run: sphinx --describe");
                print!(
                    "{}",
                    envelope::err("sphinx", cmd, err.code(), err.message(), hint)
                );
                ExitCode::from(err.exit())
            }
        };
    }
    match run(&args, false) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("sphinx: {}", err.message());
            ExitCode::from(err.exit())
        }
    }
}
