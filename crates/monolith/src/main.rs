//! `monolith`: give a local AI model its own computer and drive it.
//!
//! The leader owns this file. Each subcommand lives in its module as `cmd(...) -> Result<CmdOut>`;
//! this file only parses the first word, prints the result and maps errors to exit codes.

use std::ffi::OsString;
use std::process::ExitCode;

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};
use monolith::types::{CmdOut, Error};
use monolith::{agent, computer, eval, memory};

const USAGE: &str = "usage: monolith <command> [args]
  new NAME                      make an AI computer
  list                          list computers
  rm NAME                       delete a computer
  exec NAME -- TOOL [ARGS]      run one Monolith tool inside a computer
  log NAME [--limit N] [--offset N]   show a computer's journal
  memory NAME add KIND TEXT | list | search QUERY   KIND is fact, lesson or skill
  run NAME --task TEXT [--level 0|1|2] [--model-url URL] [--max-steps N] [--mind DIR] [--freeze]
                                let the local model work on a task
  eval --suite FILE [--suite FILE]... --out DIR [--level 0|1|2] [--selftest] [--mind DIR] [--freeze]
       [--only ID,ID] [--limit N] [--model-url URL] [--seed N]
                                run an exam and write results
  --describe                    print the machine-readable manifest
  --json <command> ...          print one JSON envelope instead of text";

const fn arg(name: &'static str, kind: Kind, required: bool) -> Arg {
    Arg {
        name,
        kind,
        required,
        default: None,
        choices: &[],
    }
}

const NAME: Arg = arg("NAME", Kind::Text, true);
const LEVEL: Arg = Arg {
    name: "--level",
    kind: Kind::Choice,
    required: false,
    default: Some("2"),
    choices: &["0", "1", "2"],
};
const MODEL_URL: Arg = Arg {
    name: "--model-url",
    kind: Kind::Text,
    required: false,
    default: Some("http://127.0.0.1:8080"),
    choices: &[],
};
const MAX_STEPS: Arg = arg("--max-steps", Kind::Int, false);
const MIND: Arg = arg("--mind", Kind::Path, false);
const FREEZE: Arg = arg("--freeze", Kind::Flag, false);
const SEED: Arg = arg("--seed", Kind::Int, false);
const TEMP: Arg = arg("--temp", Kind::Text, false);
const THINKING: Arg = arg("--thinking", Kind::Flag, false);

const COMMANDS: &[Command] = &[
    Command {
        name: "new",
        summary: "Make an AI computer: a folder with root/, journal.jsonl and memory/",
        effect: Effect::Write,
        args: &[NAME],
        returns: "name, dir",
        example: "monolith --json new dev1",
    },
    Command {
        name: "list",
        summary: "List the computers",
        effect: Effect::Read,
        args: &[],
        returns: "items: [{name, dir}], total",
        example: "monolith --json list",
    },
    Command {
        name: "rm",
        summary: "Delete a computer and everything in it",
        effect: Effect::Write,
        args: &[NAME],
        returns: "name, removed",
        example: "monolith --json rm dev1",
    },
    Command {
        name: "exec",
        summary: "Run one Monolith tool inside a computer (cwd root/, fenced, journaled); put the tool after --",
        effect: Effect::Exec,
        args: &[NAME, arg("TOOL", Kind::Text, true), arg("ARGS", Kind::Text, false)],
        returns: "exit, stdout, stderr (capped), timed_out, duration_ms",
        example: "monolith --json exec dev1 -- chronos --json init proj",
    },
    Command {
        name: "log",
        summary: "Show a computer's journal, oldest first",
        effect: Effect::Read,
        args: &[NAME, arg("--limit", Kind::Int, false), arg("--offset", Kind::Int, false)],
        returns: "items: [{seq, ts_ms, session, kind, data}], total, more, next_offset",
        example: "monolith --json log dev1 --limit 20",
    },
    Command {
        name: "memory",
        summary: "Add to, list or search a computer's memory: memory NAME add KIND TEXT | list | search QUERY",
        effect: Effect::Write,
        args: &[NAME, arg("ACTION", Kind::Text, true), arg("ARGS", Kind::Text, false)],
        returns: "add: id; list and search: items: [{id, kind, text, verified}], total",
        example: "monolith --json memory dev1 search \"which port\"",
    },
    Command {
        name: "run",
        summary: "Let the local model work on one task inside a computer",
        effect: Effect::Exec,
        args: &[
            NAME,
            arg("--task", Kind::Text, true),
            LEVEL,
            MODEL_URL,
            MAX_STEPS,
            MIND,
            FREEZE,
            SEED,
            TEMP,
            THINKING,
        ],
        returns: "answer, stop, steps, invalid_actions, failed_actions, prompt_tokens, completion_tokens, model_ms",
        example: "monolith --json run dev1 --task \"make a repository in proj and commit notes.txt\"",
    },
    Command {
        name: "eval",
        summary: "Run an exam (suite files) with the model, or with the reference solutions (--selftest)",
        effect: Effect::Exec,
        args: &[
            arg("--suite", Kind::Path, true),
            arg("--out", Kind::Path, true),
            arg("--selftest", Kind::Flag, false),
            arg("--only", Kind::Text, false),
            arg("--limit", Kind::Int, false),
            LEVEL,
            MODEL_URL,
            MAX_STEPS,
            MIND,
            FREEZE,
            SEED,
            TEMP,
            THINKING,
        ],
        returns: "tasks, passed, failed, errors, pass_rate, results: [{id, passed, steps, ...}], report",
        example: "monolith --json eval --suite suites/train.tasks --out runs/r0 --level 0",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "monolith",
    version: env!("CARGO_PKG_VERSION"),
    summary: "A harness that gives a local AI model its own computer made of the Monolith tools",
    commands: COMMANDS,
};

fn main() -> ExitCode {
    let mut raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut raw);
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    let args: Vec<String> = raw
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let (sub, rest) = match args.split_first() {
        Some((s, r)) => (s.as_str(), r),
        None => ("", &[][..]),
    };
    let result = match sub {
        "new" | "list" | "rm" | "exec" | "log" => computer::cmd(sub, rest),
        "memory" => memory::cmd(rest),
        "run" => agent::cmd(rest),
        "eval" => eval::cmd(rest),
        "--help" | "-h" | "help" => Ok(CmdOut {
            data: Json::obj([("usage", Json::Str(USAGE.into()))]),
            text: USAGE.into(),
            exit: 0,
        }),
        "" => Err(Error::new(Code::Usage, "no command").hint("run: monolith --help")),
        other => Err(
            Error::new(Code::Usage, format!("unknown command `{other}`"))
                .hint("run: monolith --describe"),
        ),
    };
    match result {
        Ok(out) => {
            if json {
                print!("{}", envelope::ok("monolith", sub, out.data));
            } else if !out.text.is_empty() {
                println!("{}", out.text.trim_end());
            }
            ExitCode::from(out.exit)
        }
        Err(e) => {
            eprintln!("monolith: {e}");
            if json {
                print!(
                    "{}",
                    envelope::err("monolith", sub, e.code, &e.message, e.hint.as_deref())
                );
            }
            ExitCode::from(if e.code == Code::Usage { 2 } else { 1 })
        }
    }
}
