//! Expansion corpus tests for `krypton` (`expand` and `expand_one`).
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use krypton::ast::{Command, Script, Word};
use krypton::{expand, expand_one, parse, Env, ExpandError};

struct TestEnv {
    vars: BTreeMap<String, String>,
    pos: Vec<String>,
    homes: BTreeMap<String, String>,
    dirs: BTreeMap<String, Vec<String>>,
}

impl TestEnv {
    fn new() -> Self {
        let mut homes = BTreeMap::new();
        homes.insert(String::new(), String::from("/home/me"));
        homes.insert(String::from("alice"), String::from("/home/alice"));

        let mut dirs = BTreeMap::new();
        dirs.insert(
            String::from("."),
            vec![
                String::from("b.txt"),
                String::from("a.txt"),
                String::from("a.rs"),
                String::from(".hidden"),
                String::from(".dot"),
                String::from("dir"),
                String::from("dir2"),
            ],
        );
        dirs.insert(
            String::from("dir"),
            vec![
                String::from("x.rs"),
                String::from("y.txt"),
                String::from(".secret"),
            ],
        );
        dirs.insert(String::from("dir2"), vec![String::from("z.rs")]);
        dirs.insert(
            String::from("/"),
            vec![String::from("bin"), String::from("etc")],
        );

        let mut vars = BTreeMap::new();
        vars.insert(String::from("?"), String::from("0"));
        vars.insert(String::from("$"), String::from("1234"));
        vars.insert(String::from("!"), String::from("5678"));
        vars.insert(String::from("-"), String::from("hB"));
        vars.insert(String::from("0"), String::from("sh"));

        Self {
            vars,
            pos: Vec::new(),
            homes,
            dirs,
        }
    }
}

impl Env for TestEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn set(&mut self, name: &str, value: &str) {
        self.vars.insert(String::from(name), String::from(value));
    }

    fn positional(&self) -> Vec<String> {
        self.pos.clone()
    }

    fn command(&mut self, s: &Script) -> String {
        if s.items.is_empty() {
            return String::new();
        }
        if let Command::Simple(ref simp) = s.items[0].first.cmds[0] {
            let args: Vec<String> = expand(&simp.words, self).unwrap_or_default();
            if args.first().map(String::as_str) == Some("echo") {
                return args[1..].join(" ") + "\n\n";
            }
            if args.first().map(String::as_str) == Some("raw") {
                return args[1..].join(" ");
            }
        }
        String::new()
    }

    fn home(&self, user: &str) -> Option<String> {
        self.homes.get(user).cloned()
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        self.dirs.get(dir).cloned().unwrap_or_default()
    }
}

fn words_of(src: &str) -> Vec<Word> {
    let s = parse(src, krypton::Escapes::Posix).unwrap();
    if let Command::Simple(simp) = &s.items[0].first.cmds[0] {
        simp.words.clone()
    } else {
        panic!("expected simple command: {src}");
    }
}

fn run_expand(env: &mut TestEnv, src: &str) -> Result<Vec<String>, ExpandError> {
    let w = words_of(src);
    expand(&w, env)
}

#[test]
fn expansion_corpus() {
    // Expected fields match POSIX XCU 2.6 (`bash --posix` word-for-word semantics).
    // 1. Tilde expansion (`echo ~ ~alice ~/bin ~nobody ~"alice" ~\alice`)
    let mut env = TestEnv::new();
    assert_eq!(
        run_expand(&mut env, r#"echo ~ ~alice ~/bin ~nobody ~"alice" ~\alice"#).unwrap(),
        vec![
            "echo",
            "/home/me",
            "/home/alice",
            "/home/me/bin",
            "~nobody",
            "~alice",
            "~alice"
        ]
    );

    // 2. Assignment tilde expansion (`expand_one` on assignment RHS: `~/bin:~alice/lib:~none`)
    let assign_script = parse("P=~/bin:~alice/lib:~none", krypton::Escapes::Posix).unwrap();
    if let Command::Simple(ref simp) = assign_script.items[0].first.cmds[0] {
        assert_eq!(
            expand_one(&simp.assigns[0].1, &mut env).unwrap(),
            "/home/me/bin:/home/alice/lib:~none"
        );
    }

    // 3. Tilde result is not split by IFS (`HOME='/a b'` -> `echo ~` stays 1 field)
    env.homes.insert(String::new(), String::from("/a b"));
    assert_eq!(
        run_expand(&mut env, "echo ~").unwrap(),
        vec!["echo", "/a b"]
    );
    env.homes.insert(String::new(), String::from("/home/me"));

    // 4. Unset, empty, quoted vs unquoted empty
    type BasicCase = (
        &'static str,
        Vec<(&'static str, &'static str)>,
        Vec<&'static str>,
    );
    let cases_basic: Vec<BasicCase> = vec![
        // unset x; echo $x
        ("echo $x", vec![], vec!["echo"]),
        // unset x; echo "$x"
        (r#"echo "$x""#, vec![], vec!["echo", ""]),
        // x=""; echo $x
        ("echo $x", vec![("x", "")], vec!["echo"]),
        // x=""; echo "$x"
        (r#"echo "$x""#, vec![("x", "")], vec!["echo", ""]),
        // echo "" '' ""''
        (r#"echo "" '' ""''"#, vec![], vec!["echo", "", "", ""]),
        // x="a b  c"; echo $x
        (
            "echo $x",
            vec![("x", "a b  c")],
            vec!["echo", "a", "b", "c"],
        ),
        // x="a b  c"; echo "$x"
        (
            r#"echo "$x""#,
            vec![("x", "a b  c")],
            vec!["echo", "a b  c"],
        ),
        // x="  a  b  "; echo $x
        ("echo $x", vec![("x", "  a  b  ")], vec!["echo", "a", "b"]),
        // x="   "; echo $x
        ("echo $x", vec![("x", "   ")], vec!["echo"]),
        // x="   "; echo ""$x
        (r#"echo ""$x"#, vec![("x", "   ")], vec!["echo", ""]),
        // x="   "; echo $x""
        (r#"echo $x"""#, vec![("x", "   ")], vec!["echo", ""]),
        // x=" a"; echo ""$x
        (r#"echo ""$x"#, vec![("x", " a")], vec!["echo", "", "a"]),
        // x="a "; echo $x""
        (r#"echo $x"""#, vec![("x", "a ")], vec!["echo", "a", ""]),
        // Special parameters: $? $$ $! $- $0
        (
            "echo $? $$ $! $- $0",
            vec![],
            vec!["echo", "0", "1234", "5678", "hB", "sh"],
        ),
        // Parameter length ${#x}
        (
            "echo ${#x} ${#unset_var}",
            vec![("x", "hello")],
            vec!["echo", "5", "0"],
        ),
        // Default: ${x-d} vs ${x:-d}
        (
            "echo ${u-def} ${u:-def}",
            vec![],
            vec!["echo", "def", "def"],
        ),
        (
            "echo ${e-def} ${e:-def}",
            vec![("e", "")],
            vec!["echo", "def"],
        ),
        (
            r#"echo "${e-def}" "${e:-def}""#,
            vec![("e", "")],
            vec!["echo", "", "def"],
        ),
        (
            "echo ${s-def} ${s:-def}",
            vec![("s", "ok")],
            vec!["echo", "ok", "ok"],
        ),
        // Default splitting vs quoted default
        ("echo ${u:-a b}", vec![], vec!["echo", "a", "b"]),
        (r#"echo "${u:-a b}""#, vec![], vec!["echo", "a b"]),
        (r#"echo ${u:-"a b"}"#, vec![], vec!["echo", "a b"]),
        // Alt: ${x+a} vs ${x:+a}
        ("echo ${u+alt} ${u:+alt}", vec![], vec!["echo"]),
        (
            "echo ${e+alt} ${e:+alt}",
            vec![("e", "")],
            vec!["echo", "alt"],
        ),
        (
            "echo ${s+alt} ${s:+alt}",
            vec![("s", "1")],
            vec!["echo", "alt", "alt"],
        ),
        ("echo ${s:+x y}", vec![("s", "1")], vec!["echo", "x", "y"]),
        // Trim prefix / suffix
        (
            "echo ${p#*/} ${p##*/}",
            vec![("p", "a/b/c")],
            vec!["echo", "b/c", "c"],
        ),
        (
            "echo ${p%/*} ${p%%/*}",
            vec![("p", "a/b/c")],
            vec!["echo", "a/b", "a"],
        ),
        (
            "echo ${p#z} ${p%z}",
            vec![("p", "abc")],
            vec!["echo", "abc", "abc"],
        ),
        // Quoted vs unquoted pattern in trim
        (
            r#"echo ${p#"$q"} ${p#$q} ${p##$q} "${p##$q}""#,
            vec![("p", "a*b"), ("q", "a*")],
            vec!["echo", "b", "*b", ""],
        ),
        (
            r#"echo ${p#"$q"} ${p##$q} "${p##$q}""#,
            vec![("p", "a*b"), ("q", "*")],
            vec!["echo", "a*b", ""],
        ),
        // Command substitution trailing newline stripping
        (
            r#"echo "$(echo hi there)" $(echo a   b)"#,
            vec![],
            vec!["echo", "hi there", "a", "b"],
        ),
        (r#"echo "$(raw "")""#, vec![], vec!["echo", ""]),
        // Arithmetic expansion inside words
        (
            "echo $((1 + 2 * 3)) $((x + 10))",
            vec![("x", "5")],
            vec!["echo", "7", "15"],
        ),
        (
            "echo $(( $x + $(raw 4) ))",
            vec![("x", "6")],
            vec!["echo", "10"],
        ),
        // IFS empty = no splitting
        (
            "echo $x",
            vec![("IFS", ""), ("x", "a b:c")],
            vec!["echo", "a b:c"],
        ),
        // IFS colon splitting edge cases
        (
            "echo $x",
            vec![("IFS", ":"), ("x", "a:b:c")],
            vec!["echo", "a", "b", "c"],
        ),
        (
            "echo $x",
            vec![("IFS", ":"), ("x", "a::b")],
            vec!["echo", "a", "", "b"],
        ),
        (
            "echo $x",
            vec![("IFS", ":"), ("x", ":a:b:")],
            vec!["echo", "", "a", "b"],
        ),
        (
            "echo $x",
            vec![("IFS", ":"), ("x", "a:b::")],
            vec!["echo", "a", "b", ""],
        ),
        ("echo $x", vec![("IFS", ":"), ("x", ":")], vec!["echo", ""]),
        (
            "echo $x",
            vec![("IFS", ":"), ("x", "::")],
            vec!["echo", "", ""],
        ),
        // IFS mixed whitespace + non-whitespace (e.g. IFS=": ")
        (
            "echo $x",
            vec![("IFS", ": "), ("x", "a : b")],
            vec!["echo", "a", "b"],
        ),
        (
            "echo $x",
            vec![("IFS", ": "), ("x", "a : : b")],
            vec!["echo", "a", "", "b"],
        ),
        (
            "echo $x",
            vec![("IFS", ": "), ("x", " : a")],
            vec!["echo", "", "a"],
        ),
        (
            "echo $x",
            vec![("IFS", ": "), ("x", "a : ")],
            vec!["echo", "a"],
        ),
        // Quoted chars are never split by IFS
        (
            r#"echo a"$q"b"#,
            vec![("IFS", ":"), ("q", ":")],
            vec!["echo", "a:b"],
        ),
        // Globbing in TestEnv virtual filesystem
        ("echo *.rs", vec![], vec!["echo", "a.rs"]),
        ("echo *.txt", vec![], vec!["echo", "a.txt", "b.txt"]),
        (
            "echo *",
            vec![],
            vec!["echo", "a.rs", "a.txt", "b.txt", "dir", "dir2"],
        ),
        ("echo .*", vec![], vec!["echo", ".dot", ".hidden"]),
        (
            "echo [.]*",
            vec![],
            vec!["echo", "a.rs", "a.txt", "b.txt", "dir", "dir2"]
                .into_iter()
                .take(0)
                .chain(vec!["echo", "[.]*"])
                .collect(),
        ),
        ("echo dir/*.rs", vec![], vec!["echo", "dir/x.rs"]),
        (
            "echo dir*/x.rs",
            vec![],
            vec!["echo", "dir/x.rs", "dir2/x.rs"],
        ),
        (
            "echo dir*/*.rs",
            vec![],
            vec!["echo", "dir/x.rs", "dir2/z.rs"],
        ),
        ("echo /*", vec![], vec!["echo", "/bin", "/etc"]),
        ("echo *.none", vec![], vec!["echo", "*.none"]),
        (r#"echo "*.rs" \*.rs"#, vec![], vec!["echo", "*.rs", "*.rs"]),
        ("echo $g", vec![("g", "*.rs")], vec!["echo", "a.rs"]),
        (r#"echo "$g""#, vec![("g", "*.rs")], vec!["echo", "*.rs"]),
    ];

    for (idx, (src, vars, expected)) in cases_basic.iter().enumerate() {
        let mut e = TestEnv::new();
        for (k, v) in vars {
            e.set(k, v);
        }
        let got = run_expand(&mut e, src).unwrap_or_else(|err| {
            panic!("case {idx} failed: {src:?} -> {err:?}");
        });
        assert_eq!(got, *expected, "case {idx}: {src:?} with {vars:?}");
    }

    // 5. Positional parameters: $@, "$@", $*, "$*", $#, $1..${10}
    type PosCase = (
        &'static str,
        Vec<&'static str>,
        Option<&'static str>,
        Vec<&'static str>,
    );
    let pos_cases: Vec<PosCase> = vec![
        // 0 positionals
        (r#"echo "$@""#, vec![], None, vec!["echo"]),
        (r#"echo $@"#, vec![], None, vec!["echo"]),
        (r#"echo "$*""#, vec![], None, vec!["echo", ""]),
        (r#"echo $*"#, vec![], None, vec!["echo"]),
        (r#"echo "a$@b""#, vec![], None, vec!["echo", "ab"]),
        (r#"echo ""$@"#, vec![], None, vec!["echo", ""]),
        (
            "echo $# ${#@} ${#*}",
            vec![],
            None,
            vec!["echo", "0", "0", "0"],
        ),
        // 1 empty positional
        (r#"echo "$@""#, vec![""], None, vec!["echo", ""]),
        (r#"echo $@"#, vec![""], None, vec!["echo"]),
        // 2 empty positionals
        (r#"echo "$@""#, vec!["", ""], None, vec!["echo", "", ""]),
        // Multi-word positionals
        (
            r#"echo "$@""#,
            vec!["a b", "c d"],
            None,
            vec!["echo", "a b", "c d"],
        ),
        (
            r#"echo "x$@y""#,
            vec!["a b", "c d"],
            None,
            vec!["echo", "xa b", "c dy"],
        ),
        (
            r#"echo $@"#,
            vec!["a b", "c d"],
            None,
            vec!["echo", "a", "b", "c", "d"],
        ),
        (
            r#"echo "$*""#,
            vec!["a b", "c d"],
            None,
            vec!["echo", "a b c d"],
        ),
        (
            r#"echo "$*""#,
            vec!["a b", "c d"],
            Some(":"),
            vec!["echo", "a b:c d"],
        ),
        (
            r#"echo "$*""#,
            vec!["a b", "c d"],
            Some(""),
            vec!["echo", "a bc d"],
        ),
        (
            r#"echo $*"#,
            vec!["a:b", "c"],
            Some(":"),
            vec!["echo", "a", "b", "c"],
        ),
        (
            r#"echo $*"#,
            vec!["a:b", "c"],
            Some(""),
            vec!["echo", "a:b", "c"],
        ),
        (
            "echo $1 $2 ${10}",
            vec!["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9", "p10"],
            None,
            vec!["echo", "p1", "p2", "p10"],
        ),
        ("echo $10", vec!["p1"], None, vec!["echo", "p10"]),
        (
            r#"echo ${@-def} "${@-def}" ${@:-def}"#,
            vec![""],
            None,
            vec!["echo", "", "def"],
        ),
        ("echo ${@-def}", vec![], None, vec!["echo", "def"]),
    ];

    for (idx, (src, pos, ifs, expected)) in pos_cases.iter().enumerate() {
        let mut e = TestEnv::new();
        e.pos = pos.iter().map(|s| String::from(*s)).collect();
        if let Some(ifs_val) = ifs {
            e.set("IFS", ifs_val);
        }
        let got = run_expand(&mut e, src).unwrap();
        assert_eq!(got, *expected, "pos case {idx}: {src:?}");
    }

    // 6. ${x:=w} and ${x=w} side effects and ${x?w} errors
    let mut e = TestEnv::new();
    assert_eq!(
        run_expand(&mut e, "echo ${v:=hello}").unwrap(),
        vec!["echo", "hello"]
    );
    assert_eq!(e.get("v").as_deref(), Some("hello"));
    e.set("empty", "");
    assert_eq!(
        run_expand(&mut e, "echo ${empty=fallback}").unwrap(),
        vec!["echo"]
    );
    assert_eq!(e.get("empty").as_deref(), Some(""));
    assert_eq!(
        run_expand(&mut e, "echo ${empty:=fallback}").unwrap(),
        vec!["echo", "fallback"]
    );
    assert_eq!(e.get("empty").as_deref(), Some("fallback"));

    assert_eq!(
        run_expand(&mut e, "echo ${missing?custom err}").unwrap_err(),
        ExpandError::Unset(String::from("missing"), String::from("custom err"))
    );
    e.set("empty2", "");
    assert_eq!(
        run_expand(&mut e, "echo ${empty2?ok}").unwrap(),
        vec!["echo"]
    );
    assert_eq!(
        run_expand(&mut e, "echo ${empty2:?}").unwrap_err(),
        ExpandError::Unset(
            String::from("empty2"),
            String::from("parameter null or not set")
        )
    );
    assert_eq!(
        run_expand(&mut e, "echo ${1:=bad}").unwrap_err(),
        ExpandError::BadSubst(String::from("1"))
    );

    // 7. expand_one does not split or glob
    e.set("w", "a  b  *.rs");
    let one_w = words_of("$w")[0].clone();
    assert_eq!(expand_one(&one_w, &mut e).unwrap(), "a  b  *.rs");

    // 8. Regression tests for RETRY 1 (non-whitespace IFS, literal splitting, $@/$*, trailing \)
    let mut r_env = TestEnv::new();
    r_env.pos = vec![
        String::from("p1"),
        String::from("p 2"),
        String::new(),
        String::from("*.rs"),
    ];
    r_env.set("IFS", "a");
    r_env.set("sp", " lead and trail ");
    // Literal words must not be split on IFS; only expansion results split.
    assert_eq!(
        run_expand(&mut r_env, "echo nomatch* [!a]*.txt").unwrap(),
        vec!["echo", "nomatch*", "b.txt"]
    );
    // Non-ws IFS at start/end of expansion result joined with quoted/empty text.
    assert_eq!(
        run_expand(&mut r_env, r##"echo "$sp"$(echo a  b) $(echo b  a)"""##).unwrap(),
        vec!["echo", " lead and trail ", " b", "b ", ""]
    );
    // Unquoted $@ and $* keep empty positional params iff IFS starts with non-whitespace.
    r_env.set("IFS", ":");
    assert_eq!(
        run_expand(&mut r_env, "echo $@").unwrap(),
        vec!["echo", "p1", "p 2", "", "a.rs"]
    );
    assert_eq!(
        run_expand(&mut r_env, "echo $*").unwrap(),
        vec!["echo", "p1", "p 2", "", "a.rs"]
    );
    r_env.set("IFS", " :");
    assert_eq!(
        run_expand(&mut r_env, "echo $@").unwrap(),
        vec!["echo", "p1", "p", "2", "a.rs"]
    );
    assert_eq!(
        run_expand(&mut r_env, "echo $*").unwrap(),
        vec!["echo", "p1", "p", "2", "a.rs"]
    );
    // Trailing backslash at EOF returns incomplete parse error.
    for src in [r"echo \", r"\", r"cat <<\"] {
        let err = parse(src, krypton::Escapes::Posix).expect_err(src);
        assert!(err.incomplete, "{src:?} should be incomplete: {err:?}");
    }
}
