//! Linear-time Pike VM executing compiled NFA programs with Python `re` priority.

use alloc::vec;
use alloc::vec::Vec;

use super::class::AnchorKind;
use super::compile::{Inst, Prog};

struct Thread {
    pc: usize,
    slots: Vec<Option<usize>>,
}

struct Visited {
    zero_gen: Vec<u32>,
    extra_gen: Vec<u32>,
    extra_masks: Vec<Vec<u64>>,
    next_queued_gen: Vec<u32>,
    gen_id: u32,
    /// Position where an empty match is refused (finditer retry after an empty match); MAX = none.
    no_empty_at: usize,
}

impl Visited {
    fn new(len: usize) -> Self {
        Self {
            zero_gen: vec![0; len],
            extra_gen: vec![0; len],
            extra_masks: vec![Vec::new(); len],
            next_queued_gen: vec![0; len],
            gen_id: 1,
            no_empty_at: usize::MAX,
        }
    }

    fn next_step(&mut self) {
        self.gen_id = self.gen_id.wrapping_add(1);
        if self.gen_id == 0 {
            self.zero_gen.fill(0);
            self.extra_gen.fill(0);
            self.next_queued_gen.fill(0);
            self.gen_id = 1;
        }
    }

    fn check_and_mark(&mut self, pc: usize, key: u64) -> bool {
        if key == 0 {
            let g = &mut self.zero_gen[pc];
            if *g == self.gen_id {
                return false;
            }
            *g = self.gen_id;
            return true;
        }
        if self.extra_gen[pc] != self.gen_id {
            self.extra_gen[pc] = self.gen_id;
            self.extra_masks[pc].clear();
        }
        let masks = &mut self.extra_masks[pc];
        if masks.contains(&key) {
            return false;
        }
        masks.push(key);
        true
    }

    fn mark_next(&mut self, pc: usize) -> bool {
        let g = &mut self.next_queued_gen[pc];
        if *g == self.gen_id {
            return false;
        }
        *g = self.gen_id;
        true
    }
}

pub(crate) fn find_at(
    prog: &Prog,
    text: &str,
    start: usize,
    no_empty_at: usize,
) -> Option<Vec<Option<(usize, usize)>>> {
    if start > text.len() || !text.is_char_boundary(start) {
        return None;
    }

    let num_slots = (prog.num_captures + 1) * 2;
    let mut visited = Visited::new(prog.insts.len());
    visited.no_empty_at = no_empty_at;
    let mut current: Vec<Thread> = Vec::new();
    let mut next: Vec<Thread> = Vec::new();
    let mut best_match: Option<Vec<Option<usize>>> = None;

    let mut pos = start;
    loop {
        let next_ch = text.get(pos..).and_then(|s| s.chars().next());
        let mut matched_here = false;

        for mut th in current.drain(..) {
            if add_thread(
                prog,
                text,
                pos,
                next_ch,
                th.pc,
                0,
                &mut th.slots,
                &mut visited,
                &mut next,
                &mut best_match,
            ) {
                matched_here = true;
                break;
            }
        }
        current.clear();

        if !matched_here && best_match.is_none() {
            let mut slots = vec![None; num_slots];
            let _ = add_thread(
                prog,
                text,
                pos,
                next_ch,
                0,
                0,
                &mut slots,
                &mut visited,
                &mut next,
                &mut best_match,
            );
        }

        if best_match.is_some() && next.is_empty() {
            break;
        }
        let Some(ch) = next_ch else {
            break;
        };
        pos += ch.len_utf8();
        core::mem::swap(&mut current, &mut next);
        visited.next_step();
    }

    best_match.map(|slots| {
        let mut groups = Vec::with_capacity(prog.num_captures + 1);
        for i in 0..=prog.num_captures {
            let s = slots.get(i * 2).copied().flatten();
            let e = slots.get(i * 2 + 1).copied().flatten();
            groups.push(s.zip(e));
        }
        groups
    })
}

#[allow(clippy::too_many_arguments)]
fn add_thread(
    prog: &Prog,
    text: &str,
    pos: usize,
    next_ch: Option<char>,
    pc: usize,
    entered: u64,
    slots: &mut Vec<Option<usize>>,
    visited: &mut Visited,
    next_threads: &mut Vec<Thread>,
    best_match: &mut Option<Vec<Option<usize>>>,
) -> bool {
    let Some(&mask) = prog.enclosing_masks.get(pc) else {
        return false;
    };
    if !visited.check_and_mark(pc, entered & mask) {
        return false;
    }

    let Some(inst) = prog.insts.get(pc) else {
        return false;
    };
    match inst {
        Inst::Match => {
            // Python finditer: an empty match at the previous empty match's spot is refused and
            // the lower-priority alternatives keep running (like sre backtracking past it).
            if pos == visited.no_empty_at && slots.first().copied().flatten() == Some(pos) {
                return false;
            }
            *best_match = Some(slots.clone());
            true
        }
        Inst::Jump(target) => add_thread(
            prog,
            text,
            pos,
            next_ch,
            *target,
            entered,
            slots,
            visited,
            next_threads,
            best_match,
        ),
        Inst::Split(a, b) => {
            if add_thread(
                prog,
                text,
                pos,
                next_ch,
                *a,
                entered,
                slots,
                visited,
                next_threads,
                best_match,
            ) {
                return true;
            }
            add_thread(
                prog,
                text,
                pos,
                next_ch,
                *b,
                entered,
                slots,
                visited,
                next_threads,
                best_match,
            )
        }
        Inst::Save(idx) => {
            let prev = slots.get(*idx).copied().flatten();
            if let Some(slot) = slots.get_mut(*idx) {
                *slot = Some(pos);
            }
            let hit = add_thread(
                prog,
                text,
                pos,
                next_ch,
                pc + 1,
                entered,
                slots,
                visited,
                next_threads,
                best_match,
            );
            if let Some(slot) = slots.get_mut(*idx) {
                *slot = prev;
            }
            hit
        }
        Inst::Assert(kind) => {
            if check_anchor(*kind, text, pos) {
                add_thread(
                    prog,
                    text,
                    pos,
                    next_ch,
                    pc + 1,
                    entered,
                    slots,
                    visited,
                    next_threads,
                    best_match,
                )
            } else {
                false
            }
        }
        Inst::LoopEnter { depth, target } => {
            let bit = 1u64 << *depth;
            add_thread(
                prog,
                text,
                pos,
                next_ch,
                *target,
                entered | bit,
                slots,
                visited,
                next_threads,
                best_match,
            )
        }
        Inst::LoopEnd { depth, head, tail } => {
            let bit = 1u64 << *depth;
            if (entered & bit) != 0 {
                add_thread(
                    prog,
                    text,
                    pos,
                    next_ch,
                    *tail,
                    entered & !bit,
                    slots,
                    visited,
                    next_threads,
                    best_match,
                )
            } else {
                add_thread(
                    prog,
                    text,
                    pos,
                    next_ch,
                    *head,
                    entered,
                    slots,
                    visited,
                    next_threads,
                    best_match,
                )
            }
        }
        Inst::Char(expected) => {
            if next_ch == Some(*expected) && visited.mark_next(pc + 1) {
                next_threads.push(Thread {
                    pc: pc + 1,
                    slots: slots.clone(),
                });
            }
            false
        }
        Inst::Class(cc) => {
            if next_ch.is_some_and(|ch| cc.matches(ch)) && visited.mark_next(pc + 1) {
                next_threads.push(Thread {
                    pc: pc + 1,
                    slots: slots.clone(),
                });
            }
            false
        }
    }
}

fn check_anchor(kind: AnchorKind, text: &str, pos: usize) -> bool {
    match kind {
        AnchorKind::Start => pos == 0,
        AnchorKind::End => {
            pos == text.len() || (pos + 1 == text.len() && text.as_bytes().get(pos) == Some(&b'\n'))
        }
        AnchorKind::WordBoundary => is_word_before(text, pos) != is_word_at(text, pos),
        AnchorKind::NonWordBoundary => is_word_before(text, pos) == is_word_at(text, pos),
    }
}

fn is_word_ascii(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_word_before(text: &str, pos: usize) -> bool {
    text.get(..pos)
        .and_then(|s| s.chars().next_back())
        .is_some_and(is_word_ascii)
}

fn is_word_at(text: &str, pos: usize) -> bool {
    text.get(pos..)
        .and_then(|s| s.chars().next())
        .is_some_and(is_word_ascii)
}
