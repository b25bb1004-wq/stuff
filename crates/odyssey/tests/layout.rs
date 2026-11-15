//! Hand-computed layout pages for `odyssey::layout`, plus no-panic checks on random HTML.
#![allow(clippy::unwrap_used)]

use assay::forall;
use odyssey::dom::Document;
use odyssey::html;
use odyssey::layout::{self, BoxRect, Layout};
use odyssey::style;

type Run = (String, i32, i32, i32, i32, u32);

/// Parse, cascade and lay out `src` in a viewport `w` pixels wide.
fn lay(src: &str, w: i32) -> (Document, Layout) {
    let doc = html::parse(src);
    let styles = style::compute(&doc);
    let l = layout::layout(&doc, &styles, w);
    (doc, l)
}

/// Border box of the block whose tag name or `id` is `sel`.
fn rect(doc: &Document, l: &Layout, sel: &str) -> BoxRect {
    l.boxes
        .iter()
        .find(|(n, _)| doc.name(*n) == Some(sel) || doc.attr(*n, "id") == Some(sel))
        .map(|(_, r)| *r)
        .expect("box for selector")
}

fn r(x: i32, y: i32, w: i32, h: i32) -> BoxRect {
    BoxRect { x, y, w, h }
}

/// Every word run as (text, x, y, w, h, scale).
fn runs(l: &Layout) -> Vec<Run> {
    l.runs
        .iter()
        .map(|t| {
            (
                t.text.clone(),
                t.rect.x,
                t.rect.y,
                t.rect.w,
                t.rect.h,
                t.scale,
            )
        })
        .collect()
}

fn run(text: &str, x: i32, y: i32, w: i32, h: i32, scale: u32) -> Run {
    (text.to_string(), x, y, w, h, scale)
}

#[test]
fn nested_default_margins_on_body_and_html() {
    let (doc, l) = lay(r#"<div id="a" style="height:10px"></div>"#, 100);
    assert_eq!(rect(&doc, &l, "html"), r(0, 0, 100, 26));
    assert_eq!(rect(&doc, &l, "body"), r(8, 8, 84, 10));
    assert_eq!(rect(&doc, &l, "a"), r(8, 8, 84, 10));
    assert_eq!(l.height, 26);
}

#[test]
fn sibling_margins_collapse_to_the_larger() {
    let src = r#"<div id="a" style="height:10px;margin:0 0 20px 0"></div>
        <div id="b" style="height:10px;margin:30px 0 0 0"></div>"#;
    let (doc, l) = lay(src, 100);
    assert_eq!(rect(&doc, &l, "a"), r(8, 8, 84, 10));
    assert_eq!(rect(&doc, &l, "b"), r(8, 48, 84, 10));
    assert_eq!(rect(&doc, &l, "body"), r(8, 8, 84, 50));
    assert_eq!(l.height, 66);
}

#[test]
fn fixed_width_with_auto_margins_centres() {
    let (doc, l) = lay(
        r#"<div id="a" style="width:40px;margin:0 auto;height:5px"></div>"#,
        100,
    );
    assert_eq!(rect(&doc, &l, "a"), r(30, 8, 40, 5));
}

#[test]
fn auto_left_margin_takes_all_slack() {
    let (doc, l) = lay(
        r#"<div id="a" style="width:30px;margin-left:auto;height:5px"></div>"#,
        100,
    );
    assert_eq!(rect(&doc, &l, "a"), r(62, 8, 30, 5));
}

#[test]
fn auto_width_keeps_fixed_margins() {
    let (doc, l) = lay(
        r#"<div id="a" style="margin:0 10px;height:5px"></div>"#,
        100,
    );
    assert_eq!(rect(&doc, &l, "a"), r(18, 8, 64, 5));
}

#[test]
fn fixed_width_wider_than_container_keeps_fixed_left_margin() {
    let (doc, l) = lay(
        r#"<div id="a" style="width:200px;margin-left:3px;height:5px"></div>"#,
        100,
    );
    assert_eq!(rect(&doc, &l, "a"), r(11, 8, 200, 5));
}

#[test]
fn padding_and_border_are_part_of_the_border_box() {
    let (doc, l) = lay(
        r#"<div id="a" style="padding:2px 3px;border-width:1px;height:10px"></div>"#,
        100,
    );
    assert_eq!(rect(&doc, &l, "a"), r(8, 8, 84, 16));
    assert_eq!(rect(&doc, &l, "body"), r(8, 8, 84, 16));
}

#[test]
fn fixed_width_with_padding_and_auto_margins() {
    let (doc, l) = lay(
        r#"<div id="a" style="width:20px;padding:0 5px;margin:0 auto;height:5px"></div>"#,
        100,
    );
    // Slack is 84 - 20 - 10 = 54, split 27 and 27; the border box is 30 wide.
    assert_eq!(rect(&doc, &l, "a"), r(35, 8, 30, 5));
}

#[test]
fn text_inside_padding_starts_at_the_content_edge() {
    let (doc, l) = lay(
        r#"<div id="d" style="padding-left:10px;padding-top:4px">ab</div>"#,
        100,
    );
    assert_eq!(rect(&doc, &l, "d"), r(8, 8, 84, 20));
    assert_eq!(runs(&l), vec![run("ab", 18, 12, 16, 16, 1)]);
}

#[test]
fn child_margin_inside_padded_parent_does_not_collapse_through() {
    let src = r#"<div id="o" style="padding-top:5px"><div id="i" style="margin-top:10px;height:20px"></div></div>"#;
    let (doc, l) = lay(src, 100);
    assert_eq!(rect(&doc, &l, "o"), r(8, 8, 84, 35));
    assert_eq!(rect(&doc, &l, "i"), r(8, 23, 84, 20));
    assert_eq!(rect(&doc, &l, "body"), r(8, 8, 84, 35));
}

#[test]
fn parent_and_child_margins_do_not_collapse() {
    let src = r#"<div id="o" style="margin-top:10px"><div id="i" style="margin-top:10px;height:5px"></div></div>"#;
    let (doc, l) = lay(src, 100);
    assert_eq!(rect(&doc, &l, "o"), r(8, 18, 84, 15));
    assert_eq!(rect(&doc, &l, "i"), r(8, 28, 84, 5));
    assert_eq!(rect(&doc, &l, "body"), r(8, 8, 84, 25));
}

#[test]
fn display_none_removes_the_subtree() {
    let src = r#"<div id="a" style="display:none;height:50px"><div id="x" style="height:3px"></div></div>
        <div id="b" style="height:5px"></div>"#;
    let (doc, l) = lay(src, 100);
    assert!(l.boxes.iter().all(|(n, _)| doc.attr(*n, "id") != Some("a")));
    assert!(l.boxes.iter().all(|(n, _)| doc.attr(*n, "id") != Some("x")));
    assert_eq!(rect(&doc, &l, "b"), r(8, 8, 84, 5));
    assert_eq!(rect(&doc, &l, "body"), r(8, 8, 84, 5));
}

#[test]
fn text_wraps_at_scale_one() {
    let (doc, l) = lay(r#"<div id="t" style="width:40px">aa bb cc</div>"#, 100);
    assert_eq!(
        runs(&l),
        vec![
            run("aa", 8, 8, 16, 16, 1),
            run("bb", 32, 8, 16, 16, 1),
            run("cc", 8, 24, 16, 16, 1),
        ]
    );
    assert_eq!(rect(&doc, &l, "t"), r(8, 8, 40, 32));
}

#[test]
fn text_wraps_at_scale_two() {
    let (doc, l) = lay(
        r#"<div id="t" style="width:70px;font-size:32px">aa bb</div>"#,
        100,
    );
    assert_eq!(
        runs(&l),
        vec![run("aa", 8, 8, 32, 32, 2), run("bb", 8, 40, 32, 32, 2)]
    );
    assert_eq!(rect(&doc, &l, "t"), r(8, 8, 70, 64));
}

#[test]
fn line_height_follows_the_largest_scale_on_the_line() {
    let src = r#"<div id="t" style="width:200px"><span style="font-size:32px">aa</span> bb</div>"#;
    let (doc, l) = lay(src, 100);
    assert_eq!(
        runs(&l),
        vec![run("aa", 8, 8, 32, 32, 2), run("bb", 48, 8, 16, 16, 1)]
    );
    assert_eq!(rect(&doc, &l, "t"), r(8, 8, 200, 32));
}

#[test]
fn words_wider_than_the_line_overflow_one_per_line() {
    let (_, l) = lay(r#"<div id="t" style="width:10px">hello world</div>"#, 100);
    assert_eq!(
        runs(&l),
        vec![
            run("hello", 8, 8, 40, 16, 1),
            run("world", 8, 24, 40, 16, 1)
        ]
    );
}

#[test]
fn inline_runs_around_a_block_form_anonymous_blocks() {
    let src = r#"<body style="margin:8px">hi<div id="b" style="height:10px"></div>yo</body>"#;
    let (doc, l) = lay(src, 100);
    assert_eq!(
        runs(&l),
        vec![run("hi", 8, 8, 16, 16, 1), run("yo", 8, 34, 16, 16, 1)]
    );
    assert_eq!(rect(&doc, &l, "b"), r(8, 24, 84, 10));
    assert_eq!(rect(&doc, &l, "body"), r(8, 8, 84, 42));
}

#[test]
fn whitespace_between_blocks_does_not_break_margin_collapsing() {
    let src =
        "<body style=\"margin:0\">\n<div id=\"b\" style=\"margin:16px 0;height:10px\"></div>\n\
        <div id=\"c\" style=\"margin:16px 0;height:10px\"></div>\n</body>";
    let (doc, l) = lay(src, 100);
    assert_eq!(rect(&doc, &l, "b"), r(0, 16, 100, 10));
    assert_eq!(rect(&doc, &l, "c"), r(0, 42, 100, 10));
    assert_eq!(rect(&doc, &l, "body"), r(0, 0, 100, 68));
}

#[test]
fn paragraph_margins_collapse_between_siblings() {
    let (doc, l) = lay(
        r#"<body style="margin:0"><p id="p">x</p><p id="q">y</p></body>"#,
        100,
    );
    assert_eq!(rect(&doc, &l, "p"), r(0, 16, 100, 16));
    assert_eq!(rect(&doc, &l, "q"), r(0, 48, 100, 16));
    assert_eq!(rect(&doc, &l, "body"), r(0, 0, 100, 80));
    assert_eq!(
        runs(&l),
        vec![run("x", 0, 16, 8, 16, 1), run("y", 0, 48, 8, 16, 1)]
    );
}

#[test]
fn heading_uses_scale_two_and_its_margins() {
    let (doc, l) = lay(r#"<body style="margin:0"><h1 id="h">ab</h1></body>"#, 300);
    assert_eq!(rect(&doc, &l, "h"), r(0, 21, 300, 32));
    assert_eq!(runs(&l), vec![run("ab", 0, 21, 32, 32, 2)]);
    assert_eq!(rect(&doc, &l, "body"), r(0, 0, 300, 74));
}

#[test]
fn list_padding_indents_items() {
    let (doc, l) = lay(
        r#"<body style="margin:0"><ul id="u"><li id="l">a</li></ul></body>"#,
        200,
    );
    assert_eq!(rect(&doc, &l, "u"), r(0, 16, 200, 16));
    assert_eq!(rect(&doc, &l, "l"), r(40, 16, 160, 16));
    assert_eq!(runs(&l), vec![run("a", 40, 16, 8, 16, 1)]);
    assert_eq!(rect(&doc, &l, "body"), r(0, 0, 200, 48));
}

#[test]
fn root_html_is_found_after_doctype() {
    let src = r#"<!DOCTYPE html><html id="r"><body style="margin:0"><div id="d" style="height:4px"></div></body></html>"#;
    let (doc, l) = lay(src, 50);
    assert_eq!(rect(&doc, &l, "html"), r(0, 0, 50, 4));
    assert_eq!(l.height, 4);
}

#[test]
fn root_margin_offsets_the_root_box() {
    let src = r#"<html style="margin:10px"><body style="margin:0"><div id="d" style="height:5px"></div></body></html>"#;
    let (doc, l) = lay(src, 100);
    assert_eq!(rect(&doc, &l, "html"), r(10, 10, 80, 5));
    assert_eq!(rect(&doc, &l, "d"), r(10, 10, 80, 5));
    assert_eq!(l.height, 5);
}

#[test]
fn block_inside_span_is_hoisted_out_of_the_inline_run() {
    let src =
        r#"<body style="margin:0"><span>a<div id="d" style="height:5px"></div>b</span></body>"#;
    let (doc, l) = lay(src, 200);
    assert_eq!(rect(&doc, &l, "d"), r(0, 16, 200, 5));
    assert_eq!(
        runs(&l),
        vec![run("a", 0, 0, 8, 16, 1), run("b", 0, 21, 8, 16, 1)]
    );
    assert_eq!(rect(&doc, &l, "body"), r(0, 0, 200, 37));
}

#[test]
fn whitespace_only_text_takes_no_height() {
    let (doc, l) = lay(r#"<div id="d">   </div>"#, 100);
    assert_eq!(rect(&doc, &l, "d"), r(8, 8, 84, 0));
    assert!(l.runs.is_empty());
    assert_eq!(rect(&doc, &l, "body"), r(8, 8, 84, 0));
}

#[test]
fn fixed_height_ignores_child_heights() {
    let src = r#"<div id="a" style="height:20px"><div id="b" style="height:50px"></div></div>"#;
    let (doc, l) = lay(src, 100);
    assert_eq!(rect(&doc, &l, "a"), r(8, 8, 84, 20));
    assert_eq!(rect(&doc, &l, "b"), r(8, 8, 84, 50));
    assert_eq!(rect(&doc, &l, "body"), r(8, 8, 84, 20));
}

#[test]
fn text_nodes_around_a_hidden_element_join_without_a_space() {
    let (_, l) = lay(
        r#"<div id="d">a<span style="display:none">b</span>c</div>"#,
        100,
    );
    assert_eq!(
        runs(&l),
        vec![run("a", 8, 8, 8, 16, 1), run("c", 16, 8, 8, 16, 1)]
    );
}

#[test]
fn zero_width_viewport_clamps_content_to_zero() {
    let (doc, l) = lay(r#"<div id="d">ab</div>"#, 0);
    assert_eq!(rect(&doc, &l, "d"), r(8, 8, 0, 16));
    assert_eq!(runs(&l), vec![run("ab", 8, 8, 16, 16, 1)]);
}

#[test]
fn negative_viewport_does_not_panic_and_boxes_stay_positive() {
    let (_, l) = lay(r#"<div style="padding:3px">ab cd</div>"#, -50);
    assert!(l.boxes.iter().all(|(_, b)| b.w >= 0 && b.h >= 0));
    assert!(l.height >= 0);
}

#[test]
fn negative_margin_cannot_make_a_block_negative() {
    let (doc, l) = lay(r#"<div id="a" style="margin-top:-30px"></div>"#, 100);
    assert_eq!(rect(&doc, &l, "a").h, 0);
}

#[test]
fn deep_nesting_is_bounded_not_fatal() {
    let src = "<div>".repeat(3000);
    let (_, l) = lay(&src, 100);
    assert!(l.boxes.len() <= 300);
}

#[test]
fn hidden_root_gives_an_empty_layout() {
    let (_, l) = lay(r#"<style>html { display: none }</style><p>x</p>"#, 100);
    assert!(l.boxes.is_empty());
    assert!(l.runs.is_empty());
    assert_eq!(l.height, 0);
}

const PIECES: &[&str] = &[
    "<div>",
    "</div>",
    "<p>",
    "</p>",
    "<span>",
    "</span>",
    "<ul>",
    "<li>",
    "</li>",
    "</ul>",
    "<h1>",
    "</h1>",
    "hello ",
    " world  ",
    "<br>",
    "<b>",
    "</b>",
    "<body>",
    "</body>",
    "<html>",
    "</html>",
    "<head>",
    "<style>p{margin:3px auto;width:20px} div{padding:1px 2px}</style>",
    "<div style=\"display:none\">",
    "<img>",
    "<!-- c -->",
    "&amp;",
    "x",
    "<table>",
    "<tr>",
    "<div style=\"padding:1px 2px;font-size:33px;width:7px;margin:0 auto\">",
    "<span style=\"font-size:0px;margin:-9px\">",
    "<h2 style=\"margin:-20px 0 -5px\">",
    "<p style=\"border-width:4px;height:-3px;width:9999999px\">",
    "\n",
    "\t",
    "</p>",
];

#[test]
fn random_html_never_panics_and_boxes_are_sane() {
    forall(300, 0x00C0_FFEE, |rng| {
        let mut src = String::new();
        for _ in 0..rng.below(60) {
            src.push_str(rng.pick(PIECES));
        }
        let raw = rng.bytes(200);
        src.push_str(&String::from_utf8_lossy(&raw));
        let doc = html::parse(&src);
        let styles = style::compute(&doc);
        let width = rng.range(0, 400) as i32 - 20;
        let l = layout::layout(&doc, &styles, width);
        assert!(l.boxes.iter().all(|(_, b)| b.w >= 0 && b.h >= 0));
        assert!(l
            .runs
            .iter()
            .all(|t| t.rect.w >= 0 && t.rect.h >= 0 && t.scale >= 1));
        assert!(l.height >= 0);
    });
}

#[test]
fn random_bytes_never_panic_in_style_or_layout() {
    forall(200, 0x5EED_0042, |rng| {
        let raw = rng.bytes(300);
        let src = String::from_utf8_lossy(&raw).into_owned();
        let doc = html::parse(&src);
        let styles = style::compute(&doc);
        let _ = layout::layout(&doc, &styles, 300);
    });
}
