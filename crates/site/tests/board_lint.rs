//! Kit lint over the board files (T130a): `neon::webkit::lint_page` on board.js and board-demo.html.
//! Bans URLs, raw requestAnimationFrame, setInterval, will-change, transitions off opacity and transform,
//! and bare durations outside a /* neon-tokens */ file.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use neon::webkit;

fn read(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("web")
        .join(name);
    std::fs::read_to_string(&path).unwrap()
}

#[test]
fn board_files_pass_kit_lint() {
    for name in ["board.js", "board-demo.html"] {
        let problems = webkit::lint_page(&read(name));
        assert!(problems.is_empty(), "{name}: {problems:?}");
    }
}
