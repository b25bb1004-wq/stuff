//! Leader check for T30 (cobalt::opt): 5000 rv64-generator programs, optimised vs unoptimised under the evaluator, Elysium and (every 10th) RV64; idempotence, span parity, regs and params kept.
//! Bool parameters and results, calls, recursion near the depth limit, overflow and division
//! traps) run on Silicon through `cobalt::rv64::run` and on Elysium; result and trap class must
//! match. Plus random IR (in and out of range) that must never panic `emit_asm`.
#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
use assay::Rng;
use cobalt::rv64::run;
use elysium::{Heap, Host, Limits, Trap as GTrap, Value, Vm};
use logos::ir::{Body, Program, Ty};

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

fn eval(p: &Program) -> Option<(String, Result<(), String>)> {
    let o = logos::eval::run_main(p, "", 20_000_000);
    match o.result {
        Err((logos::eval::Trap::OutOfFuel, _)) => None,
        r => Some((o.output, r.map_err(|(t, _)| format!("{t:?}")))),
    }
}

fn shape(p: &Program) -> Vec<(String, u32, Vec<Ty>, bool)> {
    p.funcs
        .iter()
        .map(|f| {
            (
                f.name.clone(),
                f.params,
                f.regs.clone(),
                matches!(f.body, Body::Code { .. }),
            )
        })
        .collect()
}

#[test]
fn optimised_programs_agree_on_every_back_end() {
    let mut r = Rng::new(0x0307_1EAD ^ 0x77_7777);
    let (mut changed, mut checked, mut rv) = (0u32, 0u32, 0u32);
    for case in 0..5000 {
        let src = program(&mut r);
        let p = ir(&src);
        let mut o = p.clone();
        let st = cobalt::opt::optimize(&mut o);
        if st != cobalt::opt::Stats::default() {
            changed += 1;
        }
        assert_eq!(
            shape(&p),
            shape(&o),
            "case {case}: regs/params/order changed\n{src}"
        );
        for f in &o.funcs {
            if let Body::Code { insts, spans } = &f.body {
                assert_eq!(
                    insts.len(),
                    spans.len(),
                    "case {case}: spans not parallel in {}",
                    f.name
                );
            }
        }
        let mut again = o.clone();
        assert_eq!(
            cobalt::opt::optimize(&mut again),
            cobalt::opt::Stats::default(),
            "case {case}: not idempotent\n{src}"
        );
        if let (Some(a), Some(b)) = (eval(&p), eval(&o)) {
            assert_eq!(a, b, "case {case}: evaluator differs after optimize\n{src}");
            checked += 1;
        }
        if let (Some(a), Some(b)) = (elysium(&p), elysium(&o)) {
            assert_eq!(a, b, "case {case}: elysium differs after optimize\n{src}");
        }
        if case % 10 == 0 {
            assert_eq!(
                run(&p, "main", 200_000_000),
                run(&o, "main", 200_000_000),
                "case {case}: rv64 differs\n{src}"
            );
            rv += 1;
        }
    }
    assert!(
        changed >= 2500,
        "optimizer changed only {changed} of 5000 programs"
    );
    assert!(checked >= 4000, "evaluator compared only {checked}");
    assert!(rv == 500);
}
