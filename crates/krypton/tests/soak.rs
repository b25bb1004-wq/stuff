//! Fuzz soak tests for `krypton` (`parse`, `arith`, `expand`, `expand_one`, `glob_match`).
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use assay::Rng;
use krypton::ast::{Command, Param, ParamOp, Part, Script, Word};
use krypton::{arith, expand, expand_one, glob_match, parse, Env};

#[allow(clippy::unusual_byte_groupings)]
const DEFAULT_SEED: u64 = 0x5EED_797;
const DEFAULT_MS: u64 = 300;
const MAX_INPUT_LEN: usize = 2048;

fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(val) => {
            let trimmed = val.trim();
            if let Some(hex) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
            {
                u64::from_str_radix(&hex.replace('_', ""), 16).expect("STRATA_SOAK_SEED: bad hex")
            } else {
                trimmed
                    .replace('_', "")
                    .parse::<u64>()
                    .expect("STRATA_SOAK_SEED: bad number")
            }
        }
        Err(_) => DEFAULT_SEED,
    }
}

fn soak_budget() -> Duration {
    let ms = std::env::var("STRATA_SOAK_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MS);
    Duration::from_millis(ms)
}

#[derive(Default)]
struct SoakEnv {
    vars: BTreeMap<String, String>,
    nounset: bool,
}

impl SoakEnv {
    fn seeded() -> Self {
        let mut vars = BTreeMap::new();
        vars.insert("HOME".into(), "/home/soak".into());
        vars.insert("IFS".into(), " \t\n".into());
        vars.insert("x".into(), "hello world".into());
        vars.insert("y".into(), "42".into());
        vars.insert("empty".into(), String::new());
        Self {
            vars,
            nounset: false,
        }
    }
}

impl Env for SoakEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn set(&mut self, name: &str, value: &str) {
        if self.vars.len() < 64 && value.len() <= 4096 {
            self.vars.insert(name.to_string(), value.to_string());
        }
    }

    fn positional(&self) -> Vec<String> {
        vec!["one".into(), "two three".into(), String::new()]
    }

    fn command(&mut self, _s: &Script) -> String {
        String::from("cmd_out")
    }

    fn home(&self, user: &str) -> Option<String> {
        if user.is_empty() {
            Some("/home/soak".into())
        } else {
            Some(format!("/home/{user}"))
        }
    }

    fn list_dir(&self, _dir: &str) -> Vec<String> {
        vec![
            "alpha.rs".into(),
            "beta.txt".into(),
            ".hidden".into(),
            "dir".into(),
        ]
    }

    fn nounset(&self) -> bool {
        self.nounset
    }
}

const SHELL_CORPUS: &[&str] = &[
    "echo hello world",
    "VAR=123 cmd arg1 'arg2' \"arg3 $VAR\"",
    "if true; then echo yes; elif false; then echo no; else echo maybe; fi",
    "for i in a b c; do echo \"$i\"; done",
    "while false; do break; done",
    "until true; do continue; done",
    "case $x in a|b) echo 1;; *) echo 2;; esac",
    "f() { (cd /tmp && ls -la) | grep foo; }",
    "cat <<EOF\nhello $x\nEOF\n",
    "cat <<-'END'\n\tliteral\n\tEND\n",
    "echo ${x:-default} ${x:=set} ${x:+alt} ${#x} ${x#h*} ${x%%d}",
    "echo $((1 + 2 * 3 - (4 / 2))) $(echo nested) `echo backtick`",
    "! true && false || { echo fallback; } &",
    "cmd 2>&1 >/dev/null <<'Q'\nbody\nQ\n",
];

const ARITH_CORPUS: &[&str] = &[
    "0",
    "1 + 2 * 3",
    "0x1f + 077 - 10",
    "x = 5, x += 3, x *= 2",
    "(1 << 4) | (3 & 1) ^ 2",
    "1 ? 10 : 20",
    "!0 + ~1 - -5",
    "10 / 3 + 10 % 3",
    "y && (y > 10) || (y <= 100)",
    "a = b = c = 1",
];

const GLOB_CORPUS: &[&str] = &[
    "*.rs",
    "a?c",
    "[a-z]*",
    "[!0-9]?",
    "[[:alpha:]][[:digit:]]+",
    "\\*\\[a\\]\\?",
    "[]a]",
    "a*b*c",
    "",
    "*",
];

const SHELL_TOKENS: &[u8] = b" \t\n;|&()<>'\"\\$`{}[]*?!~=#%+:-0123456789abcxyz\0";

fn random_text(rng: &mut Rng, max_len: usize) -> String {
    let len = rng.below(max_len + 1);
    if rng.chance(1, 2) {
        let mut bytes = vec![0u8; len];
        rng.fill(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(*rng.pick(SHELL_TOKENS));
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

fn mutate_text(rng: &mut Rng, corpus: &[&str]) -> String {
    let base = *rng.pick(corpus);
    let mut buf = base.as_bytes().to_vec();
    let ops = rng.range(1, 5);

    for _ in 0..ops {
        match rng.below(5) {
            0 if !buf.is_empty() => {
                // Bit flip
                let idx = rng.below(buf.len());
                let bit = 1u8 << rng.below(8);
                buf[idx] ^= bit;
            }
            1 if !buf.is_empty() => {
                // Truncation
                let new_len = rng.below(buf.len() + 1);
                buf.truncate(new_len);
            }
            2 => {
                // Splice with another corpus item
                let other = (*rng.pick(corpus)).as_bytes();
                let split = if buf.is_empty() {
                    0
                } else {
                    rng.below(buf.len() + 1)
                };
                let mut next = Vec::with_capacity((buf.len() + other.len()).min(MAX_INPUT_LEN));
                next.extend_from_slice(&buf[..split]);
                next.extend_from_slice(other);
                if split < buf.len() {
                    next.extend_from_slice(&buf[split..]);
                }
                next.truncate(MAX_INPUT_LEN);
                buf = next;
            }
            3 if !buf.is_empty() => {
                // Repeat a chunk
                let start = rng.below(buf.len());
                let end = rng.range(start as u64, buf.len() as u64 + 1) as usize;
                let chunk = buf[start..end].to_vec();
                let reps = rng.range(2, 6) as usize;
                for _ in 0..reps {
                    if buf.len() + chunk.len() > MAX_INPUT_LEN {
                        break;
                    }
                    buf.extend_from_slice(&chunk);
                }
            }
            _ => {
                // Insert a shell token byte
                if buf.len() < MAX_INPUT_LEN {
                    let idx = if buf.is_empty() {
                        0
                    } else {
                        rng.below(buf.len() + 1)
                    };
                    buf.insert(idx, *rng.pick(SHELL_TOKENS));
                }
            }
        }
    }

    String::from_utf8_lossy(&buf).into_owned()
}

fn collect_words(script: &Script, out: &mut Vec<Word>) {
    for ao in &script.items {
        for cmd in &ao.first.cmds {
            collect_cmd_words(cmd, out);
        }
        for (_, p) in &ao.rest {
            for cmd in &p.cmds {
                collect_cmd_words(cmd, out);
            }
        }
    }
}

fn collect_cmd_words(cmd: &Command, out: &mut Vec<Word>) {
    match cmd {
        Command::Simple(s) => {
            for (_, w) in &s.assigns {
                out.push(w.clone());
            }
            out.extend(s.words.iter().cloned());
        }
        Command::Subshell(sc, _) | Command::Group(sc, _) => collect_words(sc, out),
        Command::If {
            arms, otherwise, ..
        } => {
            for (c, b) in arms {
                collect_words(c, out);
                collect_words(b, out);
            }
            if let Some(o) = otherwise {
                collect_words(o, out);
            }
        }
        Command::Loop { cond, body, .. } => {
            collect_words(cond, out);
            collect_words(body, out);
        }
        Command::For { words, body, .. } => {
            if let Some(ws) = words {
                out.extend(ws.iter().cloned());
            }
            collect_words(body, out);
        }
        Command::Case { word, arms, .. } => {
            out.push(word.clone());
            for (pats, body) in arms {
                out.extend(pats.iter().cloned());
                collect_words(body, out);
            }
        }
        Command::Func { body, .. } => collect_cmd_words(body, out),
    }
}

fn build_valid_words() -> Vec<Word> {
    let mut words = Vec::new();
    for src in SHELL_CORPUS {
        if let Ok(script) = parse(src) {
            collect_words(&script, &mut words);
        }
    }
    words
}

fn check_case_time(start: Instant, target: &str, seed: u64, case: u64) {
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "target {target} exceeded 1s ({elapsed:?}) on seed {seed:#x} case {case}"
    );
}

fn soak_parse(seed: u64, budget: Duration) -> u64 {
    let mut rng = Rng::new(seed);
    let start = Instant::now();
    let mut cases = 0u64;
    while start.elapsed() < budget || cases < 10 {
        let input = if cases.is_multiple_of(2) {
            random_text(&mut rng, 512)
        } else {
            mutate_text(&mut rng, SHELL_CORPUS)
        };
        let t0 = Instant::now();
        let _ = parse(&input);
        check_case_time(t0, "parse", seed, cases);
        cases += 1;
    }
    cases
}

fn soak_arith(seed: u64, budget: Duration) -> u64 {
    let mut rng = Rng::new(seed ^ 0xA217_0001);
    let start = Instant::now();
    let mut cases = 0u64;
    while start.elapsed() < budget || cases < 10 {
        let input = if cases.is_multiple_of(2) {
            random_text(&mut rng, 256)
        } else {
            mutate_text(&mut rng, ARITH_CORPUS)
        };
        let mut env = SoakEnv::seeded();
        env.nounset = rng.chance(1, 4);
        let t0 = Instant::now();
        let _ = arith(&input, &mut env);
        check_case_time(t0, "arith", seed, cases);
        cases += 1;
    }
    cases
}

fn random_word(rng: &mut Rng, valid_words: &[Word]) -> Word {
    if rng.chance(1, 2) && !valid_words.is_empty() {
        let candidate = mutate_text(rng, SHELL_CORPUS);
        if let Ok(sc) = parse(&candidate) {
            let mut extracted = Vec::new();
            collect_words(&sc, &mut extracted);
            if !extracted.is_empty() {
                return rng.pick(&extracted).clone();
            }
        }
        rng.pick(valid_words).clone()
    } else {
        let s = random_text(rng, 128);
        match rng.below(7) {
            0 => vec![Part::Lit(s)],
            1 => vec![Part::Single(s)],
            2 => vec![Part::Escaped(s.chars().next().unwrap_or('a'))],
            3 => vec![Part::Double(vec![Part::Lit(s)])],
            4 => vec![Part::Param(Param {
                name: s,
                op: ParamOp::Plain,
            })],
            5 => vec![Part::Arith(s)],
            _ => vec![Part::Tilde(s)],
        }
    }
}

fn soak_expand(seed: u64, budget: Duration, valid_words: &[Word]) -> u64 {
    let mut rng = Rng::new(seed ^ 0xE89A_0002);
    let start = Instant::now();
    let mut cases = 0u64;
    while start.elapsed() < budget || cases < 10 {
        let count = rng.range(0, 5) as usize;
        let words: Vec<Word> = (0..count)
            .map(|_| random_word(&mut rng, valid_words))
            .collect();
        let mut env = SoakEnv::seeded();
        env.nounset = rng.chance(1, 4);
        let t0 = Instant::now();
        let _ = expand(&words, &mut env);
        check_case_time(t0, "expand", seed, cases);
        cases += 1;
    }
    cases
}

fn soak_expand_one(seed: u64, budget: Duration, valid_words: &[Word]) -> u64 {
    let mut rng = Rng::new(seed ^ 0xE891_0003);
    let start = Instant::now();
    let mut cases = 0u64;
    while start.elapsed() < budget || cases < 10 {
        let word = random_word(&mut rng, valid_words);
        let mut env = SoakEnv::seeded();
        env.nounset = rng.chance(1, 4);
        let t0 = Instant::now();
        let _ = expand_one(&word, &mut env);
        check_case_time(t0, "expand_one", seed, cases);
        cases += 1;
    }
    cases
}

fn soak_glob_match(seed: u64, budget: Duration) -> u64 {
    let mut rng = Rng::new(seed ^ 0x610B_0004);
    let start = Instant::now();
    let mut cases = 0u64;
    while start.elapsed() < budget || cases < 10 {
        let (pat, name) = if cases.is_multiple_of(2) {
            (random_text(&mut rng, 128), random_text(&mut rng, 128))
        } else {
            (
                mutate_text(&mut rng, GLOB_CORPUS),
                mutate_text(&mut rng, GLOB_CORPUS),
            )
        };
        let t0 = Instant::now();
        let _ = glob_match(&pat, &name);
        check_case_time(t0, "glob_match", seed, cases);
        cases += 1;
    }
    cases
}

#[test]
fn soak_all_targets() {
    let seed = soak_seed();
    let budget = soak_budget();
    let valid_words = build_valid_words();

    let parse_cases = soak_parse(seed, budget);
    let arith_cases = soak_arith(seed, budget);
    let expand_cases = soak_expand(seed, budget, &valid_words);
    let expand_one_cases = soak_expand_one(seed, budget, &valid_words);
    let glob_cases = soak_glob_match(seed, budget);

    println!(
        "soak seed={seed:#x} budget_ms={} parse={parse_cases} arith={arith_cases} expand={expand_cases} expand_one={expand_one_cases} glob_match={glob_cases}",
        budget.as_millis()
    );
}
