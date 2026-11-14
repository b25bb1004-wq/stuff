#![allow(clippy::unwrap_used)]
use super::parse;
use crate::ast::{Expr, Item, Module, Stmt, TypeBody};
use crate::no_span;
use crate::print::print;
use alloc::format;
use alloc::string::String;

fn module(src: &str) -> Module {
    parse(src).unwrap_or_else(|d| panic!("{:?} in {:?}", d, src))
}

fn printed(src: &str) -> String {
    print(&module(src))
}

/// Canonical text of one expression statement, written as the body of `fn f()`.
fn expr_text(src: &str) -> String {
    let p = printed(&format!("fn f() {{\n{}\n}}\n", src));
    p.lines().nth(1).unwrap().trim().into()
}

/// First statement of `fn f()`'s body.
fn first_stmt(src: &str) -> Stmt {
    let m = module(&format!("fn f() {{\n{}\n}}\n", src));
    match &m.items[0] {
        Item::Fn(f) => match &f.body {
            crate::ast::FnBody::Block(b) => b.stmts[0].clone(),
            _ => panic!("extern body"),
        },
        _ => panic!("not a fn"),
    }
}

fn roundtrip(src: &str) {
    let m = module(src);
    let p1 = print(&m);
    let m2 = parse(&p1).unwrap_or_else(|d| panic!("reprint does not parse: {:?}\n{}", d, p1));
    assert_eq!(print(&m2), p1, "print is not stable for {:?}", src);
    assert_eq!(no_span(&m2), no_span(&m), "tree changed for {:?}", src);
}

/// Asserts the message, and that the span starts at the `nth` occurrence of `snippet`.
fn expect_err(src: &str, msg: &str, snippet: &str, nth: usize) {
    let d = match parse(src) {
        Ok(_) => panic!("expected an error for {:?}", src),
        Err(d) => d,
    };
    assert_eq!(d.message, msg, "message for {:?}", src);
    let at = src.match_indices(snippet).nth(nth).map(|(i, _)| i).unwrap();
    assert_eq!(d.span.start as usize, at, "span start for {:?}", src);
    assert_eq!(
        d.span.end as usize,
        at + snippet.len(),
        "span end for {:?}",
        src
    );
}

const EXAMPLE: &str = "type Shape = Circle(int) | Rect(int, int)

fn area(s: Shape) -> int {
    match s {
        Circle(r) => 3 * r * r,
        Rect(w, h) => w * h,
    }
}

fn total(shapes: [Shape]) -> int {
    var sum = 0
    for s in shapes { sum = sum + area(s) }
    sum
}

fn main() uses io {
    [Circle(2), Rect(3, 4)] |> total() |> str() |> print()
}

test \"a rectangle\" { assert area(Rect(3, 4)) == 12 }

prop \"area is never negative\" (w: int, h: int) where w >= 0 && w < 1000 && h >= 0 && h < 1000 {
    assert area(Rect(w, h)) >= 0
}
";

#[test]
fn spec_example_parses_and_round_trips() {
    let m = module(EXAMPLE);
    assert_eq!(m.items.len(), 6);
    roundtrip(EXAMPLE);
}

#[test]
fn empty_and_blank_files() {
    assert!(module("").items.is_empty());
    assert!(module("\n\n  // nothing\n").items.is_empty());
    assert_eq!(printed(""), "");
}

#[test]
fn type_declarations() {
    let m = module("type P = { x: int, y: [P]?, }\ntype E = A | B(int, str) | C\n");
    match &m.items[0] {
        Item::Type(t) => match &t.body {
            TypeBody::Record(fs) => assert_eq!(fs.len(), 2),
            _ => panic!("record expected"),
        },
        _ => panic!(),
    }
    match &m.items[1] {
        Item::Type(t) => match &t.body {
            TypeBody::Variants(vs) => {
                assert_eq!(vs.len(), 3);
                assert_eq!(vs[1].fields.len(), 2);
                assert!(vs[2].fields.is_empty());
            }
            _ => panic!("variants expected"),
        },
        _ => panic!(),
    }
    roundtrip("type P = {\n  x: int,\n  y: int\n}\n");
}

#[test]
fn variants_may_break_after_the_equals_sign() {
    roundtrip("type E =\n  A |\n  B\n");
}

#[test]
fn const_layer_and_functions() {
    roundtrip("const limit: int = 2 * 3 + 1\nlayer net\n");
    roundtrip("fn f(a: int, b: [byte]) -> int? uses io, net {\n  return none\n}\n");
    roundtrip("fn g() = extern \"io.print\"\n");
    roundtrip("fn h(f: fn(int) -> int uses io, x: int) {\n}\n");
    roundtrip("fn k(f: fn() -> int uses io) uses net {\n}\n");
    roundtrip("fn unit() -> unit {\n}\n");
}

#[test]
fn every_type_form() {
    roundtrip("fn t(a: int, b: bool, c: byte, d: str, e: unit, f: Foo, g: [int], h: (int, str), i: int?, j: fn(int, [byte]) -> bool) {\n}\n");
}

#[test]
fn test_and_prop_items() {
    roundtrip("test \"t\" {\n  assert true\n}\n");
    roundtrip("prop \"p\" (x: int) where x > 0 {\n  assert x > 0, \"positive\"\n}\n");
    roundtrip("prop \"q\" (x: int, y: bool) {\n}\n");
}

#[test]
fn every_statement_form() {
    roundtrip(
        "fn s() {\n  let a = 1\n  let b: int = a\n  var c = [1; 3]\n  c[0] = 2\n  \
         while c[0] < 9 {\n    c[0] = c[0] + 1\n    continue\n  }\n  \
         for x in 0..10 {\n    break\n  }\n  for y in c {\n  }\n  \
         assert a == 1, \"one\"\n  return a\n}\n",
    );
    assert!(matches!(
        first_stmt("let a = 1"),
        Stmt::Let { mutable: false, .. }
    ));
    assert!(matches!(
        first_stmt("var a = 1"),
        Stmt::Let { mutable: true, .. }
    ));
    assert!(matches!(
        first_stmt("return"),
        Stmt::Return { value: None, .. }
    ));
    assert!(matches!(first_stmt("break"), Stmt::Break { .. }));
    assert!(matches!(first_stmt("continue"), Stmt::Continue { .. }));
    assert!(matches!(first_stmt("a.b = 1"), Stmt::Assign { .. }));
    assert!(matches!(first_stmt("f(1)"), Stmt::Expr { .. }));
}

#[test]
fn every_expression_form() {
    let cases = [
        "1_000",
        "0xff",
        "0b11",
        "b'\\n'",
        "\"s\\t\\u{1F600}\"",
        "true",
        "false",
        "none",
        "some(1)",
        "x",
        "Circle(1, 2)",
        "P { x: 1, y: 2 }",
        "[]",
        "[1, 2,]",
        "[0; 9]",
        "(1, 2)",
        "()",
        "(1 + 2)",
        "-x",
        "!b",
        "f(1)(2)",
        "a[0]",
        "a.field",
        "t.0",
        "if a { 1 } else if b { 2 } else { 3 }",
        "match v { A => 1, B(x) => x, _ => 0, }",
        "{ let q = 1\n q }",
        "x |> f(1) |> g()",
        "a[1..2]",
    ];
    for c in cases {
        roundtrip(&format!("fn f() {{\n    let v = {}\n}}\n", c));
    }
}

#[test]
fn every_pattern_form() {
    roundtrip(
        "fn p(v: int) {\n  match v {\n    _ => 0,\n    n => n,\n    1 => 1,\n    b'a' => 2,\n    \"s\" => 3,\n    true => 4,\n    false => 5,\n    none => 6,\n    some(x) => x,\n    Leaf => 7,\n    Node(l, r) => 8,\n    (a, b) => 9,\n  }\n}\n",
    );
}

#[test]
fn record_heads_need_parentheses() {
    // Bare records are refused in the head of if, while, for and match.
    assert!(parse("fn f() { if P { x: 1 } { } }").is_err());
    assert!(parse("fn f() { while P { x: 1 } { } }").is_err());
    assert!(parse("fn f() { for i in P { x: 1 } { } }").is_err());
    assert!(parse("fn f() { match P { x: 1 } { _ => 1 } }").is_err());
    // Parenthesised, they are fine, and the printer adds the parentheses itself.
    roundtrip("fn f() {\n  if (P { x: 1 }) {\n  }\n  match (P { x: 1 }) {\n    _ => 1,\n  }\n}\n");
    assert_eq!(
        expr_text("if (P { x: 1 } == v) { 1 } else { 2 }"),
        "if (P { x: 1 } == v) {"
    );
    // A variant without arguments in a head is not a record, so no parentheses are needed.
    assert_eq!(expr_text("if Empty { 1 } else { 2 }"), "if Empty {");
}

#[test]
fn precedence_of_adjacent_levels() {
    assert_eq!(expr_text("1 + 2 * 3"), "1 + 2 * 3");
    assert_eq!(expr_text("(1 + 2) * 3"), "(1 + 2) * 3");
    assert_eq!(expr_text("1 * 2 - 3"), "1 * 2 - 3");
    assert_eq!(expr_text("1 - (2 - 3)"), "1 - (2 - 3)");
    assert_eq!(expr_text("a || b && c"), "a || b && c");
    assert_eq!(expr_text("(a || b) && c"), "(a || b) && c");
    assert_eq!(expr_text("a && b == c"), "a && b == c");
    assert_eq!(expr_text("(a && b) == c"), "(a && b) == c");
    assert_eq!(expr_text("a == b .. c"), "a == b .. c");
    assert_eq!(expr_text("a .. b | c"), "a .. b | c");
    assert_eq!(expr_text("(a .. b) | c"), "(a .. b) | c");
    assert_eq!(expr_text("a | b ^ c"), "a | b ^ c");
    assert_eq!(expr_text("(a | b) ^ c"), "(a | b) ^ c");
    assert_eq!(expr_text("a ^ b & c"), "a ^ b & c");
    assert_eq!(expr_text("(a ^ b) & c"), "(a ^ b) & c");
    assert_eq!(expr_text("a & b << c"), "a & b << c");
    assert_eq!(expr_text("(a & b) << c"), "(a & b) << c");
    assert_eq!(expr_text("a << b + c"), "a << b + c");
    assert_eq!(expr_text("(a << b) + c"), "(a << b) + c");
    assert_eq!(expr_text("a +% b *% c"), "a +% b *% c");
    assert_eq!(expr_text("-a * b"), "-a * b");
    assert_eq!(expr_text("-(a * b)"), "-(a * b)");
    assert_eq!(expr_text("!a == b"), "!a == b");
    assert_eq!(expr_text("-a.b(1)[2]"), "-a.b(1)[2]");
    assert_eq!(expr_text("x |> f(a) |> g()"), "x |> f(a) |> g()");
    assert_eq!(expr_text("a + 1 |> f()"), "a + 1 |> f()");
    assert_eq!(expr_text("(a |> f()) + 1"), "(a |> f()) + 1");
}

#[test]
fn pipe_is_kept_as_its_own_node() {
    match first_stmt("x |> f(1)") {
        Stmt::Expr {
            expr: Expr::Pipe { .. },
            ..
        } => {}
        other => panic!("expected a Pipe node, got {:?}", other),
    }
}

#[test]
fn comparisons_do_not_chain() {
    expect_err(
        "fn f() {\n  let x = 1 < 2 < 3\n}\n",
        "expected newline or '}' after the statement, found '<'",
        "<",
        1,
    );
}

#[test]
fn minimum_int_literal_only_after_unary_minus() {
    assert_eq!(expr_text("-9223372036854775808"), "-9223372036854775808");
    assert_eq!(expr_text("- -9223372036854775808"), "--9223372036854775808");
    assert_eq!(
        expr_text("(-9223372036854775808).0"),
        "(-9223372036854775808).0"
    );
    assert_eq!(expr_text("9223372036854775807"), "9223372036854775807");
    roundtrip("fn f() {\n  let m = -9223372036854775808\n  let n = a - -9223372036854775808\n}\n");
    expect_err(
        "fn f() {\n  let x = 9223372036854775808\n}\n",
        "integer literal 9223372036854775808 does not fit in int (i64)",
        "9223372036854775808",
        0,
    );
    expect_err(
        "fn f() { let x = a - 9223372036854775808 }",
        "integer literal 9223372036854775808 does not fit in int (i64)",
        "9223372036854775808",
        0,
    );
}

#[test]
fn newline_rules_end_statements_and_continue_lines() {
    let src = "fn f() {\n  let x = a +\n    b\n  let y = c\n    |> g()\n  let z = d\n    .e\n}\n";
    let m = module(src);
    assert_eq!(m.items.len(), 1);
    roundtrip(src);
}

#[test]
fn semicolons_end_statements() {
    roundtrip("fn f() { let a = 1; let b = 2; a }\n");
    roundtrip("const a: int = 1; const b: int = 2\n");
}

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    let parens = format!(
        "fn f() {{ let x = {}1{} }}",
        "(".repeat(10_000),
        ")".repeat(10_000)
    );
    let d = parse(&parens).unwrap_err();
    assert_eq!(d.message, "nesting too deep (limit is 200 levels)");

    let blocks = format!("fn f() {}{}", "{".repeat(10_000), "}".repeat(10_000));
    let d = parse(&blocks).unwrap_err();
    assert_eq!(d.message, "nesting too deep (limit is 200 levels)");

    let negs = format!("fn f() {{ let x = {}1 }}", "-".repeat(10_000));
    assert!(parse(&negs).is_err());

    let types = format!(
        "fn f(x: {}int{}) {{\n}}\n",
        "[".repeat(10_000),
        "]".repeat(10_000)
    );
    assert!(parse(&types).is_err());

    let pats = format!(
        "fn f() {{ match v {{ {}_{} => 1 }} }}",
        "some(".repeat(10_000),
        ")".repeat(10_000)
    );
    assert!(parse(&pats).is_err());
}

#[test]
fn moderate_nesting_is_accepted() {
    let parens = format!(
        "fn f() {{ let x = {}1{} }}",
        "(".repeat(150),
        ")".repeat(150)
    );
    assert!(parse(&parens).is_ok());
    let blocks = format!("fn f() {}{}", "{".repeat(190), "}".repeat(190));
    assert!(parse(&blocks).is_ok());
}

#[test]
fn error_messages_and_spans() {
    expect_err("fn f( {\n}\n", "expected parameter name, found '{'", "{", 0);
    expect_err(
        "fn f() {\n  let = 1\n}\n",
        "expected variable name after 'let' or 'var', found '='",
        "=",
        0,
    );
    expect_err(
        "fn f() {\n  g(1\n}\n",
        "expected ',' or ')' to close the call, found '}'",
        "}",
        0,
    );
    expect_err(
        "fn f() { let a = 1 let b = 2 }",
        "expected newline or '}' after the statement, found 'let'",
        "let",
        1,
    );
    expect_err("fn f() -> {\n}\n", "expected type, found '{'", "{", 0);
    expect_err(
        "fn f() { let s = \"\\q\" }",
        "unknown escape sequence '\\q'",
        "\\q",
        0,
    );
    expect_err(
        "fn f() {\n  let x = 9223372036854775808\n}\n",
        "integer literal 9223372036854775808 does not fit in int (i64)",
        "9223372036854775808",
        0,
    );
    expect_err(
        "fn f() { 1 = 2 }",
        "left side of '=' must be a variable, a field or an index",
        "1",
        0,
    );
    expect_err(
        "fn f() { let x = _ }",
        "'_' is only allowed in patterns",
        "_",
        0,
    );
    expect_err(
        "fn f() { let x = P { }\n}\n",
        "expected field name, found '}'",
        "}",
        0,
    );
    expect_err(
        "fn f() { let x = some 1 }",
        "expected '(' after 'some', found integer 1",
        "1",
        0,
    );
    expect_err(
        "fn f() {\n  match t {\n    (a) => 1\n  }\n}\n",
        "expected ',' in the tuple pattern, found ')'",
        ")",
        1,
    );
    expect_err(
        "fn f() uses {\n}\n",
        "expected layer name after 'uses', found '{'",
        "{",
        0,
    );
    expect_err(
        "fn f() = extern io\n",
        "expected the extern import name, found name 'io'",
        "io",
        0,
    );
    expect_err(
        "let x = 1\n",
        "expected 'type', 'const', 'layer', 'fn', 'test' or 'prop', found 'let'",
        "let",
        0,
    );
    expect_err(
        "layer a layer b\n",
        "expected newline after the item, found 'layer'",
        "layer",
        1,
    );
    expect_err(
        "fn f() { x.4294967296 }",
        "tuple index is out of range",
        "4294967296",
        0,
    );
    expect_err(
        "fn f() {\n  assert\n  let x = 1\n}\n",
        "expected expression, found newline",
        "\n",
        1,
    );
    expect_err(
        "fn f() { match x {} }",
        "expected pattern, found '}'",
        "}",
        0,
    );
    expect_err(
        "fn f() { let p = P { x: 1 y: 2 } }",
        "expected ',' or '}' to close the record, found name 'y'",
        "y",
        0,
    );
    expect_err(
        "fn f() { (1 2) }",
        "expected ',' or ')' to close the group, found integer 2",
        "2",
        0,
    );
    expect_err(
        "fn f(x: [int) {\n}\n",
        "expected ']' to close the list type, found ')'",
        ")",
        0,
    );
    expect_err(
        "fn f() { match v { 1 2 } }",
        "expected '=>' after the pattern, found integer 2",
        "2",
        0,
    );
    expect_err(
        "fn f() { if x {} else x }",
        "expected '{' to open a block, found name 'x'",
        "x",
        1,
    );
    expect_err(
        "const x int = 1\n",
        "expected ':' after the constant name, found name 'int'",
        "int",
        0,
    );
    expect_err("type T = | A\n", "expected variant name, found '|'", "|", 0);
    expect_err(
        "type P = { x int }\n",
        "expected ':' after the field name, found name 'int'",
        "int",
        0,
    );
    expect_err(
        "fn f() { g(1,\n}\n",
        "expected expression, found '}'",
        "}",
        0,
    );
    expect_err(
        "fn f() { assert }",
        "expected expression, found '}'",
        "}",
        0,
    );
    expect_err(
        "fn f() {\n  let x = 1 +\n}\n",
        "expected expression, found '}'",
        "}",
        0,
    );
    expect_err(
        "fn f() { let x = 1 # 2 }",
        "unexpected character '#'",
        "#",
        0,
    );
    expect_err(
        "fn f() { let x = 1 + ; }",
        "expected expression, found ';'",
        ";",
        0,
    );
}

#[test]
fn missing_closing_brace_at_end_of_input() {
    let src = "fn f() {\n  let x = 1\n";
    let d = parse(src).unwrap_err();
    assert_eq!(
        d.message,
        "expected '}' to close the block, found end of input"
    );
    assert_eq!(d.span.start as usize, src.len());
    assert_eq!(d.span.end as usize, src.len());
}

#[test]
fn errors_stop_at_the_first_problem() {
    let d = parse("fn f() { let = 1 let = 2 }").unwrap_err();
    assert_eq!(d.span.start, 13);
}
