//! aurum: command-line wallet, Merkle proof, and chain verification tool.
//!
//! Text mode is unchanged. `--describe` prints the manifest and `--json` prints one ACI envelope
//! (docs/ai-computer/ACI.md). Each command computes its values once: text mode prints them and
//! `--json` returns them as `data`.
#![forbid(unsafe_code)]

use aci::envelope::{self, Code, DEFAULT_LIMIT};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
use aurum::{
    merkle_proof, merkle_root, Address, Block, BlockError, Chain, DecodeError, Tx, TxError,
};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

const CHAIN_MAGIC: &[u8; 8] = b"AURUMCH1";
const MAX_CHAIN_ENTRIES: usize = 65_536;
const VERSION: &str = "aurum 0.1.0";

const HELP: &str = "\
aurum 0.1.0 - Monolith chain, Merkle, and transfer CLI

USAGE:
  aurum <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
  keygen -o KEYFILE
      Generate an ed25519 keyfile (hex seed) and print its address.
      Example: aurum keygen -o alice.key
  tx KEYFILE TO AMOUNT NONCE -o TX
      Sign a transfer in library encoding and write it as a hex file.
      Example: aurum tx alice.key 0102... 100 0 -o transfer.tx
  merkle [--proof N] FILE...
      Compute the RFC 6962 SHA-256 Merkle root (and optional proof for leaf N).
      Example: aurum merkle --proof 0 a.bin b.bin
  verify CHAIN
      Validate every block of CHAIN in library encoding.
      Example: aurum verify demo.chain
  demo -o CHAIN
      Build a deterministic 3-block demo chain for verification.
      Example: aurum demo -o demo.chain

FLAGS:
  -h, --help       Print this help message
  -V, --version    Print version information
";

const KEYGEN_ARGS: &[Arg] = &[Arg {
    name: "-o",
    kind: ArgKind::Path,
    required: true,
    default: None,
    choices: &[],
}];
const TX_ARGS: &[Arg] = &[
    Arg {
        name: "KEYFILE",
        kind: ArgKind::Path,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "TO",
        kind: ArgKind::Text,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "AMOUNT",
        kind: ArgKind::Int,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "NONCE",
        kind: ArgKind::Int,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "-o",
        kind: ArgKind::Path,
        required: true,
        default: None,
        choices: &[],
    },
];
const MERKLE_ARGS: &[Arg] = &[
    Arg {
        name: "--proof",
        kind: ArgKind::Int,
        required: false,
        default: None,
        choices: &[],
    },
    Arg {
        name: "FILE",
        kind: ArgKind::Path,
        required: true,
        default: None,
        choices: &[],
    },
];
const CHAIN_ARGS: &[Arg] = &[Arg {
    name: "CHAIN",
    kind: ArgKind::Path,
    required: true,
    default: None,
    choices: &[],
}];

const COMMANDS: &[Command] = &[
    Command {
        name: "keygen",
        summary: "Generate an ed25519 keyfile at -o FILE and report its address (--json needs a file, not -)",
        effect: Effect::Write,
        args: KEYGEN_ARGS,
        returns: "path, address (lowercase hex)",
        example: "aurum --json keygen -o alice.key",
    },
    Command {
        name: "tx",
        summary: "Sign a transfer from KEYFILE to address TO and write it as hex to -o FILE",
        effect: Effect::Write,
        args: TX_ARGS,
        returns: "path, hash (transfer id, lowercase hex), bytes (encoded size)",
        example: "aurum --json tx alice.key <address> 100 0 -o transfer.tx",
    },
    Command {
        name: "merkle",
        summary: "RFC 6962 SHA-256 Merkle root of FILE...; --proof N adds the index and proof for leaf N",
        effect: Effect::Read,
        args: MERKLE_ARGS,
        returns: "root, count; with --proof: index, proof (a page of lowercase hex siblings)",
        example: "aurum --json merkle --proof 0 a.bin b.bin",
    },
    Command {
        name: "verify",
        summary: "Validate every block of CHAIN in library encoding",
        effect: Effect::Read,
        args: CHAIN_ARGS,
        returns: "height, blocks, hash (tip block, lowercase hex), root (state root, lowercase hex)",
        example: "aurum --json verify demo.chain",
    },
    Command {
        name: "demo",
        summary: "Build a deterministic 3-block demo chain at -o FILE (--json needs a file, not -)",
        effect: Effect::Write,
        args: KEYGEN_ARGS,
        returns: "path, bytes, height, blocks, hash, root",
        example: "aurum --json demo -o demo.chain",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "aurum",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Monolith chain, Merkle, and transfer CLI",
    commands: COMMANDS,
};

/// CLI error carrying its exit code (`1` = operation failed, `2` = usage error) and its `--json` code.
struct CliError {
    code: u8,
    message: String,
    kind: Code,
}

impl CliError {
    fn fail(message: impl Into<String>) -> Self {
        Self {
            code: 1,
            message: message.into(),
            kind: Code::Invalid,
        }
    }

    fn io(message: impl Into<String>, err: &io::Error) -> Self {
        let kind = if err.kind() == io::ErrorKind::NotFound {
            Code::NotFound
        } else {
            Code::Io
        };
        Self {
            code: 1,
            message: message.into(),
            kind,
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
            kind: Code::Usage,
        }
    }
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
        return run_json(&args);
    }
    match run(&args, false) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("aurum: {}", err.message);
            ExitCode::from(err.code)
        }
    }
}

/// The subcommand name for the envelope, or the empty string when the name is not one of ours.
fn known_command(first: Option<&String>) -> &'static str {
    const NAMES: [&str; 9] = [
        "keygen",
        "tx",
        "merkle",
        "verify",
        "demo",
        "--help",
        "-h",
        "--version",
        "-V",
    ];
    first
        .and_then(|s| NAMES.iter().copied().find(|n| *n == s.as_str()))
        .unwrap_or("")
}

/// `aurum --json ...`: one envelope on stdout; stderr keeps the `aurum: message` line.
fn run_json(args: &[String]) -> ExitCode {
    let cmd = known_command(args.first());
    match run(args, true) {
        Ok(data) => {
            print!("{}", envelope::ok("aurum", cmd, data));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("aurum: {}", err.message);
            let hint = if err.kind == Code::Usage {
                Some("run: aurum --describe")
            } else {
                None
            };
            print!(
                "{}",
                envelope::err("aurum", cmd, err.kind, &err.message, hint)
            );
            ExitCode::from(err.code)
        }
    }
}

/// Dispatch the top-level CLI command. Text mode prints and returns unused data; `json` returns
/// the data and prints nothing.
fn run(args: &[String], json: bool) -> Result<Json, CliError> {
    let Some(first) = args.first() else {
        return Err(CliError::usage("missing subcommand (try --help)"));
    };
    match first.as_str() {
        "--help" | "-h" => {
            if !json {
                print!("{HELP}");
            }
            Ok(Json::obj([("usage", Json::Str(HELP.into()))]))
        }
        "--version" | "-V" => {
            if !json {
                println!("{VERSION}");
            }
            Ok(Json::obj([("version", Json::Str(VERSION.into()))]))
        }
        "keygen" => cmd_keygen(&args[1..], json),
        "tx" => cmd_tx(&args[1..], json),
        "merkle" => cmd_merkle(&args[1..], json),
        "verify" => cmd_verify(&args[1..], json),
        "demo" => cmd_demo(&args[1..], json),
        other => Err(CliError::usage(format!("unknown subcommand '{other}'"))),
    }
}

/// Under `--json` the output must be the envelope on stdout, so `-o -` is refused.
fn refuse_stdout_under_json(path: &str, json: bool) -> Result<(), CliError> {
    if json && path == "-" {
        Err(CliError::usage("--json needs a file: -o FILE, not -"))
    } else {
        Ok(())
    }
}

/// Handle `aurum keygen -o KEYFILE`.
fn cmd_keygen(args: &[String], json: bool) -> Result<Json, CliError> {
    let out_path = parse_single_output_flag(args, "keygen -o KEYFILE")?;
    refuse_stdout_under_json(&out_path, json)?;
    let seed = generate_seed()?;
    let addr = derive_address(&seed);
    write_output(&out_path, format!("{}\n", to_hex(&seed)).as_bytes())?;
    if !json && out_path != "-" {
        println!("{}", to_hex(&addr));
    }
    Ok(Json::obj([
        ("path", Json::path(Path::new(&out_path))),
        ("address", Json::Str(to_hex(&addr))),
    ]))
}

/// Handle `aurum tx KEYFILE TO AMOUNT NONCE -o TX`.
fn cmd_tx(args: &[String], json: bool) -> Result<Json, CliError> {
    let mut out_path: Option<String> = None;
    let mut pos: Vec<&str> = Vec::new();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "-o" {
            let val = args
                .get(i + 1)
                .ok_or_else(|| CliError::usage("missing path after -o"))?;
            if out_path.is_some() {
                return Err(CliError::usage("duplicate -o flag"));
            }
            out_path = Some(val.clone());
            i += 2;
        } else if arg.starts_with('-') && arg != "-" {
            return Err(CliError::usage(format!("unknown flag '{arg}'")));
        } else {
            pos.push(arg.as_str());
            i += 1;
        }
    }
    let out_path =
        out_path.ok_or_else(|| CliError::usage("usage: tx KEYFILE TO AMOUNT NONCE -o TX"))?;
    refuse_stdout_under_json(&out_path, json)?;
    let [key_path, to_hex_str, amount_str, nonce_str] = pos.as_slice() else {
        return Err(CliError::usage("usage: tx KEYFILE TO AMOUNT NONCE -o TX"));
    };

    let key_text = read_input_string(key_path)?;
    let seed = decode_hex_32(key_text.trim())
        .map_err(|e| CliError::fail(format!("invalid keyfile: {e}")))?;
    let to = decode_hex_32(to_hex_str.trim())
        .map_err(|e| CliError::fail(format!("invalid recipient address: {e}")))?;
    let amount: u64 = amount_str
        .parse()
        .map_err(|_| CliError::fail(format!("invalid amount '{amount_str}'")))?;
    let nonce: u64 = nonce_str
        .parse()
        .map_err(|_| CliError::fail(format!("invalid nonce '{nonce_str}'")))?;

    let tx = Tx::signed(&seed, to, amount, 0, nonce, Vec::new());
    let encoded = tx.encode();
    write_output(&out_path, format!("{}\n", to_hex(&encoded)).as_bytes())?;
    if !json && out_path != "-" {
        println!("{}", to_hex(&tx.id()));
    }
    Ok(Json::obj([
        ("path", Json::path(Path::new(&out_path))),
        ("hash", Json::Str(to_hex(&tx.id()))),
        ("bytes", int(encoded.len())),
    ]))
}

/// Handle `aurum merkle [--proof N] FILE...`.
fn cmd_merkle(args: &[String], json: bool) -> Result<Json, CliError> {
    let mut proof_index: Option<usize> = None;
    let mut files: Vec<&str> = Vec::new();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "--proof" {
            let val = args
                .get(i + 1)
                .ok_or_else(|| CliError::usage("missing index after --proof"))?;
            proof_index = Some(parse_usize_flag(val)?);
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--proof=") {
            proof_index = Some(parse_usize_flag(val)?);
            i += 1;
        } else if arg.starts_with('-') && arg != "-" {
            return Err(CliError::usage(format!("unknown flag '{arg}'")));
        } else {
            files.push(arg.as_str());
            i += 1;
        }
    }
    if files.is_empty() {
        return Err(CliError::usage("usage: merkle [--proof N] FILE..."));
    }

    let mut leaves = Vec::with_capacity(files.len());
    let mut stdin_cache: Option<Vec<u8>> = None;
    for path in files {
        let bytes = if path == "-" {
            match &stdin_cache {
                Some(cached) => cached.clone(),
                None => {
                    let data = read_stdin_bytes()?;
                    stdin_cache = Some(data.clone());
                    data
                }
            }
        } else {
            fs::read(path).map_err(|e| CliError::io(format!("cannot read '{path}': {e}"), &e))?
        };
        leaves.push(bytes);
    }

    let root = merkle_root(&leaves);
    match proof_index {
        None => {
            if !json {
                println!("{}", to_hex(&root));
            }
            Ok(Json::obj([
                ("root", Json::Str(to_hex(&root))),
                ("count", int(leaves.len())),
            ]))
        }
        Some(idx) => {
            if idx >= leaves.len() {
                return Err(CliError::fail(format!(
                    "proof index {idx} out of range (count {})",
                    leaves.len()
                )));
            }
            let siblings = merkle_proof(&leaves, idx);
            if !json {
                println!("root {}", to_hex(&root));
                println!("index {idx}");
                println!("count {}", leaves.len());
                for sibling in &siblings {
                    println!("proof {}", to_hex(sibling));
                }
            }
            let total = siblings.len();
            let items = siblings
                .iter()
                .map(|sibling| Json::Str(to_hex(sibling)))
                .collect();
            Ok(Json::obj([
                ("root", Json::Str(to_hex(&root))),
                ("index", int(idx)),
                ("count", int(leaves.len())),
                ("proof", envelope::page(items, total, 0, DEFAULT_LIMIT)),
            ]))
        }
    }
}

/// Handle `aurum demo -o CHAIN`.
fn cmd_demo(args: &[String], json: bool) -> Result<Json, CliError> {
    let out_path = parse_single_output_flag(args, "demo -o CHAIN")?;
    refuse_stdout_under_json(&out_path, json)?;
    let (alice_seed, bob_seed, carol_seed) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let (alice, bob, carol) = (
        derive_address(&alice_seed),
        derive_address(&bob_seed),
        derive_address(&carol_seed),
    );
    let genesis = vec![(alice, 1000), (bob, 500), (carol, 100)];
    let validators = vec![alice, bob];
    let mut chain = Chain::new(&genesis, &validators);
    let mut blocks = vec![chain.genesis().clone()];

    let steps = [
        (1u64, &alice_seed, bob, 100u64, 1u64, 0u64),
        (2u64, &bob_seed, carol, 50u64, 1u64, 0u64),
        (3u64, &carol_seed, alice, 25u64, 1u64, 0u64),
    ];
    for (time, sender_seed, to, amount, fee, nonce) in steps {
        let proposer = if chain.expected_proposer() == alice {
            &alice_seed
        } else {
            &bob_seed
        };
        let tx = Tx::signed(sender_seed, to, amount, fee, nonce, Vec::new());
        let block = chain
            .propose(proposer, time, vec![tx])
            .map_err(|e| CliError::fail(format!("demo propose failed: {}", block_err_str(e))))?;
        chain
            .append(block.clone())
            .map_err(|e| CliError::fail(format!("demo append failed: {}", block_err_str(e))))?;
        blocks.push(block);
    }

    let encoded = encode_chain_file(&genesis, &validators, &blocks)?;
    write_output(&out_path, &encoded)?;
    if !json && out_path != "-" {
        println!("height {}", chain.height());
        println!("blocks {}", blocks.len());
        println!("tip {}", to_hex(&chain.tip().hash()));
        println!("state_root {}", to_hex(&chain.state_root()));
    }
    Ok(Json::obj([
        ("path", Json::path(Path::new(&out_path))),
        ("bytes", int(encoded.len())),
        ("height", int(chain.height())),
        ("blocks", int(blocks.len())),
        ("hash", Json::Str(to_hex(&chain.tip().hash()))),
        ("root", Json::Str(to_hex(&chain.state_root()))),
    ]))
}

/// Handle `aurum verify CHAIN`.
fn cmd_verify(args: &[String], json: bool) -> Result<Json, CliError> {
    let [chain_path] = args else {
        return Err(CliError::usage("usage: verify CHAIN"));
    };
    if chain_path.starts_with('-') && chain_path != "-" {
        return Err(CliError::usage(format!("unknown flag '{chain_path}'")));
    }
    let bytes = read_input_bytes(chain_path)?;
    let (genesis, validators, raw_blocks) = decode_chain_file(&bytes)?;
    if validators.is_empty() {
        return Err(CliError::fail("chain has no validators"));
    }
    if raw_blocks.is_empty() {
        return Err(CliError::fail("chain has no blocks"));
    }

    let mut chain = Chain::new(&genesis, &validators);
    for (index, raw) in raw_blocks.iter().enumerate() {
        let block = Block::decode(raw).map_err(|e| {
            CliError::fail(format!(
                "block {index}: decode error: {}",
                decode_err_str(e)
            ))
        })?;
        if index == 0 {
            if block != *chain.genesis() {
                return Err(CliError::fail("block 0: genesis mismatch"));
            }
        } else {
            chain
                .append(block)
                .map_err(|e| CliError::fail(format!("block {index}: {}", block_err_str(e))))?;
        }
    }

    if !json {
        println!("ok");
        println!("height {}", chain.height());
        println!("blocks {}", raw_blocks.len());
        println!("tip {}", to_hex(&chain.tip().hash()));
        println!("state_root {}", to_hex(&chain.state_root()));
    }
    Ok(Json::obj([
        ("height", int(chain.height())),
        ("blocks", int(raw_blocks.len())),
        ("hash", Json::Str(to_hex(&chain.tip().hash()))),
        ("root", Json::Str(to_hex(&chain.state_root()))),
    ]))
}

fn int<T: TryInto<i64>>(n: T) -> Json {
    Json::Int(n.try_into().unwrap_or(i64::MAX))
}

fn parse_single_output_flag(args: &[String], usage: &str) -> Result<String, CliError> {
    match args {
        [flag, path] if flag == "-o" => Ok(path.clone()),
        _ => Err(CliError::usage(format!("usage: {usage}"))),
    }
}

fn parse_usize_flag(val: &str) -> Result<usize, CliError> {
    val.parse()
        .map_err(|_| CliError::usage(format!("invalid --proof index '{val}'")))
}

/// Key seed from the OS CSPRNG; an error instead of a weak fallback (T80 aurum keygen).
fn generate_seed() -> Result<[u8; 32], CliError> {
    let mut seed = [0u8; 32];
    entropy::fill(&mut seed).map_err(|e| CliError::fail(format!("keygen: {e}")))?;
    Ok(seed)
}

fn derive_address(seed: &[u8; 32]) -> Address {
    Tx::signed(seed, [0u8; 32], 0, 0, 0, Vec::new()).from
}

fn encode_chain_file(
    genesis: &[(Address, u64)],
    validators: &[Address],
    blocks: &[Block],
) -> Result<Vec<u8>, CliError> {
    let mut out = Vec::new();
    out.extend_from_slice(CHAIN_MAGIC);
    push_u32(&mut out, genesis.len())?;
    for (addr, balance) in genesis {
        out.extend_from_slice(addr);
        out.extend_from_slice(&balance.to_le_bytes());
    }
    push_u32(&mut out, validators.len())?;
    for addr in validators {
        out.extend_from_slice(addr);
    }
    push_u32(&mut out, blocks.len())?;
    for block in blocks {
        let raw = block.encode();
        push_u32(&mut out, raw.len())?;
        out.extend_from_slice(&raw);
    }
    Ok(out)
}

type DecodedChain<'a> = (Vec<(Address, u64)>, Vec<Address>, Vec<&'a [u8]>);

fn decode_chain_file(bytes: &[u8]) -> Result<DecodedChain<'_>, CliError> {
    let mut cur = ByteCursor(bytes);
    if cur.take(8)? != CHAIN_MAGIC {
        return Err(CliError::fail("invalid chain file header"));
    }
    let gen_count = cur.count()?;
    let mut genesis = Vec::with_capacity(gen_count);
    for _ in 0..gen_count {
        genesis.push((cur.array::<32>()?, u64::from_le_bytes(cur.array::<8>()?)));
    }
    let val_count = cur.count()?;
    let mut validators = Vec::with_capacity(val_count);
    for _ in 0..val_count {
        validators.push(cur.array::<32>()?);
    }
    let blk_count = cur.count()?;
    let mut raw_blocks = Vec::with_capacity(blk_count);
    for idx in 0..blk_count {
        let len = usize::try_from(u32::from_le_bytes(cur.array::<4>()?))
            .map_err(|_| CliError::fail("length exceeds usize"))?;
        let raw = cur
            .take(len)
            .map_err(|_| CliError::fail(format!("block {idx}: truncated block data")))?;
        raw_blocks.push(raw);
    }
    if !cur.0.is_empty() {
        return Err(CliError::fail(format!(
            "trailing bytes after block {blk_count}"
        )));
    }
    Ok((genesis, validators, raw_blocks))
}

struct ByteCursor<'a>(&'a [u8]);

impl<'a> ByteCursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], CliError> {
        let Some((head, tail)) = self.0.split_at_checked(n) else {
            return Err(CliError::fail("truncated chain file"));
        };
        self.0 = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], CliError> {
        self.take(N)?
            .try_into()
            .map_err(|_| CliError::fail("truncated chain file"))
    }

    fn count(&mut self) -> Result<usize, CliError> {
        let n = usize::try_from(u32::from_le_bytes(self.array::<4>()?))
            .map_err(|_| CliError::fail("chain count exceeds usize"))?;
        if n > MAX_CHAIN_ENTRIES {
            return Err(CliError::fail("chain count exceeds limit"));
        }
        Ok(n)
    }
}

fn push_u32(out: &mut Vec<u8>, n: usize) -> Result<(), CliError> {
    let val = u32::try_from(n).map_err(|_| CliError::fail("length exceeds u32"))?;
    out.extend_from_slice(&val.to_le_bytes());
    Ok(())
}

fn read_input_bytes(path: &str) -> Result<Vec<u8>, CliError> {
    if path == "-" {
        read_stdin_bytes()
    } else {
        fs::read(path).map_err(|e| CliError::io(format!("cannot read '{path}': {e}"), &e))
    }
}

fn read_input_string(path: &str) -> Result<String, CliError> {
    let bytes = read_input_bytes(path)?;
    String::from_utf8(bytes).map_err(|_| CliError::fail(format!("'{path}' is not valid UTF-8")))
}

fn read_stdin_bytes() -> Result<Vec<u8>, CliError> {
    let mut buf = Vec::new();
    io::stdin()
        .read_to_end(&mut buf)
        .map_err(|e| CliError::io(format!("cannot read stdin: {e}"), &e))?;
    Ok(buf)
}

fn write_output(path: &str, data: &[u8]) -> Result<(), CliError> {
    if path == "-" {
        io::stdout()
            .write_all(data)
            .map_err(|e| CliError::io(format!("cannot write stdout: {e}"), &e))
    } else {
        fs::write(path, data).map_err(|e| CliError::io(format!("cannot write '{path}': {e}"), &e))
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex_32(s: &str) -> Result<[u8; 32], String> {
    if s.len() != 64 {
        return Err(format!("expected 64 hex chars, got {}", s.len()));
    }
    let mut out = [0u8; 32];
    for (i, &[hi_b, lo_b]) in s.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let hi = hex_nibble(hi_b)?;
        let lo = hex_nibble(lo_b)?;
        if let Some(slot) = out.get_mut(i) {
            *slot = (hi << 4) | lo;
        }
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(format!("invalid hex character {:?}", b as char)),
    }
}

fn decode_err_str(err: DecodeError) -> &'static str {
    match err {
        DecodeError::Truncated => "truncated",
        DecodeError::TrailingBytes => "trailing bytes",
        DecodeError::TooLarge => "too large",
    }
}

fn block_err_str(err: BlockError) -> String {
    match err {
        BlockError::BadHeight => String::from("bad height"),
        BlockError::BadParent => String::from("bad parent"),
        BlockError::BadTime => String::from("bad time"),
        BlockError::WrongProposer => String::from("wrong proposer"),
        BlockError::BadSignature => String::from("bad signature"),
        BlockError::TooLarge => String::from("too many transactions"),
        BlockError::BadTxRoot => String::from("bad tx root"),
        BlockError::BadStateRoot => String::from("bad state root"),
        BlockError::BadTx { index, error } => {
            let reason = match error {
                TxError::TooLarge => "data too large",
                TxError::BadSignature => "bad signature",
                TxError::BadNonce => "bad nonce",
                TxError::InsufficientFunds => "insufficient funds",
                TxError::Overflow => "overflow",
            };
            format!("bad tx {index}: {reason}")
        }
    }
}
