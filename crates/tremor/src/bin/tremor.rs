//! CLI wrapper for Tremor UDP echo and file transfer.
#![forbid(unsafe_code)]

use onyx::sha256::Sha256;
use onyx::x25519;
use std::env;
use std::fs::File;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tremor::{echo, echo_server, recv_file, send_file, Config, Lossy};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");
    match cmd {
        "keygen" => {
            if args.len() != 2 {
                return Err("usage: tremor keygen".into());
            }
            let secret = make_seed(b"keygen");
            let pubkey = x25519::public_key(&secret);
            println!("secret: {}", to_hex(&secret));
            println!("public: {}", to_hex(&pubkey));
        }
        "echo-server" => {
            let (addr, secret) =
                parse_addr_key(args.get(2), args.get(3), "echo-server <addr> <secret-hex>")?;
            if args.len() != 4 {
                return Err("usage: tremor echo-server <addr> <secret-hex>".into());
            }
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
                return Err("usage: tremor echo <addr> <pubkey-hex> <text>".into());
            }
            let (addr, pubkey) =
                parse_addr_key(args.get(2), args.get(3), "echo <addr> <pubkey-hex> <text>")?;
            let text = args.get(4).ok_or("missing text")?;
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
                return Err("usage: tremor recv <addr> <secret-hex> <out-file>".into());
            }
            let (addr, secret) = parse_addr_key(
                args.get(2),
                args.get(3),
                "recv <addr> <secret-hex> <out-file>",
            )?;
            let path = args.get(4).ok_or("missing out-file")?;
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
                return Err("usage: tremor send <addr> <pubkey-hex> <file>".into());
            }
            let (addr, pubkey) =
                parse_addr_key(args.get(2), args.get(3), "send <addr> <pubkey-hex> <file>")?;
            let path = args.get(4).ok_or("missing file")?;
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
            return Err("usage: tremor <keygen|echo-server|echo|recv|send> ...".into());
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
) -> Result<(SocketAddr, [u8; 32]), Box<dyn std::error::Error>> {
    let addr_str = addr_arg.ok_or_else(|| format!("usage: tremor {usage}"))?;
    let key_str = key_arg.ok_or_else(|| format!("usage: tremor {usage}"))?;
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

fn parse_hex32(s: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let bytes = s.as_bytes();
    if bytes.len() != 64 {
        return Err("expected 64 hex characters".into());
    }
    let mut out = [0u8; 32];
    for (i, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        let hi = hex_nibble(pair[0]).ok_or("invalid hex character")?;
        let lo = hex_nibble(pair[1]).ok_or("invalid hex character")?;
        out[i] = (hi << 4) | lo;
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
