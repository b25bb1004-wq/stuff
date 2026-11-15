#![allow(clippy::unwrap_used)]

use super::*;
use assay::{forall, Rng};
use logos::ast::{BinOp, UnOp};
use logos::ir::{Body, Builtin, Func, Inst, Lit, Program, Ty};

fn make_prog(funcs: Vec<Func>, main: Option<u32>) -> Program {
    Program {
        types: Vec::new(),
        layers: Vec::new(),
        funcs,
        tests: Vec::new(),
        props: Vec::new(),
        main,
    }
}

fn int_fn(name: &str, params: u32, regs: Vec<Ty>, insts: Vec<Inst>) -> Func {
    let spans = vec![logos::Span { start: 0, end: 0 }; insts.len()];
    Func {
        name: String::from(name),
        params,
        ret: Ty::Int,
        layers: 0,
        regs,
        body: Body::Code { insts, spans },
    }
}

#[test]
fn entry_errors() {
    let p = make_prog(
        vec![int_fn("foo", 1, vec![Ty::Int], vec![Inst::Ret(0)])],
        None,
    );
    assert_eq!(
        emit_asm(&p, "missing"),
        Err(CodegenError::NoEntry("missing".into()))
    );
    assert_eq!(
        emit_asm(&p, "foo"),
        Err(CodegenError::NoEntry("foo".into()))
    );

    let mut p_bool = make_prog(
        vec![int_fn(
            "b",
            0,
            vec![Ty::Bool],
            vec![Inst::Lit(0, Lit::Bool(true)), Inst::Ret(0)],
        )],
        None,
    );
    p_bool.funcs[0].ret = Ty::Bool;
    assert_eq!(
        emit_asm(&p_bool, "b"),
        Err(CodegenError::NoEntry("b".into()))
    );
}

#[test]
fn malformed_ir_never_panics() {
    let p_dup_lbl = make_prog(
        vec![int_fn(
            "main",
            0,
            vec![Ty::Int],
            vec![Inst::Label(1), Inst::Label(1), Inst::Ret(0)],
        )],
        Some(0),
    );
    assert!(matches!(
        emit_asm(&p_dup_lbl, "main"),
        Err(CodegenError::Unsupported(_))
    ));

    let p_bad_jmp = make_prog(
        vec![int_fn("main", 0, vec![Ty::Int], vec![Inst::Jump(99)])],
        Some(0),
    );
    assert!(matches!(
        emit_asm(&p_bad_jmp, "main"),
        Err(CodegenError::Unsupported(_))
    ));

    let p_bad_reg = make_prog(
        vec![int_fn("main", 0, vec![Ty::Int], vec![Inst::Ret(5)])],
        Some(0),
    );
    assert!(matches!(
        emit_asm(&p_bad_reg, "main"),
        Err(CodegenError::Unsupported(_))
    ));

    let p_too_many_params = make_prog(
        vec![
            int_fn(
                "main",
                0,
                vec![Ty::Int],
                vec![Inst::Lit(0, Lit::Int(0)), Inst::Ret(0)],
            ),
            int_fn("many", 9, vec![Ty::Int; 9], vec![Inst::Ret(0)]),
        ],
        Some(0),
    );
    assert!(matches!(
        emit_asm(&p_too_many_params, "main"),
        Err(CodegenError::Unsupported(_))
    ));
}

#[test]
fn unsupported_ir_instructions_named() {
    let insts: &[(&str, Inst)] = &[
        ("Lit::Byte", Inst::Lit(0, Lit::Byte(1))),
        ("Lit::Str", Inst::Lit(0, Lit::Str(String::from("hi")))),
        ("Lit::Fn", Inst::Lit(0, Lit::Fn(0))),
        ("CallValue", Inst::CallValue(0, 0, vec![])),
        ("Builtin", Inst::Builtin(0, Builtin::Abs, vec![0])),
        ("NewArray", Inst::NewArray(0, vec![0])),
        ("NewFill", Inst::NewFill(0, 0, 0)),
        ("NewTuple", Inst::NewTuple(0, vec![0, 0])),
        ("NewRecord", Inst::NewRecord(0, 0, vec![0])),
        ("NewVariant", Inst::NewVariant(0, 0, 0, vec![])),
        ("NewSome", Inst::NewSome(0, 0)),
        ("NewNone", Inst::NewNone(0)),
        ("GetField", Inst::GetField(0, 0, 0)),
        ("SetField", Inst::SetField(0, 0, 0)),
        ("Tag", Inst::Tag(0, 0)),
        ("GetIndex", Inst::GetIndex(0, 0, 0)),
        ("SetIndex", Inst::SetIndex(0, 0, 0)),
    ];
    for (needle, inst) in insts {
        let p = make_prog(
            vec![int_fn(
                "main",
                0,
                vec![Ty::Int],
                vec![inst.clone(), Inst::Ret(0)],
            )],
            Some(0),
        );
        match emit_asm(&p, "main") {
            Err(CodegenError::Unsupported(msg)) => {
                assert!(msg.contains(needle), "expected '{needle}' in '{msg}'");
            }
            other => panic!("expected Unsupported({needle}), got {other:?}"),
        }
    }
}

#[test]
fn bin_and_or_and_self_aliasing() {
    for a in [false, true] {
        for b in [false, true] {
            let p_and = make_prog(
                vec![int_fn(
                    "main",
                    0,
                    vec![Ty::Bool, Ty::Bool, Ty::Int],
                    vec![
                        Inst::Lit(0, Lit::Bool(a)),
                        Inst::Lit(1, Lit::Bool(b)),
                        Inst::Bin(0, BinOp::And, 0, 1),
                        Inst::Ret(0),
                    ],
                )],
                Some(0),
            );
            let (exit, out) = run(&p_and, "main", 10_000);
            assert_eq!(exit, silicon::machine::Exit::Pass);
            assert_eq!(out, format!("{}\n", i64::from(a && b)));

            let p_or = make_prog(
                vec![int_fn(
                    "main",
                    0,
                    vec![Ty::Bool, Ty::Bool, Ty::Int],
                    vec![
                        Inst::Lit(0, Lit::Bool(a)),
                        Inst::Lit(1, Lit::Bool(b)),
                        Inst::Bin(0, BinOp::Or, 0, 1),
                        Inst::Ret(0),
                    ],
                )],
                Some(0),
            );
            let (exit, out) = run(&p_or, "main", 10_000);
            assert_eq!(exit, silicon::machine::Exit::Pass);
            assert_eq!(out, format!("{}\n", i64::from(a || b)));
        }
    }

    let p_alias = make_prog(
        vec![int_fn(
            "main",
            0,
            vec![Ty::Int],
            vec![
                Inst::Lit(0, Lit::Int(21)),
                Inst::Bin(0, BinOp::Add, 0, 0),
                Inst::Un(0, UnOp::Neg, 0),
                Inst::Ret(0),
            ],
        )],
        Some(0),
    );
    let (exit, out) = run(&p_alias, "main", 10_000);
    assert_eq!(exit, silicon::machine::Exit::Pass);
    assert_eq!(out, "-42\n");
}

#[test]
fn fuzzed_ir_never_panics() {
    forall(200, 0x5177_E200, |r| {
        let p = random_ir(r);
        let _ = emit_asm(&p, "main");
        if r.chance(1, 5) {
            let _ = compile_elf(&p, "main");
        }
    });
}

fn random_ir(r: &mut Rng) -> Program {
    let n_funcs = r.range(1, 5) as usize;
    let mut funcs = Vec::with_capacity(n_funcs);
    for i in 0..n_funcs {
        let name = if i == 0 {
            String::from("main")
        } else {
            format!("f{i}")
        };
        let params = if i == 0 { 0 } else { r.range(0, 10) as u32 };
        let n_regs = r.range(0, 16) as usize;
        let regs: Vec<Ty> = (0..n_regs).map(|_| rand_ty(r)).collect();
        let ret = rand_ty(r);
        let body = if r.chance(1, 10) {
            Body::Extern(String::from("io.print"))
        } else {
            let n_insts = r.range(0, 25) as usize;
            let insts: Vec<Inst> = (0..n_insts)
                .map(|_| rand_inst(r, n_regs as u32, n_funcs as u32))
                .collect();
            let spans = vec![logos::Span { start: 0, end: 0 }; insts.len()];
            Body::Code { insts, spans }
        };
        funcs.push(Func {
            name,
            params,
            ret,
            layers: 0,
            regs,
            body,
        });
    }
    make_prog(funcs, Some(0))
}

fn rand_ty(r: &mut Rng) -> Ty {
    match r.below(8) {
        0..=2 => Ty::Int,
        3 | 4 => Ty::Bool,
        5 => Ty::Unit,
        6 => Ty::Byte,
        _ => Ty::Str,
    }
}

fn rand_inst(r: &mut Rng, max_reg: u32, max_fn: u32) -> Inst {
    let reg = |r: &mut Rng| r.range(0, u64::from(max_reg.max(1)) + 2) as u32;
    let lbl = |r: &mut Rng| r.range(0, 6) as u32;
    match r.below(14) {
        0 => Inst::Lit(reg(r), Lit::Int(r.u64() as i64)),
        1 => Inst::Lit(reg(r), Lit::Bool(r.chance(1, 2))),
        2 => Inst::Lit(reg(r), Lit::Unit),
        3 => Inst::Move(reg(r), reg(r)),
        4 => Inst::Un(
            reg(r),
            if r.chance(1, 2) { UnOp::Neg } else { UnOp::Not },
            reg(r),
        ),
        5 => {
            let ops = [
                BinOp::Add,
                BinOp::Sub,
                BinOp::Mul,
                BinOp::Div,
                BinOp::Rem,
                BinOp::Eq,
                BinOp::Ne,
                BinOp::Lt,
                BinOp::Le,
                BinOp::Gt,
                BinOp::Ge,
                BinOp::And,
                BinOp::Or,
                BinOp::AddWrap,
                BinOp::BitXor,
            ];
            Inst::Bin(reg(r), *r.pick(&ops), reg(r), reg(r))
        }
        6 => {
            let n = r.below(5);
            let args = (0..n).map(|_| reg(r)).collect();
            Inst::Call(reg(r), r.range(0, u64::from(max_fn) + 1) as u32, args)
        }
        7 => Inst::Label(lbl(r)),
        8 => Inst::Jump(lbl(r)),
        9 => Inst::Branch(reg(r), lbl(r), lbl(r)),
        10 => Inst::Ret(reg(r)),
        11 => Inst::Assert(reg(r), String::from("chk")),
        12 => Inst::Builtin(reg(r), Builtin::Abs, vec![reg(r)]),
        _ => Inst::NewNone(reg(r)),
    }
}
