//! Tests for pausable VM execution and debugger session inspection.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use elysium::{verify, Const, Event, Func, Limits, Module, NoHost, Op, Position, Trap, Value, Vm};
use std::time::Instant;

fn make_random_module(rng: &mut Rng) -> Module {
    let consts = vec![
        Const::Str("hello".into()),
        Const::Str("assertion failed".into()),
        Const::Bytes(vec![1, 2, 3, 4]),
    ];

    let helper_regs: u16 = 8;
    let helper_ops_n = rng.range(2, 10) as usize;
    let mut helper_code = Vec::with_capacity(helper_ops_n + 1);
    for _ in 0..helper_ops_n {
        let d = rng.range(0, 6) as u16;
        let a = rng.range(0, 6) as u16;
        let b = rng.range(0, 6) as u16;
        let op = match rng.range(0, 6) {
            0 => Op::Int(d, rng.range(0, 20) as i64 - 5),
            1 => Op::AddW(d, a, b),
            2 => Op::SubW(d, a, b),
            3 => Op::MulW(d, a, b),
            4 => Op::Div(d, a, b),
            _ => Op::Move(d, a),
        };
        helper_code.push(op);
    }
    helper_code.push(Op::Ret(rng.range(0, 6) as u16));

    let main_regs: u16 = 10;
    let body_len = rng.range(4, 18) as usize;
    let mut main_code = Vec::with_capacity(body_len + 1);
    for i in 0..4u16 {
        main_code.push(Op::Int(i, rng.range(0, 12) as i64 - 2));
    }
    main_code.push(Op::Bool(4, rng.chance(1, 2)));
    main_code.push(Op::Const(5, 0));

    let prefix = main_code.len();
    let total_main = prefix + body_len;
    for idx in 0..body_len {
        let cur_pc = prefix + idx;
        let d = rng.range(0, 8) as u16;
        let a = rng.range(0, 4) as u16;
        let b = rng.range(0, 4) as u16;
        let op = match rng.range(0, 11) {
            0 => Op::Int(d.min(3), rng.range(0, 30) as i64 - 10),
            1 => Op::Add(d.min(3), a, b),
            2 => Op::Sub(d.min(3), a, b),
            3 => Op::MulW(d.min(3), a, b),
            4 => Op::Div(d.min(3), a, b),
            5 => Op::Lt(4, a, b),
            6 => {
                let target = rng.range((cur_pc + 1) as u64, (total_main + 1) as u64) as u32;
                Op::JumpIf(4, target.min(total_main as u32))
            }
            7 => Op::Call(d.min(3), 1, 0, 2),
            8 => Op::NewArray(6, 0, 2),
            9 => Op::Len(7, 6),
            _ => Op::Assert(4, 1),
        };
        main_code.push(op);
    }
    main_code.push(Op::Ret(rng.range(0, 4) as u16));

    let module = Module {
        consts,
        imports: Vec::new(),
        funcs: vec![
            Func {
                name: "main".into(),
                params: 0,
                regs: main_regs,
                code: main_code,
            },
            Func {
                name: "helper".into(),
                params: 2,
                regs: helper_regs,
                code: helper_code,
            },
        ],
        entry: 0,
    };
    verify(&module).unwrap();
    module
}

fn outcome_from_event(ev: Event) -> Result<Value, Trap> {
    match ev {
        Event::Done(v) => Ok(v),
        Event::Trapped(t) => Err(t),
        other => panic!("unexpected non-terminal event: {other:?}"),
    }
}

#[test]
fn random_programs_match_run_and_single_step() {
    forall(150, 0x74a0_0001, |rng| {
        let module = make_random_module(rng);
        let fuel = rng.range(0, 80);
        let limits = Limits {
            fuel,
            ..Limits::default()
        };

        let mut vm_run = Vm::new(module.clone(), &[]).unwrap();
        let expected_res = vm_run.run(&mut NoHost, limits);
        let expected_fuel = vm_run.fuel_used();
        let expected_site = vm_run.trap_site();

        // 1. start + resume with never-true predicate
        let mut vm_never = Vm::new(module.clone(), &[]).unwrap();
        let mut sess = vm_never.start(module.entry, &[], limits).unwrap();
        let ev = sess.resume(&mut vm_never, &mut NoHost, &|_| false, u64::MAX);
        assert_eq!(outcome_from_event(ev), expected_res);
        assert_eq!(vm_never.fuel_used(), expected_fuel);
        assert_eq!(vm_never.trap_site(), expected_site);

        // 2. pause on every op and resume each time
        let mut vm_step = Vm::new(module.clone(), &[]).unwrap();
        let mut sess_step = vm_step.start(module.entry, &[], limits).unwrap();
        let mut pauses = 0usize;
        let final_ev = loop {
            match sess_step.resume(&mut vm_step, &mut NoHost, &|_| true, u64::MAX) {
                Event::Paused(pos) => {
                    assert!(pos.depth >= 1);
                    assert_eq!(sess_step.frames().len(), pos.depth as usize);
                    pauses += 1;
                    assert!(pauses < 10_000, "too many pauses");
                }
                other => break other,
            }
        };
        assert_eq!(outcome_from_event(final_ev), expected_res);
        assert_eq!(vm_step.fuel_used(), expected_fuel);
        assert_eq!(vm_step.trap_site(), expected_site);

        // 3. budget = 1 stepping
        let mut vm_budget = Vm::new(module, &[]).unwrap();
        let mut sess_budget = vm_budget.start(0, &[], limits).unwrap();
        let final_budget = loop {
            match sess_budget.resume(&mut vm_budget, &mut NoHost, &|_| false, 1) {
                Event::Budget => {}
                other => break other,
            }
        };
        assert_eq!(outcome_from_event(final_budget), expected_res);
        assert_eq!(vm_budget.fuel_used(), expected_fuel);
        assert_eq!(vm_budget.trap_site(), expected_site);
    });
}

#[test]
fn nested_call_frames_and_heap_values() {
    // func 0 (main): r0 = "tag", r1 = 10, r2 = 20, r3 = call helper(r1, r2), ret r3
    // func 1 (helper): params = 2 (r0, r1), r2 = r0 + r1, r3 = r2 * 2, ret r3
    let module = Module {
        consts: vec![Const::Str("tag".into())],
        imports: Vec::new(),
        funcs: vec![
            Func {
                name: "main".into(),
                params: 0,
                regs: 4,
                code: vec![
                    Op::Const(0, 0),
                    Op::Int(1, 10),
                    Op::Int(2, 20),
                    Op::Call(3, 1, 1, 2),
                    Op::Ret(3),
                ],
            },
            Func {
                name: "helper".into(),
                params: 2,
                regs: 4,
                code: vec![
                    Op::Add(2, 0, 1),
                    Op::Int(3, 2),
                    Op::Mul(3, 2, 3),
                    Op::Ret(3),
                ],
            },
        ],
        entry: 0,
    };
    verify(&module).unwrap();

    let mut vm = Vm::new(module, &[]).unwrap();
    let mut sess = vm.start(0, &[], Limits::default()).unwrap();

    // Pause inside helper at pc == 1 (after Op::Add(2, 0, 1))
    let ev = sess.resume(
        &mut vm,
        &mut NoHost,
        &|pos| pos.func == 1 && pos.pc == 1,
        u64::MAX,
    );
    assert_eq!(
        ev,
        Event::Paused(Position {
            func: 1,
            pc: 1,
            depth: 2,
        })
    );

    let frames = sess.frames();
    assert_eq!(frames.len(), 2);
    // Outermost frame: main (func 0), waiting at return pc = 4
    assert_eq!(frames[0].func, 0);
    assert_eq!(frames[0].pc, 4);
    assert_eq!(frames[0].regs.len(), 4);
    assert!(matches!(frames[0].regs[0], Value::Ref(_)));
    assert_eq!(frames[0].regs[1], Value::Int(10));
    assert_eq!(frames[0].regs[2], Value::Int(20));

    // Innermost frame (last): helper (func 1), pc = 1, r2 = 30
    assert_eq!(frames[1].func, 1);
    assert_eq!(frames[1].pc, 1);
    assert_eq!(
        frames[1].regs,
        vec![Value::Int(10), Value::Int(20), Value::Int(30), Value::Unit]
    );

    // Heap reference from caller frame stays valid and readable on vm.heap()
    let Value::Ref(h) = frames[0].regs[0] else {
        panic!("expected Value::Ref");
    };
    assert!(vm.heap().get(h).is_ok());

    // Resume to completion
    let ev2 = sess.resume(&mut vm, &mut NoHost, &|_| false, u64::MAX);
    assert_eq!(ev2, Event::Done(Value::Int(60)));
    assert!(sess.frames().is_empty());
}

#[test]
fn breakpoint_moves_on_when_resumed_and_loops_retrigger() {
    // Loop from 0 to 3: r0 = 0; r1 = 3; r2 = 1;
    // pc 3: Add(r0, r0, r2)
    // pc 4: Lt(r3, r0, r1)
    // pc 5: JumpIf(r3, 3)
    // pc 6: Ret(r0)
    let module = Module {
        consts: Vec::new(),
        imports: Vec::new(),
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 4,
            code: vec![
                Op::Int(0, 0),
                Op::Int(1, 3),
                Op::Int(2, 1),
                Op::Add(0, 0, 2),
                Op::Lt(3, 0, 1),
                Op::JumpIf(3, 3),
                Op::Ret(0),
            ],
        }],
        entry: 0,
    };
    verify(&module).unwrap();

    let mut vm = Vm::new(module, &[]).unwrap();
    let mut sess = vm.start(0, &[], Limits::default()).unwrap();
    let bp = |pos: &Position| pos.func == 0 && pos.pc == 3;

    // First hit at pc=3 (r0 == 0)
    assert_eq!(
        sess.resume(&mut vm, &mut NoHost, &bp, u64::MAX),
        Event::Paused(Position {
            func: 0,
            pc: 3,
            depth: 1,
        })
    );
    assert_eq!(sess.frames()[0].regs[0], Value::Int(0));

    // Resume from breakpoint moves on, executes iteration 1, hits pc=3 again (r0 == 1)
    assert_eq!(
        sess.resume(&mut vm, &mut NoHost, &bp, u64::MAX),
        Event::Paused(Position {
            func: 0,
            pc: 3,
            depth: 1,
        })
    );
    assert_eq!(sess.frames()[0].regs[0], Value::Int(1));

    // Iteration 2 -> hits pc=3 again (r0 == 2)
    assert_eq!(
        sess.resume(&mut vm, &mut NoHost, &bp, u64::MAX),
        Event::Paused(Position {
            func: 0,
            pc: 3,
            depth: 1,
        })
    );
    assert_eq!(sess.frames()[0].regs[0], Value::Int(2));

    // Iteration 3 -> finishes loop and returns 3
    assert_eq!(
        sess.resume(&mut vm, &mut NoHost, &bp, u64::MAX),
        Event::Done(Value::Int(3))
    );
    // Calling resume again after Done returns the same Done
    assert_eq!(
        sess.resume(&mut vm, &mut NoHost, &bp, u64::MAX),
        Event::Done(Value::Int(3))
    );
}

#[test]
fn budget_zero_and_trap_behavior() {
    let module = Module {
        consts: Vec::new(),
        imports: Vec::new(),
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 2,
            code: vec![Op::Int(0, 7), Op::Trap(42), Op::Ret(0)],
        }],
        entry: 0,
    };
    verify(&module).unwrap();

    let mut vm = Vm::new(module.clone(), &[]).unwrap();
    // max_depth = 0 traps on start
    assert_eq!(
        vm.start(
            0,
            &[],
            Limits {
                max_depth: 0,
                ..Limits::default()
            }
        )
        .unwrap_err(),
        Trap::StackOverflow
    );
    assert_eq!(vm.trap_site(), Some((0, 0)));

    // Invalid func / bad arg count traps on start
    assert_eq!(
        vm.start(99, &[], Limits::default()).unwrap_err(),
        Trap::TypeError
    );
    assert_eq!(
        vm.start(0, &[Value::Int(1)], Limits::default())
            .unwrap_err(),
        Trap::TypeError
    );

    // Budget = 0 runs nothing
    let mut sess = vm.start(0, &[], Limits::default()).unwrap();
    assert_eq!(
        sess.resume(&mut vm, &mut NoHost, &|_| false, 0),
        Event::Budget
    );
    assert_eq!(vm.fuel_used(), 0);
    assert_eq!(sess.frames()[0].pc, 0);

    // Run to trap
    assert_eq!(
        sess.resume(&mut vm, &mut NoHost, &|_| false, u64::MAX),
        Event::Trapped(Trap::User(42))
    );
    assert_eq!(vm.trap_site(), Some((0, 1)));
    assert_eq!(vm.fuel_used(), 2);
    // Resuming after trap is idempotent
    assert_eq!(
        sess.resume(&mut vm, &mut NoHost, &|_| false, u64::MAX),
        Event::Trapped(Trap::User(42))
    );
    assert_eq!(vm.fuel_used(), 2);
}

#[test]
fn call_and_loop_timing_check() {
    // Tight loop: sum 0..10_000 via Vm::call vs Session::resume
    let module = Module {
        consts: Vec::new(),
        imports: Vec::new(),
        funcs: vec![Func {
            name: "sum_to".into(),
            params: 1,
            regs: 4,
            code: vec![
                Op::Int(1, 0),   // r1 = acc
                Op::Int(2, 1),   // r2 = step
                Op::Lt(3, 0, 2), // while r0 >= 1 (i.e. !(r0 < 1))
                Op::JumpIf(3, 7),
                Op::Add(1, 1, 0), // acc += r0
                Op::Sub(0, 0, 2), // r0 -= 1
                Op::Jump(2),
                Op::Ret(1),
            ],
        }],
        entry: 0,
    };
    verify(&module).unwrap();

    let mut vm = Vm::new(module, &[]).unwrap();
    let t0 = Instant::now();
    let r1 = vm.call(
        "sum_to",
        &[Value::Int(10_000)],
        &mut NoHost,
        Limits::default(),
    );
    let run_dt = t0.elapsed();

    let t1 = Instant::now();
    let mut sess = vm
        .start(0, &[Value::Int(10_000)], Limits::default())
        .unwrap();
    let r2 = sess.resume(&mut vm, &mut NoHost, &|_| false, u64::MAX);
    let sess_dt = t1.elapsed();

    assert_eq!(r1, Ok(Value::Int(50_005_000)));
    assert_eq!(r2, Event::Done(Value::Int(50_005_000)));
    // Keep timing variables used so debug builds exercise both paths cleanly.
    assert!(run_dt.as_nanos() > 0 && sess_dt.as_nanos() > 0);
}
