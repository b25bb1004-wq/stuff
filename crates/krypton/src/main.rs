//! `krypton` POSIX shell binary.
#![deny(unsafe_code)]

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;

use krypton::exec::Shell;

mod interactive;
mod term;

fn print_help() {
    let msg = "\
krypton - POSIX shell for Monolith

Usage:
  krypton -c SCRIPT [ARG0 [ARGS...]]   Execute SCRIPT with optional $0 and positional args
  krypton FILE [ARGS...]               Execute script file with positional args (or - for stdin)
  krypton                              Interactive shell (reads from stdin)
  krypton --help                       Show this help message
  krypton --version                    Show version information

Examples:
  krypton -c 'echo hello'
  krypton script.sh arg1 arg2
  krypton

Note: Terminal job control (Ctrl-Z, fg, bg, process groups) is out of scope.
";
    let _ = io::stdout().write_all(msg.as_bytes());
    let _ = io::stdout().flush();
}

fn print_version() {
    let _ = writeln!(io::stdout(), "krypton {}", env!("CARGO_PKG_VERSION"));
    let _ = io::stdout().flush();
}

fn set_positional(shell: &mut Shell, pos: &[String]) {
    if pos.is_empty() {
        return;
    }
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
                match krypton::parse(&buf, krypton::Escapes::host()) {
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
                        let _ = writeln!(stderr, "krypton: {}:{}: {}", e.line, e.col, e.msg);
                        buf.clear();
                    }
                }
            }
            Err(e) => {
                let _ = writeln!(stderr, "krypton: read error: {e}");
                return 1;
            }
        }
    }
}

fn run_file(
    shell: &mut Shell,
    file: &str,
    pos: &[String],
    stdin: &mut io::Stdin,
    stdout: &mut io::Stdout,
    stderr: &mut io::Stderr,
) -> i32 {
    match std::fs::read_to_string(file) {
        Ok(src) => {
            set_positional(shell, pos);
            let st = shell.run(&src, stdin, stdout, stderr);
            shell.exited().unwrap_or(st)
        }
        Err(e) => {
            let _ = writeln!(stderr, "krypton: {file}: {e}");
            1
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut shell = Shell::new();
    shell.set_escapes(krypton::Escapes::host());
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    let rest = args.get(1..).unwrap_or(&[]);
    let code = match rest.first().map(String::as_str) {
        Some("--help") | Some("-h") => {
            print_help();
            0
        }
        Some("--version") => {
            print_version();
            0
        }
        Some("-c") => {
            let Some(script) = rest.get(1) else {
                let _ = writeln!(stderr, "krypton: -c: option requires an argument");
                return ExitCode::from(2);
            };
            if rest.get(2).is_some() {
                set_positional(&mut shell, rest.get(3..).unwrap_or(&[]));
            }
            let st = shell.run(script, &mut stdin, &mut stdout, &mut stderr);
            shell.exited().unwrap_or(st)
        }
        Some("-") => {
            let mut src = String::new();
            match stdin.read_to_string(&mut src) {
                Ok(_) => {
                    set_positional(&mut shell, rest.get(1..).unwrap_or(&[]));
                    let mut empty_in = io::empty();
                    let st = shell.run(&src, &mut empty_in, &mut stdout, &mut stderr);
                    shell.exited().unwrap_or(st)
                }
                Err(e) => {
                    let _ = writeln!(stderr, "krypton: -: {e}");
                    1
                }
            }
        }
        Some("--") => match rest.get(1) {
            Some(file) => run_file(
                &mut shell,
                file,
                rest.get(2..).unwrap_or(&[]),
                &mut stdin,
                &mut stdout,
                &mut stderr,
            ),
            None => interactive::run(&mut shell).unwrap_or_else(|| run_interactive(&mut shell)),
        },
        Some(opt) if opt.starts_with('-') => {
            let _ = writeln!(stderr, "krypton: unknown option: {opt}");
            2
        }
        Some(file) => run_file(
            &mut shell,
            file,
            rest.get(1..).unwrap_or(&[]),
            &mut stdin,
            &mut stdout,
            &mut stderr,
        ),
        None => interactive::run(&mut shell).unwrap_or_else(|| run_interactive(&mut shell)),
    };

    ExitCode::from((code & 0xff) as u8)
}
