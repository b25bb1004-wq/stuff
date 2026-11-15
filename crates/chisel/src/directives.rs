//! Assembler directive parsing, data emission, and LLVM-compatible alignment padding.

use crate::asm::{Item, Pass1Env};
use crate::expr::{parse_expr, Expr};
use crate::expr_eval::{eval_expr, EvalCtx};
use crate::lexer::{is_valid_ident, parse_string_lit, split_operands};
use crate::AsmError;

fn is_ignored_directive(head: &str) -> bool {
    matches!(
        head,
        ".option"
            | ".text"
            | ".data"
            | ".section"
            | ".globl"
            | ".global"
            | ".type"
            | ".size"
            | ".file"
            | ".attribute"
    )
}

fn align_pad_len(addr: u64, align: u64) -> usize {
    let mask = align - 1;
    ((align.wrapping_sub(addr & mask)) & mask) as usize
}

/// Emit LLVM-style instruction alignment padding (`00`, `01 00`, `13 00 00 00`).
pub(crate) fn emit_align_pad(out: &mut Vec<u8>, mut addr: u64, mut rem: usize) {
    if rem > 0 && (addr & 1) == 1 {
        out.push(0x00);
        addr = addr.wrapping_add(1);
        rem -= 1;
    }
    if rem >= 2 && (addr & 3) == 2 {
        out.extend_from_slice(&[0x01, 0x00]);
        rem -= 2;
    }
    while rem >= 4 {
        out.extend_from_slice(&[0x13, 0x00, 0x00, 0x00]);
        rem -= 4;
    }
}

fn parse_data_exprs(tail: &str, line: usize) -> Result<Vec<Expr>, AsmError> {
    let ops = split_operands(tail, line)?;
    if ops.is_empty() {
        return Err(AsmError::new(line, "expected at least one data value"));
    }
    ops.iter().map(|s| parse_expr(s, line)).collect()
}

/// Parse a `.directive` statement in Pass 1.
pub(crate) fn parse_directive(
    head: &str,
    tail: &str,
    line: usize,
    stmt_idx: usize,
    pc: u64,
    env: &mut Pass1Env,
) -> Result<(Item, usize), AsmError> {
    if is_ignored_directive(head) {
        return Ok((Item::None, 0));
    }
    let ctx = EvalCtx {
        line,
        stmt_idx,
        pc,
        equs: &env.equs,
        labels: &env.labels,
        num_labels: &env.num_labels,
        pcrel_auipcs: None,
        allow_pcrel_hi: false,
    };
    match head {
        ".equ" | ".set" => {
            let ops = split_operands(tail, line)?;
            if ops.len() != 2 {
                return Err(AsmError::new(line, format!("{head} expects 2 operands")));
            }
            let name = ops[0].trim();
            if !is_valid_ident(name) {
                return Err(AsmError::new(
                    line,
                    format!("invalid constant name '{name}'"),
                ));
            }
            let expr = parse_expr(ops[1], line)?;
            let val = eval_expr(&expr, &ctx)?;
            env.equs.insert(name.to_string(), val);
            Ok((
                Item::Equ {
                    name: name.to_string(),
                    expr,
                },
                0,
            ))
        }
        ".byte" => {
            let exprs = parse_data_exprs(tail, line)?;
            let len = exprs.len();
            Ok((Item::Data { width: 1, exprs }, len))
        }
        ".half" | ".2byte" => {
            let exprs = parse_data_exprs(tail, line)?;
            let len = exprs.len() * 2;
            Ok((Item::Data { width: 2, exprs }, len))
        }
        ".word" | ".4byte" | ".long" => {
            let exprs = parse_data_exprs(tail, line)?;
            let len = exprs.len() * 4;
            Ok((Item::Data { width: 4, exprs }, len))
        }
        ".dword" | ".8byte" | ".quad" => {
            let exprs = parse_data_exprs(tail, line)?;
            let len = exprs.len() * 8;
            Ok((Item::Data { width: 8, exprs }, len))
        }
        ".zero" | ".space" => {
            let ops = split_operands(tail, line)?;
            if ops.is_empty() || (head == ".zero" && ops.len() != 1) || ops.len() > 2 {
                return Err(AsmError::new(
                    line,
                    format!("wrong operand count for {head}"),
                ));
            }
            let n = eval_expr(&parse_expr(ops[0], line)?, &ctx)?;
            if n.label_balance != 0 || n.val < 0 || n.val > 64 * 1024 * 1024 {
                return Err(AsmError::new(line, format!("invalid size in {head}")));
            }
            let fill = if ops.len() == 2 {
                let f = eval_expr(&parse_expr(ops[1], line)?, &ctx)?;
                if f.label_balance != 0 || !(-128..=255).contains(&f.val) {
                    return Err(AsmError::new(line, "fill byte out of range"));
                }
                f.val as u8
            } else {
                0
            };
            let bytes = vec![fill; n.val as usize];
            let len = bytes.len();
            Ok((Item::Bytes(bytes), len))
        }
        ".ascii" | ".asciz" | ".string" => {
            let ops = split_operands(tail, line)?;
            if ops.is_empty() {
                return Err(AsmError::new(line, format!("{head} expects a string")));
            }
            let nul = head != ".ascii";
            let mut bytes = Vec::new();
            for op in ops {
                bytes.extend_from_slice(&parse_string_lit(op, line)?);
                if nul {
                    bytes.push(0);
                }
            }
            let len = bytes.len();
            Ok((Item::Bytes(bytes), len))
        }
        ".balign" => {
            let ops = split_operands(tail, line)?;
            if ops.len() != 1 {
                return Err(AsmError::new(line, ".balign expects 1 operand"));
            }
            let v = eval_expr(&parse_expr(ops[0], line)?, &ctx)?;
            if v.label_balance != 0 || v.val <= 0 || !(v.val as u64).is_power_of_two() {
                return Err(AsmError::new(
                    line,
                    ".balign alignment must be a power of two",
                ));
            }
            let align = v.val as u64;
            if align > (1 << 30) {
                return Err(AsmError::new(line, ".balign alignment too large"));
            }
            let pad = align_pad_len(pc, align);
            Ok((Item::AlignPad(pad), pad))
        }
        ".p2align" | ".align" => {
            let ops = split_operands(tail, line)?;
            if ops.len() != 1 {
                return Err(AsmError::new(line, format!("{head} expects 1 operand")));
            }
            let v = eval_expr(&parse_expr(ops[0], line)?, &ctx)?;
            if v.label_balance != 0 || !(0..=30).contains(&v.val) {
                return Err(AsmError::new(
                    line,
                    format!("{head} shift out of range 0..30"),
                ));
            }
            let align = 1u64 << (v.val as u32);
            let pad = align_pad_len(pc, align);
            Ok((Item::AlignPad(pad), pad))
        }
        _ => Err(AsmError::new(line, format!("unknown directive '{head}'"))),
    }
}

/// Evaluate and emit `.byte`/`.half`/`.word`/`.dword` values in Pass 2.
pub(crate) fn emit_data(
    width: u8,
    exprs: &[Expr],
    ctx: &EvalCtx<'_>,
    out: &mut Vec<u8>,
) -> Result<(), AsmError> {
    for expr in exprs {
        let v = eval_expr(expr, ctx)?;
        if v.label_balance != 0 && width != 4 && width != 8 {
            return Err(AsmError::new(ctx.line, "relocatable label does not fit"));
        }
        if v.label_balance != 0 && v.label_balance != 1 {
            return Err(AsmError::new(ctx.line, "invalid label expression in data"));
        }
        match width {
            1 => {
                if !(-128..=255).contains(&v.val) {
                    return Err(AsmError::new(ctx.line, "data value out of range for .byte"));
                }
                out.push(v.val as u8);
            }
            2 => {
                if !(-32768..=65535).contains(&v.val) {
                    return Err(AsmError::new(ctx.line, "data value out of range for .half"));
                }
                out.extend_from_slice(&(v.val as u16).to_le_bytes());
            }
            4 => {
                if v.label_balance == 0 && !(-2_147_483_648..=4_294_967_295).contains(&v.val) {
                    return Err(AsmError::new(ctx.line, "data value out of range for .word"));
                }
                out.extend_from_slice(&(v.val as u32).to_le_bytes());
            }
            8 => {
                out.extend_from_slice(&(v.val as u64).to_le_bytes());
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}
