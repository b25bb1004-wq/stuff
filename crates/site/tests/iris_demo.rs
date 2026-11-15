//! Iris demo through the same registry lookup the wasm `site_demo` export uses.
#![allow(clippy::unwrap_used)]

use site::demos::{self, Output};

fn run_iris(input: &str) -> Result<Output, String> {
    let demo = demos::find("iris").unwrap();
    (demo.run)(input)
}

#[test]
fn default_script_renders_with_terminal() {
    // krypton's Shell::new avoids std::process::id on wasm, so the default opens a terminal.
    let demo = demos::find("iris").unwrap();
    assert!(demo.default_input.contains("open terminal"));
    let Output::Pixels { w, h, rgba } = run_iris(demo.default_input).unwrap() else {
        panic!("expected pixels for the iris default script");
    };
    assert_eq!((w, h), (640, 400));
    assert_eq!(rgba.len(), 640 * 400 * 4);
}

#[test]
fn second_example_without_terminal_renders() {
    let script = "open files / 260 190\nmove 1 290 70\nfocus prev\nwait 200";
    let Output::Pixels { w, h, .. } = run_iris(script).unwrap() else {
        panic!("expected pixels");
    };
    assert_eq!((w, h), (640, 400));
}

#[test]
fn terminal_request_is_a_clean_error_text_not_a_panic() {
    // Native builds open the terminal; the wasm build returns the same error text as Err.
    // Either way the entry point must return, never trap.
    match run_iris("open terminal\nwarp 99") {
        Ok(_) => panic!("unknown command must fail"),
        Err(msg) => assert!(!msg.is_empty()),
    }
}
