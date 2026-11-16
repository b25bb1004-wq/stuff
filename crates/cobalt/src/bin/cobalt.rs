//! Command-line tool for Cobalt: Logos IR, optimiser and Elysium bytecode compiler.
#![forbid(unsafe_code)]

use std::env;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

use std::ffi::OsString;
use std::path::Path;

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};
use cobalt::opt;
use logos::ast::{BinOp, UnOp};
use logos::ir::{Body, Builtin, Inst, Lit, Program, Ty, TypeKind};

const VERSION: &str = "cobalt 0.1.0";

const HELP: &str = "\
cobalt - Logos IR, optimiser and Elysium bytecode compiler

USAGE:
    cobalt <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
    check FILE.logos               Type-check FILE.logos; prints nothing on success
    fmt FILE.logos                 Print FILE.logos in canonical form (refuses comments)
    ir FILE.logos                  Print typed Logos IR for FILE.logos
    opt FILE.logos                 Optimise Logos IR and print IR plus Stats
    build FILE.logos -o OUT.mbc    Compile and verify Elysium bytecode module

OPTIONS:
    -h, --help                     Print help information
    -V, --version                  Print version information

Note: '-' as FILE.logos reads stdin; '-' as OUT.mbc writes stdout.

EXAMPLES:
    cobalt check hello.logos
    cobalt fmt hello.logos
    cobalt ir hello.logos
    cobalt opt hello.logos
    cobalt build hello.logos -o hello.mbc";

fn main() {
    let mut raw: Vec<OsString> = env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut raw);
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return;
    }
    if json {
        process::exit(run_json(&raw));
    }
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        usage_error("missing subcommand (try --help)");
    };
    match cmd {
        "--help" | "-h" => println!("{HELP}"),
        "--version" | "-V" => println!("{VERSION}"),
        "check" => cmd_check(&args[1..]),
        "fmt" => cmd_fmt(&args[1..]),
        "ir" => cmd_ir(&args[1..]),
        "opt" => cmd_opt(&args[1..]),
        "build" => cmd_build(&args[1..]),
        _ => usage_error(&format!("unknown subcommand '{cmd}' (try --help)")),
    }
}

fn usage_error(msg: &str) -> ! {
    eprintln!("cobalt: {msg}");
    process::exit(2);
}

fn fail(msg: &str) -> ! {
    eprintln!("cobalt: {msg}");
    process::exit(1);
}

fn cmd_check(args: &[String]) {
    let path = parse_single_file(args, "usage: cobalt check FILE.logos");
    let _ = load_and_check(path);
}

fn cmd_fmt(args: &[String]) {
    let path = parse_single_file(args, "usage: cobalt fmt FILE.logos");
    let src = read_source(path);
    let label = source_label(path);
    let tokens = match logos::lexer::lex(&src) {
        Ok(t) => t,
        Err(diag) => report(&src, label, &diag),
    };
    // The printer drops comments, so formatting such a file would lose text.
    if has_comment(&src, &tokens) {
        fail(&format!("{label}: refusing to format a file with comments"));
    }
    let ast = match logos::parser::parse(&src) {
        Ok(m) => m,
        Err(diag) => report(&src, label, &diag),
    };
    write_stdout_text(&logos::print::print(&ast));
}

/// True if the source has text between tokens, which can only be a `//` comment.
fn has_comment(src: &str, tokens: &[logos::lexer::Token]) -> bool {
    let mut pos = 0;
    for t in tokens {
        let start = usize::try_from(t.span.start).unwrap_or(usize::MAX);
        if src.get(pos..start).is_some_and(|gap| gap.contains('/')) {
            return true;
        }
        pos = usize::try_from(t.span.end).unwrap_or(usize::MAX);
    }
    src.get(pos..).is_some_and(|gap| gap.contains('/'))
}

fn cmd_ir(args: &[String]) {
    let path = parse_single_file(args, "usage: cobalt ir FILE.logos");
    let prog = load_and_check(path);
    let mut out = String::new();
    format_program(&mut out, &prog);
    write_stdout_text(&out);
}

fn cmd_opt(args: &[String]) {
    let path = parse_single_file(args, "usage: cobalt opt FILE.logos");
    let mut prog = load_and_check(path);
    let stats = opt::optimize(&mut prog);
    let mut out = String::new();
    format_program(&mut out, &prog);
    let _ = writeln!(out, "stats.folded {}", stats.folded);
    let _ = writeln!(out, "stats.copies {}", stats.copies);
    let _ = writeln!(out, "stats.removed {}", stats.removed);
    write_stdout_text(&out);
}

fn cmd_build(args: &[String]) {
    let (in_path, out_path) = parse_build_args(args);
    let prog = load_and_check(in_path);
    let compiled = cobalt::compile(&prog);
    if let Err(err) = elysium::verify(&compiled.module) {
        fail(&format!("verify failed: {err:?}"));
    }
    let bytes = elysium::encode(&compiled.module);
    if out_path == "-" {
        let mut stdout = io::stdout().lock();
        if let Err(err) = stdout.write_all(&bytes).and_then(|()| stdout.flush()) {
            fail(&format!("stdout: {err}"));
        }
    } else if let Err(err) = fs::write(out_path, &bytes) {
        fail(&format!("{out_path}: {err}"));
    }
}

fn write_stdout_text(text: &str) {
    let mut stdout = io::stdout().lock();
    if let Err(err) = stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        fail(&format!("stdout: {err}"));
    }
}

fn parse_single_file<'a>(args: &'a [String], usage: &str) -> &'a str {
    let mut file: Option<&'a str> = None;
    for arg in args {
        let a = arg.as_str();
        if a == "--help" || a == "-h" {
            println!("{HELP}");
            process::exit(0);
        } else if a != "-" && a.starts_with('-') {
            usage_error(&format!("unknown option '{a}'"));
        } else if file.is_none() {
            file = Some(a);
        } else {
            usage_error(&format!("unexpected argument '{a}'"));
        }
    }
    file.unwrap_or_else(|| usage_error(usage))
}

fn parse_build_args<'a>(args: &'a [String]) -> (&'a str, &'a str) {
    let mut input: Option<&'a str> = None;
    let mut output: Option<&'a str> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--help" || a == "-h" {
            println!("{HELP}");
            process::exit(0);
        } else if a == "-o" || a == "--output" {
            i += 1;
            let Some(val) = args.get(i).map(String::as_str) else {
                usage_error(&format!("missing value for {a}"));
            };
            if output.is_some() {
                usage_error("duplicate -o/--output option");
            }
            output = Some(val);
        } else if let Some(val) = a
            .strip_prefix("-o=")
            .or_else(|| a.strip_prefix("--output="))
        {
            if val.is_empty() {
                usage_error("empty value for output option");
            }
            if output.is_some() {
                usage_error("duplicate -o/--output option");
            }
            output = Some(val);
        } else if a != "-" && a.starts_with('-') {
            usage_error(&format!("unknown option '{a}'"));
        } else if input.is_none() {
            input = Some(a);
        } else {
            usage_error(&format!("unexpected argument '{a}'"));
        }
        i += 1;
    }
    let Some(in_path) = input else {
        usage_error("usage: cobalt build FILE.logos -o OUT.mbc");
    };
    let Some(out_path) = output else {
        usage_error("missing -o OUT.mbc (usage: cobalt build FILE.logos -o OUT.mbc)");
    };
    (in_path, out_path)
}

/// Reads a source file, or stdin for `-`.
fn read_source(path: &str) -> String {
    if path == "-" {
        let mut buf = String::new();
        if let Err(err) = io::stdin().read_to_string(&mut buf) {
            fail(&format!("stdin: {err}"));
        }
        buf
    } else {
        match fs::read_to_string(path) {
            Ok(s) => s,
            Err(err) => fail(&format!("{path}: {err}")),
        }
    }
}

/// Name of the source in error messages.
fn source_label(path: &str) -> &str {
    if path == "-" {
        "<stdin>"
    } else {
        path
    }
}

/// Prints a parse or check error at its line and column, then exits 1.
fn report(src: &str, label: &str, diag: &logos::Diag) -> ! {
    let (line, col) = logos::line_col(src, diag.span.start);
    fail(&format!("{label}:{line}:{col}: {}", diag.message));
}

fn load_and_check(path: &str) -> Program {
    let src = read_source(path);
    let label = source_label(path);
    let ast = match logos::parser::parse(&src) {
        Ok(m) => m,
        Err(diag) => {
            let (line, col) = logos::line_col(&src, diag.span.start);
            fail(&format!("{label}:{line}:{col}: {}", diag.message));
        }
    };
    match logos::check::check(&ast) {
        Ok(prog) => prog,
        Err(diag) => {
            let (line, col) = logos::line_col(&src, diag.span.start);
            fail(&format!("{label}:{line}:{col}: {}", diag.message));
        }
    }
}

fn format_program(out: &mut String, prog: &Program) {
    for (idx, info) in prog.types.iter().enumerate() {
        let _ = write!(out, "type #{idx} {} = ", info.name);
        match &info.kind {
            TypeKind::Record(fields) => {
                out.push_str("{ ");
                for (i, (name, ty)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let _ = write!(out, "{name}: ");
                    format_ty(out, ty);
                }
                out.push_str(" }\n");
            }
            TypeKind::Variants(variants) => {
                for (i, (name, payload)) in variants.iter().enumerate() {
                    if i > 0 {
                        out.push_str(" | ");
                    }
                    out.push_str(name);
                    if !payload.is_empty() {
                        out.push('(');
                        for (j, ty) in payload.iter().enumerate() {
                            if j > 0 {
                                out.push_str(", ");
                            }
                            format_ty(out, ty);
                        }
                        out.push(')');
                    }
                }
                out.push('\n');
            }
        }
    }
    for (idx, layer) in prog.layers.iter().enumerate() {
        let _ = writeln!(out, "layer #{idx} {layer}");
    }
    if let Some(main_id) = prog.main {
        let _ = writeln!(out, "main fn#{main_id}");
    }
    for (name, fid) in &prog.tests {
        let _ = writeln!(out, "test {name:?} -> fn#{fid}");
    }
    for prop in &prog.props {
        let _ = write!(out, "prop {:?} -> fn#{}", prop.name, prop.func);
        if let Some(guard) = prop.guard {
            let _ = write!(out, " where fn#{guard}");
        }
        out.push('\n');
    }
    for (fid, func) in prog.funcs.iter().enumerate() {
        let _ = write!(out, "fn #{fid} {}(", func.name);
        for i in 0..func.params as usize {
            if i > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "r{i}: ");
            match func.regs.get(i) {
                Some(ty) => format_ty(out, ty),
                None => out.push('?'),
            }
        }
        out.push_str(") -> ");
        format_ty(out, &func.ret);
        let _ = writeln!(
            out,
            " [layers=0x{:x}, regs={}]",
            func.layers,
            func.regs.len()
        );
        match &func.body {
            Body::Extern(name) => {
                let _ = writeln!(out, "  extern {name:?}");
            }
            Body::Code { insts, .. } => {
                for (pc, inst) in insts.iter().enumerate() {
                    let _ = write!(out, "  {pc}: ");
                    format_inst(out, inst);
                    out.push('\n');
                }
            }
        }
    }
}

fn format_ty(out: &mut String, ty: &Ty) {
    match ty {
        Ty::Int => out.push_str("int"),
        Ty::Byte => out.push_str("byte"),
        Ty::Bool => out.push_str("bool"),
        Ty::Str => out.push_str("str"),
        Ty::Unit => out.push_str("unit"),
        Ty::Array(elem) => {
            out.push('[');
            format_ty(out, elem);
            out.push(']');
        }
        Ty::Tuple(items) => {
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                format_ty(out, item);
            }
            out.push(')');
        }
        Ty::Opt(inner) => {
            format_ty(out, inner);
            out.push('?');
        }
        Ty::Named(tid) => {
            let _ = write!(out, "type#{tid}");
        }
        Ty::Fn {
            params,
            ret,
            layers,
        } => {
            out.push_str("fn(");
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                format_ty(out, p);
            }
            out.push_str(") -> ");
            format_ty(out, ret);
            if *layers != 0 {
                let _ = write!(out, " [0x{layers:x}]");
            }
        }
    }
}

fn format_regs(out: &mut String, regs: &[u32]) {
    for (i, r) in regs.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "r{r}");
    }
}

fn format_lit(out: &mut String, lit: &Lit) {
    match lit {
        Lit::Int(v) => {
            let _ = write!(out, "{v}");
        }
        Lit::Byte(b) => {
            let _ = write!(out, "{b}u8");
        }
        Lit::Bool(b) => {
            let _ = write!(out, "{b}");
        }
        Lit::Unit => out.push_str("()"),
        Lit::Str(s) => {
            let _ = write!(out, "{s:?}");
        }
        Lit::Fn(fid) => {
            let _ = write!(out, "fn#{fid}");
        }
    }
}

fn unop_str(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "-",
        UnOp::Not => "!",
    }
}

fn binop_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Or => "||",
        BinOp::And => "&&",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::BitAnd => "&",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::AddWrap => "+%",
        BinOp::SubWrap => "-%",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::MulWrap => "*%",
    }
}

fn builtin_str(b: Builtin) -> &'static str {
    match b {
        Builtin::Len => "len",
        Builtin::Push => "push",
        Builtin::Pop => "pop",
        Builtin::Slice => "slice",
        Builtin::Concat => "concat",
        Builtin::IntToStr => "str",
        Builtin::ParseInt => "parse_int",
        Builtin::Bytes => "bytes",
        Builtin::Text => "text",
        Builtin::ToInt => "int",
        Builtin::ToByte => "byte",
        Builtin::Min => "min",
        Builtin::Max => "max",
        Builtin::Abs => "abs",
    }
}

fn format_inst(out: &mut String, inst: &Inst) {
    match inst {
        Inst::Lit(d, lit) => {
            let _ = write!(out, "Lit r{d}, ");
            format_lit(out, lit);
        }
        Inst::Move(d, s) => {
            let _ = write!(out, "Move r{d}, r{s}");
        }
        Inst::Un(d, op, s) => {
            let _ = write!(out, "Un r{d}, {}r{s}", unop_str(*op));
        }
        Inst::Bin(d, op, a, b) => {
            let _ = write!(out, "Bin r{d}, r{a} {} r{b}", binop_str(*op));
        }
        Inst::Call(d, fid, args) => {
            let _ = write!(out, "Call r{d}, fn#{fid}(");
            format_regs(out, args);
            out.push(')');
        }
        Inst::CallValue(d, callee, args) => {
            let _ = write!(out, "CallValue r{d}, r{callee}(");
            format_regs(out, args);
            out.push(')');
        }
        Inst::Builtin(d, b, args) => {
            let _ = write!(out, "Builtin r{d}, {}(", builtin_str(*b));
            format_regs(out, args);
            out.push(')');
        }
        Inst::NewArray(d, args) => {
            let _ = write!(out, "NewArray r{d}, [");
            format_regs(out, args);
            out.push(']');
        }
        Inst::NewFill(d, val, count) => {
            let _ = write!(out, "NewFill r{d}, [r{val}; r{count}]");
        }
        Inst::NewTuple(d, args) => {
            let _ = write!(out, "NewTuple r{d}, (");
            format_regs(out, args);
            out.push(')');
        }
        Inst::NewRecord(d, tid, args) => {
            let _ = write!(out, "NewRecord r{d}, type#{tid}(");
            format_regs(out, args);
            out.push(')');
        }
        Inst::NewVariant(d, tid, tag, args) => {
            let _ = write!(out, "NewVariant r{d}, type#{tid}#{tag}(");
            format_regs(out, args);
            out.push(')');
        }
        Inst::NewSome(d, s) => {
            let _ = write!(out, "NewSome r{d}, r{s}");
        }
        Inst::NewNone(d) => {
            let _ = write!(out, "NewNone r{d}");
        }
        Inst::GetField(d, base, idx) => {
            let _ = write!(out, "GetField r{d}, r{base}.{idx}");
        }
        Inst::SetField(base, idx, val) => {
            let _ = write!(out, "SetField r{base}.{idx}, r{val}");
        }
        Inst::Tag(d, s) => {
            let _ = write!(out, "Tag r{d}, r{s}");
        }
        Inst::GetIndex(d, base, idx) => {
            let _ = write!(out, "GetIndex r{d}, r{base}[r{idx}]");
        }
        Inst::SetIndex(base, idx, val) => {
            let _ = write!(out, "SetIndex r{base}[r{idx}], r{val}");
        }
        Inst::Label(lbl) => {
            let _ = write!(out, "L{lbl}:");
        }
        Inst::Jump(lbl) => {
            let _ = write!(out, "Jump L{lbl}");
        }
        Inst::Branch(cond, t_lbl, f_lbl) => {
            let _ = write!(out, "Branch r{cond}, L{t_lbl}, L{f_lbl}");
        }
        Inst::Ret(r) => {
            let _ = write!(out, "Ret r{r}");
        }
        Inst::Assert(r, msg) => {
            let _ = write!(out, "Assert r{r}, {msg:?}");
        }
    }
}

// ---- --describe and --json (docs/ai-computer/ACI.md). The text path above is unchanged. ----

const FILE_ARG: Arg = Arg {
    name: "FILE",
    kind: Kind::Path,
    required: true,
    default: None,
    choices: &[],
};

const BUILD_ARGS: &[Arg] = &[
    FILE_ARG,
    Arg {
        name: "-o",
        kind: Kind::Path,
        required: true,
        default: None,
        choices: &[],
    },
];

const COMMANDS: &[Command] = &[
    Command {
        name: "check",
        summary: "Type-check FILE; prints nothing on success",
        effect: Effect::Read,
        args: &[FILE_ARG],
        returns: "count (0), diagnostics ([]); a compile error is code invalid with line and column in the message",
        example: "cobalt --json check prog.logos",
    },
    Command {
        name: "fmt",
        summary: "Print FILE in canonical form (refuses files with comments)",
        effect: Effect::Read,
        args: &[FILE_ARG],
        returns: "text (capped, with truncated, bytes when they apply)",
        example: "cobalt --json fmt prog.logos",
    },
    Command {
        name: "ir",
        summary: "Print the typed Logos IR of FILE",
        effect: Effect::Read,
        args: &[FILE_ARG],
        returns: "text (capped, with truncated, bytes when they apply)",
        example: "cobalt --json ir prog.logos",
    },
    Command {
        name: "opt",
        summary: "Optimise the Logos IR of FILE and print it with the optimiser stats",
        effect: Effect::Read,
        args: &[FILE_ARG],
        returns: "text (capped, the optimised IR), stats: {folded, copies, removed}",
        example: "cobalt --json opt prog.logos",
    },
    Command {
        name: "build",
        summary: "Compile FILE to a verified Elysium module written to -o OUT (a file; stdout is refused under --json)",
        effect: Effect::Write,
        args: BUILD_ARGS,
        returns: "path (the module file), bytes",
        example: "cobalt --json build prog.logos -o prog.mbc",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "cobalt",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Logos compiler: check, format, IR, optimiser and Elysium bytecode",
    commands: COMMANDS,
};

/// A failed `--json` run: envelope code and message, the hint, and the process exit code.
struct Fail {
    code: Code,
    message: String,
    hint: Option<String>,
    exit: i32,
}

impl Fail {
    fn usage(message: String) -> Fail {
        Fail {
            code: Code::Usage,
            message,
            hint: Some(String::from("run: cobalt --describe")),
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

fn run_json(raw: &[OsString]) -> i32 {
    let args: Vec<String> = raw
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let first = args.first().map(String::as_str);
    let cmd = COMMANDS
        .iter()
        .map(|c| c.name)
        .find(|c| first == Some(*c))
        .unwrap_or("");
    match json_command(&args) {
        Ok((data, exit)) => {
            print!("{}", envelope::ok("cobalt", cmd, data));
            exit
        }
        Err(fail) => {
            eprintln!("cobalt: {}", fail.message);
            print!(
                "{}",
                envelope::err(
                    "cobalt",
                    cmd,
                    fail.code,
                    &fail.message,
                    fail.hint.as_deref()
                )
            );
            fail.exit
        }
    }
}

fn json_command(args: &[String]) -> Result<(Json, i32), Fail> {
    let Some(cmd) = args.first().map(String::as_str) else {
        return Err(Fail::usage(String::from("missing subcommand (try --help)")));
    };
    let rest = &args[1..];
    match cmd {
        "--help" | "-h" => Ok((Json::obj([("usage", Json::Str(HELP.into()))]), 0)),
        "--version" | "-V" => Ok((Json::obj([("version", Json::Str(VERSION.into()))]), 0)),
        "check" => {
            // Findings are data here (as in `athena check`): ok:true, exit code as the text path.
            let path = single_file(rest, "check")?;
            let src = read_json(path)?;
            let diag = match logos::parser::parse(&src) {
                Err(d) => Some(d),
                Ok(ast) => logos::check::check(&ast).err(),
            };
            let Some(diag) = diag else {
                let data = Json::obj([
                    ("count", Json::Int(0)),
                    ("diagnostics", Json::Arr(vec![])),
                    ("clean", Json::Bool(true)),
                ]);
                return Ok((data, 0));
            };
            let (line, col) = logos::line_col(&src, diag.span.start);
            let label = source_label(path);
            eprintln!("cobalt: {label}:{line}:{col}: {}", diag.message);
            let item = Json::obj([
                ("path", Json::Str(label.into())),
                ("line", Json::Int(i64::from(line))),
                ("col", Json::Int(i64::from(col))),
                ("severity", Json::Str("error".into())),
                ("message", Json::Str(diag.message.clone())),
            ]);
            let data = Json::obj([
                ("count", Json::Int(1)),
                ("diagnostics", Json::Arr(vec![item])),
                ("clean", Json::Bool(false)),
            ]);
            Ok((data, 1))
        }
        "fmt" => fmt_json(single_file(rest, "fmt")?),
        "ir" => {
            let prog = load_json(single_file(rest, "ir")?)?;
            let mut out = String::new();
            format_program(&mut out, &prog);
            Ok((Json::Obj(envelope::capped(out.as_bytes())), 0))
        }
        "opt" => {
            let mut prog = load_json(single_file(rest, "opt")?)?;
            let stats = opt::optimize(&mut prog);
            let mut out = String::new();
            format_program(&mut out, &prog);
            let mut pairs = envelope::capped(out.as_bytes());
            let stats = Json::obj([
                ("folded", int(stats.folded)),
                ("copies", int(stats.copies)),
                ("removed", int(stats.removed)),
            ]);
            pairs.push((String::from("stats"), stats));
            Ok((Json::Obj(pairs), 0))
        }
        "build" => build_json(rest),
        other => Err(Fail::usage(format!(
            "unknown subcommand '{other}' (try --help)"
        ))),
    }
}

fn int(n: usize) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

/// The one FILE argument of a single-file command.
fn single_file<'a>(args: &'a [String], cmd: &str) -> Result<&'a str, Fail> {
    let mut file: Option<&str> = None;
    for arg in args {
        let a = arg.as_str();
        if a != "-" && a.starts_with('-') {
            return Err(Fail::usage(format!("unknown option '{a}'")));
        }
        if file.is_some() {
            return Err(Fail::usage(format!("unexpected argument '{a}'")));
        }
        file = Some(a);
    }
    file.ok_or_else(|| Fail::usage(format!("usage: cobalt {cmd} FILE.logos")))
}

/// Reads a source file, or stdin for `-`, for `--json`.
fn read_json(path: &str) -> Result<String, Fail> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|err| Fail::op(Code::Io, format!("stdin: {err}")))?;
        return Ok(buf);
    }
    fs::read_to_string(path).map_err(|err| {
        let code = if err.kind() == io::ErrorKind::NotFound {
            Code::NotFound
        } else {
            Code::Io
        };
        Fail::op(code, format!("{path}: {err}"))
    })
}

/// A parse or check error as the envelope message: `label:line:col: message`, code invalid.
fn compile_fail(src: &str, path: &str, diag: &logos::Diag) -> Fail {
    let (line, col) = logos::line_col(src, diag.span.start);
    Fail {
        code: Code::Invalid,
        message: format!("{}:{line}:{col}: {}", source_label(path), diag.message),
        hint: Some(format!("run: cobalt --json check {path}")),
        exit: 1,
    }
}

fn load_json(path: &str) -> Result<Program, Fail> {
    let src = read_json(path)?;
    let ast = logos::parser::parse(&src).map_err(|d| compile_fail(&src, path, &d))?;
    logos::check::check(&ast).map_err(|d| compile_fail(&src, path, &d))
}

fn fmt_json(path: &str) -> Result<(Json, i32), Fail> {
    let src = read_json(path)?;
    let tokens = logos::lexer::lex(&src).map_err(|d| compile_fail(&src, path, &d))?;
    // The printer drops comments, so formatting such a file would lose text.
    if has_comment(&src, &tokens) {
        return Err(Fail::op(
            Code::Invalid,
            format!(
                "{}: refusing to format a file with comments",
                source_label(path)
            ),
        ));
    }
    let ast = logos::parser::parse(&src).map_err(|d| compile_fail(&src, path, &d))?;
    let text = logos::print::print(&ast);
    Ok((Json::Obj(envelope::capped(text.as_bytes())), 0))
}

fn build_json(args: &[String]) -> Result<(Json, i32), Fail> {
    let (in_path, out_path) = build_args_json(args)?;
    // The module is binary: under --json it must go to a file, never to stdout.
    if out_path == "-" {
        return Err(Fail {
            code: Code::Usage,
            message: String::from(
                "binary output to stdout is not available under --json; give -o FILE",
            ),
            hint: Some(String::from(
                "run: cobalt --json build FILE.logos -o OUT.mbc",
            )),
            exit: 2,
        });
    }
    let prog = load_json(in_path)?;
    let compiled = cobalt::compile(&prog);
    elysium::verify(&compiled.module)
        .map_err(|err| Fail::op(Code::Invalid, format!("verify failed: {err:?}")))?;
    let bytes = elysium::encode(&compiled.module);
    fs::write(out_path, &bytes).map_err(|err| Fail::op(Code::Io, format!("{out_path}: {err}")))?;
    let data = Json::obj([
        ("path", Json::path(Path::new(out_path))),
        ("bytes", int(bytes.len())),
    ]);
    Ok((data, 0))
}

/// The input FILE and the `-o` output of `build`, the same rules as the text path.
fn build_args_json(args: &[String]) -> Result<(&str, &str), Fail> {
    let mut input: Option<&str> = None;
    let mut output: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-o" || a == "--output" {
            i += 1;
            let val = args
                .get(i)
                .map(String::as_str)
                .ok_or_else(|| Fail::usage(format!("missing value for {a}")))?;
            if output.replace(val).is_some() {
                return Err(Fail::usage(String::from("duplicate -o/--output option")));
            }
        } else if let Some(val) = a
            .strip_prefix("-o=")
            .or_else(|| a.strip_prefix("--output="))
        {
            if val.is_empty() {
                return Err(Fail::usage(String::from("empty value for output option")));
            }
            if output.replace(val).is_some() {
                return Err(Fail::usage(String::from("duplicate -o/--output option")));
            }
        } else if a != "-" && a.starts_with('-') {
            return Err(Fail::usage(format!("unknown option '{a}'")));
        } else if input.is_none() {
            input = Some(a);
        } else {
            return Err(Fail::usage(format!("unexpected argument '{a}'")));
        }
        i += 1;
    }
    let input = input
        .ok_or_else(|| Fail::usage(String::from("usage: cobalt build FILE.logos -o OUT.mbc")))?;
    let output = output.ok_or_else(|| {
        Fail::usage(String::from(
            "missing -o OUT.mbc (usage: cobalt build FILE.logos -o OUT.mbc)",
        ))
    })?;
    Ok((input, output))
}
