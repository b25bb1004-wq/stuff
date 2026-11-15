//! Tests for the motion kit: the generated CSS and table from `indium::motion`, the reduced-motion
//! switch, the checked-in copies of the generated files, and the size and lint of `motion.js`.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use neon::webkit::{lint, motion_css, motion_table_js, FILES};

fn kit_file(name: &str) -> &'static str {
    FILES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
        .unwrap()
}

#[test]
fn generated_css_has_three_springs_and_reduced_motion_switch() {
    let css = motion_css();
    for name in ["firm", "live", "heavy"] {
        assert!(
            css.contains(&format!("--spring-{name}:")),
            "missing spring {name}"
        );
        assert!(
            css.contains(&format!("--spring-{name}-t:")),
            "missing duration of {name}"
        );
    }
    assert!(css.contains("linear("), "no linear() curve");
    assert!(css.contains("@media (prefers-reduced-motion: reduce)"));
    assert!(css.contains(":root[data-motion=\"reduce\"]"));
    assert!(lint(&css).is_empty(), "{:?}", lint(&css));
}

#[test]
fn generated_table_is_a_js_global_with_three_springs() {
    let js = motion_table_js();
    assert!(js.contains("var MOTION_TABLE = {"));
    for name in ["firm", "live", "heavy"] {
        assert!(
            js.contains(&format!("\"{name}\": {{\"ms\": ")),
            "missing {name}"
        );
    }
}

#[test]
fn checked_in_generated_files_match_the_generator() {
    assert_eq!(
        kit_file("motion.css"),
        motion_css(),
        "web/kit/motion.css is stale; regenerate with: cargo test -p neon --test motion -- --ignored regenerate_motion_files"
    );
    assert_eq!(
        kit_file("motion-table.js"),
        motion_table_js(),
        "web/kit/motion-table.js is stale; regenerate with: cargo test -p neon --test motion -- --ignored regenerate_motion_files"
    );
}

/// Rewrites `web/kit/motion.css` and `motion-table.js` from `indium::motion`. Run after changing a preset.
#[test]
#[ignore = "rewrites web/kit/motion.css and motion-table.js; run with --ignored"]
fn regenerate_motion_files() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("web/kit");
    std::fs::write(dir.join("motion.css"), motion_css()).unwrap();
    std::fs::write(dir.join("motion-table.js"), motion_table_js()).unwrap();
}

#[test]
fn selftest_loads_the_generated_files() {
    let page = kit_file("selftest.html");
    assert!(page.contains("href=\"motion.css\""));
    assert!(page.contains("src=\"motion-table.js\""));
}

#[test]
fn motion_js_is_under_10_kib() {
    let text = kit_file("motion.js");
    assert!(text.len() < 10240, "motion.js is {} bytes", text.len());
}

#[test]
fn kit_js_has_no_interval_or_external_loads() {
    for (name, text) in FILES {
        assert!(!text.contains("setInterval("), "{name} uses setInterval");
        assert!(
            !text.contains("http://") && !text.contains("https://"),
            "{name} loads a URL"
        );
    }
}
