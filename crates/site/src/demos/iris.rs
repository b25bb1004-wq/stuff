//! Iris compositing desktop window manager event-replay demo.

use indium::{Event, Key};
use iris::{Desktop, Editor, Files, Layout, Terminal, WinId};

use super::{canvas_to_output, check_input, Demo, Output};

const SCREEN_W: u32 = 640;
const SCREEN_H: u32 = 400;
const DEFAULT_W: u32 = 300;
const DEFAULT_H: u32 = 180;
const MAX_LINES: usize = 128;
const MAX_WAIT_MS: u64 = 5_000;
const TOTAL_WAIT_CAP_MS: u64 = 15_000;
const STEP_MS: u64 = 90;

const DEFAULT_SCRIPT: &str = "\
open terminal 300 180
text echo hello iris
key enter
open editor /note.txt 280 180
tile on
wait 180";

/// Registered Iris compositing desktop demo.
pub const DEMO: Demo = Demo {
    id: "iris",
    title: "Iris Desktop",
    default_input: DEFAULT_SCRIPT,
    run,
};

/// Replay an Iris desktop event script onto a 640x400 framebuffer and return RGBA pixels.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let mut desktop = Desktop::new(SCREEN_W, SCREEN_H);
    let mut now_ms: u64 = 0;
    let mut total_waited: u64 = 0;
    let mut commands: usize = 0;
    let mut lines_seen: usize = 0;

    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        lines_seen += 1;
        if lines_seen > MAX_LINES {
            return Err(format!("line {line_no}: exceeds {MAX_LINES} commands"));
        }

        let (cmd, rest) = split_cmd(line);
        match cmd {
            "open" => {
                run_open(&mut desktop, rest, now_ms, line_no)?;
            }
            "close" => {
                let id = parse_opt_win(&desktop, rest, line_no)?;
                if !desktop.close(id, now_ms) {
                    return Err(format!("line {line_no}: unknown window {id}"));
                }
            }
            "move" => {
                run_move(&mut desktop, rest, &mut now_ms, line_no)?;
            }
            "focus" => {
                run_focus(&mut desktop, rest, now_ms, line_no)?;
            }
            "tile" => {
                let arg = rest.trim();
                let layout = match arg {
                    "" | "on" => Layout::Tiling,
                    "off" => Layout::Floating,
                    _ => return Err(format!("line {line_no}: expected `tile [on|off]`")),
                };
                desktop.set_layout(layout, now_ms);
            }
            "float" => {
                if !rest.trim().is_empty() {
                    return Err(format!("line {line_no}: `float` takes no arguments"));
                }
                desktop.set_layout(Layout::Floating, now_ms);
            }
            "key" => {
                let (key, super_held, text_ch) = parse_key(rest.trim(), line_no)?;
                if let Some(k) = key {
                    desktop.input(Event::Key(k), super_held, now_ms);
                } else if let Some(ch) = text_ch {
                    desktop.input(Event::Text(ch), super_held, now_ms);
                }
            }
            "text" => {
                let payload = raw_text_arg(line);
                for ch in payload.chars() {
                    desktop.input(Event::Text(ch), false, now_ms);
                }
            }
            "wait" => {
                let ms = rest
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| format!("line {line_no}: invalid wait ms `{}`", rest.trim()))?;
                if ms > MAX_WAIT_MS {
                    return Err(format!("line {line_no}: wait exceeds {MAX_WAIT_MS} ms cap"));
                }
                total_waited = total_waited.saturating_add(ms);
                if total_waited > TOTAL_WAIT_CAP_MS {
                    return Err(format!(
                        "line {line_no}: total wait exceeds {TOTAL_WAIT_CAP_MS} ms cap"
                    ));
                }
                advance_frames(&mut desktop, &mut now_ms, ms);
            }
            other => {
                return Err(format!("line {line_no}: unknown command `{other}`"));
            }
        }
        commands += 1;
    }

    if commands == 0 {
        return Err(String::from("empty iris event script"));
    }

    now_ms = now_ms.saturating_add(180);
    desktop.frame(now_ms);
    Ok(canvas_to_output(desktop.screen().clone()))
}

fn split_cmd(line: &str) -> (&str, &str) {
    match line.find(char::is_whitespace) {
        Some(pos) => (&line[..pos], line[pos..].trim_start()),
        None => (line, ""),
    }
}

fn raw_text_arg(line: &str) -> &str {
    match line.find(char::is_whitespace) {
        Some(pos) => line[pos..]
            .strip_prefix(' ')
            .unwrap_or(line[pos..].trim_start()),
        None => "",
    }
}

fn parse_dims(tokens: &[&str], line_no: usize) -> Result<(u32, u32), String> {
    match tokens {
        [] => Ok((DEFAULT_W, DEFAULT_H)),
        [ws, hs] => {
            let w = ws
                .parse::<u32>()
                .map_err(|_| format!("line {line_no}: invalid width `{ws}`"))?;
            let h = hs
                .parse::<u32>()
                .map_err(|_| format!("line {line_no}: invalid height `{hs}`"))?;
            if !(64..=SCREEN_W).contains(&w) || !(48..=SCREEN_H).contains(&h) {
                return Err(format!(
                    "line {line_no}: window size must fit 64..={SCREEN_W} x 48..={SCREEN_H}"
                ));
            }
            Ok((w, h))
        }
        _ => Err(format!("line {line_no}: expected `[W H]` dimensions")),
    }
}

fn run_open(desktop: &mut Desktop, rest: &str, now_ms: u64, line_no: usize) -> Result<(), String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let Some((kind, args)) = parts.split_first() else {
        return Err(format!(
            "line {line_no}: `open` requires terminal|editor|files"
        ));
    };
    match *kind {
        "terminal" => {
            let (w, h) = parse_dims(args, line_no)?;
            desktop.launch(Box::new(Terminal::new()), w, h, now_ms);
            Ok(())
        }
        "editor" => {
            let (path, dim_args) = match args.first() {
                Some(first) if first.parse::<u32>().is_err() => (*first, &args[1..]),
                _ => ("/note.txt", args),
            };
            let (w, h) = parse_dims(dim_args, line_no)?;
            let fs = desktop.fs();
            desktop.launch(Box::new(Editor::new(fs, path)), w, h, now_ms);
            Ok(())
        }
        "files" => {
            let (path, dim_args) = match args.first() {
                Some(first) if first.parse::<u32>().is_err() => (*first, &args[1..]),
                _ => ("/", args),
            };
            let (w, h) = parse_dims(dim_args, line_no)?;
            let fs = desktop.fs();
            desktop.launch(Box::new(Files::new(fs, path)), w, h, now_ms);
            Ok(())
        }
        other => Err(format!("line {line_no}: unknown app `{other}`")),
    }
}

fn parse_opt_win(desktop: &Desktop, rest: &str, line_no: usize) -> Result<WinId, String> {
    let arg = rest.trim();
    if arg.is_empty() {
        return desktop
            .focused()
            .ok_or_else(|| format!("line {line_no}: no focused window"));
    }
    arg.parse::<WinId>()
        .map_err(|_| format!("line {line_no}: invalid window id `{arg}`"))
}

fn run_move(
    desktop: &mut Desktop,
    rest: &str,
    now_ms: &mut u64,
    line_no: usize,
) -> Result<(), String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let (id, xs, ys) = match parts.as_slice() {
        [x, y] => {
            let fid = desktop
                .focused()
                .ok_or_else(|| format!("line {line_no}: no focused window to move"))?;
            (fid, *x, *y)
        }
        [id_s, x, y] => {
            let id = id_s
                .parse::<WinId>()
                .map_err(|_| format!("line {line_no}: invalid window id `{id_s}`"))?;
            (id, *x, *y)
        }
        _ => return Err(format!("line {line_no}: expected `move [ID] X Y`")),
    };
    let x = xs
        .parse::<i32>()
        .map_err(|_| format!("line {line_no}: invalid X `{xs}`"))?;
    let y = ys
        .parse::<i32>()
        .map_err(|_| format!("line {line_no}: invalid Y `{ys}`"))?;
    let r = desktop
        .rect(id)
        .ok_or_else(|| format!("line {line_no}: unknown window {id}"))?;
    let grab_x = r.x.saturating_add(16);
    let grab_y = r.y.saturating_add(6);
    let dst_x = x.saturating_add(16);
    let dst_y = y.saturating_add(6);
    desktop.input(
        Event::MouseDown {
            x: grab_x,
            y: grab_y,
        },
        false,
        *now_ms,
    );
    desktop.input(Event::MouseMove { x: dst_x, y: dst_y }, false, *now_ms);
    desktop.input(Event::MouseUp { x: dst_x, y: dst_y }, false, *now_ms);
    Ok(())
}

fn run_focus(desktop: &mut Desktop, rest: &str, now_ms: u64, line_no: usize) -> Result<(), String> {
    let arg = rest.trim();
    match arg {
        "" | "next" => {
            desktop.input(Event::Key(Key::Tab), true, now_ms);
        }
        "prev" => {
            desktop.input(Event::Key(Key::ShiftTab), true, now_ms);
        }
        id_s => {
            let id = id_s
                .parse::<WinId>()
                .map_err(|_| format!("line {line_no}: invalid focus target `{id_s}`"))?;
            let r = desktop
                .rect(id)
                .ok_or_else(|| format!("line {line_no}: unknown window {id}"))?;
            let cx = r.x.saturating_add(i32::try_from(r.w / 2).unwrap_or(8));
            let cy = r.y.saturating_add(i32::try_from(r.h / 2).unwrap_or(8));
            desktop.input(Event::MouseDown { x: cx, y: cy }, false, now_ms);
            desktop.input(Event::MouseUp { x: cx, y: cy }, false, now_ms);
        }
    }
    Ok(())
}

fn parse_key(spec: &str, line_no: usize) -> Result<(Option<Key>, bool, Option<char>), String> {
    if spec.is_empty() {
        return Err(format!("line {line_no}: `key` requires a key name"));
    }
    let (super_held, base) = if let Some(rest) = spec.strip_prefix("super+") {
        (true, rest)
    } else {
        (false, spec)
    };
    let key = match base {
        "enter" => Some(Key::Enter),
        "tab" => Some(Key::Tab),
        "shifttab" => Some(Key::ShiftTab),
        "space" => Some(Key::Space),
        "backspace" => Some(Key::Backspace),
        "esc" => Some(Key::Escape),
        _ => None,
    };
    if key.is_some() {
        return Ok((key, super_held, None));
    }
    let mut chars = base.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return Ok((None, super_held, Some(ch)));
    }
    Err(format!("line {line_no}: unknown key `{spec}`"))
}

fn advance_frames(desktop: &mut Desktop, now_ms: &mut u64, ms: u64) {
    let target = now_ms.saturating_add(ms);
    while *now_ms < target {
        *now_ms = (*now_ms).saturating_add(STEP_MS).min(target);
        desktop.frame(*now_ms);
    }
    desktop.frame(*now_ms);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn move_and_focus_produce_valid_640x400_frame() {
        let script =
            "open files / 260 180\nopen terminal 280 180\nmove 2 300 80\nfocus 1\nwait 180";
        let Output::Pixels { w, h, rgba } = run(script).unwrap() else {
            panic!("expected pixels");
        };
        assert_eq!((w, h), (640, 400));
        assert_eq!(rgba.len(), 640 * 400 * 4);
    }
}
