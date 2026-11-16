#![forbid(unsafe_code)]

//! Command-line tool for inspecting, verifying, disassembling, and running MBC1 modules.

use aci::envelope::{self, Code, DEFAULT_LIMIT, MAX_LIMIT};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};
use elysium::{
    decode, verify, Const, DecodeError, Heap, Limits, LoadError, Module, NoHost, Obj, Op, Trap,
    Value, VerifyError, Vm,
};
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;
use std::time::Instant;

const VERSION: &str = "elysium 0.1.0";

const HELP: &str = "\
Usage: elysium <command> [options] [args]

Commands:
  verify FILE.mbc            Verify an MBC1 bytecode module (- for stdin)
  dis FILE.mbc               Disassemble an MBC1 bytecode module (- for stdin)
  run FILE.mbc [--fuel N]    Run an MBC1 module's entry function (- for stdin)

Options:
  -h, --help                 Show this help message
  -V, --version              Show version

Examples:
  elysium verify prog.mbc
  elysium dis prog.mbc
  elysium run prog.mbc --fuel 10000";

enum Cmd {
    Help,
    Version,
    Verify { path: String },
    Dis { path: String },
    Run { path: String, fuel: Option<u64> },
}

enum CliError {
    Usage(String),
    Failed(String),
}

fn main() -> ExitCode {
    let mut raw: Vec<OsString> = env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut raw);
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    if json {
        return run_json(&raw);
    }
    match run_cli() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Failed(msg)) => {
            eprintln!("elysium: {msg}");
            ExitCode::from(1)
        }
        Err(CliError::Usage(msg)) => {
            eprintln!("elysium: {msg}");
            ExitCode::from(2)
        }
    }
}

fn run_cli() -> Result<(), CliError> {
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_args(&args)? {
        Cmd::Help => {
            println!("{HELP}");
            Ok(())
        }
        Cmd::Version => {
            println!("{VERSION}");
            Ok(())
        }
        Cmd::Verify { path } => cmd_verify(&path),
        Cmd::Dis { path } => cmd_dis(&path),
        Cmd::Run { path, fuel } => cmd_run(&path, fuel),
    }
}

fn parse_args(args: &[String]) -> Result<Cmd, CliError> {
    let Some(first) = args.first() else {
        return Err(CliError::Usage(String::from(
            "missing command (try --help)",
        )));
    };
    if first == "-h" || first == "--help" {
        return Ok(Cmd::Help);
    }
    if first == "-V" || first == "--version" {
        return Ok(Cmd::Version);
    }
    let rest = &args[1..];
    if rest.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(Cmd::Help);
    }
    match first.as_str() {
        "verify" => Ok(Cmd::Verify {
            path: parse_single_file("verify", rest)?,
        }),
        "dis" => Ok(Cmd::Dis {
            path: parse_single_file("dis", rest)?,
        }),
        "run" => parse_run_args(rest),
        other => Err(CliError::Usage(format!(
            "unknown command '{other}' (try --help)"
        ))),
    }
}

fn parse_single_file(cmd: &str, args: &[String]) -> Result<String, CliError> {
    let mut path: Option<String> = None;
    for arg in args {
        if arg != "-" && arg.starts_with('-') {
            return Err(CliError::Usage(format!(
                "unknown option '{arg}' for '{cmd}'"
            )));
        }
        if path.is_some() {
            return Err(CliError::Usage(format!(
                "unexpected argument '{arg}' for '{cmd}'"
            )));
        }
        path = Some(arg.clone());
    }
    path.ok_or_else(|| CliError::Usage(format!("missing FILE.mbc for '{cmd}'")))
}

fn parse_run_args(args: &[String]) -> Result<Cmd, CliError> {
    let mut path: Option<String> = None;
    let mut fuel: Option<u64> = None;
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "--fuel" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| CliError::Usage(String::from("missing value for '--fuel'")))?;
            let n = val
                .parse::<u64>()
                .map_err(|_| CliError::Usage(format!("invalid '--fuel' value '{val}'")))?;
            fuel = Some(n);
        } else if let Some(val) = arg.strip_prefix("--fuel=") {
            let n = val
                .parse::<u64>()
                .map_err(|_| CliError::Usage(format!("invalid '--fuel' value '{val}'")))?;
            fuel = Some(n);
        } else if arg != "-" && arg.starts_with('-') {
            return Err(CliError::Usage(format!("unknown option '{arg}' for 'run'")));
        } else if path.is_some() {
            return Err(CliError::Usage(format!(
                "unexpected argument '{arg}' for 'run'"
            )));
        } else {
            path = Some(arg.clone());
        }
        i += 1;
    }
    let path = path.ok_or_else(|| CliError::Usage(String::from("missing FILE.mbc for 'run'")))?;
    Ok(Cmd::Run { path, fuel })
}

fn read_input(path: &str) -> Result<Vec<u8>, CliError> {
    if path == "-" {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| CliError::Failed(format!("failed to read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read(path).map_err(|e| CliError::Failed(format!("failed to read '{path}': {e}")))
    }
}

fn decode_file(path: &str) -> Result<Module, CliError> {
    let bytes = read_input(path)?;
    decode(&bytes).map_err(|e| CliError::Failed(fmt_decode_error(e)))
}

fn cmd_verify(path: &str) -> Result<(), CliError> {
    let module = decode_file(path)?;
    verify(&module).map_err(|e| CliError::Failed(fmt_verify_error(&e)))?;
    println!("ok");
    println!("consts: {}", module.consts.len());
    println!("imports: {}", module.imports.len());
    println!("funcs: {}", module.funcs.len());
    println!("entry: {}", module.entry);
    Ok(())
}

fn cmd_dis(path: &str) -> Result<(), CliError> {
    let module = decode_file(path)?;
    println!("entry: {}", module.entry);
    for (idx, c) in module.consts.iter().enumerate() {
        match c {
            Const::Str(s) => println!("const {idx}: str {s:?}"),
            Const::Bytes(b) => println!("const {idx}: bytes {} {}", b.len(), hex_bytes(b)),
        }
    }
    for (idx, name) in module.imports.iter().enumerate() {
        println!("import {idx}: {name}");
    }
    for (fi, func) in module.funcs.iter().enumerate() {
        println!(
            "func {fi} {} params={} regs={}:",
            func.name, func.params, func.regs
        );
        for (pc, op) in func.code.iter().enumerate() {
            println!("  {pc:04}: {}", fmt_op(op));
        }
    }
    Ok(())
}

fn cmd_run(path: &str, fuel: Option<u64>) -> Result<(), CliError> {
    let module = decode_file(path)?;
    let mut vm = Vm::new(module, &[]).map_err(|e| CliError::Failed(fmt_load_error(&e)))?;
    let mut limits = Limits::default();
    if let Some(n) = fuel {
        limits.fuel = n;
    }
    match vm.run(&mut NoHost, limits) {
        Ok(val) => {
            let mut seen = BTreeSet::new();
            let rendered = fmt_value(val, vm.heap(), &mut seen, 0);
            println!("result: {rendered}");
            println!("steps: {}", vm.fuel_used());
            Ok(())
        }
        Err(trap) => {
            let msg = fmt_trap(&trap);
            if let Some((func, at)) = vm.trap_site() {
                Err(CliError::Failed(format!("trap at fn#{func}+{at}: {msg}")))
            } else {
                Err(CliError::Failed(format!("trap: {msg}")))
            }
        }
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn fmt_value(val: Value, heap: &Heap, seen: &mut BTreeSet<u32>, depth: usize) -> String {
    match val {
        Value::Unit => String::from("()"),
        Value::Int(i) => i.to_string(),
        Value::Byte(b) => format!("{b}u8"),
        Value::Bool(b) => b.to_string(),
        Value::Fn(f) => format!("fn#{f}"),
        Value::Ref(h) => {
            if depth >= 32 {
                return format!("ref#{}(...)", h.0);
            }
            if !seen.insert(h.0) {
                return format!("ref#{}(<cycle>)", h.0);
            }
            let rendered = match heap.get(h) {
                Ok(Obj::Str(s)) => format!("{s:?}"),
                Ok(Obj::Bytes(b)) => format!("bytes[{}]({})", b.len(), hex_bytes(b)),
                Ok(Obj::Array(items)) => {
                    let parts: Vec<String> = items
                        .iter()
                        .map(|v| fmt_value(*v, heap, seen, depth + 1))
                        .collect();
                    format!("[{}]", parts.join(", "))
                }
                Ok(Obj::Record(fields)) => {
                    let parts: Vec<String> = fields
                        .iter()
                        .map(|v| fmt_value(*v, heap, seen, depth + 1))
                        .collect();
                    format!("{{{}}}", parts.join(", "))
                }
                Ok(Obj::Variant { tag, fields }) => {
                    let parts: Vec<String> = fields
                        .iter()
                        .map(|v| fmt_value(*v, heap, seen, depth + 1))
                        .collect();
                    format!("variant#{tag}({})", parts.join(", "))
                }
                Err(_) => format!("ref#{}(<dangling>)", h.0),
            };
            seen.remove(&h.0);
            rendered
        }
    }
}

fn fmt_decode_error(err: DecodeError) -> String {
    match err {
        DecodeError::BadMagic => String::from("decode error: bad magic (expected MBC1)"),
        DecodeError::Truncated => String::from("decode error: unexpected end of input"),
        DecodeError::BadOpcode(b) => format!("decode error: invalid opcode or tag 0x{b:02x}"),
        DecodeError::BadUtf8 => String::from("decode error: invalid UTF-8 string"),
        DecodeError::TooLarge => String::from("decode error: length exceeds remaining input"),
        DecodeError::Overlong => String::from("decode error: overlong LEB128 integer"),
    }
}

fn fmt_verify_error(err: &VerifyError) -> String {
    match err {
        VerifyError::TooManyFuncs => String::from("verify error: too many functions"),
        VerifyError::TooManyConsts => String::from("verify error: too many constants"),
        VerifyError::TooManyImports => String::from("verify error: too many imports"),
        VerifyError::FuncNameTooLong { func } => {
            format!("verify error: fn#{func} name exceeds 255 bytes")
        }
        VerifyError::ImportNameTooLong { import } => {
            format!("verify error: import#{import} name exceeds 255 bytes")
        }
        VerifyError::EntryOutOfRange { entry } => {
            format!("verify error: entry function {entry} out of range")
        }
        VerifyError::FrameTooLarge { func } => {
            format!("verify error: fn#{func} frame exceeds max registers")
        }
        VerifyError::ParamsExceedRegs { func } => {
            format!("verify error: fn#{func} params exceed frame registers")
        }
        VerifyError::TooManyInstructions { func } => {
            format!("verify error: fn#{func} exceeds max instructions")
        }
        VerifyError::EmptyCode { func } => format!("verify error: fn#{func} has no instructions"),
        VerifyError::BadLastInstruction { func } => {
            format!("verify error: fn#{func} must end with ret, jump, or trap")
        }
        VerifyError::RegisterOutOfRange { func, at, reg } => {
            format!("verify error: fn#{func}+{at}: register r{reg} out of range")
        }
        VerifyError::RegisterRangeOutOfRange { func, at } => {
            format!("verify error: fn#{func}+{at}: register range out of range")
        }
        VerifyError::ConstOutOfRange { func, at, index } => {
            format!("verify error: fn#{func}+{at}: const c#{index} out of range")
        }
        VerifyError::FuncOutOfRange { func, at, index } => {
            format!("verify error: fn#{func}+{at}: function fn#{index} out of range")
        }
        VerifyError::ImportOutOfRange { func, at, index } => {
            format!("verify error: fn#{func}+{at}: import imp#{index} out of range")
        }
        VerifyError::JumpOutOfRange { func, at, target } => {
            format!("verify error: fn#{func}+{at}: jump target @{target} out of range")
        }
        VerifyError::ArgCountMismatch {
            func,
            at,
            callee,
            expected,
            got,
        } => {
            format!("verify error: fn#{func}+{at}: fn#{callee} expects {expected} args, got {got}")
        }
        VerifyError::AssertNotString { func, at, index } => {
            format!("verify error: fn#{func}+{at}: assert const c#{index} is not a string")
        }
    }
}

fn fmt_load_error(err: &LoadError) -> String {
    match err {
        LoadError::Verify(e) => fmt_verify_error(e),
        LoadError::MissingImport(name) => format!("missing host import '{name}'"),
    }
}

fn fmt_trap(trap: &Trap) -> String {
    match trap {
        Trap::Overflow => String::from("integer overflow"),
        Trap::DivideByZero => String::from("divide by zero"),
        Trap::IndexOutOfRange => String::from("index out of range"),
        Trap::AssertFailed(msg) => format!("assert failed: {msg}"),
        Trap::TypeError => String::from("type error"),
        Trap::OutOfFuel => String::from("out of fuel"),
        Trap::OutOfMemory => String::from("out of memory"),
        Trap::StackOverflow => String::from("stack overflow"),
        Trap::Host(msg) => format!("host error: {msg}"),
        Trap::User(code) => format!("user trap {code}"),
    }
}

fn fmt_op(op: &Op) -> String {
    match *op {
        Op::Int(d, i) => format!("int r{d}, {i}"),
        Op::Byte(d, b) => format!("byte r{d}, {b}"),
        Op::Bool(d, b) => format!("bool r{d}, {b}"),
        Op::Unit(d) => format!("unit r{d}"),
        Op::FnRef(d, x) => format!("fn_ref r{d}, fn#{x}"),
        Op::Const(d, x) => format!("const r{d}, c#{x}"),
        Op::Move(d, a) => format!("move r{d}, r{a}"),
        Op::Add(d, a, b) => format!("add r{d}, r{a}, r{b}"),
        Op::Sub(d, a, b) => format!("sub r{d}, r{a}, r{b}"),
        Op::Mul(d, a, b) => format!("mul r{d}, r{a}, r{b}"),
        Op::AddW(d, a, b) => format!("add_w r{d}, r{a}, r{b}"),
        Op::SubW(d, a, b) => format!("sub_w r{d}, r{a}, r{b}"),
        Op::MulW(d, a, b) => format!("mul_w r{d}, r{a}, r{b}"),
        Op::Div(d, a, b) => format!("div r{d}, r{a}, r{b}"),
        Op::Rem(d, a, b) => format!("rem r{d}, r{a}, r{b}"),
        Op::Neg(d, a) => format!("neg r{d}, r{a}"),
        Op::And(d, a, b) => format!("and r{d}, r{a}, r{b}"),
        Op::Or(d, a, b) => format!("or r{d}, r{a}, r{b}"),
        Op::Xor(d, a, b) => format!("xor r{d}, r{a}, r{b}"),
        Op::Shl(d, a, b) => format!("shl r{d}, r{a}, r{b}"),
        Op::Shr(d, a, b) => format!("shr r{d}, r{a}, r{b}"),
        Op::Not(d, a) => format!("not r{d}, r{a}"),
        Op::Eq(d, a, b) => format!("eq r{d}, r{a}, r{b}"),
        Op::Ne(d, a, b) => format!("ne r{d}, r{a}, r{b}"),
        Op::Lt(d, a, b) => format!("lt r{d}, r{a}, r{b}"),
        Op::Le(d, a, b) => format!("le r{d}, r{a}, r{b}"),
        Op::Gt(d, a, b) => format!("gt r{d}, r{a}, r{b}"),
        Op::Ge(d, a, b) => format!("ge r{d}, r{a}, r{b}"),
        Op::Jump(t) => format!("jump @{t}"),
        Op::JumpIf(a, t) => format!("jump_if r{a}, @{t}"),
        Op::JumpIfNot(a, t) => format!("jump_if_not r{a}, @{t}"),
        Op::Call(d, f, first, n) => format!("call r{d}, fn#{f}, r{first}..+{n}"),
        Op::CallValue(d, a, first, n) => format!("call_value r{d}, r{a}, r{first}..+{n}"),
        Op::CallHost(d, h, first, n) => format!("call_host r{d}, imp#{h}, r{first}..+{n}"),
        Op::Ret(a) => format!("ret r{a}"),
        Op::NewArray(d, first, n) => format!("new_array r{d}, r{first}..+{n}"),
        Op::NewBytes(d, first, n) => format!("new_bytes r{d}, r{first}..+{n}"),
        Op::NewFill(d, a, b) => format!("new_fill r{d}, r{a}, r{b}"),
        Op::NewRecord(d, first, n) => format!("new_record r{d}, r{first}..+{n}"),
        Op::NewVariant(d, tag, first, n) => format!("new_variant r{d}, tag#{tag}, r{first}..+{n}"),
        Op::GetField(d, a, i) => format!("get_field r{d}, r{a}, .{i}"),
        Op::SetField(a, i, b) => format!("set_field r{a}, .{i}, r{b}"),
        Op::Tag(d, a) => format!("tag r{d}, r{a}"),
        Op::GetIndex(d, a, b) => format!("get_index r{d}, r{a}, r{b}"),
        Op::SetIndex(a, b, c) => format!("set_index r{a}, r{b}, r{c}"),
        Op::Len(d, a) => format!("len r{d}, r{a}"),
        Op::Push(a, b) => format!("push r{a}, r{b}"),
        Op::Pop(d, a) => format!("pop r{d}, r{a}"),
        Op::Slice(d, a, lo, hi) => format!("slice r{d}, r{a}, r{lo}, r{hi}"),
        Op::Concat(d, a, b) => format!("concat r{d}, r{a}, r{b}"),
        Op::IntToStr(d, a) => format!("int_to_str r{d}, r{a}"),
        Op::ParseInt(d, a) => format!("parse_int r{d}, r{a}"),
        Op::StrToBytes(d, a) => format!("str_to_bytes r{d}, r{a}"),
        Op::BytesToStr(d, a) => format!("bytes_to_str r{d}, r{a}"),
        Op::ToInt(d, a) => format!("to_int r{d}, r{a}"),
        Op::ToByte(d, a) => format!("to_byte r{d}, r{a}"),
        Op::Assert(a, k) => format!("assert r{a}, c#{k}"),
        Op::Trap(code) => format!("trap {code}"),
    }
}

// ---- --describe and --json (docs/ai-computer/ACI.md). The text path above is unchanged. ----

const VERIFY_ARGS: &[Arg] = &[Arg {
    name: "FILE",
    kind: Kind::Path,
    required: true,
    default: None,
    choices: &[],
}];

const DIS_ARGS: &[Arg] = &[
    Arg {
        name: "FILE",
        kind: Kind::Path,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "--limit",
        kind: Kind::Int,
        required: false,
        default: Some("100"),
        choices: &[],
    },
    Arg {
        name: "--offset",
        kind: Kind::Int,
        required: false,
        default: Some("0"),
        choices: &[],
    },
];

const RUN_ARGS: &[Arg] = &[
    Arg {
        name: "FILE",
        kind: Kind::Path,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "--fuel",
        kind: Kind::Int,
        required: false,
        default: None,
        choices: &[],
    },
];

const COMMANDS: &[Command] = &[
    Command {
        name: "verify",
        summary: "Verify an MBC1 bytecode module FILE",
        effect: Effect::Read,
        args: VERIFY_ARGS,
        returns: "consts, imports, funcs, entry, diagnostics: [], count: 0 (an invalid module is code invalid)",
        example: "elysium --json verify prog.mbc",
    },
    Command {
        name: "dis",
        summary: "Disassemble FILE; --limit and --offset page the functions",
        effect: Effect::Read,
        args: DIS_ARGS,
        returns: "entry, consts: [{index, kind, text}], imports: [{index, name}], items: functions [{index, name, params, regs, code: [{pc, text}]}], total, more, next_offset",
        example: "elysium --json dis prog.mbc --limit 10",
    },
    Command {
        name: "run",
        summary: "Run the entry function of FILE with an optional --fuel budget",
        effect: Effect::Exec,
        args: RUN_ARGS,
        returns: "result (capped, with result_truncated, result_bytes, result_lossy when they apply), exit (0), stop (exit), steps, duration_ms; a trap is code invalid",
        example: "elysium --json run prog.mbc --fuel 10000",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "elysium",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Bytecode VM (Elysium) for MBC1 modules",
    commands: COMMANDS,
};

/// A failed `--json` run: the envelope code and message, the hint, and the process exit code.
struct Fail {
    code: Code,
    message: String,
    hint: Option<String>,
    exit: u8,
}

impl Fail {
    fn usage(message: String) -> Fail {
        Fail {
            code: Code::Usage,
            message,
            hint: Some(String::from("run: elysium --describe")),
            exit: 2,
        }
    }

    fn op(code: Code, message: String) -> Fail {
        Fail {
            code,
            message,
            hint: None,
            exit: 1,
        }
    }
}

fn run_json(raw: &[OsString]) -> ExitCode {
    let args: Vec<String> = raw
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let first = args.first().map(String::as_str);
    let cmd = ["verify", "dis", "run"]
        .into_iter()
        .find(|c| first == Some(*c))
        .unwrap_or("");
    match json_command(&args) {
        Ok((data, exit)) => {
            print!("{}", envelope::ok("elysium", cmd, data));
            ExitCode::from(exit)
        }
        Err(fail) => {
            eprintln!("elysium: {}", fail.message);
            print!(
                "{}",
                envelope::err(
                    "elysium",
                    cmd,
                    fail.code,
                    &fail.message,
                    fail.hint.as_deref()
                )
            );
            ExitCode::from(fail.exit)
        }
    }
}

fn json_command(args: &[String]) -> Result<(Json, u8), Fail> {
    let (args, limit, offset) = take_page(args)?;
    match parse_args(&args) {
        Err(CliError::Usage(msg)) => Err(Fail::usage(msg)),
        Err(CliError::Failed(msg)) => Err(Fail::op(Code::Invalid, msg)),
        Ok(Cmd::Help) => Ok((Json::obj([("usage", Json::Str(HELP.into()))]), 0)),
        Ok(Cmd::Version) => Ok((Json::obj([("version", Json::Str(VERSION.into()))]), 0)),
        Ok(Cmd::Verify { path }) => verify_json(&path),
        Ok(Cmd::Dis { path }) => dis_json(&path, limit, offset),
        Ok(Cmd::Run { path, fuel }) => run_json_cmd(&path, fuel),
    }
}

/// Pull `--limit` and `--offset` out of `dis` arguments. They are JSON-only paging flags.
fn take_page(args: &[String]) -> Result<(Vec<String>, usize, usize), Fail> {
    let is_dis = args.first().map(String::as_str) == Some("dis");
    let mut rest = Vec::with_capacity(args.len());
    let (mut limit, mut offset) = (DEFAULT_LIMIT, 0usize);
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let flag = arg.split_once('=').map_or(arg, |(f, _)| f);
        if is_dis && (flag == "--limit" || flag == "--offset") {
            let val = match arg.split_once('=') {
                Some((_, v)) => v.to_string(),
                None => {
                    i += 1;
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| Fail::usage(format!("missing value for '{flag}'")))?
                }
            };
            let n = val
                .parse::<usize>()
                .map_err(|_| Fail::usage(format!("invalid '{flag}' value '{val}'")))?;
            if flag == "--limit" {
                if n == 0 || n > MAX_LIMIT {
                    return Err(Fail::usage(format!("--limit must be 1 to {MAX_LIMIT}")));
                }
                limit = n;
            } else {
                offset = n;
            }
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((rest, limit, offset))
}

/// Read and decode a module for `--json`: an unreadable file is io, a bad module is invalid.
fn decode_json(path: &str) -> Result<Module, Fail> {
    let bytes = read_input(path).map_err(|e| match e {
        CliError::Usage(m) => Fail::usage(m),
        CliError::Failed(m) => Fail::op(Code::Io, m),
    })?;
    decode(&bytes).map_err(|e| Fail::op(Code::Invalid, fmt_decode_error(e)))
}

fn int(n: usize) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn verify_json(path: &str) -> Result<(Json, u8), Fail> {
    let module = decode_json(path)?;
    verify(&module).map_err(|e| Fail {
        code: Code::Invalid,
        message: fmt_verify_error(&e),
        hint: Some(format!("run: elysium --json dis {path}")),
        exit: 1,
    })?;
    let data = Json::obj([
        ("consts", int(module.consts.len())),
        ("imports", int(module.imports.len())),
        ("funcs", int(module.funcs.len())),
        ("entry", int(module.entry as usize)),
        ("diagnostics", Json::Arr(vec![])),
        ("count", Json::Int(0)),
    ]);
    Ok((data, 0))
}

fn dis_json(path: &str, limit: usize, offset: usize) -> Result<(Json, u8), Fail> {
    let module = decode_json(path)?;
    let consts = module
        .consts
        .iter()
        .enumerate()
        .map(|(i, c)| match c {
            Const::Str(s) => Json::obj([
                ("index", int(i)),
                ("kind", Json::Str("str".into())),
                ("text", Json::Str(s.clone())),
            ]),
            Const::Bytes(b) => Json::obj([
                ("index", int(i)),
                ("kind", Json::Str("bytes".into())),
                ("text", Json::Str(hex_bytes(b))),
            ]),
        })
        .collect();
    let imports = module
        .imports
        .iter()
        .enumerate()
        .map(|(i, name)| Json::obj([("index", int(i)), ("name", Json::Str(name.clone()))]))
        .collect();
    let total = module.funcs.len();
    let items: Vec<Json> = module
        .funcs
        .iter()
        .enumerate()
        .skip(offset)
        .take(limit)
        .map(|(fi, func)| {
            let code = func
                .code
                .iter()
                .enumerate()
                .map(|(pc, op)| Json::obj([("pc", int(pc)), ("text", Json::Str(fmt_op(op)))]))
                .collect();
            Json::obj([
                ("index", int(fi)),
                ("name", Json::Str(func.name.clone())),
                ("params", int(usize::from(func.params))),
                ("regs", int(usize::from(func.regs))),
                ("code", Json::Arr(code)),
            ])
        })
        .collect();
    let mut pairs = vec![
        ("entry".to_string(), int(module.entry as usize)),
        ("consts".to_string(), Json::Arr(consts)),
        ("imports".to_string(), Json::Arr(imports)),
    ];
    if let Json::Obj(page) = envelope::page(items, total, offset, limit) {
        pairs.extend(page);
    }
    Ok((Json::Obj(pairs), 0))
}

fn run_json_cmd(path: &str, fuel: Option<u64>) -> Result<(Json, u8), Fail> {
    let module = decode_json(path)?;
    let mut vm = Vm::new(module, &[]).map_err(|e| Fail::op(Code::Invalid, fmt_load_error(&e)))?;
    let mut limits = Limits::default();
    if let Some(n) = fuel {
        limits.fuel = n;
    }
    let started = Instant::now();
    match vm.run(&mut NoHost, limits) {
        Ok(val) => {
            let mut seen = BTreeSet::new();
            let rendered = fmt_value(val, vm.heap(), &mut seen, 0);
            let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
            let mut pairs = vec![("exit".to_string(), Json::Int(0))];
            pairs.extend(stream("result", rendered.as_bytes()));
            pairs.push(("stop".to_string(), Json::Str("exit".into())));
            pairs.push((
                "steps".to_string(),
                Json::Int(i64::try_from(vm.fuel_used()).unwrap_or(i64::MAX)),
            ));
            pairs.push(("duration_ms".to_string(), Json::Int(duration_ms)));
            Ok((Json::Obj(pairs), 0))
        }
        Err(trap) => {
            // The text path also treats a trap as an error (exit 1), so this is an error envelope.
            let msg = fmt_trap(&trap);
            let message = match vm.trap_site() {
                Some((func, at)) => format!("trap at fn#{func}+{at}: {msg}"),
                None => format!("trap: {msg}"),
            };
            let hint = if matches!(trap, Trap::OutOfFuel) {
                format!("run: elysium --json run {path} --fuel <larger N>")
            } else {
                format!("run: elysium --json dis {path}")
            };
            Err(Fail {
                code: Code::Invalid,
                message,
                hint: Some(hint),
                exit: 1,
            })
        }
    }
}

/// A captured text value: `name` for the text, `name_<field>` for `truncated`, `bytes`, `lossy`.
fn stream(name: &str, bytes: &[u8]) -> Vec<(String, Json)> {
    envelope::capped(bytes)
        .into_iter()
        .map(|(key, value)| {
            if key == "text" {
                (name.to_string(), value)
            } else {
                (format!("{name}_{key}"), value)
            }
        })
        .collect()
}
