//! Host tests for `athena::lang` (definition, hover, format).
#![allow(clippy::unwrap_used)]

use assay::forall;
use athena::lang::{definition, format, hover};
use athena::service::check;

const MAIN_LOGOS: &str = r#"fn print(s: str) uses io = extern "io.print"

fn main() uses io {
  var sum = 0
  for i in 1..6 {
    sum = sum + i
    print("step " + str(i) + ": sum=" + str(sum) + "\n")
  }
}
"#;

const MATH_LOGOS: &str = r#"fn add(a: int, b: int) -> int {
  a + b
}

fn max2(a: int, b: int) -> int {
  if a > b { a } else { b }
}

test "add works" {
  assert(add(2, 3) == 5)
}

test "max2 works" {
  assert(max2(7, 3) == 7)
  assert(max2(-2, 4) == 4)
}

prop "max2 >= both" (a: int, b: int) {
  let m = max2(a, b)
  assert(m >= a)
  assert(m >= b)
}
"#;

const BROKEN_LOGOS: &str = "fn main() {\n  let x: int = \"not an int\"\n}\n";

fn nth_offset(src: &str, needle: &str, nth: usize) -> usize {
    let mut start = 0usize;
    for i in 0..=nth {
        let found = src[start..].find(needle).unwrap();
        let abs = start + found;
        if i == nth {
            return abs;
        }
        start = abs + needle.len();
    }
    unreachable!()
}

#[test]
fn definition_call_param_shadowing_variant_and_match_binding() {
    let src = r#"type Shape = Dot | Circle(int)

fn area(s: Shape) -> int {
  match s {
    Dot => 0,
    Circle(r) => r * r,
  }
}

fn scope_test(p: int) -> int {
  let x: int = p
  if true {
    let x = x + 1
    let y = area(Circle(x))
    y
  } else {
    x
  }
}

fn main() -> int {
  let d = Dot
  let s = area(d)
  s + scope_test(3)
}
"#;

    let area_decl = nth_offset(src, "area", 0);
    let area_call = nth_offset(src, "area", 1);
    assert_eq!(
        definition(src, area_call),
        Some((area_decl, area_decl + "area".len()))
    );

    let param_p_decl = src.find("p: int").unwrap();
    let param_p_use = src.find("= p").unwrap() + 2;
    assert_eq!(
        definition(src, param_p_use),
        Some((param_p_decl, param_p_decl + 1))
    );

    let outer_x_decl = src.find("x: int").unwrap();
    let inner_x_decl = src.find("let x = x + 1").unwrap() + "let ".len();
    let rhs_x_use = src.find("= x + 1").unwrap() + 2;
    let inner_x_use = src.find("Circle(x)").unwrap() + "Circle(".len();
    let else_x_use = src.rfind("\n    x\n").unwrap() + "\n    ".len();

    assert_eq!(
        definition(src, rhs_x_use),
        Some((outer_x_decl, outer_x_decl + 1))
    );
    assert_eq!(
        definition(src, inner_x_use),
        Some((inner_x_decl, inner_x_decl + 1))
    );
    assert_eq!(
        definition(src, else_x_use),
        Some((outer_x_decl, outer_x_decl + 1))
    );

    let circle_decl = nth_offset(src, "Circle", 0);
    let circle_pat = nth_offset(src, "Circle", 1);
    let circle_ctor = nth_offset(src, "Circle", 2);
    assert_eq!(
        definition(src, circle_pat),
        Some((circle_decl, circle_decl + "Circle".len()))
    );
    assert_eq!(
        definition(src, circle_ctor),
        Some((circle_decl, circle_decl + "Circle".len()))
    );

    let r_pat = src.find("Circle(r)").unwrap() + "Circle(".len();
    let r_body = src.find("r * r").unwrap();
    assert_eq!(definition(src, r_body), Some((r_pat, r_pat + 1)));

    let shape_decl = nth_offset(src, "Shape", 0);
    let shape_use = nth_offset(src, "Shape", 1);
    assert_eq!(
        definition(src, shape_use),
        Some((shape_decl, shape_decl + "Shape".len()))
    );
}

#[test]
fn definition_returns_none_for_whitespace_keywords_extern_builtins_and_invalid_src() {
    assert_eq!(definition(MAIN_LOGOS, 2), None); // space after "fn"
    assert_eq!(definition(MAIN_LOGOS, 0), None); // keyword "fn"
    assert_eq!(
        definition(
            MAIN_LOGOS,
            MAIN_LOGOS
                .find("let")
                .unwrap_or(MAIN_LOGOS.find("var").unwrap())
        ),
        None
    );

    let extern_call = MAIN_LOGOS.rfind("print(").unwrap();
    assert_eq!(definition(MAIN_LOGOS, extern_call), None);
    let builtin_call = MAIN_LOGOS.find("str(i)").unwrap();
    assert_eq!(definition(MAIN_LOGOS, builtin_call), None);

    let var_i_use = MAIN_LOGOS.find("sum + i").unwrap() + "sum + ".len();
    let var_i_decl = MAIN_LOGOS.find("for i").unwrap() + "for ".len();
    assert_eq!(
        definition(MAIN_LOGOS, var_i_use),
        Some((var_i_decl, var_i_decl + 1))
    );

    assert_eq!(definition("fn ( {", 0), None);
    assert_eq!(definition(MAIN_LOGOS, MAIN_LOGOS.len()), None);
}

#[test]
fn hover_covers_functions_types_params_bindings_and_literals() {
    let src = r#"type Point = { x: int, y: int }

fn max2(a: int, b: int) -> int {
  if a > b { a } else { b }
}

fn demo(p: Point) -> str {
  let annotated: int = 42
  let inferred = "hello"
  let m = max2(annotated, p.x)
  inferred
}
"#;

    let max2_call = src.rfind("max2(").unwrap();
    assert_eq!(
        hover(src, max2_call).as_deref(),
        Some("fn max2(a: int, b: int) -> int")
    );

    let point_use = src.find("p: Point").unwrap() + 3;
    assert_eq!(
        hover(src, point_use).as_deref(),
        Some("type Point = { x: int, y: int }")
    );

    let param_a_use = src.find("if a > b").unwrap() + 3;
    assert_eq!(hover(src, param_a_use).as_deref(), Some("param a: int"));

    let annotated_use = src.find("max2(annotated").unwrap() + 5;
    assert_eq!(
        hover(src, annotated_use).as_deref(),
        Some("let annotated: int")
    );

    let inferred_use = src.rfind("inferred").unwrap();
    assert_eq!(hover(src, inferred_use).as_deref(), Some("let inferred"));

    let int_lit = src.find("42").unwrap();
    assert_eq!(hover(src, int_lit).as_deref(), Some("int literal"));

    let str_lit = src.find("\"hello\"").unwrap() + 1;
    assert_eq!(hover(src, str_lit).as_deref(), Some("str literal"));
}

#[test]
fn format_seeded_files_is_idempotent_and_refuses_comments() {
    for src in [MAIN_LOGOS, MATH_LOGOS, BROKEN_LOGOS] {
        let first = format(src).unwrap();
        let second = format(&first).unwrap();
        assert_eq!(first, second, "format not idempotent");
        if check(src).is_empty() {
            assert!(
                check(&first).is_empty(),
                "formatted clean file failed check"
            );
        }
    }

    let with_comment = "// note\nfn main() -> int {\n  1\n}\n";
    let err = format(with_comment).unwrap_err();
    assert_eq!(err, "format would drop comments");

    let string_with_slashes = "fn main() -> str {\n  \"http://example // ok\"\n}\n";
    let formatted = format(string_with_slashes).unwrap();
    assert_eq!(format(&formatted).unwrap(), formatted);
}

#[test]
fn random_bytes_and_offsets_never_panic() {
    forall(200, 0xA7E4_A073, |rng| {
        let bytes = rng.bytes(256);
        let src = String::from_utf8_lossy(&bytes);
        let off = rng.below(src.len().saturating_add(8));
        let _ = definition(&src, off);
        let _ = hover(&src, off);
        let _ = format(&src);
    });

    let utf8_src = "fn f() -> str {\n  let s = \"αβγ\"\n  s\n}\n";
    for off in 0..=utf8_src.len() + 4 {
        let _ = definition(utf8_src, off);
        let _ = hover(utf8_src, off);
    }
}
