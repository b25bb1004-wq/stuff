//! RISC-V register, CSR, and fence bitmask parsers.

use crate::expr::{parse_expr, Expr};
use crate::AsmError;

/// Parse an `imm(rs1)` or `(rs1)` memory operand into `(Option<imm_expr>, reg)`.
pub(crate) fn parse_mem_operand(s: &str, line: usize) -> Result<(Option<Expr>, u8), AsmError> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.last() != Some(&b')') {
        return Err(AsmError::new(line, format!("expected imm(rs1), got '{s}'")));
    }
    let mut depth = 0usize;
    let mut open_idx = None;
    for (idx, &b) in bytes.iter().enumerate().rev() {
        match b {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    open_idx = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(open) = open_idx else {
        return Err(AsmError::new(line, format!("unmatched ')' in '{s}'")));
    };
    let reg_str = s[open + 1..s.len() - 1].trim();
    let reg = parse_reg(reg_str)
        .ok_or_else(|| AsmError::new(line, format!("unknown register '{reg_str}'")))?;
    let imm_str = s[..open].trim();
    let imm = if imm_str.is_empty() {
        None
    } else {
        Some(parse_expr(imm_str, line)?)
    };
    Ok((imm, reg))
}

/// Parse an integer register name (`x0`..`x31` or lowercase ABI name).
pub(crate) fn parse_reg(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('x') {
        if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
            // Reject leading zeros like x00 unless it's literally x0.
            if rest.len() > 1 && rest.starts_with('0') {
                return None;
            }
            let n: u8 = rest.parse().ok()?;
            if n < 32 {
                return Some(n);
            }
        }
        return None;
    }
    match s {
        "zero" => Some(0),
        "ra" => Some(1),
        "sp" => Some(2),
        "gp" => Some(3),
        "tp" => Some(4),
        "t0" => Some(5),
        "t1" => Some(6),
        "t2" => Some(7),
        "s0" | "fp" => Some(8),
        "s1" => Some(9),
        "a0" => Some(10),
        "a1" => Some(11),
        "a2" => Some(12),
        "a3" => Some(13),
        "a4" => Some(14),
        "a5" => Some(15),
        "a6" => Some(16),
        "a7" => Some(17),
        "s2" => Some(18),
        "s3" => Some(19),
        "s4" => Some(20),
        "s5" => Some(21),
        "s6" => Some(22),
        "s7" => Some(23),
        "s8" => Some(24),
        "s9" => Some(25),
        "s10" => Some(26),
        "s11" => Some(27),
        "t3" => Some(28),
        "t4" => Some(29),
        "t5" => Some(30),
        "t6" => Some(31),
        _ => None,
    }
}

/// Convert a register number in `x8..=x15` to its 3-bit compressed register encoding.
pub(crate) fn c_reg(r: u8) -> Option<u8> {
    if (8..=15).contains(&r) {
        Some(r - 8)
    } else {
        None
    }
}

/// Parse a named CSR into its 12-bit address.
pub(crate) fn parse_csr_name(s: &str) -> Option<u16> {
    match s.trim() {
        "cycle" => Some(0xC00),
        "time" => Some(0xC01),
        "instret" => Some(0xC02),
        "sstatus" => Some(0x100),
        "sie" => Some(0x104),
        "stvec" => Some(0x105),
        "scounteren" => Some(0x106),
        "sscratch" => Some(0x140),
        "sepc" => Some(0x141),
        "scause" => Some(0x142),
        "stval" => Some(0x143),
        "sip" => Some(0x144),
        "satp" => Some(0x180),
        "mvendorid" => Some(0xF11),
        "marchid" => Some(0xF12),
        "mimpid" => Some(0xF13),
        "mhartid" => Some(0xF14),
        "mstatus" => Some(0x300),
        "misa" => Some(0x301),
        "medeleg" => Some(0x302),
        "mideleg" => Some(0x303),
        "mie" => Some(0x304),
        "mtvec" => Some(0x305),
        "mcounteren" => Some(0x306),
        "mscratch" => Some(0x340),
        "mepc" => Some(0x341),
        "mcause" => Some(0x342),
        "mtval" => Some(0x343),
        "mip" => Some(0x344),
        "mcycle" => Some(0xB00),
        "minstret" => Some(0xB02),
        _ => None,
    }
}

/// Parse a `fence` pred/succ set: non-empty subset of `iorw` in that order.
pub(crate) fn parse_fence_set(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut bits = 0u32;
    let mut last_idx = -1i32;
    for ch in s.bytes() {
        let (idx, bit) = match ch {
            b'i' => (0, 8),
            b'o' => (1, 4),
            b'r' => (2, 2),
            b'w' => (3, 1),
            _ => return None,
        };
        if idx <= last_idx {
            return None;
        }
        last_idx = idx;
        bits |= bit;
    }
    Some(bits)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn reg_names() {
        assert_eq!(parse_reg("zero"), Some(0));
        assert_eq!(parse_reg("x0"), Some(0));
        assert_eq!(parse_reg("x00"), None);
        assert_eq!(parse_reg("x31"), Some(31));
        assert_eq!(parse_reg("x32"), None);
        assert_eq!(parse_reg("fp"), Some(8));
        assert_eq!(parse_reg("s0"), Some(8));
        assert_eq!(parse_reg("t6"), Some(31));
        assert_eq!(parse_reg("A0"), None);
    }

    #[test]
    fn fence_sets() {
        assert_eq!(parse_fence_set("iorw"), Some(0xf));
        assert_eq!(parse_fence_set("rw"), Some(0x3));
        assert_eq!(parse_fence_set("io"), Some(0xc));
        assert_eq!(parse_fence_set("wr"), None);
        assert_eq!(parse_fence_set("ii"), None);
        assert_eq!(parse_fence_set(""), None);
    }
}
