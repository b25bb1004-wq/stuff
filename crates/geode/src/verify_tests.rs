//! Directed tests: a valid module with exactly one verifier rule broken, per rule.

#![allow(clippy::unwrap_used)]

use crate::module::{MAX_CODE, MAX_REGS};
use crate::{verify, Const, Func, Module, Op, VerifyError};

/// A module with one function "main" (4 registers) running `code`, plus a one-parameter "id".
fn module_with(code: Vec<Op>) -> Module {
    Module {
        consts: vec![Const::Str("assert failed".into()), Const::Bytes(vec![1, 2])],
        imports: vec!["io.print".into()],
        funcs: vec![
            Func {
                name: "main".into(),
                params: 0,
                regs: 4,
                code,
            },
            Func {
                name: "id".into(),
                params: 1,
                regs: 1,
                code: vec![Op::Ret(0)],
            },
        ],
        entry: 0,
    }
}

/// The module with the rule broken in `code`, and the error it must give.
fn expect(code: Vec<Op>, err: VerifyError) {
    assert_eq!(verify(&module_with(code)), Err(err));
}

#[test]
fn valid_module_passes() {
    let code = vec![
        Op::Int(0, 1),
        Op::Call(1, 1, 2, 1),
        Op::Assert(0, 0),
        Op::CallHost(2, 0, 0, 0),
        Op::Const(3, 1),
        Op::FnRef(3, 1),
        Op::Ret(1),
    ];
    assert_eq!(verify(&module_with(code)), Ok(()));
}

#[test]
fn entry_out_of_range() {
    let mut m = module_with(vec![Op::Ret(0)]);
    m.entry = 2;
    assert_eq!(verify(&m), Err(VerifyError::EntryOutOfRange { entry: 2 }));
}

#[test]
fn register_out_of_range() {
    expect(
        vec![Op::Move(4, 0), Op::Ret(0)],
        VerifyError::RegisterOutOfRange {
            func: 0,
            at: 0,
            reg: 4,
        },
    );
}

#[test]
fn register_out_of_range_in_second_function() {
    let mut m = module_with(vec![Op::Ret(0)]);
    m.funcs[1].code = vec![Op::Ret(3)];
    assert_eq!(
        verify(&m),
        Err(VerifyError::RegisterOutOfRange {
            func: 1,
            at: 0,
            reg: 3,
        })
    );
}

#[test]
fn register_span_out_of_range() {
    expect(
        vec![Op::Call(0, 1, 4, 1), Op::Ret(0)],
        VerifyError::RegisterRangeOutOfRange { func: 0, at: 0 },
    );
}

#[test]
fn constant_out_of_range() {
    expect(
        vec![Op::Const(0, 2), Op::Ret(0)],
        VerifyError::ConstOutOfRange {
            func: 0,
            at: 0,
            index: 2,
        },
    );
}

#[test]
fn function_out_of_range() {
    expect(
        vec![Op::FnRef(0, 2), Op::Ret(0)],
        VerifyError::FuncOutOfRange {
            func: 0,
            at: 0,
            index: 2,
        },
    );
}

#[test]
fn import_out_of_range() {
    expect(
        vec![Op::CallHost(0, 1, 0, 0), Op::Ret(0)],
        VerifyError::ImportOutOfRange {
            func: 0,
            at: 0,
            index: 1,
        },
    );
}

#[test]
fn jump_target_out_of_range() {
    expect(
        vec![Op::Jump(2), Op::Ret(0)],
        VerifyError::JumpOutOfRange {
            func: 0,
            at: 0,
            target: 2,
        },
    );
}

#[test]
fn call_argument_count_must_match_params() {
    expect(
        vec![Op::Call(0, 1, 0, 2), Op::Ret(0)],
        VerifyError::ArgCountMismatch {
            func: 0,
            at: 0,
            callee: 1,
            expected: 1,
            got: 2,
        },
    );
}

#[test]
fn assert_constant_must_be_a_string() {
    expect(
        vec![Op::Assert(0, 1), Op::Ret(0)],
        VerifyError::AssertNotString {
            func: 0,
            at: 0,
            index: 1,
        },
    );
}

#[test]
fn params_must_not_exceed_regs() {
    let mut m = module_with(vec![Op::Ret(0)]);
    m.funcs[0].params = 5;
    assert_eq!(verify(&m), Err(VerifyError::ParamsExceedRegs { func: 0 }));
}

#[test]
fn frame_size_limit() {
    let mut m = module_with(vec![Op::Ret(0)]);
    m.funcs[0].regs = MAX_REGS + 1;
    assert_eq!(verify(&m), Err(VerifyError::FrameTooLarge { func: 0 }));
    m.funcs[0].regs = MAX_REGS;
    assert_eq!(verify(&m), Ok(()));
}

#[test]
fn empty_code() {
    expect(vec![], VerifyError::EmptyCode { func: 0 });
}

#[test]
fn last_instruction_must_end_the_function() {
    expect(
        vec![Op::Ret(0), Op::Unit(0)],
        VerifyError::BadLastInstruction { func: 0 },
    );
}

#[test]
fn instruction_limit() {
    let mut code = vec![Op::Trap(0); MAX_CODE];
    assert_eq!(verify(&module_with(code.clone())), Ok(()));
    code.push(Op::Trap(0));
    expect(code, VerifyError::TooManyInstructions { func: 0 });
}

#[test]
fn table_limits() {
    let mut m = module_with(vec![Op::Ret(0)]);
    m.funcs = vec![m.funcs[1].clone(); 65_537];
    assert_eq!(verify(&m), Err(VerifyError::TooManyFuncs));

    let mut m = module_with(vec![Op::Ret(0)]);
    m.consts = vec![Const::Str(String::new()); 65_537];
    assert_eq!(verify(&m), Err(VerifyError::TooManyConsts));

    let mut m = module_with(vec![Op::Ret(0)]);
    m.imports = vec![String::from("x"); 65_537];
    assert_eq!(verify(&m), Err(VerifyError::TooManyImports));
}

#[test]
fn name_length_limit() {
    let mut m = module_with(vec![Op::Ret(0)]);
    m.imports = vec!["a".repeat(255)];
    assert_eq!(verify(&m), Ok(()));
    m.imports = vec!["a".repeat(256)];
    assert_eq!(
        verify(&m),
        Err(VerifyError::ImportNameTooLong { import: 0 })
    );

    let mut m = module_with(vec![Op::Ret(0)]);
    m.funcs[1].name = "b".repeat(256);
    assert_eq!(verify(&m), Err(VerifyError::FuncNameTooLong { func: 1 }));
}
