//! Python-semantics, error, and linear-time tests for `slate::Regex`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use slate::Regex;
use std::time::Instant;

type Case<'a> = (&'a str, &'a str, Option<&'a [Option<(usize, usize)>]>);

#[test]
fn regex_python_semantics_table() {
    let s0 = Some([Some((0, 0))].as_slice());
    let cases: &[Case<'_>] = &[
        // Literals and empty pattern
        ("", "", s0),
        ("", "abc", s0),
        ("abc", "xxabcyy", Some(&[Some((2, 5))])),
        ("abc", "ab", None),
        // Escapes
        (r"a\.b\*c\+", "a.b*c+", Some(&[Some((0, 6))])),
        (r"a\.b", "axb", None),
        (r"\\\/\-\^\$", r"\/-^$", Some(&[Some((0, 5))])),
        (r"\(\)\[\]\{\}\|", "()[]{}|", Some(&[Some((0, 7))])),
        (r"a\nb\tc", "a\nb\tc", Some(&[Some((0, 5))])),
        // Dot and UTF-8 char matching
        (".", "\n", None),
        (".", "\na", Some(&[Some((1, 2))])),
        (".", "é", Some(&[Some((0, 2))])),
        (".", "🦀", Some(&[Some((0, 4))])),
        (".+", "aé🦀\nb", Some(&[Some((0, 7))])),
        // Character classes
        ("[abc]+", "zzabcbazz", Some(&[Some((2, 7))])),
        ("[^a-z]+", "abc123\ndef", Some(&[Some((3, 7))])),
        (r"[a\-z]+", "a-zb", Some(&[Some((0, 3))])),
        ("[]a]+", "x]a]y", Some(&[Some((1, 4))])),
        ("[^]a]+", "]a!?", Some(&[Some((2, 4))])),
        ("[-a]+", "-a-b", Some(&[Some((0, 3))])),
        ("[a-]+", "a-ab", Some(&[Some((0, 3))])),
        ("[a-c-e]+", "ab-ed", Some(&[Some((0, 4))])),
        ("[]-a]+", "]^_`a", Some(&[Some((0, 5))])),
        (r"[\b]", "\x08", Some(&[Some((0, 1))])),
        // Shorthands (re.ASCII)
        (r"\d+", "ab123cd", Some(&[Some((2, 5))])),
        (r"\D+", "12ab\n34", Some(&[Some((2, 5))])),
        (r"\w+", "café_1", Some(&[Some((0, 3))])),
        (r"\W+", "abé!cd", Some(&[Some((2, 5))])),
        (r"\s+", "ab \t\n\rcd", Some(&[Some((2, 6))])),
        (r"\S+", " \t\nxyz ", Some(&[Some((3, 6))])),
        (r"[\d_]+", "a1_2b", Some(&[Some((1, 4))])),
        (r"[^\D]+", "a98b", Some(&[Some((1, 3))])),
        // Anchors ^ $ \b \B (non-multiline)
        ("^abc", "abc", Some(&[Some((0, 3))])),
        ("^abc", "xabc", None),
        ("^abc", "x\nabc", None),
        ("a$", "a", Some(&[Some((0, 1))])),
        ("a$", "a\n", Some(&[Some((0, 1))])),
        ("a$", "a\n\n", None),
        ("a$", "ba\nc", None),
        (r"\bword\b", "aword word!", Some(&[Some((6, 10))])),
        (r"\b", "", None),
        (r"\B", "", s0),
        (r"\B\w+\B", "abcd", Some(&[Some((1, 3))])),
        (r"\b\w+", "éfoo", Some(&[Some((2, 5))])),
        // Groups and non-capturing groups
        (
            "(a)(b)",
            "ab",
            Some(&[Some((0, 2)), Some((0, 1)), Some((1, 2))]),
        ),
        ("(?:a)(b)", "ab", Some(&[Some((0, 2)), Some((1, 2))])),
        (
            "((a)b)",
            "ab",
            Some(&[Some((0, 2)), Some((0, 2)), Some((0, 1))]),
        ),
        ("(a)|(b)", "b", Some(&[Some((0, 1)), None, Some((0, 1))])),
        ("(a){0}", "a", Some(&[Some((0, 0)), None])),
        // Alternation preference (leftmost then first alternative)
        ("a|ab", "ab", Some(&[Some((0, 1))])),
        ("ab|a", "ab", Some(&[Some((0, 2))])),
        ("(a|ab)c", "abc", Some(&[Some((0, 3)), Some((0, 2))])),
        ("a|", "b", s0),
        ("|a", "a", s0),
        // Greedy and lazy quantifiers
        ("a*", "aaa", Some(&[Some((0, 3))])),
        ("a*?", "aaa", s0),
        ("a+?", "aaa", Some(&[Some((0, 1))])),
        ("a??", "aaa", s0),
        ("a*b", "aaab", Some(&[Some((0, 4))])),
        ("a*?b", "aaab", Some(&[Some((0, 4))])),
        ("<.*>", "<a><b>", Some(&[Some((0, 6))])),
        ("<.*?>", "<a><b>", Some(&[Some((0, 3))])),
        ("a{3}", "aaaa", Some(&[Some((0, 3))])),
        ("a{2,}", "aaaa", Some(&[Some((0, 4))])),
        ("a{2,}?", "aaaa", Some(&[Some((0, 2))])),
        ("a{1,3}", "aaaa", Some(&[Some((0, 3))])),
        ("a{1,3}?", "aaaa", Some(&[Some((0, 1))])),
        ("a{,2}", "aaaa", Some(&[Some((0, 2))])),
        ("a{,2}?", "aaaa", s0),
        ("a{0,0}", "aaa", s0),
        ("a{,}", "a{,}", Some(&[Some((0, 4))])),
        ("a{foo}", "a{foo}", Some(&[Some((0, 6))])),
        ("a{5", "a{5", Some(&[Some((0, 3))])),
        // Nullable loops and capture group semantics matching Python re
        ("(a)*", "aa", Some(&[Some((0, 2)), Some((1, 2))])),
        ("(a)*", "b", Some(&[Some((0, 0)), None])),
        ("(a?)*", "aa", Some(&[Some((0, 2)), Some((2, 2))])),
        ("(a*)*", "aa", Some(&[Some((0, 2)), Some((2, 2))])),
        ("(a*)+", "aa", Some(&[Some((0, 2)), Some((2, 2))])),
        ("(a*)+", "b", Some(&[Some((0, 0)), Some((0, 0))])),
        ("(a|)*", "a", Some(&[Some((0, 1)), Some((1, 1))])),
        ("(a*)*?", "aa", Some(&[Some((0, 0)), None])),
        ("(a?){2,}", "a", Some(&[Some((0, 1)), Some((1, 1))])),
        (
            "(?:(a)|(b))*",
            "ab",
            Some(&[Some((0, 2)), Some((0, 1)), Some((1, 2))]),
        ),
        (
            "((a|)*)*",
            "a",
            Some(&[Some((0, 1)), Some((1, 1)), Some((1, 1))]),
        ),
        ("((a)|)*a", "a", Some(&[Some((0, 1)), Some((0, 0)), None])),
        ("(a?){0,2}a", "a", Some(&[Some((0, 1)), Some((0, 0))])),
        ("^*", "abc", s0),
        ("^+a", "a", Some(&[Some((0, 1))])),
        ("^+a", "ba", None),
        // Leftmost wins over earlier alternative later in string
        ("b|a", "ab", Some(&[Some((0, 1))])),
        ("foo|bar", "xxbarfoo", Some(&[Some((2, 5))])),
    ];

    assert!(cases.len() >= 80, "need 80+ cases, got {}", cases.len());
    for (i, &(pat, text, expected)) in cases.iter().enumerate() {
        let re = Regex::new(pat).unwrap_or_else(|e| panic!("case {i} compile {pat:?}: {e:?}"));
        let got = re.find_at(text, 0);
        assert_eq!(
            got.as_deref(),
            expected,
            "case {i}: pat={pat:?} text={text:?}"
        );
        assert_eq!(re.is_match(text), expected.is_some(), "is_match case {i}");
    }
}

#[test]
fn regex_find_at_offset_and_find_iter() {
    let re_word = Regex::new(r"\b\w+").unwrap();
    assert_eq!(re_word.find_at("hello world", 1), Some(vec![Some((6, 11))]));
    assert_eq!(re_word.find_at("hello world", 6), Some(vec![Some((6, 11))]));
    assert_eq!(re_word.find_at("hello", 99), None);
    assert_eq!(re_word.find_at("é", 1), None);

    assert_eq!(
        Regex::new("a*").unwrap().find_iter("a"),
        vec![(0, 1), (1, 1)]
    );
    assert_eq!(
        Regex::new("a*").unwrap().find_iter("aba"),
        vec![(0, 1), (1, 1), (2, 3), (3, 3)]
    );
    assert_eq!(
        Regex::new(r"\b").unwrap().find_iter("ab"),
        vec![(0, 0), (2, 2)]
    );
    assert_eq!(
        Regex::new("$").unwrap().find_iter("a\n"),
        vec![(1, 1), (2, 2)]
    );
    assert_eq!(
        Regex::new("").unwrap().find_iter("éa"),
        vec![(0, 0), (2, 2), (3, 3)]
    );
    assert_eq!(
        Regex::new(r"\w+").unwrap().find_iter("café_1"),
        vec![(0, 3), (5, 7)]
    );
}

#[test]
fn regex_error_cases() {
    let bad_patterns = [
        "(",
        ")",
        "(a))",
        "((a)",
        "(?:a",
        "[abc",
        "[]",
        "[^]",
        "[z-a]",
        r"[\d-a]",
        r"[a-\w]",
        "*",
        "+",
        "?",
        "{2}",
        "a**",
        "a+*",
        "a?+",
        "a{1,2}*",
        "a*??",
        "a{5,2}",
        "a{1001}",
        "a{1,1001}",
        "a{1001,}",
        "a{,1001}",
        "a{99999999999999999999}",
        "|",
        "(*)",
        "a|*",
        r"\",
        r"\q",
        r"\1",
        r"[\B]",
        r"[\q]",
        "(?P<x>a)",
    ];
    for pat in bad_patterns {
        if pat == "|" {
            // Note: "|" is valid in Python re (empty | empty); verify it compiles.
            assert!(Regex::new(pat).is_ok());
            continue;
        }
        assert!(Regex::new(pat).is_err(), "expected error for {pat:?}");
    }
    // Boundary: 1000 is allowed, 1001 is rejected.
    assert!(Regex::new("a{1000}").is_ok());
    assert!(Regex::new("a{1001}").is_err());
}

#[test]
fn regex_pathological_linear_time() {
    let re = Regex::new("(a*)*b").unwrap();
    let input = "a".repeat(30_000);
    let t0 = Instant::now();
    assert_eq!(re.find_at(&input, 0), None);
    let elapsed = t0.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");

    let mut input_with_b = input;
    input_with_b.push('b');
    let t1 = Instant::now();
    let res = re.find_at(&input_with_b, 0);
    assert_eq!(res, Some(vec![Some((0, 30_001)), Some((30_000, 30_000))]));
    assert!(t1.elapsed().as_secs_f64() < 1.0);
}

#[test]
fn regex_random_no_panic_forall() {
    const PIECES: &[&str] = &[
        "a", "b", ".", "\\d", "\\w", "\\s", "\\b", "^", "$", "|", "*", "+", "?", "(", ")", "(?:",
        "[a-c]", "[^a]", "{0,2}", "\\q", "[", "]",
    ];
    forall(500, 0x5E6E_0003, |r| {
        let n = 1 + r.below(10);
        let mut pat = String::new();
        for _ in 0..n {
            pat.push_str(r.pick(PIECES));
        }
        if let Ok(re) = Regex::new(&pat) {
            let _ = re.find_at("ab12 cd\né", 0);
            let _ = re.find_iter("ab12 cd\n");
        }
    });
}
