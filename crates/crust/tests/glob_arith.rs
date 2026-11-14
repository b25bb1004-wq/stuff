//! Arithmetic and glob matching test tables for `crust`.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use crust::ast::Script;
use crust::{arith, glob_match, Env, ExpandError};

struct ArithEnv {
    vars: BTreeMap<String, String>,
}

impl ArithEnv {
    fn new() -> Self {
        Self {
            vars: BTreeMap::new(),
        }
    }
}

impl Env for ArithEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }
    fn set(&mut self, name: &str, value: &str) {
        self.vars.insert(String::from(name), String::from(value));
    }
    fn positional(&self) -> Vec<String> {
        Vec::new()
    }
    fn command(&mut self, _: &Script) -> String {
        String::new()
    }
    fn home(&self, _: &str) -> Option<String> {
        None
    }
    fn list_dir(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

#[test]
fn arith_table() {
    let mut env = ArithEnv::new();
    env.set("a", "10");
    env.set("b", "-3");
    env.set("empty", "");
    env.set("hexvar", "0x10");
    env.set("octvar", "010");

    let cases: Vec<(&str, Result<i64, ()>)> = vec![
        ("", Ok(0)),
        ("   ", Ok(0)),
        ("42", Ok(42)),
        ("0", Ok(0)),
        ("0x1a", Ok(26)),
        ("0XFF", Ok(255)),
        ("077", Ok(63)),
        ("010", Ok(8)),
        ("08", Err(())),
        ("0x", Err(())),
        ("unset_var", Ok(0)),
        ("empty", Ok(0)),
        ("a + b", Ok(7)),
        ("hexvar + octvar", Ok(24)),
        ("1 + 2 * 3", Ok(7)),
        ("(1 + 2) * 3", Ok(9)),
        ("20 / 3", Ok(6)),
        ("20 % 3", Ok(2)),
        ("-20 / 3", Ok(-6)),
        ("-20 % 3", Ok(-2)),
        ("10 - 3 - 2", Ok(5)),
        ("1 << 4", Ok(16)),
        ("32 >> 2", Ok(8)),
        ("1 + 2 << 3", Ok(24)),
        ("3 < 5", Ok(1)),
        ("5 <= 5", Ok(1)),
        ("5 > 5", Ok(0)),
        ("5 >= 5", Ok(1)),
        ("3 == 3", Ok(1)),
        ("3 != 4", Ok(1)),
        ("12 & 10", Ok(8)),
        ("12 ^ 10", Ok(6)),
        ("12 | 3", Ok(15)),
        ("1 | 2 & 0", Ok(1)),
        ("0 && 1", Ok(0)),
        ("1 && 5", Ok(1)),
        ("0 || 0", Ok(0)),
        ("0 || 9", Ok(1)),
        ("0 && (1 / 0)", Ok(0)),
        ("1 || (1 / 0)", Ok(1)),
        ("1 ? 10 : (1 / 0)", Ok(10)),
        ("0 ? (1 / 0) : 20", Ok(20)),
        ("0 ? 1 : 0 ? 2 : 3", Ok(3)),
        ("!0", Ok(1)),
        ("!42", Ok(0)),
        ("~0", Ok(-1)),
        ("--5", Ok(5)),
        ("-+5", Ok(-5)),
        ("1 / 0", Err(())),
        ("1 % 0", Err(())),
        ("9223372036854775807 + 1", Ok(i64::MIN)),
        ("-9223372036854775808 / -1", Ok(i64::MIN)),
        ("1 = 2", Err(())),
        ("(a) = 2", Err(())),
        ("1 +", Err(())),
        ("(1 + 2", Err(())),
    ];

    assert!(cases.len() >= 40);
    for (idx, (expr, expected)) in cases.iter().enumerate() {
        let got = arith(expr, &mut env);
        match expected {
            Ok(v) => assert_eq!(got, Ok(*v), "case {idx}: {expr:?}"),
            Err(()) => assert!(
                matches!(got, Err(ExpandError::Arith(_))),
                "case {idx} should error: {expr:?}, got {got:?}"
            ),
        }
    }

    // Assignment operators and Env::set side effects
    assert_eq!(arith("x = 5", &mut env), Ok(5));
    assert_eq!(env.get("x").as_deref(), Some("5"));
    assert_eq!(arith("x += 3", &mut env), Ok(8));
    assert_eq!(env.get("x").as_deref(), Some("8"));
    assert_eq!(arith("x -= 2", &mut env), Ok(6));
    assert_eq!(arith("x *= 4", &mut env), Ok(24));
    assert_eq!(arith("x /= 5", &mut env), Ok(4));
    assert_eq!(arith("x %= 3", &mut env), Ok(1));
    assert_eq!(arith("y = x = 9", &mut env), Ok(9));
    assert_eq!(env.get("y").as_deref(), Some("9"));
    assert_eq!(env.get("x").as_deref(), Some("9"));
    assert_eq!(arith("0 && (x = 99)", &mut env), Ok(0));
    assert_eq!(env.get("x").as_deref(), Some("9"));
    env.set("bad", "abc");
    assert!(matches!(
        arith("bad + 1", &mut env),
        Err(ExpandError::Arith(_))
    ));
}

#[test]
fn glob_match_extra() {
    let cases = [
        ("foo*", "foobar", true),
        ("*bar", "foobar", true),
        ("*oo*", "foobar", true),
        ("f??b?r", "foobar", true),
        ("f??b?r", "fobar", false),
        ("[a-c][1-3]", "b2", true),
        ("[a-c][1-3]", "d2", false),
        ("[!0-9]*", "abc", true),
        ("[!0-9]*", "1abc", false),
        ("[^a-z]", "Z", true),
        ("[^a-z]", "z", false),
        ("[[:upper:]][[:lower:]]", "Ab", true),
        ("[[:upper:]][[:lower:]]", "ab", false),
        ("[\\]]", "]", true),
        ("[a\\-z]", "-", true),
        ("[a\\-z]", "m", false),
    ];
    for (p, s, ok) in cases {
        assert_eq!(glob_match(p, s), ok, "glob_match({p:?}, {s:?})");
    }
}
