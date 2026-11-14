//! Builtin execution tests for `crust::exec::Shell`.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use crust::exec::Shell;

static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn make_temp_dir(prefix: &str) -> PathBuf {
    let id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("crust_test_{prefix}_{}_{id}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_script(src: &str, stdin_data: &[u8]) -> (String, String, i32, Option<i32>) {
    let mut sh = Shell::new();
    let mut stdin = Cursor::new(stdin_data);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let st = sh.run(src, &mut stdin, &mut stdout, &mut stderr);
    (
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
        st,
        sh.exited(),
    )
}

#[test]
fn builtin_table_scripts() {
    let cases: &[(&str, &str, &str, i32)] = &[
        // Basic & simple commands
        ("empty", "", "", 0),
        ("true_cmd", "true", "", 0),
        ("false_cmd", "false", "", 1),
        ("colon_cmd", ":", "", 0),
        ("echo_basic", "echo hello world", "hello world\n", 0),
        ("echo_n", "echo -n hello", "hello", 0),
        ("echo_no_e", "echo -e hi", "-e hi\n", 0),
        ("vars_assign", "x=42; echo $x", "42\n", 0),
        ("temp_assign_builtin", "X=temp eval 'echo $X'; echo after:$X", "temp\nafter:\n", 0),
        // Logical operators & pipelines
        ("and_true", "true && echo ok", "ok\n", 0),
        ("and_false", "false && echo no", "", 1),
        ("or_false", "false || echo ok", "ok\n", 0),
        ("or_true", "true || echo no", "", 0),
        ("bang_true", "! true", "", 1),
        ("bang_false", "! false", "", 0),
        ("status_var", "false; echo $?", "1\n", 0),
        // Control flow: if / elif / else
        ("if_true", "if true; then echo yes; fi", "yes\n", 0),
        ("if_false_no_else", "if false; then echo no; fi", "", 0),
        ("if_elif_else", "if false; then echo 1; elif true; then echo 2; else echo 3; fi", "2\n", 0),
        ("if_else", "if false; then echo 1; else echo 2; fi", "2\n", 0),
        // Loops: while / until / for
        (
            "while_loop",
            "i=0; while [ $i -lt 3 ]; do echo $i; i=$((i+1)); done",
            "0\n1\n2\n",
            0,
        ),
        (
            "until_loop",
            "i=0; until [ $i -ge 3 ]; do echo $i; i=$((i+1)); done",
            "0\n1\n2\n",
            0,
        ),
        ("while_never", "while false; do echo no; done", "", 0),
        ("for_list", "for x in a b c; do echo $x; done", "a\nb\nc\n", 0),
        ("for_positional", "set -- p q; for x; do echo $x; done", "p\nq\n", 0),
        (
            "break_1",
            "for x in 1 2 3; do if [ $x -eq 2 ]; then break; fi; echo $x; done",
            "1\n",
            0,
        ),
        (
            "break_2",
            "for i in 1 2; do for j in a b c; do if [ $j = b ]; then break 2; fi; echo $i$j; done; done",
            "1a\n",
            0,
        ),
        (
            "continue_1",
            "for x in 1 2 3; do if [ $x -eq 2 ]; then continue; fi; echo $x; done",
            "1\n3\n",
            0,
        ),
        (
            "continue_2",
            "for i in 1 2; do for j in a b; do echo $i$j; continue 2; echo nope; done; echo after; done",
            "1a\n2a\n",
            0,
        ),
        // Case
        (
            "case_glob",
            "for x in foo.rs bar.txt baz; do case $x in *.rs) echo rs;; *.txt) echo txt;; *) echo other;; esac; done",
            "rs\ntxt\nother\n",
            0,
        ),
        ("case_no_match", "case abc in xyz) echo no;; esac", "", 0),
        // Subshell & brace group
        ("group_mutates", "x=1; { x=2; echo $x; }; echo $x", "2\n2\n", 0),
        ("subshell_isolated", "x=1; (x=2; echo $x); echo $x", "2\n1\n", 0),
        ("subshell_status", "(exit 7); echo $?", "7\n", 0),
        // Functions, recursion, local, return
        ("func_basic", "f() { echo hi $1 $2 $#; }; f a b", "hi a b 2\n", 0),
        ("func_return", "f() { return 5; echo no; }; f; echo $?", "5\n", 0),
        (
            "func_local",
            "x=outer; f() { local x=inner; echo $x; }; f; echo $x",
            "inner\nouter\n",
            0,
        ),
        (
            "func_recursion",
            "fact() { if [ $1 -le 1 ]; then echo 1; else local n=$(($1 - 1)); local r=$(fact $n); echo $(($1 * r)); fi; }; fact 5",
            "120\n",
            0,
        ),
        // set -e cases
        ("errexit_stops", "set -e; echo a; false; echo b", "a\n", 1),
        ("errexit_if_cond", "set -e; if false; then echo no; fi; echo ok", "ok\n", 0),
        ("errexit_and_list_skip", "set -e; false && true; echo ok", "ok\n", 0),
        ("errexit_and_list_fail", "set -e; true && false; echo no", "", 1),
        ("errexit_bang", "set -e; ! false; ! true; echo ok", "ok\n", 0),
        ("errexit_subshell", "set -e; (false; echo no); echo also_no", "", 1),
        // set --, shift, unset, export
        ("set_positional", "set -- a b c; echo $# $1 $3; shift 2; echo $# $1", "3 a c\n1 c\n", 0),
        ("unset_var", "x=1; unset x; echo \"[$x]\"", "[]\n", 0),
        ("unset_func", "f() { echo hi; }; unset -f f; type f >/dev/null 2>&1", "", 1),
        // printf
        ("printf_basic", "printf '%s:%d:%x:%o:%%\\n' hi 255 255 8", "hi:255:ff:10:%\n", 0),
        ("printf_width_left", "printf '[%5s][%-5s]\\n' ab cd", "[   ab][cd   ]\n", 0),
        ("printf_reuse", "printf '%s-' a b c; echo", "a-b-c-\n", 0),
        ("printf_b_escape", "printf '%b' 'a\\nb\\tc\\nd'", "a\nb\tc\nd", 0),
        ("printf_b_stop", "printf '%b' 'hi\\cthere'", "hi", 0),
        ("printf_char", "printf '%c%c\\n' alpha beta", "ab\n", 0),
        // test / [
        ("test_str", "test -n abc && [ -z '' ] && [ a = a ] && [ a != b ]", "", 0),
        ("test_int", "[ 10 -eq 10 ] && [ 9 -ne 10 ] && [ 2 -lt 3 ] && [ 3 -le 3 ] && [ 5 -gt 4 ] && [ 5 -ge 5 ]", "", 0),
        ("test_logic", "[ ! -z hi -a '(' 1 -eq 1 -o 2 -eq 3 ')' ]", "", 0),
        ("test_empty", "test", "", 1),
        ("test_one", "test non_empty", "", 0),
        // eval & command substitution
        ("eval_basic", "cmd='echo hello'; eval $cmd", "hello\n", 0),
        ("cmd_subst", "x=$(echo a; echo b); echo \"[$x]\"", "[a\nb]\n", 0),
        // Here-docs
        ("heredoc_expanded", "x=world; read line <<EOF\nhello $x\nEOF\necho \"$line\"", "hello world\n", 0),
        ("heredoc_quoted", "x=world; read line <<'EOF'\nhello $x\nEOF\necho \"$line\"", "hello $x\n", 0),
        // Pipelines of builtins
        ("pipe_echo_read_isolated", "x=orig; echo new | read x; echo $x", "orig\n", 0),
        ("pipe_group_to_group", "{ echo 1; echo 2; } | { read a; read b; echo $b-$a; }", "2-1\n", 0),
        ("pipe_3_stages", "echo hello | { read w; echo $w world; } | { read a b; echo $b:$a; }", "world:hello\n", 0),
        // Background & + wait
        ("bg_and_wait", "(echo bg) & wait; echo done", "bg\ndone\n", 0),
        ("bg_status_wait", "(exit 4) & p=$!; wait $p; echo $?", "4\n", 0),
        // type builtin
        ("type_builtin", "type echo", "echo is a shell builtin\n", 0),
        ("type_func", "myfn() { :; }; type myfn", "myfn is a function\n", 0),
    ];

    assert!(
        cases.len() >= 60,
        "need at least 60 table cases, got {}",
        cases.len()
    );
    for (name, script, exp_out, exp_status) in cases {
        let (out, err, st, _) = run_script(script, b"");
        assert_eq!(
            out, *exp_out,
            "case `{name}` stdout mismatch (stderr={err:?})"
        );
        assert_eq!(
            st, *exp_status,
            "case `{name}` status mismatch (out={out:?}, stderr={err:?})"
        );
    }
}

#[test]
fn read_with_ifs_and_stdin() {
    let (out, _, st, _) = run_script(
        "IFS=: read a b c; echo \"$a|$b|$c\"",
        b"one:two:three:four\n",
    );
    assert_eq!(st, 0);
    assert_eq!(out, "one|two|three:four\n");

    let (out2, _, st2, _) = run_script("read -r x; echo \"$x\"", b"a\\nb\n");
    assert_eq!(st2, 0);
    assert_eq!(out2, "a\\nb\n");
}

#[test]
fn redirects_temp_files_and_fds() {
    let dir = make_temp_dir("redirs");
    let mut sh = Shell::new();
    sh.set_cwd(dir.clone());
    let mut stdin = Cursor::new(b"");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let script = r#"
echo first > out.txt
echo second >> out.txt
read a < out.txt
{ read a; read b; } < out.txt
echo "$a/$b"
echo overwrite >| out.txt
read c < out.txt
echo "$c"
"#;
    let st = sh.run(script, &mut stdin, &mut stdout, &mut stderr);
    assert_eq!(st, 0);
    assert_eq!(
        String::from_utf8_lossy(&stdout),
        "first/second\noverwrite\n"
    );

    // Swap stdout and stderr with 3>&1 1>&2 2>&3
    stdout.clear();
    stderr.clear();
    let st2 = sh.run(
        "{ echo to_out; echo to_err >&2; } 3>&1 1>&2 2>&3",
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(st2, 0);
    assert_eq!(String::from_utf8_lossy(&stdout), "to_err\n");
    assert_eq!(String::from_utf8_lossy(&stderr), "to_out\n");

    // Failed > returns status 1 and writes to stderr
    stdout.clear();
    stderr.clear();
    let st3 = sh.run(
        "echo x > nonexistent_dir/file.txt",
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(st3, 1);
    assert!(!stderr.is_empty());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dot_source_and_cd_pwd() {
    let dir = make_temp_dir("dot_cd");
    let sub = dir.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(dir.join("lib.sh"), "GREET=hello\nreturn 3\nGREET=never\n").unwrap();

    let mut sh = Shell::new();
    sh.set_cwd(dir.clone());
    let mut stdin = Cursor::new(b"");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let st = sh.run(
        ". ./lib.sh; echo $?; echo $GREET; cd sub; pwd; cd - >/dev/null",
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(st, 0);
    let out = String::from_utf8_lossy(&stdout);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "3");
    assert_eq!(lines[1], "hello");
    assert!(lines[2].ends_with("sub"), "pwd was {}", lines[2]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xtrace_and_exit_builtin() {
    let mut sh = Shell::new();
    let mut stdin = Cursor::new(b"");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let st = sh.run(
        "set -x; echo hi; exit 42; echo never",
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(st, 42);
    assert_eq!(sh.exited(), Some(42));
    assert_eq!(String::from_utf8_lossy(&stdout), "hi\n");
    let err = String::from_utf8_lossy(&stderr);
    assert!(err.contains("+ echo hi"), "stderr was {err:?}");
}

#[test]
fn jobs_lists_running_and_done() {
    let (out, _, st, _) = run_script("(echo a) & wait; jobs", b"");
    assert_eq!(st, 0);
    assert_eq!(out, "a\n");
}
