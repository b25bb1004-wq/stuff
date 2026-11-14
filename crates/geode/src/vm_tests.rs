//! Directed unit tests for every opcode, every trap it can raise, and the Heap / Vm API.

#![allow(clippy::unwrap_used)]

use crate::Op::*;
use crate::{
    Const, Func, Handle, Heap, Host, Limits, LoadError, Module, NoHost, Obj, Op, Trap, Value,
    VerifyError, Vm,
};

fn mod1(regs: u16, code: Vec<Op>) -> Module {
    Module {
        consts: vec![
            Const::Str("hello world".into()),
            Const::Bytes(vec![0x61, 0x62, 0xff]),
        ],
        imports: vec![],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs,
            code,
        }],
        entry: 0,
    }
}

fn run_ops(regs: u16, code: Vec<Op>) -> (Result<Value, Trap>, Vm) {
    let mut vm = Vm::new(mod1(regs, code), &[]).unwrap();
    let res = vm.run(&mut NoHost, Limits::default());
    (res, vm)
}

fn check_all(cases: &[(u16, &[Op], Result<Value, Trap>)]) {
    for (regs, code, expected) in cases {
        assert_eq!(
            &run_ops(*regs, code.to_vec()).0,
            expected,
            "failed: {code:?}"
        );
    }
}

#[test]
fn immediates_move_const_fnref_and_call_value() {
    let (res, vm) = run_ops(
        6,
        vec![
            Int(0, -42),
            Byte(1, 250),
            Bool(2, true),
            Unit(3),
            FnRef(4, 0),
            Move(5, 0),
            Ret(5),
        ],
    );
    assert_eq!(res, Ok(Value::Int(-42)));
    assert_eq!(vm.fuel_used(), 7);

    let (res, vm) = run_ops(2, vec![Const(0, 0), Const(1, 1), Ret(0)]);
    let Value::Ref(h0) = res.unwrap() else {
        panic!("expected Ref");
    };
    assert_eq!(vm.heap().get(h0), Ok(&Obj::Str("hello world".into())));
    assert_eq!(
        vm.heap().get(Handle(1)),
        Ok(&Obj::Bytes(vec![0x61, 0x62, 0xff]))
    );
    assert_eq!(vm.fuel_used(), 6);

    let m_cv = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![
            Func {
                name: "main".into(),
                params: 0,
                regs: 3,
                code: vec![FnRef(0, 1), Int(1, 41), CallValue(2, 0, 1, 1), Ret(2)],
            },
            Func {
                name: "inc".into(),
                params: 1,
                regs: 2,
                code: vec![Int(1, 1), Add(0, 0, 1), Ret(0)],
            },
        ],
        entry: 0,
    };
    let mut vm_cv = Vm::new(m_cv, &[]).unwrap();
    assert_eq!(
        vm_cv.run(&mut NoHost, Limits::default()),
        Ok(Value::Int(42))
    );
    check_all(&[
        (
            2,
            &[FnRef(0, 0), CallValue(1, 0, 0, 1), Ret(1)],
            Err(Trap::TypeError),
        ),
        (
            2,
            &[Int(0, 5), CallValue(1, 0, 0, 0), Ret(1)],
            Err(Trap::TypeError),
        ),
    ]);
}

#[test]
fn arithmetic_bitwise_shifts_comparisons_and_jumps() {
    use Trap::*;
    use Value as V;
    check_all(&[
        (
            3,
            &[Int(0, 20), Int(1, 22), Add(2, 0, 1), Ret(2)],
            Ok(V::Int(42)),
        ),
        (
            3,
            &[Int(0, 50), Int(1, 8), Sub(2, 0, 1), Ret(2)],
            Ok(V::Int(42)),
        ),
        (
            3,
            &[Int(0, 6), Int(1, 7), Mul(2, 0, 1), Ret(2)],
            Ok(V::Int(42)),
        ),
        (
            3,
            &[Byte(0, 200), Byte(1, 55), Add(2, 0, 1), Ret(2)],
            Ok(V::Byte(255)),
        ),
        (
            3,
            &[Byte(0, 10), Byte(1, 3), Sub(2, 0, 1), Ret(2)],
            Ok(V::Byte(7)),
        ),
        (
            3,
            &[Byte(0, 15), Byte(1, 17), Mul(2, 0, 1), Ret(2)],
            Ok(V::Byte(255)),
        ),
        (
            2,
            &[Int(0, i64::MAX), Int(1, 1), Add(0, 0, 1), Ret(0)],
            Err(Overflow),
        ),
        (
            2,
            &[Int(0, i64::MIN), Int(1, 1), Sub(0, 0, 1), Ret(0)],
            Err(Overflow),
        ),
        (
            2,
            &[Int(0, i64::MAX), Int(1, 2), Mul(0, 0, 1), Ret(0)],
            Err(Overflow),
        ),
        (
            2,
            &[Byte(0, 200), Byte(1, 100), Add(0, 0, 1), Ret(0)],
            Err(Overflow),
        ),
        (
            2,
            &[Byte(0, 5), Byte(1, 6), Sub(0, 0, 1), Ret(0)],
            Err(Overflow),
        ),
        (
            2,
            &[Byte(0, 20), Byte(1, 20), Mul(0, 0, 1), Ret(0)],
            Err(Overflow),
        ),
        (
            2,
            &[Int(0, 1), Byte(1, 1), Add(0, 0, 1), Ret(0)],
            Err(TypeError),
        ),
        (
            2,
            &[Int(0, i64::MAX), Int(1, 1), AddW(0, 0, 1), Ret(0)],
            Ok(V::Int(i64::MIN)),
        ),
        (
            2,
            &[Int(0, i64::MIN), Int(1, 1), SubW(0, 0, 1), Ret(0)],
            Ok(V::Int(i64::MAX)),
        ),
        (
            2,
            &[Int(0, i64::MAX), Int(1, 2), MulW(0, 0, 1), Ret(0)],
            Ok(V::Int(-2)),
        ),
        (
            2,
            &[Byte(0, 1), Byte(1, 1), AddW(0, 0, 1), Ret(0)],
            Err(TypeError),
        ),
        (
            2,
            &[Int(0, -17), Int(1, 5), Div(0, 0, 1), Ret(0)],
            Ok(V::Int(-3)),
        ),
        (
            2,
            &[Int(0, -17), Int(1, 5), Rem(0, 0, 1), Ret(0)],
            Ok(V::Int(-2)),
        ),
        (
            2,
            &[Int(0, 10), Int(1, 0), Div(0, 0, 1), Ret(0)],
            Err(DivideByZero),
        ),
        (
            2,
            &[Int(0, 10), Int(1, 0), Rem(0, 0, 1), Ret(0)],
            Err(DivideByZero),
        ),
        (
            2,
            &[Int(0, i64::MIN), Int(1, -1), Div(0, 0, 1), Ret(0)],
            Err(Overflow),
        ),
        (1, &[Int(0, 42), Neg(0, 0), Ret(0)], Ok(V::Int(-42))),
        (1, &[Int(0, i64::MIN), Neg(0, 0), Ret(0)], Err(Overflow)),
        (1, &[Byte(0, 1), Neg(0, 0), Ret(0)], Err(TypeError)),
        (
            2,
            &[Int(0, 12), Int(1, 10), And(0, 0, 1), Ret(0)],
            Ok(V::Int(8)),
        ),
        (
            2,
            &[Bool(0, true), Bool(1, false), Or(0, 0, 1), Ret(0)],
            Ok(V::Bool(true)),
        ),
        (
            2,
            &[Bool(0, true), Bool(1, true), Xor(0, 0, 1), Ret(0)],
            Ok(V::Bool(false)),
        ),
        (
            2,
            &[Byte(0, 1), Byte(1, 1), And(0, 0, 1), Ret(0)],
            Err(TypeError),
        ),
        (
            2,
            &[Int(0, 1), Int(1, 63), Shl(0, 0, 1), Ret(0)],
            Ok(V::Int(i64::MIN)),
        ),
        (
            2,
            &[Int(0, -16), Int(1, 2), Shr(0, 0, 1), Ret(0)],
            Ok(V::Int(-4)),
        ),
        (
            2,
            &[Int(0, 1), Int(1, 64), Shl(0, 0, 1), Ret(0)],
            Err(Overflow),
        ),
        (
            2,
            &[Int(0, 1), Int(1, -1), Shr(0, 0, 1), Ret(0)],
            Err(Overflow),
        ),
        (1, &[Bool(0, false), Not(0, 0), Ret(0)], Ok(V::Bool(true))),
        (1, &[Int(0, 0), Not(0, 0), Ret(0)], Err(TypeError)),
        (
            3,
            &[Const(0, 0), Const(1, 0), Eq(2, 0, 1), Ret(2)],
            Ok(V::Bool(true)),
        ),
        (
            3,
            &[Const(0, 1), Const(1, 1), Ne(2, 0, 1), Ret(2)],
            Ok(V::Bool(false)),
        ),
        (
            3,
            &[NewArray(0, 0, 1), NewArray(1, 0, 1), Eq(2, 0, 1), Ret(2)],
            Err(TypeError),
        ),
        (
            2,
            &[Int(0, 3), Int(1, 5), Lt(0, 0, 1), Ret(0)],
            Ok(V::Bool(true)),
        ),
        (
            2,
            &[Byte(0, 5), Byte(1, 5), Le(0, 0, 1), Ret(0)],
            Ok(V::Bool(true)),
        ),
        (
            2,
            &[Int(0, 9), Int(1, 5), Gt(0, 0, 1), Ret(0)],
            Ok(V::Bool(true)),
        ),
        (
            2,
            &[Const(0, 0), Const(1, 0), Ge(0, 0, 1), Ret(0)],
            Ok(V::Bool(true)),
        ),
        (
            2,
            &[Const(0, 1), Const(1, 1), Lt(0, 0, 1), Ret(0)],
            Err(TypeError),
        ),
        (1, &[Int(0, 1), JumpIf(0, 0), Ret(0)], Err(TypeError)),
    ]);
    let (res, _) = run_ops(
        2,
        vec![
            Bool(0, true),
            JumpIf(0, 3),
            Int(1, 99),
            JumpIfNot(0, 5),
            Int(1, 42),
            Jump(6),
            Ret(1),
        ],
    );
    assert_eq!(res, Ok(V::Int(42)));
}

#[test]
fn objects_arrays_bytes_records_variants_and_slices() {
    let (res, mut vm) = run_ops(
        5,
        vec![
            Byte(0, 10),
            Byte(1, 20),
            NewBytes(2, 0, 2),
            Byte(3, 30),
            Push(2, 3),
            Int(4, 1),
            SetIndex(2, 4, 3),
            Pop(4, 2),
            Len(0, 2),
            Ret(0),
        ],
    );
    assert_eq!(res, Ok(Value::Int(2)));
    assert!(vm.heap().live_objects() >= 2);
    let h_custom = vm
        .heap_mut()
        .alloc(Obj::Array(vec![Value::Int(1)]))
        .unwrap();
    if let Obj::Array(v) = vm.heap_mut().get_mut(h_custom).unwrap() {
        v.push(Value::Int(2));
    }
    assert_eq!(vm.heap().get(Handle(999)), Err(Trap::TypeError));

    check_all(&[
        (
            3,
            &[Byte(0, 1), NewBytes(1, 0, 1), Int(2, 5), Push(1, 2), Ret(0)],
            Err(Trap::TypeError),
        ),
        (
            2,
            &[NewArray(0, 0, 0), Int(1, 0), GetIndex(0, 0, 1), Ret(0)],
            Err(Trap::IndexOutOfRange),
        ),
        (
            2,
            &[NewArray(0, 0, 0), Int(1, 0), SetIndex(0, 1, 1), Ret(0)],
            Err(Trap::IndexOutOfRange),
        ),
        (
            2,
            &[Int(0, 1), Int(1, -1), NewFill(0, 0, 1), Ret(0)],
            Err(Trap::IndexOutOfRange),
        ),
        (
            4,
            &[
                Int(0, 10),
                Int(1, 20),
                NewRecord(2, 0, 2),
                SetField(2, 1, 0),
                GetField(3, 2, 1),
                Ret(3),
            ],
            Ok(Value::Int(10)),
        ),
        (
            2,
            &[NewArray(0, 0, 1), GetField(1, 0, 0), Ret(1)],
            Err(Trap::TypeError),
        ),
        (
            2,
            &[NewRecord(0, 0, 1), GetField(1, 0, 5), Ret(1)],
            Err(Trap::IndexOutOfRange),
        ),
    ]);

    let mut vm_oom = Vm::new(
        mod1(
            2,
            vec![Int(0, 1), Int(1, 1 << 40), NewFill(0, 0, 1), Ret(0)],
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        vm_oom.run(&mut NoHost, Limits::default()),
        Err(Trap::OutOfFuel)
    );
    assert_eq!(
        vm_oom.run(
            &mut NoHost,
            Limits {
                fuel: u64::MAX,
                max_heap_bytes: 1024 * 1024,
                max_depth: 64
            }
        ),
        Err(Trap::OutOfMemory)
    );

    let (res, vm) = run_ops(
        4,
        vec![
            Const(0, 0),
            Int(1, 0),
            Int(2, 5),
            Slice(3, 0, 1, 2),
            Concat(3, 3, 3),
            Ret(3),
        ],
    );
    let Value::Ref(h) = res.unwrap() else {
        panic!("expected Ref");
    };
    assert_eq!(vm.heap().get(h), Ok(&Obj::Str("hellohello".into())));

    let mut m_utf = mod1(
        4,
        vec![Const(0, 0), Int(1, 0), Int(2, 1), Slice(3, 0, 1, 2), Ret(3)],
    );
    m_utf.consts[0] = Const::Str("é".into());
    let mut vm_utf = Vm::new(m_utf, &[]).unwrap();
    assert_eq!(
        vm_utf.run(&mut NoHost, Limits::default()),
        Err(Trap::IndexOutOfRange)
    );
}

#[test]
fn conversions_assert_trap_calls_and_host() {
    let (res, vm) = run_ops(
        4,
        vec![
            Int(0, -12345),
            IntToStr(1, 0),
            ParseInt(2, 1),
            GetField(3, 2, 0),
            Ret(3),
        ],
    );
    assert_eq!(res, Ok(Value::Int(-12345)));
    assert!(vm.fuel_used() > 0);

    let mut m = mod1(2, vec![Const(0, 0), ParseInt(1, 0), Tag(0, 1), Ret(0)]);
    m.consts[0] = Const::Str("+12".into());
    let mut vm = Vm::new(m, &[]).unwrap();
    assert_eq!(vm.run(&mut NoHost, Limits::default()), Ok(Value::Int(0)));

    check_all(&[
        (
            3,
            &[
                Const(0, 0),
                StrToBytes(1, 0),
                BytesToStr(2, 1),
                Tag(0, 2),
                Ret(0),
            ],
            Ok(Value::Int(1)),
        ),
        (
            2,
            &[Const(0, 1), BytesToStr(1, 0), Tag(0, 1), Ret(0)],
            Ok(Value::Int(0)),
        ),
        (
            2,
            &[Byte(0, 200), ToInt(1, 0), ToByte(0, 1), Ret(0)],
            Ok(Value::Byte(200)),
        ),
        (1, &[Int(0, 256), ToByte(0, 0), Ret(0)], Err(Trap::Overflow)),
        (
            1,
            &[Bool(0, false), Assert(0, 0), Ret(0)],
            Err(Trap::AssertFailed("hello world".into())),
        ),
        (1, &[Trap(77)], Err(Trap::User(77))),
    ]);

    let m_imp = Module {
        consts: vec![],
        imports: vec!["env.add".into()],
        funcs: vec![Func {
            name: "entry".into(),
            params: 2,
            regs: 3,
            code: vec![CallHost(2, 0, 0, 2), Ret(2)],
        }],
        entry: 0,
    };
    assert_eq!(
        Vm::new(m_imp.clone(), &[]).unwrap_err(),
        LoadError::MissingImport("env.add".into())
    );
    let mut vm = Vm::new(m_imp, &["env.add"]).unwrap();
    assert_eq!(vm.run(&mut NoHost, Limits::default()), Err(Trap::TypeError));
    assert_eq!(
        vm.call(
            "entry",
            &[Value::Int(10), Value::Int(32)],
            &mut NoHost,
            Limits::default()
        ),
        Err(Trap::Host("no host".into()))
    );

    struct AddHost;
    impl Host for AddHost {
        fn call(&mut self, name: &str, args: &[Value], _heap: &mut Heap) -> Result<Value, Trap> {
            assert_eq!(name, "env.add");
            let (Value::Int(a), Value::Int(b)) = (args[0], args[1]) else {
                return Err(Trap::TypeError);
            };
            Ok(Value::Int(a + b))
        }
    }
    assert_eq!(
        vm.call(
            "entry",
            &[Value::Int(10), Value::Int(32)],
            &mut AddHost,
            Limits::default()
        ),
        Ok(Value::Int(42))
    );
    assert_eq!(
        Vm::new(
            Module {
                consts: vec![],
                imports: vec![],
                funcs: vec![],
                entry: 0
            },
            &[]
        )
        .unwrap_err(),
        LoadError::Verify(VerifyError::EntryOutOfRange { entry: 0 })
    );
}

#[test]
fn trap_site_records_failing_op_and_clears_on_clean_run() {
    let m = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![
            Func {
                name: "main".into(),
                params: 0,
                regs: 2,
                code: vec![Int(0, 1), Call(1, 1, 0, 1), Ret(1)],
            },
            Func {
                name: "boom".into(),
                params: 1,
                regs: 2,
                code: vec![Int(1, 0), Div(0, 0, 1), Ret(0)],
            },
            Func {
                name: "clean".into(),
                params: 0,
                regs: 1,
                code: vec![Int(0, 7), Ret(0)],
            },
        ],
        entry: 0,
    };
    let mut vm = Vm::new(m, &[]).unwrap();
    assert_eq!(vm.trap_site(), None);
    assert_eq!(
        vm.run(&mut NoHost, Limits::default()),
        Err(Trap::DivideByZero)
    );
    assert_eq!(vm.trap_site(), Some((1, 1)));
    assert_eq!(
        vm.call("clean", &[], &mut NoHost, Limits::default()),
        Ok(Value::Int(7))
    );
    assert_eq!(vm.trap_site(), None);
}
