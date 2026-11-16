//! The interactive shell: line editing with history and completion, the startup file, and the
//! prompt. Used only when stdin and stdout are a terminal; piped input keeps the plain loop.

pub(crate) mod complete;
pub(crate) mod edit;
pub(crate) mod history;
pub(crate) mod prompt;

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use krypton::exec::Shell;

use crate::term::{self, Input, RawMode};
use complete::CompleteEnv;
use edit::{Editor, Event};
use history::History;

/// The home folder from `HOME`, or `USERPROFILE` on Windows.
pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Run the interactive loop. Returns `None` when the terminal is not usable, so the caller
/// falls back to the plain line reader.
pub(crate) fn run(shell: &mut Shell) -> Option<i32> {
    if !term::is_tty() || !io::IsTerminal::is_terminal(&io::stdout()) {
        return None;
    }
    term::ignore_interrupts();
    term::restore_on_panic();
    shell.set_inherit_stdio(true);
    let home = home_dir();
    let history_file = home.as_ref().map(|h| h.join(".krypton_history"));
    let mut history = history_file
        .as_deref()
        .map(History::load)
        .unwrap_or_default();
    run_startup_file(shell, home.as_deref());

    let mut input = Input::default();
    let mut pending = String::new();
    loop {
        if let Some(code) = shell.exited() {
            return Some(code);
        }
        let prompt_text = if pending.is_empty() {
            prompt::prompt(shell.cwd(), home.as_deref())
        } else {
            String::from("> ")
        };
        let event = match read_line(&prompt_text, &mut input, &history, shell, home.as_deref()) {
            Ok(event) => event,
            Err(_) => break,
        };
        let line = match event {
            Event::Submit(line) => line,
            Event::Cancel => {
                pending.clear();
                continue;
            }
            Event::Eof => break,
            Event::Redraw | Event::Clear | Event::List(_) => continue,
        };
        pending.push_str(&line);
        pending.push('\n');
        match krypton::parse(&pending, krypton::Escapes::host()) {
            Err(e) if e.incomplete => continue,
            Err(e) => {
                eprintln!("krypton: {}:{}: {}", e.line, e.col, e.msg);
                pending.clear();
                continue;
            }
            Ok(_) => {}
        }
        let command = pending.trim_end_matches('\n').to_string();
        history.push(&command);
        if let Some(file) = &history_file {
            let _ = history.save(file);
        }
        let mut stdin = io::stdin();
        let mut out = io::stdout();
        let mut err = io::stderr();
        shell.run(&command, &mut stdin, &mut out, &mut err);
        let _ = out.flush();
        pending.clear();
    }
    Some(shell.exited().unwrap_or_else(|| shell.status()))
}

/// Run `~/.kryptonrc` (if present) before the first prompt.
fn run_startup_file(shell: &mut Shell, home: Option<&Path>) {
    let Some(path) = home.map(|h| h.join(".kryptonrc")) else {
        return;
    };
    let Ok(source) = std::fs::read_to_string(&path) else {
        return;
    };
    let mut stdin = io::stdin();
    let mut out = io::stdout();
    let mut err = io::stderr();
    shell.run(&source, &mut stdin, &mut out, &mut err);
    let _ = out.flush();
}

/// Read one edited line. Raw mode is on only while this function runs.
fn read_line(
    prompt: &str,
    input: &mut Input,
    history: &History,
    shell: &Shell,
    home: Option<&Path>,
) -> io::Result<Event> {
    let _raw = RawMode::enable()?;
    let env = CompleteEnv {
        cwd: shell.cwd().to_path_buf(),
        home: home.map(Path::to_path_buf),
        path: shell.var("PATH").map(str::to_string),
        pathext: shell.var("PATHEXT").map(str::to_string),
        windows: cfg!(windows),
    };
    let mut editor = Editor::new();
    term::write_text(&editor.render(prompt))?;
    loop {
        let key = input.next_key();
        let event = editor.handle(key, history, &env);
        match event {
            Event::Redraw => term::write_text(&editor.render(prompt))?,
            Event::Clear => term::write_text(&format!("\x1b[2J\x1b[H{}", editor.render(prompt)))?,
            Event::List(names) => {
                let listing = format!("\r\n{}\r\n{}", names.join("  "), editor.render(prompt));
                term::write_text(&listing)?;
            }
            Event::Submit(line) => {
                term::write_text(&editor.render_final(prompt, ""))?;
                return Ok(Event::Submit(line));
            }
            Event::Cancel => {
                term::write_text(&editor.render_final(prompt, "^C"))?;
                return Ok(Event::Cancel);
            }
            Event::Eof => return Ok(Event::Eof),
        }
    }
}
