//! Tests for `neon::webkit`: the kit file list, the bundle lint, the service worker template and the hash.
#![allow(clippy::unwrap_used)]

use neon::webkit::{content_hash, lint, sw_js, FILES};

#[test]
fn files_lists_the_three_kit_files() {
    let names: Vec<&str> = FILES.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["tokens.css", "wasm.js", "term.js"]);
    for (name, text) in FILES {
        assert!(!text.is_empty(), "empty kit file {name}");
    }
}

#[test]
fn lint_passes_the_kit_files() {
    for (name, text) in FILES {
        assert!(lint(text).is_empty(), "{name}: {:?}", lint(text));
    }
}

#[test]
fn lint_rejects_external_urls() {
    let sample = "const x = fetch(\"https://example.com/a.wasm\");";
    let problems = lint(sample);
    assert!(
        problems.iter().any(|p| p.contains("https://")),
        "{problems:?}"
    );
    assert!(!lint("<a href=\"http://example.org\">").is_empty());
}

#[test]
fn lint_rejects_layout_animating_transitions() {
    for sample in [
        ".a { transition: width 200ms; }",
        ".a { transition: opacity 1s, height 1s; }",
        ".a { transition:height .2s }",
        ".a { transition-property: width; }",
    ] {
        assert!(!lint(sample).is_empty(), "accepted {sample}");
    }
    assert!(lint(".a { transition: opacity 120ms, transform 120ms; }").is_empty());
    assert!(lint(".a { transition-duration: 120ms; }").is_empty());
}

#[test]
fn sw_template_never_uses_global_caches_match() {
    let js = sw_js("site", "0123abcd", &["_kit/wasm.js", "_kit/term.js"]);
    assert!(
        !js.contains("caches.match("),
        "sw.js must not call caches.match"
    );
    assert!(js.contains("caches.open(CACHE)"));
    assert!(js.contains("\"site\""));
    assert!(js.contains("\"_kit/term.js\""));
}

#[test]
fn sw_js_escapes_its_inputs() {
    let js = sw_js("a\"b", "h", &["x</script>.js"]);
    assert!(js.contains("\"a\\\"b\""));
    assert!(!js.contains("</script>"));
}

#[test]
fn content_hash_is_stable_and_separates_parts() {
    let a = content_hash(&[b"ab", b"c"]);
    assert_eq!(a.len(), 16);
    assert_eq!(a, content_hash(&[b"ab", b"c"]));
    assert_ne!(a, content_hash(&[b"a", b"bc"]));
}
