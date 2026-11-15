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
    let expected_text: &[(&str, &str)] = &[
        ("iridium", "sum_depth"),
        ("elysium", "6! = 720"),
        ("mercury", "HTTP/1.1 200 OK"),
        ("cobalt", "answer = 42"),
        ("aurum", "verified=true"),
        (
            "sphinx",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        ("silicon", "a0 (x10) = 0x000000000000002a (42)"),
        ("hades", "fsck: 0 errors, 0 fixed"),
        ("pandora", "validated: ok"),
        ("chisel", "0x80000000:"),
        ("argo", "alpha = \"1.1.0\""),
        ("krypton", "[2] /home/demo/notes"),
        ("calliope", "matches: 3"),
        ("chronos", "commit [feat]"),
        ("xenon", "=== delivered in order ==="),
        ("lithium", "exit:"),
        ("kairos", "=== task shares vs weight ==="),
    ];

    for (id, fragment) in expected_text {
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

    let expected_pixels: &[(&str, u32, Option<u32>)] = &[
        ("helios", 320, Some(240)),
        ("odyssey", 480, None),
        ("iris", 640, Some(400)),
        ("indium", 480, Some(320)),
    ];

    for &(id, exp_w, exp_h) in expected_pixels {
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
            Output::Pixels { w, h, rgba } => {
                assert_eq!(w, exp_w, "unexpected width for `{id}`");
                assert!(
                    w <= 640 && h <= 400,
                    "pixel output out of bounds for `{id}`: {w}x{h}"
                );
                if let Some(eh) = exp_h {
                    assert_eq!(h, eh, "unexpected height for `{id}`");
                } else {
                    assert!(h >= 160, "too short height for `{id}`: {h}");
                }
                assert_eq!(rgba.len(), (w * h * 4) as usize);
                let first = &rgba[0..4];
                assert!(
                    rgba.as_chunks::<4>()
                        .0
                        .iter()
                        .any(|px| px.as_slice() != first),
                    "demo `{id}` produced blank uniform frame"
                );
            }
            Output::Text(t) => panic!("expected pixel output for `{id}`, got text:\n{t}"),
        }
    }

    assert_eq!(
        demos::all().len(),
        expected_text.len() + expected_pixels.len()
    );
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
fn new_demos_default_key_lines() {
    let cobalt_out = expect_text("cobalt", demos::find("cobalt").unwrap().default_input);
    assert!(cobalt_out.contains("=== IR before optimize ==="));
    assert!(cobalt_out.contains("=== optimize stats ==="));
    assert!(cobalt_out.contains("answer = 42"));

    let elysium_out = expect_text("elysium", demos::find("elysium").unwrap().default_input);
    assert!(elysium_out.contains("MBC1 verified:"));
    assert!(elysium_out.contains("steps:"));
    assert!(elysium_out.contains("6! = 720"));

    let mercury_out = expect_text("mercury", demos::find("mercury").unwrap().default_input);
    assert!(mercury_out.lines().any(|l| l == "method: POST"));
    assert!(mercury_out.lines().any(|l| l == "target: /api/layers"));
    assert!(mercury_out.lines().any(|l| l == "version: HTTP/1.1"));
    assert!(mercury_out.contains("body length: 13 bytes"));
    assert!(mercury_out.contains("HTTP/1.1 200 OK"));

    let aurum_out = expect_text("aurum", demos::find("aurum").unwrap().default_input);
    assert!(aurum_out.contains("block #1"));
    assert!(aurum_out.contains("block #2"));
    assert!(aurum_out.contains("block #3"));
    assert!(aurum_out.contains("verified=true"));
    assert!(aurum_out.contains("alice: 890 (nonce 2)"));

    let argo_out = expect_text("argo", demos::find("argo").unwrap().default_input);
    assert!(argo_out.lines().any(|l| l == "# argo lock v1"));
    assert!(argo_out.lines().any(|l| l == "alpha = \"1.1.0\""));
    assert!(argo_out.lines().any(|l| l == "beta = \"1.1.0\""));
    assert!(argo_out.lines().any(|l| l == "gamma = \"2.0.0\""));

    let krypton_out = expect_text("krypton", demos::find("krypton").unwrap().default_input);
    assert!(krypton_out.contains("=== parse tree ==="));
    assert!(krypton_out.lines().any(|l| l == "  [0] echo"));
    assert!(krypton_out.lines().any(|l| l == "  [1] 7"));
    assert!(krypton_out.lines().any(|l| l == "  [2] /home/demo/notes"));
    assert!(krypton_out.lines().any(|l| l == "  [3] 42"));
    assert!(krypton_out.lines().any(|l| l == "  [4] fallback"));

    let calliope_out = expect_text("calliope", demos::find("calliope").unwrap().default_input);
    assert!(calliope_out.lines().any(|l| l == "2:11 calc(42)"));
    assert!(calliope_out.lines().any(|l| l == "3:11 step(7)"));
    assert!(calliope_out.lines().any(|l| l == "3:21 calc(108)"));
    assert!(calliope_out.lines().any(|l| l == "matches: 3"));

    let chronos_out = expect_text("chronos", demos::find("chronos").unwrap().default_input);
    assert!(chronos_out.contains("write README.md -> blob"));
    assert!(chronos_out.contains("commit [main]"));
    assert!(chronos_out.contains("branch feat ->"));
    assert!(chronos_out.contains("log (2 commits):"));

    let kairos_out = expect_text("kairos", demos::find("kairos").unwrap().default_input);
    assert!(kairos_out.contains("cpu 0:"));
    assert!(kairos_out.contains("cpu 1:"));
    assert!(kairos_out.contains("task alpha"));
    assert!(kairos_out.contains("task beta"));
    assert!(kairos_out.contains("task gamma"));

    let xenon_out = expect_text("xenon", demos::find("xenon").unwrap().default_input);
    assert!(xenon_out.contains("[stream 0] hello xenon"));
    assert!(xenon_out.contains("[stream 2] reliable over udp"));
    assert!(xenon_out.contains("[stream 4] encrypted frames"));
    assert!(xenon_out.contains("retransmits:"));
    assert!(xenon_out.contains("client:"));
    assert!(xenon_out.contains("server:"));

    let pandora_out = expect_text("pandora", demos::find("pandora").unwrap().default_input);
    assert!(pandora_out.contains("validated: ok"));
    assert!(pandora_out.contains("rootfs: /var/lib/pandora/alpine"));
    assert!(pandora_out.contains("hostname: monolith-box"));
    assert!(pandora_out.contains("controllers: +memory +pids +cpu"));
    assert!(pandora_out.contains("memory.max = 67108864"));

    let indium_demo = demos::find("indium").unwrap();
    match (indium_demo.run)(indium_demo.default_input).unwrap() {
        Output::Pixels { w, h, rgba } => {
            assert_eq!((w, h), (480, 320));
            assert!(
                rgba.as_chunks::<4>().0.contains(&[30, 110, 240, 255]),
                "indium output missing focus ring pixel color"
            );
        }
        Output::Text(_) => panic!("expected pixels for indium"),
    }
}

#[test]
fn bad_and_oversized_inputs_return_err_without_panicking() {
    let bad_cases: &[(&str, &str)] = &[
        ("iridium", "SELECT FROM ???"),
        ("cobalt", "fn main( { }"),
        ("elysium", "fn main() -> int { \"bad\" }"),
        ("mercury", "BAD REQUEST LINE\n\n"),
        ("helios", "ticks 10\nfn main() -> int { 1 }"),
        ("odyssey", ""),
        ("aurum", "alice unknown 10"),
        ("argo", "[index]\na 1.0.0\n[manifest]\n[package]\nname = \"x\"\nversion = \"0.1.0\"\n[dependencies]\na = \"^2.0\""),
        ("krypton", "if then fi"),
        ("iris", "open terminal\nwarp 99"),
        ("indium", "column\n  button \"A\"\nfocus 5"),
        ("calliope", "[unclosed\nhello"),
        ("chronos", "unknowncmd foo"),
        ("xenon", "loss 100\ndelay 10\nsend unreachable"),
        ("pandora", "rootfs relative/path\ncmd /bin/sh"),
        ("kairos", "cpus 0\ntask a 1 5"),
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
        forall(200, 7500 + idx as u64, |r| {
            let len = r.below(2049);
            let raw = r.bytes(len);
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
