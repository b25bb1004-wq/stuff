#![allow(clippy::unwrap_used)]
use crate::lexer::{lex, Tok};
use crate::span::Span;
use alloc::string::String;
use alloc::vec::Vec;

/// Token kinds of `src`, without the final Eof.
fn kinds(src: &str) -> Vec<Tok> {
    let mut v: Vec<Tok> = lex(src).unwrap().into_iter().map(|t| t.tok).collect();
    assert_eq!(v.pop(), Some(Tok::Eof));
    v
}

fn ident(s: &str) -> Tok {
    Tok::LIdent(String::from(s))
}

fn upper(s: &str) -> Tok {
    Tok::UIdent(String::from(s))
}

fn err(src: &str) -> (String, Span) {
    let d = lex(src).unwrap_err();
    (d.message, d.span)
}

#[test]
fn every_keyword_and_punctuation_kind() {
    let src = "fn type layer const let var if else match while for in return break continue \
               assert test prop where uses extern true false none some";
    let words: Vec<Tok> = vec![
        Tok::Fn,
        Tok::Type,
        Tok::Layer,
        Tok::Const,
        Tok::Let,
        Tok::Var,
        Tok::If,
        Tok::Else,
        Tok::Match,
        Tok::While,
        Tok::For,
        Tok::In,
        Tok::Return,
        Tok::Break,
        Tok::Continue,
        Tok::Assert,
        Tok::Test,
        Tok::Prop,
        Tok::Where,
        Tok::Uses,
        Tok::Extern,
        Tok::True,
        Tok::False,
        Tok::None,
        Tok::Some,
    ];
    assert_eq!(kinds(src), words);
    let punct = "+ - * / % +% -% *% == != < <= > >= && || ! & | ^ << >> = |> -> => . .. , : ; ? ( ) [ ] { }";
    let expect = vec![
        Tok::Plus,
        Tok::Minus,
        Tok::Star,
        Tok::Slash,
        Tok::Percent,
        Tok::PlusWrap,
        Tok::MinusWrap,
        Tok::StarWrap,
        Tok::EqEq,
        Tok::NotEq,
        Tok::Lt,
        Tok::Le,
        Tok::Gt,
        Tok::Ge,
        Tok::AndAnd,
        Tok::OrOr,
        Tok::Bang,
        Tok::Amp,
        Tok::Bar,
        Tok::Caret,
        Tok::Shl,
        Tok::Shr,
        Tok::Eq,
        Tok::PipeArrow,
        Tok::Arrow,
        Tok::FatArrow,
        Tok::Dot,
        Tok::DotDot,
        Tok::Comma,
        Tok::Colon,
        Tok::Semi,
        Tok::Question,
        Tok::LParen,
        Tok::RParen,
        Tok::LBracket,
        Tok::RBracket,
        Tok::LBrace,
        Tok::RBrace,
    ];
    assert_eq!(kinds(punct), expect);
}

#[test]
fn names_split_into_two_kinds() {
    assert_eq!(
        kinds("abc _x a1 Point Ab9"),
        vec![
            ident("abc"),
            ident("_x"),
            ident("a1"),
            upper("Point"),
            upper("Ab9"),
        ]
    );
    // Builtin type names are plain lowercase names to the lexer.
    assert_eq!(kinds("int bool"), vec![ident("int"), ident("bool")]);
    // An underscore ends an uppercase name.
    assert_eq!(kinds("Foo_bar"), vec![upper("Foo"), ident("_bar")]);
}

#[test]
fn integer_forms() {
    assert_eq!(kinds("0"), vec![Tok::Int(0)]);
    assert_eq!(kinds("1_000"), vec![Tok::Int(1000)]);
    assert_eq!(kinds("0xff"), vec![Tok::Int(255)]);
    assert_eq!(kinds("0xFF_FF"), vec![Tok::Int(0xffff)]);
    assert_eq!(kinds("0b1010"), vec![Tok::Int(10)]);
    assert_eq!(
        kinds("9223372036854775807"),
        vec![Tok::Int(i64::MAX as u64)]
    );
    assert_eq!(kinds("0x7fffffffffffffff"), vec![Tok::Int(i64::MAX as u64)]);
    // The one literal above i64::MAX that the lexer lets through (the parser checks it).
    assert_eq!(kinds("9223372036854775808"), vec![Tok::Int(1u64 << 63)]);
}

#[test]
fn integer_overflow_is_a_lex_error() {
    let (msg, span) = err("x = 9223372036854775809");
    assert_eq!(msg, "integer literal does not fit in int (i64)");
    assert_eq!(span, Span { start: 4, end: 23 });
    let (msg, _) = err("0x8000000000000000");
    assert_eq!(msg, "integer literal does not fit in int (i64)");
    let (msg, _) = err("0xffffffffffffffffffffffffffffffffff");
    assert_eq!(msg, "integer literal does not fit in int (i64)");
}

#[test]
fn malformed_numbers() {
    assert_eq!(err("0x").0, "expected digits after the number prefix");
    assert_eq!(err("0b102").0, "invalid digit in number literal");
    assert_eq!(err("12abc").0, "invalid digit in number literal");
}

#[test]
fn byte_literals_and_escapes() {
    assert_eq!(kinds("b'a'"), vec![Tok::Byte(b'a')]);
    assert_eq!(
        kinds("b'\\n' b'\\t' b'\\r' b'\\0' b'\\\\' b'\\'' b'\\x7f' b'\\xff'"),
        vec![
            Tok::Byte(10),
            Tok::Byte(9),
            Tok::Byte(13),
            Tok::Byte(0),
            Tok::Byte(92),
            Tok::Byte(39),
            Tok::Byte(0x7f),
            Tok::Byte(0xff),
        ]
    );
    // `b` alone is still a name.
    assert_eq!(kinds("b"), vec![ident("b")]);
    assert_eq!(err("b'ab'").0, "expected ' to close the byte literal");
    assert_eq!(err("b''").0, "empty byte literal");
    assert_eq!(err("b'\\u{41}'").0, "unknown escape sequence '\\u'");
    assert_eq!(err("b'\\q'").0, "unknown escape sequence '\\q'");
    assert_eq!(err("b'\\x4'").0, "expected hex digits in escape sequence");
}

#[test]
fn string_escapes() {
    let s = |t: &str| match kinds(t).as_slice() {
        [Tok::Str(s)] => s.clone(),
        other => panic!("not one string: {:?}", other),
    };
    assert_eq!(s("\"a\\nb\\tc\\rd\\0e\\\\f\\\"g\""), "a\nb\tc\rd\0e\\f\"g");
    assert_eq!(s("\"\\x41\\x7f\""), "A\u{7f}");
    assert_eq!(s("\"\\u{1F600}\\u{41}\""), "\u{1F600}A");
    assert_eq!(s("\"héllo\""), "héllo");
    assert_eq!(
        err("\"\\xff\"").0,
        "\\x escape in a string must be at most \\x7f"
    );
    assert_eq!(err("\"\\'\"").0, "unknown escape sequence '\\''");
    assert_eq!(err("\"\\u{}\"").0, "expected hex digits in escape sequence");
    assert_eq!(err("\"\\u{110000}\"").0, "invalid unicode escape");
    assert_eq!(err("\"\\u41\"").0, "expected '{' after \\u");
    assert_eq!(err("\"\\u{41\"").0, "expected '}' to close \\u{...}");
}

#[test]
fn unterminated_and_multiline_strings() {
    assert_eq!(err("\"abc").0, "unterminated string literal");
    let (msg, span) = err("\"ab\ncd\"");
    assert_eq!(msg, "string literal cannot span lines");
    assert_eq!(span, Span { start: 0, end: 3 });
}

#[test]
fn comments_run_to_the_end_of_the_line() {
    assert_eq!(
        kinds("a // b c\nd"),
        vec![ident("a"), Tok::Newline, ident("d")]
    );
    assert_eq!(kinds("// only"), vec![]);
    // A comment is not an operator: a newline after the code before it still counts.
    assert_eq!(
        kinds("a + // x\nb"),
        vec![ident("a"), Tok::Plus, ident("b")]
    );
}

#[test]
fn newline_rule_a_operator_at_line_end_suppresses() {
    for op in [
        "+", "-", "=", ",", ":", "->", "=>", "(", "[", "{", "|>", "&&", "||", "..", ".",
    ] {
        let src = format!("a {}\nb", op);
        let got = kinds(&src);
        assert!(!got.contains(&Tok::Newline), "no Newline after {:?}", op);
    }
}

#[test]
fn newline_rule_a_negative_plain_end_keeps_newline() {
    assert_eq!(kinds("a\nb"), vec![ident("a"), Tok::Newline, ident("b")]);
    // A type ending in `?` is not an operator, so the statement still ends.
    assert_eq!(
        kinds("x: int?\ny"),
        vec![
            ident("x"),
            Tok::Colon,
            ident("int"),
            Tok::Question,
            Tok::Newline,
            ident("y"),
        ]
    );
    // A closing bracket at line end ends the statement.
    assert_eq!(
        kinds("f()\nb"),
        vec![
            ident("f"),
            Tok::LParen,
            Tok::RParen,
            Tok::Newline,
            ident("b"),
        ]
    );
}

#[test]
fn newline_rule_b_continuation_tokens_at_line_start_suppress() {
    assert_eq!(
        kinds("a\n|> f()"),
        vec![
            ident("a"),
            Tok::PipeArrow,
            ident("f"),
            Tok::LParen,
            Tok::RParen,
        ]
    );
    assert_eq!(kinds("a\n.b"), vec![ident("a"), Tok::Dot, ident("b")]);
    assert_eq!(kinds("a\n&& b"), vec![ident("a"), Tok::AndAnd, ident("b")]);
    assert_eq!(kinds("a\n|| b"), vec![ident("a"), Tok::OrOr, ident("b")]);
    assert_eq!(
        kinds("}\nelse {"),
        vec![Tok::RBrace, Tok::Else, Tok::LBrace]
    );
    assert_eq!(kinds("a\n)"), vec![ident("a"), Tok::RParen]);
    assert_eq!(kinds("a\n]"), vec![ident("a"), Tok::RBracket]);
    assert_eq!(kinds("a\n}"), vec![ident("a"), Tok::RBrace]);
}

#[test]
fn newline_rule_b_negative_other_tokens_keep_newline() {
    assert_eq!(
        kinds("a\n|x"),
        vec![ident("a"), Tok::Newline, Tok::Bar, ident("x")]
    );
    assert_eq!(
        kinds("a\n..b"),
        vec![ident("a"), Tok::Newline, Tok::DotDot, ident("b")]
    );
    assert_eq!(
        kinds("a\n+b"),
        vec![ident("a"), Tok::Newline, Tok::Plus, ident("b")]
    );
}

#[test]
fn newline_rule_c_inside_parens_and_brackets_suppresses() {
    assert_eq!(
        kinds("f(a\nb)"),
        vec![ident("f"), Tok::LParen, ident("a"), ident("b"), Tok::RParen,]
    );
    assert_eq!(
        kinds("[1\n2]"),
        vec![Tok::LBracket, Tok::Int(1), Tok::Int(2), Tok::RBracket]
    );
    // Inside braces that sit inside parens the line break is kept again.
    assert_eq!(
        kinds("f({\na\nb\n})"),
        vec![
            ident("f"),
            Tok::LParen,
            Tok::LBrace,
            ident("a"),
            Tok::Newline,
            ident("b"),
            Tok::RBrace,
            Tok::RParen,
        ]
    );
}

#[test]
fn several_terminators_count_as_one() {
    assert_eq!(
        kinds("a\n\n\nb"),
        vec![ident("a"), Tok::Newline, ident("b")]
    );
    assert_eq!(kinds("a;b"), vec![ident("a"), Tok::Semi, ident("b")]);
    assert_eq!(kinds("a;\n;\nb"), vec![ident("a"), Tok::Semi, ident("b")]);
    assert_eq!(kinds("\n\na"), vec![ident("a")]);
}

#[test]
fn error_spans_point_at_the_bad_character() {
    let (msg, span) = err("let x = 1 # 2");
    assert_eq!(msg, "unexpected character '#'");
    assert_eq!(span, Span { start: 10, end: 11 });
    // A non-ASCII character outside a string is reported at its own bytes.
    let (msg, span) = err("a é");
    assert_eq!(msg, "unexpected character 'é'");
    assert_eq!(span, Span { start: 2, end: 4 });
}

#[test]
fn spans_cover_the_token_text() {
    let toks = lex("ab  12\n\"x\"").unwrap();
    assert_eq!(toks[0].span, Span { start: 0, end: 2 });
    assert_eq!(toks[1].span, Span { start: 4, end: 6 });
    assert_eq!(toks[2].span, Span { start: 6, end: 7 });
    assert_eq!(toks[3].span, Span { start: 7, end: 10 });
    let eof = toks.last().unwrap();
    assert_eq!(eof.tok, Tok::Eof);
    assert_eq!(eof.span, Span { start: 10, end: 10 });
}

#[test]
fn ambiguous_operator_pairs_take_the_longest_match() {
    assert_eq!(kinds("a-%b"), vec![ident("a"), Tok::MinusWrap, ident("b")]);
    assert_eq!(kinds("a->b"), vec![ident("a"), Tok::Arrow, ident("b")]);
    assert_eq!(
        kinds("a..=b"),
        vec![ident("a"), Tok::DotDot, Tok::Eq, ident("b")]
    );
    assert_eq!(kinds("a|>b"), vec![ident("a"), Tok::PipeArrow, ident("b")]);
    assert_eq!(kinds("a|b"), vec![ident("a"), Tok::Bar, ident("b")]);
    assert_eq!(kinds("x.0"), vec![ident("x"), Tok::Dot, Tok::Int(0)]);
}
