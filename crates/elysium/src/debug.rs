//! Pausable VM execution session and debugger inspection types.

use crate::exec::{step_op, Frame, StepOutcome};
use crate::value::{Trap, Value};
use crate::vm::{Host, Limits, Vm};
use alloc::{vec, vec::Vec};

/// Execution coordinates of the innermost active frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Function index currently executing.
    pub func: u32,
    /// Instruction index of the next instruction to execute.
    pub pc: u32,
    /// Active call-stack depth (1 for the initial frame).
    pub depth: u32,
}

/// Snapshot of a single call frame's state for inspection.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameView {
    /// Function index of this frame.
    pub func: u32,
    /// Instruction index of the next instruction in this frame.
    pub pc: u32,
    /// Register values belonging to this frame.
    pub regs: Vec<Value>,
}

/// Outcome returned by `Session::resume`.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Execution paused before the op at `Position` because `until` returned `true`.
    Paused(Position),
    /// Execution stopped because `max_steps` instructions ran.
    Budget,
    /// Execution finished normally with the returned `Value`.
    Done(Value),
    /// Execution raised a `Trap`.
    Trapped(Trap),
}

/// Pausable interpreter state for a running VM invocation.
#[derive(Debug, Clone)]
pub struct Session {
    frames: Vec<Frame>,
    reg_stack: Vec<Value>,
    limits: Limits,
    fuel_used: u64,
    outcome: Option<Result<Value, Trap>>,
}

impl Session {
    pub(crate) fn new(
        func: u32,
        init_regs: u16,
        args: &[Value],
        limits: Limits,
    ) -> Result<Self, Trap> {
        let mut reg_stack = vec![Value::Unit; usize::from(init_regs)];
        let Some(dst) = reg_stack.get_mut(..args.len()) else {
            return Err(Trap::TypeError);
        };
        dst.copy_from_slice(args);
        let frames = vec![Frame {
            func,
            pc: 0,
            bp: 0,
            ret_reg: 0,
            regs: init_regs,
        }];
        Ok(Self {
            frames,
            reg_stack,
            limits,
            fuel_used: 0,
            outcome: None,
        })
    }

    /// Continue execution until `until` returns `true` (not checked on the first op), `max_steps` ops run, or execution terminates.
    pub fn resume(
        &mut self,
        vm: &mut Vm,
        host: &mut dyn Host,
        until: &dyn Fn(&Position) -> bool,
        max_steps: u64,
    ) -> Event {
        self.resume_inner::<true>(vm, host, until, max_steps)
    }

    /// Snapshot the active call frames with the innermost frame last.
    pub fn frames(&self) -> Vec<FrameView> {
        self.frames
            .iter()
            .map(|frame| {
                let end = frame.bp.saturating_add(usize::from(frame.regs));
                let regs = self.reg_stack.get(frame.bp..end).unwrap_or(&[]).to_vec();
                FrameView {
                    func: frame.func,
                    pc: u32::try_from(frame.pc).unwrap_or(u32::MAX),
                    regs,
                }
            })
            .collect()
    }

    pub(crate) fn run_to_end(&mut self, vm: &mut Vm, host: &mut dyn Host) -> Result<Value, Trap> {
        match self.resume_inner::<false>(vm, host, &|_| false, u64::MAX) {
            Event::Done(v) => Ok(v),
            Event::Trapped(t) => Err(t),
            Event::Paused(_) | Event::Budget => Err(Trap::OutOfFuel),
        }
    }

    fn resume_inner<const PAUSABLE: bool>(
        &mut self,
        vm: &mut Vm,
        host: &mut dyn Host,
        until: &dyn Fn(&Position) -> bool,
        max_steps: u64,
    ) -> Event {
        if let Some(ref res) = self.outcome {
            return match res {
                Ok(v) => Event::Done(*v),
                Err(t) => Event::Trapped(t.clone()),
            };
        }
        vm.fuel_used = self.fuel_used;
        let mut steps: u64 = 0;
        loop {
            if PAUSABLE {
                if steps >= max_steps {
                    self.fuel_used = vm.fuel_used;
                    vm.trap_site = None;
                    return Event::Budget;
                }
                if steps > 0 {
                    if let Some(frame) = self.frames.last() {
                        let pos = Position {
                            func: frame.func,
                            pc: u32::try_from(frame.pc).unwrap_or(u32::MAX),
                            depth: u32::try_from(self.frames.len()).unwrap_or(u32::MAX),
                        };
                        if until(&pos) {
                            self.fuel_used = vm.fuel_used;
                            vm.trap_site = None;
                            return Event::Paused(pos);
                        }
                    }
                }
            }
            match step_op(vm, &mut self.frames, &mut self.reg_stack, host, self.limits) {
                Ok(StepOutcome::Continue) => {
                    steps = steps.saturating_add(1);
                }
                Ok(StepOutcome::Done(val)) => {
                    self.fuel_used = vm.fuel_used;
                    vm.trap_site = None;
                    self.outcome = Some(Ok(val));
                    return Event::Done(val);
                }
                Err(trap) => {
                    self.fuel_used = vm.fuel_used;
                    self.outcome = Some(Err(trap.clone()));
                    return Event::Trapped(trap);
                }
            }
        }
    }
}

impl Vm {
    /// Start a pausable execution `Session` for function index `func` with `args` under `limits`.
    pub fn start(&mut self, func: u32, args: &[Value], limits: Limits) -> Result<Session, Trap> {
        self.fuel_used = 0;
        self.trap_site = None;
        self.heap.set_max_bytes(limits.max_heap_bytes);
        let f_def = self
            .module
            .funcs
            .get(func as usize)
            .ok_or(Trap::TypeError)?;
        if usize::from(f_def.params) != args.len() {
            return Err(Trap::TypeError);
        }
        if limits.max_depth == 0 {
            self.trap_site = Some((func, 0));
            return Err(Trap::StackOverflow);
        }
        Session::new(func, f_def.regs, args, limits)
    }
}
