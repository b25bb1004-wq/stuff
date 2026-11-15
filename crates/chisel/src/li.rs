//! 64-bit `li` immediate synthesis matching LLVM 19's instruction-count rules.

use silicon::inst::{encode, Inst};

/// Synthesize a minimal RV64I instruction sequence loading `val` into `rd`.
pub(crate) fn synth_li(rd: u8, val: i64) -> Vec<u32> {
    if (-2048..=2047).contains(&val) {
        return vec![encode(Inst::Addi {
            rd,
            rs1: 0,
            imm: val,
        })];
    }
    if val >= i64::from(i32::MIN) && val <= i64::from(i32::MAX) {
        if (val & 0xfff) == 0 {
            return vec![encode(Inst::Lui { rd, imm: val })];
        }
        let lo = (val << 52) >> 52;
        let hi = (val.wrapping_sub(lo)) as i32 as i64;
        return vec![
            encode(Inst::Lui { rd, imm: hi }),
            encode(Inst::Addiw {
                rd,
                rs1: rd,
                imm: lo,
            }),
        ];
    }
    let mut seq = Vec::new();
    synth_li64(rd, val, &mut seq);
    seq
}

fn synth_li64(rd: u8, val: i64, out: &mut Vec<u32>) {
    if (-2048..=2047).contains(&val) {
        out.push(encode(Inst::Addi {
            rd,
            rs1: 0,
            imm: val,
        }));
        return;
    }
    if val >= i64::from(i32::MIN) && val <= i64::from(i32::MAX) {
        let lo = (val << 52) >> 52;
        let hi = (val.wrapping_sub(lo)) as i32 as i64;
        if hi != 0 {
            out.push(encode(Inst::Lui { rd, imm: hi }));
            if lo != 0 {
                out.push(encode(Inst::Addiw {
                    rd,
                    rs1: rd,
                    imm: lo,
                }));
            }
        } else {
            out.push(encode(Inst::Addi {
                rd,
                rs1: 0,
                imm: lo,
            }));
        }
        return;
    }
    let lo = (val << 52) >> 52;
    let rem = val.wrapping_sub(lo);
    let shift = i64::from(rem.trailing_zeros());
    let hi = rem >> shift;
    synth_li64(rd, hi, out);
    out.push(encode(Inst::Slli {
        rd,
        rs1: rd,
        imm: shift,
    }));
    if lo != 0 {
        out.push(encode(Inst::Addi {
            rd,
            rs1: rd,
            imm: lo,
        }));
    }
}
