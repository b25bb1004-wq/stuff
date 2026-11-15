//! Hand-computed flex pages for `odyssey::layout` and `odyssey::style::compute_flex`, plus
//! no-panic checks on random flex HTML.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use odyssey::dom::Document;
use odyssey::html;
use odyssey::layout::{self, BoxRect, Layout};
use odyssey::style::{self, Display, Flex, FlexAlign, FlexDir, FlexJustify};

/// Parse, cascade and lay out `src` in a viewport `w` pixels wide.
fn lay(src: &str, w: i32) -> (Document, Layout) {
    let doc = html::parse(src);
    let styles = style::compute(&doc);
    let l = layout::layout(&doc, &styles, w);
    (doc, l)
}

/// Wrap body markup in a page whose body has no margin, so boxes start at (0, 0).
fn page(body: &str) -> String {
    format!("<html><body style=\"margin:0\">{body}</body></html>")
}

/// Border box of the block with `id`.
fn rect(doc: &Document, l: &Layout, id: &str) -> BoxRect {
    l.boxes
        .iter()
        .find(|(n, _)| doc.attr(*n, "id") == Some(id))
        .map(|(_, r)| *r)
        .expect("box for id")
}

fn r(x: i32, y: i32, w: i32, h: i32) -> BoxRect {
    BoxRect { x, y, w, h }
}

/// The node with `id`.
fn node(doc: &Document, id: &str) -> usize {
    doc.elements()
        .into_iter()
        .find(|&n| doc.attr(n, "id") == Some(id))
        .expect("node for id")
}

#[test]
fn row_start_fixed_sizes() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:400px"><div id="a" style="width:50px;height:20px"></div><div id="b" style="width:70px;height:30px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "f"), r(0, 0, 400, 30));
    assert_eq!(rect(&d, &l, "a"), r(0, 0, 50, 20));
    assert_eq!(rect(&d, &l, "b"), r(50, 0, 70, 30));
}

#[test]
fn row_justify_center() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:400px;justify-content:center"><div id="a" style="width:50px;height:30px"></div><div id="b" style="width:70px;height:30px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a").x, 140);
    assert_eq!(rect(&d, &l, "b").x, 190);
}

#[test]
fn row_justify_end() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:400px;justify-content:flex-end"><div id="a" style="width:50px;height:30px"></div><div id="b" style="width:70px;height:30px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a").x, 280);
    assert_eq!(rect(&d, &l, "b").x, 330);
}

#[test]
fn row_space_between_three_items() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:400px;justify-content:space-between"><div id="a" style="width:50px;height:10px"></div><div id="b" style="width:50px;height:10px"></div><div id="c" style="width:50px;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a").x, 0);
    assert_eq!(rect(&d, &l, "b").x, 175);
    assert_eq!(rect(&d, &l, "c").x, 350);
}

#[test]
fn row_space_between_one_item_is_start() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:400px;justify-content:space-between"><div id="a" style="width:50px;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a"), r(0, 0, 50, 10));
}

#[test]
fn row_gap_with_end_justify() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:400px;gap:10px;justify-content:flex-end"><div id="a" style="width:50px;height:10px"></div><div id="b" style="width:50px;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a").x, 290);
    assert_eq!(rect(&d, &l, "b").x, 350);
}

#[test]
fn row_grow_remainder_goes_to_last() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:100px"><div id="a" style="flex-grow:1;height:10px"></div><div id="b" style="flex-grow:1;height:10px"></div><div id="c" style="flex-grow:1;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a"), r(0, 0, 33, 10));
    assert_eq!(rect(&d, &l, "b"), r(33, 0, 33, 10));
    assert_eq!(rect(&d, &l, "c"), r(66, 0, 34, 10));
}

#[test]
fn row_grow_weighted_with_fixed_item() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:300px"><div id="a" style="width:100px;height:10px"></div><div id="b" style="flex-grow:2;height:10px"></div><div id="c" style="flex-grow:1;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a"), r(0, 0, 100, 10));
    assert_eq!(rect(&d, &l, "b"), r(100, 0, 133, 10));
    assert_eq!(rect(&d, &l, "c"), r(233, 0, 67, 10));
}

#[test]
fn row_align_center_in_fixed_height() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;height:100px;align-items:center"><div id="a" style="width:50px;height:20px"></div><div id="b" style="width:50px;height:40px"></div><div id="c" style="width:50px;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "f").h, 100);
    assert_eq!(rect(&d, &l, "a").y, 40);
    assert_eq!(rect(&d, &l, "b").y, 30);
    assert_eq!(rect(&d, &l, "c").y, 45);
}

#[test]
fn row_align_end_in_fixed_height() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;height:100px;align-items:flex-end"><div id="a" style="width:50px;height:20px"></div><div id="b" style="width:50px;height:40px"></div><div id="c" style="width:50px;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a").y, 80);
    assert_eq!(rect(&d, &l, "b").y, 60);
    assert_eq!(rect(&d, &l, "c").y, 90);
}

#[test]
fn row_stretch_auto_height_item() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex"><div id="a" style="width:50px;height:40px"></div><div id="b" style="width:50px"><div id="in" style="height:25px"></div></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "f").h, 40);
    assert_eq!(rect(&d, &l, "b"), r(50, 0, 50, 40));
    assert_eq!(rect(&d, &l, "in"), r(50, 0, 50, 25));
}

#[test]
fn row_stretch_keeps_margin_and_padding() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;height:60px"><div id="a" style="width:50px;margin:5px;padding:3px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "f").h, 60);
    assert_eq!(rect(&d, &l, "a"), r(5, 5, 56, 50));
}

#[test]
fn row_center_uses_outer_box_with_margin() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;height:50px;align-items:center"><div id="a" style="width:50px;height:20px;margin-top:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a"), r(0, 20, 50, 20));
}

#[test]
fn row_inline_item_is_reported_as_block() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:100px"><span id="s" style="width:20px;height:10px"></span></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "s"), r(0, 0, 20, 10));
}

#[test]
fn text_children_of_flex_are_not_laid_out() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:100px">hello<div id="a" style="width:10px;height:5px"></div></div>"#,
        ),
        400,
    );
    assert!(l.runs.is_empty());
    assert_eq!(rect(&d, &l, "a"), r(0, 0, 10, 5));
}

#[test]
fn column_stacks_with_gap_and_margin() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;flex-direction:column;width:200px;gap:10px"><div id="a" style="height:20px"></div><div id="b" style="height:30px;margin-top:5px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a"), r(0, 0, 200, 20));
    assert_eq!(rect(&d, &l, "b"), r(0, 35, 200, 30));
    assert_eq!(rect(&d, &l, "f"), r(0, 0, 200, 65));
}

#[test]
fn column_align_center() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;flex-direction:column;width:200px;align-items:center"><div id="a" style="width:50px;height:10px"></div><div id="b" style="width:80px;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a").x, 75);
    assert_eq!(rect(&d, &l, "b").x, 60);
}

#[test]
fn column_align_end() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;flex-direction:column;width:200px;align-items:flex-end"><div id="a" style="width:50px;height:10px"></div><div id="b" style="width:80px;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a").x, 150);
    assert_eq!(rect(&d, &l, "b").x, 120);
}

#[test]
fn column_grow_in_fixed_height() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;flex-direction:column;width:200px;height:100px"><div id="a" style="height:10px;flex-grow:1"></div><div id="b" style="height:20px;flex-grow:3"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a"), r(0, 0, 200, 27));
    assert_eq!(rect(&d, &l, "b"), r(0, 27, 200, 73));
    assert_eq!(rect(&d, &l, "f").h, 100);
}

#[test]
fn column_justify_center() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;flex-direction:column;width:200px;height:100px;justify-content:center"><div id="a" style="height:20px"></div><div id="b" style="height:30px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a").y, 25);
    assert_eq!(rect(&d, &l, "b").y, 45);
}

#[test]
fn column_justify_space_between() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;flex-direction:column;width:200px;height:100px;justify-content:space-between"><div id="a" style="height:10px"></div><div id="b" style="height:10px"></div><div id="c" style="height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a").y, 0);
    assert_eq!(rect(&d, &l, "b").y, 45);
    assert_eq!(rect(&d, &l, "c").y, 90);
}

#[test]
fn column_stretch_fills_between_margins() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;flex-direction:column;width:200px"><div id="a" style="margin-left:10px;margin-right:20px;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a"), r(10, 0, 170, 10));
}

#[test]
fn display_none_item_is_skipped() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:100px;justify-content:flex-end"><div id="a" style="display:none;width:100px"></div><div id="b" style="width:50px;height:10px"></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "b"), r(50, 0, 50, 10));
    assert!(l.boxes.iter().all(|(n, _)| doc_id(&d, *n) != Some("a")));
}

fn doc_id(d: &Document, n: usize) -> Option<&str> {
    d.attr(n, "id")
}

#[test]
fn block_inside_flex_item_is_stacked() {
    let (d, l) = lay(
        &page(
            r#"<div id="f" style="display:flex;width:300px;height:50px"><div id="a" style="width:50px"><div id="in" style="height:20px"></div></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "a"), r(0, 0, 50, 50));
    assert_eq!(rect(&d, &l, "in"), r(0, 0, 50, 20));
}

#[test]
fn flex_inside_block_with_margin() {
    let (d, l) = lay(
        &page(
            r#"<div id="blk" style="margin:10px"><div id="f" style="display:flex;height:30px"><div id="a" style="width:20px"></div></div></div>"#,
        ),
        400,
    );
    assert_eq!(rect(&d, &l, "blk"), r(10, 10, 380, 30));
    assert_eq!(rect(&d, &l, "f"), r(10, 10, 380, 30));
    assert_eq!(rect(&d, &l, "a"), r(10, 10, 20, 30));
}

#[test]
fn flex_properties_parse_and_reject_bad_values() {
    let doc = html::parse(&page(
        r#"<div id="x" style="display:flex; flex-direction:column; justify-content:space-between; align-items:center; flex-grow:3; gap:-4px"></div><div id="y" style="flex-grow:-1; gap:2px 3px; justify-content:left; align-items:baseline; flex-direction:up"></div><div id="z" style="flex-grow:2.5"></div>"#,
    ));
    let styles = style::compute(&doc);
    let fx = style::compute_flex(&doc);
    assert_eq!(styles[node(&doc, "x")].unwrap().display, Display::Flex);
    assert_eq!(
        fx[node(&doc, "x")],
        Some(Flex {
            dir: FlexDir::Column,
            justify: FlexJustify::SpaceBetween,
            align: FlexAlign::Center,
            grow: 3,
            gap: 0,
        })
    );
    let initial = Flex {
        dir: FlexDir::Row,
        justify: FlexJustify::Start,
        align: FlexAlign::Stretch,
        grow: 0,
        gap: 0,
    };
    assert_eq!(fx[node(&doc, "y")], Some(initial));
    assert_eq!(fx[node(&doc, "z")], Some(initial));
}

#[test]
fn flex_properties_do_not_inherit() {
    let doc = html::parse(&page(
        r#"<div id="p" style="display:flex;gap:9px;flex-grow:4;flex-direction:column"><div id="c"></div></div>"#,
    ));
    let fx = style::compute_flex(&doc);
    assert_eq!(fx[node(&doc, "c")].unwrap().gap, 0);
    assert_eq!(fx[node(&doc, "c")].unwrap().grow, 0);
    assert_eq!(fx[node(&doc, "c")].unwrap().dir, FlexDir::Row);
}

/// Random flex markup built from a pool of properties, some of them invalid or extreme.
fn random_markup(rng: &mut Rng, depth: usize, out: &mut String) {
    const PROPS: &[&str] = &[
        "display:flex",
        "display:flex;flex-direction:column",
        "display:none",
        "display:inline",
        "width:30px",
        "width:auto",
        "height:-5px",
        "height:40px",
        "gap:7px",
        "gap:-3px",
        "gap:99999999999px",
        "flex-grow:2",
        "flex-grow:99999999999999",
        "flex-grow:-1",
        "justify-content:space-between",
        "justify-content:center",
        "justify-content:flex-end",
        "align-items:center",
        "align-items:stretch",
        "align-items:flex-end",
        "margin:4px auto",
        "margin-left:auto;margin-right:auto",
        "margin-top:-9999999px",
        "padding:2px 5px",
        "border-width:3px",
        "width:2147483647px",
    ];
    let children = rng.below(4);
    for _ in 0..children {
        let mut style = String::new();
        for _ in 0..rng.below(4) {
            style.push_str(rng.pick(PROPS));
            style.push(';');
        }
        out.push_str(&format!("<div style=\"{style}\">"));
        if depth < 5 && rng.chance(2, 3) {
            random_markup(rng, depth + 1, out);
        }
        if rng.chance(1, 2) {
            out.push_str("some words here");
        }
        out.push_str("</div>");
    }
}

#[test]
fn random_flex_markup_never_panics() {
    forall(300, 0xF1E8, |rng| {
        let mut body = String::new();
        random_markup(rng, 0, &mut body);
        let doc = html::parse(&format!("<html><body>{body}</body></html>"));
        let styles = style::compute(&doc);
        let _ = style::compute_flex(&doc);
        let width = i32::try_from(rng.range(0, 1000)).unwrap_or(0);
        let l = layout::layout(&doc, &styles, width);
        assert!(l.boxes.len() <= doc.elements().len() + 1);
    });
}
