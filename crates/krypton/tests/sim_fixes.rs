#![allow(missing_docs)]
// One regression test per krypton-1 fix (reports/sim/p2-shell.md rows R3, R8-R13, R15, R21).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("krypton-sim1-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn krypton_c(script: &str, dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_krypton"))
        .args(["-c", script])
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("run krypton")
}

/// stdout with CRLF folded to LF (children on Windows print CRLF).
fn out_text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n")
}

fn err_text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).replace("\r\n", "\n")
}

#[test]
fn r15_piped_stdin_writes_no_prompt() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_krypton"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn krypton");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"echo a\n")
        .expect("write script");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out_text(&out), "a\n");
}

#[test]
fn r9_crlf_script_file_and_c_text_read_as_lf() {
    let dir = temp_dir("crlf");
    fs::write(dir.join("c.sh"), "x=5\r\necho \"x=$x\"\r\n").expect("write script");
    let out = Command::new(env!("CARGO_BIN_EXE_krypton"))
        .arg("c.sh")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("run krypton");
    assert_eq!(out_text(&out), "x=5\n");
    let out = krypton_c("x=5\r\necho \"x=$x\"\r\n", &dir);
    assert_eq!(out_text(&out), "x=5\n");
}

#[test]
fn r12_dollar_zero_is_the_script_path_or_arg0() {
    let dir = temp_dir("dollar0");
    fs::write(dir.join("s.sh"), "echo \"$0\"\n").expect("write script");
    let out = Command::new(env!("CARGO_BIN_EXE_krypton"))
        .arg("s.sh")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("run krypton");
    assert_eq!(out_text(&out), "s.sh\n");
    assert_eq!(out_text(&krypton_c("echo $0", &dir)), "krypton\n");
    let out = Command::new(env!("CARGO_BIN_EXE_krypton"))
        .args(["-c", "echo $0 $1", "myname", "arg"])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("run krypton");
    assert_eq!(out_text(&out), "myname arg\n");
}

#[test]
fn r13_syntax_error_names_file_and_line() {
    let dir = temp_dir("syntax");
    fs::write(dir.join("bad2.sh"), "echo one\necho two )\n").expect("write script");
    let out = Command::new(env!("CARGO_BIN_EXE_krypton"))
        .arg("bad2.sh")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("run krypton");
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        err_text(&out).trim_end(),
        "bad2.sh: line 2: syntax error near unexpected token ')'"
    );
    assert_eq!(
        out_text(&out),
        "",
        "a script with a syntax error runs nothing"
    );
}

#[test]
fn r8_pipefail_and_unknown_set_option() {
    let dir = temp_dir("pipefail");
    assert_eq!(
        out_text(&krypton_c(
            "set -o pipefail; false | true; echo rc=$?",
            &dir
        )),
        "rc=1\n"
    );
    assert_eq!(
        out_text(&krypton_c(
            "set -o pipefail; set +o pipefail; false | true; echo rc=$?",
            &dir
        )),
        "rc=0\n"
    );
    assert!(out_text(&krypton_c("set -o", &dir)).contains("pipefail"));
    let out = krypton_c("set -z; echo after", &dir);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(out_text(&out), "");
}

#[test]
fn r10_command_v_and_command_skips_functions() {
    let dir = temp_dir("command");
    let out = krypton_c("command -v echo; echo rc=$?", &dir);
    assert_eq!(out_text(&out), "echo\nrc=0\n");
    let out = krypton_c("command -v no_such_krypton_zz; echo rc=$?", &dir);
    assert_eq!(out_text(&out), "rc=1\n");
    let k = env!("CARGO_BIN_EXE_krypton");
    let out = krypton_c(&format!("command -v '{k}'"), &dir);
    assert_eq!(out_text(&out).trim(), k);
    let out = krypton_c("echo() { printf 'func\\n'; }; command echo hi", &dir);
    assert_eq!(out_text(&out), "hi\n");
}

#[test]
fn r11_exit_trap_runs_once_at_shell_exit() {
    let dir = temp_dir("trap");
    assert_eq!(
        out_text(&krypton_c("trap 'echo bye' EXIT; echo body", &dir)),
        "body\nbye\n"
    );
    let out = krypton_c("trap 'echo bye' EXIT; exit 3", &dir);
    assert_eq!(
        (out_text(&out).as_str(), out.status.code()),
        ("bye\n", Some(3))
    );
    let out = krypton_c("set -e; trap 'echo bye' EXIT; false; echo no", &dir);
    assert_eq!(
        (out_text(&out).as_str(), out.status.code()),
        ("bye\n", Some(1))
    );
    assert_eq!(
        out_text(&krypton_c(
            "trap 'echo bye' EXIT; trap - EXIT; echo body",
            &dir
        )),
        "body\n"
    );
    // INT and TERM are accepted and stored but never run here.
    assert_eq!(
        out_text(&krypton_c("trap 'echo x' INT TERM; echo ok", &dir)),
        "ok\n"
    );
}

#[test]
fn r3_exec_redirections_change_the_shell_fds() {
    let dir = temp_dir("exec");
    let out = krypton_c("exec 3>fx.txt; echo hi >&3; echo rc=$?", &dir);
    assert_eq!(out_text(&out), "rc=0\n");
    assert_eq!(
        fs::read_to_string(dir.join("fx.txt")).unwrap_or_default(),
        "hi\n"
    );
    let out = krypton_c("exec echo done; echo never", &dir);
    assert_eq!(
        (out_text(&out).as_str(), out.status.code()),
        ("done\n", Some(0))
    );
    let out = krypton_c("exec false; echo never", &dir);
    assert_eq!((out_text(&out).as_str(), out.status.code()), ("", Some(1)));
}

#[cfg(unix)]
#[test]
fn r21_child_argv0_is_the_name_as_typed() {
    let dir = temp_dir("argv0");
    let out = krypton_c("cat /no/such_krypton_zz", &dir);
    let err = err_text(&out);
    assert!(err.contains("cat: /no/such_krypton_zz"), "{err}");
    assert!(!err.contains("/usr/bin/cat"), "{err}");
}
