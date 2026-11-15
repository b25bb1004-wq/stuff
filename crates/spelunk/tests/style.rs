//! Cascade, shorthand, inheritance and invalid-value tests for `spelunk::style`.
#![allow(clippy::unwrap_used)]

use spelunk::dom::{Document, NodeId};
use spelunk::html;
use spelunk::style::{self, Computed, Display, Len};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 128, 0, 255];

fn find(doc: &Document, id: &str) -> NodeId {
    doc.elements()
        .into_iter()
        .find(|&n| doc.attr(n, "id") == Some(id))
        .expect("element with that id")
}

/// Computed style of the element with `id="..."`.
fn of(src: &str, id: &str) -> Computed {
    let doc = html::parse(src);
    let styles = style::compute(&doc);
    styles[find(&doc, id)].expect("element style")
}

#[test]
fn type_selector_is_lowest_specificity() {
    let src = r#"<style>#i { color: blue } .c { color: green } p { color: red }</style>
        <p id="i" class="c">x</p>"#;
    assert_eq!(of(src, "i").color, BLUE);
    let src = r#"<style>.c { color: green } p { color: red }</style><p id="i" class="c">x</p>"#;
    assert_eq!(of(src, "i").color, GREEN);
    let src = r#"<style>p { color: red }</style><p id="i" class="c">x</p>"#;
    assert_eq!(of(src, "i").color, RED);
}

#[test]
fn later_rule_wins_at_equal_specificity() {
    let src = r#"<style>p { color: red } p { color: blue }</style><p id="i">x</p>"#;
    assert_eq!(of(src, "i").color, BLUE);
}

#[test]
fn important_beats_higher_specificity() {
    let src = r#"<style>#i { color: red } p { color: blue !important }</style><p id="i">x</p>"#;
    assert_eq!(of(src, "i").color, BLUE);
}

#[test]
fn style_attribute_beats_author_sheet() {
    let src = r#"<style>#i { color: blue }</style><p id="i" style="color: red">x</p>"#;
    assert_eq!(of(src, "i").color, RED);
}

#[test]
fn author_beats_user_agent_sheet() {
    // The UA sheet gives p a 16px top margin; an author rule of equal specificity wins.
    let src = r#"<style>p { margin: 0 }</style><p id="i">x</p>"#;
    assert_eq!(of(src, "i").margin[0], Len::Px(0));
    let src = r#"<p id="i">x</p>"#;
    assert_eq!(of(src, "i").margin[0], Len::Px(16));
}

#[test]
fn margin_shorthand_expands_one_to_four_values() {
    let src = r#"<div id="a" style="margin: 1px 2px 3px 4px"></div>
        <div id="b" style="margin: 1px 2px"></div>
        <div id="c" style="margin: 1px 2px 3px"></div>
        <div id="d" style="margin: 5px"></div>"#;
    assert_eq!(
        of(src, "a").margin,
        [Len::Px(1), Len::Px(2), Len::Px(3), Len::Px(4)]
    );
    assert_eq!(
        of(src, "b").margin,
        [Len::Px(1), Len::Px(2), Len::Px(1), Len::Px(2)]
    );
    assert_eq!(
        of(src, "c").margin,
        [Len::Px(1), Len::Px(2), Len::Px(3), Len::Px(2)]
    );
    assert_eq!(of(src, "d").margin, [Len::Px(5); 4]);
}

#[test]
fn margin_auto_and_longhands() {
    let src = r#"<div id="a" style="margin: 0 auto; margin-top: 7px"></div>"#;
    let c = of(src, "a");
    assert_eq!(c.margin, [Len::Px(7), Len::Auto, Len::Px(0), Len::Auto]);
}

#[test]
fn padding_and_border_shorthands() {
    let src = r#"<div id="a" style="padding: 2px 3px 4px; border-width: 1px 2px"></div>"#;
    let c = of(src, "a");
    assert_eq!(c.padding, [2, 3, 4, 3]);
    assert_eq!(c.border, [1, 2, 1, 2]);
    let src = r#"<div id="b" style="border-left-width: 6px; padding-bottom: 9px"></div>"#;
    let c = of(src, "b");
    assert_eq!(c.border, [0, 0, 0, 6]);
    assert_eq!(c.padding, [0, 0, 9, 0]);
}

#[test]
fn negative_padding_and_border_clamp_to_zero() {
    let src = r#"<div id="a" style="padding: -5px; border-width: -2px"></div>"#;
    let c = of(src, "a");
    assert_eq!(c.padding, [0; 4]);
    assert_eq!(c.border, [0; 4]);
}

#[test]
fn colors_hex_names_and_transparent() {
    let src = r##"<div id="a" style="color: #abc; background-color: transparent"></div>
        <div id="b" style="color: #112233; background-color: Red"></div>
        <div id="c" style="color: gray"></div>"##;
    assert_eq!(of(src, "a").color, [170, 187, 204, 255]);
    assert_eq!(of(src, "a").background, [0, 0, 0, 0]);
    assert_eq!(of(src, "b").color, [17, 34, 51, 255]);
    assert_eq!(of(src, "b").background, RED);
    assert_eq!(of(src, "c").color, [128, 128, 128, 255]);
}

#[test]
fn color_and_font_size_inherit_and_background_does_not() {
    let src = r#"<div id="p" style="color: red; font-size: 20px; background-color: blue; padding: 5px">
        <span id="s"><b id="b">x</b></span></div>"#;
    let s = of(src, "s");
    let b = of(src, "b");
    assert_eq!(s.color, RED);
    assert_eq!(b.color, RED);
    assert_eq!(b.font_px, 20);
    assert_eq!(b.background, [0, 0, 0, 0]);
    assert_eq!(b.padding, [0; 4]);
    assert_eq!(s.display, Display::Inline);
}

#[test]
fn initial_values_without_rules() {
    let src = r#"<span id="s">x</span>"#;
    let c = of(src, "s");
    assert_eq!(c.display, Display::Inline);
    assert_eq!(c.width, Len::Auto);
    assert_eq!(c.height, Len::Auto);
    assert_eq!(c.color, [0, 0, 0, 255]);
    assert_eq!(c.background, [0, 0, 0, 0]);
    assert_eq!(c.font_px, 16);
}

#[test]
fn user_agent_display_and_font_rules() {
    let src = r#"<head><title>t</title></head><body><h1 id="h">x</h1><div id="d"></div></body>"#;
    assert_eq!(of(src, "h").font_px, 32);
    assert_eq!(of(src, "h").margin[0], Len::Px(21));
    assert_eq!(of(src, "d").display, Display::Block);
    let doc = html::parse(src);
    let styles = style::compute(&doc);
    let head = doc
        .elements()
        .into_iter()
        .find(|&n| doc.name(n) == Some("head"))
        .unwrap();
    assert_eq!(styles[head].unwrap().display, Display::None);
}

#[test]
fn author_display_none_and_block_override() {
    let src = r#"<style>#h { display: none } #d { display: block }</style>
        <span id="h">x</span><span id="d">y</span>"#;
    assert_eq!(of(src, "h").display, Display::None);
    assert_eq!(of(src, "d").display, Display::Block);
}

#[test]
fn bad_values_are_ignored() {
    let src = r#"<div id="a" style="width: 10foo; height: -4px; color: notacolor; color: red;
        margin: 1px 2px 3px 4px 5px; display: flexx; font-size: -3px; padding: auto"></div>"#;
    let c = of(src, "a");
    assert_eq!(c.width, Len::Auto);
    assert_eq!(c.height, Len::Auto);
    assert_eq!(c.color, RED);
    assert_eq!(c.margin, [Len::Px(0); 4]);
    assert_eq!(c.display, Display::Block);
    assert_eq!(c.font_px, 16);
    assert_eq!(c.padding, [0; 4]);
}

#[test]
fn bad_value_does_not_override_a_good_one() {
    let src = r#"<style>#a { width: 40px }</style><div id="a" style="width: 9xx"></div>"#;
    assert_eq!(of(src, "a").width, Len::Px(40));
}

#[test]
fn bare_zero_is_zero_pixels_and_unknown_properties_are_ignored() {
    let src = r#"<div id="a" style="margin: 0; width: 0; flex-grow: 3; float: left"></div>"#;
    let c = of(src, "a");
    assert_eq!(c.margin, [Len::Px(0); 4]);
    assert_eq!(c.width, Len::Px(0));
}

#[test]
fn attribute_and_nth_child_selectors_parse_and_match() {
    let src = r#"<style>a[href="x"] { color: red } li:nth-child(2n+1) { color: blue }</style>
        <a id="a1" href="x">a</a><a id="a2" href="y">b</a>
        <ul><li id="l1">1</li><li id="l2">2</li><li id="l3">3</li></ul>"#;
    assert_eq!(of(src, "a1").color, RED);
    assert_eq!(of(src, "a2").color, [0, 0, 0, 255]);
    assert_eq!(of(src, "l1").color, BLUE);
    assert_eq!(of(src, "l2").color, [0, 0, 0, 255]);
    assert_eq!(of(src, "l3").color, BLUE);
}

#[test]
fn non_elements_have_no_style_and_length_matches_nodes() {
    let src = "text only";
    let doc = html::parse(src);
    let styles = style::compute(&doc);
    assert_eq!(styles.len(), doc.nodes.len());
    for (id, s) in styles.iter().enumerate() {
        let is_el = doc.name(id).is_some();
        assert_eq!(s.is_some(), is_el);
    }
}
