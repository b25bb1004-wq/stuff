#![allow(missing_docs)]
// Regression tests for krypton-2 fixes (reports/sim/p2-shell.md rows R5, R6, R7, R19 and daily builtins).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("krypton-sim2-{tag}-{}", std::process::id()));
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

fn run_c(script: &str) -> Output {
    // One folder per call: tests run in parallel and temp_dir() clears its folder first.
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = temp_dir(&format!("c{n}"));
    krypton_c(script, &dir)
}

// R19: aliases

#[test]
fn r19_alias_expands_first_word_with_arguments() {
    let o = run_c("alias ll='echo listed'; ll x");
    assert_eq!(out_text(&o), "listed x\n");
}

#[test]
fn r19_alias_listing_and_single_lookup_use_bash_form() {
    let o = run_c("alias a='echo hi'; alias; alias a");
    assert_eq!(out_text(&o), "alias a='echo hi'\nalias a='echo hi'\n");
}

#[test]
fn r19_alias_value_quotes_are_escaped_in_listing() {
    let o = run_c("alias q=\"echo it\\'s\"; alias q");
    // Inside double quotes \' is not an escape (POSIX), so the value keeps its backslash, as in bash.
    assert_eq!(out_text(&o), "alias q='echo it\\'\\''s'\n");
}

#[test]
fn r19_unalias_removes_and_missing_name_fails() {
    let o = run_c("alias a='echo x'; unalias a; a; echo rc=$?; unalias a; echo rc=$?");
    assert!(out_text(&o).contains("rc=127"), "stdout: {}", out_text(&o));
    assert!(out_text(&o).ends_with("rc=1\n"), "stdout: {}", out_text(&o));
    assert!(err_text(&o).contains("unalias: a: not found"));
}

#[test]
fn r19_unalias_all_clears_every_alias() {
    let o = run_c("alias a=echo; alias b=echo; unalias -a; alias; echo done");
    assert_eq!(out_text(&o), "done\n");
}

#[test]
fn r19_self_referencing_alias_stops_at_the_guard() {
    // `echo` expands to `echo [`; the second `echo` is not expanded again.
    let o = run_c("alias echo='echo ['; echo hi");
    assert_eq!(out_text(&o), "[ hi\n");
}

#[test]
fn r19_alias_chain_follows_other_aliases() {
    let o = run_c("alias a='b x'; alias b='echo'; a y");
    assert_eq!(out_text(&o), "x y\n");
}

#[test]
fn r19_alias_with_list_body_runs_when_used_alone() {
    let o = run_c("alias both='echo 1; echo 2'; both");
    assert_eq!(out_text(&o), "1\n2\n");
}

#[test]
fn r19_alias_with_list_body_rejects_arguments() {
    let o = run_c("alias both='echo 1; echo 2'; both extra; echo rc=$?");
    assert!(
        err_text(&o).contains("takes no arguments"),
        "stderr: {}",
        err_text(&o)
    );
    assert!(out_text(&o).ends_with("rc=1\n"));
}

#[test]
fn r19_alias_applies_in_a_script_file() {
    let dir = temp_dir("script");
    fs::write(dir.join("a.sh"), "alias hi='echo hello'\nhi there\n").expect("write script");
    let o = Command::new(env!("CARGO_BIN_EXE_krypton"))
        .arg("a.sh")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("run krypton");
    assert_eq!(out_text(&o), "hello there\n");
}

// R5: printf conversions

#[test]
fn r5_printf_width_and_precision_float() {
    let o = run_c("printf '%5.2f|%s\\n' 3.14159 ok");
    assert_eq!(out_text(&o), " 3.14|ok\n");
}

#[test]
fn r5_printf_exponent_form_has_sign_and_two_digits() {
    let o = run_c("printf '%e %E\\n' 1234.5 0.00012");
    assert_eq!(out_text(&o), "1.234500e+03 1.200000E-04\n");
}

#[test]
fn r5_printf_general_form_chooses_fixed_or_exponent() {
    let o = run_c("printf '%g %g %g %g\\n' 0.0001 100000 1000000 2.5");
    assert_eq!(out_text(&o), "0.0001 100000 1e+06 2.5\n");
}

#[test]
fn r5_printf_flags_on_integers() {
    let o = run_c("printf '%+d|%05d|%-4d|%x|%X|%#o|%#x|% d\\n' 5 42 7 255 255 8 255 3");
    assert_eq!(out_text(&o), "+5|00042|7   |ff|FF|010|0xff| 3\n");
}

#[test]
fn r5_printf_star_width_and_integer_precision() {
    let o = run_c("printf '%*d|%.3d|%-*s|\\n' 4 7 5 3 ab");
    assert_eq!(out_text(&o), "   7|005|ab |\n");
}

#[test]
fn r5_printf_string_precision_truncates() {
    let o = run_c("printf '%.2s|%c%%\\n' abcdef hi");
    assert_eq!(out_text(&o), "ab|h%\n");
}

#[test]
fn r5_printf_unsigned_prints_two_complement() {
    let o = run_c("printf '%u\\n' -1");
    assert_eq!(out_text(&o), "18446744073709551615\n");
}

#[test]
fn r5_printf_bad_number_errors_and_exits_one() {
    let o = run_c("printf '%d|' abc; echo rc=$?");
    assert_eq!(out_text(&o), "0|rc=1\n", "stdout: {}", out_text(&o));
    assert!(
        err_text(&o).contains("printf: abc: invalid number"),
        "stderr: {}",
        err_text(&o)
    );
}

#[test]
fn r5_printf_bad_float_reports_and_status_is_one() {
    let o = run_c("printf '%f\\n' xyz; echo rc=$?");
    assert!(out_text(&o).ends_with("rc=1\n"), "stdout: {}", out_text(&o));
    assert!(err_text(&o).contains("printf: xyz: invalid number"));
}

#[test]
fn r5_printf_b_with_width_then_stop() {
    let o = run_c("printf '%5b|' 'hi\\cthere'");
    assert_eq!(out_text(&o), "   hi");
}

// R6: $'...' ANSI-C quoting

#[test]
fn r6_ansi_c_tab_and_newline() {
    let o = run_c("printf '%s\\n' $'a\\tb'");
    assert_eq!(out_text(&o), "a\tb\n");
}

#[test]
fn r6_ansi_c_hex_octal_unicode_and_escapes() {
    let o = run_c("printf '%s\\n' $'\\x41\\101\\u0042\\\\\\x27\\\"'");
    assert_eq!(out_text(&o), "AAB\\'\"\n");
}

#[test]
fn r6_ansi_c_unknown_escape_keeps_backslash() {
    let o = run_c("printf '%s\\n' $'a\\qb'");
    assert_eq!(out_text(&o), "a\\qb\n");
}

#[test]
fn r6_ansi_c_escaped_single_quote_does_not_end_string() {
    let o = run_c("printf '%s\\n' $'it\\'s'");
    assert_eq!(out_text(&o), "it's\n");
}

#[test]
fn r6_ansi_c_result_is_not_split_or_globbed() {
    let o = run_c("printf '[%s]\\n' $'a b'");
    assert_eq!(out_text(&o), "[a b]\n");
}

#[test]
fn r6_dollar_single_quote_inside_double_quotes_is_literal() {
    let o = run_c("printf '%s\\n' \"$'x'\"");
    assert_eq!(out_text(&o), "$'x'\n");
}

// R7: kill -0 on the running shell

#[test]
fn r7_kill_zero_on_own_pid_succeeds() {
    let o = run_c("kill -0 $$; echo rc=$?");
    assert_eq!(out_text(&o), "rc=0\n");
}

#[test]
fn r7_kill_zero_with_signal_option_succeeds() {
    let o = run_c("kill -s 0 $$ && echo alive");
    assert_eq!(out_text(&o), "alive\n");
}

// umask and hash

#[test]
fn umask_prints_default_then_stores_new_value() {
    let o = run_c("umask; umask 077; umask");
    assert_eq!(out_text(&o), "0022\n0077\n");
}

#[test]
fn umask_rejects_a_bad_mode() {
    let o = run_c("umask 999; echo rc=$?");
    assert!(out_text(&o).ends_with("rc=1\n"), "stdout: {}", out_text(&o));
    assert!(err_text(&o).contains("umask: 999: invalid mode"));
}

#[test]
fn hash_r_is_a_no_op_success() {
    let o = run_c("hash -r; echo rc=$?");
    assert_eq!(out_text(&o), "rc=0\n");
}
