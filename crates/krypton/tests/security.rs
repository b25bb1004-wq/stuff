//! Security tests for krypton parsing, arithmetic, globbing, and expansion.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use krypton::{arith, ast, expand, expand_one, glob_match, parse, Env};

#[derive(Default)]
struct TestEnv {
    vars: BTreeMap<String, String>,
    pos: Vec<String>,
    dirs: BTreeMap<String, Vec<String>>,
    cmd_calls: usize,
}

impl Env for TestEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn set(&mut self, name: &str, value: &str) {
        self.vars.insert(name.to_string(), value.to_string());
    }

    fn positional(&self) -> Vec<String> {
        self.pos.clone()
    }

    fn command(&mut self, _s: &ast::Script) -> String {
        self.cmd_calls += 1;
        String::from("0")
    }

    fn home(&self, _user: &str) -> Option<String> {
        Some(String::from("/home/user"))
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        self.dirs.get(dir).cloned().unwrap_or_default()
    }
}

fn first_word(src: &str) -> ast::Word {
    let script = parse(src, krypton::Escapes::Posix).unwrap();
    match &script.items[0].first.cmds[0] {
        ast::Command::Simple(s) => s.words[0].clone(),
        _ => panic!("expected simple command"),
    }
}

#[test]
fn arith_chained_ternary_stack_overflow() {
    let mut env = TestEnv::default();
    let expr = format!("{}2", "0?1:".repeat(20_000));
    assert!(arith(&expr, &mut env).is_err());
}

#[test]
fn expand_assign_exponential_blowup() {
    let mut env = TestEnv::default();
    let mut src = String::from("${v0=aaaaaaaaaaaaaaaa}");
    for i in 1..=20 {
        src.push_str(&format!("${{v{i}=$v{prev}$v{prev}}}", prev = i - 1));
    }
    let word = first_word(&src);
    assert!(expand_one(&word, &mut env).is_err());

    let mut env2 = TestEnv::default();
    let mut nested = String::from("aaaaaaaaaaaaaaaa");
    for _ in 0..18 {
        nested = format!("${{x={nested}$x$x}}");
    }
    let word2 = first_word(&nested);
    assert!(expand(&[word2], &mut env2).is_err());
}

#[test]
fn expand_pathname_combinatorial_blowup() {
    let mut env = TestEnv::default();
    let entries: Vec<String> = (0..8).map(|i| format!("d{i}")).collect();
    env.dirs.insert(String::from("."), entries.clone());
    // Populate nested directory listings dynamically by matching any queried path?
    // TestEnv uses exact dir keys, so build a 7-level tree of 8-way dirs? No, 8^7 keys is huge!
    // Use a custom Env that returns 8 entries for every dir.
    struct FanoutEnv(Vec<String>);
    impl Env for FanoutEnv {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
        fn set(&mut self, _: &str, _: &str) {}
        fn positional(&self) -> Vec<String> {
            Vec::new()
        }
        fn command(&mut self, _: &ast::Script) -> String {
            String::new()
        }
        fn home(&self, _: &str) -> Option<String> {
            None
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            self.0.clone()
        }
    }

    let mut fanout = FanoutEnv(entries);
    let word = first_word("*/*/*/*/*/*/*");
    assert!(expand(&[word], &mut fanout).is_err());
}

#[test]
fn trim_prefix_suffix_quadratic_hang() {
    let mut env = TestEnv::default();
    env.set("v", &"a".repeat(1500));
    let pat = format!("*{}b", "a".repeat(300));
    let src_pre = format!("${{v#{pat}}}");
    let src_suf = format!("${{v%{pat}}}");
    let w_pre = first_word(&src_pre);
    let w_suf = first_word(&src_suf);

    let start = Instant::now();
    let res_pre = expand_one(&w_pre, &mut env).unwrap();
    let res_suf = expand_one(&w_suf, &mut env).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(res_pre.len(), 1500);
    assert_eq!(res_suf.len(), 1500);
    assert!(
        elapsed < Duration::from_secs(1),
        "trim_prefix/trim_suffix took {elapsed:?}"
    );
}

#[test]
fn arith_template_unterminated_heredoc_accept() {
    let mut env = TestEnv::default();
    let word = first_word("$(( $(cat <<EOF) ))");
    assert!(expand_one(&word, &mut env).is_err());
    assert_eq!(env.cmd_calls, 0);
}

#[test]
fn arith_div_zero_and_i64_min_overflow() {
    let mut env = TestEnv::default();
    assert!(arith("1 / 0", &mut env).is_err());
    assert!(arith("1 % 0", &mut env).is_err());
    env.set("x", "5");
    assert!(arith("x /= 0", &mut env).is_err());
    assert!(arith("x %= 0", &mut env).is_err());
    // Short-circuiting must not evaluate division by zero.
    assert_eq!(arith("1 || 1 / 0", &mut env).unwrap(), 1);
    assert_eq!(arith("0 && 1 / 0", &mut env).unwrap(), 0);
    assert_eq!(arith("1 ? 42 : 1 / 0", &mut env).unwrap(), 42);

    // Signed overflow edge cases must wrap without panicking.
    assert_eq!(
        arith("-9223372036854775808 / -1", &mut env).unwrap(),
        i64::MIN
    );
    assert_eq!(arith("-9223372036854775808 % -1", &mut env).unwrap(), 0);
    assert_eq!(
        arith("-(-9223372036854775808)", &mut env).unwrap(),
        i64::MIN
    );
    assert_eq!(arith("1 << 999999", &mut env).unwrap(), 1 << (999999 & 63));
    assert!(arith("99999999999999999999999999 + 1", &mut env).is_ok());
}

#[test]
fn arith_deep_nesting_parens_unary_assign() {
    let mut env = TestEnv::default();
    let parens = format!("{}1{}", "(".repeat(1000), ")".repeat(1000));
    assert!(arith(&parens, &mut env).is_err());
    let unaries = format!("{}1", "-".repeat(1000));
    assert!(arith(&unaries, &mut env).is_err());
    let assigns = format!("{}1", "x=".repeat(1000));
    assert!(arith(&assigns, &mut env).is_err());
}

#[test]
fn parser_deep_nesting_subshells_groups_substs_quotes() {
    let subs = format!("{}true{}", "(".repeat(500), ")".repeat(500));
    assert!(parse(&subs, krypton::Escapes::Posix).is_err());

    let groups = format!("{}true{}", "{ ".repeat(500), "; }".repeat(500));
    assert!(parse(&groups, krypton::Escapes::Posix).is_err());

    let ifs = format!("{}true{}", "if true; then ".repeat(500), "; fi".repeat(500));
    assert!(parse(&ifs, krypton::Escapes::Posix).is_err());

    let cmd_subst = format!("echo {}1{}", "$(".repeat(500), ")".repeat(500));
    assert!(parse(&cmd_subst, krypton::Escapes::Posix).is_err());

    let braces = format!("echo {}1{}", "${x:-".repeat(500), "}".repeat(500));
    assert!(parse(&braces, krypton::Escapes::Posix).is_err());

    let quoted_braces = format!("echo {}\"a\"{}", "${x:-\"".repeat(500), "\"}".repeat(500));
    assert!(parse(&quoted_braces, krypton::Escapes::Posix).is_err());
}

#[test]
fn glob_many_stars_no_exponential_backtracking() {
    let pat = format!("*{}b", "a*".repeat(200));
    let name = "a".repeat(500);
    let start = Instant::now();
    assert!(!glob_match(&pat, &name));
    assert!(start.elapsed() < Duration::from_secs(1));

    let pat_match = format!("*{}a", "a*".repeat(200));
    assert!(glob_match(&pat_match, &name));
}

#[test]
fn parser_unterminated_constructs_rejected() {
    let cases = [
        "'unterminated",
        "\"unterminated",
        "\"trailing\\",
        "echo \\",
        "`unterminated",
        "`trailing\\",
        "$(echo hi",
        "$((1 + 2",
        "$((1 + 2)",
        "$(('abc",
        "${",
        "${x",
        "${x:-abc",
        "${x:",
        "(echo hi",
        "{ echo hi",
        "if true; then echo 1",
        "if true; then echo 1; else",
        "while true; do echo 1",
        "for x in a b",
        "for x in a b;",
        "case x in a) echo 1",
        "f()",
        "f(",
        "echo 1 |",
        "echo 1 &&",
        "!",
        "cat <<EOF\nhello",
        "cat <<EOF",
        "cat <<'EOF",
        "cat <<\"EOF",
        "cat <<\\",
    ];
    for input in cases {
        let err = parse(input, krypton::Escapes::Posix)
            .expect_err(&format!("expected error for {input:?}"));
        assert!(err.incomplete || !err.msg.is_empty());
    }
}

#[test]
fn parser_and_arith_hostile_numbers_and_fds() {
    let mut env = TestEnv::default();
    assert!(arith("08", &mut env).is_err());
    assert!(arith("0x", &mut env).is_err());
    assert!(arith("123abc", &mut env).is_err());
    assert!(parse("99999999999999999999999999<file", krypton::Escapes::Posix).is_err());
    assert!(!glob_match("[[:alpha:]]", "\u{00e9}"));
    assert!(glob_match("[a-z]", "m"));
    assert!(!glob_match("[z-a]", "m"));
    assert!(!glob_match("a\\", "a\\"));
}
