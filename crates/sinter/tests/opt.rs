//! Tests for the sinter IR optimiser: differential runs against the Mica evaluator and pass unit tests.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use assay::Rng;
use mica::ast::BinOp;
use mica::check::check;
use mica::eval::{run_main, Outcome};
use mica::ir::{Body, Func, Inst, Lit, Program, Ty};
use mica::parser::parse;
use mica::Span;
use sinter::opt::{optimize, Stats};

const FUEL: u64 = 1_000_000;
const WANTED: usize = 300;

/// Output plus trap kind; spans are ignored because removed instructions shift them.
fn key(o: &Outcome) -> (String, Result<(), String>) {
    (
        o.output.clone(),
        o.result.clone().map_err(|(t, _)| format!("{t:?}")),
    )
}

fn int_leaf(rng: &mut Rng, nvars: usize) -> String {
    if nvars > 0 && rng.chance(1, 2) {
        return format!("v{}", rng.below(nvars));
    }
    let lits = [
        "0",
        "1",
        "2",
        "3",
        "7",
        "9223372036854775807",
        "4611686018427387904",
    ];
    lits[rng.below(lits.len())].to_string()
}

fn int_expr(rng: &mut Rng, nvars: usize, depth: u32) -> String {
    if depth == 0 || rng.chance(1, 3) {
        return int_leaf(rng, nvars);
    }
    match rng.below(4) {
        0 => {
            let op = *rng.pick(&["+", "-", "*", "/", "%", "+%", "-%", "*%"]);
            format!(
                "({} {op} {})",
                int_expr(rng, nvars, depth - 1),
                int_expr(rng, nvars, depth - 1)
            )
        }
        1 => format!("(-{})", int_expr(rng, nvars, depth - 1)),
        _ => int_leaf(rng, nvars),
    }
}

fn cmp(rng: &mut Rng, nvars: usize) -> String {
    let op = *rng.pick(&["==", "!=", "<", "<=", ">", ">="]);
    format!(
        "({} {op} {})",
        int_expr(rng, nvars, 2),
        int_expr(rng, nvars, 2)
    )
}

fn bool_expr(rng: &mut Rng, nvars: usize) -> String {
    match rng.below(4) {
        0 => format!("({} && {})", cmp(rng, nvars), cmp(rng, nvars)),
        1 => format!("({} || {})", cmp(rng, nvars), cmp(rng, nvars)),
        2 => format!("(!{})", cmp(rng, nvars)),
        _ => cmp(rng, nvars),
    }
}

/// Random small Mica program; many fail the checker and are skipped by the caller.
fn gen_program(rng: &mut Rng) -> String {
    let nv = 2 + rng.below(4);
    let mut src =
        String::from("fn print(s: str) uses io = extern \"io.print\"\nfn main() uses io {\n");
    for i in 0..nv {
        src += &format!("  var v{i} = {}\n", int_expr(rng, i, 2));
    }
    for _ in 0..(1 + rng.below(4)) {
        let i = rng.below(nv);
        let j = rng.below(nv);
        match rng.below(3) {
            0 => src += &format!("  v{i} = {}\n", int_expr(rng, nv, 2)),
            1 => {
                let c = bool_expr(rng, nv);
                let a = int_expr(rng, nv, 2);
                let b = int_expr(rng, nv, 2);
                src += &format!("  if {c} {{ v{i} = {a} }} else {{ v{j} = {b} }}\n");
            }
            _ => {
                let c = bool_expr(rng, nv);
                src += &format!("  if {c} {{ v{i} = v{j} + 1 }}\n");
            }
        }
    }
    for i in 0..nv {
        src += &format!("  print(str(v{i}))\n");
    }
    src += "}\n";
    src
}

#[test]
fn differential_random_programs() {
    let mut checked = 0usize;
    let mut traps = 0usize;
    let mut changed = 0usize;
    for seed in 0..(WANTED as u64 * 3) {
        let mut rng = Rng::new(seed);
        let src = gen_program(&mut rng);
        let Ok(module) = parse(&src) else { continue };
        let Ok(before) = check(&module) else { continue };
        checked += 1;
        let mut after = before.clone();
        let stats = optimize(&mut after);
        if stats != Stats::default() {
            changed += 1;
        }
        let want = run_main(&before, "", FUEL);
        let got = run_main(&after, "", FUEL);
        if want.result.is_err() {
            traps += 1;
        }
        assert_eq!(
            key(&want),
            key(&got),
            "outcome changed for seed {seed}:\n{src}"
        );
        let again = optimize(&mut after);
        assert_eq!(again, Stats::default(), "not idempotent for seed {seed}");
        assert_spans_parallel(&after);
    }
    assert!(
        checked >= WANTED,
        "only {checked} programs passed the checker"
    );
    assert!(
        changed > 0 && traps > 0,
        "generator does not exercise folds ({changed}) or traps ({traps})"
    );
}

fn assert_spans_parallel(p: &Program) {
    for f in &p.funcs {
        if let Body::Code { insts, spans } = &f.body {
            assert_eq!(insts.len(), spans.len());
        }
    }
}

fn single(regs: Vec<Ty>, params: u32, insts: Vec<Inst>) -> Program {
    let spans = vec![Span { start: 0, end: 0 }; insts.len()];
    Program {
        types: vec![],
        layers: vec![],
        funcs: vec![Func {
            name: String::from("f"),
            params,
            ret: Ty::Int,
            layers: 0,
            regs,
            body: Body::Code { insts, spans },
        }],
        tests: vec![],
        props: vec![],
        main: Some(0),
    }
}

fn code(p: &Program) -> Vec<Inst> {
    match &p.funcs[0].body {
        Body::Code { insts, .. } => insts.clone(),
        Body::Extern(_) => panic!("expected code"),
    }
}

fn ints(n: usize) -> Vec<Ty> {
    vec![Ty::Int; n]
}

#[test]
fn fold_int_add_to_literal() {
    let mut p = single(
        ints(4),
        0,
        vec![
            Inst::Lit(1, Lit::Int(2)),
            Inst::Lit(2, Lit::Int(3)),
            Inst::Bin(3, BinOp::Add, 1, 2),
            Inst::Ret(3),
        ],
    );
    let s = optimize(&mut p);
    assert_eq!(s.folded, 1);
    assert!(code(&p).contains(&Inst::Lit(3, Lit::Int(5))));
}

#[test]
fn fold_keeps_trapping_int_ops() {
    let cases = [
        (i64::MAX, 1, BinOp::Add),
        (i64::MIN, -1, BinOp::Div),
        (i64::MIN, -1, BinOp::Rem),
        (5, 0, BinOp::Div),
        (5, 0, BinOp::Rem),
        (i64::MIN, 1, BinOp::Sub),
    ];
    for (a, b, op) in cases {
        let mut p = single(
            ints(4),
            0,
            vec![
                Inst::Lit(1, Lit::Int(a)),
                Inst::Lit(2, Lit::Int(b)),
                Inst::Bin(3, op, 1, 2),
                Inst::Ret(3),
            ],
        );
        optimize(&mut p);
        let c = code(&p);
        assert!(
            c.iter().any(|i| matches!(i, Inst::Bin(_, _, _, _))),
            "{op:?} {a} {b} was folded: {c:?}"
        );
    }
}

#[test]
fn fold_byte_add_and_overflow() {
    let mut p = single(
        vec![Ty::Byte; 4],
        0,
        vec![
            Inst::Lit(1, Lit::Byte(200)),
            Inst::Lit(2, Lit::Byte(50)),
            Inst::Bin(3, BinOp::Add, 1, 2),
            Inst::Ret(3),
        ],
    );
    optimize(&mut p);
    assert!(code(&p).contains(&Inst::Lit(3, Lit::Byte(250))));

    let mut q = single(
        vec![Ty::Byte; 4],
        0,
        vec![
            Inst::Lit(1, Lit::Byte(200)),
            Inst::Lit(2, Lit::Byte(100)),
            Inst::Bin(3, BinOp::Add, 1, 2),
            Inst::Ret(3),
        ],
    );
    optimize(&mut q);
    assert!(matches!(code(&q)[2], Inst::Bin(_, _, _, _)));
}

#[test]
fn fold_branch_on_known_bool() {
    let mut p = single(
        vec![Ty::Bool, Ty::Int, Ty::Int],
        0,
        vec![
            Inst::Lit(0, Lit::Bool(true)),
            Inst::Branch(0, 1, 2),
            Inst::Label(1),
            Inst::Lit(1, Lit::Int(1)),
            Inst::Ret(1),
            Inst::Label(2),
            Inst::Lit(2, Lit::Int(2)),
            Inst::Ret(2),
        ],
    );
    let s = optimize(&mut p);
    assert_eq!(s.folded, 1);
    let c = code(&p);
    assert_eq!(c[0], Inst::Jump(1));
    assert!(c.contains(&Inst::Lit(1, Lit::Int(1))));
}

#[test]
fn fold_does_not_cross_label() {
    let mut p = single(
        ints(4),
        0,
        vec![
            Inst::Lit(1, Lit::Int(2)),
            Inst::Label(9),
            Inst::Bin(3, BinOp::Add, 1, 1),
            Inst::Ret(3),
        ],
    );
    let s = optimize(&mut p);
    assert_eq!(s.folded, 0);
    assert!(matches!(code(&p)[2], Inst::Bin(_, _, _, _)));
}

#[test]
fn copy_propagation_rewrites_reads_and_drops_move() {
    let mut p = single(
        ints(3),
        1,
        vec![
            Inst::Move(1, 0),
            Inst::Bin(2, BinOp::Add, 1, 1),
            Inst::Ret(2),
        ],
    );
    let s = optimize(&mut p);
    assert_eq!(s.copies, 2);
    assert_eq!(s.removed, 1);
    assert_eq!(code(&p), vec![Inst::Bin(2, BinOp::Add, 0, 0), Inst::Ret(2)]);
}

#[test]
fn copy_propagation_stops_at_label() {
    let mut p = single(
        ints(3),
        1,
        vec![
            Inst::Move(1, 0),
            Inst::Label(4),
            Inst::Bin(2, BinOp::Add, 1, 1),
            Inst::Ret(2),
        ],
    );
    let s = optimize(&mut p);
    assert_eq!(s.copies, 0);
    assert_eq!(code(&p)[2], Inst::Bin(2, BinOp::Add, 1, 1));
}

#[test]
fn copy_propagation_skips_heap_values() {
    let tup = Ty::Tuple(vec![Ty::Int, Ty::Int]);
    let mut p = single(
        vec![tup.clone(), tup, Ty::Int],
        1,
        vec![Inst::Move(1, 0), Inst::GetField(2, 1, 0), Inst::Ret(2)],
    );
    let s = optimize(&mut p);
    assert_eq!(s.copies, 0);
}

#[test]
fn dead_code_after_ret_is_removed_up_to_label() {
    let mut p = single(
        ints(2),
        0,
        vec![
            Inst::Lit(0, Lit::Int(1)),
            Inst::Ret(0),
            Inst::Lit(1, Lit::Int(2)),
            Inst::Label(3),
            Inst::Ret(0),
        ],
    );
    let s = optimize(&mut p);
    assert_eq!(s.removed, 1);
    assert_eq!(
        code(&p),
        vec![
            Inst::Lit(0, Lit::Int(1)),
            Inst::Ret(0),
            Inst::Label(3),
            Inst::Ret(0)
        ]
    );
}

#[test]
fn pure_unread_results_removed_but_trapping_kept() {
    let mut p = single(
        ints(4),
        2,
        vec![
            Inst::Bin(2, BinOp::Eq, 0, 1),
            Inst::Bin(3, BinOp::Div, 0, 1),
            Inst::Lit(4, Lit::Int(0)),
            Inst::Ret(4),
        ],
    );
    let s = optimize(&mut p);
    assert_eq!(s.removed, 1);
    let c = code(&p);
    assert!(
        matches!(c[0], Inst::Bin(3, BinOp::Div, 0, 1)),
        "trapping Div must stay: {c:?}"
    );
    assert_eq!(c.len(), 3);
}

#[test]
fn extern_bodies_untouched() {
    let mut p = single(ints(2), 1, vec![Inst::Move(0, 0), Inst::Ret(0)]);
    p.funcs.push(Func {
        name: String::from("host"),
        params: 0,
        ret: Ty::Int,
        layers: 0,
        regs: vec![],
        body: Body::Extern(String::from("host.x")),
    });
    let before = p.clone();
    let s = optimize(&mut p);
    assert_eq!(
        s,
        Stats {
            folded: 0,
            copies: 0,
            removed: 1
        }
    );
    assert_eq!(p.funcs[0].params, before.funcs[0].params);
    assert_eq!(p.funcs[0].regs, before.funcs[0].regs);
    assert_eq!(p.funcs[1], before.funcs[1]);
}

#[test]
fn second_run_changes_nothing() {
    let mut p = single(
        ints(4),
        1,
        vec![
            Inst::Lit(1, Lit::Int(4)),
            Inst::Lit(2, Lit::Int(6)),
            Inst::Bin(3, BinOp::Mul, 1, 2),
            Inst::Move(0, 3),
            Inst::Bin(2, BinOp::Add, 0, 0),
            Inst::Ret(2),
        ],
    );
    let first = optimize(&mut p);
    assert_ne!(first, Stats::default());
    let snapshot = code(&p);
    assert_eq!(optimize(&mut p), Stats::default());
    assert_eq!(code(&p), snapshot);
}
