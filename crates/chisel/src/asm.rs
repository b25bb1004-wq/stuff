//! Two-pass RV64IMAC assembler driver.

use std::collections::HashMap;

use crate::directives::{emit_align_pad, emit_data, parse_directive};

/// Largest assembled output in bytes.
const MAX_OUTPUT: usize = 256 * 1024 * 1024;
use crate::encode_c::{encode_c, CInst};
use crate::encode_rv::{encode_rv, RvInst};
use crate::expr::Expr;
use crate::expr_eval::{eval_expr, EvalCtx, EvalVal};
use crate::lexer::{preprocess, split_head, split_operands, LabelDef};
use crate::parse_c::parse_c;
use crate::parse_rv::parse_rv;
use crate::{AsmError, Output};

#[derive(Debug, Clone)]
pub(crate) enum Item {
    None,
    Equ { name: String, expr: Expr },
    Bytes(Vec<u8>),
    Data { width: u8, exprs: Vec<Expr> },
    AlignPad(usize),
    C(CInst),
    Rv(RvInst),
}

struct ParsedStmt {
    line: usize,
    pc: u64,
    item: Item,
}

fn update_err(best: &mut Option<AsmError>, err: AsmError) {
    match best {
        Some(prev) if prev.line <= err.line => {}
        _ => *best = Some(err),
    }
}

pub(crate) struct Pass1Env {
    pub equs: HashMap<String, EvalVal>,
    pub labels: HashMap<String, u64>,
    pub num_labels: HashMap<u64, Vec<(usize, u64)>>,
}

fn parse_stmt(
    text: &str,
    line: usize,
    stmt_idx: usize,
    pc: u64,
    env: &mut Pass1Env,
) -> Result<(Item, usize), AsmError> {
    if text.is_empty() {
        return Ok((Item::None, 0));
    }
    let (head, tail) = split_head(text);
    if head.starts_with('.') {
        return parse_directive(head, tail, line, stmt_idx, pc, env);
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

    let ops = split_operands(tail, line)?;
    if let Some(cinst) = parse_c(head, &ops, line)? {
        return Ok((Item::C(cinst), 2));
    }
    let rv = parse_rv(head, &ops, &ctx)?;
    let len = rv.byte_len();
    Ok((Item::Rv(rv), len))
}

/// Assemble source text starting at `base`.
pub(crate) fn assemble_impl(src: &str, base: u64) -> Result<Output, AsmError> {
    let (stmts, pre_err) = preprocess(src);

    let mut env = Pass1Env {
        equs: HashMap::new(),
        labels: HashMap::new(),
        num_labels: HashMap::new(),
    };
    let mut symbols: Vec<(String, u64)> = Vec::new();
    let mut parsed: Vec<ParsedStmt> = Vec::with_capacity(stmts.len());
    let mut offset: usize = 0;
    let mut first_err: Option<AsmError> = pre_err;

    for (stmt_idx, st) in stmts.iter().enumerate() {
        let pc = base.wrapping_add(offset as u64);
        for lbl in &st.labels {
            match lbl {
                LabelDef::Named(name) => {
                    if env.labels.contains_key(name) {
                        update_err(
                            &mut first_err,
                            AsmError::new(st.line, format!("duplicate label '{name}'")),
                        );
                    } else {
                        env.labels.insert(name.clone(), pc);
                        symbols.push((name.clone(), pc));
                    }
                }
                LabelDef::Numeric(num) => {
                    env.num_labels.entry(*num).or_default().push((stmt_idx, pc));
                }
            }
        }
        match parse_stmt(&st.text, st.line, stmt_idx, pc, &mut env) {
            Ok((item, len)) => {
                offset = offset.saturating_add(len);
                parsed.push(ParsedStmt {
                    line: st.line,
                    pc,
                    item,
                });
                // each directive is capped, but their sum is not; bound the whole output
                if offset > MAX_OUTPUT {
                    update_err(
                        &mut first_err,
                        AsmError::new(st.line, "output larger than 256 MiB"),
                    );
                    break;
                }
            }
            Err(e) => {
                update_err(&mut first_err, e);
                parsed.push(ParsedStmt {
                    line: st.line,
                    pc,
                    item: Item::None,
                });
            }
        }
    }

    let Pass1Env {
        mut equs,
        labels,
        num_labels,
    } = env;

    let err_cap = first_err.as_ref().map_or(usize::MAX, |e| e.line);
    let mut pcrel_auipcs: HashMap<u64, i64> = HashMap::new();
    equs.clear();
    for (stmt_idx, ps) in parsed.iter().enumerate() {
        if ps.line >= err_cap {
            break;
        }
        match &ps.item {
            Item::Equ { name, expr } => {
                let ctx = EvalCtx {
                    line: ps.line,
                    stmt_idx,
                    pc: ps.pc,
                    equs: &equs,
                    labels: &labels,
                    num_labels: &num_labels,
                    pcrel_auipcs: None,
                    allow_pcrel_hi: false,
                };
                if let Ok(v) = eval_expr(expr, &ctx) {
                    equs.insert(name.clone(), v);
                }
            }
            Item::Rv(rv) => {
                if let Some(inner) = rv.pcrel_hi_inner() {
                    let ctx = EvalCtx {
                        line: ps.line,
                        stmt_idx,
                        pc: ps.pc,
                        equs: &equs,
                        labels: &labels,
                        num_labels: &num_labels,
                        pcrel_auipcs: None,
                        allow_pcrel_hi: false,
                    };
                    match eval_expr(inner, &ctx) {
                        Ok(v) => {
                            pcrel_auipcs.insert(ps.pc, v.val.wrapping_sub(ps.pc as i64));
                        }
                        Err(e) => update_err(&mut first_err, e),
                    }
                }
            }
            _ => {}
        }
    }

    let err_cap = first_err.as_ref().map_or(usize::MAX, |e| e.line);
    let mut bytes = Vec::with_capacity(offset);
    equs.clear();
    for (stmt_idx, ps) in parsed.iter().enumerate() {
        if ps.line >= err_cap {
            break;
        }
        let ctx = EvalCtx {
            line: ps.line,
            stmt_idx,
            pc: ps.pc,
            equs: &equs,
            labels: &labels,
            num_labels: &num_labels,
            pcrel_auipcs: Some(&pcrel_auipcs),
            allow_pcrel_hi: true,
        };
        let res = match &ps.item {
            Item::None => Ok(()),
            Item::Equ { name, expr } => match eval_expr(expr, &ctx) {
                Ok(v) => {
                    equs.insert(name.clone(), v);
                    Ok(())
                }
                Err(e) => Err(e),
            },
            Item::Bytes(b) => {
                bytes.extend_from_slice(b);
                Ok(())
            }
            Item::Data { width, exprs } => emit_data(*width, exprs, &ctx, &mut bytes),
            Item::AlignPad(pad) => {
                emit_align_pad(&mut bytes, ps.pc, *pad);
                Ok(())
            }
            Item::C(cinst) => encode_c(cinst, &ctx).map(|hw| {
                bytes.extend_from_slice(&hw.to_le_bytes());
            }),
            Item::Rv(rv) => encode_rv(rv, &ctx).map(|words| {
                for w in words {
                    bytes.extend_from_slice(&w.to_le_bytes());
                }
            }),
        };
        res?;
    }

    if let Some(e) = first_err {
        return Err(e);
    }
    Ok(Output { bytes, symbols })
}
