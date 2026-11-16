#![cfg(feature = "std")]
#![allow(missing_docs, clippy::unwrap_used)]
// printf width/precision: honored up to 1 MiB, an error (exit 1) beyond it, never silently cut.

use std::process::{Command, Output};

fn krypton_c(script: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_krypton"))
        .args(["-c", script])
        .output()
        .expect("failed to execute krypton")
}

#[test]
fn printf_width_20000_is_not_capped_at_4096() {
    let out = krypton_c("printf '%20000s' ''");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout.len(), 20000);
    assert!(out.stdout.iter().all(|&b| b == b' '));
}

#[test]
fn printf_width_over_1_mib_is_an_error_not_truncation() {
    let out = krypton_c("printf '%1048577s' ''");
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("1 MiB"), "{err}");
}
