//! Helios Logos-scripted game engine rendering demo.

use helios::scene::{Command, Scene};
use helios::script::{render, ScriptScene};

use super::{canvas_to_output, check_input, Demo, Output};

const FRAME_W: u32 = 320;
const FRAME_H: u32 = 240;
const MAX_TICKS: u32 = 600;
const FUEL_PER_FRAME: u64 = 100_000;

const DEFAULT_GAME: &str = "\
ticks 30
fn g_frame() -> int = extern \"game.frame\"
fn g_get(i: int) -> int = extern \"game.get\"
fn g_set(i: int, v: int) = extern \"game.set\"
fn g_rand(n: int) -> int = extern \"game.rand\"
fn g_rect(x: int, y: int, w: int, h: int, c: int) = extern \"game.rect\"

fn init() {
    g_set(0, 152)
    g_set(1, 196)
    for i in 0..4 {
        g_set(10 + i * 2, 24 + i * 74)
        g_set(11 + i * 2, i * 32)
    }
}

fn update() {
    let f = g_frame()
    g_rect(0, 0, 320, 240, 0x090d1a)
    for s in 0..6 {
        let sx = (s * 53 + 17) % 316
        let sy = (s * 41 + f * (1 + (s % 2))) % 240
        g_rect(sx, sy, 2, 2, 0x334155)
    }
    for i in 0..4 {
        let mx = g_get(10 + i * 2)
        let my = (g_get(11 + i * 2) + 3) % 240
        g_set(11 + i * 2, my)
        g_rect(mx, my, 14, 14, 0xef4444)
    }
    let sx = 40 + (g_rand(1) + f * 3) % 240
    g_rect(sx, 110, 10, 10, 0x22c55e)
    let px = g_get(0) + ((f / 4) % 3) - 1
    let py = g_get(1)
    g_rect(px, py, 16, 12, 0x38bdf8)
    g_rect(px + 5, py - 3, 6, 4, 0xe0f2fe)
    g_rect(0, 0, 320, 14, 0x1e293b)
    g_rect(6, 4, min(180, f * 4), 6, 0x38bdf8)
}";

/// Registered Helios Logos-scripted game rendering demo.
pub const DEMO: Demo = Demo {
    id: "helios",
    title: "Helios Engine",
    default_input: DEFAULT_GAME,
    run,
};

/// Run a `ticks N` + Logos game script on `ScriptScene` and render the last 320x240 frame.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let (first_line, src) = match input.find('\n') {
        Some(idx) => (&input[..idx], &input[idx + 1..]),
        None => (input, ""),
    };
    let header = first_line.trim();
    let Some(ticks_str) = header.strip_prefix("ticks") else {
        return Err(String::from("first line must be `ticks N`"));
    };
    if ticks_str.is_empty() || !ticks_str.starts_with(char::is_whitespace) {
        return Err(String::from("first line must be `ticks N`"));
    }
    let ticks = ticks_str
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("invalid tick count `{}`", ticks_str.trim()))?;
    if !(1..=MAX_TICKS).contains(&ticks) {
        return Err(format!("ticks must be in 1..={MAX_TICKS}, got {ticks}"));
    }
    if src.trim().is_empty() {
        return Err(String::from("missing Logos game source after `ticks N`"));
    }

    let mut scene = ScriptScene::new(src, 1, FUEL_PER_FRAME)?;
    let mut cmds = Vec::new();
    for _ in 0..ticks {
        cmds.clear();
        scene.update(0, &mut cmds);
        if let Some(trap_line) = scene.log().lines().find(|l| l.starts_with("trap:")) {
            return Err(trap_line.to_owned());
        }
        if cmds
            .iter()
            .any(|c| matches!(c, Command::Quit | Command::Pop))
        {
            break;
        }
    }

    let canvas = render(scene.draws(), FRAME_W, FRAME_H);
    Ok(canvas_to_output(canvas))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn ticks_advance_rendered_frame() {
        let t1 = DEFAULT_GAME.replacen("ticks 30", "ticks 1", 1);
        let Output::Pixels { rgba: r1, .. } = run(&t1).unwrap() else {
            panic!("expected pixels");
        };
        let Output::Pixels { rgba: r30, .. } = run(DEFAULT_GAME).unwrap() else {
            panic!("expected pixels");
        };
        assert_ne!(r1, r30);
    }
}
