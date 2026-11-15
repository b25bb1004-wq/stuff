//! Unit, corpus, reject, and fuzz tests for the Logos type and layer checker.
#![allow(clippy::unwrap_used)]

use super::{check, validate};
use crate::ast::BinOp;
use crate::ir::{Body, Builtin, Func, Inst, Lit, Program, Ty};
use crate::parser::parse;
use crate::span::Span;
use assay::{forall, Rng};
use std::fs;
use std::path::PathBuf;

fn load_dir(subdir: &str) -> Vec<(String, String)> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("tests");
    dir.push(subdir);
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {:?}: {}", dir, e))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "logos"))
        .collect();
    entries.sort();
    entries
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let src = fs::read_to_string(&p).unwrap();
            (name, src)
        })
        .collect()
}

#[test]
fn typed_corpus_passes_and_validates() {
    let files = load_dir("typed");
    assert!(
        files.len() >= 20,
        "expected at least 20 typed corpus files, found {}",
        files.len()
    );
    for (name, src) in &files {
        let m = parse(src).unwrap_or_else(|d| panic!("typed/{} parse failed: {:?}", name, d));
        let prog = check(&m).unwrap_or_else(|d| panic!("typed/{} check failed: {:?}", name, d));
        validate(&prog).unwrap_or_else(|d| panic!("typed/{} validate failed: {:?}", name, d));
    }
}

#[test]
fn reject_corpus_fails_with_expected_prefix_and_valid_span() {
    let files = load_dir("reject");
    assert!(
        files.len() >= 40,
        "expected at least 40 reject files, found {}",
        files.len()
    );
    for (name, src) in &files {
        let first_line = src.lines().next().unwrap_or("");
        let prefix = first_line
            .strip_prefix("// error: ")
            .unwrap_or_else(|| panic!("reject/{} missing '// error: <prefix>' header", name));
        let m = parse(src).unwrap_or_else(|d| panic!("reject/{} failed to parse: {:?}", name, d));
        let err = check(&m)
            .err()
            .unwrap_or_else(|| panic!("reject/{} unexpectedly passed type check", name));
        assert!(
            err.message.starts_with(prefix),
            "reject/{}: expected message starting with {:?}, got {:?} at {:?}",
            name,
            prefix,
            err.message,
            err.span
        );
        assert!(
            err.span.start <= err.span.end && (err.span.end as usize) <= src.len(),
            "reject/{}: invalid span {:?} for file length {}",
            name,
            err.span,
            src.len()
        );
    }
}

#[test]
fn lowering_short_circuit_and_compound_eq_and_for_array() {
    let src = r#"
fn check_ops(a: bool, b: bool, t1: (int, str?), t2: (int, str?), xs: [int]) -> int {
    let logic = (a && b) || (!a && !b)
    let eq = t1 == t2 && t1 != t2
    var sum = 0
    for x in xs {
        if logic && eq {
            sum = sum + x
        }
    }
    sum
}
"#;
    let m = parse(src).unwrap();
    let prog = check(&m).unwrap();
    validate(&prog).unwrap();
    let Body::Code { insts, .. } = &prog.funcs[0].body else {
        panic!("expected code body");
    };
    let mut len_calls = 0;
    for inst in insts {
        if let Inst::Bin(_, op, a, _) = inst {
            assert!(!matches!(op, BinOp::And | BinOp::Or));
            if matches!(op, BinOp::Eq | BinOp::Ne) {
                let rty = &prog.funcs[0].regs[*a as usize];
                assert!(matches!(
                    rty,
                    Ty::Int | Ty::Byte | Ty::Bool | Ty::Unit | Ty::Str
                ));
            }
        }
        if matches!(inst, Inst::Builtin(_, Builtin::Len, _)) {
            len_calls += 1;
        }
    }
    assert_eq!(len_calls, 1);
}

#[test]
fn lowering_props_tests_and_consts() {
    let src = r#"
fn out(s: str) uses io = extern "io.print"
const base: int = (10 + 20) * 2

test "t" {
    out("ok")
    assert base == 60
}

prop "pos" (x: int) where x > 0 {
    assert x + base > 60
}
"#;
    let m = parse(src).unwrap();
    let prog = check(&m).unwrap();
    validate(&prog).unwrap();
    assert_eq!(prog.tests.len(), 1);
    assert_eq!(prog.props.len(), 1);
    let test_fn = &prog.funcs[prog.tests[0].1 as usize];
    assert_eq!(test_fn.layers, 1);
    let prop = &prog.props[0];
    let guard_fn = &prog.funcs[prop.guard.unwrap() as usize];
    assert_eq!(guard_fn.ret, Ty::Bool);
    assert_eq!(guard_fn.layers, 0);
    let body_fn = &prog.funcs[prop.func as usize];
    assert_eq!(body_fn.ret, Ty::Unit);
    assert_eq!(body_fn.layers, 0);
}

#[test]
fn validate_catches_invalid_ir() {
    let mut prog = Program {
        types: Vec::new(),
        layers: vec![
            "io".into(),
            "fs".into(),
            "net".into(),
            "proc".into(),
            "env".into(),
            "time".into(),
            "rand".into(),
        ],
        funcs: vec![Func {
            name: "f".into(),
            params: 0,
            ret: Ty::Unit,
            layers: 0,
            regs: vec![Ty::Unit],
            bindings: Vec::new(),
            body: Body::Code {
                insts: vec![Inst::Lit(0, Lit::Unit), Inst::Ret(0)],
                spans: vec![Span { start: 0, end: 0 }; 2],
            },
        }],
        tests: Vec::new(),
        props: Vec::new(),
        main: None,
    };
    assert!(validate(&prog).is_ok());

    // Register out of bounds
    prog.funcs[0].body = Body::Code {
        insts: vec![Inst::Ret(5)],
        spans: vec![Span { start: 0, end: 0 }],
    };
    assert!(validate(&prog).is_err());

    // Undefined label
    prog.funcs[0].body = Body::Code {
        insts: vec![Inst::Jump(99)],
        spans: vec![Span { start: 0, end: 0 }],
    };
    assert!(validate(&prog).is_err());
}

fn random_single_char_edit(src: &str, rng: &mut Rng) -> String {
    let mut chars: Vec<char> = src.chars().collect();
    let choices = [
        ' ', '\n', ';', '(', ')', '[', ']', '{', '}', '"', '\'', '.', ',', ':', '=', '+', '-', '*',
        '/', '<', '>', '!', '&', '|', '^', '?', '0', '1', 'a', 'x', 'y', '_',
    ];
    if chars.is_empty() {
        chars.push(*rng.pick(&choices));
    } else {
        let idx = rng.below(chars.len());
        match rng.below(3) {
            0 => {
                chars.remove(idx);
            }
            1 => {
                chars[idx] = *rng.pick(&choices);
            }
            _ => {
                chars.insert(idx, *rng.pick(&choices));
            }
        }
    }
    chars.into_iter().collect()
}

#[test]
fn fuzz_corpus_edits_never_panic() {
    let corpus = load_dir("corpus");
    forall(2000, 0xABCD_1234_5678_9ABC, |rng| {
        let (_, base) = rng.pick(&corpus);
        let edited = random_single_char_edit(base, rng);
        if let Ok(m) = parse(&edited) {
            if let Ok(prog) = check(&m) {
                validate(&prog).unwrap();
            }
        }
    });
}

const TOKENS: &[&str] = &[
    "fn", "type", "layer", "const", "let", "var", "if", "else", "match", "while", "for", "in",
    "return", "break", "continue", "assert", "test", "prop", "where", "uses", "extern", "true",
    "false", "none", "some", "int", "bool", "byte", "str", "unit", "x", "y", "f", "main", "_",
    "Point", "Shape", "Circle", "0", "1", "42", "b'a'", "\"s\"", "+", "-", "*", "/", "%", "+%",
    "-%", "*%", "==", "!=", "<", "<=", ">", ">=", "&&", "||", "!", "&", "|", "^", "<<", ">>", "=",
    "|>", "->", "=>", ".", "..", ",", ":", ";", "?", "(", ")", "[", "]", "{", "}", "\n",
];

#[test]
fn fuzz_token_soup_never_panics() {
    forall(2000, 0xFEDC_BA98_7654_3210, |rng| {
        let count = rng.range(0, 64) as usize;
        let mut src = String::new();
        for i in 0..count {
            if i > 0 {
                src.push(' ');
            }
            src.push_str(rng.pick(TOKENS));
        }
        if let Ok(m) = parse(&src) {
            if let Ok(prog) = check(&m) {
                validate(&prog).unwrap();
            }
        }
    });
}

#[test]
fn bindings_cover_for_variables_and_match_arms() {
    let src = r#"
fn f(o: int?) -> int {
    var sum = 0
    for i in 1..4 {
        sum = sum + i
    }
    match o {
        some(v) => v + 1,
        none => sum,
    }
}
"#;
    let m = parse(src).unwrap();
    let prog = check(&m).unwrap();
    validate(&prog).unwrap();
    let f = &prog.funcs[0];
    let find = |name: &str| f.bindings.iter().find(|b| b.name == name);
    let i = find("i").expect("for variable is recorded");
    assert!(i.from < i.to, "for variable has a non-empty scope");
    let v = find("v").expect("match-arm binding is recorded");
    assert!(v.from < v.to, "match-arm binding has a non-empty scope");
    let sum = find("sum").expect("var is recorded");
    assert!(
        sum.to < u32::MAX,
        "var scope is closed at the end of the body"
    );
    assert!(sum.to > i.to, "var outlives the for loop");
}
