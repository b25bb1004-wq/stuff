//! Parser corpus tests for `crust`.
#![allow(clippy::unwrap_used)]

use crust::ast::*;
use crust::{parse, ParseError};

fn lit(s: &str) -> Word {
    vec![Part::Lit(String::from(s))]
}

fn simp(words: Vec<Word>) -> Command {
    Command::Simple(Simple {
        assigns: Vec::new(),
        words,
        redirs: Vec::new(),
    })
}

fn ao(cmd: Command) -> AndOr {
    AndOr {
        first: Pipeline {
            bang: false,
            cmds: vec![cmd],
        },
        rest: Vec::new(),
        background: false,
    }
}

fn script(cmds: Vec<Command>) -> Script {
    Script {
        items: cmds.into_iter().map(ao).collect(),
    }
}

#[test]
fn parser_corpus_ok() {
    let cases: Vec<(&str, Script)> = vec![
        // 1. Empty & whitespace/comments
        ("", Script::default()),
        ("   \n\t\n# just a comment\n", Script::default()),
        // 2. Simple command
        ("echo hello world", script(vec![simp(vec![lit("echo"), lit("hello"), lit("world")])])),
        // 3. Semicolon and newline separated commands
        ("a; b\nc", script(vec![simp(vec![lit("a")]), simp(vec![lit("b")]), simp(vec![lit("c")])])),
        // 4. Background &
        (
            "sleep 1 & echo done",
            Script {
                items: vec![
                    AndOr {
                        first: Pipeline {
                            bang: false,
                            cmds: vec![simp(vec![lit("sleep"), lit("1")])],
                        },
                        rest: Vec::new(),
                        background: true,
                    },
                    ao(simp(vec![lit("echo"), lit("done")])),
                ],
            },
        ),
        // 5. Pipeline and bang
        (
            "! a | b | c",
            Script {
                items: vec![AndOr {
                    first: Pipeline {
                        bang: true,
                        cmds: vec![
                            simp(vec![lit("a")]),
                            simp(vec![lit("b")]),
                            simp(vec![lit("c")]),
                        ],
                    },
                    rest: Vec::new(),
                    background: false,
                }],
            },
        ),
        // 6. AndOr && || with newline after operator
        (
            "a &&\nb ||\nc &",
            Script {
                items: vec![AndOr {
                    first: Pipeline {
                        bang: false,
                        cmds: vec![simp(vec![lit("a")])],
                    },
                    rest: vec![
                        (true, Pipeline { bang: false, cmds: vec![simp(vec![lit("b")])] }),
                        (false, Pipeline { bang: false, cmds: vec![simp(vec![lit("c")])] }),
                    ],
                    background: true,
                }],
            },
        ),
        // 7. Assignments with tilde after = and :
        (
            "PATH=~/bin:~bob/bin A=1 cmd B=2",
            script(vec![Command::Simple(Simple {
                assigns: vec![
                    (
                        String::from("PATH"),
                        vec![
                            Part::Tilde(String::new()),
                            Part::Lit(String::from("/bin:")),
                            Part::Tilde(String::from("bob")),
                            Part::Lit(String::from("/bin")),
                        ],
                    ),
                    (String::from("A"), lit("1")),
                ],
                words: vec![lit("cmd"), lit("B=2")],
                redirs: Vec::new(),
            })]),
        ),
        // 8. Word-start tilde vs quoted tilde
        (
            "echo ~ ~alice ~/x ~\"alice\" ~\\a",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Tilde(String::new())],
                vec![Part::Tilde(String::from("alice"))],
                vec![Part::Tilde(String::new()), Part::Lit(String::from("/x"))],
                vec![Part::Lit(String::from("~")), Part::Double(vec![Part::Lit(String::from("alice"))])],
                vec![Part::Lit(String::from("~")), Part::Escaped('a')],
            ])]),
        ),
        // 9. Single quotes preserve backslash and newline
        (
            "echo 'a\\nb\n$x'",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Single(String::from("a\\nb\n$x"))],
            ])]),
        ),
        // 10. Double quotes with escapes
        (
            r#"echo "a\"b\\c\$d\`e\nf""#,
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Double(vec![
                    Part::Lit(String::from("a")),
                    Part::Escaped('"'),
                    Part::Lit(String::from("b")),
                    Part::Escaped('\\'),
                    Part::Lit(String::from("c")),
                    Part::Escaped('$'),
                    Part::Lit(String::from("d")),
                    Part::Escaped('`'),
                    Part::Lit(String::from("e\\nf")),
                ])],
            ])]),
        ),
        // 11. Line continuation outside and inside double quotes
        (
            "ec\\\nho \"he\\\nllo\"",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Double(vec![Part::Lit(String::from("hello"))])],
            ])]),
        ),
        // 12. Nested quotes inside $(...) inside "..."
        (
            r#"echo "hi $(echo "world" '!')""#,
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Double(vec![
                    Part::Lit(String::from("hi ")),
                    Part::Cmd(script(vec![simp(vec![
                        lit("echo"),
                        vec![Part::Double(vec![Part::Lit(String::from("world"))])],
                        vec![Part::Single(String::from("!"))],
                    ])])),
                ])],
            ])]),
        ),
        // 13. Backquotes outside and inside double quotes
        (
            r#"echo `echo \`echo x\`` "`echo \"y\"`""#,
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Cmd(script(vec![simp(vec![
                    lit("echo"),
                    vec![Part::Cmd(script(vec![simp(vec![lit("echo"), lit("x")])]))],
                ])]))],
                vec![Part::Double(vec![Part::Cmd(script(vec![simp(vec![
                    lit("echo"),
                    vec![Part::Double(vec![Part::Lit(String::from("y"))])],
                ])]))])],
            ])]),
        ),
        // 14. Arithmetic vs subshell inside $(...)
        (
            "echo $((1+(2*3))) $( (echo a); (echo b))",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Arith(String::from("1+(2*3)"))],
                vec![Part::Cmd(script(vec![
                    Command::Subshell(script(vec![simp(vec![lit("echo"), lit("a")])]), Vec::new()),
                    Command::Subshell(script(vec![simp(vec![lit("echo"), lit("b")])]), Vec::new()),
                ]))],
            ])]),
        ),
        // 15. Plain params and special params
        (
            "echo $a $10 $@ $* $# $? $$ $! $- $",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Param(Param { name: String::from("a"), op: ParamOp::Plain })],
                vec![
                    Part::Param(Param { name: String::from("1"), op: ParamOp::Plain }),
                    Part::Lit(String::from("0")),
                ],
                vec![Part::Param(Param { name: String::from("@"), op: ParamOp::Plain })],
                vec![Part::Param(Param { name: String::from("*"), op: ParamOp::Plain })],
                vec![Part::Param(Param { name: String::from("#"), op: ParamOp::Plain })],
                vec![Part::Param(Param { name: String::from("?"), op: ParamOp::Plain })],
                vec![Part::Param(Param { name: String::from("$"), op: ParamOp::Plain })],
                vec![Part::Param(Param { name: String::from("!"), op: ParamOp::Plain })],
                vec![Part::Param(Param { name: String::from("-"), op: ParamOp::Plain })],
                lit("$"),
            ])]),
        ),
        // 16. Brace param operators
        (
            "echo ${10} ${#x} ${#} ${x:-a} ${x-b} ${x:=c} ${x=d} ${x:?e} ${x?f} ${x:+g} ${x+h} ${x%p} ${x%%q} ${x#r} ${x##s}",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Param(Param { name: String::from("10"), op: ParamOp::Plain })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::Length })],
                vec![Part::Param(Param { name: String::from("#"), op: ParamOp::Plain })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::Default(true, lit("a")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::Default(false, lit("b")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::Assign(true, lit("c")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::Assign(false, lit("d")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::Error(true, lit("e")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::Error(false, lit("f")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::Alt(true, lit("g")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::Alt(false, lit("h")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::TrimSuffix(false, lit("p")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::TrimSuffix(true, lit("q")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::TrimPrefix(false, lit("r")) })],
                vec![Part::Param(Param { name: String::from("x"), op: ParamOp::TrimPrefix(true, lit("s")) })],
            ])]),
        ),
        // 17. Quoted } inside ${x:-'}'}
        (
            "echo ${x:-'}'}",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Param(Param {
                    name: String::from("x"),
                    op: ParamOp::Default(true, vec![Part::Single(String::from("}"))]),
                })],
            ])]),
        ),
        // 18. All non-heredoc redirections
        (
            "<in >out >>app >|clob <>rw 0<&3 2>&1 cmd",
            script(vec![Command::Simple(Simple {
                assigns: Vec::new(),
                words: vec![lit("cmd")],
                redirs: vec![
                    Redir { fd: None, op: RedirOp::In, target: lit("in") },
                    Redir { fd: None, op: RedirOp::Out, target: lit("out") },
                    Redir { fd: None, op: RedirOp::Append, target: lit("app") },
                    Redir { fd: None, op: RedirOp::Clobber, target: lit("clob") },
                    Redir { fd: None, op: RedirOp::ReadWrite, target: lit("rw") },
                    Redir { fd: Some(0), op: RedirOp::DupIn, target: lit("3") },
                    Redir { fd: Some(2), op: RedirOp::DupOut, target: lit("1") },
                ],
            })]),
        ),
        // 19. Multiple heredocs on one line, quoted and <<-
        (
            "cat <<EOF <<-'END'\nhi $x\nEOF\n\t\t$y\n\tEND\n",
            script(vec![Command::Simple(Simple {
                assigns: Vec::new(),
                words: vec![lit("cat")],
                redirs: vec![
                    Redir {
                        fd: None,
                        op: RedirOp::HereDoc {
                            strip_tabs: false,
                            quoted: false,
                            body: vec![
                                Part::Lit(String::from("hi ")),
                                Part::Param(Param { name: String::from("x"), op: ParamOp::Plain }),
                                Part::Lit(String::from("\n")),
                            ],
                        },
                        target: Vec::new(),
                    },
                    Redir {
                        fd: None,
                        op: RedirOp::HereDoc {
                            strip_tabs: true,
                            quoted: true,
                            body: lit("$y\n"),
                        },
                        target: Vec::new(),
                    },
                ],
            })]),
        ),
        // 20. Quoted heredoc empty body has one Lit
        (
            "cat <<'EOF'\nEOF",
            script(vec![Command::Simple(Simple {
                assigns: Vec::new(),
                words: vec![lit("cat")],
                redirs: vec![Redir {
                    fd: None,
                    op: RedirOp::HereDoc {
                        strip_tabs: false,
                        quoted: true,
                        body: lit(""),
                    },
                    target: Vec::new(),
                }],
            })]),
        ),
        // 21. Subshell and Brace Group with redirs
        (
            "(a) >/dev/null; { b; } 2>&1",
            script(vec![
                Command::Subshell(
                    script(vec![simp(vec![lit("a")])]),
                    vec![Redir { fd: None, op: RedirOp::Out, target: lit("/dev/null") }],
                ),
                Command::Group(
                    script(vec![simp(vec![lit("b")])]),
                    vec![Redir { fd: Some(2), op: RedirOp::DupOut, target: lit("1") }],
                ),
            ]),
        ),
        // 22. Reserved words as arguments are not reserved
        (
            "echo if then fi { }",
            script(vec![simp(vec![lit("echo"), lit("if"), lit("then"), lit("fi"), lit("{"), lit("}")])]),
        ),
        // 23. If / elif / else / fi
        (
            "if c1; then b1; elif c2; then b2; else b3; fi",
            script(vec![Command::If {
                arms: vec![
                    (script(vec![simp(vec![lit("c1")])]), script(vec![simp(vec![lit("b1")])])),
                    (script(vec![simp(vec![lit("c2")])]), script(vec![simp(vec![lit("b2")])])),
                ],
                otherwise: Some(script(vec![simp(vec![lit("b3")])])),
                redirs: Vec::new(),
            }]),
        ),
        // 24. While & Until
        (
            "while t; do w; done; until u; do v; done",
            script(vec![
                Command::Loop {
                    until: false,
                    cond: script(vec![simp(vec![lit("t")])]),
                    body: script(vec![simp(vec![lit("w")])]),
                    redirs: Vec::new(),
                },
                Command::Loop {
                    until: true,
                    cond: script(vec![simp(vec![lit("u")])]),
                    body: script(vec![simp(vec![lit("v")])]),
                    redirs: Vec::new(),
                },
            ]),
        ),
        // 25. For variants
        (
            "for x; do a; done; for y in; do b; done; for z in 1 2; do c; done",
            script(vec![
                Command::For {
                    var: String::from("x"),
                    words: None,
                    body: script(vec![simp(vec![lit("a")])]),
                    redirs: Vec::new(),
                },
                Command::For {
                    var: String::from("y"),
                    words: Some(Vec::new()),
                    body: script(vec![simp(vec![lit("b")])]),
                    redirs: Vec::new(),
                },
                Command::For {
                    var: String::from("z"),
                    words: Some(vec![lit("1"), lit("2")]),
                    body: script(vec![simp(vec![lit("c")])]),
                    redirs: Vec::new(),
                },
            ]),
        ),
        // 26. Case with | patterns, optional (, empty arm, omitted ;; on last arm
        (
            "case $x in (a|b) ;; c) echo hi; esac",
            script(vec![Command::Case {
                word: vec![Part::Param(Param { name: String::from("x"), op: ParamOp::Plain })],
                arms: vec![
                    (vec![lit("a"), lit("b")], Script::default()),
                    (vec![lit("c")], script(vec![simp(vec![lit("echo"), lit("hi")])])),
                ],
                redirs: Vec::new(),
            }]),
        ),
        // 27. Function definition
        (
            "foo() { echo 1; } >out",
            script(vec![Command::Func {
                name: String::from("foo"),
                body: Box::new(Command::Group(
                    script(vec![simp(vec![lit("echo"), lit("1")])]),
                    vec![Redir { fd: None, op: RedirOp::Out, target: lit("out") }],
                )),
            }]),
        ),
        // 28. Function with subshell body
        (
            "bar () (\n echo ok\n)",
            script(vec![Command::Func {
                name: String::from("bar"),
                body: Box::new(Command::Subshell(
                    script(vec![simp(vec![lit("echo"), lit("ok")])]),
                    Vec::new(),
                )),
            }]),
        ),
        // 29. Comments inside pipelines and blocks
        (
            "a | # pipe comment\nb # end comment",
            Script {
                items: vec![AndOr {
                    first: Pipeline {
                        bang: false,
                        cmds: vec![simp(vec![lit("a")]), simp(vec![lit("b")])],
                    },
                    rest: Vec::new(),
                    background: false,
                }],
            },
        ),
        // 30. Redirection before assignment and command name with io number
        (
            "3<in A=1 cmd B=2",
            script(vec![Command::Simple(Simple {
                assigns: vec![(String::from("A"), lit("1"))],
                words: vec![lit("cmd"), lit("B=2")],
                redirs: vec![Redir { fd: Some(3), op: RedirOp::In, target: lit("in") }],
            })]),
        ),
    ];

    // Add 30+ more distinct valid scripts to reach >= 60 corpus scripts.
    let extra: Vec<(&str, Script)> = vec![
        (
            "echo 'a'\"b\"\\c",
            script(vec![simp(vec![
                lit("echo"),
                vec![
                    Part::Single(String::from("a")),
                    Part::Double(vec![Part::Lit(String::from("b"))]),
                    Part::Escaped('c'),
                ],
            ])]),
        ),
        (
            "echo \"\"",
            script(vec![simp(vec![lit("echo"), vec![Part::Double(vec![])]])]),
        ),
        (
            "echo ''",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Single(String::new())],
            ])]),
        ),
        (
            "A=",
            script(vec![Command::Simple(Simple {
                assigns: vec![(String::from("A"), Vec::new())],
                words: Vec::new(),
                redirs: Vec::new(),
            })]),
        ),
        (
            "echo $( )",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Cmd(Script::default())],
            ])]),
        ),
        (
            "echo ``",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Cmd(Script::default())],
            ])]),
        ),
        (
            "echo $(case x in a) echo y;; esac)",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Cmd(script(vec![Command::Case {
                    word: lit("x"),
                    arms: vec![(
                        vec![lit("a")],
                        script(vec![simp(vec![lit("echo"), lit("y")])]),
                    )],
                    redirs: Vec::new(),
                }]))],
            ])]),
        ),
        (
            "case x in esac",
            script(vec![Command::Case {
                word: lit("x"),
                arms: Vec::new(),
                redirs: Vec::new(),
            }]),
        ),
        (
            "case x in a)\nesac",
            script(vec![Command::Case {
                word: lit("x"),
                arms: vec![(vec![lit("a")], Script::default())],
                redirs: Vec::new(),
            }]),
        ),
        (
            "for x in a\ndo\n  echo $x\ndone",
            script(vec![Command::For {
                var: String::from("x"),
                words: Some(vec![lit("a")]),
                body: script(vec![simp(vec![
                    lit("echo"),
                    vec![Part::Param(Param {
                        name: String::from("x"),
                        op: ParamOp::Plain,
                    })],
                ])]),
                redirs: Vec::new(),
            }]),
        ),
        (
            "for x do echo $x; done",
            script(vec![Command::For {
                var: String::from("x"),
                words: None,
                body: script(vec![simp(vec![
                    lit("echo"),
                    vec![Part::Param(Param {
                        name: String::from("x"),
                        op: ParamOp::Plain,
                    })],
                ])]),
                redirs: Vec::new(),
            }]),
        ),
        (
            "if true; then\n  ok\nfi",
            script(vec![Command::If {
                arms: vec![(
                    script(vec![simp(vec![lit("true")])]),
                    script(vec![simp(vec![lit("ok")])]),
                )],
                otherwise: None,
                redirs: Vec::new(),
            }]),
        ),
        ("in arg", script(vec![simp(vec![lit("in"), lit("arg")])])),
        ("{echo}", script(vec![simp(vec![lit("{echo}")])])),
        (
            "echo a#b",
            script(vec![simp(vec![lit("echo"), lit("a#b")])]),
        ),
        (
            "echo \"#notcomment\"",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Double(vec![Part::Lit(String::from("#notcomment"))])],
            ])]),
        ),
        (
            "echo 2 >out",
            script(vec![Command::Simple(Simple {
                assigns: Vec::new(),
                words: vec![lit("echo"), lit("2")],
                redirs: vec![Redir {
                    fd: None,
                    op: RedirOp::Out,
                    target: lit("out"),
                }],
            })]),
        ),
        (
            "cat <<EOF\nline1\\\nline2\nEOF\n",
            script(vec![Command::Simple(Simple {
                assigns: Vec::new(),
                words: vec![lit("cat")],
                redirs: vec![Redir {
                    fd: None,
                    op: RedirOp::HereDoc {
                        strip_tabs: false,
                        quoted: false,
                        body: lit("line1line2\n"),
                    },
                    target: Vec::new(),
                }],
            })]),
        ),
        (
            "cat <<EOF\na\\\nEOF\nEOF\n",
            script(vec![Command::Simple(Simple {
                assigns: Vec::new(),
                words: vec![lit("cat")],
                redirs: vec![Redir {
                    fd: None,
                    op: RedirOp::HereDoc {
                        strip_tabs: false,
                        quoted: false,
                        body: lit("aEOF\n"),
                    },
                    target: Vec::new(),
                }],
            })]),
        ),
        (
            "echo ${#@} ${##} ${#-}",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Param(Param {
                    name: String::from("@"),
                    op: ParamOp::Length,
                })],
                vec![Part::Param(Param {
                    name: String::from("#"),
                    op: ParamOp::Length,
                })],
                vec![Part::Param(Param {
                    name: String::from("-"),
                    op: ParamOp::Length,
                })],
            ])]),
        ),
        (
            "echo ${#-x}",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Param(Param {
                    name: String::from("#"),
                    op: ParamOp::Default(false, lit("x")),
                })],
            ])]),
        ),
        (
            "echo \"${x:-a b}\"",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Double(vec![Part::Param(Param {
                    name: String::from("x"),
                    op: ParamOp::Default(true, lit("a b")),
                })])],
            ])]),
        ),
        (
            "a &",
            Script {
                items: vec![AndOr {
                    first: Pipeline {
                        bang: false,
                        cmds: vec![simp(vec![lit("a")])],
                    },
                    rest: Vec::new(),
                    background: true,
                }],
            },
        ),
        (
            "a && b || c",
            Script {
                items: vec![AndOr {
                    first: Pipeline {
                        bang: false,
                        cmds: vec![simp(vec![lit("a")])],
                    },
                    rest: vec![
                        (
                            true,
                            Pipeline {
                                bang: false,
                                cmds: vec![simp(vec![lit("b")])],
                            },
                        ),
                        (
                            false,
                            Pipeline {
                                bang: false,
                                cmds: vec![simp(vec![lit("c")])],
                            },
                        ),
                    ],
                    background: false,
                }],
            },
        ),
        (
            "echo $((1 + (2)))",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Arith(String::from("1 + (2)"))],
            ])]),
        ),
        (
            "echo ${x:-~}",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Param(Param {
                    name: String::from("x"),
                    op: ParamOp::Default(true, vec![Part::Tilde(String::new())]),
                })],
            ])]),
        ),
        (
            "cat <<EOF | wc\nhi\nEOF\n",
            Script {
                items: vec![AndOr {
                    first: Pipeline {
                        bang: false,
                        cmds: vec![
                            Command::Simple(Simple {
                                assigns: Vec::new(),
                                words: vec![lit("cat")],
                                redirs: vec![Redir {
                                    fd: None,
                                    op: RedirOp::HereDoc {
                                        strip_tabs: false,
                                        quoted: false,
                                        body: lit("hi\n"),
                                    },
                                    target: Vec::new(),
                                }],
                            }),
                            simp(vec![lit("wc")]),
                        ],
                    },
                    rest: Vec::new(),
                    background: false,
                }],
            },
        ),
        (
            "echo $(cat <<EOF\nhi\nEOF\n)",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Cmd(script(vec![Command::Simple(Simple {
                    assigns: Vec::new(),
                    words: vec![lit("cat")],
                    redirs: vec![Redir {
                        fd: None,
                        op: RedirOp::HereDoc {
                            strip_tabs: false,
                            quoted: false,
                            body: lit("hi\n"),
                        },
                        target: Vec::new(),
                    }],
                })]))],
            ])]),
        ),
        (
            "f() if true; then a; fi",
            script(vec![Command::Func {
                name: String::from("f"),
                body: Box::new(Command::If {
                    arms: vec![(
                        script(vec![simp(vec![lit("true")])]),
                        script(vec![simp(vec![lit("a")])]),
                    )],
                    otherwise: None,
                    redirs: Vec::new(),
                }),
            }]),
        ),
        (
            "echo '\\\\'",
            script(vec![simp(vec![
                lit("echo"),
                vec![Part::Single(String::from("\\\\"))],
            ])]),
        ),
    ];

    let all: Vec<(&str, Script)> = cases.into_iter().chain(extra).collect();
    assert!(all.len() >= 60);
    for (idx, (src, expected)) in all.iter().enumerate() {
        let got = parse(src).unwrap_or_else(|e| panic!("case {idx} failed: {src:?} -> {e:?}"));
        assert_eq!(&got, expected, "case {idx}: {src:?}");
    }
}

#[test]
fn parser_corpus_errors() {
    let cases: Vec<(&str, u32, u32, bool)> = vec![
        ("echo 'unterminated", 1, 6, true),
        ("echo \"unterminated", 1, 6, true),
        ("echo $(cmd", 1, 11, true),
        ("echo $((1+2", 1, 7, true),
        ("echo `cmd", 1, 6, true),
        ("echo ${x", 1, 7, true),
        ("(echo a", 1, 8, true),
        ("{ echo a", 1, 9, true),
        ("if true; then echo a", 1, 21, true),
        ("while true; do echo a", 1, 22, true),
        ("for x in a b", 1, 13, true),
        ("case x in a) echo 1;;", 1, 22, true),
        ("echo a |", 1, 9, true),
        ("echo a &&", 1, 10, true),
        ("echo a ||\n", 2, 1, true),
        ("cat <<EOF\nhello", 1, 5, true),
        ("cat <<EOF", 1, 5, true),
        ("!", 1, 2, true),
        ("()", 1, 3, false),
        ("{ }", 1, 4, false),
        ("; echo a", 1, 1, false),
        ("echo a;;", 1, 7, false),
        ("echo >", 1, 7, false),
        ("echo ${}", 1, 8, false),
        ("fi", 1, 3, false),
        ("then", 1, 5, false),
        ("foo(bar)", 1, 5, false),
        ("foo() echo hi", 1, 7, false),
        ("99999999999999999>out", 1, 1, false),
        ("if ; then echo a; fi", 1, 4, false),
    ];

    for (idx, (src, line, col, incomplete)) in cases.iter().enumerate() {
        let err: ParseError = parse(src).expect_err(&format!("case {idx} should fail: {src:?}"));
        assert_eq!(err.line, *line, "case {idx} line for {src:?}: {err:?}");
        assert_eq!(err.col, *col, "case {idx} col for {src:?}: {err:?}");
        assert_eq!(
            err.incomplete, *incomplete,
            "case {idx} incomplete for {src:?}: {err:?}"
        );
    }
}
