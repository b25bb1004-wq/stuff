//! Text and structural checks for `crates/athena/web/*` and command palette wiring.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::path::Path;

fn extract_block<'a>(src: &'a str, start_marker: &str, open: char, close: char) -> &'a str {
    let start = src.find(start_marker).unwrap() + start_marker.len();
    let mut depth = 0usize;
    let mut begin = None;
    for (idx, ch) in src[start..].char_indices() {
        if ch == open {
            if depth == 0 {
                begin = Some(start + idx);
            }
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return &src[begin.unwrap()..=start + idx];
            }
        }
    }
    panic!("unclosed block after {start_marker}");
}

fn extract_quoted_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("{key}: \"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let mut out = String::new();
    let mut escaped = false;
    for ch in rest.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

#[test]
fn web_files_have_no_forbidden_calls_or_external_urls() {
    let web_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("web");
    let mut count = 0usize;
    for entry in std::fs::read_dir(&web_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        count += 1;
        let text = std::fs::read_to_string(&path).unwrap();
        for forbidden in [
            "alert(", "confirm(", "prompt(", "http://", "https://", "ws://", "wss://", "http:",
            "https:",
        ] {
            assert!(
                !text.contains(forbidden),
                "{} contains forbidden string `{forbidden}`",
                path.display()
            );
        }
    }
    assert!(count >= 6, "expected at least 6 web files, found {count}");
}

#[test]
fn palette_commands_have_handlers_and_shortcut_labels() {
    let web_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("web");
    let workbench_js = std::fs::read_to_string(web_dir.join("workbench.js")).unwrap();
    let athena_js = std::fs::read_to_string(web_dir.join("athena.js")).unwrap();

    assert!(
        workbench_js.lines().count() <= 900,
        "workbench.js exceeds 900 lines: {}",
        workbench_js.lines().count()
    );
    assert!(
        athena_js.lines().count() <= 900,
        "athena.js exceeds 900 lines: {}",
        athena_js.lines().count()
    );
    assert!(athena_js.contains("athena.tabs.v1"));

    let cmds_block = extract_block(&workbench_js, "const PALETTE_COMMANDS =", '[', ']');
    let handlers_block = extract_block(&workbench_js, "const COMMAND_HANDLERS =", '{', '}');

    let mut parsed_ids = BTreeSet::new();
    for line in cmds_block.lines() {
        let Some(id) = extract_quoted_field(line, "id") else {
            continue;
        };
        let label = extract_quoted_field(line, "label")
            .unwrap_or_else(|| panic!("command `{id}` missing label"));
        let shortcut = extract_quoted_field(line, "shortcut")
            .unwrap_or_else(|| panic!("command `{id}` missing shortcut"));
        assert!(!label.is_empty(), "command `{id}` has empty label");
        assert!(!shortcut.is_empty(), "command `{id}` has empty shortcut");

        let bare_handler = format!("{id}: () =>");
        let quoted_handler = format!("\"{id}\": () =>");
        assert!(
            handlers_block.contains(&bare_handler) || handlers_block.contains(&quoted_handler),
            "command `{id}` has no handler in COMMAND_HANDLERS"
        );
        assert!(parsed_ids.insert(id.clone()), "duplicate command id `{id}`");
    }

    let expected_ids: BTreeSet<String> = [
        "run",
        "test",
        "format",
        "new-file",
        "save",
        "close-tab",
        "toggle-panel",
        "toggle-sidebar",
        "toggle-theme",
        "goto-definition",
        "show-hover",
        "focus-terminal",
        "install-app",
        "debug-continue",
        "debug-step-over",
        "debug-step-into",
        "debug-step-out",
        "debug-restart",
        "debug-stop",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    assert_eq!(
        parsed_ids, expected_ids,
        "palette command ids do not match required set"
    );
}

#[test]
fn debug_ui_is_packed_and_wired() {
    let web_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("web");
    let debug_js = std::fs::read_to_string(web_dir.join("debug.js")).unwrap();
    let workbench_js = std::fs::read_to_string(web_dir.join("workbench.js")).unwrap();
    let athena_js = std::fs::read_to_string(web_dir.join("athena.js")).unwrap();
    let pack_rs = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("pack.rs"),
    )
    .unwrap();

    assert!(
        pack_rs.contains("\"debug.js\""),
        "pack.rs must copy debug.js"
    );
    assert!(
        athena_js.contains("./debug.js"),
        "athena.js must import debug.js"
    );
    assert!(
        debug_js.contains("athena.breakpoints.v1"),
        "breakpoints need their storage key"
    );
    for id in [
        "debug-continue",
        "debug-step-over",
        "debug-step-into",
        "debug-step-out",
        "debug-restart",
        "debug-stop",
    ] {
        assert!(
            debug_js.contains(&format!("\"{id}\"")),
            "debug.js lacks command `{id}`"
        );
        assert!(
            workbench_js.contains(&format!("\"{id}\": () => app.debugCommand(\"{id}\")")),
            "workbench.js lacks the palette handler for `{id}`"
        );
    }
    for key in ["\"F5\"", "\"F10\"", "\"F11\""] {
        assert!(
            debug_js.contains(key),
            "debug.js lacks the F-key handler {key}"
        );
    }
    assert!(
        debug_js.contains("ev.shiftKey"),
        "Shift+F5 and Shift+F11 need the shift check"
    );
    for forbidden in ["alert(", "confirm(", "prompt("] {
        assert!(!debug_js.contains(forbidden), "debug.js uses `{forbidden}`");
    }
    assert!(
        debug_js.contains("requestAnimationFrame"),
        "budget pauses must continue from requestAnimationFrame"
    );
}

#[test]
fn service_worker_only_touches_its_own_caches() {
    // The worker is generated by the shared template; check the text the pack writes.
    let sw = neon::webkit::sw_js("athena", "0123456789abcdef", &["./"]);
    assert!(
        sw.contains("k.indexOf(PREFIX + \"-\") === 0"),
        "sw.js must delete only caches with Athena's prefix on activate"
    );
    assert!(
        !sw.contains("caches.match("),
        "sw.js must answer from its own opened cache, never the origin-wide caches.match("
    );
}
