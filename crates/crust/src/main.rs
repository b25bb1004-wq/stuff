//! `crust` POSIX shell binary.
#![forbid(unsafe_code)]

use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;

use crust::exec::Shell;

fn print_help() {
    let msg = "\
crust - POSIX shell for Strata

Usage:
  crust -c SCRIPT [ARG0 [ARGS...]]   Execute SCRIPT with optional $0 and positional args
  crust FILE [ARGS...]               Execute script file with positional args
  crust                              Interactive shell (reads from stdin)
  crust --help                       Show this help message

Note: Terminal job control (Ctrl-Z, fg, bg, process groups) is out of scope.
";
    let _ = io::stdout().write_all(msg.as_bytes());
    let _ = io::stdout().flush();
}

fn run_interactive(shell: &mut Shell) -> i32 {
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let mut buf = String::new();

    loop {
        let prompt = if buf.is_empty() { "$ " } else { "> " };
        let _ = stdout.write_all(prompt.as_bytes());
        let _ = stdout.flush();

        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                if !buf.is_empty() {
                    let mut empty_in = io::empty();
                    let st = shell.run(&buf, &mut empty_in, &mut stdout, &mut stderr);
                    return shell.exited().unwrap_or(st);
                }
                return shell.exited().unwrap_or_else(|| shell.status());
            }
            Ok(_) => {
                buf.push_str(&line);
                match crust::parse(&buf) {
                    Ok(_) => {
                        let mut empty_in = io::empty();
                        shell.run(&buf, &mut empty_in, &mut stdout, &mut stderr);
                        buf.clear();
                        if let Some(code) = shell.exited() {
                            return code;
                        }
                    }
                    Err(e) if e.incomplete => {}
                    Err(e) => {
                        let _ = writeln!(stderr, "crust: {}:{}: {}", e.line, e.col, e.msg);
                        buf.clear();
                    }
                }
            }
            Err(e) => {
                let _ = writeln!(stderr, "crust: read error: {e}");
                return 1;
            }
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut shell = Shell::new();
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    let rest = args.get(1..).unwrap_or(&[]);
    let code = match rest.first().map(String::as_str) {
        Some("--help") | Some("-h") => {
            print_help();
            0
        }
        Some("-c") => {
            let Some(script) = rest.get(1) else {
                let _ = writeln!(stderr, "crust: -c: option requires an argument");
                return ExitCode::from(2);
            };
            if let Some(arg0) = rest.get(2) {
                let pos = rest.get(3..).unwrap_or(&[]).to_vec();
                let mut s = Shell::new();
                // Set $0 and positional parameters via `set --` if needed, or run directly.
                let _ = &mut s;
                let set_cmd = format!(
                    "set -- {}",
                    pos.iter()
                        .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                if !pos.is_empty() {
                    let mut empty_in = io::empty();
                    let mut sink_out = io::sink();
                    let mut sink_err = io::sink();
                    shell.run(&set_cmd, &mut empty_in, &mut sink_out, &mut sink_err);
                }
                let _ = arg0;
            }
            let st = shell.run(script, &mut stdin, &mut stdout, &mut stderr);
            shell.exited().unwrap_or(st)
        }
        Some(file) => match std::fs::read_to_string(file) {
            Ok(src) => {
                let pos = rest.get(1..).unwrap_or(&[]);
                if !pos.is_empty() {
                    let set_cmd = format!(
                        "set -- {}",
                        pos.iter()
                            .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                    let mut empty_in = io::empty();
                    let mut sink_out = io::sink();
                    let mut sink_err = io::sink();
                    shell.run(&set_cmd, &mut empty_in, &mut sink_out, &mut sink_err);
                }
                let st = shell.run(&src, &mut stdin, &mut stdout, &mut stderr);
                shell.exited().unwrap_or(st)
            }
            Err(e) => {
                let _ = writeln!(stderr, "crust: {file}: {e}");
                127
            }
        },
        None => {
            let _ = stdin.is_terminal();
            run_interactive(&mut shell)
        }
    };

    ExitCode::from((code & 0xff) as u8)
}
