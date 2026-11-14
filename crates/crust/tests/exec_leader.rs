//! Leader check for T57: builtin-only scripts (fixed corpus + generated) against real
//! `bash --posix` on stdout and exit status, the crust binary as external command, binary modes.
#![cfg(feature = "std")]
#![allow(clippy::unwrap_used)]
use assay::Rng;
use crust::exec::Shell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const FIXED: &[&str] = &[
    "echo hello world",
    "x=5; y=$((x*3+1)); echo $y",
    "if [ 3 -gt 2 ]; then echo yes; else echo no; fi",
    "i=0; while [ $i -lt 5 ]; do printf '%d,' $i; i=$((i+1)); done; echo",
    "for w in a b c; do echo \"<$w>\"; done",
    "case abc in a*) echo A;; *) echo other;; esac",
    "f() { echo \"f:$1:$#\"; return 3; }; f x y; echo $?",
    "fact() { if [ $1 -le 1 ]; then echo 1; else echo $(( $1 * $(fact $(( $1 - 1 ))) )); fi; }; fact 6",
    "false; echo $?; true; echo $?; ! true; echo $?",
    "set -e; true; false; echo unreachable",
    "set -e; if false; then :; fi; false || echo ok; echo end",
    "set -e; f() { false; echo inside; }; f || echo caught",
    "x=1; (x=2; echo $x); echo $x",
    "echo one > t.txt; echo two >> t.txt; while read l; do echo \"[$l]\"; done < t.txt",
    "v=W\nwhile read l; do echo \"$l\"; done <<EOF\nhi $v\nEOF",
    "v=W\nwhile read l; do echo \"$l\"; done <<'EOF'\nhi $v\nEOF",
    "IFS=: read a b c <<EOF\nx:y:z:w\nEOF\necho \"$a|$b|$c\"",
    "read a b <<EOF\n  lead   x y z\nEOF\necho \"[$a][$b]\"",
    "printf '%5s|%-5s|%x|%o|%c|%%\\n' ab cd 255 8 xyz",
    "printf '%s-' a b c; echo",
    "printf '%b\\n' 'a\\tb'",
    "printf '%3d|%i\\n' 7 -4",
    "for e in '-z \"\"' '-n x' 'a = a' 'a != b' '1 -eq 1' '2 -ne 2' '! -z x' '-z x -o -n y' '-n x -a -z y' '\\( a = a \\)'; do eval \"test $e\" && echo T || echo F; done",
    "echo z > f1; [ -f f1 ] && echo F1; [ -d f1 ] || echo notdir; [ -e nope ] || echo none; [ -s f1 ] && echo nonempty; [ -d sub ] && echo dir",
    "for i in 1 2 3; do for j in a b c; do if [ $j = b ]; then continue 2; fi; if [ $i = 3 ]; then break 2; fi; printf $i$j; done; done; echo",
    "eval 'x=4; echo $((x+1))'",
    "echo 'y=sourced' > s.sh; . ./s.sh; echo $y",
    "echo a | read x; echo \"[$x]\"",
    "echo hi | { read x; echo got $x; }",
    "{ echo x > bg.txt; } & wait $!; echo $?; read l < bg.txt; echo $l",
    "(exit 7) & wait $!; echo $?",
    "set -- a \"b c\" d; echo $#; shift; echo \"$1\"; shift 2; echo $#",
    "set -u; x=1; echo $x; echo ${y:-def}",
    "exit 3",
    "echo a; exit; echo b",
    "false; exit",
    "x=outer; f() { local x=inner; echo $x; }; f; echo $x",
    "x=1; unset x; echo \"[${x-unset}]\"",
    "n=0; until [ $n -ge 3 ]; do n=$((n+1)); done; echo $n",
    "echo -n ab; echo cd",
    "x=1 y=2; echo $((x<y)) $((x==y)) $((7%3)) $((-7/2))",
    "{ echo out; echo err >&2; } 2>e.txt >o.txt; read a < o.txt; read b < e.txt; echo $a $b",
    "echo a 2>e2.txt 1>&2; read z < e2.txt; echo \"[$z]\"",
    "echo a 1>&2 2>e3.txt; read z < e3.txt; echo \"[$z]\"",
    "x=$(exit 3); echo $?",
    "case \"x y\" in \"x y\") echo quoted;; esac",
    "case 5 in [0-9]) echo digit;; esac",
    "while true; do break; done; echo $?",
    "f() { return; }; false; f; echo $?",
    "echo $((1 << 4)) $((0x10)) $((010))",
    "printf '%s\\n' \"$(printf 'a\\n\\n\\n')\"",
    "x='a  b'; echo $x; echo \"$x\"",
    "true | false; echo $?; false | true; echo $?",
    "! false | false; echo $?",
    "test 1 -lt 2 -a 3 -gt 2; echo $?",
    "[ abc ]; echo $?; [ \"\" ]; echo $?",
    "y=$(f() { echo fn; }; f); echo $y",
    "p=$(pwd); (cd sub); [ \"$(pwd)\" = \"$p\" ] && echo same; cd sub; [ \"$(pwd)\" = \"$p\" ] || echo moved",
    "v=$(printf '%s' abc); echo ${#v}",
    "set -x; echo t",
    "cd nosuch 2>e.txt; echo $?",
    "echo a > nodir/x 2>e.txt; echo $?",
    "x=3; while [ $x -gt 0 ]; do x=$((x-1)); [ $x = 1 ] && continue; echo $x; done",
    "export A=1; echo $A; unset A; echo \"[${A-gone}]\"",
    "set -e; x=$(false; echo no) || echo subfail; echo after",
    "f() { echo \"$@\"; echo $#; }; f \"a b\" c; set -- 1 2; f \"$@\"",
    "set -u; echo a; echo $nope; echo b",
    "set -u; echo ${nope:-d}; f() { echo $1; }; f; echo no",
    "set -u; echo \"$@\"; echo ok",
    "set -e; f() { false; echo in; }; if f; then echo t; fi; ! f; echo $?",
    "set -e; (false; echo sub) && echo and; echo end",
    "set -e; g() { false; }; g; echo unreachable",
];

fn gen(r: &mut Rng) -> String {
    let mut s = String::new();
    if r.chance(1, 5) {
        s.push_str("set -e\n");
    }
    s.push_str("f() { echo \"f$1\"; return $(( $1 & 3 )); }\n");
    s.push_str(&format!(
        "a={}; b={}; c={}\n",
        r.below(20),
        r.below(20) as i64 - 10,
        r.below(50)
    ));
    for _ in 0..3 + r.below(6) {
        let st = match r.below(11) {
            0 => format!("a=$((a {} b))", r.pick(&["+", "-", "* 2 -"])),
            1 => "if [ $a -lt $b ]; then echo L$a; else echo G$b; fi".to_string(),
            2 => format!("printf '%d:%s|' $c {}", r.pick(&["x", "yy", "'z z'"])),
            3 => "for i in 1 2 3; do c=$((c+i)); done".to_string(),
            4 => "case $a in *1*) echo one;; -*) echo neg;; *) echo n$a;; esac".to_string(),
            5 => "f $a; echo s$?".to_string(),
            6 => "x=$(echo $b | { read q; echo $((q+1)); }); echo x$x".to_string(),
            7 => "(a=0; echo sub$a); echo out$a".to_string(),
            8 => "[ $((a % 2)) -eq 0 ] && echo even || echo odd".to_string(),
            9 => "for i in 1 2 3 4; do [ $i -gt $((c % 4)) ] && break; printf $i; done; echo"
                .to_string(),
            _ => format!(
                "while [ $c -gt {} ]; do c=$((c-7)); done; echo c$c",
                r.below(30)
            ),
        };
        s.push_str(&st);
        s.push('\n');
    }
    s
}

fn root(name: &str) -> PathBuf {
    let r = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&r);
    std::fs::create_dir_all(&r).unwrap();
    r
}

fn case_dir(root: &Path, i: usize) -> PathBuf {
    let d = root.join(format!("d{i}"));
    std::fs::create_dir_all(d.join("sub")).unwrap();
    d
}

/// One script in a fresh crust Shell, with a watchdog; None on timeout.
fn crust(dir: PathBuf, src: String) -> Option<(String, i32)> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut sh = Shell::new();
        sh.set_cwd(dir);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let st = sh.run(&src, &mut std::io::empty(), &mut out, &mut err);
        let _ = tx.send((String::from_utf8_lossy(&out).into_owned(), st));
    });
    rx.recv_timeout(Duration::from_secs(20)).ok()
}

/// All scripts through one `bash --posix -s` fed on stdin, each in a subshell in its own dir.
fn bash(root: &Path, scripts: &[String]) -> Vec<(String, i32)> {
    let mut s = String::from("export LC_ALL=C\n");
    for (i, sc) in scripts.iter().enumerate() {
        s.push_str(&format!(
            "printf '@@B{i}\\n'\n( cd d{i} || exit 99\n{sc}\n)\nst=$?\nprintf '\\n@@E{i} %d\\n' \"$st\"\n"
        ));
    }
    let mut ch = Command::new("bash")
        .args(["--posix", "-s"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    ch.stdin.take().unwrap().write_all(s.as_bytes()).unwrap();
    let text = String::from_utf8_lossy(&ch.wait_with_output().unwrap().stdout).into_owned();
    (0..scripts.len())
        .map(|i| {
            let (b, e) = (format!("@@B{i}\n"), format!("\n@@E{i} "));
            let start = text
                .find(&b)
                .unwrap_or_else(|| panic!("bash lost case {i}"))
                + b.len();
            let end = start + text[start..].find(&e).unwrap();
            let st = text[end + e.len()..].lines().next().unwrap().trim();
            (text[start..end].to_string(), st.parse().unwrap())
        })
        .collect()
}

fn differential(name: &str, scripts: &[String]) {
    let rb = root(name);
    for i in 0..scripts.len() {
        case_dir(&rb, i);
    }
    let want = bash(&rb, scripts);
    let rc = root(&format!("{name}-crust"));
    let mut bad = Vec::new();
    for (i, sc) in scripts.iter().enumerate() {
        let got = crust(case_dir(&rc, i), sc.clone());
        if got.as_ref() != Some(&want[i]) {
            bad.push(format!(
                "case {i}: {sc:?}\n  bash  {:?}\n  crust {got:?}",
                want[i]
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} differ:\n{}",
        bad.len(),
        scripts.len(),
        bad[..bad.len().min(8)].join("\n")
    );
}

#[test]
fn leader_fixed_corpus_matches_bash() {
    let s: Vec<String> = FIXED.iter().map(|x| x.to_string()).collect();
    differential("t57-fixed", &s);
}

#[test]
fn leader_generated_match_bash() {
    let mut r = Rng::new(0x57_2026_1115);
    let s: Vec<String> = (0..300).map(|_| gen(&mut r)).collect();
    differential("t57-gen", &s);
}

#[test]
fn leader_externals_and_binary() {
    let exe = env!("CARGO_BIN_EXE_crust");
    let r = root("t57-ext");
    let cases: &[(&str, &str)] = &[
        ("\"$C\" -c 'echo hi; exit 3'; echo $?", "hi\n3\n"),
        ("printf 'a\\nb\\n' | \"$C\" -c 'read x; read y; echo $y$x'", "ba\n"),
        ("\"$C\" -c 'echo out' > o.txt; read l < o.txt; echo \"$l\"", "out\n"),
        ("echo in > i.txt; \"$C\" -c 'read l; echo \"<$l>\"' < i.txt", "<in>\n"),
        ("nosuchcmd_zz_1 2>e.txt; echo $?", "127\n"),
        ("\"$C\" -c 'exit 1' && echo no || echo yes", "yes\n"),
        ("\"$C\" -c 'echo 1; echo 2; echo 3' | \"$C\" -c 'while read l; do echo x$l; done' | { read a; read b; echo $a$b; }", "x1x2\n"),
        ("\"$C\" -c 'exit 5' & wait $!; echo $?", "5\n"),
        ("\"$C\" -c 'echo e >&2' 2>e.txt; read l < e.txt; echo $l", "e\n"),
        ("V=7 \"$C\" -c 'echo $V'; echo \"[${V-}]\"", "7\n[]\n"),
        ("export W=9; \"$C\" -c 'echo $W'", "9\n"),
        ("x=$(\"$C\" -c 'echo sub'); echo \"[$x]\"", "[sub]\n"),
    ];
    for (i, (sc, want)) in cases.iter().enumerate() {
        let got = crust(case_dir(&r, i), format!("C='{exe}'\n{sc}"));
        assert_eq!(got.map(|g| g.0), Some(want.to_string()), "case {i}: {sc}");
    }
    let o = Command::new(exe)
        .args(["-c", "echo $((2+3)); exit 6"])
        .output()
        .unwrap();
    assert_eq!(
        (String::from_utf8_lossy(&o.stdout).as_ref(), o.status.code()),
        ("5\n", Some(6))
    );
    let f = r.join("s.sh");
    std::fs::write(&f, "echo $# $1 $2\n").unwrap();
    let o = Command::new(exe)
        .arg(&f)
        .args(["a", "b c"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout), "2 a b c\n");
    let mut ch = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(b"echo a\nif true; then\necho b\nfi\nexit 4\n")
        .unwrap();
    let o = ch.wait_with_output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("a\n") && out.contains("b\n"), "{out:?}");
    assert_eq!(o.status.code(), Some(4));
}
