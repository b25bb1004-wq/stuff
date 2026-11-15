//! Leader check for T29a: random multi-function integer Logos programs (loops, branches, asserts,
//! Bool parameters and results, calls, recursion near the depth limit, overflow and division
//! traps) run on Silicon through `cobalt::rv64::run` and on Elysium; result and trap class must
//! match. Plus random IR (in and out of range) that must never panic `emit_asm`.
#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
use assay::Rng;
use cobalt::rv64::{emit_asm, run, CodegenError};
use elysium::{Heap, Host, Limits, Trap as GTrap, Value, Vm};
use logos::ast::{BinOp, UnOp};
use logos::ir::{Body, Func, Inst, Lit, Program, Ty};
use logos::Span;
use silicon::machine::Exit;

struct NoHost;
impl Host for NoHost {
    fn call(&mut self, name: &str, _: &[Value], _: &mut Heap) -> Result<Value, GTrap> {
        Err(GTrap::Host(name.to_string()))
    }
}

/// Elysium outcome: Ok(value), Err(trap class), or None when fuel ran out (case skipped).
fn elysium(p: &Program) -> Option<Result<i64, u32>> {
    let c = cobalt::compile(p);
    let mut vm = Vm::new(c.module, &[]).unwrap();
    let lim = Limits {
        fuel: 3_000_000,
        max_heap_bytes: 1 << 24,
        max_depth: 10_000,
    };
    match vm.call("main", &[], &mut NoHost, lim) {
        Ok(Value::Int(v)) => Some(Ok(v)),
        Err(GTrap::Overflow) => Some(Err(1)),
        Err(GTrap::DivideByZero) => Some(Err(2)),
        Err(GTrap::AssertFailed(_)) => Some(Err(3)),
        Err(GTrap::StackOverflow) => Some(Err(4)),
        Err(GTrap::OutOfFuel) => None,
        other => panic!("unexpected elysium result {other:?}"),
    }
}

#[derive(Clone)]
struct Sig {
    name: String,
    params: Vec<bool>, // true = bool
    ret_bool: bool,
}

struct G<'a> {
    r: &'a mut Rng,
    ints: Vec<String>,
    bools: Vec<String>,
    mut_ints: Vec<String>,
    mut_bools: Vec<String>,
    callable: Vec<Sig>,
    fresh: u32,
}

const LITS: [&str; 14] = [
    "0",
    "1",
    "(-1)",
    "2",
    "7",
    "(-13)",
    "100",
    "2147483648",
    "3037000500",
    "(-3037000500)",
    "9223372036854775807",
    "(-9223372036854775807 - 1)",
    "(-2)",
    "4611686018427387904",
];

impl G<'_> {
    fn name(&mut self, p: &str) -> String {
        self.fresh += 1;
        format!("{p}{}", self.fresh)
    }
    fn int(&mut self, d: u32) -> String {
        if d == 0 || self.r.chance(1, 3) {
            if !self.ints.is_empty() && self.r.chance(2, 3) {
                return self.r.pick(&self.ints).clone();
            }
            if self.r.chance(1, 6) {
                return (*self.r.pick(&LITS)).to_string();
            }
            return format!("{}", self.r.below(41) as i64 - 20);
        }
        match self.r.below(9) {
            0..=3 => {
                let op = *self.r.pick(&["+", "-", "*", "/", "%", "+", "-", "+", "-"]);
                format!("({} {op} {})", self.int(d - 1), self.int(d - 1))
            }
            4 => format!("(-{})", self.int(d - 1)),
            5 => format!(
                "(if {} {{ {} }} else {{ {} }})",
                self.bool(d - 1),
                self.int(d - 1),
                self.int(d - 1)
            ),
            _ => self.call(d, false).unwrap_or_else(|| self.int(d - 1)),
        }
    }
    fn bool(&mut self, d: u32) -> String {
        if d == 0 || self.r.chance(1, 4) {
            if !self.bools.is_empty() && self.r.chance(1, 2) {
                return self.r.pick(&self.bools).clone();
            }
            return (*self.r.pick(&["true", "false"])).to_string();
        }
        match self.r.below(8) {
            0..=2 => {
                let op = *self.r.pick(&["==", "!=", "<", "<=", ">", ">="]);
                format!("({} {op} {})", self.int(d - 1), self.int(d - 1))
            }
            3 => format!("({} && {})", self.bool(d - 1), self.bool(d - 1)),
            4 => format!("({} || {})", self.bool(d - 1), self.bool(d - 1)),
            5 => format!("(!{})", self.bool(d - 1)),
            _ => self.call(d, true).unwrap_or_else(|| self.bool(d - 1)),
        }
    }
    fn call(&mut self, d: u32, want_bool: bool) -> Option<String> {
        let cands: Vec<Sig> = self
            .callable
            .iter()
            .filter(|s| s.ret_bool == want_bool)
            .cloned()
            .collect();
        if cands.is_empty() {
            return None;
        }
        let s = self.r.pick(&cands).clone();
        let args: Vec<String> = s
            .params
            .iter()
            .map(|&b| if b { self.bool(d - 1) } else { self.int(d - 1) })
            .collect();
        Some(format!("{}({})", s.name, args.join(", ")))
    }
    fn block(&mut self, out: &mut String, n: usize, loops: u32, ind: usize) {
        let (ni, nb, nmi, nmb) = (
            self.ints.len(),
            self.bools.len(),
            self.mut_ints.len(),
            self.mut_bools.len(),
        );
        let pad = " ".repeat(ind);
        for _ in 0..n {
            match self.r.below(10) {
                0 | 1 if !self.mut_ints.is_empty() => {
                    let v = self.r.pick(&self.mut_ints).clone();
                    let e = self.int(3);
                    out.push_str(&format!("{pad}{v} = {e}\n"));
                }
                2 if !self.mut_bools.is_empty() => {
                    let v = self.r.pick(&self.mut_bools).clone();
                    let e = self.bool(2);
                    out.push_str(&format!("{pad}{v} = {e}\n"));
                }
                3 => {
                    let v = self.name("t");
                    let e = self.int(3);
                    out.push_str(&format!("{pad}let {v} = {e}\n"));
                    self.ints.push(v);
                }
                4 | 5 => {
                    let c = self.bool(2);
                    out.push_str(&format!("{pad}if {c} {{\n"));
                    let k = 1 + self.r.below(3);
                    self.block(out, k, loops, ind + 4);
                    if self.r.chance(1, 2) {
                        out.push_str(&format!("{pad}}} else {{\n"));
                        let k = 1 + self.r.below(3);
                        self.block(out, k, loops, ind + 4);
                    }
                    out.push_str(&format!("{pad}}}\n"));
                }
                6 | 7 if loops > 0 => {
                    let c = self.name("c");
                    let lim = 1 + self.r.below(6);
                    out.push_str(&format!(
                        "{pad}var {c} = 0\n{pad}while {c} < {lim} {{\n{pad}    {c} = {c} + 1\n"
                    ));
                    self.ints.push(c);
                    let k = 1 + self.r.below(3);
                    self.block(out, k, loops - 1, ind + 4);
                    out.push_str(&format!("{pad}}}\n"));
                }
                8 if self.r.chance(1, 3) => {
                    let c = self.bool(2);
                    out.push_str(&format!("{pad}assert {c}\n"));
                }
                _ => {
                    let v = self.name("m");
                    if self.r.chance(1, 3) {
                        let e = self.bool(2);
                        out.push_str(&format!("{pad}var {v} = {e}\n"));
                        self.bools.push(v.clone());
                        self.mut_bools.push(v);
                    } else {
                        let e = self.int(3);
                        out.push_str(&format!("{pad}var {v} = {e}\n"));
                        self.ints.push(v.clone());
                        self.mut_ints.push(v);
                    }
                }
            }
        }
        self.ints.truncate(ni);
        self.bools.truncate(nb);
        self.mut_ints.truncate(nmi);
        self.mut_bools.truncate(nmb);
    }
    fn func(&mut self, s: &Sig) -> String {
        let ps: Vec<String> = s
            .params
            .iter()
            .enumerate()
            .map(|(i, &b)| format!("p{i}: {}", if b { "bool" } else { "int" }))
            .collect();
        for (i, &b) in s.params.iter().enumerate() {
            if b { &mut self.bools } else { &mut self.ints }.push(format!("p{i}"));
        }
        let mut body = String::new();
        let n = 2 + self.r.below(5);
        // block() drops its locals at the end, so emit the tail expression from inside a wrapper
        self.block_keep(&mut body, n);
        let tail = if s.ret_bool {
            self.bool(3)
        } else {
            self.int(3)
        };
        self.ints.clear();
        self.bools.clear();
        self.mut_ints.clear();
        self.mut_bools.clear();
        let ret = if s.ret_bool { "bool" } else { "int" };
        format!(
            "fn {}({}) -> {ret} {{\n{body}    {tail}\n}}\n",
            s.name,
            ps.join(", ")
        )
    }
    /// Like block, but top-level locals stay in scope for the tail expression.
    fn block_keep(&mut self, out: &mut String, n: usize) {
        for _ in 0..n {
            let v = self.name("m");
            let e = self.int(3);
            out.push_str(&format!("    var {v} = {e}\n"));
            self.ints.push(v.clone());
            self.mut_ints.push(v);
            let k = 1 + self.r.below(2);
            self.block(out, k, 2, 4);
        }
    }
}

const PRELUDE: &str = "
fn rec(n: int, acc: int) -> int { if n <= 0 { acc } else { rec(n - 1, acc + n) } }
fn ev(n: int) -> bool { if n == 0 { true } else { od(n - 1) } }
fn od(n: int) -> bool { if n == 0 { false } else { ev(n - 1) } }
fn deep(n: int) -> int { if n == 0 { 0 } else { 1 + deep(n - 1) } }
";

fn program(r: &mut Rng) -> String {
    let mut g = G {
        r,
        ints: vec![],
        bools: vec![],
        mut_ints: vec![],
        mut_bools: vec![],
        callable: vec![],
        fresh: 0,
    };
    let mut src = String::from(PRELUDE);
    let nf = g.r.below(4);
    for i in 0..nf {
        let np = g.r.below(4);
        let params = (0..np).map(|_| g.r.chance(1, 3)).collect();
        let s = Sig {
            name: format!("f{i}"),
            params,
            ret_bool: g.r.chance(1, 4),
        };
        src.push_str(&g.func(&s));
        g.callable.push(s);
    }
    let main = Sig {
        name: "main".into(),
        params: vec![],
        ret_bool: false,
    };
    let mut m = g.func(&main);
    // recursion near the 10_000 frame limit, or mutual recursion through Bool results
    match g.r.below(8) {
        0 => {
            let n = 9_990 + g.r.below(20);
            m = m.replacen("{\n", &format!("{{\n    let dd = deep({n})\n"), 1);
        }
        1 => {
            let n = g.r.below(400);
            m = m.replacen(
                "{\n",
                &format!("{{\n    assert ev({n}) == ({n} % 2 == 0)\n"),
                1,
            );
        }
        2 | 3 => {
            let n = g.r.below(300);
            m = m.replacen(
                "{
",
                &format!(
                    "{{
    let rr = rec({n}, 1)
"
                ),
                1,
            );
        }
        _ => {}
    }
    src.push_str(&m);
    src
}

fn ir(src: &str) -> Program {
    let ast = logos::parser::parse(src).unwrap_or_else(|d| panic!("parse: {d:?}\n{src}"));
    logos::check::check(&ast).unwrap_or_else(|d| panic!("check: {d:?}\n{src}"))
}

#[test]
fn random_programs_match_geode() {
    let mut r = Rng::new(0x29A_0001);
    let (mut traps, mut vals, mut skipped) = ([0u32; 5], 0, 0);
    for case in 0..250 {
        let src = program(&mut r);
        let p = ir(&src);
        assert!(
            emit_asm(&p, "main").is_ok(),
            "case {case}: {:?}\n{src}",
            emit_asm(&p, "main")
        );
        let Some(want) = elysium(&p) else {
            skipped += 1;
            continue;
        };
        let (exit, out) = run(&p, "main", 200_000_000);
        match want {
            Ok(v) => {
                vals += 1;
                assert_eq!(
                    (exit, out),
                    (Exit::Pass, format!("{v}\n")),
                    "case {case}\n{src}"
                );
            }
            Err(t) => {
                traps[t as usize] += 1;
                assert_eq!(
                    (exit, out),
                    (Exit::Fail(t), format!("trap {t}\n")),
                    "case {case}\n{src}"
                );
            }
        }
    }
    // the generator must exercise values and every trap class
    assert!(
        vals >= 40 && traps[1..].iter().all(|&t| t >= 3),
        "values {vals} traps {traps:?} skipped {skipped}"
    );
    assert!(skipped <= 25, "skipped {skipped}");
}

#[test]
fn depth_limit_is_exact() {
    // the limit counts main as one frame, as Elysium does; scan both sides of the boundary
    for n in 9_995..10_002 {
        let src = format!("{PRELUDE}fn main() -> int {{ deep({n}) }}");
        let p = ir(&src);
        let want = elysium(&p).unwrap();
        let (exit, out) = run(&p, "main", 50_000_000);
        let got = match exit {
            Exit::Pass => Ok(out.trim_end().parse::<i64>().unwrap()),
            Exit::Fail(t) => Err(t),
            Exit::OutOfSteps => panic!("n {n}: out of steps"),
        };
        assert_eq!(got, want, "n {n}");
    }
}

fn rand_ty(r: &mut Rng) -> Ty {
    match r.below(8) {
        0..=3 => Ty::Int,
        4 | 5 => Ty::Bool,
        6 => Ty::Unit,
        _ => r
            .pick(&[Ty::Byte, Ty::Str, Ty::Array(Box::new(Ty::Int))])
            .clone(),
    }
}

#[test]
fn random_ir_never_panics_emit() {
    let mut r = Rng::new(0x29A_0002);
    let bins = [
        BinOp::Or,
        BinOp::And,
        BinOp::Eq,
        BinOp::Ne,
        BinOp::Lt,
        BinOp::Le,
        BinOp::Gt,
        BinOp::Ge,
        BinOp::BitOr,
        BinOp::Shl,
        BinOp::Add,
        BinOp::Sub,
        BinOp::AddWrap,
        BinOp::Mul,
        BinOp::Div,
        BinOp::Rem,
    ];
    let (mut ok, mut unsupported) = (0, 0);
    for _ in 0..3000 {
        let wild = r.chance(1, 4);
        let tame = !wild && r.chance(1, 2); // supported types and instructions only // allow out-of-range registers, labels and functions
        let nf = 1 + r.below(4);
        let mut funcs = Vec::new();
        for fi in 0..nf {
            let nregs = 1 + r.below(12);
            let regs: Vec<Ty> = (0..nregs)
                .map(|_| {
                    if tame {
                        if r.chance(1, 3) {
                            Ty::Bool
                        } else {
                            Ty::Int
                        }
                    } else {
                        rand_ty(&mut r)
                    }
                })
                .collect();
            let params = r.below(nregs.min(10) + 1) as u32;
            let reg = |r: &mut Rng| {
                if wild && r.chance(1, 10) {
                    r.below(40) as u32
                } else {
                    r.below(nregs) as u32
                }
            };
            let lab = |r: &mut Rng| {
                if wild {
                    r.below(8) as u32
                } else {
                    r.below(3) as u32
                }
            };
            let mut insts = Vec::new();
            for _ in 0..r.below(30) {
                insts.push(match r.below(if tame { 12 } else { 14 }) {
                    0 => {
                        let lits = if tame {
                            vec![Lit::Int(r.u64() as i64), Lit::Bool(true)]
                        } else {
                            vec![
                                Lit::Int(r.u64() as i64),
                                Lit::Bool(true),
                                Lit::Unit,
                                Lit::Byte(3),
                                Lit::Str("x".into()),
                            ]
                        };
                        Inst::Lit(reg(&mut r), r.pick(&lits).clone())
                    }
                    1 => Inst::Move(reg(&mut r), reg(&mut r)),
                    2 => Inst::Un(reg(&mut r), *r.pick(&[UnOp::Neg, UnOp::Not]), reg(&mut r)),
                    3 | 4 => Inst::Bin(
                        reg(&mut r),
                        *r.pick(if tame { &bins[..8] } else { &bins[..] }),
                        reg(&mut r),
                        reg(&mut r),
                    ),
                    5 => {
                        let f = if wild { r.below(nf + 2) } else { r.below(nf) } as u32;
                        let args = (0..if tame { 0 } else { r.below(4) })
                            .map(|_| reg(&mut r))
                            .collect();
                        Inst::Call(reg(&mut r), f, args)
                    }
                    6 if !tame => Inst::Label(lab(&mut r)),
                    6 => Inst::Move(reg(&mut r), reg(&mut r)),
                    7 => Inst::Jump(lab(&mut r)),
                    8 => Inst::Branch(reg(&mut r), lab(&mut r), lab(&mut r)),
                    9 | 10 => Inst::Ret(reg(&mut r)),
                    11 => Inst::Assert(reg(&mut r), "a".into()),
                    12 => Inst::NewArray(reg(&mut r), vec![reg(&mut r)]),
                    _ => Inst::GetIndex(reg(&mut r), reg(&mut r), reg(&mut r)),
                });
            }
            if tame {
                // well formed: labels 0..3 each defined exactly once
                for l in 0..3 {
                    let at = r.below(insts.len() + 1);
                    insts.insert(at, Inst::Label(l));
                }
            }
            let spans = vec![Span { start: 0, end: 0 }; insts.len()];
            let body = if !tame && r.chance(1, 20) {
                Body::Extern("x.y".into())
            } else {
                Body::Code { insts, spans }
            };
            let name = if fi == 0 && r.chance(3, 4) {
                "main".to_string()
            } else {
                format!("g{fi}")
            };
            let is_main = name == "main" && (tame || r.chance(4, 5));
            let (params, ret) = if is_main || tame {
                (0, Ty::Int)
            } else {
                (params, rand_ty(&mut r))
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
        let p = Program {
            types: vec![],
            layers: vec![],
            funcs,
            tests: vec![],
            props: vec![],
            main: None,
        };
        match emit_asm(&p, "main") {
            Ok(_) => ok += 1,
            Err(CodegenError::Unsupported(m)) => {
                assert!(!m.is_empty());
                unsupported += 1;
            }
            Err(CodegenError::NoEntry(e)) => assert_eq!(e, "main"),
        }
    }
    assert!(
        ok > 100 && unsupported > 300,
        "ok {ok} unsupported {unsupported}"
    );
}
