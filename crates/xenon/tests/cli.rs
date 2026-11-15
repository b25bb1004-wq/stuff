#![allow(missing_docs)]
// CLI integration tests for xenon tool

#[test]
fn test_help() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xenon"))
        .arg("--help")
        .output()
        .expect("failed to execute xenon --help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
}

#[test]
fn test_version() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xenon"))
        .arg("--version")
        .output()
        .expect("failed to execute xenon --version");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim() == "xenon 0.1.0");
}

#[test]
fn test_unknown_subcommand() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xenon"))
        .arg("bogus")
        .output()
        .expect("failed to execute xenon bogus");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("xenon: unknown subcommand"));
}

#[test]
fn test_usage_error() {
    // keygen should have no extra arguments
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xenon"))
        .args(["keygen", "extra"])
        .output()
        .expect("failed to execute xenon keygen extra");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("xenon: usage:"));
}

#[test]
fn test_runtime_error() {
    // recv with invalid secret hex (not 64 chars) triggers runtime error
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xenon"))
        .args(["recv", "127.0.0.1:9000", "deadbeef", "out.bin"])
        .output()
        .expect("failed to execute xenon recv");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("xenon: expected 64 hex characters"));
}
