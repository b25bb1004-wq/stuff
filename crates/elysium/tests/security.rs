//! Security and trust-boundary attack tests for `elysium` bytecode loading, verification, GC, and VM execution.

#![allow(clippy::unwrap_used)]

use elysium::{
    decode, encode, verify, Const, DecodeError, Func, Handle, Limits, LoadError, Module, NoHost,
    Obj, Op, Trap, Value, VerifyError, Vm,
};

fn base_module(regs: u16, code: Vec<Op>) -> Module {
    Module {
        consts: vec![
            Const::Str("ok".into()),
            Const::Bytes(vec![0xff, 0x00, 0x41]),
        ],
        imports: vec!["env.log".into()],
        funcs: vec![
            Func {
                name: "main".into(),
                params: 0,
                regs,
                code,
            },
            Func {
                name: "takes_two".into(),
                params: 2,
                regs: 2,
                code: vec![Op::Add(0, 0, 1), Op::Ret(0)],
            },
        ],
        entry: 0,
    }
}

#[test]
fn decode_rejects_bad_magic_truncated_and_trailing_bytes() {
    assert_eq!(decode(b""), Err(DecodeError::Truncated));
    assert_eq!(decode(b"MBC"), Err(DecodeError::Truncated));
    assert_eq!(decode(b"MBC2\0\0\0\0"), Err(DecodeError::BadMagic));
    assert_eq!(decode(b"\0\0\0\0\0\0\0\0"), Err(DecodeError::BadMagic));

    let valid = encode(&base_module(2, vec![Op::Int(0, 1), Op::Ret(0)]));
    for prefix in 0..valid.len() {
        assert!(
            decode(&valid[..prefix]).is_err(),
            "truncated prefix {prefix} unexpectedly decoded"
        );
    }

    let mut with_trailing = valid;
    with_trailing.push(0x42);
    assert_eq!(decode(&with_trailing), Err(DecodeError::BadOpcode(0x42)));
}

#[test]
fn decode_rejects_hostile_counts_overlong_leb_bad_tags_and_bad_utf8() {
    // Huge constant count (u32::MAX) with no payload must fail before allocating.
    let mut huge_consts = b"MBC1".to_vec();
    huge_consts.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x0f]);
    assert_eq!(decode(&huge_consts), Err(DecodeError::TooLarge));

    // Huge code count inside a function must fail before allocating.
    let huge_code = [
        b'M', b'B', b'C', b'1', // magic
        0,    // 0 consts
        0,    // 0 imports
        1,    // 1 func
        1, b'f', // name "f"
        0, 1, // params=0, regs=1
        0xff, 0xff, 0xff, 0xff, 0x0f, // ncode = u32::MAX
    ];
    assert_eq!(decode(&huge_code), Err(DecodeError::TooLarge));

    // Overlong LEB128 (redundant zero group) in entry index.
    let overlong_entry = [b'M', b'B', b'C', b'1', 0, 0, 0, 0x80, 0x00];
    assert_eq!(decode(&overlong_entry), Err(DecodeError::Overlong));

    // >16-bit LEB128 value for regs.
    let overflow_regs = [
        b'M', b'B', b'C', b'1', 0, 0, 1, 1, b'f', 0, 0x80, 0x80, 0x04, 0,
    ];
    assert_eq!(decode(&overflow_regs), Err(DecodeError::Overlong));

    // Invalid constant tag (2).
    let bad_const_tag = [b'M', b'B', b'C', b'1', 1, 2, 0, 0, 0, 0];
    assert_eq!(decode(&bad_const_tag), Err(DecodeError::BadOpcode(2)));

    // Invalid boolean immediate (2) in Op::Bool.
    let bad_bool = [
        b'M', b'B', b'C', b'1', 0, 0, 1, 1, b'f', 0, 1, 1, 2, 0, 2, 0,
    ];
    assert_eq!(decode(&bad_bool), Err(DecodeError::BadOpcode(2)));

    // Invalid UTF-8 in function name.
    let bad_utf8 = [b'M', b'B', b'C', b'1', 0, 0, 1, 2, 0xf0, 0x28, 0, 1, 0, 0];
    assert_eq!(decode(&bad_utf8), Err(DecodeError::BadUtf8));

    // Unknown opcode (255).
    let bad_op = [b'M', b'B', b'C', b'1', 0, 0, 1, 1, b'f', 0, 1, 1, 255, 0];
    assert_eq!(decode(&bad_op), Err(DecodeError::BadOpcode(255)));
}

#[test]
fn verify_rejects_bad_jumps_and_unterminated_code() {
    assert_eq!(
        verify(&base_module(1, vec![])),
        Err(VerifyError::EmptyCode { func: 0 })
    );
    assert_eq!(
        verify(&base_module(1, vec![Op::Jump(1)])),
        Err(VerifyError::JumpOutOfRange {
            func: 0,
            at: 0,
            target: 1,
        })
    );
    assert_eq!(
        verify(&base_module(1, vec![Op::Jump(u32::MAX)])),
        Err(VerifyError::JumpOutOfRange {
            func: 0,
            at: 0,
            target: u32::MAX,
        })
    );
    assert_eq!(
        verify(&base_module(1, vec![Op::JumpIf(0, 2), Op::Ret(0)])),
        Err(VerifyError::JumpOutOfRange {
            func: 0,
            at: 0,
            target: 2,
        })
    );
    assert_eq!(
        verify(&base_module(1, vec![Op::JumpIfNot(0, 99), Op::Ret(0)])),
        Err(VerifyError::JumpOutOfRange {
            func: 0,
            at: 0,
            target: 99,
        })
    );
    assert_eq!(
        verify(&base_module(1, vec![Op::Bool(0, true), Op::JumpIf(0, 0)])),
        Err(VerifyError::BadLastInstruction { func: 0 })
    );
    assert_eq!(
        verify(&base_module(1, vec![Op::Int(0, 42)])),
        Err(VerifyError::BadLastInstruction { func: 0 })
    );
    assert_eq!(
        verify(&base_module(1, vec![Op::Trap(0); 1_048_577])),
        Err(VerifyError::TooManyInstructions { func: 0 })
    );
}

#[test]
fn verify_rejects_bad_registers_spans_and_frames() {
    assert_eq!(
        verify(&base_module(2, vec![Op::Move(0, 2), Op::Ret(0)])),
        Err(VerifyError::RegisterOutOfRange {
            func: 0,
            at: 0,
            reg: 2,
        })
    );
    assert_eq!(
        verify(&base_module(2, vec![Op::Int(u16::MAX, 1), Op::Ret(0)])),
        Err(VerifyError::RegisterOutOfRange {
            func: 0,
            at: 0,
            reg: u16::MAX,
        })
    );
    assert_eq!(
        verify(&base_module(4, vec![Op::Call(0, 1, 3, 2), Op::Ret(0)])),
        Err(VerifyError::RegisterRangeOutOfRange { func: 0, at: 0 })
    );
    assert_eq!(
        verify(&base_module(
            4,
            vec![Op::NewArray(0, 2, u16::MAX), Op::Ret(0)]
        )),
        Err(VerifyError::RegisterRangeOutOfRange { func: 0, at: 0 })
    );
    assert_eq!(
        verify(&base_module(4, vec![Op::CallHost(0, 0, 1, 4), Op::Ret(0)])),
        Err(VerifyError::RegisterRangeOutOfRange { func: 0, at: 0 })
    );

    let mut m_regs = base_module(4097, vec![Op::Trap(0)]);
    assert_eq!(verify(&m_regs), Err(VerifyError::FrameTooLarge { func: 0 }));

    m_regs.funcs[0].regs = 1;
    m_regs.funcs[0].params = 2;
    assert_eq!(
        verify(&m_regs),
        Err(VerifyError::ParamsExceedRegs { func: 0 })
    );
}

#[test]
fn verify_rejects_bad_indices_arities_and_table_limits() {
    assert_eq!(
        verify(&base_module(2, vec![Op::Const(0, 99), Op::Ret(0)])),
        Err(VerifyError::ConstOutOfRange {
            func: 0,
            at: 0,
            index: 99,
        })
    );
    assert_eq!(
        verify(&base_module(2, vec![Op::FnRef(0, 99), Op::Ret(0)])),
        Err(VerifyError::FuncOutOfRange {
            func: 0,
            at: 0,
            index: 99,
        })
    );
    assert_eq!(
        verify(&base_module(2, vec![Op::CallHost(0, 1, 0, 0), Op::Ret(0)])),
        Err(VerifyError::ImportOutOfRange {
            func: 0,
            at: 0,
            index: 1,
        })
    );
    assert_eq!(
        verify(&base_module(2, vec![Op::Call(0, 1, 0, 1), Op::Ret(0)])),
        Err(VerifyError::ArgCountMismatch {
            func: 0,
            at: 0,
            callee: 1,
            expected: 2,
            got: 1,
        })
    );
    assert_eq!(
        verify(&base_module(
            2,
            vec![Op::Bool(0, true), Op::Assert(0, 1), Op::Ret(0)]
        )),
        Err(VerifyError::AssertNotString {
            func: 0,
            at: 1,
            index: 1,
        })
    );

    let mut bad_entry = base_module(1, vec![Op::Trap(0)]);
    bad_entry.entry = 2;
    assert_eq!(
        verify(&bad_entry),
        Err(VerifyError::EntryOutOfRange { entry: 2 })
    );

    let mut bad_name = base_module(1, vec![Op::Trap(0)]);
    bad_name.funcs[0].name = "x".repeat(256);
    assert_eq!(
        verify(&bad_name),
        Err(VerifyError::FuncNameTooLong { func: 0 })
    );

    let mut too_many_consts = base_module(1, vec![Op::Trap(0)]);
    too_many_consts.consts = vec![Const::Str(String::new()); 65_537];
    assert_eq!(verify(&too_many_consts), Err(VerifyError::TooManyConsts));

    assert_eq!(
        Vm::new(base_module(1, vec![Op::Trap(0)]), &[]).unwrap_err(),
        LoadError::MissingImport("env.log".into())
    );
}

#[test]
fn vm_rejects_huge_allocations_via_new_fill_concat_and_push() {
    let huge_limits = Limits {
        fuel: u64::MAX,
        max_heap_bytes: 1024 * 1024,
        max_depth: 64,
    };

    // Huge NewFill of Int (i64::MAX) traps with OutOfMemory without allocating.
    let m_fill_arr = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 2,
            code: vec![
                Op::Int(0, 1),
                Op::Int(1, i64::MAX),
                Op::NewFill(0, 0, 1),
                Op::Ret(0),
            ],
        }],
        entry: 0,
    };
    let mut vm = Vm::new(m_fill_arr, &[]).unwrap();
    assert_eq!(vm.run(&mut NoHost, huge_limits), Err(Trap::OutOfMemory));

    // Huge NewFill of Byte traps with OutOfMemory without allocating.
    let m_fill_bytes = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 2,
            code: vec![
                Op::Byte(0, 7),
                Op::Int(1, 1 << 30),
                Op::NewFill(0, 0, 1),
                Op::Ret(0),
            ],
        }],
        entry: 0,
    };
    let mut vm_b = Vm::new(m_fill_bytes, &[]).unwrap();
    assert_eq!(vm_b.run(&mut NoHost, huge_limits), Err(Trap::OutOfMemory));

    // Negative NewFill count traps with IndexOutOfRange.
    let m_fill_neg = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 2,
            code: vec![
                Op::Byte(0, 7),
                Op::Int(1, i64::MIN),
                Op::NewFill(0, 0, 1),
                Op::Ret(0),
            ],
        }],
        entry: 0,
    };
    let mut vm_neg = Vm::new(m_fill_neg, &[]).unwrap();
    assert_eq!(
        vm_neg.run(&mut NoHost, huge_limits),
        Err(Trap::IndexOutOfRange)
    );

    // Exponential string doubling via Concat hits max_heap_bytes cleanly.
    let m_concat = Module {
        consts: vec![Const::Str("0123456789abcdef".into())],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 1,
            code: vec![Op::Const(0, 0), Op::Concat(0, 0, 0), Op::Jump(1)],
        }],
        entry: 0,
    };
    let mut vm_c = Vm::new(m_concat, &[]).unwrap();
    assert_eq!(vm_c.run(&mut NoHost, huge_limits), Err(Trap::OutOfMemory));
    assert!(vm_c.heap().live_bytes() <= huge_limits.max_heap_bytes);

    // Unbounded Push loop hits max_heap_bytes cleanly.
    let m_push = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 2,
            code: vec![
                Op::NewArray(0, 0, 0),
                Op::Int(1, 99),
                Op::Push(0, 1),
                Op::Jump(2),
            ],
        }],
        entry: 0,
    };
    let mut vm_p = Vm::new(m_push, &[]).unwrap();
    assert_eq!(vm_p.run(&mut NoHost, huge_limits), Err(Trap::OutOfMemory));
    assert!(vm_p.heap().live_bytes() <= huge_limits.max_heap_bytes);
}

#[test]
fn vm_stops_deep_recursion_and_zero_max_depth() {
    let m = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "self_rec".into(),
            params: 0,
            regs: 4096,
            code: vec![Op::Call(0, 0, 0, 0), Op::Ret(0)],
        }],
        entry: 0,
    };
    let mut vm = Vm::new(m, &[]).unwrap();
    assert_eq!(
        vm.run(
            &mut NoHost,
            Limits {
                fuel: 100_000,
                max_heap_bytes: 64 * 1024,
                max_depth: 0,
            }
        ),
        Err(Trap::StackOverflow)
    );
    assert_eq!(vm.trap_site(), Some((0, 0)));

    assert_eq!(
        vm.run(
            &mut NoHost,
            Limits {
                fuel: 100_000,
                max_heap_bytes: 64 * 1024,
                max_depth: 512,
            }
        ),
        Err(Trap::StackOverflow)
    );
    assert_eq!(vm.trap_site(), Some((0, 0)));
}

#[test]
fn vm_gc_collects_cyclic_objects_and_pop_rolls_back_on_oom() {
    // Build self-referential arrays in a loop and drop them: GC must collect cycles without hanging or OOM.
    let m_cycles = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 4,
            code: vec![
                Op::Int(0, 0),
                Op::Int(1, 20_000),
                Op::Int(2, 1),
                Op::Ge(3, 0, 1),
                Op::JumpIf(3, 9),
                Op::NewArray(3, 0, 0),
                Op::Push(3, 3), // self-cycle: arr.push(arr)
                Op::Add(0, 0, 2),
                Op::Jump(3),
                Op::Ret(0),
            ],
        }],
        entry: 0,
    };
    let mut vm = Vm::new(m_cycles, &[]).unwrap();
    let limits = Limits {
        fuel: 5_000_000,
        max_heap_bytes: 256 * 1024,
        max_depth: 16,
    };
    assert_eq!(vm.run(&mut NoHost, limits), Ok(Value::Int(20_000)));
    assert!(vm.heap().live_bytes() <= limits.max_heap_bytes);

    // Pop on a 1-element array when heap is exactly full (32 bytes = 16 header + 16 elem)
    // must fail with OutOfMemory and roll back the popped element.
    let m_pop_oom = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![
            Func {
                name: "main".into(),
                params: 0,
                regs: 2,
                code: vec![
                    Op::Int(0, 777),
                    Op::NewArray(0, 0, 1), // 32 bytes
                    Op::Pop(1, 0), // popping frees 16 -> 16, then Variant(1) needs 32 -> 48 > 32
                    Op::Ret(1),
                ],
            },
            Func {
                name: "len_of".into(),
                params: 1,
                regs: 1,
                code: vec![Op::Len(0, 0), Op::Ret(0)],
            },
        ],
        entry: 0,
    };
    let mut vm_pop = Vm::new(m_pop_oom, &[]).unwrap();
    assert_eq!(
        vm_pop.run(
            &mut NoHost,
            Limits {
                fuel: 1000,
                max_heap_bytes: 32,
                max_depth: 8,
            }
        ),
        Err(Trap::OutOfMemory)
    );
    // Slot 0 still holds the 1-element array after rollback.
    assert_eq!(
        vm_pop.heap().get(Handle(0)),
        Ok(&Obj::Array(vec![Value::Int(777)]))
    );
}

#[test]
fn vm_traps_on_hostile_arithmetic_slices_and_handles() {
    let m = Module {
        consts: vec![Const::Str("é".into())],
        imports: vec![],
        funcs: vec![
            Func {
                name: "main".into(),
                params: 0,
                regs: 1,
                code: vec![Op::Int(0, 0), Op::Ret(0)],
            },
            Func {
                name: "div_min".into(),
                params: 0,
                regs: 2,
                code: vec![
                    Op::Int(0, i64::MIN),
                    Op::Int(1, -1),
                    Op::Div(0, 0, 1),
                    Op::Ret(0),
                ],
            },
            Func {
                name: "rem_min".into(),
                params: 0,
                regs: 2,
                code: vec![
                    Op::Int(0, i64::MIN),
                    Op::Int(1, -1),
                    Op::Rem(0, 0, 1),
                    Op::Ret(0),
                ],
            },
            Func {
                name: "bad_slice".into(),
                params: 0,
                regs: 3,
                code: vec![
                    Op::Const(0, 0),
                    Op::Int(1, 0),
                    Op::Int(2, 1),
                    Op::Slice(0, 0, 1, 2),
                    Op::Ret(0),
                ],
            },
            Func {
                name: "use_ref".into(),
                params: 1,
                regs: 1,
                code: vec![Op::Len(0, 0), Op::Ret(0)],
            },
        ],
        entry: 0,
    };
    let mut vm = Vm::new(m, &[]).unwrap();
    let limits = Limits::default();
    assert_eq!(
        vm.call("div_min", &[], &mut NoHost, limits),
        Err(Trap::Overflow)
    );
    assert_eq!(
        vm.call("rem_min", &[], &mut NoHost, limits),
        Err(Trap::Overflow)
    );
    assert_eq!(
        vm.call("bad_slice", &[], &mut NoHost, limits),
        Err(Trap::IndexOutOfRange)
    );
    assert_eq!(
        vm.call(
            "use_ref",
            &[Value::Ref(Handle(u32::MAX))],
            &mut NoHost,
            limits
        ),
        Err(Trap::TypeError)
    );
    assert_eq!(
        vm.call("no_such_fn", &[], &mut NoHost, limits),
        Err(Trap::TypeError)
    );
}

#[test]
fn vm_fuel_stops_infinite_loop_and_zero_fuel_traps_immediately() {
    let m = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 1,
            code: vec![Op::Jump(0)],
        }],
        entry: 0,
    };
    let mut vm = Vm::new(m, &[]).unwrap();
    assert_eq!(
        vm.run(
            &mut NoHost,
            Limits {
                fuel: 0,
                max_heap_bytes: 1024,
                max_depth: 8,
            }
        ),
        Err(Trap::OutOfFuel)
    );
    assert_eq!(vm.fuel_used(), 0);
    assert_eq!(vm.trap_site(), Some((0, 0)));

    assert_eq!(
        vm.run(
            &mut NoHost,
            Limits {
                fuel: 10_000,
                max_heap_bytes: 1024,
                max_depth: 8,
            }
        ),
        Err(Trap::OutOfFuel)
    );
    assert_eq!(vm.fuel_used(), 10_000);
}
