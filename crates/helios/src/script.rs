//! Logos scripted scenes, host externs, deterministic state hashing, and rectangle rendering.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use elysium::{Heap, Host, Limits, Obj, Trap, Value, Vm};
use logos::ir::{Body, Ty};
use pigment::{Canvas, Rect, Rgba};

use crate::rng::Pcg32;
use crate::scene::{Command, Scene};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;
const SLOT_COUNT: usize = 256;
const MAX_LOG_BYTES: usize = 64 * 1024;

const HOST_NAMES: &[&str] = &[
    "game.keys",
    "game.frame",
    "game.get",
    "game.set",
    "game.rand",
    "game.rect",
    "game.log",
    "game.quit",
];

/// Source code of the built-in original arcade game.
pub const GAME_SRC: &str = include_str!("game.logos");

/// Axis-aligned colored rectangle emitted by a scripted scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Draw {
    /// Left edge in pixels.
    pub x: i32,
    /// Top edge in pixels.
    pub y: i32,
    /// Width in pixels.
    pub w: i32,
    /// Height in pixels.
    pub h: i32,
    /// RGB color in `0xRRGGBB` format.
    pub color: u32,
}

struct ScriptHost {
    keys: u32,
    frame: u64,
    slots: [i64; SLOT_COUNT],
    rng: Pcg32,
    draws: Vec<Draw>,
    log: String,
    quit_requested: bool,
}

impl ScriptHost {
    fn new(seed: u64) -> Self {
        Self {
            keys: 0,
            frame: 0,
            slots: [0; SLOT_COUNT],
            rng: Pcg32::new(seed, 0),
            draws: Vec::new(),
            log: String::new(),
            quit_requested: false,
        }
    }

    fn push_log(&mut self, s: &str) {
        let avail = MAX_LOG_BYTES.saturating_sub(self.log.len());
        if avail == 0 {
            return;
        }
        let mut end = avail.min(s.len());
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.log.push_str(&s[..end]);
    }

    fn rand_below(&mut self, n: i64) -> i64 {
        if n <= 0 {
            return 0;
        }
        if n <= i64::from(u32::MAX) {
            return i64::from(self.rng.below(n as u32));
        }
        let bound = n as u64;
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let hi = u64::from(self.rng.next_u32());
            let lo = u64::from(self.rng.next_u32());
            let r = (hi << 32) | lo;
            if r >= threshold {
                return (r % bound) as i64;
            }
        }
    }
}

fn arg_int(args: &[Value], idx: usize) -> Result<i64, Trap> {
    match args.get(idx).copied() {
        Some(Value::Int(v)) => Ok(v),
        _ => Err(Trap::TypeError),
    }
}

fn sat_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn sat_u32(v: i64) -> u32 {
    v.clamp(0, i64::from(u32::MAX)) as u32
}

impl Host for ScriptHost {
    fn call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Result<Value, Trap> {
        match name {
            "game.keys" => {
                if !args.is_empty() {
                    return Err(Trap::TypeError);
                }
                Ok(Value::Int(i64::from(self.keys)))
            }
            "game.frame" => {
                if !args.is_empty() {
                    return Err(Trap::TypeError);
                }
                Ok(Value::Int(i64::try_from(self.frame).unwrap_or(i64::MAX)))
            }
            "game.get" => {
                if args.len() != 1 {
                    return Err(Trap::TypeError);
                }
                let idx = arg_int(args, 0)?;
                let val = usize::try_from(idx)
                    .ok()
                    .and_then(|i| self.slots.get(i).copied())
                    .unwrap_or(0);
                Ok(Value::Int(val))
            }
            "game.set" => {
                if args.len() != 2 {
                    return Err(Trap::TypeError);
                }
                let idx = arg_int(args, 0)?;
                let val = arg_int(args, 1)?;
                if let Some(slot) = usize::try_from(idx)
                    .ok()
                    .and_then(|i| self.slots.get_mut(i))
                {
                    *slot = val;
                }
                Ok(Value::Unit)
            }
            "game.rand" => {
                if args.len() != 1 {
                    return Err(Trap::TypeError);
                }
                let n = arg_int(args, 0)?;
                Ok(Value::Int(self.rand_below(n)))
            }
            "game.rect" => {
                if args.len() != 5 {
                    return Err(Trap::TypeError);
                }
                let x = sat_i32(arg_int(args, 0)?);
                let y = sat_i32(arg_int(args, 1)?);
                let w = sat_i32(arg_int(args, 2)?);
                let h = sat_i32(arg_int(args, 3)?);
                let color = sat_u32(arg_int(args, 4)?);
                self.draws.push(Draw { x, y, w, h, color });
                Ok(Value::Unit)
            }
            "game.log" => {
                if args.len() != 1 {
                    return Err(Trap::TypeError);
                }
                let Some(Value::Ref(h)) = args.first().copied() else {
                    return Err(Trap::TypeError);
                };
                match heap.get(h)? {
                    Obj::Str(s) => {
                        self.push_log(s);
                        Ok(Value::Unit)
                    }
                    _ => Err(Trap::TypeError),
                }
            }
            "game.quit" => {
                if !args.is_empty() {
                    return Err(Trap::TypeError);
                }
                self.quit_requested = true;
                Ok(Value::Unit)
            }
            _ => Err(Trap::Host(format!("unknown host function: {name}"))),
        }
    }
}

/// A deterministic [`Scene`] backed by a compiled Logos script running on a Elysium VM.
pub struct ScriptScene {
    vm: Vm,
    host: ScriptHost,
    fuel_per_frame: u64,
    has_init: bool,
    inited: bool,
}

fn format_diag(src: &str, d: logos::Diag) -> String {
    let (line, col) = logos::line_col(src, d.span.start);
    format!("{line}:{col}: {}", d.message)
}

fn fnv1a_update(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(FNV_PRIME);
    }
}

impl ScriptScene {
    /// Parse, type-check, and compile `src` into a [`ScriptScene`] with `seed` and `fuel_per_frame`.
    pub fn new(src: &str, seed: u64, fuel_per_frame: u64) -> Result<ScriptScene, String> {
        let ast = logos::parser::parse(src).map_err(|d| format_diag(src, d))?;
        let ir = logos::check::check(&ast).map_err(|d| format_diag(src, d))?;

        let update_fn = ir
            .funcs
            .iter()
            .find(|f| f.name == "update")
            .ok_or_else(|| String::from("missing required function `fn update()`"))?;
        if update_fn.params != 0 || update_fn.ret != Ty::Unit {
            return Err(String::from(
                "`update` must have signature `fn update() -> unit`",
            ));
        }
        if matches!(update_fn.body, Body::Extern(_)) {
            return Err(String::from("`update` cannot be extern"));
        }

        let has_init = if let Some(init_fn) = ir.funcs.iter().find(|f| f.name == "init") {
            if init_fn.params != 0 || init_fn.ret != Ty::Unit {
                return Err(String::from(
                    "`init` must have signature `fn init() -> unit`",
                ));
            }
            if matches!(init_fn.body, Body::Extern(_)) {
                return Err(String::from("`init` cannot be extern"));
            }
            true
        } else {
            false
        };

        let compiled = cobalt::compile(&ir);
        let vm =
            Vm::new(compiled.module, HOST_NAMES).map_err(|e| format!("vm load error: {e:?}"))?;

        Ok(ScriptScene {
            vm,
            host: ScriptHost::new(seed),
            fuel_per_frame,
            has_init,
            inited: false,
        })
    }

    /// Return the draw list emitted during the last frame.
    pub fn draws(&self) -> &[Draw] {
        &self.host.draws
    }

    /// Read slot `i` (`0..256`), returning `0` when out of range.
    pub fn slot(&self, i: usize) -> i64 {
        self.host.slots.get(i).copied().unwrap_or(0)
    }

    /// Return the accumulated log output, including any trap lines.
    pub fn log(&self) -> &str {
        &self.host.log
    }

    fn record_trap(&mut self, trap: &Trap, out: &mut Vec<Command>) {
        if self.host.log.len() < MAX_LOG_BYTES {
            let msg = format!("trap: {trap:?}\n");
            self.host.push_log(&msg);
        }
        out.push(Command::Pop);
    }
}

impl Scene for ScriptScene {
    fn update(&mut self, keys: u32, out: &mut Vec<Command>) {
        self.host.keys = keys;
        self.host.draws.clear();
        self.host.quit_requested = false;

        let limits = Limits {
            fuel: self.fuel_per_frame,
            ..Limits::default()
        };

        if self.has_init && !self.inited {
            self.inited = true;
            if let Err(trap) = self.vm.call("init", &[], &mut self.host, limits) {
                self.record_trap(&trap, out);
                self.host.frame = self.host.frame.saturating_add(1);
                return;
            }
        }

        if let Err(trap) = self.vm.call("update", &[], &mut self.host, limits) {
            self.record_trap(&trap, out);
        } else if self.host.quit_requested {
            out.push(Command::Quit);
        }

        self.host.frame = self.host.frame.saturating_add(1);
    }

    fn state_hash(&self) -> u64 {
        let mut h = FNV_OFFSET;
        for &slot in &self.host.slots {
            fnv1a_update(&mut h, &slot.to_le_bytes());
        }
        for d in &self.host.draws {
            fnv1a_update(&mut h, &d.x.to_le_bytes());
            fnv1a_update(&mut h, &d.y.to_le_bytes());
            fnv1a_update(&mut h, &d.w.to_le_bytes());
            fnv1a_update(&mut h, &d.h.to_le_bytes());
            fnv1a_update(&mut h, &d.color.to_le_bytes());
        }
        h
    }
}

/// Render `draws` onto a black `width` x `height` canvas, clipping each rectangle in order.
pub fn render(draws: &[Draw], width: u32, height: u32) -> Canvas {
    let mut canvas = Canvas::new(width, height, Rgba(0, 0, 0, 255));
    for d in draws {
        if d.w <= 0 || d.h <= 0 {
            continue;
        }
        let r = ((d.color >> 16) & 0xff) as u8;
        let g = ((d.color >> 8) & 0xff) as u8;
        let b = (d.color & 0xff) as u8;
        let rect = Rect {
            x: d.x,
            y: d.y,
            w: d.w as u32,
            h: d.h as u32,
        };
        canvas.fill_rect(rect, Rgba(r, g, b, 255));
    }
    canvas
}
