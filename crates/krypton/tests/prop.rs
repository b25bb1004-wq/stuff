//! Property-based tests (`assay::forall`) for `krypton`.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;

use assay::{forall, Rng};
use krypton::ast::*;
use krypton::{arith, glob_match, parse, Env};

struct DummyEnv;

impl Env for DummyEnv {
    fn get(&self, _: &str) -> Option<String> {
        None
    }
    fn set(&mut self, _: &str, _: &str) {}
    fn positional(&self) -> Vec<String> {
        Vec::new()
    }
    fn command(&mut self, _: &Script) -> String {
        String::new()
    }
    fn home(&self, _: &str) -> Option<String> {
        None
    }
    fn list_dir(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

#[test]
fn fuzz_parse_never_panics_2000() {
    let corpus = [
        "echo hello world",
        "if true; then a; elif false; then b; else c; fi",
        "while x; do y; done",
        "for i in 1 2 3; do echo $i; done",
        "case $x in (a|b) echo 1;; *) echo 2 esac",
        "cat <<EOF <<-'END'\nhi $x\nEOF\n\tEND\n",
        "foo() { (a | ! b) && c || d & }",
        "echo \"$(echo `echo $((1+2))`)\" ${x:-'y'}",
    ];

    // 1000 random byte strings
    forall(1000, 0x5EED_0001, |r| {
        let bytes = r.bytes(160);
        let s = String::from_utf8_lossy(&bytes);
        let _ = parse(&s, krypton::Escapes::Posix);
        let _ = arith(&s, &mut DummyEnv);
        let _ = glob_match(&s, "sample_name");
    });

    // 1000 mutated corpus scripts
    forall(1000, 0x5EED_0002, |r| {
        let base = *r.pick(&corpus);
        let mut buf = base.as_bytes().to_vec();
        let edits = r.range(1, 8);
        for _ in 0..edits {
            let op = r.below(3);
            if op == 0 && !buf.is_empty() {
                let idx = r.below(buf.len());
                buf.remove(idx);
            } else if op == 1 {
                let idx = r.below(buf.len() + 1);
                let ch = *r.pick(b" '\"\\$(){};|&<>\n`#*?[]!:-=%~01a");
                buf.insert(idx, ch);
            } else if !buf.is_empty() {
                let idx = r.below(buf.len());
                buf[idx] = *r.pick(b" '\"\\$(){};|&<>\n`#*?[]!:-=%~01a\x00\xff");
            }
        }
        let s = String::from_utf8_lossy(&buf);
        let _ = parse(&s, krypton::Escapes::Posix);
    });
}

fn gen_ident(r: &mut Rng) -> String {
    let names = ["alpha", "beta", "gamma", "v1", "x_2", "item"];
    String::from(*r.pick(&names))
}

fn gen_safe_lit(r: &mut Rng) -> String {
    let words = ["foo", "bar", "baz", "arg1", "hello", "path/to/file", "w0"];
    String::from(*r.pick(&words))
}

fn gen_word(r: &mut Rng, depth: usize, allow_tilde: bool) -> Word {
    let mut parts = Vec::new();
    let count = if depth == 0 { r.range(1, 3) } else { 1 };
    for i in 0..count {
        if i == 0 && allow_tilde && r.chance(1, 6) {
            let user = if r.chance(1, 2) {
                String::new()
            } else {
                String::from("bob")
            };
            parts.push(Part::Tilde(user));
            parts.push(Part::Lit(String::from("/dir")));
            continue;
        }
        let kind = if depth >= 2 { 0 } else { r.below(7) };
        let p = match kind {
            0 => Part::Lit(gen_safe_lit(r)),
            1 => Part::Single(String::from("sq $x")),
            2 => Part::Escaped(*r.pick(&[' ', '*', '?', 'a', '$'])),
            3 => {
                let inner = vec![
                    Part::Lit(String::from("dq ")),
                    Part::Escaped('"'),
                    Part::Param(Param {
                        name: gen_ident(r),
                        op: ParamOp::Plain,
                    }),
                ];
                Part::Double(inner)
            }
            4 => {
                let op = match r.below(5) {
                    0 => ParamOp::Plain,
                    1 => ParamOp::Length,
                    2 => ParamOp::Default(r.chance(1, 2), gen_word(r, depth + 1, false)),
                    3 => ParamOp::Alt(r.chance(1, 2), gen_word(r, depth + 1, false)),
                    _ => ParamOp::TrimSuffix(r.chance(1, 2), vec![Part::Lit(String::from("*.rs"))]),
                };
                Part::Param(Param {
                    name: gen_ident(r),
                    op,
                })
            }
            5 => Part::Cmd(gen_script(r, depth + 1, 1)),
            _ => Part::Arith(String::from("1 + 2")),
        };
        match p {
            Part::Lit(s) => {
                if let Some(Part::Lit(last)) = parts.last_mut() {
                    last.push_str(&s);
                } else {
                    parts.push(Part::Lit(s));
                }
            }
            other => parts.push(other),
        }
    }
    parts
}

fn gen_redir(r: &mut Rng, depth: usize) -> Redir {
    let fd = if r.chance(1, 3) {
        Some(r.range(0, 4) as u32)
    } else {
        None
    };
    let op = match r.below(7) {
        0 => RedirOp::In,
        1 => RedirOp::Out,
        2 => RedirOp::Append,
        3 => RedirOp::Clobber,
        4 => RedirOp::ReadWrite,
        5 => RedirOp::DupIn,
        _ => RedirOp::DupOut,
    };
    Redir {
        fd,
        op,
        target: gen_word(r, depth + 1, true),
    }
}

fn gen_command(r: &mut Rng, depth: usize) -> Command {
    if depth >= 2 || r.chance(3, 5) {
        let mut assigns = Vec::new();
        if r.chance(1, 3) {
            assigns.push((gen_ident(r), gen_word(r, depth + 1, true)));
        }
        let mut words = Vec::new();
        let wcount = if assigns.is_empty() {
            r.range(1, 4)
        } else {
            r.range(0, 3)
        };
        for _ in 0..wcount {
            words.push(gen_word(r, depth + 1, true));
        }
        let mut redirs = Vec::new();
        if r.chance(1, 3) {
            redirs.push(gen_redir(r, depth));
        }
        return Command::Simple(Simple {
            assigns,
            words,
            redirs,
        });
    }

    match r.below(7) {
        0 => Command::Subshell(gen_script(r, depth + 1, 2), Vec::new()),
        1 => Command::Group(gen_script(r, depth + 1, 2), Vec::new()),
        2 => Command::If {
            arms: vec![(gen_script(r, depth + 1, 1), gen_script(r, depth + 1, 1))],
            otherwise: if r.chance(1, 2) {
                Some(gen_script(r, depth + 1, 1))
            } else {
                None
            },
            redirs: Vec::new(),
        },
        3 => Command::Loop {
            until: r.chance(1, 2),
            cond: gen_script(r, depth + 1, 1),
            body: gen_script(r, depth + 1, 1),
            redirs: Vec::new(),
        },
        4 => Command::For {
            var: gen_ident(r),
            words: if r.chance(1, 2) {
                Some(vec![gen_word(r, depth + 1, true)])
            } else {
                None
            },
            body: gen_script(r, depth + 1, 1),
            redirs: Vec::new(),
        },
        5 => Command::Case {
            word: gen_word(r, depth + 1, true),
            arms: vec![(
                vec![gen_word(r, depth + 1, true)],
                gen_script(r, depth + 1, 1),
            )],
            redirs: Vec::new(),
        },
        _ => Command::Func {
            name: gen_ident(r),
            body: Box::new(Command::Group(gen_script(r, depth + 1, 1), Vec::new())),
        },
    }
}

fn gen_pipeline(r: &mut Rng, depth: usize) -> Pipeline {
    let bang = r.chance(1, 4);
    let n = r.range(1, 3);
    let mut cmds = Vec::new();
    for _ in 0..n {
        cmds.push(gen_command(r, depth));
    }
    Pipeline { bang, cmds }
}

fn gen_script(r: &mut Rng, depth: usize, max_items: u64) -> Script {
    let n = r.range(1, max_items + 1);
    let mut items = Vec::new();
    for _ in 0..n {
        let first = gen_pipeline(r, depth);
        let rest_n = if depth == 0 { r.range(0, 2) } else { 0 };
        let mut rest = Vec::new();
        for _ in 0..rest_n {
            rest.push((r.chance(1, 2), gen_pipeline(r, depth)));
        }
        items.push(AndOr {
            first,
            rest,
            background: r.chance(1, 4),
        });
    }
    Script { items }
}

fn print_word(w: &[Part], out: &mut String) {
    for p in w {
        match p {
            Part::Lit(s) => out.push_str(s),
            Part::Single(s) => {
                out.push('\'');
                out.push_str(s);
                out.push('\'');
            }
            Part::Escaped(c) => {
                out.push('\\');
                out.push(*c);
            }
            Part::Double(parts) => {
                out.push('"');
                print_word(parts, out);
                out.push('"');
            }
            Part::Param(pm) => {
                out.push_str("${");
                if pm.op == ParamOp::Length {
                    out.push('#');
                }
                out.push_str(&pm.name);
                match &pm.op {
                    ParamOp::Plain | ParamOp::Length => {}
                    ParamOp::Default(c, w) => {
                        out.push_str(if *c { ":-" } else { "-" });
                        print_word(w, out);
                    }
                    ParamOp::Assign(c, w) => {
                        out.push_str(if *c { ":=" } else { "=" });
                        print_word(w, out);
                    }
                    ParamOp::Error(c, w) => {
                        out.push_str(if *c { ":?" } else { "?" });
                        print_word(w, out);
                    }
                    ParamOp::Alt(c, w) => {
                        out.push_str(if *c { ":+" } else { "+" });
                        print_word(w, out);
                    }
                    ParamOp::TrimSuffix(l, w) => {
                        out.push_str(if *l { "%%" } else { "%" });
                        print_word(w, out);
                    }
                    ParamOp::TrimPrefix(l, w) => {
                        out.push_str(if *l { "##" } else { "#" });
                        print_word(w, out);
                    }
                }
                out.push('}');
            }
            Part::Cmd(s) => {
                out.push_str("$(");
                print_script(s, out);
                out.push(')');
            }
            Part::Arith(raw) => {
                let _ = write!(out, "$(({raw}))");
            }
            Part::Tilde(u) => {
                out.push('~');
                out.push_str(u);
            }
        }
    }
}

fn print_redirs(redirs: &[Redir], out: &mut String) {
    for r in redirs {
        out.push(' ');
        if let Some(fd) = r.fd {
            let _ = write!(out, "{fd}");
        }
        match &r.op {
            RedirOp::In => out.push('<'),
            RedirOp::Out => out.push('>'),
            RedirOp::Append => out.push_str(">>"),
            RedirOp::Clobber => out.push_str(">|"),
            RedirOp::ReadWrite => out.push_str("<>"),
            RedirOp::DupIn => out.push_str("<&"),
            RedirOp::DupOut => out.push_str(">&"),
            RedirOp::HereDoc { .. } => unreachable!(),
        }
        print_word(&r.target, out);
    }
}

fn ends_with_bg(s: &Script) -> bool {
    s.items.last().is_some_and(|ao| ao.background)
}

fn sep_if_needed(s: &Script, out: &mut String) {
    if !ends_with_bg(s) {
        out.push(';');
    }
}

fn print_command(cmd: &Command, out: &mut String) {
    match cmd {
        Command::Simple(s) => {
            let mut first = true;
            for (k, v) in &s.assigns {
                if !first {
                    out.push(' ');
                }
                first = false;
                out.push_str(k);
                out.push('=');
                print_word(v, out);
            }
            for w in &s.words {
                if !first {
                    out.push(' ');
                }
                first = false;
                print_word(w, out);
            }
            print_redirs(&s.redirs, out);
        }
        Command::Subshell(s, r) => {
            out.push('(');
            print_script(s, out);
            out.push(')');
            print_redirs(r, out);
        }
        Command::Group(s, r) => {
            out.push_str("{ ");
            print_script(s, out);
            sep_if_needed(s, out);
            out.push_str(" }");
            print_redirs(r, out);
        }
        Command::If {
            arms,
            otherwise,
            redirs,
        } => {
            for (i, (c, b)) in arms.iter().enumerate() {
                out.push_str(if i == 0 { "if " } else { " elif " });
                print_script(c, out);
                sep_if_needed(c, out);
                out.push_str(" then ");
                print_script(b, out);
                sep_if_needed(b, out);
            }
            if let Some(o) = otherwise {
                out.push_str(" else ");
                print_script(o, out);
                sep_if_needed(o, out);
            }
            out.push_str(" fi");
            print_redirs(redirs, out);
        }
        Command::Loop {
            until,
            cond,
            body,
            redirs,
        } => {
            out.push_str(if *until { "until " } else { "while " });
            print_script(cond, out);
            sep_if_needed(cond, out);
            out.push_str(" do ");
            print_script(body, out);
            sep_if_needed(body, out);
            out.push_str(" done");
            print_redirs(redirs, out);
        }
        Command::For {
            var,
            words,
            body,
            redirs,
        } => {
            let _ = write!(out, "for {var}");
            if let Some(ws) = words {
                out.push_str(" in");
                for w in ws {
                    out.push(' ');
                    print_word(w, out);
                }
            }
            out.push_str("; do ");
            print_script(body, out);
            sep_if_needed(body, out);
            out.push_str(" done");
            print_redirs(redirs, out);
        }
        Command::Case { word, arms, redirs } => {
            out.push_str("case ");
            print_word(word, out);
            out.push_str(" in ");
            for (pats, body) in arms {
                for (i, p) in pats.iter().enumerate() {
                    if i > 0 {
                        out.push('|');
                    }
                    print_word(p, out);
                }
                out.push_str(") ");
                print_script(body, out);
                out.push_str(";; ");
            }
            out.push_str("esac");
            print_redirs(redirs, out);
        }
        Command::Func { name, body } => {
            let _ = write!(out, "{name}() ");
            print_command(body, out);
        }
    }
}

fn print_pipeline(p: &Pipeline, out: &mut String) {
    if p.bang {
        out.push_str("! ");
    }
    for (i, cmd) in p.cmds.iter().enumerate() {
        if i > 0 {
            out.push_str(" | ");
        }
        print_command(cmd, out);
    }
}

fn print_script(s: &Script, out: &mut String) {
    for (i, ao) in s.items.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        print_pipeline(&ao.first, out);
        for (is_and, p) in &ao.rest {
            out.push_str(if *is_and { " && " } else { " || " });
            print_pipeline(p, out);
        }
        if ao.background {
            out.push_str(" &");
        } else if i + 1 < s.items.len() {
            out.push(';');
        }
    }
}

#[test]
fn roundtrip_printed_ast_500() {
    forall(500, 0xCAFE_BEEF, |r| {
        let orig = gen_script(r, 0, 3);
        let mut text = String::new();
        print_script(&orig, &mut text);
        let reparsed = parse(&text, krypton::Escapes::Posix).unwrap_or_else(|e| {
            panic!("failed to reparse:\n{text}\nerr: {e:?}");
        });
        assert_eq!(reparsed, orig, "AST mismatch for printed script:\n{text}");
    });
}
