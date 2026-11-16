//! Command-line tool for inspecting and rendering HTML/CSS documents with odyssey.
//!
//! Text mode prints the same lines as before. `--describe` prints the manifest, and `--json` prints one
//! ACI envelope (docs/ai-computer/ACI.md). Both modes share the parse, read, select and render code below.
#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use aci::envelope::{self, Code, DEFAULT_LIMIT, MAX_LIMIT};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
use odyssey::dom::{Data, Document, NodeId};
use odyssey::html::{self, Token};
use odyssey::{layout, paint, script, select, style};
use pigment::{encode_png, Canvas, Rgba};

const VERSION: &str = "odyssey 0.1.0";
const DEFAULT_WIDTH: u32 = 800;
/// Tree node text is cut at this many bytes (the node's `truncated` and `bytes` say so).
const NODE_TEXT_CAP: usize = 400;

const HELP: &str = "\
Usage: odyssey <subcommand> [options] [args]

Subcommands:
  tokens FILE.html                       Print HTML tokens, one per line
  tree FILE.html                         Print indented DOM tree
  select SEL FILE.html                   Print matching elements with their path
  render FILE.html [--width N] --out PNG Render page to a PNG image (default width: 800)

Options:
  -h, --help                             Show this help message
  -V, --version                          Show version

Use '-' for FILE.html to read stdin, or '--out -' to write PNG bytes to stdout.

Examples:
  odyssey tokens page.html
  odyssey tree page.html
  odyssey select \"div > p.lead\" page.html
  odyssey render page.html --width 800 --out page.png
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

const fn opt(name: &'static str, kind: ArgKind, default: &'static str) -> Arg {
    Arg {
        name,
        kind,
        required: false,
        default: Some(default),
        choices: &[],
    }
}

const PAGE_FILE_ARGS: &[Arg] = &[
    req("FILE", ArgKind::Path),
    opt("--limit", ArgKind::Int, "100"),
    opt("--offset", ArgKind::Int, "0"),
];
const SELECT_ARGS: &[Arg] = &[
    req("SEL", ArgKind::Text),
    req("FILE", ArgKind::Path),
    opt("--limit", ArgKind::Int, "100"),
    opt("--offset", ArgKind::Int, "0"),
];
const RENDER_ARGS: &[Arg] = &[
    req("FILE", ArgKind::Path),
    opt("--width", ArgKind::Int, "800"),
    req("--out", ArgKind::Path),
];

const COMMANDS: &[Command] = &[
    Command {
        name: "tokens",
        summary: "Print HTML tokens of FILE; --limit and --offset page them (--json only)",
        effect: Effect::Read,
        args: PAGE_FILE_ARGS,
        returns: "items: [{kind, text}] (start_tag and end_tag add name, start_tag adds attrs: [{name, text}] and self_closing), total, more, next_offset",
        example: "odyssey --json tokens page.html",
    },
    Command {
        name: "tree",
        summary: "Print the DOM tree of FILE in document order; --limit and --offset page it (--json only)",
        effect: Effect::Read,
        args: PAGE_FILE_ARGS,
        returns: "nodes: [{depth, kind, name, text, attrs}] in document order, total, more, next_offset; node text is cut at 400 bytes with truncated and bytes",
        example: "odyssey --json tree page.html",
    },
    Command {
        name: "select",
        summary: "Print elements of FILE matching SEL with their path; --limit and --offset page them (--json only)",
        effect: Effect::Read,
        args: SELECT_ARGS,
        returns: "items: [{path, name, attrs, text}], total, more, next_offset; a bad selector is code invalid",
        example: "odyssey --json select \"div > p\" page.html",
    },
    Command {
        name: "render",
        summary: "Render FILE to a PNG at --out FILE (default width 800)",
        effect: Effect::Write,
        args: RENDER_ARGS,
        returns: "path, bytes, width, height (--out - is refused under --json)",
        example: "odyssey --json render page.html --width 800 --out page.png",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "odyssey",
    version: env!("CARGO_PKG_VERSION"),
    summary: "HTML and CSS inspector and renderer",
    commands: COMMANDS,
};

/// A failed run: the exit code the text path uses, the envelope code, the message and an optional hint.
struct Fail {
    exit: u8,
    code: Code,
    message: String,
    hint: Option<&'static str>,
}

type Res<T> = Result<T, Fail>;

fn usage(message: impl Into<String>) -> Fail {
    Fail {
        exit: 2,
        code: Code::Usage,
        message: message.into(),
        hint: None,
    }
}

fn failure(code: Code, message: impl Into<String>) -> Fail {
    Fail {
        exit: 1,
        code,
        message: message.into(),
        hint: None,
    }
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
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(fail) => {
            eprintln!("odyssey: {}", fail.message);
            ExitCode::from(fail.exit)
        }
    }
}

fn run(args: &[String]) -> Res<()> {
    let Some(first) = args.first() else {
        return Err(usage("missing subcommand (try --help)"));
    };

    match first.as_str() {
        "-h" | "--help" => {
            print!("{HELP}");
            Ok(())
        }
        "-V" | "--version" => {
            println!("{VERSION}");
            Ok(())
        }
        "tokens" => cmd_tokens(&args[1..]),
        "tree" => cmd_tree(&args[1..]),
        "select" => cmd_select(&args[1..]),
        "render" => cmd_render(&args[1..]),
        other if other.starts_with('-') => Err(usage(format!("unknown option '{other}'"))),
        other => Err(usage(format!("unknown subcommand '{other}'"))),
    }
}

/// `odyssey --json ...`: one envelope on stdout; stderr keeps the `odyssey: message` line.
fn run_json(args: &[OsString]) -> ExitCode {
    let args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cmd = match args.first().map(String::as_str) {
        Some("tokens") => "tokens",
        Some("tree") => "tree",
        Some("select") => "select",
        Some("render") => "render",
        Some("-h" | "--help") => "--help",
        Some("-V" | "--version") => "--version",
        _ => "",
    };
    match json_command(&args) {
        Ok(data) => {
            print!("{}", envelope::ok("odyssey", cmd, data));
            ExitCode::SUCCESS
        }
        Err(fail) => {
            eprintln!("odyssey: {}", fail.message);
            let hint = match (fail.hint, fail.code) {
                (Some(hint), _) => Some(hint),
                (None, Code::Usage) => Some("run: odyssey --describe"),
                (None, _) => None,
            };
            print!(
                "{}",
                envelope::err("odyssey", cmd, fail.code, &fail.message, hint)
            );
            ExitCode::from(fail.exit)
        }
    }
}

fn json_command(args: &[String]) -> Res<Json> {
    match args.first().map(String::as_str) {
        None => Err(usage("missing subcommand (try --help)")),
        Some("-h" | "--help") => Ok(Json::obj([("usage", Json::Str(HELP.into()))])),
        Some("-V" | "--version") => Ok(Json::obj([("version", Json::Str(VERSION.into()))])),
        Some("tokens") => json_tokens(&args[1..]),
        Some("tree") => json_tree(&args[1..]),
        Some("select") => json_select(&args[1..]),
        Some("render") => json_render(&args[1..]),
        Some(other) if other.starts_with('-') => Err(usage(format!("unknown option '{other}'"))),
        Some(other) => Err(usage(format!("unknown subcommand '{other}'"))),
    }
}

fn has_help_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "-h" || a == "--help")
}

/// Refuse an option-looking path; `-` alone is stdin.
fn check_path_arg(path: &str) -> Res<()> {
    if path.starts_with('-') && path != "-" {
        return Err(usage(format!("unknown option '{path}'")));
    }
    Ok(())
}

fn read_input(path: &str) -> Res<String> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| failure(Code::Io, format!("stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read_to_string(path).map_err(|e| {
            let code = if e.kind() == io::ErrorKind::NotFound {
                Code::NotFound
            } else {
                Code::Io
            };
            failure(code, format!("{path}: {e}"))
        })
    }
}

fn escape_inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn format_attrs(attrs: &[(String, String)]) -> String {
    let mut out = String::new();
    for (k, v) in attrs {
        out.push(' ');
        out.push_str(&escape_inline(k));
        out.push_str("=\"");
        out.push_str(&escape_inline(v));
        out.push('"');
    }
    out
}

fn format_token(tok: &Token) -> String {
    match tok {
        Token::Doctype(name) => format!("DOCTYPE {}", escape_inline(name)),
        Token::StartTag {
            name,
            attrs,
            self_closing,
        } => {
            let mut line = format!("START {}", escape_inline(name));
            line.push_str(&format_attrs(attrs));
            if *self_closing {
                line.push_str(" /");
            }
            line
        }
        Token::EndTag(name) => format!("END {}", escape_inline(name)),
        Token::Text(text) => format!("TEXT \"{}\"", escape_inline(text)),
        Token::Comment(text) => format!("COMMENT \"{}\"", escape_inline(text)),
    }
}

fn cmd_tokens(args: &[String]) -> Res<()> {
    if has_help_flag(args) {
        print!("{HELP}");
        return Ok(());
    }
    if args.len() != 1 {
        return Err(usage("usage: odyssey tokens FILE.html"));
    }
    let path = &args[0];
    check_path_arg(path)?;
    let src = read_input(path)?;
    let out = io::stdout();
    let mut lock = out.lock();
    for tok in html::tokenize(&src) {
        writeln!(lock, "{}", format_token(&tok))
            .map_err(|e| failure(Code::Io, format!("stdout: {e}")))?;
    }
    Ok(())
}

fn cmd_tree(args: &[String]) -> Res<()> {
    if has_help_flag(args) {
        print!("{HELP}");
        return Ok(());
    }
    if args.len() != 1 {
        return Err(usage("usage: odyssey tree FILE.html"));
    }
    let path = &args[0];
    check_path_arg(path)?;
    let src = read_input(path)?;
    let doc = html::parse(&src);
    let dumped = doc.dump();
    io::stdout()
        .write_all(dumped.as_bytes())
        .map_err(|e| failure(Code::Io, format!("stdout: {e}")))
}

fn element_path(doc: &Document, target: NodeId) -> String {
    let mut chain = Vec::new();
    let mut cur = Some(target);
    let limit = doc.nodes.len();
    let mut steps = 0usize;
    while let Some(id) = cur {
        if steps > limit {
            break;
        }
        steps += 1;
        let Some(node) = doc.nodes.get(id) else {
            break;
        };
        if let Data::Element { name, .. } = &node.data {
            let mut seg = name.clone();
            if let Some(pid) = node.parent {
                if let Some(parent) = doc.nodes.get(pid) {
                    let mut idx = 0usize;
                    let mut total = 0usize;
                    for &cid in &parent.children {
                        if doc.name(cid) == Some(name.as_str()) {
                            total += 1;
                            if cid == id {
                                idx = total;
                            }
                        }
                    }
                    if total > 1 && idx > 0 {
                        use std::fmt::Write as _;
                        let _ = write!(seg, "[{idx}]");
                    }
                }
            }
            chain.push(seg);
        }
        cur = node.parent;
    }
    chain.reverse();
    format!("/{}", chain.join("/"))
}

fn format_element_tag(doc: &Document, id: NodeId) -> String {
    match doc.nodes.get(id).map(|n| &n.data) {
        Some(Data::Element { name, attrs }) => {
            format!("<{}{}>", escape_inline(name), format_attrs(attrs))
        }
        _ => String::new(),
    }
}

fn cmd_select(args: &[String]) -> Res<()> {
    if has_help_flag(args) {
        print!("{HELP}");
        return Ok(());
    }
    if args.len() != 2 {
        return Err(usage("usage: odyssey select SEL FILE.html"));
    }
    let sel = &args[0];
    let path = &args[1];
    check_path_arg(path)?;
    let src = read_input(path)?;
    let doc = html::parse(&src);
    let matches = select_nodes(&doc, sel)?;
    let out = io::stdout();
    let mut lock = out.lock();
    for id in matches {
        writeln!(
            lock,
            "{} {}",
            element_path(&doc, id),
            format_element_tag(&doc, id)
        )
        .map_err(|e| failure(Code::Io, format!("stdout: {e}")))?;
    }
    Ok(())
}

/// The elements matching `sel`, shared by the text and JSON paths.
fn select_nodes(doc: &Document, sel: &str) -> Res<Vec<NodeId>> {
    select::select(doc, sel).map_err(|_| failure(Code::Invalid, format!("invalid selector: {sel}")))
}

fn parse_width(raw: &str) -> Res<u32> {
    let w: u32 = raw
        .parse()
        .map_err(|_| usage(format!("invalid width '{raw}'")))?;
    if w == 0 || i32::try_from(w).is_err() {
        return Err(usage(format!("width out of range: '{raw}'")));
    }
    Ok(w)
}

/// The arguments of `render`, after the checks both modes share.
struct RenderArgs {
    file: String,
    out: String,
    width: u32,
}

fn parse_render(args: &[String]) -> Res<RenderArgs> {
    let mut file_path: Option<String> = None;
    let mut out_path: Option<String> = None;
    let mut width: u32 = DEFAULT_WIDTH;
    let mut i = 0usize;

    while i < args.len() {
        let arg = &args[i];
        if arg == "--width" {
            let Some(val) = args.get(i + 1) else {
                return Err(usage("--width requires a value"));
            };
            width = parse_width(val)?;
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--width=") {
            width = parse_width(val)?;
            i += 1;
        } else if arg == "--out" {
            let Some(val) = args.get(i + 1) else {
                return Err(usage("--out requires a path"));
            };
            out_path = Some(val.clone());
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--out=") {
            out_path = Some(val.to_string());
            i += 1;
        } else if arg.starts_with('-') && arg != "-" {
            return Err(usage(format!("unknown option '{arg}'")));
        } else if file_path.is_none() {
            file_path = Some(arg.clone());
            i += 1;
        } else {
            return Err(usage(format!("unexpected argument '{arg}'")));
        }
    }

    let Some(file) = file_path else {
        return Err(usage(
            "usage: odyssey render FILE.html [--width N] --out PAGE.png",
        ));
    };
    let Some(out) = out_path else {
        return Err(usage("missing required --out option"));
    };
    Ok(RenderArgs { file, out, width })
}

/// Read, script, style, lay out and paint a page; returns the PNG bytes and the page height.
fn render_page(file: &str, width: u32) -> Res<(Vec<u8>, u32)> {
    let src = read_input(file)?;
    let mut doc = html::parse(&src);
    let _ = script::run_page_scripts(&mut doc, script::DEFAULT_FUEL);
    let styles = style::compute(&doc);
    let width_i32 =
        i32::try_from(width).map_err(|_| usage(format!("width out of range: {width}")))?;
    let laid_out = layout::layout(&doc, &styles, width_i32);
    let height_u32 = u32::try_from(laid_out.height.max(1))
        .map_err(|_| failure(Code::Invalid, "rendered page height overflows u32"))?;

    let mut canvas = Canvas::new(width, height_u32, Rgba(255, 255, 255, 255));
    if canvas.width == 0 || canvas.height == 0 {
        return Err(failure(
            Code::Internal,
            format!("failed to allocate canvas ({width}x{height_u32})"),
        ));
    }
    paint::paint(&doc, &styles, &laid_out, &mut canvas);
    Ok((encode_png(&canvas), height_u32))
}

fn cmd_render(args: &[String]) -> Res<()> {
    if has_help_flag(args) {
        print!("{HELP}");
        return Ok(());
    }

    let parsed = parse_render(args)?;
    let (png, height) = render_page(&parsed.file, parsed.width)?;

    if parsed.out == "-" {
        io::stdout()
            .write_all(&png)
            .map_err(|e| failure(Code::Io, format!("stdout: {e}")))?;
    } else {
        fs::write(&parsed.out, &png)
            .map_err(|e| failure(Code::Io, format!("{}: {e}", parsed.out)))?;
        println!("{}: {}x{height}", parsed.out, parsed.width);
    }
    Ok(())
}

// ---------- --json ----------

/// `--limit N` and `--offset N`, pulled out of the arguments before the positional checks.
struct Paging {
    limit: usize,
    offset: usize,
}

fn take_paging(args: &[String]) -> Res<(Vec<String>, Paging)> {
    let mut rest = Vec::new();
    let mut page = Paging {
        limit: DEFAULT_LIMIT,
        offset: 0,
    };
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--limit" || arg == "--offset" {
            let Some(raw) = args.get(i + 1) else {
                return Err(usage(format!("{arg} requires a value")));
            };
            let n: usize = raw
                .parse()
                .map_err(|_| usage(format!("invalid value for {arg}: '{raw}'")))?;
            if arg == "--limit" {
                if n == 0 || n > MAX_LIMIT {
                    return Err(usage(format!("--limit must be 1 to {MAX_LIMIT}, got {n}")));
                }
                page.limit = n;
            } else {
                page.offset = n;
            }
            i += 2;
        } else {
            rest.push(args[i].clone());
            i += 1;
        }
    }
    Ok((rest, page))
}

fn field(key: &str, value: Json) -> (String, Json) {
    (key.to_string(), value)
}

fn text(value: &str) -> Json {
    Json::Str(value.to_string())
}

fn int(n: usize) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

/// An object with `pairs` followed by the capped `text` fields (ACI.md, text cap).
fn with_text(mut pairs: Vec<(String, Json)>, value: &[u8]) -> Json {
    pairs.extend(envelope::capped(value));
    Json::Obj(pairs)
}

/// Rename the `items` key of a page to `key` (the tree uses `nodes`).
fn paged(key: &str, items: Vec<Json>, total: usize, page: &Paging) -> Json {
    match envelope::page(items, total, page.offset, page.limit) {
        Json::Obj(pairs) => Json::Obj(
            pairs
                .into_iter()
                .map(|(k, v)| if k == "items" { field(key, v) } else { (k, v) })
                .collect(),
        ),
        other => other,
    }
}

fn attrs_json(attrs: &[(String, String)]) -> Json {
    Json::Arr(
        attrs
            .iter()
            .map(|(k, v)| with_text(vec![field("name", text(k))], v.as_bytes()))
            .collect(),
    )
}

fn token_json(tok: &Token) -> Json {
    match tok {
        Token::Doctype(name) => with_text(vec![field("kind", text("doctype"))], name.as_bytes()),
        Token::StartTag {
            name,
            attrs,
            self_closing,
        } => Json::Obj(vec![
            field("kind", text("start_tag")),
            field("name", text(name)),
            field("attrs", attrs_json(attrs)),
            field("self_closing", Json::Bool(*self_closing)),
            field("text", text("")),
        ]),
        Token::EndTag(name) => Json::Obj(vec![
            field("kind", text("end_tag")),
            field("name", text(name)),
            field("text", text("")),
        ]),
        Token::Text(value) => with_text(vec![field("kind", text("text"))], value.as_bytes()),
        Token::Comment(value) => with_text(vec![field("kind", text("comment"))], value.as_bytes()),
    }
}

/// Node text, cut at `NODE_TEXT_CAP` bytes; a cut node carries `truncated` and the full `bytes`.
fn node_text(value: &str) -> Vec<(String, Json)> {
    if value.len() <= NODE_TEXT_CAP {
        return envelope::capped(value.as_bytes());
    }
    let mut cut = NODE_TEXT_CAP;
    while !value.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut fields = envelope::capped(&value.as_bytes()[..cut]);
    fields.push(field("truncated", Json::Bool(true)));
    fields.push(field("bytes", int(value.len())));
    fields
}

fn node_json(doc: &Document, depth: usize, id: NodeId) -> Json {
    let (kind, name, value, attrs): (&str, &str, &str, Json) =
        match doc.nodes.get(id).map(|n| &n.data) {
            Some(Data::Doctype(name)) => ("doctype", name, "", Json::Arr(vec![])),
            Some(Data::Element { name, attrs }) => ("element", name, "", attrs_json(attrs)),
            Some(Data::Text(value)) => ("text", "", value, Json::Arr(vec![])),
            Some(Data::Comment(value)) => ("comment", "", value, Json::Arr(vec![])),
            Some(Data::Document) | None => ("document", "", "", Json::Arr(vec![])),
        };
    let mut pairs = vec![
        field("depth", int(depth)),
        field("kind", text(kind)),
        field("name", text(name)),
        field("attrs", attrs),
    ];
    pairs.extend(node_text(value));
    Json::Obj(pairs)
}

/// The text of an element and everything below it, in document order.
fn text_content(doc: &Document, root: NodeId) -> String {
    let mut out = String::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let Some(node) = doc.nodes.get(id) else {
            continue;
        };
        if let Data::Text(value) = &node.data {
            out.push_str(value);
        }
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    out
}

fn select_json(doc: &Document, id: NodeId) -> Json {
    let name = doc.name(id).unwrap_or("");
    let attrs = match doc.nodes.get(id).map(|n| &n.data) {
        Some(Data::Element { attrs, .. }) => attrs_json(attrs),
        _ => Json::Arr(vec![]),
    };
    with_text(
        vec![
            field("path", text(&element_path(doc, id))),
            field("name", text(name)),
            field("attrs", attrs),
        ],
        text_content(doc, id).as_bytes(),
    )
}

/// The single FILE argument of a paged command, after the paging flags are removed.
fn one_file<'a>(rest: &'a [String], usage_line: &str) -> Res<&'a str> {
    match rest {
        [path] => {
            check_path_arg(path)?;
            Ok(path.as_str())
        }
        _ => Err(usage(usage_line)),
    }
}

fn json_tokens(args: &[String]) -> Res<Json> {
    let (rest, page) = take_paging(args)?;
    let path = one_file(&rest, "usage: odyssey tokens FILE.html")?;
    let src = read_input(path)?;
    let tokens = html::tokenize(&src);
    let total = tokens.len();
    let items = tokens
        .iter()
        .skip(page.offset)
        .take(page.limit)
        .map(token_json)
        .collect();
    Ok(envelope::page(items, total, page.offset, page.limit))
}

fn json_tree(args: &[String]) -> Res<Json> {
    let (rest, page) = take_paging(args)?;
    let path = one_file(&rest, "usage: odyssey tree FILE.html")?;
    let src = read_input(path)?;
    let doc = html::parse(&src);
    let walk = doc.walk();
    let total = walk.len();
    let nodes = walk
        .iter()
        .skip(page.offset)
        .take(page.limit)
        .map(|(depth, id)| node_json(&doc, *depth, *id))
        .collect();
    Ok(paged("nodes", nodes, total, &page))
}

fn json_select(args: &[String]) -> Res<Json> {
    let (rest, page) = take_paging(args)?;
    let [sel, path] = rest.as_slice() else {
        return Err(usage("usage: odyssey select SEL FILE.html"));
    };
    check_path_arg(path)?;
    let src = read_input(path)?;
    let doc = html::parse(&src);
    let matches = select_nodes(&doc, sel)?;
    let total = matches.len();
    let items = matches
        .iter()
        .skip(page.offset)
        .take(page.limit)
        .map(|id| select_json(&doc, *id))
        .collect();
    Ok(envelope::page(items, total, page.offset, page.limit))
}

fn json_render(args: &[String]) -> Res<Json> {
    if has_help_flag(args) {
        return Ok(Json::obj([("usage", Json::Str(HELP.into()))]));
    }
    let parsed = parse_render(args)?;
    if parsed.out == "-" {
        return Err(Fail {
            exit: 2,
            code: Code::Usage,
            message: String::from("--json needs --out FILE.png; PNG bytes cannot go to stdout"),
            hint: Some("use --out FILE.png"),
        });
    }
    let (png, height) = render_page(&parsed.file, parsed.width)?;
    fs::write(&parsed.out, &png).map_err(|e| failure(Code::Io, format!("{}: {e}", parsed.out)))?;
    Ok(Json::obj([
        ("path", Json::path(std::path::Path::new(&parsed.out))),
        ("bytes", int(png.len())),
        ("width", int(parsed.width as usize)),
        ("height", int(height as usize)),
    ]))
}
