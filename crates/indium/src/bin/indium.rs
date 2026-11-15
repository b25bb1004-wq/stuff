//! Command-line tool for rendering `indium` layouts and inspecting focus order.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

use indium::{Action, Align, Canvas, Dir, Event, Insets, Key, Kind, NodeId, Style, Theme, Ui};
use pigment::encode_png;

const VERSION: &str = "indium 0.1.0";
const DEFAULT_WIDTH: u32 = 640;
const DEFAULT_HEIGHT: u32 = 480;
const MAX_LIST_ITEMS: usize = 10_000;

const HELP: &str = "\
indium - widget layout renderer and focus inspector

USAGE:
  indium render <LAYOUT> --out <UI.png> [--width W] [--height H] [--focus N]
  indium --help
  indium --version

SUBCOMMANDS:
  render    Parse an indented layout file, print focus order, and write a PNG.
            Use '-' for <LAYOUT> to read from stdin, or '-' for --out to write
            PNG bytes to stdout.

OPTIONS:
  --out <PATH>     Output PNG path (required for render)
  --width <W>      Canvas width in pixels (default: 640)
  --height <H>     Canvas height in pixels (default: 480)
  --focus <N>      Draw the focus ring on the N-th focusable widget (0-based)
  -h, --help       Show this help text
  -V, --version    Print version information

LAYOUT FORMAT:
  Space-indented text, one widget per line (blank lines and '#' lines ignored):
    column         Vertical flex container
    row            Horizontal flex container
    text \"...\"     Non-focusable text label
    button \"...\"   Focusable push button
    list N         Vertical list box of N focusable items (\"item 1\" .. \"item N\")

EXAMPLE:
  indium render layout.txt --out ui.png --width 480 --height 320 --focus 0
";

/// Exit code wrapper for CLI errors.
struct CliError {
    code: i32,
    message: String,
}

impl CliError {
    fn usage(msg: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: msg.into(),
        }
    }

    fn fail(msg: impl Into<String>) -> Self {
        Self {
            code: 1,
            message: msg.into(),
        }
    }
}

/// Parsed arguments for `indium render`.
struct RenderArgs {
    layout_path: String,
    out_path: String,
    width: u32,
    height: u32,
    focus: Option<usize>,
}

/// Metadata recorded for each created node so focus order lines are descriptive.
struct WidgetInfo {
    kind_name: &'static str,
    label: String,
}

/// Container frame on the indentation parser stack.
struct Frame {
    indent: isize,
    node_id: NodeId,
    child_indent: Option<usize>,
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(err) => {
            eprintln!("indium: {}", err.message);
            process::exit(err.code);
        }
    }
}

fn run() -> Result<(), CliError> {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return Err(CliError::usage("missing subcommand (try --help)"));
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
        "render" => {
            let rest: Vec<String> = args.collect();
            if rest.iter().any(|a| a == "-h" || a == "--help") {
                print!("{HELP}");
                return Ok(());
            }
            let parsed = parse_render_args(&rest)?;
            cmd_render(&parsed)
        }
        other => Err(CliError::usage(format!(
            "unknown subcommand '{other}' (try --help)"
        ))),
    }
}

fn parse_render_args(args: &[String]) -> Result<RenderArgs, CliError> {
    let mut layout_path: Option<String> = None;
    let mut out_path: Option<String> = None;
    let mut width = DEFAULT_WIDTH;
    let mut height = DEFAULT_HEIGHT;
    let mut focus: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if let Some(val) = arg.strip_prefix("--out=") {
            out_path = Some(val.to_string());
            i += 1;
        } else if let Some(val) = arg.strip_prefix("--width=") {
            width = parse_positive_u32("--width", val)?;
            i += 1;
        } else if let Some(val) = arg.strip_prefix("--height=") {
            height = parse_positive_u32("--height", val)?;
            i += 1;
        } else if let Some(val) = arg.strip_prefix("--focus=") {
            focus = Some(parse_usize("--focus", val)?);
            i += 1;
        } else if arg == "--out" {
            let val = take_value(args, &mut i, "--out")?;
            out_path = Some(val.to_string());
        } else if arg == "--width" {
            let val = take_value(args, &mut i, "--width")?;
            width = parse_positive_u32("--width", val)?;
        } else if arg == "--height" {
            let val = take_value(args, &mut i, "--height")?;
            height = parse_positive_u32("--height", val)?;
        } else if arg == "--focus" {
            let val = take_value(args, &mut i, "--focus")?;
            focus = Some(parse_usize("--focus", val)?);
        } else if arg != "-" && arg.starts_with('-') {
            return Err(CliError::usage(format!("unknown option '{arg}'")));
        } else if layout_path.is_none() {
            layout_path = Some(arg.clone());
            i += 1;
        } else {
            return Err(CliError::usage(format!("unexpected argument '{arg}'")));
        }
    }

    let layout_path = layout_path
        .ok_or_else(|| CliError::usage("missing <LAYOUT> argument for render (try --help)"))?;
    let out_path = out_path.ok_or_else(|| CliError::usage("missing --out <UI.png> for render"))?;

    Ok(RenderArgs {
        layout_path,
        out_path,
        width,
        height,
        focus,
    })
}

fn take_value<'a>(args: &'a [String], i: &mut usize, flag: &str) -> Result<&'a str, CliError> {
    let next_idx = *i + 1;
    let Some(val) = args.get(next_idx) else {
        return Err(CliError::usage(format!("missing value for {flag}")));
    };
    *i = next_idx + 1;
    Ok(val.as_str())
}

fn parse_positive_u32(flag: &str, val: &str) -> Result<u32, CliError> {
    let n: u32 = val
        .parse()
        .map_err(|_| CliError::usage(format!("invalid integer for {flag}: '{val}'")))?;
    if n == 0 {
        return Err(CliError::fail(format!("{flag} must be greater than zero")));
    }
    Ok(n)
}

fn parse_usize(flag: &str, val: &str) -> Result<usize, CliError> {
    val.parse()
        .map_err(|_| CliError::usage(format!("invalid index for {flag}: '{val}'")))
}

fn cmd_render(args: &RenderArgs) -> Result<(), CliError> {
    let source = read_layout(&args.layout_path)?;
    let (mut ui, info) = parse_layout(&source)?;

    let focus_order = collect_focus_order(&ui);
    if let Some(focus_idx) = args.focus {
        if focus_idx >= focus_order.len() {
            return Err(CliError::fail(format!(
                "focus index {focus_idx} out of range ({} focusable widgets)",
                focus_order.len()
            )));
        }
        for _ in 0..=focus_idx {
            ui.event(Event::Key(Key::Tab));
        }
    }

    ui.layout(args.width, args.height);
    let mut canvas = Canvas::new(args.width, args.height, Theme::dark().bg);
    if canvas.width != args.width || canvas.height != args.height {
        return Err(CliError::fail(format!(
            "cannot allocate {}x{} canvas",
            args.width, args.height
        )));
    }
    ui.render(&mut canvas);

    let png_bytes = encode_png(&canvas);
    write_output(&args.out_path, &png_bytes)?;

    if args.out_path != "-" {
        print_focus_order(&focus_order, &info, args.focus);
    }
    Ok(())
}

fn read_layout(path: &str) -> Result<String, CliError> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::fail(format!("failed to read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read_to_string(path)
            .map_err(|e| CliError::fail(format!("failed to read '{path}': {e}")))
    }
}

fn write_output(path: &str, bytes: &[u8]) -> Result<(), CliError> {
    if path == "-" {
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(bytes)
            .and_then(|()| stdout.flush())
            .map_err(|e| CliError::fail(format!("failed to write stdout: {e}")))
    } else {
        fs::write(path, bytes).map_err(|e| CliError::fail(format!("failed to write '{path}': {e}")))
    }
}

/// Walk `Ui` focus order using `Key::Tab` so order matches library dispatch exactly.
fn collect_focus_order(ui: &Ui) -> Vec<NodeId> {
    let mut probe = ui.clone();
    let mut order = Vec::new();
    loop {
        let actions = probe.event(Event::Key(Key::Tab));
        let focused = actions.into_iter().find_map(|a| match a {
            Action::Focused(id) => id,
            _ => None,
        });
        let Some(nid) = focused else {
            break;
        };
        if order.first().copied() == Some(nid) {
            break;
        }
        order.push(nid);
    }
    order
}

fn print_focus_order(order: &[NodeId], info: &HashMap<NodeId, WidgetInfo>, active: Option<usize>) {
    for (idx, nid) in order.iter().enumerate() {
        if let Some(meta) = info.get(nid) {
            println!("{idx}: {} \"{}\" (node {nid})", meta.kind_name, meta.label);
        } else {
            println!("{idx}: widget (node {nid})");
        }
    }
    if let Some(focus_idx) = active {
        if let Some(nid) = order.get(focus_idx) {
            println!("focused: {focus_idx} (node {nid})");
        }
    }
}

fn parse_layout(source: &str) -> Result<(Ui, HashMap<NodeId, WidgetInfo>), CliError> {
    let mut ui = Ui::new(Theme::dark());
    let mut info = HashMap::new();
    let mut stack = vec![Frame {
        indent: -1,
        node_id: ui.root(),
        child_indent: None,
    }];
    let mut widget_count = 0usize;

    for (line_idx, raw_line) in source.lines().enumerate() {
        let line_no = line_idx + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed_start = line.trim_start_matches(' ');
        if trimmed_start.starts_with('\t') {
            return Err(CliError::fail(format!(
                "line {line_no}: tabs are not allowed in indentation; use spaces"
            )));
        }
        if trimmed_start.is_empty() || trimmed_start.starts_with('#') {
            continue;
        }

        let indent = line.len() - trimmed_start.len();
        let indent_i = isize::try_from(indent)
            .map_err(|_| CliError::fail(format!("line {line_no}: indentation too deep")))?;

        while let Some(top) = stack.last() {
            if top.indent < indent_i {
                break;
            }
            stack.pop();
        }

        let Some(parent) = stack.last_mut() else {
            return Err(CliError::fail(format!(
                "line {line_no}: invalid indentation"
            )));
        };

        match parent.child_indent {
            Some(expected) if expected != indent => {
                return Err(CliError::fail(format!(
                    "line {line_no}: indentation mismatch (expected {expected} spaces, got {indent})"
                )));
            }
            None => parent.child_indent = Some(indent),
            Some(_) => {}
        }

        let parent_id = parent.node_id;
        let spec = trimmed_start.trim_end();
        let (kind, style, is_container, list_count, label_meta) = parse_widget_line(line_no, spec)?;

        let nid = ui.add(parent_id, kind, style).ok_or_else(|| {
            CliError::fail(format!("line {line_no}: cannot add child to non-container"))
        })?;
        widget_count += 1;

        if let Some((kind_name, label)) = label_meta {
            info.insert(nid, WidgetInfo { kind_name, label });
        }

        if let Some(count) = list_count {
            let item_style = Style {
                padding: Insets {
                    top: 4,
                    right: 8,
                    bottom: 4,
                    left: 8,
                },
                ..Style::default()
            };
            for idx in 1..=count {
                let item_label = format!("item {idx}");
                let item_id = ui
                    .add(nid, Kind::Button(item_label.clone()), item_style)
                    .ok_or_else(|| {
                        CliError::fail(format!("line {line_no}: failed to add list item"))
                    })?;
                info.insert(
                    item_id,
                    WidgetInfo {
                        kind_name: "list-item",
                        label: item_label,
                    },
                );
            }
        }

        if is_container {
            stack.push(Frame {
                indent: indent_i,
                node_id: nid,
                child_indent: None,
            });
        }
    }

    if widget_count == 0 {
        return Err(CliError::fail("layout contains no widgets"));
    }

    Ok((ui, info))
}

type ParsedWidget = (
    Kind,
    Style,
    bool,
    Option<usize>,
    Option<(&'static str, String)>,
);

fn parse_widget_line(line_no: usize, spec: &str) -> Result<ParsedWidget, CliError> {
    if spec == "column" {
        let style = Style {
            dir: Dir::Column,
            align: Align::Start,
            gap: 8,
            padding: Insets {
                top: 8,
                right: 8,
                bottom: 8,
                left: 8,
            },
            ..Style::default()
        };
        return Ok((Kind::Box, style, true, None, None));
    }

    if spec == "row" {
        let style = Style {
            dir: Dir::Row,
            align: Align::Center,
            gap: 8,
            padding: Insets {
                top: 8,
                right: 8,
                bottom: 8,
                left: 8,
            },
            ..Style::default()
        };
        return Ok((Kind::Box, style, true, None, None));
    }

    if let Some(rest) = spec.strip_prefix("text") {
        if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
            return Err(CliError::fail(format!(
                "line {line_no}: expected 'text \"...\"'"
            )));
        }
        let text = parse_quoted_arg(line_no, rest.trim())?;
        let style = Style {
            padding: Insets {
                top: 2,
                right: 4,
                bottom: 2,
                left: 4,
            },
            ..Style::default()
        };
        return Ok((Kind::Label(text), style, false, None, None));
    }

    if let Some(rest) = spec.strip_prefix("button") {
        if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
            return Err(CliError::fail(format!(
                "line {line_no}: expected 'button \"...\"'"
            )));
        }
        let text = parse_quoted_arg(line_no, rest.trim())?;
        let style = Style {
            padding: Insets {
                top: 6,
                right: 12,
                bottom: 6,
                left: 12,
            },
            ..Style::default()
        };
        return Ok((
            Kind::Button(text.clone()),
            style,
            false,
            None,
            Some(("button", text)),
        ));
    }

    if let Some(rest) = spec.strip_prefix("list") {
        if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
            return Err(CliError::fail(format!("line {line_no}: expected 'list N'")));
        }
        let count_str = rest.trim();
        let count: usize = count_str.parse().map_err(|_| {
            CliError::fail(format!(
                "line {line_no}: invalid list item count '{count_str}'"
            ))
        })?;
        if count > MAX_LIST_ITEMS {
            return Err(CliError::fail(format!(
                "line {line_no}: list count {count} exceeds maximum {MAX_LIST_ITEMS}"
            )));
        }
        let style = Style {
            dir: Dir::Column,
            align: Align::Stretch,
            gap: 4,
            padding: Insets {
                top: 6,
                right: 6,
                bottom: 6,
                left: 6,
            },
            min_w: 140,
            ..Style::default()
        };
        return Ok((Kind::Box, style, false, Some(count), None));
    }

    Err(CliError::fail(format!(
        "line {line_no}: unknown widget '{spec}'"
    )))
}

fn parse_quoted_arg(line_no: usize, raw: &str) -> Result<String, CliError> {
    let mut chars = raw.chars();
    if chars.next() != Some('"') {
        return Err(CliError::fail(format!(
            "line {line_no}: expected quoted string"
        )));
    }
    let mut out = String::new();
    let mut closed = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                closed = true;
                break;
            }
            '\\' => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    return Err(CliError::fail(format!(
                        "line {line_no}: unknown escape '\\{other}'"
                    )));
                }
                None => {
                    return Err(CliError::fail(format!(
                        "line {line_no}: unterminated escape in string"
                    )));
                }
            },
            other => out.push(other),
        }
    }
    if !closed {
        return Err(CliError::fail(format!(
            "line {line_no}: unterminated string literal"
        )));
    }
    let trailing = chars.as_str().trim();
    if !trailing.is_empty() && !trailing.starts_with('#') {
        return Err(CliError::fail(format!(
            "line {line_no}: unexpected trailing tokens after string: '{trailing}'"
        )));
    }
    Ok(out)
}
