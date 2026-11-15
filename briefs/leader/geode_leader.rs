//! Leader-run check for the Elysium VM: hand-built modules, exact fuel, GC churn, deep data,
//! limits, and a fuzz run over the public API only. `STRATA_SEED`/`STRATA_CASES` change the run.
#![allow(clippy::unwrap_used)]
use assay::{forall, Rng};
use elysium::{
    verify, Const, Func, Heap, Host, Limits, LoadError, Module, NoHost, Op, Trap, Value, Vm,
};
use Op::*;

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn func(name: &str, params: u16, regs: u16, code: Vec<Op>) -> Func {
    Func {
        name: name.into(),
        params,
        regs,
        code,
    }
}

fn module(consts: Vec<Const>, imports: &[&str], funcs: Vec<Func>) -> Module {
    Module {
        consts,
        imports: imports.iter().map(|s| s.to_string()).collect(),
        funcs,
        entry: 0,
    }
}

fn limits(fuel: u64, heap: usize, depth: u32) -> Limits {
    Limits {
        fuel,
        max_heap_bytes: heap,
        max_depth: depth,
    }
}

fn run(m: Module, l: Limits) -> (Result<Value, Trap>, u64) {
    let mut vm = Vm::new(m, &[]).unwrap();
    let r = vm.run(&mut NoHost, l);
    (r, vm.fuel_used())
}

/// Count r0 down from n: 3 setup + 4 per turn + 3 to leave = 4n + 6 instructions.
fn countdown(n: i64) -> Module {
    let code = vec![
        Int(0, n),
        Int(1, 0),
        Int(2, 1),
        Gt(3, 0, 1),
        JumpIfNot(3, 7),
        Sub(0, 0, 2),
        Jump(3),
        Ret(0),
    ];
    module(vec![], &[], vec![func("main", 0, 4, code)])
}

#[test]
fn leader_fuel_is_exact() {
    for n in [0i64, 1, 7, 100] {
        let full = 4 * n as u64 + 6;
        let (r, used) = run(countdown(n), limits(full, 1 << 20, 64));
        assert_eq!(r, Ok(Value::Int(0)));
        assert_eq!(used, full, "n {n}");
        for k in 0..full {
            let (r, used) = run(countdown(n), limits(k, 1 << 20, 64));
            assert_eq!(r, Err(Trap::OutOfFuel), "n {n} fuel {k}");
            assert!(used <= k, "used {used} > fuel {k}");
        }
    }
}

#[test]
fn leader_recursive_fib() {
    let fib = vec![
        Int(1, 2),
        Lt(2, 0, 1),
        JumpIfNot(2, 4),
        Ret(0),
        Int(1, 1),
        Sub(3, 0, 1),
        Call(3, 1, 3, 1),
        Int(1, 2),
        Sub(4, 0, 1),
        Call(4, 1, 4, 1),
        Add(3, 3, 4),
        Ret(3),
    ];
    let main = vec![Int(0, 20), Call(0, 1, 0, 1), Ret(0)];
    let m = module(
        vec![],
        &[],
        vec![func("main", 0, 1, main), func("fib", 1, 5, fib)],
    );
    let (r, used) = run(m.clone(), Limits::default());
    assert_eq!(r, Ok(Value::Int(6765)));
    let (r2, used2) = run(m.clone(), Limits::default());
    assert_eq!(
        (r2, used2),
        (Ok(Value::Int(6765)), used),
        "not deterministic"
    );
    let mut vm = Vm::new(m, &[]).unwrap();
    assert_eq!(
        vm.call("fib", &[Value::Int(10)], &mut NoHost, Limits::default()),
        Ok(Value::Int(55))
    );
    assert!(vm.call("fib", &[], &mut NoHost, Limits::default()).is_err());
    assert!(vm
        .call("nope", &[], &mut NoHost, Limits::default())
        .is_err());
}

fn churn(keep: bool) -> Module {
    let mut code = vec![
        Int(0, 200_000),
        Int(1, 0),
        Int(2, 1),
        NewArray(5, 0, 0),
        Gt(3, 0, 1),
        JumpIfNot(3, 10),
        Const(4, 0),
        if keep { Push(5, 4) } else { Move(6, 4) },
        Sub(0, 0, 2),
        Jump(4),
    ];
    code.push(Ret(0));
    module(
        vec![Const::Str("some garbage text, 32 bytes long".into())],
        &[],
        vec![func("main", 0, 7, code)],
    )
}

#[test]
fn leader_gc_churn_and_out_of_memory() {
    let mut vm = Vm::new(churn(false), &[]).unwrap();
    assert_eq!(
        vm.run(&mut NoHost, limits(u64::MAX, 1 << 20, 64)),
        Ok(Value::Int(0))
    );
    assert!(vm.heap().live_bytes() <= 1 << 20);
    let (r, _) = run(churn(true), limits(u64::MAX, 1 << 20, 64));
    assert_eq!(r, Err(Trap::OutOfMemory));
}

#[test]
fn leader_deep_linked_list_survives_collections() {
    // 300 000 records chained through field 1, built under a heap that forces many collections.
    let code = vec![
        Int(0, 300_000),
        Int(1, 0),
        Int(2, 1),
        Unit(5),
        Gt(3, 0, 1),
        JumpIfNot(3, 11),
        Move(6, 0),
        Move(7, 5),
        NewRecord(5, 6, 2),
        Sub(0, 0, 2),
        Jump(4),
        GetField(8, 5, 0),
        Ret(8),
    ];
    let m = module(vec![], &[], vec![func("main", 0, 9, code)]);
    let (r, _) = run(m, limits(u64::MAX, 64 << 20, 64));
    assert_eq!(r, Ok(Value::Int(1)));
}

#[test]
fn leader_depth_limits() {
    let f = vec![Call(0, 1, 0, 0), Ret(0)];
    let main = vec![Call(0, 1, 0, 0), Ret(0)];
    let m = module(
        vec![],
        &[],
        vec![func("main", 0, 1, main), func("f", 0, 1, f)],
    );
    let (r, _) = run(m.clone(), limits(u64::MAX, 64 << 20, 1000));
    assert_eq!(r, Err(Trap::StackOverflow));
    let (r, _) = run(m, limits(3_000_000, 1 << 30, 1_000_000));
    assert!(
        matches!(r, Err(Trap::StackOverflow) | Err(Trap::OutOfFuel)),
        "{r:?}"
    );
}

fn trap_of(code: Vec<Op>, consts: Vec<Const>) -> Result<Value, Trap> {
    run(
        module(consts, &[], vec![func("main", 0, 8, code)]),
        limits(10_000_000, 1 << 24, 64),
    )
    .0
}

#[test]
fn leader_traps() {
    assert_eq!(
        trap_of(vec![Int(0, 1), Int(1, 0), Div(2, 0, 1), Ret(2)], vec![]),
        Err(Trap::DivideByZero)
    );
    assert_eq!(
        trap_of(
            vec![Int(0, i64::MAX), Int(1, 1), Add(2, 0, 1), Ret(2)],
            vec![]
        ),
        Err(Trap::Overflow)
    );
    assert_eq!(
        trap_of(
            vec![Int(0, i64::MIN), Int(1, -1), Div(2, 0, 1), Ret(2)],
            vec![]
        ),
        Err(Trap::Overflow)
    );
    assert_eq!(
        trap_of(vec![Int(0, 1), Int(1, 64), Shl(2, 0, 1), Ret(2)], vec![]),
        Err(Trap::Overflow)
    );
    assert_eq!(
        trap_of(
            vec![Byte(0, 200), Byte(1, 100), Add(2, 0, 1), Ret(2)],
            vec![]
        ),
        Err(Trap::Overflow)
    );
    assert_eq!(
        trap_of(vec![Int(0, 1), Bool(1, true), Add(2, 0, 1), Ret(2)], vec![]),
        Err(Trap::TypeError)
    );
    assert_eq!(
        trap_of(
            vec![NewArray(0, 0, 0), Int(1, 0), GetIndex(2, 0, 1), Ret(2)],
            vec![]
        ),
        Err(Trap::IndexOutOfRange)
    );
    assert_eq!(
        trap_of(
            vec![Bool(0, false), Assert(0, 0), Ret(0)],
            vec![Const::Str("boom".into())]
        ),
        Err(Trap::AssertFailed("boom".into()))
    );
    assert_eq!(trap_of(vec![Op::Trap(7)], vec![]), Err(Trap::User(7)));
    let huge = trap_of(
        vec![Int(0, 0), Int(1, 1 << 40), NewFill(2, 0, 1), Ret(2)],
        vec![],
    );
    assert!(
        matches!(huge, Err(Trap::OutOfMemory) | Err(Trap::OutOfFuel)),
        "{huge:?}"
    );
    let neg = trap_of(
        vec![Int(0, 0), Int(1, -1), NewFill(2, 0, 1), Ret(2)],
        vec![],
    );
    assert_eq!(neg, Err(Trap::IndexOutOfRange));
    // "héllo"[0..2] cuts inside 'é'.
    let cut = trap_of(
        vec![Const(0, 0), Int(1, 0), Int(2, 2), Slice(3, 0, 1, 2), Ret(3)],
        vec![Const::Str("héllo".into())],
    );
    assert_eq!(cut, Err(Trap::IndexOutOfRange));
}

#[test]
fn leader_strings_and_options() {
    // ParseInt("-42") then Tag and field: some(-42) -> tag 1, field 0 = -42.
    let code = vec![
        Const(0, 0),
        ParseInt(1, 0),
        GetField(2, 1, 0),
        Tag(3, 1),
        Add(4, 2, 3),
        Ret(4),
    ];
    assert_eq!(
        trap_of(code, vec![Const::Str("-42".into())]),
        Ok(Value::Int(-41))
    );
    for bad in ["", "-", "+1", "1_0", "9223372036854775808", " 1"] {
        let code = vec![Const(0, 0), ParseInt(1, 0), Tag(2, 1), Ret(2)];
        assert_eq!(
            trap_of(code, vec![Const::Str(bad.into())]),
            Ok(Value::Int(0)),
            "{bad:?}"
        );
    }
    let code = vec![
        Int(0, -1234),
        IntToStr(1, 0),
        Const(2, 0),
        Eq(3, 1, 2),
        Ret(3),
    ];
    assert_eq!(
        trap_of(code, vec![Const::Str("-1234".into())]),
        Ok(Value::Bool(true))
    );
}

struct Counter(u32);
impl Host for Counter {
    fn call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Result<Value, Trap> {
        self.0 += 1;
        if name == "io.len" {
            return Ok(Value::Int(args.len() as i64));
        }
        let h = heap.alloc(elysium::Obj::Str(name.into()))?;
        Ok(Value::Ref(h))
    }
}

#[test]
fn leader_host_and_imports() {
    let code = vec![Int(0, 5), Int(1, 6), CallHost(2, 0, 0, 2), Ret(2)];
    let m = module(vec![], &["io.len"], vec![func("main", 0, 3, code)]);
    assert!(matches!(
        Vm::new(m.clone(), &[]),
        Err(LoadError::MissingImport(_))
    ));
    let mut vm = Vm::new(m, &["io.len", "io.print"]).unwrap();
    let mut host = Counter(0);
    assert_eq!(vm.run(&mut host, Limits::default()), Ok(Value::Int(2)));
    assert_eq!(host.0, 1);
}

fn random_op(r: &mut Rng, regs: u16, len: u32, funcs: &[(u16, u16)], nconst: u32) -> Op {
    let g = |r: &mut Rng| r.below(regs as usize) as u16;
    let t = |r: &mut Rng| r.below(len as usize) as u32;
    let span = |r: &mut Rng, n: u16| -> u16 { r.below(regs.saturating_sub(n) as usize + 1) as u16 };
    match r.below(30) {
        0 => Int(g(r), (r.u64() as i64) >> r.below(64)),
        1 => Byte(g(r), r.u64() as u8),
        2 => Bool(g(r), r.chance(1, 2)),
        3 if nconst > 0 => Const(g(r), r.below(nconst as usize) as u32),
        4 => Add(g(r), g(r), g(r)),
        5 => Sub(g(r), g(r), g(r)),
        6 => Mul(g(r), g(r), g(r)),
        7 => Div(g(r), g(r), g(r)),
        8 => Lt(g(r), g(r), g(r)),
        9 => Eq(g(r), g(r), g(r)),
        10 => Jump(t(r)),
        11 => JumpIf(g(r), t(r)),
        12 => {
            let f = r.below(funcs.len());
            let n = funcs[f].0;
            Call(g(r), f as u32, span(r, n), n)
        }
        13 => {
            let n = r.below(regs as usize + 1) as u16;
            NewArray(g(r), span(r, n), n)
        }
        14 => NewFill(g(r), g(r), g(r)),
        15 => GetIndex(g(r), g(r), g(r)),
        16 => SetIndex(g(r), g(r), g(r)),
        17 => Push(g(r), g(r)),
        18 => Pop(g(r), g(r)),
        19 => Concat(g(r), g(r), g(r)),
        20 => Slice(g(r), g(r), g(r), g(r)),
        21 => Len(g(r), g(r)),
        22 => IntToStr(g(r), g(r)),
        23 => {
            let n = r.below(regs as usize + 1) as u16;
            NewRecord(g(r), span(r, n), n)
        }
        24 => GetField(g(r), g(r), r.below(3) as u16),
        25 => FnRef(g(r), r.below(funcs.len()) as u32),
        26 => CallValue(g(r), g(r), 0, 0),
        27 => Shl(g(r), g(r), g(r)),
        28 => Ret(g(r)),
        _ => Move(g(r), g(r)),
    }
}

fn random_module(r: &mut Rng) -> Module {
    let nf = 1 + r.below(3);
    let shapes: Vec<(u16, u16)> = (0..nf)
        .map(|i| {
            let regs = 1 + r.below(8) as u16;
            let params = if i == 0 {
                0
            } else {
                r.below(regs as usize + 1) as u16
            };
            (params, regs)
        })
        .collect();
    let consts = vec![Const::Str("ab€".into()), Const::Bytes(vec![1, 2, 3])];
    let funcs = shapes
        .iter()
        .enumerate()
        .map(|(i, &(params, regs))| {
            let len = 1 + r.below(30) as u32;
            let mut code: Vec<Op> = (0..len - 1)
                .map(|_| random_op(r, regs, len, &shapes, 2))
                .collect();
            code.push(Ret(r.below(regs as usize) as u16));
            func(&format!("f{i}"), params, regs, code)
        })
        .collect();
    module(consts, &[], funcs)
}

#[test]
fn leader_fuzz_valid_modules_never_panic_and_are_deterministic() {
    let seed = env("STRATA_SEED", 0x0009_e0de_2026_1114);
    let cases = env("STRATA_CASES", 3000) as u32;
    let mut verified = 0u32;
    forall(cases, seed, |r| {
        let m = random_module(r);
        if verify(&m).is_err() {
            return;
        }
        verified += 1;
        let l = limits(5_000, 64 << 10, 50);
        let mut a = Vm::new(m.clone(), &[]).unwrap();
        let mut b = Vm::new(m, &[]).unwrap();
        let ra = a.run(&mut NoHost, l);
        let rb = b.run(&mut NoHost, l);
        assert_eq!(format!("{ra:?}"), format!("{rb:?}"));
        assert_eq!(a.fuel_used(), b.fuel_used());
        assert!(a.fuel_used() <= 5_000);
    });
    assert!(
        verified > cases / 4,
        "generator too weak: {verified} verified"
    );
}
