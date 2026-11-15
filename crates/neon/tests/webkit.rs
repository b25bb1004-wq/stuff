//! Tests for `neon::webkit`: the kit file list, the bundle lint, the service worker template and the hash.
#![allow(clippy::unwrap_used)]

use neon::webkit::{content_hash, lint, lint_page, sw_js, FILES};

#[test]
fn files_lists_the_kit_files() {
    let names: Vec<&str> = FILES.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        [
            "tokens.css",
            "wasm.js",
            "term.js",
            "motion.css",
            "motion-table.js",
            "motion.js",
            "selftest.html"
        ]
    );
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
    assert!(lint(".a { transition: opacity var(--t-fast), transform var(--t-fast); }").is_empty());
    assert!(lint(".a { transition-duration: var(--t-fast); }").is_empty());
}

#[test]
fn lint_rejects_will_change_and_set_interval() {
    assert!(!lint(".a { will-change: transform; }").is_empty());
    assert!(!lint("setInterval(tick, 16);").is_empty());
}

#[test]
fn lint_rejects_bare_durations_outside_token_files() {
    for sample in [
        ".a { animation-duration: 200ms; }",
        ".a { transition-duration: .5s; }",
        "el.animate(f, { duration: 300 + 1s });",
        ".a { --x: 0.01ms; }",
    ] {
        assert!(!lint(sample).is_empty(), "accepted {sample}");
    }
    assert!(lint(".a { animation-duration: var(--t-med); }").is_empty());
    assert!(lint(".a { --x: 10 px; font-size: 1.5rem; }").is_empty());
    let tokens = "/* neon-tokens */\n.a { --t-press: 80ms; }";
    assert!(lint(tokens).is_empty(), "{:?}", lint(tokens));
}

#[test]
fn lint_rejects_keyframes_and_transitions_on_other_properties() {
    assert!(!lint("@keyframes spin { from { transform: none } to { left: 0 } }").is_empty());
    assert!(
        lint("@keyframes fade { from { opacity: 0 } to { opacity: 1; transform: none } }")
            .is_empty()
    );
    assert!(!lint(".a { transition: background-color 1s; }").is_empty());
    assert!(!lint(".a { transition: all 1s; }").is_empty());
}

#[test]
fn lint_page_rejects_request_animation_frame_but_kit_lint_does_not() {
    let sample = "requestAnimationFrame(tick);";
    assert!(!lint_page(sample).is_empty());
    assert!(lint(sample).is_empty());
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
