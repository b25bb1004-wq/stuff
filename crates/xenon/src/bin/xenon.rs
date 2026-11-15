//! CLI wrapper for Xenon UDP echo and file transfer.
#![forbid(unsafe_code)]

use sphinx::sha256::Sha256;
use sphinx::x25519;
use std::env;
use std::fmt;
use std::fs::File;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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

enum CliError {
    Usage(String),
    Runtime(String),
}

impl CliError {
    fn usage(msg: impl Into<String>) -> Self {
        Self::Usage(msg.into())
    }

    fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }

    fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Runtime(_) => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(msg) | Self::Runtime(msg) => f.write_str(msg),
        }
    }
}

impl<E: std::error::Error> From<E> for CliError {
    fn from(err: E) -> Self {
        Self::Runtime(err.to_string())
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("xenon: {err}");
        std::process::exit(err.exit_code());
    }
}

fn run() -> Result<(), CliError> {
    let args: Vec<String> = env::args().collect();
    let Some(cmd) = args.get(1).map(String::as_str) else {
        return Err(CliError::usage(
            "missing subcommand; usage: xenon <keygen|echo-server|echo|recv|send> ...",
        ));
    };

    if matches!(cmd, "--help" | "-h") {
        if args.len() != 2 {
            return Err(CliError::usage(format!("unexpected arguments after {cmd}")));
        }
        print!("{HELP}");
        return Ok(());
    }

    if cmd == "--version" {
        if args.len() != 2 {
            return Err(CliError::usage("unexpected arguments after --version"));
        }
        println!("{VERSION}");
        return Ok(());
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
        print!("{HELP}");
        return Ok(());
    }

    match cmd {
        "keygen" => {
            if args.len() != 2 {
                return Err(CliError::usage("usage: xenon keygen"));
            }
            let secret = make_seed(b"keygen");
            let pubkey = x25519::public_key(&secret);
            println!("secret: {}", to_hex(&secret));
            println!("public: {}", to_hex(&pubkey));
        }
        "echo-server" => {
            if args.len() != 4 {
                return Err(CliError::usage(
                    "usage: xenon echo-server <addr> <secret-hex>",
                ));
            }
            let (addr, secret) =
                parse_addr_key(args.get(2), args.get(3), "echo-server <addr> <secret-hex>")?;
            let socket = UdpSocket::bind(addr)?;
            let stop = AtomicBool::new(false);
            echo_server(
                socket,
                secret,
                make_seed(b"echo-server"),
                Config::default(),
                Lossy::default(),
                &stop,
            )?;
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
                make_seed(b"echo"),
                Config::default(),
                Lossy::default(),
                text.as_bytes(),
                Duration::from_secs(10),
            )?;
            println!("{}", String::from_utf8_lossy(&reply));
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
            let socket = UdpSocket::bind(addr)?;
            let mut out = File::create(path)?;
            let (len, hash) = recv_file(
                socket,
                secret,
                make_seed(b"recv"),
                Config::default(),
                Lossy::default(),
                &mut out,
                Duration::from_secs(60),
            )?;
            println!("received {len} bytes sha256={}", to_hex(&hash));
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
            let mut input = File::open(path)?;
            let len = input.metadata()?.len();
            let socket = bind_client(addr)?;
            let hash = send_file(
                socket,
                addr,
                pubkey,
                make_seed(b"send"),
                Config::default(),
                Lossy::default(),
                &mut input,
                len,
                Duration::from_secs(60),
            )?;
            println!("sent {len} bytes sha256={}", to_hex(&hash));
        }
        _ => {
            return Err(CliError::usage(format!(
                "unknown subcommand '{cmd}'; usage: xenon <keygen|echo-server|echo|recv|send> ..."
            )));
        }
    }
    Ok(())
}

fn bind_client(server: SocketAddr) -> std::io::Result<UdpSocket> {
    let bind_addr = if server.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    UdpSocket::bind(bind_addr)
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
    let addr: SocketAddr = addr_str.parse()?;
    let key = parse_hex32(key_str)?;
    Ok((addr, key))
}

fn make_seed(tag: &[u8]) -> [u8; 32] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let pid = std::process::id();
    let mut h = Sha256::new();
    h.update(tag);
    h.update(&nanos.to_le_bytes());
    h.update(&pid.to_le_bytes());
    h.finalize()
}

fn parse_hex32(s: &str) -> Result<[u8; 32], CliError> {
    let bytes = s.as_bytes();
    if bytes.len() != 64 {
        return Err(CliError::runtime("expected 64 hex characters"));
    }
    let mut out = [0u8; 32];
    for (dst, pair) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
        let hi = hex_nibble(pair[0]).ok_or_else(|| CliError::runtime("invalid hex character"))?;
        let lo = hex_nibble(pair[1]).ok_or_else(|| CliError::runtime("invalid hex character"))?;
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
