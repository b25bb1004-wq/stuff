//! Debugger session for Athena: start, breakpoints, stepping and inspection over cobalt's debug API.
//!
//! Text protocol (`encode`): one record per line, `key rest-of-line`. Free text is escaped: `\` becomes
//! `\\`, a newline `\n`, a carriage return `\r`. Keys: `status` (paused, budget, done, trapped, error),
//! `line` and `col` (1-based; 0 when there is no position), `selected` (the frame whose variables
//! follow), `message`, `output` (program output so far), `frame <index> <name> <line>` (index 0 is the
//! innermost frame) and `var <name> <value>` (parameters of the selected frame; `let` bindings are not
//! recorded by cobalt yet).
use crate::service::check;
use cobalt::{Compiled, Step, STEP_BUDGET};
use elysium::{Event, Heap, Host, Limits, Obj, Position, Session, Trap as VmTrap, Value, Vm};
use std::sync::{Mutex, MutexGuard};

const MAX_DEPTH: u32 = 3;
const MAX_ITEMS: usize = 8;
const MAX_CALL_DEPTH: u32 = 10_000;

/// Where the debugged program stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Stopped at a breakpoint or a step.
    Paused,
    /// The step ran its op budget without stopping; call step again to go on.
    Budget,
    /// `main` returned.
    Done,
    /// The program trapped; `message` says why.
    Trapped,
    /// The request itself failed (compile error, no session).
    Error,
}

impl Status {
    fn word(self) -> &'static str {
        match self {
            Status::Paused => "paused",
            Status::Budget => "budget",
            Status::Done => "done",
            Status::Trapped => "trapped",
            Status::Error => "error",
        }
    }
}

/// One call frame, innermost first in `State::frames`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameInfo {
    /// Function name.
    pub name: String,
    /// 1-based line of the frame's next op, 0 if unknown.
    pub line: u32,
}

/// A parameter of the selected frame with its value as Logos source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarInfo {
    /// Parameter name.
    pub name: String,
    /// Value printed as Logos source.
    pub value: String,
}

/// Snapshot of the debugged program after a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Where the program stands.
    pub status: Status,
    /// 1-based line of the innermost frame, 0 if none.
    pub line: u32,
    /// 1-based byte column of the innermost frame, 0 if none.
    pub col: u32,
    /// Index of the frame whose variables are listed.
    pub selected: usize,
    /// Trap or error text, empty otherwise.
    pub message: String,
    /// Program output so far.
    pub output: String,
    /// Call stack, innermost first.
    pub frames: Vec<FrameInfo>,
    /// Parameters of the selected frame.
    pub vars: Vec<VarInfo>,
}

impl State {
    fn bare(status: Status, message: &str) -> Self {
        Self {
            status,
            line: 0,
            col: 0,
            selected: 0,
            message: message.to_owned(),
            output: String::new(),
            frames: Vec::new(),
            vars: Vec::new(),
        }
    }
}

/// Host functions for a debugged program: prints are captured, input is empty, `rand.next` is seeded.
struct CaptureHost {
    output: String,
    rng: u64,
}

impl Host for CaptureHost {
    fn call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Result<Value, VmTrap> {
        match name {
            "io.print" => {
                if let Some(Value::Ref(h)) = args.first().copied() {
                    if let Ok(Obj::Str(s)) = heap.get(h) {
                        self.output.push_str(s);
                    }
                }
                Ok(Value::Unit)
            }
            "io.read_line" => {
                let h = heap.alloc(Obj::Variant {
                    tag: 0,
                    fields: Vec::new(),
                })?;
                Ok(Value::Ref(h))
            }
            "time.now" => Ok(Value::Int(0)),
            "rand.next" => Ok(Value::Int(next_rand(&mut self.rng))),
            other => Err(VmTrap::Host(other.to_owned())),
        }
    }
}

/// SplitMix64 step, the same generator the cobalt runner uses.
fn next_rand(state: &mut u64) -> i64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    i64::from_ne_bytes((z ^ (z >> 31)).to_ne_bytes())
}

/// Where a step began: its mode, line and call depth.
#[derive(Debug, Clone, Copy)]
struct Anchor {
    mode: Step,
    line: Option<u32>,
    depth: u32,
}

/// One debugged run of a compiled program.
pub struct Debugger {
    compiled: Compiled,
    vm: Vm,
    session: Session,
    host: CaptureHost,
    status: Status,
    message: String,
    selected: usize,
    anchor: Option<Anchor>,
}

impl Debugger {
    /// Compile `src` and start `main`, paused before its first op; errors are `line:col: message`.
    pub fn start(src: &str) -> Result<Self, String> {
        if let Some(d) = check(src).first() {
            return Err(format!("{}:{}: {}", d.line, d.col, d.message));
        }
        let ast = logos::parser::parse(src).map_err(|_| String::from("parse error"))?;
        let prog = logos::check::check(&ast).map_err(|_| String::from("check error"))?;
        let compiled = cobalt::compile_with_source(&prog, src);
        let main = compiled
            .main
            .ok_or_else(|| String::from("no main function"))?;
        let names: Vec<&str> = compiled.module.imports.iter().map(String::as_str).collect();
        let mut vm = Vm::new(compiled.module.clone(), &names).map_err(|e| format!("{e:?}"))?;
        let limits = Limits {
            fuel: u64::MAX,
            max_heap_bytes: usize::MAX / 2,
            max_depth: MAX_CALL_DEPTH,
        };
        let session = vm.start(main, &[], limits).map_err(|t| format!("{t:?}"))?;
        Ok(Self {
            compiled,
            vm,
            session,
            host: CaptureHost {
                output: String::new(),
                rng: 1,
            },
            status: Status::Paused,
            message: String::new(),
            selected: 0,
            anchor: None,
        })
    }

    /// Run one step of `mode` stopping at `breakpoints` (1-based lines); selects the innermost frame.
    pub fn step(&mut self, mode: Step, breakpoints: &[u32]) -> State {
        self.step_budget(mode, breakpoints, STEP_BUDGET)
    }

    /// Like `step` with at most `budget` ops. A `Budget` pause keeps the stepping anchor (line and depth
    /// where the step began), so a later call with the same mode still stops where the step should.
    pub fn step_budget(&mut self, mode: Step, breakpoints: &[u32], budget: u64) -> State {
        if matches!(self.status, Status::Done | Status::Trapped) {
            return self.state();
        }
        self.selected = 0;
        let anchor = match self.anchor.take() {
            Some(a) => a,
            None => {
                let frames = self.session.frames();
                let top = frames.last();
                Anchor {
                    mode,
                    line: top
                        .and_then(|f| cobalt::line_of(&self.compiled, f.func, f.pc))
                        .map(|l| l.0),
                    depth: u32::try_from(frames.len()).unwrap_or(u32::MAX),
                }
            }
        };
        let bp_pcs: Vec<(u32, u32)> = breakpoints
            .iter()
            .flat_map(|&line| cobalt::first_pc_of_line(&self.compiled, line))
            .collect();
        let compiled = &self.compiled;
        // Same rules as `cobalt::step`, with the anchor kept across calls.
        let until = |pos: &Position| -> bool {
            if bp_pcs.contains(&(pos.func, pos.pc)) {
                return true;
            }
            let changed = cobalt::line_of(compiled, pos.func, pos.pc).map(|l| l.0) != anchor.line;
            match anchor.mode {
                Step::Continue => false,
                Step::Into => changed,
                Step::Over => changed && pos.depth <= anchor.depth,
                Step::Out => pos.depth < anchor.depth,
            }
        };
        let ev = self
            .session
            .resume(&mut self.vm, &mut self.host, &until, budget);
        if let Event::Budget = ev {
            self.anchor = Some(anchor);
        }
        match ev {
            Event::Paused(_) => self.status = Status::Paused,
            Event::Budget => self.status = Status::Budget,
            Event::Done(_) => self.status = Status::Done,
            Event::Trapped(t) => {
                self.status = Status::Trapped;
                self.message = format!("{t:?}");
            }
        }
        self.state()
    }

    /// Select frame `index` (0 is innermost) and return the state with its variables.
    pub fn select(&mut self, index: usize) -> Result<State, String> {
        if index >= self.session.frames().len() {
            return Err(format!("no frame {index}"));
        }
        self.selected = index;
        Ok(self.state())
    }

    /// The current state without running anything.
    pub fn state(&self) -> State {
        let frames = self.session.frames();
        let mut st = State {
            status: self.status,
            line: 0,
            col: 0,
            selected: self.selected,
            message: self.message.clone(),
            output: self.host.output.clone(),
            frames: Vec::new(),
            vars: Vec::new(),
        };
        if self.status == Status::Done {
            return st;
        }
        // A caller's pc is the op after its call, so its line is the op before that (the call itself).
        let innermost = frames.len().saturating_sub(1);
        for (i, f) in frames.iter().enumerate().rev() {
            let pc = if i == innermost {
                f.pc
            } else {
                f.pc.saturating_sub(1)
            };
            let (line, _) = cobalt::line_of(&self.compiled, f.func, pc).unwrap_or((0, 0));
            st.frames.push(FrameInfo {
                name: self.func_name(f.func),
                line,
            });
        }
        if let Some(top) = frames.last() {
            let (line, col) = cobalt::line_of(&self.compiled, top.func, top.pc).unwrap_or((0, 0));
            st.line = line;
            st.col = col;
        }
        let sel = frames.len().checked_sub(self.selected.saturating_add(1));
        if let Some(f) = sel.and_then(|i| frames.get(i)) {
            for (name, reg) in cobalt::locals_at(&self.compiled, f.func, f.pc) {
                let v = f.regs.get(reg as usize).copied().unwrap_or(Value::Unit);
                st.vars.push(VarInfo {
                    name,
                    value: show(self.vm.heap(), v, 0),
                });
            }
        }
        st
    }

    fn func_name(&self, func: u32) -> String {
        self.compiled
            .module
            .funcs
            .get(func as usize)
            .map_or_else(|| String::from("?"), |f| f.name.clone())
    }
}

/// Prints a value as Logos source, depth-limited and cut at a few items.
fn show(heap: &Heap, v: Value, depth: u32) -> String {
    match v {
        Value::Unit => String::from("()"),
        Value::Int(n) => n.to_string(),
        Value::Byte(b) => b.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Fn(i) => format!("<fn {i}>"),
        Value::Ref(h) => match heap.get(h) {
            Ok(Obj::Str(s)) => quote(s),
            Ok(Obj::Bytes(bs)) => {
                let vals: Vec<Value> = bs.iter().map(|b| Value::Byte(*b)).collect();
                seq(heap, &vals, depth, "[", "]")
            }
            Ok(Obj::Array(xs)) => seq(heap, xs, depth, "[", "]"),
            Ok(Obj::Record(xs)) => seq(heap, xs, depth, "(", ")"),
            Ok(Obj::Variant { tag: 0, fields }) if fields.is_empty() => String::from("none"),
            Ok(Obj::Variant { tag: 1, fields }) => match fields.as_slice() {
                [x] if depth < MAX_DEPTH => {
                    format!("some({})", show(heap, *x, depth.saturating_add(1)))
                }
                _ => seq(heap, fields, depth, "some(", ")"),
            },
            Ok(Obj::Variant { tag, fields }) => {
                format!("variant {tag}{}", seq(heap, fields, depth, "(", ")"))
            }
            Err(_) => String::from("<dangling>"),
        },
    }
}

fn seq(heap: &Heap, xs: &[Value], depth: u32, open: &str, close: &str) -> String {
    if depth >= MAX_DEPTH {
        return String::from("...");
    }
    let next = depth.saturating_add(1);
    let mut parts: Vec<String> = xs
        .iter()
        .take(MAX_ITEMS)
        .map(|x| show(heap, *x, next))
        .collect();
    if xs.len() > MAX_ITEMS {
        parts.push(String::from("..."));
    }
    format!("{open}{}{close}", parts.join(", "))
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Escapes free text for the line protocol: backslash, newline and carriage return.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// Encodes a state in the line protocol described at the top of this module.
pub fn encode(st: &State) -> String {
    let mut out = format!(
        "status {}\nline {}\ncol {}\nselected {}\nmessage {}\noutput {}\n",
        st.status.word(),
        st.line,
        st.col,
        st.selected,
        escape(&st.message),
        escape(&st.output)
    );
    for (i, f) in st.frames.iter().enumerate() {
        out.push_str(&format!("frame {i} {} {}\n", escape(&f.name), f.line));
    }
    for v in &st.vars {
        out.push_str(&format!("var {} {}\n", escape(&v.name), escape(&v.value)));
    }
    out
}

/// Breakpoint lines from text: every run of digits is one 1-based line; other text is ignored.
pub fn parse_breakpoints(text: &str) -> Vec<u32> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter_map(|t| t.parse::<u32>().ok())
        .filter(|&n| n > 0)
        .collect()
}

/// Maps the protocol's step mode (0 continue, 1 over, 2 into, 3 out) to a `Step`.
pub fn mode_of(mode: u32) -> Option<Step> {
    match mode {
        0 => Some(Step::Continue),
        1 => Some(Step::Over),
        2 => Some(Step::Into),
        3 => Some(Step::Out),
        _ => None,
    }
}

/// Encoded error state with `message`.
pub fn error_text(message: &str) -> String {
    encode(&State::bare(Status::Error, message))
}

static SESSION: Mutex<Option<Debugger>> = Mutex::new(None);

fn lock() -> MutexGuard<'static, Option<Debugger>> {
    SESSION.lock().unwrap_or_else(|p| p.into_inner())
}

/// Starts the one global session (replacing any); returns the encoded state.
pub fn start_text(src: &str) -> String {
    let mut guard = lock();
    *guard = None;
    match Debugger::start(src) {
        Ok(d) => {
            let text = encode(&d.state());
            *guard = Some(d);
            text
        }
        Err(msg) => error_text(&msg),
    }
}

/// Steps the global session in `mode`; returns the encoded state.
pub fn step_text(mode: u32, breakpoints: &[u32]) -> String {
    let Some(step) = mode_of(mode) else {
        return error_text("unknown step mode");
    };
    match lock().as_mut() {
        Some(d) => encode(&d.step(step, breakpoints)),
        None => error_text("no debug session"),
    }
}

/// Selects frame `index` of the global session; returns the encoded state.
pub fn frame_text(index: usize) -> String {
    match lock().as_mut() {
        Some(d) => match d.select(index) {
            Ok(st) => encode(&st),
            Err(msg) => error_text(&msg),
        },
        None => error_text("no debug session"),
    }
}

/// Ends the global session.
pub fn stop() {
    *lock() = None;
}
