//! Indium widget layout and focus ring rendering demo.

use indium::{Canvas, Dir, Event, Insets, Key, Kind, NodeId, Style, Theme, Ui};

use super::{canvas_to_output, check_input, Demo, Output};

const CANVAS_W: u32 = 480;
const CANVAS_H: u32 = 320;
const MAX_LINES: usize = 128;
const MAX_LIST_ITEMS: usize = 64;

const DEFAULT_LAYOUT: &str = "\
column
  text \"Indium GUI Toolkit\"
  row
    button \"Open\"
    button \"Save\"
    button \"Run\"
  list 3
focus 1";

/// Registered Indium widget layout and focus ring demo.
pub const DEMO: Demo = Demo {
    id: "indium",
    title: "Indium GUI",
    default_input: DEFAULT_LAYOUT,
    run,
};

struct Frame {
    indent: usize,
    node_id: NodeId,
}

/// Parse an Indium layout script (`column`, `row`, `text`, `button`, `list N`, `focus N`) and render RGBA pixels.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let mut ui = Ui::new(Theme::light());
    let root = ui.root();

    let mut stack: Vec<Frame> = Vec::new();
    let mut focus_idx: Option<usize> = None;
    let mut focusable_count: usize = 0;
    let mut widget_count: usize = 0;
    let mut lines_seen: usize = 0;

    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        if raw.contains('\t') {
            return Err(format!(
                "line {line_no}: tabs are not allowed in indentation"
            ));
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        lines_seen += 1;
        if lines_seen > MAX_LINES {
            return Err(format!("line {line_no}: exceeds {MAX_LINES} lines"));
        }

        let indent = raw.bytes().take_while(|&b| b == b' ').count();
        if let Some(rest) = trimmed.strip_prefix("focus") {
            if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
                return Err(format!("line {line_no}: expected `focus N`"));
            }
            if focus_idx.is_some() {
                return Err(format!("line {line_no}: duplicate `focus` directive"));
            }
            let n = rest
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("line {line_no}: invalid focus index `{}`", rest.trim()))?;
            focus_idx = Some(n);
            continue;
        }
        if focus_idx.is_some() {
            return Err(format!("line {line_no}: `focus N` must be the final line"));
        }

        while stack.last().is_some_and(|top| top.indent >= indent) {
            stack.pop();
        }
        let parent = stack.last().map_or(root, |f| f.node_id);

        if trimmed == "column" {
            let id = add_box(&mut ui, parent, Dir::Column, line_no)?;
            stack.push(Frame {
                indent,
                node_id: id,
            });
            widget_count += 1;
        } else if trimmed == "row" {
            let id = add_box(&mut ui, parent, Dir::Row, line_no)?;
            stack.push(Frame {
                indent,
                node_id: id,
            });
            widget_count += 1;
        } else if let Some(rest) = trimmed.strip_prefix("text") {
            let label = parse_quoted_arg(rest, "text", line_no)?;
            add_leaf(&mut ui, parent, Kind::Label(label), 2, 4, line_no)?;
            widget_count += 1;
        } else if let Some(rest) = trimmed.strip_prefix("button") {
            let label = parse_quoted_arg(rest, "button", line_no)?;
            add_leaf(&mut ui, parent, Kind::Button(label), 6, 10, line_no)?;
            focusable_count += 1;
            widget_count += 1;
        } else if let Some(rest) = trimmed.strip_prefix("list") {
            let n = parse_list_count(rest, line_no)?;
            let list_id = add_box(&mut ui, parent, Dir::Column, line_no)?;
            for i in 1..=n {
                add_leaf(
                    &mut ui,
                    list_id,
                    Kind::Button(format!("item {i}")),
                    4,
                    8,
                    line_no,
                )?;
                focusable_count += 1;
            }
            widget_count += 1;
        } else {
            return Err(format!("line {line_no}: unknown widget `{trimmed}`"));
        }
    }

    if widget_count == 0 {
        return Err(String::from("layout contains no widgets"));
    }

    if let Some(target) = focus_idx {
        if target >= focusable_count {
            return Err(format!(
                "focus index {target} out of range (only {focusable_count} focusable widgets)"
            ));
        }
        for _ in 0..=target {
            ui.event(Event::Key(Key::Tab));
        }
    }

    ui.layout(CANVAS_W, CANVAS_H);
    let mut canvas = Canvas::new(CANVAS_W, CANVAS_H, Theme::light().bg);
    ui.render(&mut canvas);
    Ok(canvas_to_output(canvas))
}

fn add_box(ui: &mut Ui, parent: NodeId, dir: Dir, line_no: usize) -> Result<NodeId, String> {
    let style = Style {
        dir,
        gap: 6,
        padding: Insets {
            top: 8,
            right: 8,
            bottom: 8,
            left: 8,
        },
        ..Style::default()
    };
    ui.add(parent, Kind::Box, style)
        .ok_or_else(|| format!("line {line_no}: parent node cannot contain children"))
}

fn add_leaf(
    ui: &mut Ui,
    parent: NodeId,
    kind: Kind,
    v_pad: u32,
    h_pad: u32,
    line_no: usize,
) -> Result<NodeId, String> {
    let style = Style {
        padding: Insets {
            top: v_pad,
            right: h_pad,
            bottom: v_pad,
            left: h_pad,
        },
        ..Style::default()
    };
    ui.add(parent, kind, style)
        .ok_or_else(|| format!("line {line_no}: parent node cannot contain children"))
}

fn parse_quoted_arg(rest: &str, kw: &str, line_no: usize) -> Result<String, String> {
    if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
        return Err(format!("line {line_no}: expected `{kw} \"...\"`"));
    }
    let s = rest.trim();
    if !s.starts_with('"') || !s.ends_with('"') || s.len() < 2 {
        return Err(format!("line {line_no}: {kw} argument must be quoted"));
    }
    let inner = &s[1..s.len() - 1];
    if inner.contains('"') {
        return Err(format!("line {line_no}: unexpected quote inside string"));
    }
    Ok(inner.to_owned())
}

fn parse_list_count(rest: &str, line_no: usize) -> Result<usize, String> {
    if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
        return Err(format!("line {line_no}: expected `list N`"));
    }
    let n = rest
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("line {line_no}: invalid list count `{}`", rest.trim()))?;
    if !(1..=MAX_LIST_ITEMS).contains(&n) {
        return Err(format!(
            "line {line_no}: list count must be in 1..={MAX_LIST_ITEMS}, got {n}"
        ));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn focus_index_moves_focus_ring() {
        let a = "column\n  button \"One\"\n  button \"Two\"\nfocus 0";
        let b = "column\n  button \"One\"\n  button \"Two\"\nfocus 1";
        let Output::Pixels { rgba: ra, .. } = run(a).unwrap() else {
            panic!("expected pixels");
        };
        let Output::Pixels { rgba: rb, .. } = run(b).unwrap() else {
            panic!("expected pixels");
        };
        assert_ne!(ra, rb);
        assert!(run("column\n  button \"One\"\nfocus 1").is_err());
    }
}
