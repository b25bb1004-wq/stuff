//! Tests for all registered site demos: default outputs, error handling, and fuzz bounds.
#![allow(clippy::unwrap_used)]

use std::time::Instant;

use assay::forall;
use site::demos::{self, Output, MAX_INPUT_BYTES};

fn expect_text(demo_id: &str, input: &str) -> String {
    let demo = demos::find(demo_id).unwrap();
    match (demo.run)(input).unwrap() {
        Output::Text(t) => t,
        Output::Pixels { .. } => panic!("expected text output for {demo_id}"),
    }
}

#[test]
fn all_default_demos_run_fast_and_match_expected_fragments() {
    let expected: &[(&str, &str)] = &[
        ("iridium", "sum_depth"),
        (
            "sphinx",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        ("silicon", "a0 (x10) = 0x000000000000002a (42)"),
        ("hades", "fsck: 0 errors, 0 fixed"),
        ("chisel", "0x80000000:"),
        ("lithium", "exit:"),
    ];

    for (id, fragment) in expected {
        let demo = demos::find(id).unwrap();
        let t0 = Instant::now();
        let out = (demo.run)(demo.default_input)
            .unwrap_or_else(|e| panic!("demo `{id}` failed on default input: {e}"));
        let elapsed = t0.elapsed();
        assert!(
            elapsed.as_millis() < 200,
            "demo `{id}` took {elapsed:?} (>= 200 ms)"
        );
        match out {
            Output::Text(text) => {
                assert!(
                    text.contains(fragment),
                    "demo `{id}` output missing `{fragment}`:\n{text}"
                );
            }
            Output::Pixels { .. } => panic!("unexpected pixel output for `{id}`"),
        }
    }

    assert_eq!(demos::all().len(), expected.len());
}

#[test]
fn sphinx_abc_vector_and_roundtrip() {
    let out = expect_text("sphinx", "abc");
    assert!(out.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
    assert!(out.contains("roundtrip=ok"));
    assert!(out.contains("verify=ok"));
}

#[test]
fn lithium_produces_uart_output() {
    let out = expect_text("lithium", "2000000");
    assert!(
        out.contains("lithium") || out.contains("Lithium") || out.lines().count() >= 2,
        "unexpected lithium output:\n{out}"
    );
}

#[test]
fn bad_and_oversized_inputs_return_err_without_panicking() {
    let bad_cases: &[(&str, &str)] = &[
        ("iridium", "SELECT FROM ???"),
        ("hades", "unknowncmd /foo"),
        ("chisel", "not_an_instruction r99"),
        ("silicon", "badop x0, x1"),
        ("lithium", "not-a-number"),
        ("lithium", "0"),
    ];

    for (id, bad) in bad_cases {
        let demo = demos::find(id).unwrap();
        let err = (demo.run)(bad).expect_err(&format!("expected Err for `{id}` on `{bad}`"));
        assert!(!err.is_empty(), "empty error message for `{id}`");
    }

    let oversized = "a".repeat(MAX_INPUT_BYTES + 1);
    for demo in demos::all() {
        let err = (demo.run)(&oversized)
            .expect_err(&format!("expected size limit Err for `{}`", demo.id));
        assert!(err.contains("limit"), "unexpected size error: {err}");
    }
}

#[test]
fn forall_random_inputs_never_panic_and_stay_bounded() {
    for (idx, demo) in demos::all().iter().enumerate() {
        let run = demo.run;
        forall(50, 7500 + idx as u64, |r| {
            let raw = r.bytes(512);
            let s = String::from_utf8_lossy(&raw);
            let t0 = Instant::now();
            let _ = run(&s);
            assert!(
                t0.elapsed().as_millis() < 500,
                "demo `{}` exceeded time bound",
                demo.id
            );
        });
    }
}
