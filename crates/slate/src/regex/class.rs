//! Character class codepoint range sets and regex AST nodes.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnchorKind {
    Start,
    End,
    WordBoundary,
    NonWordBoundary,
}

/// Sorted, non-overlapping inclusive Unicode codepoint ranges.
#[derive(Clone, Debug)]
pub(crate) struct CharClass {
    pub(crate) ranges: Vec<(u32, u32)>,
}

impl CharClass {
    pub(crate) fn dot() -> Self {
        Self {
            ranges: vec![(0, b'\n' as u32 - 1), (b'\n' as u32 + 1, 0x10_FFFF)],
        }
    }

    pub(crate) fn digit() -> Self {
        Self {
            ranges: vec![(b'0' as u32, b'9' as u32)],
        }
    }

    pub(crate) fn non_digit() -> Self {
        Self::from_ranges(true, vec![(b'0' as u32, b'9' as u32)])
    }

    pub(crate) fn word() -> Self {
        Self {
            ranges: vec![
                (b'0' as u32, b'9' as u32),
                (b'A' as u32, b'Z' as u32),
                (b'_' as u32, b'_' as u32),
                (b'a' as u32, b'z' as u32),
            ],
        }
    }

    pub(crate) fn non_word() -> Self {
        Self::from_ranges(true, Self::word().ranges)
    }

    pub(crate) fn space() -> Self {
        // ASCII whitespace in Python re.ASCII: [ \t\n\r\f\v]
        Self {
            ranges: vec![(0x09, 0x0D), (0x20, 0x20)],
        }
    }

    pub(crate) fn non_space() -> Self {
        Self::from_ranges(true, Self::space().ranges)
    }

    pub(crate) fn from_ranges(negated: bool, mut ranges: Vec<(u32, u32)>) -> Self {
        ranges.sort_unstable();
        let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
        for (lo, hi) in ranges {
            if let Some(last) = merged.last_mut() {
                if lo <= last.1.saturating_add(1) {
                    last.1 = last.1.max(hi);
                    continue;
                }
            }
            merged.push((lo, hi));
        }
        if !negated {
            return Self { ranges: merged };
        }
        let mut inv = Vec::new();
        let mut cur = 0u32;
        for (lo, hi) in merged {
            if cur < lo {
                inv.push((cur, lo - 1));
            }
            cur = hi.saturating_add(1);
        }
        if cur <= 0x10_FFFF {
            inv.push((cur, 0x10_FFFF));
        }
        Self { ranges: inv }
    }

    pub(crate) fn matches(&self, c: char) -> bool {
        let code = c as u32;
        let idx = self.ranges.partition_point(|&(_, hi)| hi < code);
        self.ranges
            .get(idx)
            .is_some_and(|&(lo, hi)| lo <= code && code <= hi)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Ast {
    Empty,
    Literal(char),
    Class(CharClass),
    Anchor(AnchorKind),
    Concat(Vec<Ast>),
    Alt(Vec<Ast>),
    Group {
        capture: Option<usize>,
        body: Box<Ast>,
    },
    Repeat {
        body: Box<Ast>,
        min: u16,
        max: Option<u16>,
        greedy: bool,
    },
}
