//! Source-level debugging on top of Elysium pausable sessions.
//!
//! Lines come from a line table built by `compile_with_source`; `compile` alone has no source
//! text, so without it `line_of` returns `None` and line-based stepping never changes line.
use crate::Compiled;
use alloc::string::String;
use alloc::vec::Vec;
use elysium::{Event, Func, Host, Position, Session, Vm};
use logos::ir::Program;

/// Default bound on the ops one `step` call may run before it returns `Event::Budget`.
pub const STEP_BUDGET: u64 = 5_000_000;

/// Debug tables of a compiled module, indexed like `Compiled::module.funcs`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Debug {
    /// Byte offset of the start of each 1-based line (`line_starts[0] == 0`); empty without source.
    pub line_starts: Vec<u32>,
    /// Per-function debug info.
    pub functions: Vec<FuncDebug>,
}

/// Debug info for one Elysium function.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FuncDebug {
    /// Elysium function name.
    pub name: String,
    /// Named locals: parameters, then `let`/`var` bindings in definition order.
    pub locals: Vec<LocalVar>,
}

/// A named register-backed local and the op range where it is live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVar {
    /// Register index in the function frame.
    pub reg: u32,
    /// Name as written in the source.
    pub name: String,
    /// First op index where the local is live (inclusive).
    pub from_pc: u32,
    /// Op index where the local stops being live (exclusive).
    pub to_pc: u32,
}

/// What `step` stops for, relative to the position it starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Run until a breakpoint.
    Continue,
    /// Stop at the next line at the same or a smaller call depth.
    Over,
    /// Stop at the next line at any depth.
    Into,
    /// Stop once the call depth drops below the starting depth.
    Out,
}

/// Compiles `p` like `compile`, and records the line table of `src` and the parameter names.
///
/// `src` must be the text `p` was checked from. Parameter names come from re-parsing `src`;
/// `let`/`var` names and ranges come from the logos bindings in `p`.
pub fn compile_with_source(p: &Program, src: &str) -> Compiled {
    let mut c = crate::compile(p);
    c.debug.line_starts = line_starts(src);
    let Ok(module) = logos::parser::parse(src) else {
        return c;
    };
    let Compiled {
        module: m, debug, ..
    } = &mut c;
    for item in &module.items {
        let logos::ast::Item::Fn(def) = item else {
            continue;
        };
        let Some(idx) = m.funcs.iter().position(|f| f.name == def.name.name) else {
            continue;
        };
        let Some(code_len) = m.funcs.get(idx).map(|f| f.code.len()) else {
            continue;
        };
        let to_pc = u32::try_from(code_len).unwrap_or(u32::MAX);
        let Some(fd) = debug.functions.get_mut(idx) else {
            continue;
        };
        // Parameters go first, then the `let`/`var` bindings recorded by logos.
        let mut locals: Vec<LocalVar> = def
            .params
            .iter()
            .enumerate()
            .map(|(i, prm)| LocalVar {
                reg: u32::try_from(i).unwrap_or(u32::MAX),
                name: prm.name.name.clone(),
                from_pc: 0,
                to_pc,
            })
            .collect();
        locals.append(&mut fd.locals);
        fd.locals = locals;
    }
    c
}

/// Byte offsets of every line start, with `0` first.
fn line_starts(src: &str) -> Vec<u32> {
    let mut starts = alloc::vec![0u32];
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            starts.push(u32::try_from(i + 1).unwrap_or(u32::MAX));
        }
    }
    starts
}

/// Builds the debug entry of one lowered function; locals are filled from source later.
pub(crate) fn func_debug(func: &Func) -> FuncDebug {
    FuncDebug {
        name: func.name.clone(),
        locals: Vec::new(),
    }
}

/// `(line, column)` of op `pc` of function `func`, both 1-based; the column counts bytes.
/// `None` without a line table or for an op out of range.
pub fn line_of(c: &Compiled, func: u32, pc: u32) -> Option<(u32, u32)> {
    let sp = c.spans.get(func as usize)?.get(pc as usize)?;
    let starts = &c.debug.line_starts;
    let idx = starts.partition_point(|&s| s <= sp.start);
    let line_start = *starts.get(idx.checked_sub(1)?)?;
    let line = u32::try_from(idx).ok()?;
    Some((line, sp.start.saturating_sub(line_start).saturating_add(1)))
}

/// Entry points `(func, pc)` of every run of ops that start on the 1-based source line `line`.
pub fn first_pc_of_line(c: &Compiled, line: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for (f, spans) in c.spans.iter().enumerate() {
        let func = u32::try_from(f).unwrap_or(u32::MAX);
        let mut prev = None;
        for pc in 0..spans.len() {
            let pc = u32::try_from(pc).unwrap_or(u32::MAX);
            let now = line_of(c, func, pc).map(|(l, _)| l);
            if now == Some(line) && prev != Some(line) {
                out.push((func, pc));
            }
            prev = now;
        }
    }
    out
}

/// Named locals of function `func` that are live at op `pc`, as `(name, reg)`.
pub fn locals_at(c: &Compiled, func: u32, pc: u32) -> Vec<(String, u32)> {
    let Some(f) = c.debug.functions.get(func as usize) else {
        return Vec::new();
    };
    f.locals
        .iter()
        .filter(|l| l.from_pc <= pc && pc < l.to_pc)
        .map(|l| (l.name.clone(), l.reg))
        .collect()
}

/// Runs `session` until a breakpoint or a `mode` stop, or `STEP_BUDGET` ops; `breakpoints` are line numbers.
pub fn step(
    c: &Compiled,
    session: &mut Session,
    vm: &mut Vm,
    host: &mut dyn Host,
    mode: Step,
    breakpoints: &[u32],
) -> Event {
    let frames = session.frames();
    let Some(top) = frames.last() else {
        return session.resume(vm, host, &|_| false, STEP_BUDGET);
    };
    let line_no = |func: u32, pc: u32| line_of(c, func, pc).map(|(l, _)| l);
    let start_line = line_no(top.func, top.pc);
    let start_depth = u32::try_from(frames.len()).unwrap_or(u32::MAX);
    let bp_pcs: Vec<(u32, u32)> = breakpoints
        .iter()
        .flat_map(|&line| first_pc_of_line(c, line))
        .collect();
    let until = |pos: &Position| -> bool {
        if bp_pcs.contains(&(pos.func, pos.pc)) {
            return true;
        }
        let changed = line_no(pos.func, pos.pc) != start_line;
        match mode {
            Step::Continue => false,
            Step::Into => changed,
            Step::Over => changed && pos.depth <= start_depth,
            Step::Out => pos.depth < start_depth,
        }
    };
    session.resume(vm, host, &until, STEP_BUDGET)
}
