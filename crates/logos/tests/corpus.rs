//! Corpus round-trip and fuzz tests for the Logos front end.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use logos::lexer::lex;
use logos::parser::parse;
use logos::print::print;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

/// Debug text of a tree with every span removed, for comparing trees structurally.
fn no_span<T: Debug>(value: &T) -> String {
    let text = format!("{:?}", value);
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("Span { start:") {
        out.push_str(&rest[..i]);
        out.push_str("Span");
        match rest[i..].find('}') {
            Some(j) => rest = &rest[i + j + 1..],
            None => rest = "",
        }
    }
    out.push_str(rest);
    out
}

fn load_corpus() -> Vec<(String, String)> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("tests");
    dir.push("corpus");
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
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
fn corpus_round_trips() {
    let corpus = load_corpus();
    assert!(
        corpus.len() >= 20,
        "expected at least 20 corpus files, found {}",
        corpus.len()
    );
    for (name, src) in &corpus {
        let m1 = parse(src).unwrap_or_else(|d| panic!("corpus {} failed: {:?}", name, d));
        let p1 = print(&m1);
        let m2 = parse(&p1).unwrap_or_else(|d| {
            panic!(
                "corpus {} reprinted text failed to parse: {:?}\n{}",
                name, d, p1
            )
        });
        let p2 = print(&m2);
        assert_eq!(p2, p1, "print is not idempotent for {}", name);
        assert_eq!(no_span(&m2), no_span(&m1), "tree changed for {}", name);
    }
}

#[test]
fn fuzz_random_bytes_never_panics() {
    forall(2000, 0x1111_2222_3333_4444, |rng| {
        let bytes = rng.bytes(256);
        let src = String::from_utf8_lossy(&bytes);
        let _ = lex(&src);
        if let Ok(m) = parse(&src) {
            let p1 = print(&m);
            if let Ok(m2) = parse(&p1) {
                assert_eq!(print(&m2), p1);
            }
        }
    });
}

const TOKENS: &[&str] = &[
    "fn",
    "type",
    "layer",
    "const",
    "let",
    "var",
    "if",
    "else",
    "match",
    "while",
    "for",
    "in",
    "return",
    "break",
    "continue",
    "assert",
    "test",
    "prop",
    "where",
    "uses",
    "extern",
    "true",
    "false",
    "none",
    "some",
    "int",
    "bool",
    "byte",
    "str",
    "unit",
    "x",
    "y",
    "f",
    "_",
    "Point",
    "Shape",
    "Circle",
    "0",
    "1",
    "42",
    "9223372036854775808",
    "b'a'",
    "\"s\"",
    "+",
    "-",
    "*",
    "/",
    "%",
    "+%",
    "-%",
    "*%",
    "==",
    "!=",
    "<",
    "<=",
    ">",
    ">=",
    "&&",
    "||",
    "!",
    "&",
    "|",
    "^",
    "<<",
    ">>",
    "=",
    "|>",
    "->",
    "=>",
    ".",
    "..",
    ",",
    ":",
    ";",
    "?",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    "\n",
];

#[test]
fn fuzz_random_valid_tokens_never_panics() {
    forall(2000, 0x5555_6666_7777_8888, |rng| {
        let count = rng.range(0, 64) as usize;
        let mut src = String::new();
        for i in 0..count {
            if i > 0 {
                src.push(' ');
            }
            src.push_str(rng.pick(TOKENS));
        }
        let _ = lex(&src);
        if let Ok(m) = parse(&src) {
            let p1 = print(&m);
            let m2 = parse(&p1).unwrap_or_else(|d| {
                panic!(
                    "reprint failed to parse: {:?}\nsrc: {:?}\np1: {:?}",
                    d, src, p1
                )
            });
            assert_eq!(print(&m2), p1);
            assert_eq!(no_span(&m2), no_span(&m));
        }
    });
}

fn random_single_char_edit(src: &str, rng: &mut Rng) -> String {
    let mut chars: Vec<char> = src.chars().collect();
    let choices = [
        ' ', '\n', ';', '(', ')', '[', ']', '{', '}', '"', '\'', '\\', '#', '.', ',', ':', '=',
        '+', '-', '*', '/', '<', '>', '!', '&', '|', '^', '?', '0', '9', 'a', 'A', '_',
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
fn fuzz_corpus_single_char_edits_never_panics() {
    let corpus = load_corpus();
    forall(2000, 0x9999_aaaa_bbbb_cccc, |rng| {
        let (_, base) = rng.pick(&corpus);
        let edited = random_single_char_edit(base, rng);
        let _ = lex(&edited);
        if let Ok(m) = parse(&edited) {
            let p1 = print(&m);
            let m2 = parse(&p1).unwrap_or_else(|d| {
                panic!(
                    "reprint failed to parse after edit: {:?}\nedited: {:?}\np1: {:?}",
                    d, edited, p1
                )
            });
            assert_eq!(print(&m2), p1);
            assert_eq!(no_span(&m2), no_span(&m));
        }
    });
}
