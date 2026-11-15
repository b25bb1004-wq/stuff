//! Command-line interface for sphinx cryptographic primitives.
#![forbid(unsafe_code)]

use sphinx::aead;
use sphinx::ed25519;
use sphinx::hmac::hmac_sha256;
use sphinx::sha256::Sha256;
use sphinx::sha512::Sha512;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::process::ExitCode;

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

enum CliError {
    Op(String),
    Usage(String),
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
        return Err(CliError::Op(String::from("hex string has odd length")));
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
    let v = decode_hex_vec(s).map_err(|_| CliError::Op(format!("invalid {label} hex")))?;
    let len = v.len();
    v.try_into().map_err(|_| {
        CliError::Op(format!(
            "invalid {label} length: expected {N} bytes, got {len}"
        ))
    })
}

fn hex_nibble(b: u8) -> Result<u8, CliError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(CliError::Op(String::from("invalid hex character"))),
    }
}

fn read_input(path: &str) -> Result<Vec<u8>, CliError> {
    let mut buf = Vec::new();
    if path == "-" {
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| CliError::Op(format!("failed to read stdin: {e}")))?;
    } else {
        let mut f =
            File::open(path).map_err(|e| CliError::Op(format!("cannot open '{path}': {e}")))?;
        f.read_to_end(&mut buf)
            .map_err(|e| CliError::Op(format!("failed to read '{path}': {e}")))?;
    }
    Ok(buf)
}

fn read_key32(path: &str) -> Result<[u8; 32], CliError> {
    let data = read_input(path)?;
    let len = data.len();
    data.try_into()
        .map_err(|_| CliError::Op(format!("key file '{path}' must be 32 bytes, got {len}")))
}

fn write_output(out_path: Option<&str>, data: &[u8]) -> Result<(), CliError> {
    match out_path {
        None | Some("-") => io::stdout()
            .write_all(data)
            .map_err(|e| CliError::Op(format!("failed to write stdout: {e}"))),
        Some(path) => {
            fs::write(path, data).map_err(|e| CliError::Op(format!("cannot write '{path}': {e}")))
        }
    }
}

fn fill_random(buf: &mut [u8]) -> Result<(), CliError> {
    entropy::fill(buf).map_err(|e| CliError::Op(e.to_string()))
}

fn write_secret_key32(path: &str, key: &[u8; 32]) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut f = opts
            .open(path)
            .map_err(|e| CliError::Op(format!("cannot write '{path}': {e}")))?;
        f.write_all(key)
            .map_err(|e| CliError::Op(format!("failed to write '{path}': {e}")))?;
        f.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| CliError::Op(format!("cannot set permissions on '{path}': {e}")))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        let mut f = opts
            .open(path)
            .map_err(|e| CliError::Op(format!("cannot write '{path}': {e}")))?;
        f.write_all(key)
            .map_err(|e| CliError::Op(format!("failed to write '{path}': {e}")))?;
        Ok(())
    }
}

fn cmd_hash(args: &[String]) -> Result<(), CliError> {
    let [alg, path] = args else {
        return Err(CliError::Usage(String::from("usage: sphinx hash ALG FILE")));
    };
    let hex = match alg.as_str() {
        "sha256" => {
            let mut h = Sha256::new();
            hash_reader(path, |chunk| h.update(chunk))?;
            to_hex(&h.finalize())
        }
        "sha512" => {
            let mut h = Sha512::new();
            hash_reader(path, |chunk| h.update(chunk))?;
            to_hex(&h.finalize())
        }
        other => {
            return Err(CliError::Op(format!(
                "unsupported hash algorithm '{other}' (expected sha256 or sha512)"
            )));
        }
    };
    println!("{hex}");
    Ok(())
}

fn hash_reader<F: FnMut(&[u8])>(path: &str, mut update: F) -> Result<(), CliError> {
    let mut buf = [0u8; 8192];
    if path == "-" {
        let mut stdin = io::stdin().lock();
        loop {
            let n = stdin
                .read(&mut buf)
                .map_err(|e| CliError::Op(format!("failed to read stdin: {e}")))?;
            if n == 0 {
                break;
            }
            update(&buf[..n]);
        }
    } else {
        let mut f =
            File::open(path).map_err(|e| CliError::Op(format!("cannot open '{path}': {e}")))?;
        loop {
            let n = f
                .read(&mut buf)
                .map_err(|e| CliError::Op(format!("failed to read '{path}': {e}")))?;
            if n == 0 {
                break;
            }
            update(&buf[..n]);
        }
    }
    Ok(())
}

fn cmd_hmac(args: &[String]) -> Result<(), CliError> {
    let [key_hex, path] = args else {
        return Err(CliError::Usage(String::from(
            "usage: sphinx hmac KEYHEX FILE",
        )));
    };
    let key = decode_hex_vec(key_hex).map_err(|_| CliError::Op(String::from("invalid key hex")))?;
    let data = read_input(path)?;
    let tag = hmac_sha256(&key, &data);
    println!("{}", to_hex(&tag));
    Ok(())
}

fn cmd_keygen(args: &[String]) -> Result<(), CliError> {
    let [flag, path] = args else {
        return Err(CliError::Usage(String::from(
            "usage: sphinx keygen -o KEYFILE",
        )));
    };
    if flag != "-o" {
        return Err(CliError::Usage(String::from(
            "usage: sphinx keygen -o KEYFILE",
        )));
    }
    let mut seed = [0u8; 32];
    fill_random(&mut seed)?;
    if path == "-" {
        io::stdout()
            .write_all(&seed)
            .map_err(|e| CliError::Op(format!("failed to write stdout: {e}")))?;
        return Ok(());
    }
    write_secret_key32(path, &seed)?;
    let pub_key = ed25519::public_key(&seed);
    println!("{}", to_hex(&pub_key));
    Ok(())
}

fn cmd_sign(args: &[String]) -> Result<(), CliError> {
    let [key_path, msg_path] = args else {
        return Err(CliError::Usage(String::from(
            "usage: sphinx sign KEYFILE FILE",
        )));
    };
    let seed = read_key32(key_path)?;
    let msg = read_input(msg_path)?;
    let sig = ed25519::sign(&seed, &msg);
    println!("{}", to_hex(&sig));
    Ok(())
}

fn cmd_verify(args: &[String]) -> Result<(), CliError> {
    let [pub_hex, sig_hex, msg_path] = args else {
        return Err(CliError::Usage(String::from(
            "usage: sphinx verify PUBHEX SIGHEX FILE",
        )));
    };
    let pub_key: [u8; 32] = decode_hex_array(pub_hex, "public key")?;
    let sig: [u8; 64] = decode_hex_array(sig_hex, "signature")?;
    let msg = read_input(msg_path)?;
    match ed25519::verify(&pub_key, &msg, &sig) {
        Ok(()) => {
            println!("ok");
            Ok(())
        }
        Err(_) => Err(CliError::Op(String::from("verification failed"))),
    }
}

fn parse_seal_open_args<'a>(
    args: &'a [String],
    verb: &str,
) -> Result<(&'a str, &'a str, Option<&'a str>), CliError> {
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

fn cmd_seal(args: &[String]) -> Result<(), CliError> {
    let (key_path, in_path, out_path) = parse_seal_open_args(args, "seal")?;
    let key = read_key32(key_path)?;
    let plaintext = read_input(in_path)?;
    let mut nonce = [0u8; 12];
    fill_random(&mut nonce)?;
    let body = aead::seal(&key, &nonce, b"", &plaintext);
    let mut out = Vec::with_capacity(nonce.len() + body.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    write_output(out_path, &out)
}

fn cmd_open(args: &[String]) -> Result<(), CliError> {
    let (key_path, in_path, out_path) = parse_seal_open_args(args, "open")?;
    let key = read_key32(key_path)?;
    let sealed = read_input(in_path)?;
    let Some((nonce, body)) = sealed.split_first_chunk::<12>() else {
        return Err(CliError::Op(String::from("sealed input is too short")));
    };
    let plaintext = aead::open(&key, nonce, b"", body)
        .map_err(|_| CliError::Op(String::from("decryption failed")))?;
    write_output(out_path, &plaintext)
}

fn run() -> Result<(), CliError> {
    let args: Vec<String> = env::args().collect();
    let Some(cmd) = args.get(1).map(String::as_str) else {
        return Err(CliError::Usage(String::from(
            "missing subcommand (try --help)",
        )));
    };
    let rest = args.get(2..).unwrap_or(&[]);
    match cmd {
        "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        "--version" | "-V" => {
            println!("{VERSION}");
            Ok(())
        }
        "hash" => cmd_hash(rest),
        "hmac" => cmd_hmac(rest),
        "keygen" => cmd_keygen(rest),
        "sign" => cmd_sign(rest),
        "verify" => cmd_verify(rest),
        "seal" => cmd_seal(rest),
        "open" => cmd_open(rest),
        other => Err(CliError::Usage(format!(
            "unknown subcommand '{other}' (try --help)"
        ))),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Op(msg)) => {
            eprintln!("sphinx: {msg}");
            ExitCode::from(1)
        }
        Err(CliError::Usage(msg)) => {
            eprintln!("sphinx: {msg}");
            ExitCode::from(2)
        }
    }
}
