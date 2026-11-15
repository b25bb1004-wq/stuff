//! Source-level stepping tests for `cobalt::step`, breakpoints, locals and `line_of`.
#![allow(clippy::unwrap_used)]
use cobalt::{compile_with_source, first_pc_of_line, line_of, locals_at, step, Compiled, Step};
use elysium::{Event, Limits, NoHost, Op, Session, Value, Vm};

/// `add` is on line 1, `main` (the call) on line 2.
const CALL_SRC: &str = "fn add(a: int, b: int) -> int { a + b }\nfn main() -> int { add(2, 3) }\n";
const ADD_LINE: u32 = 1;
/// Line 4 is the call in `CALL_SRC2`; line 3 is a statement before it, so the session start is not on it.
const CALL_SRC2: &str =
    "fn add(a: int, b: int) -> int { a + b }\nfn main() -> int {\n var x = 1;\n add(2, 3)\n}\n";
const CALL_LINE2: u32 = 4;
/// Line 6 is `i = i + 1;`, the loop body.
const LOOP_SRC: &str =
    "fn main() -> int {\n var i = 0;\n var s = 0;\n while i < 3 {\n s = s + i;\n i = i + 1;\n }\n s\n}\n";
const LOOP_BODY_LINE: u32 = 6;
const ENDLESS_SRC: &str =
    "fn main() -> int {\n var i = 0;\n while true {\n i = i + 1;\n }\n i\n}\n";

fn build(src: &str) -> Compiled {
    let ast = logos::parser::parse(src).unwrap_or_else(|d| panic!("parse: {d:?}\n{src}"));
    let prog = logos::check::check(&ast).unwrap_or_else(|d| panic!("check: {d:?}\n{src}"));
    compile_with_source(&prog, src)
}

fn func_index(c: &Compiled, name: &str) -> u32 {
    c.module.funcs.iter().position(|f| f.name == name).unwrap() as u32
}

/// Index of the first (or last, when `last`) op of `func` matching `pred`.
fn find_op(c: &Compiled, func: u32, pred: fn(&Op) -> bool, last: bool) -> u32 {
    let code = &c.module.funcs[func as usize].code;
    let pc = if last {
        code.iter().rposition(pred)
    } else {
        code.iter().position(pred)
    };
    pc.unwrap() as u32
}

fn start(c: &Compiled) -> (Vm, Session) {
    let mut vm = Vm::new(c.module.clone(), &[]).unwrap();
    let limits = Limits {
        fuel: 100_000_000,
        max_heap_bytes: 1 << 20,
        max_depth: 100,
    };
    let session = vm.start(c.main.unwrap(), &[], limits).unwrap();
    (vm, session)
}

fn is_add(op: &Op) -> bool {
    matches!(op, Op::Add(..))
}

#[test]
fn breakpoint_in_callee_pauses_with_named_argument_values() {
    let c = build(CALL_SRC);
    let add = func_index(&c, "add");
    let body = find_op(&c, add, is_add, false);
    let (mut vm, mut s) = start(&c);
    match step(
        &c,
        &mut s,
        &mut vm,
        &mut NoHost,
        Step::Continue,
        &[ADD_LINE],
    ) {
        Event::Paused(p) => {
            assert_eq!((p.func, p.pc, p.depth), (add, body, 2));
            let frames = s.frames();
            let regs = &frames.last().unwrap().regs;
            assert_eq!(regs[0], Value::Int(2));
            assert_eq!(regs[1], Value::Int(3));
            let locals = locals_at(&c, add, p.pc);
            assert_eq!(locals, vec![("a".to_string(), 0), ("b".to_string(), 1)]);
        }
        other => panic!("expected pause, got {other:?}"),
    }
}

#[test]
fn over_on_call_line_does_not_stop_inside_callee() {
    let c = build(CALL_SRC2);
    let main = c.main.unwrap();
    let add = func_index(&c, "add");
    let (mut vm, mut s) = start(&c);
    assert!(matches!(
        step(&c, &mut s, &mut vm, &mut NoHost, Step::Continue, &[CALL_LINE2]),
        Event::Paused(p) if p.func == main && line_of(&c, main, p.pc).unwrap().0 == CALL_LINE2
    ));
    match step(&c, &mut s, &mut vm, &mut NoHost, Step::Over, &[]) {
        Event::Paused(p) => assert_ne!(p.func, add, "stopped inside callee"),
        Event::Done(v) => assert_eq!(v, Value::Int(5)),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn into_on_call_line_enters_callee() {
    let c = build(CALL_SRC2);
    let main = c.main.unwrap();
    let add = func_index(&c, "add");
    let (mut vm, mut s) = start(&c);
    assert!(matches!(
        step(&c, &mut s, &mut vm, &mut NoHost, Step::Continue, &[CALL_LINE2]),
        Event::Paused(p) if p.func == main
    ));
    match step(&c, &mut s, &mut vm, &mut NoHost, Step::Into, &[]) {
        Event::Paused(p) => assert_eq!((p.func, p.depth), (add, 2)),
        other => panic!("expected pause in callee, got {other:?}"),
    }
}

#[test]
fn out_returns_to_caller() {
    let c = build(CALL_SRC);
    let main = c.main.unwrap();
    let add = func_index(&c, "add");
    let (mut vm, mut s) = start(&c);
    assert!(matches!(
        step(&c, &mut s, &mut vm, &mut NoHost, Step::Continue, &[ADD_LINE]),
        Event::Paused(p) if p.func == add
    ));
    match step(&c, &mut s, &mut vm, &mut NoHost, Step::Out, &[]) {
        Event::Paused(p) => assert_eq!((p.func, p.depth), (main, 1)),
        other => panic!("expected pause in caller, got {other:?}"),
    }
}

#[test]
fn loop_breakpoint_pauses_once_per_iteration() {
    let c = build(LOOP_SRC);
    let main = c.main.unwrap();
    let body = find_op(&c, main, is_add, true);
    assert_eq!(line_of(&c, main, body).unwrap().0, LOOP_BODY_LINE);
    let (mut vm, mut s) = start(&c);
    let mut pauses = 0;
    loop {
        match step(
            &c,
            &mut s,
            &mut vm,
            &mut NoHost,
            Step::Continue,
            &[LOOP_BODY_LINE],
        ) {
            Event::Paused(p) => {
                assert_eq!(p.func, main);
                assert_eq!(line_of(&c, main, p.pc).unwrap().0, LOOP_BODY_LINE);
                pauses += 1;
            }
            Event::Done(v) => {
                assert_eq!(v, Value::Int(3));
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(pauses, 3);
}

#[test]
fn line_of_gives_real_lines_matching_source() {
    let c = build(CALL_SRC);
    for (f, func) in c.module.funcs.iter().enumerate() {
        for pc in 0..func.code.len() {
            let sp = c.spans[f][pc];
            let (line, col) = line_of(&c, f as u32, pc as u32).unwrap();
            assert_eq!(line, logos::line_col(CALL_SRC, sp.start).0);
            assert!(col >= 1);
        }
    }
    let add = func_index(&c, "add");
    let body = find_op(&c, add, is_add, false);
    assert_eq!(line_of(&c, add, body).unwrap().0, ADD_LINE);
    assert!(first_pc_of_line(&c, ADD_LINE).contains(&(add, body)));
    assert_eq!(line_of(&c, u32::MAX, 0), None);
    // Without source there is no line table.
    let plain = {
        let ast = logos::parser::parse(CALL_SRC).unwrap();
        let prog = logos::check::check(&ast).unwrap();
        cobalt::compile(&prog)
    };
    assert_eq!(line_of(&plain, add, body), None);
}

#[test]
fn endless_loop_with_continue_returns_budget() {
    let c = build(ENDLESS_SRC);
    let (mut vm, mut s) = start(&c);
    assert_eq!(
        step(&c, &mut s, &mut vm, &mut NoHost, Step::Continue, &[]),
        Event::Budget
    );
}

/// `x` is defined on line 2, `y` on line 3, and `y` is the result on line 4.
const LET_SRC: &str = "fn main() -> int {\n var x = 1;\n let y = x + 2;\n y\n}\n";

#[test]
fn locals_include_lets_in_scope_with_their_registers() {
    let c = build(LET_SRC);
    let main = func_index(&c, "main");
    // Line 3 starts before `y` is defined, so only `x` is in scope there.
    let (mut vm, mut s) = start(&c);
    match step(&c, &mut s, &mut vm, &mut NoHost, Step::Continue, &[3]) {
        Event::Paused(p) => {
            let names: Vec<String> = locals_at(&c, main, p.pc)
                .into_iter()
                .map(|(n, _)| n)
                .collect();
            assert_eq!(names, vec!["x".to_string()]);
        }
        other => panic!("expected pause on line 3, got {other:?}"),
    }
    // Line 4 has both; each register must hold the value of its binding.
    let (mut vm, mut s) = start(&c);
    match step(&c, &mut s, &mut vm, &mut NoHost, Step::Continue, &[4]) {
        Event::Paused(p) => {
            assert_eq!(line_of(&c, main, p.pc).unwrap().0, 4);
            let locals = locals_at(&c, main, p.pc);
            let names: Vec<&str> = locals.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, vec!["x", "y"]);
            let frames = s.frames();
            let regs = &frames.last().unwrap().regs;
            for (name, reg) in &locals {
                let want = if name == "x" { 1 } else { 3 };
                assert_eq!(regs[*reg as usize], Value::Int(want), "local {name}");
            }
            assert_ne!(locals[0].1, locals[1].1, "x and y need separate registers");
        }
        other => panic!("expected pause on line 4, got {other:?}"),
    }
}

/// `work` is on line 1; the caller's `var x = work(300);` is on line 4.
const RET_SRC: &str = "fn work(n: int) -> int { n + 1 }\nfn main() {\n var y = 1;\n var x = work(300);\n var z = 2;\n}\n";
/// Same, but the call is the last statement, so the implicit return follows it.
const RET_LAST_SRC: &str =
    "fn work(n: int) -> int { n + 1 }\nfn main() {\n var y = 1;\n var x = work(300);\n}\n";

/// Over from a statement that calls a function must stop on the next statement, never on the
/// `fn` header line the implicit return used to borrow its span from.
#[test]
fn over_after_returned_call_stays_on_caller_line() {
    let c = build(RET_SRC);
    let main = func_index(&c, "main");
    let (mut vm, mut s) = start(&c);
    match step(&c, &mut s, &mut vm, &mut NoHost, Step::Continue, &[4]) {
        Event::Paused(p) => assert_eq!(p.func, main),
        other => panic!("expected pause on line 4, got {other:?}"),
    }
    match step(&c, &mut s, &mut vm, &mut NoHost, Step::Over, &[]) {
        Event::Paused(p) => {
            let line = line_of(&c, p.func, p.pc).unwrap().0;
            assert_eq!((p.func, line), (main, 5), "over stopped on line {line}");
        }
        other => panic!("expected pause after Over, got {other:?}"),
    }

    let c = build(RET_LAST_SRC);
    let (mut vm, mut s) = start(&c);
    match step(&c, &mut s, &mut vm, &mut NoHost, Step::Continue, &[4]) {
        Event::Paused(_) => {}
        other => panic!("expected pause on line 4, got {other:?}"),
    }
    match step(&c, &mut s, &mut vm, &mut NoHost, Step::Over, &[]) {
        Event::Paused(p) => {
            let line = line_of(&c, p.func, p.pc).unwrap().0;
            assert_ne!(line, 2, "over stopped on the fn header line");
        }
        Event::Done(_) => {}
        other => panic!("unexpected {other:?}"),
    }
}
