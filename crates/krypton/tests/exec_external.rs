//! External command and binary CLI tests for `krypton`.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use krypton::exec::Shell;

static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn make_temp_dir(prefix: &str) -> PathBuf {
    let id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("crust_ext_{prefix}_{}_{id}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn crust_bin() -> &'static str {
    env!("CARGO_BIN_EXE_krypton")
}

#[test]
fn external_crust_pipelines_redirects_and_exit_codes() {
    let dir = make_temp_dir("pipe_redir");
    let mut sh = Shell::new();
    sh.set_cwd(dir.clone());
    let mut stdin = Cursor::new(b"");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let bin = crust_bin().replace('\\', "/");

    // Exit codes 0, 1, 127
    let st0 = sh.run(
        &format!("'{bin}' -c 'exit 0'"),
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(st0, 0);

    let st1 = sh.run(
        &format!("'{bin}' -c 'exit 1'"),
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(st1, 1);

    let st127 = sh.run(
        "definitely_nonexistent_cmd_xyz_123",
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(st127, 127);

    // External in pipeline + redirect to/from file
    stdout.clear();
    stderr.clear();
    let script = format!(
        "'{bin}' -c 'echo from_ext' | '{bin}' -c 'read x; echo got:$x' > ext.txt; read y < ext.txt; echo \"$y\""
    );
    let st_pipe = sh.run(&script, &mut stdin, &mut stdout, &mut stderr);
    assert_eq!(st_pipe, 0, "stderr={}", String::from_utf8_lossy(&stderr));
    assert_eq!(String::from_utf8_lossy(&stdout), "got:from_ext\n");

    // External stderr redirect
    stdout.clear();
    stderr.clear();
    let script_err =
        format!("'{bin}' -c 'echo oops >&2' 2> err.txt; read e < err.txt; echo \"err=$e\"");
    let st_err = sh.run(&script_err, &mut stdin, &mut stdout, &mut stderr);
    assert_eq!(st_err, 0);
    assert_eq!(String::from_utf8_lossy(&stdout), "err=oops\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn binary_c_file_help_and_interactive_pipe() {
    let dir = make_temp_dir("bin_cli");

    // --help mentions terminal job control is out of scope
    let help = Command::new(crust_bin()).arg("--help").output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(
        help_text.to_lowercase().contains("job control"),
        "help was: {help_text}"
    );

    // -c
    let c_out = Command::new(crust_bin())
        .args(["-c", "echo hi; exit 3"])
        .output()
        .unwrap();
    assert_eq!(c_out.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&c_out.stdout), "hi\n");

    // FILE [ARGS]
    let script_path = dir.join("test.sh");
    fs::write(&script_path, "echo \"$1:$2:$#\"\n").unwrap();
    let f_out = Command::new(crust_bin())
        .arg(&script_path)
        .args(["alpha", "beta"])
        .output()
        .unwrap();
    assert_eq!(f_out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&f_out.stdout), "alpha:beta:2\n");

    // Interactive session fed through a pipe with continuation prompt `> `
    let mut child = Command::new(crust_bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut cstdin = child.stdin.take().unwrap();
        cstdin
            .write_all(b"if true; then\necho multi\nfi\nexit 5\n")
            .unwrap();
    }
    let inter = child.wait_with_output().unwrap();
    assert_eq!(inter.status.code(), Some(5));
    let inter_out = String::from_utf8_lossy(&inter.stdout);
    assert!(
        inter_out.contains("$ "),
        "missing `$ ` prompt: {inter_out:?}"
    );
    assert!(
        inter_out.contains("> "),
        "missing `> ` prompt: {inter_out:?}"
    );
    assert!(
        inter_out.contains("multi\n"),
        "missing output: {inter_out:?}"
    );

    let _ = fs::remove_dir_all(&dir);
}
