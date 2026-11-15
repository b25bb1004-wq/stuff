//! Deterministic demonstration of the `krypton` POSIX shell crate.

use std::collections::BTreeMap;
use std::io;

use krypton::ast::{Command, Script};
use krypton::exec::Shell;
use krypton::{arith, expand, expand_one, glob_match, parse, Env};

struct DemoEnv {
    vars: BTreeMap<String, String>,
    args: Vec<String>,
}

impl DemoEnv {
    fn new() -> Self {
        let mut vars = BTreeMap::new();
        vars.insert(String::from("NAME"), String::from("world"));
        vars.insert(String::from("COUNT"), String::from("3"));
        Self {
            vars,
            args: vec![String::from("alpha"), String::from("beta")],
        }
    }
}

impl Env for DemoEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn set(&mut self, name: &str, value: &str) {
        self.vars.insert(String::from(name), String::from(value));
    }

    fn positional(&self) -> Vec<String> {
        self.args.clone()
    }

    fn command(&mut self, _s: &Script) -> String {
        String::new()
    }

    fn home(&self, _user: &str) -> Option<String> {
        Some(String::from("/home/demo"))
    }

    fn list_dir(&self, _dir: &str) -> Vec<String> {
        vec![
            String::from("a.rs"),
            String::from("b.txt"),
            String::from("c.rs"),
        ]
    }
}

fn main() {
    let mut env = DemoEnv::new();

    let src = "echo \"hello $NAME\" *.rs\nif true; then echo ${MISSING:-fallback}; fi";
    match parse(src) {
        Ok(script) => {
            println!("parse: {} top-level items", script.items.len());
            if let Some(first) = script.items.first() {
                if let Some(Command::Simple(simple)) = first.first.cmds.first() {
                    match expand(&simple.words, &mut env) {
                        Ok(words) => println!("expand: {:?}", words),
                        Err(e) => println!("expand error: {:?}", e),
                    }
                }
            }
            if let Some(second) = script.items.get(1) {
                if let Some(Command::If { arms, .. }) = second.first.cmds.first() {
                    if let Some((_, body)) = arms.first() {
                        if let Some(and_or) = body.items.first() {
                            if let Some(Command::Simple(simple)) = and_or.first.cmds.first() {
                                if let Some(word) = simple.words.get(1) {
                                    match expand_one(word, &mut env) {
                                        Ok(val) => println!("expand_one: {}", val),
                                        Err(e) => println!("expand_one error: {:?}", e),
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Err(e) => println!("parse error: {}:{} {}", e.line, e.col, e.msg),
    }

    match arith("1 + 2 * 3 + COUNT", &mut env) {
        Ok(val) => println!("arith: 1 + 2 * 3 + COUNT = {}", val),
        Err(e) => println!("arith error: {:?}", e),
    }

    println!(
        "glob_match: *.rs on lib.rs = {}, on README.md = {}",
        glob_match("*.rs", "lib.rs"),
        glob_match("*.rs", "README.md")
    );

    let mut shell = Shell::new();
    let mut stdin = io::empty();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let script =
        "x=6; y=7; echo \"product=$((x * y))\"; for w in one two; do echo \"item=$w\"; done";
    let status = shell.run(script, &mut stdin, &mut stdout, &mut stderr);
    let out_text = String::from_utf8_lossy(&stdout);
    println!("shell status: {}", status);
    for line in out_text.lines() {
        println!("shell out: {}", line);
    }
}
