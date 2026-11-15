//! Command-line tool for inspecting and rendering HTML/CSS documents with odyssey.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use odyssey::dom::{Data, Document, NodeId};
use odyssey::html::{self, Token};
use odyssey::{layout, paint, script, select, style};
use pigment::{encode_png, Canvas, Rgba};

const VERSION: &str = "odyssey 0.1.0";
const DEFAULT_WIDTH: u32 = 800;

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

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, msg)) => {
            eprintln!("odyssey: {msg}");
            ExitCode::from(code)
        }
    }
}

fn run(args: &[String]) -> Result<(), (u8, String)> {
    let Some(first) = args.first() else {
        return Err((2, String::from("missing subcommand (try --help)")));
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
        other if other.starts_with('-') => Err((2, format!("unknown option '{other}'"))),
        other => Err((2, format!("unknown subcommand '{other}'"))),
    }
}

fn has_help_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "-h" || a == "--help")
}

fn read_input(path: &str) -> Result<String, (u8, String)> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| (1, format!("stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read_to_string(path).map_err(|e| (1, format!("{path}: {e}")))
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

fn cmd_tokens(args: &[String]) -> Result<(), (u8, String)> {
    if has_help_flag(args) {
        print!("{HELP}");
        return Ok(());
    }
    if args.len() != 1 {
        return Err((2, String::from("usage: odyssey tokens FILE.html")));
    }
    let path = &args[0];
    if path.starts_with('-') && path != "-" {
        return Err((2, format!("unknown option '{path}'")));
    }
    let src = read_input(path)?;
    let out = io::stdout();
    let mut lock = out.lock();
    for tok in html::tokenize(&src) {
        writeln!(lock, "{}", format_token(&tok)).map_err(|e| (1, format!("stdout: {e}")))?;
    }
    Ok(())
}

fn cmd_tree(args: &[String]) -> Result<(), (u8, String)> {
    if has_help_flag(args) {
        print!("{HELP}");
        return Ok(());
    }
    if args.len() != 1 {
        return Err((2, String::from("usage: odyssey tree FILE.html")));
    }
    let path = &args[0];
    if path.starts_with('-') && path != "-" {
        return Err((2, format!("unknown option '{path}'")));
    }
    let src = read_input(path)?;
    let doc = html::parse(&src);
    let dumped = doc.dump();
    io::stdout()
        .write_all(dumped.as_bytes())
        .map_err(|e| (1, format!("stdout: {e}")))
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

fn cmd_select(args: &[String]) -> Result<(), (u8, String)> {
    if has_help_flag(args) {
        print!("{HELP}");
        return Ok(());
    }
    if args.len() != 2 {
        return Err((2, String::from("usage: odyssey select SEL FILE.html")));
    }
    let sel = &args[0];
    let path = &args[1];
    if path.starts_with('-') && path != "-" {
        return Err((2, format!("unknown option '{path}'")));
    }
    let src = read_input(path)?;
    let doc = html::parse(&src);
    let matches = select::select(&doc, sel).map_err(|_| (1, format!("invalid selector: {sel}")))?;
    let out = io::stdout();
    let mut lock = out.lock();
    for id in matches {
        writeln!(
            lock,
            "{} {}",
            element_path(&doc, id),
            format_element_tag(&doc, id)
        )
        .map_err(|e| (1, format!("stdout: {e}")))?;
    }
    Ok(())
}

fn parse_width(raw: &str) -> Result<u32, (u8, String)> {
    let w: u32 = raw
        .parse()
        .map_err(|_| (2, format!("invalid width '{raw}'")))?;
    if w == 0 || i32::try_from(w).is_err() {
        return Err((2, format!("width out of range: '{raw}'")));
    }
    Ok(w)
}

fn cmd_render(args: &[String]) -> Result<(), (u8, String)> {
    if has_help_flag(args) {
        print!("{HELP}");
        return Ok(());
    }

    let mut file_path: Option<String> = None;
    let mut out_path: Option<String> = None;
    let mut width: u32 = DEFAULT_WIDTH;
    let mut i = 0usize;

    while i < args.len() {
        let arg = &args[i];
        if arg == "--width" {
            let Some(val) = args.get(i + 1) else {
                return Err((2, String::from("--width requires a value")));
            };
            width = parse_width(val)?;
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--width=") {
            width = parse_width(val)?;
            i += 1;
        } else if arg == "--out" {
            let Some(val) = args.get(i + 1) else {
                return Err((2, String::from("--out requires a path")));
            };
            out_path = Some(val.clone());
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--out=") {
            out_path = Some(val.to_string());
            i += 1;
        } else if arg.starts_with('-') && arg != "-" {
            return Err((2, format!("unknown option '{arg}'")));
        } else if file_path.is_none() {
            file_path = Some(arg.clone());
            i += 1;
        } else {
            return Err((2, format!("unexpected argument '{arg}'")));
        }
    }

    let Some(file_path) = file_path else {
        return Err((
            2,
            String::from("usage: odyssey render FILE.html [--width N] --out PAGE.png"),
        ));
    };
    let Some(out_path) = out_path else {
        return Err((2, String::from("missing required --out option")));
    };

    let src = read_input(&file_path)?;
    let mut doc = html::parse(&src);
    let _ = script::run_page_scripts(&mut doc, script::DEFAULT_FUEL);
    let styles = style::compute(&doc);
    let width_i32 =
        i32::try_from(width).map_err(|_| (2, format!("width out of range: {width}")))?;
    let laid_out = layout::layout(&doc, &styles, width_i32);
    let height_u32 = u32::try_from(laid_out.height.max(1))
        .map_err(|_| (1, String::from("rendered page height overflows u32")))?;

    let mut canvas = Canvas::new(width, height_u32, Rgba(255, 255, 255, 255));
    if canvas.width == 0 || canvas.height == 0 {
        return Err((
            1,
            format!("failed to allocate canvas ({width}x{height_u32})"),
        ));
    }
    paint::paint(&doc, &styles, &laid_out, &mut canvas);
    let png = encode_png(&canvas);

    if out_path == "-" {
        io::stdout()
            .write_all(&png)
            .map_err(|e| (1, format!("stdout: {e}")))?;
    } else {
        fs::write(&out_path, &png).map_err(|e| (1, format!("{out_path}: {e}")))?;
        println!("{out_path}: {width}x{height_u32}");
    }
    Ok(())
}
