//! AST to Thompson NFA bytecode compiler.

use alloc::vec::Vec;

use super::class::{AnchorKind, Ast, CharClass};
use super::RegexError;

const MAX_INSTS: usize = 250_000;
const MAX_LOOP_DEPTH: usize = 64;

#[derive(Clone, Debug)]
pub(crate) enum Inst {
    Match,
    Jump(usize),
    Split(usize, usize),
    Save(usize),
    Char(char),
    Class(CharClass),
    Assert(AnchorKind),
    LoopEnter { depth: u8, target: usize },
    LoopEnd { depth: u8, head: usize, tail: usize },
}

#[derive(Clone, Debug)]
pub(crate) struct Prog {
    pub(crate) insts: Vec<Inst>,
    pub(crate) enclosing_masks: Vec<u64>,
    pub(crate) num_captures: usize,
}

pub(crate) fn compile(ast: &Ast, num_captures: usize) -> Result<Prog, RegexError> {
    let mut compiler = Compiler {
        insts: Vec::new(),
        enclosing_masks: Vec::new(),
        loop_depth: 0,
        active_mask: 0,
    };

    compiler.emit(Inst::Save(0))?;
    compiler.compile_ast(ast)?;
    compiler.emit(Inst::Save(1))?;
    compiler.emit(Inst::Match)?;

    Ok(Prog {
        insts: compiler.insts,
        enclosing_masks: compiler.enclosing_masks,
        num_captures,
    })
}

struct Compiler {
    insts: Vec<Inst>,
    enclosing_masks: Vec<u64>,
    loop_depth: usize,
    active_mask: u64,
}

impl Compiler {
    fn pc(&self) -> usize {
        self.insts.len()
    }

    fn emit(&mut self, inst: Inst) -> Result<usize, RegexError> {
        if self.insts.len() >= MAX_INSTS {
            return Err(RegexError {
                pos: 0,
                msg: "compiled pattern too large",
            });
        }
        let pc = self.insts.len();
        self.insts.push(inst);
        self.enclosing_masks.push(self.active_mask);
        Ok(pc)
    }

    fn patch_split(&mut self, pc: usize, a: usize, b: usize) {
        if let Some(Inst::Split(x, y)) = self.insts.get_mut(pc) {
            *x = a;
            *y = b;
        }
    }

    fn patch_jump(&mut self, pc: usize, target: usize) {
        if let Some(Inst::Jump(x)) = self.insts.get_mut(pc) {
            *x = target;
        }
    }

    fn patch_loop_enter(&mut self, pc: usize, target: usize) {
        if let Some(Inst::LoopEnter { target: t, .. }) = self.insts.get_mut(pc) {
            *t = target;
        }
    }

    fn patch_loop_end(&mut self, pc: usize, head: usize, tail: usize) {
        if let Some(Inst::LoopEnd {
            head: h, tail: t, ..
        }) = self.insts.get_mut(pc)
        {
            *h = head;
            *t = tail;
        }
    }

    fn compile_ast(&mut self, ast: &Ast) -> Result<(), RegexError> {
        match ast {
            Ast::Empty => Ok(()),
            Ast::Literal(c) => {
                self.emit(Inst::Char(*c))?;
                Ok(())
            }
            Ast::Class(cc) => {
                // Note: '.' was parsed as CharClass::dot(), or we can use Class directly.
                self.emit(Inst::Class(cc.clone()))?;
                Ok(())
            }
            Ast::Anchor(a) => {
                self.emit(Inst::Assert(*a))?;
                Ok(())
            }
            Ast::Concat(items) => {
                for item in items {
                    self.compile_ast(item)?;
                }
                Ok(())
            }
            Ast::Alt(branches) => self.compile_alt(branches),
            Ast::Group { capture, body } => {
                if let Some(id) = capture {
                    self.emit(Inst::Save(id * 2))?;
                    self.compile_ast(body)?;
                    self.emit(Inst::Save(id * 2 + 1))?;
                } else {
                    self.compile_ast(body)?;
                }
                Ok(())
            }
            Ast::Repeat {
                body,
                min,
                max,
                greedy,
            } => self.compile_repeat(body, *min, *max, *greedy),
        }
    }

    fn compile_alt(&mut self, branches: &[Ast]) -> Result<(), RegexError> {
        if branches.is_empty() {
            return Ok(());
        }
        if branches.len() == 1 {
            return self.compile_ast(&branches[0]);
        }
        let mut end_jumps = Vec::with_capacity(branches.len() - 1);
        for branch in &branches[..branches.len() - 1] {
            let split_pc = self.emit(Inst::Split(0, 0))?;
            let b_start = self.pc();
            self.compile_ast(branch)?;
            let jmp_pc = self.emit(Inst::Jump(0))?;
            end_jumps.push(jmp_pc);
            let next_pc = self.pc();
            self.patch_split(split_pc, b_start, next_pc);
        }
        if let Some(last) = branches.last() {
            self.compile_ast(last)?;
        }
        let end_pc = self.pc();
        for jmp_pc in end_jumps {
            self.patch_jump(jmp_pc, end_pc);
        }
        Ok(())
    }

    fn compile_repeat(
        &mut self,
        body: &Ast,
        min: u16,
        max: Option<u16>,
        greedy: bool,
    ) -> Result<(), RegexError> {
        match max {
            Some(m) => {
                for _ in 0..min {
                    self.compile_ast(body)?;
                }
                for _ in min..m {
                    let split_pc = self.emit(Inst::Split(0, 0))?;
                    let b_start = self.pc();
                    self.compile_ast(body)?;
                    let tail_pc = self.pc();
                    if greedy {
                        self.patch_split(split_pc, b_start, tail_pc);
                    } else {
                        self.patch_split(split_pc, tail_pc, b_start);
                    }
                }
                Ok(())
            }
            None => {
                if min == 0 {
                    self.compile_star(body, greedy)
                } else {
                    for _ in 0..(min - 1) {
                        self.compile_ast(body)?;
                    }
                    self.compile_plus(body, greedy)
                }
            }
        }
    }

    fn compile_star(&mut self, body: &Ast, greedy: bool) -> Result<(), RegexError> {
        if !is_nullable(body) {
            let split_pc = self.emit(Inst::Split(0, 0))?;
            let b_start = self.pc();
            self.compile_ast(body)?;
            self.emit(Inst::Jump(split_pc))?;
            let tail_pc = self.pc();
            if greedy {
                self.patch_split(split_pc, b_start, tail_pc);
            } else {
                self.patch_split(split_pc, tail_pc, b_start);
            }
            return Ok(());
        }

        let depth = self.push_loop()?;
        let split_pc = self.emit(Inst::Split(0, 0))?;
        let enter_pc = self.emit(Inst::LoopEnter { depth, target: 0 })?;
        let b_start = self.pc();
        self.patch_loop_enter(enter_pc, b_start);

        self.set_loop_bit(depth, true);
        self.compile_ast(body)?;
        let end_pc = self.emit(Inst::LoopEnd {
            depth,
            head: split_pc,
            tail: 0,
        })?;
        self.set_loop_bit(depth, false);
        self.pop_loop();

        let tail_pc = self.pc();
        self.patch_loop_end(end_pc, split_pc, tail_pc);
        if greedy {
            self.patch_split(split_pc, enter_pc, tail_pc);
        } else {
            self.patch_split(split_pc, tail_pc, enter_pc);
        }
        Ok(())
    }

    fn compile_plus(&mut self, body: &Ast, greedy: bool) -> Result<(), RegexError> {
        if !is_nullable(body) {
            let b_start = self.pc();
            self.compile_ast(body)?;
            let split_pc = self.emit(Inst::Split(0, 0))?;
            let tail_pc = self.pc();
            if greedy {
                self.patch_split(split_pc, b_start, tail_pc);
            } else {
                self.patch_split(split_pc, tail_pc, b_start);
            }
            return Ok(());
        }

        let depth = self.push_loop()?;
        let enter_pc = self.emit(Inst::LoopEnter { depth, target: 0 })?;
        let b_start = self.pc();
        self.patch_loop_enter(enter_pc, b_start);

        self.set_loop_bit(depth, true);
        self.compile_ast(body)?;
        let end_pc = self.emit(Inst::LoopEnd {
            depth,
            head: 0,
            tail: 0,
        })?;
        self.set_loop_bit(depth, false);
        self.pop_loop();

        let split_pc = self.emit(Inst::Split(0, 0))?;
        let tail_pc = self.pc();
        self.patch_loop_end(end_pc, split_pc, tail_pc);
        if greedy {
            self.patch_split(split_pc, enter_pc, tail_pc);
        } else {
            self.patch_split(split_pc, tail_pc, enter_pc);
        }
        Ok(())
    }

    fn push_loop(&mut self) -> Result<u8, RegexError> {
        if self.loop_depth >= MAX_LOOP_DEPTH {
            return Err(RegexError {
                pos: 0,
                msg: "loop nesting depth exceeded",
            });
        }
        let d = self.loop_depth as u8;
        self.loop_depth += 1;
        Ok(d)
    }

    fn pop_loop(&mut self) {
        self.loop_depth = self.loop_depth.saturating_sub(1);
    }

    fn set_loop_bit(&mut self, depth: u8, on: bool) {
        let bit = 1u64 << depth;
        if on {
            self.active_mask |= bit;
        } else {
            self.active_mask &= !bit;
        }
    }
}

fn is_nullable(ast: &Ast) -> bool {
    match ast {
        Ast::Empty | Ast::Anchor(_) => true,
        Ast::Literal(_) | Ast::Class(_) => false,
        Ast::Concat(items) => items.iter().all(is_nullable),
        Ast::Alt(branches) => branches.iter().any(is_nullable),
        Ast::Group { body, .. } => is_nullable(body),
        Ast::Repeat { min, body, .. } => *min == 0 || is_nullable(body),
    }
}
