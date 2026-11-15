//! Command-line replay tool for the `iris` desktop compositor.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

use indium::{Event, Key};
use iris::{App, Desktop, Editor, Files, Layout, Terminal, WinId};
use pigment::encode_png;

const VERSION: &str = "iris 0.1.0";
const MAX_DIM: u32 = 8192;
const ANIM_SETTLE_MS: u64 = 200;
const STEP_MS: u64 = 10;

const HELP: &str = "\
iris 0.1.0 - Monolith desktop compositor CLI

USAGE:
  iris replay EVENTS --out FRAME.png [--width W --height H]
  iris --help
  iris --version

SUBCOMMANDS:
  replay    Replay desktop events, write final PNG frame, print windows & damage

OPTIONS:
  --out PATH      Output PNG path ('-' for stdout; stats go to stderr)
  --width W       Desktop width in pixels (1..=8192, default 800)
  --height H      Desktop height in pixels (1..=8192, default 600)
  -h, --help      Print this help message
  -V, --version   Print version

EVENTS FORMAT (one command per line, '#' comments and blank lines ignored, '-' for stdin):
  open terminal [W H]          Open a terminal window (default 320x200)
  open editor [PATH] [W H]     Open an editor on shared fs (default /note.txt)
  open files [PATH] [W H]      Open a file browser on shared fs (default /)
  close [ID]                   Close window ID (default: focused window)
  move [ID] X Y                Drag window ID (or focused) to (X, Y) in floating mode
  focus [ID|next|prev]         Focus window ID or cycle focus
  tile [on|off]                Switch to tiling (or floating with 'off') layout
  float                        Switch to floating layout
  key NAME                     Send key (enter, tab, shifttab, space, backspace, esc,
                               super+<key>, or single char)
  text STRING                  Send each character of STRING as text input
  wait MS                      Advance clock by MS milliseconds and composite

EXAMPLE:
  iris replay events.txt --out frame.png --width 800 --height 600
";

enum CliAction {
    Help,
    Version,
    Replay(ReplayArgs),
}

struct ReplayArgs {
    events_path: String,
    out_path: String,
    width: u32,
    height: u32,
}

enum CliError {
    Usage(String),
    Runtime(String),
}

struct Stats {
    event_count: usize,
    frame_count: usize,
    damage_rects: usize,
    damage_pixels: u64,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_args(&args).and_then(|act| match act {
        CliAction::Help => {
            print!("{HELP}");
            Ok(())
        }
        CliAction::Version => {
            println!("{VERSION}");
            Ok(())
        }
        CliAction::Replay(cfg) => run_replay(&cfg),
    }) {
        Ok(()) => {}
        Err(CliError::Usage(msg)) => {
            eprintln!("iris: {msg}");
            process::exit(2);
        }
        Err(CliError::Runtime(msg)) => {
            eprintln!("iris: {msg}");
            process::exit(1);
        }
    }
}

fn parse_args(args: &[String]) -> Result<CliAction, CliError> {
    let Some(first) = args.first() else {
        return Err(CliError::Usage(String::from(
            "missing subcommand (try --help)",
        )));
    };
    match first.as_str() {
        "-h" | "--help" | "help" => Ok(CliAction::Help),
        "-V" | "--version" => Ok(CliAction::Version),
        "replay" => parse_replay_args(&args[1..]),
        other => Err(CliError::Usage(format!(
            "unknown subcommand '{other}' (try --help)"
        ))),
    }
}

fn parse_replay_args(args: &[String]) -> Result<CliAction, CliError> {
    let mut events_path: Option<String> = None;
    let mut out_path: Option<String> = None;
    let mut width: u32 = 800;
    let mut height: u32 = 600;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => return Ok(CliAction::Help),
            "--out" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| CliError::Usage(String::from("--out requires a path")))?;
                out_path = Some(val.clone());
            }
            "--width" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| CliError::Usage(String::from("--width requires a value")))?;
                width = parse_dim(val, "--width")?;
            }
            "--height" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| CliError::Usage(String::from("--height requires a value")))?;
                height = parse_dim(val, "--height")?;
            }
            _ if arg.starts_with("--out=") => {
                let val = &arg["--out=".len()..];
                if val.is_empty() {
                    return Err(CliError::Usage(String::from("--out requires a path")));
                }
                out_path = Some(val.to_owned());
            }
            _ if arg.starts_with("--width=") => {
                width = parse_dim(&arg["--width=".len()..], "--width")?;
            }
            _ if arg.starts_with("--height=") => {
                height = parse_dim(&arg["--height=".len()..], "--height")?;
            }
            _ if arg.starts_with('-') && arg != "-" => {
                return Err(CliError::Usage(format!("unknown option '{arg}'")));
            }
            _ => {
                if events_path.is_some() {
                    return Err(CliError::Usage(format!("unexpected argument '{arg}'")));
                }
                events_path = Some(arg.clone());
            }
        }
        i += 1;
    }

    let events_path = events_path
        .ok_or_else(|| CliError::Usage(String::from("replay requires an EVENTS path")))?;
    let out_path =
        out_path.ok_or_else(|| CliError::Usage(String::from("replay requires --out FRAME.png")))?;

    Ok(CliAction::Replay(ReplayArgs {
        events_path,
        out_path,
        width,
        height,
    }))
}

fn parse_dim(s: &str, flag: &str) -> Result<u32, CliError> {
    let val: u32 = s
        .parse()
        .map_err(|_| CliError::Usage(format!("invalid {flag} value '{s}'")))?;
    if val == 0 || val > MAX_DIM {
        return Err(CliError::Usage(format!(
            "{flag} must be between 1 and {MAX_DIM}, got {val}"
        )));
    }
    Ok(val)
}

fn read_events(path: &str) -> Result<String, CliError> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::Runtime(format!("failed to read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read_to_string(path)
            .map_err(|e| CliError::Runtime(format!("failed to read '{path}': {e}")))
    }
}

fn run_replay(cfg: &ReplayArgs) -> Result<(), CliError> {
    let script = read_events(&cfg.events_path)?;
    let mut desktop = Desktop::new(cfg.width, cfg.height);
    let mut now_ms: u64 = 0;
    let mut layout = Layout::Floating;
    let mut stats = Stats {
        event_count: 0,
        frame_count: 0,
        damage_rects: 0,
        damage_pixels: 0,
    };

    for (idx, raw_line) in script.lines().enumerate() {
        let line_num = idx + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        stats.event_count = stats.event_count.saturating_add(1);
        apply_line(
            trimmed,
            line_num,
            cfg.width,
            cfg.height,
            &mut desktop,
            &mut layout,
            &mut now_ms,
            &mut stats,
        )
        .map_err(CliError::Runtime)?;
    }

    if stats.frame_count == 0 {
        record_frame(&mut desktop, now_ms, &mut stats);
    }

    let png = encode_png(desktop.screen());
    if cfg.out_path == "-" {
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(&png)
            .map_err(|e| CliError::Runtime(format!("failed to write stdout: {e}")))?;
        stdout
            .flush()
            .map_err(|e| CliError::Runtime(format!("failed to flush stdout: {e}")))?;
        let mut stderr = io::stderr().lock();
        write_report(&mut stderr, &desktop, layout, &stats, cfg, png.len())
            .map_err(|e| CliError::Runtime(format!("failed to write report: {e}")))?;
    } else {
        fs::write(&cfg.out_path, &png)
            .map_err(|e| CliError::Runtime(format!("failed to write '{}': {e}", cfg.out_path)))?;
        let mut stdout = io::stdout().lock();
        write_report(&mut stdout, &desktop, layout, &stats, cfg, png.len())
            .map_err(|e| CliError::Runtime(format!("failed to write report: {e}")))?;
    }

    Ok(())
}

fn record_frame(desktop: &mut Desktop, now_ms: u64, stats: &mut Stats) {
    let rects = desktop.frame(now_ms);
    stats.frame_count = stats.frame_count.saturating_add(1);
    stats.damage_rects = stats.damage_rects.saturating_add(rects.len());
    for r in rects {
        let px = u64::from(r.w).saturating_mul(u64::from(r.h));
        stats.damage_pixels = stats.damage_pixels.saturating_add(px);
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_line(
    line: &str,
    line_num: usize,
    screen_w: u32,
    screen_h: u32,
    desktop: &mut Desktop,
    layout: &mut Layout,
    now_ms: &mut u64,
    stats: &mut Stats,
) -> Result<(), String> {
    let (verb, rest) = split_verb(line);
    match verb {
        "open" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let (app, w, h) = parse_open(&parts, screen_w, screen_h, desktop)
                .map_err(|m| format!("line {line_num}: {m}"))?;
            let _ = desktop.launch(app, w, h, *now_ms);
            *now_ms = now_ms.saturating_add(ANIM_SETTLE_MS);
            record_frame(desktop, *now_ms, stats);
        }
        "close" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let id = match parts.as_slice() {
                [] => desktop
                    .focused()
                    .ok_or_else(|| format!("line {line_num}: no focused window to close"))?,
                [raw_id] => parse_win_id(raw_id, line_num)?,
                _ => return Err(format!("line {line_num}: usage: close [ID]")),
            };
            if !desktop.close(id, *now_ms) {
                return Err(format!("line {line_num}: unknown window {id}"));
            }
            *now_ms = now_ms.saturating_add(ANIM_SETTLE_MS);
            record_frame(desktop, *now_ms, stats);
        }
        "move" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let (id, x, y) = match parts.as_slice() {
                [rx, ry] => {
                    let id = desktop
                        .focused()
                        .ok_or_else(|| format!("line {line_num}: no focused window to move"))?;
                    (id, parse_i32(rx, line_num)?, parse_i32(ry, line_num)?)
                }
                [rid, rx, ry] => (
                    parse_win_id(rid, line_num)?,
                    parse_i32(rx, line_num)?,
                    parse_i32(ry, line_num)?,
                ),
                _ => return Err(format!("line {line_num}: usage: move [ID] X Y")),
            };
            if *layout != Layout::Floating {
                return Err(format!("line {line_num}: move requires floating layout"));
            }
            move_window(desktop, id, x, y, *now_ms).map_err(|m| format!("line {line_num}: {m}"))?;
            record_frame(desktop, *now_ms, stats);
            *now_ms = now_ms.saturating_add(STEP_MS);
        }
        "focus" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            match parts.as_slice() {
                [] | ["next"] => {
                    desktop.input(Event::Key(Key::Tab), true, *now_ms);
                }
                ["prev"] => {
                    desktop.input(Event::Key(Key::ShiftTab), true, *now_ms);
                }
                [rid] => {
                    let id = parse_win_id(rid, line_num)?;
                    focus_window(desktop, id, *now_ms)
                        .map_err(|m| format!("line {line_num}: {m}"))?;
                }
                _ => return Err(format!("line {line_num}: usage: focus [ID|next|prev]")),
            }
            record_frame(desktop, *now_ms, stats);
            *now_ms = now_ms.saturating_add(STEP_MS);
        }
        "tile" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let next = match parts.as_slice() {
                [] | ["on"] => Layout::Tiling,
                ["off"] => Layout::Floating,
                _ => return Err(format!("line {line_num}: usage: tile [on|off]")),
            };
            *layout = next;
            desktop.set_layout(next, *now_ms);
            *now_ms = now_ms.saturating_add(ANIM_SETTLE_MS);
            record_frame(desktop, *now_ms, stats);
        }
        "float" => {
            if !rest.trim().is_empty() {
                return Err(format!("line {line_num}: usage: float"));
            }
            *layout = Layout::Floating;
            desktop.set_layout(Layout::Floating, *now_ms);
            *now_ms = now_ms.saturating_add(ANIM_SETTLE_MS);
            record_frame(desktop, *now_ms, stats);
        }
        "key" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let [name] = parts.as_slice() else {
                return Err(format!("line {line_num}: usage: key NAME"));
            };
            let (ev, super_held) = parse_key(name).map_err(|m| format!("line {line_num}: {m}"))?;
            desktop.input(ev, super_held, *now_ms);
            if *layout == Layout::Tiling && super_held && matches!(ev, Event::Key(Key::Enter)) {
                *now_ms = now_ms.saturating_add(ANIM_SETTLE_MS);
            }
            record_frame(desktop, *now_ms, stats);
            *now_ms = now_ms.saturating_add(STEP_MS);
        }
        "text" => {
            let payload = rest.strip_prefix(' ').unwrap_or(rest);
            if payload.is_empty() {
                return Err(format!("line {line_num}: usage: text STRING"));
            }
            for ch in payload.chars() {
                desktop.input(Event::Text(ch), false, *now_ms);
            }
            record_frame(desktop, *now_ms, stats);
            *now_ms = now_ms.saturating_add(STEP_MS);
        }
        "wait" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let [raw_ms] = parts.as_slice() else {
                return Err(format!("line {line_num}: usage: wait MS"));
            };
            let ms: u64 = raw_ms
                .parse()
                .map_err(|_| format!("line {line_num}: invalid milliseconds '{raw_ms}'"))?;
            *now_ms = now_ms.saturating_add(ms);
            record_frame(desktop, *now_ms, stats);
        }
        other => return Err(format!("line {line_num}: unknown command '{other}'")),
    }
    Ok(())
}

fn split_verb(line: &str) -> (&str, &str) {
    match line.find(char::is_whitespace) {
        Some(pos) => (&line[..pos], &line[pos..]),
        None => (line, ""),
    }
}

fn default_win_size(screen_w: u32, screen_h: u32) -> (u32, u32) {
    let w = 320.min(screen_w.saturating_sub(4).max(16)).max(1);
    let h = 200.min(screen_h.saturating_sub(24).max(16)).max(1);
    (w, h)
}

fn parse_win_dims(w_str: &str, h_str: &str) -> Result<(u32, u32), String> {
    let w: u32 = w_str
        .parse()
        .map_err(|_| format!("invalid window width '{w_str}'"))?;
    let h: u32 = h_str
        .parse()
        .map_err(|_| format!("invalid window height '{h_str}'"))?;
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return Err(format!(
            "window dimensions must be in 1..={MAX_DIM}, got {w}x{h}"
        ));
    }
    Ok((w, h))
}

fn parse_open(
    parts: &[&str],
    screen_w: u32,
    screen_h: u32,
    desktop: &Desktop,
) -> Result<(Box<dyn App>, u32, u32), String> {
    let Some(&kind) = parts.first() else {
        return Err(String::from("usage: open <terminal|editor|files> ..."));
    };
    let rest = &parts[1..];
    let (def_w, def_h) = default_win_size(screen_w, screen_h);

    match kind {
        "terminal" | "term" => {
            let (w, h) = match rest {
                [] => (def_w, def_h),
                [rw, rh] => parse_win_dims(rw, rh)?,
                _ => return Err(String::from("usage: open terminal [W H]")),
            };
            Ok((Box::new(Terminal::new()), w, h))
        }
        "editor" => {
            let (path, w, h) = parse_path_and_dims(rest, "/note.txt", def_w, def_h, "editor")?;
            Ok((Box::new(Editor::new(desktop.fs(), path)), w, h))
        }
        "files" => {
            let (path, w, h) = parse_path_and_dims(rest, "/", def_w, def_h, "files")?;
            Ok((Box::new(Files::new(desktop.fs(), path)), w, h))
        }
        other => Err(format!(
            "unknown app '{other}' (expected terminal, editor, or files)"
        )),
    }
}

fn parse_path_and_dims<'a>(
    rest: &'a [&'a str],
    default_path: &'a str,
    def_w: u32,
    def_h: u32,
    name: &str,
) -> Result<(&'a str, u32, u32), String> {
    match rest {
        [] => Ok((default_path, def_w, def_h)),
        [a, b] => {
            if let (Ok(w), Ok(h)) = (a.parse::<u32>(), b.parse::<u32>()) {
                if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
                    return Err(format!(
                        "window dimensions must be in 1..={MAX_DIM}, got {w}x{h}"
                    ));
                }
                Ok((default_path, w, h))
            } else {
                Err(format!("usage: open {name} [PATH] [W H]"))
            }
        }
        [path] => Ok((*path, def_w, def_h)),
        [path, rw, rh] => {
            let (w, h) = parse_win_dims(rw, rh)?;
            Ok((*path, w, h))
        }
        _ => Err(format!("usage: open {name} [PATH] [W H]")),
    }
}

fn parse_win_id(s: &str, line_num: usize) -> Result<WinId, String> {
    s.parse::<WinId>()
        .map_err(|_| format!("line {line_num}: invalid window id '{s}'"))
}

fn parse_i32(s: &str, line_num: usize) -> Result<i32, String> {
    s.parse::<i32>()
        .map_err(|_| format!("line {line_num}: invalid coordinate '{s}'"))
}

fn focus_window(desktop: &mut Desktop, id: WinId, now_ms: u64) -> Result<(), String> {
    let order = desktop.order();
    if !order.contains(&id) {
        return Err(format!("unknown window {id}"));
    }
    let n = order.len();
    if desktop.focused() != Some(id) {
        for _ in 0..n {
            desktop.input(Event::Key(Key::Tab), true, now_ms);
            if desktop.focused() == Some(id) {
                return Ok(());
            }
        }
        return Err(format!("failed to focus window {id}"));
    }
    if n > 1 && order.last() != Some(&id) {
        for _ in 0..n {
            desktop.input(Event::Key(Key::Tab), true, now_ms);
        }
    }
    Ok(())
}

fn move_window(
    desktop: &mut Desktop,
    id: WinId,
    target_x: i32,
    target_y: i32,
    now_ms: u64,
) -> Result<(), String> {
    focus_window(desktop, id, now_ms)?;
    let frame = desktop
        .rect(id)
        .ok_or_else(|| format!("unknown window {id}"))?;
    if frame.w <= 2 || frame.h <= 2 {
        return Err(format!("window {id} is too small to drag"));
    }
    let grab_x = frame.x.saturating_add(1);
    let grab_y = frame.y.saturating_add(1);
    let drop_x = target_x.saturating_add(1);
    let drop_y = target_y.saturating_add(1);
    desktop.input(
        Event::MouseDown {
            x: grab_x,
            y: grab_y,
        },
        false,
        now_ms,
    );
    desktop.input(
        Event::MouseMove {
            x: drop_x,
            y: drop_y,
        },
        false,
        now_ms,
    );
    desktop.input(
        Event::MouseUp {
            x: drop_x,
            y: drop_y,
        },
        false,
        now_ms,
    );
    Ok(())
}

fn parse_key(name: &str) -> Result<(Event, bool), String> {
    let lower = name.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("super+") {
        let orig_rest = &name["super+".len()..];
        let (ev, _) = parse_key_inner(&lower[..], rest, orig_rest)?;
        return Ok((ev, true));
    }
    parse_key_inner(&lower, &lower, name)
}

fn parse_key_inner(full_lower: &str, key_lower: &str, orig: &str) -> Result<(Event, bool), String> {
    match key_lower {
        "enter" | "return" => Ok((Event::Key(Key::Enter), false)),
        "tab" => Ok((Event::Key(Key::Tab), false)),
        "shifttab" | "shift-tab" | "shift+tab" => Ok((Event::Key(Key::ShiftTab), false)),
        "space" => Ok((Event::Key(Key::Space), false)),
        "backspace" | "bs" => Ok((Event::Key(Key::Backspace), false)),
        "escape" | "esc" => Ok((Event::Key(Key::Escape), false)),
        _ => {
            let mut chars = orig.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => Ok((Event::Text(ch), false)),
                _ => Err(format!("unknown key '{full_lower}'")),
            }
        }
    }
}

fn write_report(
    w: &mut dyn Write,
    desktop: &Desktop,
    layout: Layout,
    stats: &Stats,
    cfg: &ReplayArgs,
    png_bytes: usize,
) -> io::Result<()> {
    let layout_str = match layout {
        Layout::Floating => "floating",
        Layout::Tiling => "tiling",
    };
    let focused_str = match desktop.focused() {
        Some(id) => id.to_string(),
        None => String::from("none"),
    };

    writeln!(w, "screen {}x{}", cfg.width, cfg.height)?;
    writeln!(w, "events {}", stats.event_count)?;
    writeln!(w, "frames {}", stats.frame_count)?;
    writeln!(w, "damage_rects {}", stats.damage_rects)?;
    writeln!(w, "damage_pixels {}", stats.damage_pixels)?;
    writeln!(w, "layout {layout_str}")?;
    writeln!(w, "focused {focused_str}")?;

    let order = desktop.order();
    writeln!(w, "windows {}", order.len())?;
    for id in order {
        let title = desktop.app(id).map_or("", App::title);
        let is_focused = desktop.focused() == Some(id);
        if let Some(r) = desktop.rect(id) {
            writeln!(
                w,
                "window {id} x={} y={} w={} h={} focused={is_focused} title={title}",
                r.x, r.y, r.w, r.h
            )?;
        }
    }
    writeln!(w, "out {} ({png_bytes} bytes)", cfg.out_path)?;
    Ok(())
}
