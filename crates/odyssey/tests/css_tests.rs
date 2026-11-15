//! Integration tests for CSS tokenization, values, declarations, and stylesheets.
#![allow(clippy::unwrap_used)]

use odyssey::css::{
    parse_declarations, parse_stylesheet, parse_values, tokenize, Declaration, Rule, Token, Value,
};

#[test]
fn css_tokenizer_table() {
    assert_eq!(
        tokenize("/* c */ a\r\n\x0C\0b"),
        vec![
            Token::Whitespace,
            Token::Ident("a".into()),
            Token::Whitespace,
            Token::Ident("\u{FFFD}b".into()),
        ]
    );
    assert_eq!(
        tokenize("#foo #1a @media <!-- -->"),
        vec![
            Token::Hash {
                value: "foo".into(),
                id: true
            },
            Token::Whitespace,
            Token::Hash {
                value: "1a".into(),
                id: false
            },
            Token::Whitespace,
            Token::AtKeyword("media".into()),
            Token::Whitespace,
            Token::Cdo,
            Token::Whitespace,
            Token::Cdc,
        ]
    );
    assert_eq!(
        tokenize("url(https://a.b/c) url(bad url) url(\"quoted\")"),
        vec![
            Token::Url("https://a.b/c".into()),
            Token::Whitespace,
            Token::BadUrl,
            Token::Whitespace,
            Token::Function("url".into()),
            Token::Str("quoted".into()),
            Token::CloseParen,
        ]
    );
    assert_eq!(
        tokenize("\"ok\" \"bad\n\"eof"),
        vec![
            Token::Str("ok".into()),
            Token::Whitespace,
            Token::BadStr,
            Token::Whitespace,
            Token::Str("eof".into()),
        ]
    );

    let nums = tokenize("-0 1.5 1e400 10px 50%");
    match &nums[0] {
        Token::Number { value, int: true } => assert!(value.is_sign_negative() && *value == 0.0),
        other => panic!("expected -0 number, got {other:?}"),
    }
    assert_eq!(
        nums[2],
        Token::Number {
            value: 1.5,
            int: false
        }
    );
    assert_eq!(
        nums[4],
        Token::Number {
            value: f64::INFINITY,
            int: false
        }
    );
    assert_eq!(
        nums[6],
        Token::Dimension {
            value: 10.0,
            int: true,
            unit: "px".into()
        }
    );
    assert_eq!(nums[8], Token::Percentage(50.0));
}

#[test]
fn css_values_and_declarations() {
    assert_eq!(
        parse_values("f(1, [2]) ) ] }"),
        vec![
            Value::Function {
                name: "f".into(),
                args: vec![
                    Value::Token(Token::Number {
                        value: 1.0,
                        int: true
                    }),
                    Value::Token(Token::Comma),
                    Value::Token(Token::Whitespace),
                    Value::Block {
                        open: '[',
                        items: vec![Value::Token(Token::Number {
                            value: 2.0,
                            int: true
                        })],
                    },
                ],
            },
            Value::Token(Token::Whitespace),
            Value::Token(Token::CloseParen),
            Value::Token(Token::Whitespace),
            Value::Token(Token::CloseSquare),
            Value::Token(Token::Whitespace),
            Value::Token(Token::CloseCurly),
        ]
    );

    let decls = parse_declarations(
        "Color: red !IMPORTANT; @bad 1; a: {x} y; b: x {y}; c: {x} {y}; d: x!!important; e: x ! ! important; --foo: bar",
    );
    assert_eq!(
        decls,
        vec![
            Declaration {
                name: "Color".into(),
                value: vec![Value::Token(Token::Ident("red".into()))],
                important: true,
            },
            Declaration {
                name: "a".into(),
                value: vec![
                    Value::Block {
                        open: '{',
                        items: vec![Value::Token(Token::Ident("x".into()))],
                    },
                    Value::Token(Token::Whitespace),
                    Value::Token(Token::Ident("y".into())),
                ],
                important: false,
            },
            Declaration {
                name: "d".into(),
                value: vec![
                    Value::Token(Token::Ident("x".into())),
                    Value::Token(Token::Delim('!')),
                    Value::Token(Token::Delim('!')),
                    Value::Token(Token::Ident("important".into())),
                ],
                important: false,
            },
            Declaration {
                name: "e".into(),
                value: vec![
                    Value::Token(Token::Ident("x".into())),
                    Value::Token(Token::Whitespace),
                    Value::Token(Token::Delim('!')),
                    Value::Token(Token::Whitespace),
                    Value::Token(Token::Delim('!')),
                    Value::Token(Token::Whitespace),
                    Value::Token(Token::Ident("important".into())),
                ],
                important: false,
            },
            Declaration {
                name: "--foo".into(),
                value: vec![Value::Token(Token::Ident("bar".into()))],
                important: false,
            },
        ]
    );
}

#[test]
fn css_stylesheet_rules() {
    let rules = parse_stylesheet(
        "<!-- @import 'a.css'; @media screen { p { color: red } } div > p { @nest; margin: 0; } dropped",
    );
    assert_eq!(rules.len(), 3);
    assert!(matches!(&rules[0], Rule::At { name, block: None, .. } if name == "import"));
    assert!(matches!(&rules[1], Rule::At { name, block: Some(_), .. } if name == "media"));
    match &rules[2] {
        Rule::Style { declarations, .. } => {
            assert_eq!(declarations.len(), 1);
            assert_eq!(declarations[0].name, "margin");
        }
        other => panic!("expected style rule, got {other:?}"),
    }
}
