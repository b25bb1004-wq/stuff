//! Leader check for T53: word expansion against real `bash --posix` (one process, each case in a
//! subshell, LC_ALL=C, a temp dir for globs), parser shapes, errors and `incomplete`.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use crust::ast::{Command, Script};
use crust::{expand, parse, Env};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command as Proc;

struct T {
    vars: HashMap<String, String>,
    pos: Vec<String>,
    dir: PathBuf,
}

impl Env for T {
    fn get(&self, n: &str) -> Option<String> {
        self.vars.get(n).cloned()
    }
    fn set(&mut self, n: &str, v: &str) {
        self.vars.insert(n.into(), v.into());
    }
    fn positional(&self) -> Vec<String> {
        self.pos.clone()
    }
    fn command(&mut self, s: &Script) -> String {
        // Only `echo WORDS` is used inside $(...) by this check.
        let Command::Simple(c) = &s.items[0].first.cmds[0] else {
            panic!("unexpected substitution {s:?}")
        };
        let f = expand(&c.words, self).unwrap();
        assert_eq!(f[0], "echo");
        format!("{}\n\n", f[1..].join(" "))
    }
    fn home(&self, u: &str) -> Option<String> {
        (u.is_empty()).then(|| "/home/u".into())
    }
    fn list_dir(&self, d: &str) -> Vec<String> {
        let d = d.trim_start_matches("./");
        let p = if d.is_empty() || d == "." {
            self.dir.clone()
        } else {
            self.dir.join(d)
        };
        std::fs::read_dir(p)
            .map(|r| {
                r.map(|e| e.unwrap().file_name().into_string().unwrap())
                    .collect()
            })
            .unwrap_or_default()
    }
}

const VARS: &[(&str, &str)] = &[
    ("a", "x  y.tar.gz"),
    ("b", ""),
    ("c", "p/q/r"),
    ("star", "*.txt"),
    ("n", "5"),
    ("sp", " lead and trail "),
    ("col", "a:b::c"),
];
const POS: &[&str] = &["p1", "p 2", "", "*.md"];

const FRAGS: &[&str] = &[
    "$a",
    "\"$a\"",
    "${a}",
    "$b",
    "\"$b\"",
    "${b:-dflt}",
    "${b-dflt}",
    "${u:-x y}",
    "\"${u:-x y}\"",
    "${u+set}",
    "${a:+alt}",
    "${#a}",
    "${#u}",
    "${a%%.*}",
    "${a%.*}",
    "${a#*.}",
    "${a##*.}",
    "${c#*/}",
    "${c%/*}",
    "\"$@\"",
    "$@",
    "$*",
    "\"$*\"",
    "$#",
    "$1",
    "\"$2\"",
    "$3",
    "${4}",
    "$((n*3+1))",
    "$((n<<2|1))",
    "$((0x10+010))",
    "$((n>3?7:9))",
    "$((-n%3))",
    "$((u+1))",
    "'q $a'",
    "\\$a",
    "\\ ",
    "~",
    "~/x",
    "\"~\"",
    "*.txt",
    "d/*",
    "[ab]*",
    "?.md",
    "nomatch*",
    "\"*.txt\"",
    "$star",
    "\"$star\"",
    "$(echo a  b)",
    "\"$(echo a  b)\"",
    "$sp",
    "\"$sp\"",
    "$col",
    "x",
    "\"\"",
    "''",
    "\"a\\\"b\"",
    "\"\\$a\"",
    "[!a]*.txt",
    "\"${u:-$a}\"",
    "${u:-$a}",
];
const IFS: &[Option<&str>] = &[None, Some(":"), Some(""), Some(" :"), Some("a")];

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[test]
fn leader_expansion_matches_bash() {
    let dir = std::env::temp_dir().join(format!("crust-leader-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("d")).unwrap();
    for f in [
        "a.txt",
        "b.txt",
        "c.md",
        "sp ace.txt",
        "d/e.txt",
        "d/f.txt",
        ".hid.txt",
    ] {
        std::fs::write(dir.join(f), "").unwrap();
    }
    let mut r = Rng::new(0x53_2026_1114);
    let mut cases: Vec<(Option<&str>, String)> =
        FRAGS.iter().map(|f| (None, f.to_string())).collect();
    for _ in 0..400 {
        let words: Vec<String> = (0..r.range(1, 4))
            .map(|_| (0..r.range(1, 3)).map(|_| *r.pick(FRAGS)).collect())
            .collect();
        cases.push((*r.pick(IFS), words.join(" ")));
    }
    let mut script = String::from("LC_ALL=C; HOME=/home/u; export LC_ALL HOME\n");
    for (ifs, w) in &cases {
        script += "( set --";
        for p in POS {
            script += &format!(" {}", sh_quote(p));
        }
        script += "; ";
        for (k, v) in VARS {
            script += &format!("{k}={}; ", sh_quote(v));
        }
        script += &match ifs {
            None => "unset IFS; ".to_string(),
            Some(s) => format!("IFS={}; ", sh_quote(s)),
        };
        script += &format!("printf '<%s>' {w} ); printf '\\n'\n");
    }
    let out = Proc::new("bash")
        .args(["--posix", "-c", &script])
        .current_dir(&dir)
        .output()
        .expect("bash on PATH");
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.split('\n').collect();
    assert!(
        lines.len() > cases.len(),
        "bash: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut bad = Vec::new();
    for (i, (ifs, w)) in cases.iter().enumerate() {
        let mut env = T {
            vars: VARS
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            pos: POS.iter().map(|s| s.to_string()).collect(),
            dir: dir.clone(),
        };
        if let Some(s) = ifs {
            env.vars.insert("IFS".into(), s.to_string());
        }
        let src = format!("printf '<%s>' {w}");
        let s = parse(&src).unwrap_or_else(|e| panic!("parse {src:?}: {e:?}"));
        let Command::Simple(c) = &s.items[0].first.cmds[0] else {
            panic!("{src:?} is not a simple command")
        };
        let f = expand(&c.words, &mut env).unwrap_or_else(|e| panic!("{src:?}: {e:?}"));
        let got: String = if f.len() <= 2 {
            "<>".into()
        } else {
            f[2..].iter().map(|x| format!("<{x}>")).collect()
        };
        if got != lines[i] {
            bad.push(format!(
                "IFS={ifs:?} {w}\n   bash {}\n  crust {got}",
                lines[i]
            ));
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        bad.is_empty(),
        "{} of {} differ:\n{}",
        bad.len(),
        cases.len(),
        bad[..bad.len().min(15)].join("\n")
    );
}

#[test]
fn leader_parser_shapes_errors_and_incomplete() {
    for src in [
        "if true; then",
        "echo \"abc",
        "echo 'abc",
        "echo $(ls",
        "cat <<EOF\nx\n",
        "case a in",
        "{ echo",
        "echo \\",
        "while x; do y",
        "f() {",
        "a &&",
        "a |",
        "echo ${a",
        "echo `ls",
    ] {
        let e = parse(src).expect_err(src);
        assert!(e.incomplete, "{src:?} should be incomplete: {e:?}");
    }
    for src in [
        "fi",
        "echo )",
        ";;",
        "| x",
        "&& y",
        "then",
        "done",
        "( )",
        "if then fi",
    ] {
        let e = parse(src).expect_err(src);
        assert!(!e.incomplete, "{src:?} is a hard error: {e:?}");
        assert!(e.line >= 1 && e.col >= 1);
    }
    let s = parse("a && b || c &\n! x | y\n").unwrap();
    assert_eq!(s.items.len(), 2);
    assert!(s.items[0].background && s.items[0].rest.len() == 2);
    assert!(s.items[0].rest[0].0 && !s.items[0].rest[1].0);
    assert!(s.items[1].first.bang && s.items[1].first.cmds.len() == 2);

    let s = parse("cat <<A <<-B; echo z\nline $x\nA\n\t\ttabbed\n\tB\n").unwrap();
    assert_eq!(s.items.len(), 2);
    let Command::Simple(c) = &s.items[0].first.cmds[0] else {
        panic!()
    };
    assert_eq!(c.redirs.len(), 2);
    let s = parse("case $x in a|b) echo 1;; *) echo 2\nesac").unwrap();
    let Command::Case { arms, .. } = &s.items[0].first.cmds[0] else {
        panic!()
    };
    assert_eq!((arms.len(), arms[0].0.len()), (2, 2));
    assert!(parse("echo if then; for i in 1 2; do :; done # c ) (").is_ok());
    // Arbitrary bytes never panic.
    let mut r = Rng::new(9);
    for _ in 0..3000 {
        let n = r.below(40);
        let b: Vec<u8> = (0..n)
            .map(|_| *r.pick(b"ab $(){}`'\"\\|&;<>\n#*?[]!=~-0"))
            .collect();
        let _ = parse(&String::from_utf8_lossy(&b));
    }
}
