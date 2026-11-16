//! CLI wrapper for Xenon UDP echo and file transfer.
//!
//! Text mode prints the same lines as before. `--describe` prints the manifest, and `--json` prints one
//! ACI envelope (docs/ai-computer/ACI.md). Each command builds its text and its data from the same values.
#![forbid(unsafe_code)]

use sphinx::x25519;
use std::env;
use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::net::{SocketAddr, UdpSocket};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
use xenon::{echo, echo_server, recv_file, send_file, Config, Lossy};

const VERSION: &str = "xenon 0.1.0";

const HELP: &str = "\
xenon - UDP echo and encrypted file transfer

Usage:
  xenon <subcommand> [args...]
  xenon --help | -h
  xenon --version

Subcommands:
  keygen
      Print a secret: and public: hex line (X25519 keys).
      Example: xenon keygen
  echo-server <addr> <secret-hex>
      Run a UDP echo server on <addr>.
      Example: xenon echo-server 127.0.0.1:9000 <secret-hex>
  echo <addr> <pubkey-hex> <text>
      Send <text> to an echo server and print the reply.
      Example: xenon echo 127.0.0.1:9000 <pubkey-hex> hello
  recv <addr> <secret-hex> <out-file>
      Receive a file and write it to <out-file>.
      Example: xenon recv 127.0.0.1:9001 <secret-hex> out.bin
  send <addr> <pubkey-hex> <file>
      Send <file> to a receiver at <addr>.
      Example: xenon send 127.0.0.1:9001 <pubkey-hex> in.bin
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

const ADDR_KEY: &[Arg] = &[req("ADDR", ArgKind::Text), req("SECRET", ArgKind::Text)];
const KEYGEN_ARGS: &[Arg] = &[];
const ECHO_ARGS: &[Arg] = &[
    req("ADDR", ArgKind::Text),
    req("PUBKEY", ArgKind::Text),
    req("TEXT", ArgKind::Text),
];
const RECV_ARGS: &[Arg] = &[
    req("ADDR", ArgKind::Text),
    req("SECRET", ArgKind::Text),
    req("OUT", ArgKind::Path),
];
const SEND_ARGS: &[Arg] = &[
    req("ADDR", ArgKind::Text),
    req("PUBKEY", ArgKind::Text),
    req("FILE", ArgKind::Path),
];

const COMMANDS: &[Command] = &[
    Command {
        name: "keygen",
        summary: "Make an X25519 key pair; prints a secret and a public key",
        effect: Effect::Read,
        args: KEYGEN_ARGS,
        returns: "secret_key, public_key (lowercase hex)",
        example: "xenon --json keygen",
    },
    Command {
        name: "echo-server",
        summary: "Run a UDP echo server on ADDR with SECRET; prints one envelope once bound, then runs",
        effect: Effect::Net,
        args: ADDR_KEY,
        returns: "listen (the bound address), printed once the socket is bound",
        example: "xenon --json echo-server 127.0.0.1:0 0101010101010101010101010101010101010101010101010101010101010101",
    },
    Command {
        name: "echo",
        summary: "Send TEXT to an echo server at ADDR with PUBKEY and return the reply",
        effect: Effect::Net,
        args: ECHO_ARGS,
        returns: "text (the reply, capped, with truncated, bytes, lossy when they apply)",
        example: "xenon --json echo 127.0.0.1:9000 0101010101010101010101010101010101010101010101010101010101010101 hello",
    },
    Command {
        name: "recv",
        summary: "Receive one encrypted file on ADDR with SECRET and write it to OUT",
        effect: Effect::Net,
        args: RECV_ARGS,
        returns: "path (the written file), bytes, hash (sha256, lowercase hex), duration_ms",
        example: "xenon --json recv 127.0.0.1:9001 0101010101010101010101010101010101010101010101010101010101010101 out.bin",
    },
    Command {
        name: "send",
        summary: "Send FILE encrypted to a receiver at ADDR with PUBKEY",
        effect: Effect::Net,
        args: SEND_ARGS,
        returns: "path (the sent file), bytes, hash (sha256, lowercase hex), duration_ms",
        example: "xenon --json send 127.0.0.1:9001 0101010101010101010101010101010101010101010101010101010101010101 in.bin",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "xenon",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Encrypted UDP transport: echo and file transfer",
    commands: COMMANDS,
};

enum CliError {
    Usage(String),
    Runtime(Code, String),
}

impl CliError {
    fn usage(msg: impl Into<String>) -> Self {
        Self::Usage(msg.into())
    }

    fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(Code::Io, msg.into())
    }

    fn invalid(msg: impl Into<String>) -> Self {
        Self::Runtime(Code::Invalid, msg.into())
    }

    /// A file or socket error: missing files are `not_found`, everything else `io`.
    fn from_io(err: io::Error) -> Self {
        let code = if err.kind() == io::ErrorKind::NotFound {
            Code::NotFound
        } else {
            Code::Io
        };
        Self::Runtime(code, err.to_string())
    }

    fn code(&self) -> Code {
        match self {
            Self::Usage(_) => Code::Usage,
            Self::Runtime(code, _) => *code,
        }
    }

    fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Runtime(..) => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(msg) | Self::Runtime(_, msg) => f.write_str(msg),
        }
    }
}

impl<E: std::error::Error> From<E> for CliError {
    fn from(err: E) -> Self {
        Self::runtime(err.to_string())
    }
}

/// What one command produced: the text line(s) for text mode and the data for `--json`.
/// `data` is None when the command already printed its envelope (echo-server).
struct Report {
    text: String,
    data: Option<Json>,
}

fn report(text: String, data: Json) -> Report {
    Report {
        text,
        data: Some(data),
    }
}

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if args.len() == 2 && args[1] == "--describe" {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    let json = args.get(1).map(String::as_str) == Some("--json");
    if json {
        args.remove(1);
    }
    let cmd = known_command(args.get(1));
    match dispatch(&args, json) {
        Ok(out) => {
            if !json {
                print!("{}", out.text);
            } else if let Some(data) = out.data {
                print!("{}", envelope::ok("xenon", cmd, data));
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("xenon: {err}");
            if json {
                let hint = matches!(err, CliError::Usage(_)).then_some("run: xenon --describe");
                print!(
                    "{}",
                    envelope::err("xenon", cmd, err.code(), &err.to_string(), hint)
                );
            }
            ExitCode::from(err.exit_code())
        }
    }
}

/// The subcommand name for the envelope, or the empty string when the name is not one of ours.
fn known_command(arg: Option<&String>) -> &'static str {
    const NAMES: [&str; 5] = ["keygen", "echo-server", "echo", "recv", "send"];
    arg.and_then(|a| NAMES.iter().copied().find(|n| *n == a.as_str()))
        .unwrap_or("")
}

fn dispatch(args: &[String], json: bool) -> Result<Report, CliError> {
    let Some(cmd) = args.get(1).map(String::as_str) else {
        return Err(CliError::usage(
            "missing subcommand; usage: xenon <keygen|echo-server|echo|recv|send> ...",
        ));
    };

    if matches!(cmd, "--help" | "-h") {
        if args.len() != 2 {
            return Err(CliError::usage(format!("unexpected arguments after {cmd}")));
        }
        return Ok(report(
            HELP.to_string(),
            Json::obj([("usage", Json::Str(HELP.into()))]),
        ));
    }

    if cmd == "--version" {
        if args.len() != 2 {
            return Err(CliError::usage("unexpected arguments after --version"));
        }
        return Ok(report(
            format!("{VERSION}\n"),
            Json::obj([("version", Json::Str(VERSION.into()))]),
        ));
    }

    if cmd.starts_with('-') {
        return Err(CliError::usage(format!(
            "unknown flag '{cmd}' (try 'xenon --help')"
        )));
    }

    if matches!(cmd, "keygen" | "echo-server" | "echo" | "recv" | "send")
        && args.len() == 3
        && matches!(args.get(2).map(String::as_str), Some("--help" | "-h"))
    {
        return Ok(report(
            HELP.to_string(),
            Json::obj([("usage", Json::Str(HELP.into()))]),
        ));
    }

    match cmd {
        "keygen" => {
            if args.len() != 2 {
                return Err(CliError::usage("usage: xenon keygen"));
            }
            let secret = make_seed()?;
            let pubkey = x25519::public_key(&secret);
            let (secret_hex, public_hex) = (to_hex(&secret), to_hex(&pubkey));
            let text = format!("secret: {secret_hex}\npublic: {public_hex}\n");
            Ok(report(
                text,
                Json::obj([
                    ("secret_key", Json::Str(secret_hex)),
                    ("public_key", Json::Str(public_hex)),
                ]),
            ))
        }
        "echo-server" => {
            if args.len() != 4 {
                return Err(CliError::usage(
                    "usage: xenon echo-server <addr> <secret-hex>",
                ));
            }
            let (addr, secret) =
                parse_addr_key(args.get(2), args.get(3), "echo-server <addr> <secret-hex>")?;
            let socket = UdpSocket::bind(addr).map_err(CliError::from_io)?;
            if json {
                let data = Json::obj([("listen", Json::Str(socket.local_addr()?.to_string()))]);
                print!("{}", envelope::ok("xenon", "echo-server", data));
                io::stdout().flush()?;
            }
            let stop = AtomicBool::new(false);
            echo_server(
                socket,
                secret,
                make_seed()?,
                Config::default(),
                Lossy::default(),
                &stop,
            )?;
            Ok(Report {
                text: String::new(),
                data: None,
            })
        }
        "echo" => {
            if args.len() != 5 {
                return Err(CliError::usage(
                    "usage: xenon echo <addr> <pubkey-hex> <text>",
                ));
            }
            let (addr, pubkey) =
                parse_addr_key(args.get(2), args.get(3), "echo <addr> <pubkey-hex> <text>")?;
            let text = args
                .get(4)
                .ok_or_else(|| CliError::usage("usage: xenon echo <addr> <pubkey-hex> <text>"))?;
            let socket = bind_client(addr)?;
            let reply = echo(
                socket,
                addr,
                pubkey,
                make_seed()?,
                Config::default(),
                Lossy::default(),
                text.as_bytes(),
                Duration::from_secs(10),
            )?;
            let line = format!("{}\n", String::from_utf8_lossy(&reply));
            Ok(Report {
                text: line,
                data: Some(Json::Obj(envelope::capped(&reply))),
            })
        }
        "recv" => {
            if args.len() != 5 {
                return Err(CliError::usage(
                    "usage: xenon recv <addr> <secret-hex> <out-file>",
                ));
            }
            let (addr, secret) = parse_addr_key(
                args.get(2),
                args.get(3),
                "recv <addr> <secret-hex> <out-file>",
            )?;
            let path = args.get(4).ok_or_else(|| {
                CliError::usage("usage: xenon recv <addr> <secret-hex> <out-file>")
            })?;
            let socket = UdpSocket::bind(addr).map_err(CliError::from_io)?;
            let mut out = File::create(path).map_err(CliError::from_io)?;
            let started = Instant::now();
            let (len, hash) = recv_file(
                socket,
                secret,
                make_seed()?,
                Config::default(),
                Lossy::default(),
                &mut out,
                Duration::from_secs(60),
            )?;
            let hex = to_hex(&hash);
            let text = format!("received {len} bytes sha256={hex}\n");
            Ok(report(
                text,
                Json::obj([
                    ("path", Json::path(Path::new(path))),
                    ("bytes", int(len)),
                    ("hash", Json::Str(hex)),
                    ("duration_ms", int(started.elapsed().as_millis())),
                ]),
            ))
        }
        "send" => {
            if args.len() != 5 {
                return Err(CliError::usage(
                    "usage: xenon send <addr> <pubkey-hex> <file>",
                ));
            }
            let (addr, pubkey) =
                parse_addr_key(args.get(2), args.get(3), "send <addr> <pubkey-hex> <file>")?;
            let path = args
                .get(4)
                .ok_or_else(|| CliError::usage("usage: xenon send <addr> <pubkey-hex> <file>"))?;
            let mut input = File::open(path).map_err(CliError::from_io)?;
            let len = input.metadata().map_err(CliError::from_io)?.len();
            let socket = bind_client(addr)?;
            let started = Instant::now();
            let hash = send_file(
                socket,
                addr,
                pubkey,
                make_seed()?,
                Config::default(),
                Lossy::default(),
                &mut input,
                len,
                Duration::from_secs(60),
            )?;
            let hex = to_hex(&hash);
            let text = format!("sent {len} bytes sha256={hex}\n");
            Ok(report(
                text,
                Json::obj([
                    ("path", Json::path(Path::new(path))),
                    ("bytes", int(len)),
                    ("hash", Json::Str(hex)),
                    ("duration_ms", int(started.elapsed().as_millis())),
                ]),
            ))
        }
        _ => Err(CliError::usage(format!(
            "unknown subcommand '{cmd}'; usage: xenon <keygen|echo-server|echo|recv|send> ..."
        ))),
    }
}

fn int<T: TryInto<i64>>(n: T) -> Json {
    Json::Int(n.try_into().unwrap_or(i64::MAX))
}

fn bind_client(server: SocketAddr) -> Result<UdpSocket, CliError> {
    let bind_addr = if server.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    UdpSocket::bind(bind_addr).map_err(CliError::from_io)
}

fn parse_addr_key(
    addr_arg: Option<&String>,
    key_arg: Option<&String>,
    usage: &str,
) -> Result<(SocketAddr, [u8; 32]), CliError> {
    let addr_str = addr_arg.ok_or_else(|| CliError::usage(format!("usage: xenon {usage}")))?;
    let key_str = key_arg.ok_or_else(|| CliError::usage(format!("usage: xenon {usage}")))?;
    if addr_str.starts_with('-') {
        return Err(CliError::usage(format!("unexpected flag '{addr_str}'")));
    }
    if key_str.starts_with('-') {
        return Err(CliError::usage(format!("unexpected flag '{key_str}'")));
    }
    let addr: SocketAddr = addr_str
        .parse()
        .map_err(|e: std::net::AddrParseError| CliError::invalid(e.to_string()))?;
    let key = parse_hex32(key_str)?;
    Ok((addr, key))
}

/// Fresh 32-byte secret from the OS CSPRNG; fails closed when there is none.
fn make_seed() -> Result<[u8; 32], CliError> {
    let mut seed = [0u8; 32];
    entropy::fill(&mut seed).map_err(|e| CliError::runtime(e.to_string()))?;
    Ok(seed)
}

fn parse_hex32(s: &str) -> Result<[u8; 32], CliError> {
    let bytes = s.as_bytes();
    if bytes.len() != 64 {
        return Err(CliError::invalid("expected 64 hex characters"));
    }
    let mut out = [0u8; 32];
    for (dst, pair) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
        let hi = hex_nibble(pair[0]).ok_or_else(|| CliError::invalid("invalid hex character"))?;
        let lo = hex_nibble(pair[1]).ok_or_else(|| CliError::invalid("invalid hex character"))?;
        *dst = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}
