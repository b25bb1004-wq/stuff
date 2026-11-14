//! Program tests, fuel exactness, GC churn, deep recursion and random module fuzzing.

#![allow(clippy::unwrap_used)]

use crate::testgen::random_module;
use crate::{Const, Func, Heap, Host, Limits, Module, NoHost, Obj, Op, Trap, Value, Vm};
use assay::forall;

#[test]
fn iterative_and_recursive_factorial_and_fibonacci() {
    let m = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![
            // 0: iter_fact(n: r0) -> r1
            Func {
                name: "iter_fact".into(),
                params: 1,
                regs: 4,
                code: vec![
                    Op::Int(1, 1),    // 0: acc = 1
                    Op::Int(2, 1),    // 1: one = 1
                    Op::Le(3, 0, 2),  // 2: n <= 1
                    Op::JumpIf(3, 7), // 3
                    Op::Mul(1, 1, 0), // 4: acc *= n
                    Op::Sub(0, 0, 2), // 5: n -= 1
                    Op::Jump(2),      // 6
                    Op::Ret(1),       // 7
                ],
            },
            // 1: rec_fact(n: r0) -> r0
            Func {
                name: "rec_fact".into(),
                params: 1,
                regs: 4,
                code: vec![
                    Op::Int(1, 1),        // 0
                    Op::Le(2, 0, 1),      // 1: n <= 1
                    Op::JumpIf(2, 7),     // 2
                    Op::Sub(2, 0, 1),     // 3: n - 1
                    Op::Call(3, 1, 2, 1), // 4: rec_fact(n - 1)
                    Op::Mul(0, 0, 3),     // 5: n * res
                    Op::Ret(0),           // 6
                    Op::Ret(1),           // 7: return 1
                ],
            },
            // 2: fib(n: r0) -> r1
            Func {
                name: "fib".into(),
                params: 1,
                regs: 5,
                code: vec![
                    Op::Int(1, 0),     // 0: a = 0
                    Op::Int(2, 1),     // 1: b = 1
                    Op::Int(3, 0),     // 2: zero = 0
                    Op::Le(4, 0, 3),   // 3: n <= 0
                    Op::JumpIf(4, 10), // 4
                    Op::Add(4, 1, 2),  // 5: next = a + b
                    Op::Move(1, 2),    // 6: a = b
                    Op::Move(2, 4),    // 7: b = next
                    Op::Int(3, 1),     // 8
                    Op::Sub(0, 0, 3),  // 9: n -= 1; then jump back
                    Op::Int(3, 0),     // 10 (wait: fix jump target below)
                    Op::Ret(1),
                ],
            },
        ],
        entry: 0,
    };
    let mut m = m;
    m.funcs[2].code = vec![
        Op::Int(1, 0),     // 0: a = 0
        Op::Int(2, 1),     // 1: b = 1
        Op::Int(3, 0),     // 2: zero = 0
        Op::Le(4, 0, 3),   // 3: cond = (n <= 0)
        Op::JumpIf(4, 11), // 4: if cond goto 11
        Op::Add(4, 1, 2),  // 5: next = a + b
        Op::Move(1, 2),    // 6: a = b
        Op::Move(2, 4),    // 7: b = next
        Op::Int(4, 1),     // 8: one = 1
        Op::Sub(0, 0, 4),  // 9: n -= 1
        Op::Jump(3),       // 10: goto 3
        Op::Ret(1),        // 11: return a
    ];
    let mut vm = Vm::new(m, &[]).unwrap();
    assert_eq!(
        vm.call(
            "iter_fact",
            &[Value::Int(10)],
            &mut NoHost,
            Limits::default()
        ),
        Ok(Value::Int(3_628_800))
    );
    assert_eq!(
        vm.call(
            "rec_fact",
            &[Value::Int(10)],
            &mut NoHost,
            Limits::default()
        ),
        Ok(Value::Int(3_628_800))
    );
    assert_eq!(
        vm.call("fib", &[Value::Int(20)], &mut NoHost, Limits::default()),
        Ok(Value::Int(6765))
    );
}

#[test]
fn string_loop_linked_list_of_records_and_array_of_variants() {
    // 1. Building a string in a loop
    let m_str = Module {
        consts: vec![Const::Str(String::new()), Const::Str("ab".into())],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 5,
            code: vec![
                Op::Const(0, 0),     // 0: acc = ""
                Op::Const(1, 1),     // 1: chunk = "ab"
                Op::Int(2, 5),       // 2: n = 5
                Op::Int(3, 0),       // 3: zero = 0
                Op::Le(4, 2, 3),     // 4: n <= 0
                Op::JumpIf(4, 10),   // 5
                Op::Concat(0, 0, 1), // 6: acc = acc + chunk
                Op::Int(4, 1),       // 7
                Op::Sub(2, 2, 4),    // 8: n -= 1
                Op::Jump(4),         // 9
                Op::Ret(0),          // 10
            ],
        }],
        entry: 0,
    };
    let mut vm = Vm::new(m_str, &[]).unwrap();
    let Value::Ref(h) = vm.run(&mut NoHost, Limits::default()).unwrap() else {
        panic!("expected Ref");
    };
    assert_eq!(vm.heap().get(h), Ok(&Obj::Str("ababababab".into())));

    // 2. Linked list of records: node = Record([val, next_variant]) walked to sum values
    let m_list = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 6,
            code: vec![
                Op::NewVariant(0, 0, 0, 0), // 0: list = None (tag 0)
                Op::Int(1, 1),              // 1: i = 1
                Op::Int(2, 5),              // 2: max = 5
                Op::Gt(3, 1, 2),            // 3: i > 5
                Op::JumpIf(3, 10),          // 4
                Op::Move(3, 1),             // 5: r3 = i
                Op::Move(4, 0),             // 6: r4 = list
                Op::NewRecord(5, 3, 2),     // 7: node = Record([i, list])
                Op::NewVariant(0, 1, 5, 1), // 8: list = Some(node)
                Op::Int(3, 1),              // 9
                Op::Add(1, 1, 3),           // 10: i += 1
                Op::Jump(3),                // 11
                // Walk list and sum node.0
                Op::Int(1, 0),         // 12: sum = 0
                Op::Tag(2, 0),         // 13: tag = Tag(list)
                Op::Int(3, 0),         // 14
                Op::Eq(4, 2, 3),       // 15: tag == 0
                Op::JumpIf(4, 21),     // 16
                Op::GetField(5, 0, 0), // 17: node = list.0
                Op::GetField(3, 5, 0), // 18: val = node.0
                Op::Add(1, 1, 3),      // 19: sum += val
                Op::GetField(0, 5, 1), // 20: list = node.1; jump 13
                Op::Jump(13),          // 21
                Op::Ret(1),            // 22
            ],
        }],
        entry: 0,
    };
    let mut m_list = m_list;
    m_list.funcs[0].code[4] = Op::JumpIf(3, 12);
    m_list.funcs[0].code[16] = Op::JumpIf(4, 22);
    let mut vm_list = Vm::new(m_list, &[]).unwrap();
    assert_eq!(
        vm_list.run(&mut NoHost, Limits::default()),
        Ok(Value::Int(15))
    );

    // 3. Array of variants walked with Tag and GetField
    let m_var = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 7,
            code: vec![
                Op::Int(0, 10),
                Op::NewVariant(1, 1, 0, 1), // Some(10)
                Op::NewVariant(2, 0, 0, 0), // None
                Op::Int(0, 32),
                Op::NewVariant(3, 1, 0, 1), // Some(32)
                Op::NewArray(0, 1, 3),      // arr = [Some(10), None, Some(32)]
                Op::Int(1, 0),              // sum = 0
                Op::Int(2, 0),              // idx = 0
                Op::Len(3, 0),              // len = 3
                Op::Ge(4, 2, 3),            // 9: idx >= len
                Op::JumpIf(4, 20),          // 10
                Op::GetIndex(4, 0, 2),      // 11: elem = arr[idx]
                Op::Tag(5, 4),              // 12: t = Tag(elem)
                Op::Int(6, 1),              // 13
                Op::Eq(5, 5, 6),            // 14: t == 1
                Op::JumpIfNot(5, 18),       // 15
                Op::GetField(5, 4, 0),      // 16: v = elem.0
                Op::Add(1, 1, 5),           // 17: sum += v
                Op::Add(2, 2, 6),           // 18: idx += 1
                Op::Jump(9),                // 19
                Op::Ret(1),                 // 20
            ],
        }],
        entry: 0,
    };
    let mut vm_var = Vm::new(m_var, &[]).unwrap();
    assert_eq!(
        vm_var.run(&mut NoHost, Limits::default()),
        Ok(Value::Int(42))
    );
}

#[test]
fn fuel_exact_loop_and_every_smaller_limit() {
    // Setup: 3 instructions (Int, Int, Int) = 3 fuel.
    // Loop of N=15 iterations: each iteration executes Ge(1), JumpIf(1), Add(1), Jump(1) = 4 fuel.
    // Final iteration executes Ge(1), JumpIf(1), Ret(1) = 3 fuel.
    // Total exact fuel = 3 + 15 * 4 + 3 = 66.
    let n: i64 = 15;
    let m = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 4,
            code: vec![
                Op::Int(0, 0),    // 0: i = 0
                Op::Int(1, n),    // 1: limit = 15
                Op::Int(2, 1),    // 2: step = 1
                Op::Ge(3, 0, 1),  // 3: i >= limit
                Op::JumpIf(3, 7), // 4
                Op::Add(0, 0, 2), // 5: i += 1
                Op::Jump(3),      // 6
                Op::Ret(0),       // 7
            ],
        }],
        entry: 0,
    };
    let exact_cost = 3 + (n as u64) * 4 + 3;
    let mut vm = Vm::new(m, &[]).unwrap();
    let res = vm.run(
        &mut NoHost,
        Limits {
            fuel: exact_cost,
            ..Limits::default()
        },
    );
    assert_eq!(res, Ok(Value::Int(n)));
    assert_eq!(vm.fuel_used(), exact_cost);

    for k in 0..exact_cost {
        let r = vm.run(
            &mut NoHost,
            Limits {
                fuel: k,
                ..Limits::default()
            },
        );
        assert_eq!(r, Err(Trap::OutOfFuel));
        assert_eq!(vm.fuel_used(), k);
    }
}

#[test]
fn gc_100k_garbage_objects_in_1mib_and_oom_when_kept() {
    // Allocate 100,000 garbage 2-element records (48 bytes each = 4.8 MB total) under a 1 MiB heap.
    let m_garbage = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 5,
            code: vec![
                Op::Int(0, 0),          // 0: i = 0
                Op::Int(1, 100_000),    // 1: n = 100_000
                Op::Int(2, 1),          // 2: step = 1
                Op::Ge(3, 0, 1),        // 3: i >= n
                Op::JumpIf(3, 8),       // 4
                Op::NewRecord(4, 0, 2), // 5: r4 = Record([i, n]) (overwrites previous r4)
                Op::Add(0, 0, 2),       // 6: i += 1
                Op::Jump(3),            // 7
                Op::Ret(0),             // 8
            ],
        }],
        entry: 0,
    };
    let mut vm = Vm::new(m_garbage, &[]).unwrap();
    let limits = Limits {
        fuel: 50_000_000,
        max_heap_bytes: 1024 * 1024,
        max_depth: 64,
    };
    assert_eq!(vm.run(&mut NoHost, limits), Ok(Value::Int(100_000)));
    assert!(vm.heap().live_objects() < 25_000);
    assert!(vm.heap().live_bytes() <= 1024 * 1024);

    // Loop that keeps every object (linked list) under 1 MiB gives OutOfMemory.
    let m_keep = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 2,
            code: vec![
                Op::Unit(0),            // 0
                Op::Int(1, 1),          // 1
                Op::NewRecord(0, 0, 2), // 2: r0 = Record([r0, 1]) (chains forever)
                Op::Jump(2),            // 3
            ],
        }],
        entry: 0,
    };
    let mut vm_keep = Vm::new(m_keep, &[]).unwrap();
    assert_eq!(vm_keep.run(&mut NoHost, limits), Err(Trap::OutOfMemory));
}

#[test]
fn recursion_depth_one_million_and_stack_overflow() {
    // Recurse to depth 1_000_000 (initial frame + 999_999 recursive calls = 1_000_000 frames).
    let m = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "dive".into(),
            params: 1,
            regs: 3,
            code: vec![
                Op::Int(1, 1),        // 0
                Op::Le(2, 0, 1),      // 1: n <= 1
                Op::JumpIf(2, 6),     // 2
                Op::Sub(2, 0, 1),     // 3: n - 1
                Op::Call(0, 0, 2, 1), // 4: dive(n - 1)
                Op::Ret(0),           // 5
                Op::Ret(0),           // 6
            ],
        }],
        entry: 0,
    };
    let mut vm = Vm::new(m, &[]).unwrap();
    let limits = Limits {
        fuel: 50_000_000,
        max_heap_bytes: 64 * 1024 * 1024,
        max_depth: 1_000_000,
    };
    assert_eq!(
        vm.call("dive", &[Value::Int(1_000_000)], &mut NoHost, limits),
        Ok(Value::Int(1))
    );
    assert_eq!(
        vm.call("dive", &[Value::Int(1_000_001)], &mut NoHost, limits),
        Err(Trap::StackOverflow)
    );
}

struct CountingHost {
    calls: i64,
}

impl Host for CountingHost {
    fn call(&mut self, _name: &str, args: &[Value], _heap: &mut Heap) -> Result<Value, Trap> {
        self.calls += 1;
        Ok(Value::Int(self.calls + args.len() as i64))
    }
}

#[test]
fn fuzz_random_modules_are_deterministic_and_never_panic() {
    let limits = Limits {
        fuel: 500,
        max_heap_bytes: 16 * 1024,
        max_depth: 32,
    };
    let host_names = ["io.f0", "io.f1", "io.f2", "io.f3"];

    forall(1200, 9009, |r| {
        let m = random_module(r);
        let mut vm1 = Vm::new(m.clone(), &host_names).unwrap();
        let mut vm2 = Vm::new(m.clone(), &host_names).unwrap();
        let res1 = vm1.run(&mut NoHost, limits);
        let res2 = vm2.run(&mut NoHost, limits);
        assert_eq!(res1, res2);
        assert_eq!(vm1.fuel_used(), vm2.fuel_used());

        let mut vm3 = Vm::new(m.clone(), &host_names).unwrap();
        let mut vm4 = Vm::new(m, &host_names).unwrap();
        let mut h3 = CountingHost { calls: 0 };
        let mut h4 = CountingHost { calls: 0 };
        let res3 = vm3.run(&mut h3, limits);
        let res4 = vm4.run(&mut h4, limits);
        assert_eq!(res3, res4);
        assert_eq!(vm3.fuel_used(), vm4.fuel_used());
        assert_eq!(h3.calls, h4.calls);
    });
}
