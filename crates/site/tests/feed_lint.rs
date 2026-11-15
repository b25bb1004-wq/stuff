//! Static check for the board feed (T130b): `web/feed.js` passes the page lint (no raw
//! `requestAnimationFrame`, no `setInterval`, no external URLs, animation tokens only) and exposes
//! `window.Feed.start`. Behaviour is checked by `scripts/feed-check.mjs`.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use neon::webkit;

fn feed_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("web/feed.js");
    std::fs::read_to_string(&path).unwrap()
}

#[test]
fn feed_js_passes_page_lint() {
    let problems = webkit::lint_page(&feed_source());
    assert!(problems.is_empty(), "feed.js lint problems: {problems:#?}");
}

#[test]
fn feed_js_exposes_start() {
    let text = feed_source();
    assert!(
        text.contains("window.Feed = { start };"),
        "window.Feed export missing"
    );
    assert!(
        !text.contains("setInterval("),
        "feed.js must not call setInterval"
    );
}
