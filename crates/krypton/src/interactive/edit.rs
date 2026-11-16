//! The line editor as a pure state machine: key presses go in, an [`Event`] comes out, and
//! [`Editor::render`] produces the screen update. No console is touched here.

use std::string::String;
use std::vec::Vec;

use super::complete::{common_prefix, complete_at, CompleteEnv};
use super::history::History;
use crate::term::keys::Key;

/// What the caller must do after a key press.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// Redraw the line; nothing else to do.
    Redraw,
    /// The line was accepted; run it.
    Submit(String),
    /// Ctrl-C: drop the line and show a new prompt.
    Cancel,
    /// Ctrl-D on an empty line, or input ended.
    Eof,
    /// Ctrl-L: clear the screen and redraw.
    Clear,
    /// Several completions and no longer common prefix: list them, then redraw.
    List(Vec<String>),
}

/// Reverse incremental search (Ctrl-R) state.
struct Search {
    query: Vec<char>,
    /// History index of the current match, if any.
    found: Option<usize>,
    /// Text of the current match, shown on the line.
    shown: String,
}

/// One line being edited.
#[derive(Default)]
pub(crate) struct Editor {
    buf: Vec<char>,
    pos: usize,
    /// History entry currently shown (None: the line the user is typing).
    hist: Option<usize>,
    /// The user's own line, kept while browsing history.
    own: Vec<char>,
    search: Option<Search>,
}

fn is_word_char(c: char) -> bool {
    !c.is_whitespace()
}

impl Editor {
    /// An empty line with the cursor at the start.
    pub(crate) fn new() -> Editor {
        Editor::default()
    }

    /// The line as typed.
    pub(crate) fn text(&self) -> String {
        self.buf.iter().collect()
    }

    fn set_text(&mut self, text: &str) {
        self.buf = text.chars().collect();
        self.pos = self.buf.len();
    }

    fn insert(&mut self, c: char) {
        self.buf.insert(self.pos, c);
        self.pos += 1;
    }

    fn delete_before(&mut self, count_from: usize) {
        self.buf.drain(count_from..self.pos);
        self.pos = count_from;
    }

    /// Apply one key press.
    pub(crate) fn handle(&mut self, key: Key, hist: &History, env: &CompleteEnv) -> Event {
        if self.search.is_some() {
            return self.handle_search(key, hist, env);
        }
        match key {
            Key::Char(c) => self.insert(c),
            Key::Enter => return Event::Submit(self.text()),
            Key::Ctrl('c') => return Event::Cancel,
            Key::Eof => return Event::Eof,
            Key::Ctrl('d') => {
                if self.buf.is_empty() {
                    return Event::Eof;
                }
                if self.pos < self.buf.len() {
                    self.buf.remove(self.pos);
                }
            }
            Key::Ctrl('a') | Key::Home => self.pos = 0,
            Key::Ctrl('e') | Key::End => self.pos = self.buf.len(),
            Key::Left | Key::Ctrl('b') => self.pos = self.pos.saturating_sub(1),
            Key::Right | Key::Ctrl('f') => self.pos = (self.pos + 1).min(self.buf.len()),
            Key::Backspace | Key::Ctrl('h') => {
                if self.pos > 0 {
                    self.delete_before(self.pos - 1);
                }
            }
            Key::Delete => {
                if self.pos < self.buf.len() {
                    self.buf.remove(self.pos);
                }
            }
            Key::Ctrl('k') => self.buf.truncate(self.pos),
            Key::Ctrl('u') => self.delete_before(0),
            Key::Ctrl('w') => {
                let mut start = self.pos;
                while start > 0 && !is_word_char(self.buf[start - 1]) {
                    start -= 1;
                }
                while start > 0 && is_word_char(self.buf[start - 1]) {
                    start -= 1;
                }
                self.delete_before(start);
            }
            Key::Up | Key::Ctrl('p') => self.history_prev(hist),
            Key::Down | Key::Ctrl('n') => self.history_next(hist),
            Key::Ctrl('r') => {
                self.search = Some(Search {
                    query: Vec::new(),
                    found: None,
                    shown: String::new(),
                });
            }
            Key::Ctrl('l') => return Event::Clear,
            Key::Tab => return self.complete(env),
            _ => {}
        }
        Event::Redraw
    }

    fn history_prev(&mut self, hist: &History) {
        let index = match self.hist {
            None if !hist.is_empty() => {
                self.own = self.buf.clone();
                hist.len() - 1
            }
            Some(i) if i > 0 => i - 1,
            _ => return,
        };
        self.hist = Some(index);
        self.set_text(hist.get(index).unwrap_or(""));
    }

    fn history_next(&mut self, hist: &History) {
        match self.hist {
            Some(i) if i + 1 < hist.len() => {
                self.hist = Some(i + 1);
                self.set_text(hist.get(i + 1).unwrap_or(""));
            }
            Some(_) => {
                self.hist = None;
                self.buf = self.own.clone();
                self.pos = self.buf.len();
            }
            None => {}
        }
    }

    fn complete(&mut self, env: &CompleteEnv) -> Event {
        let done = complete_at(&self.buf, self.pos, env);
        if done.candidates.is_empty() {
            return Event::Redraw;
        }
        let typed = self.pos.saturating_sub(done.word_start);
        let replacement = if let [only] = done.candidates.as_slice() {
            let mut text = only.clone();
            if !text.ends_with(['/', '\\']) {
                text.push(' ');
            }
            text
        } else {
            let common = common_prefix(&done.candidates);
            if common.chars().count() <= typed {
                return Event::List(done.candidates);
            }
            common
        };
        let new_chars: Vec<char> = replacement.chars().collect();
        let end = self.pos;
        self.buf
            .splice(done.word_start..end, new_chars.iter().copied());
        self.pos = done.word_start + new_chars.len();
        Event::Redraw
    }

    fn handle_search(&mut self, key: Key, hist: &History, env: &CompleteEnv) -> Event {
        let Some(search) = self.search.as_mut() else {
            return Event::Redraw;
        };
        match key {
            Key::Char(c) => {
                search.query.push(c);
                refind(search, hist, None);
            }
            Key::Backspace | Key::Ctrl('h') => {
                search.query.pop();
                refind(search, hist, None);
            }
            Key::Ctrl('r') => {
                let before = search.found.unwrap_or(hist.len());
                refind(search, hist, Some(before));
            }
            Key::Escape | Key::Ctrl('g') => self.search = None,
            Key::Enter => {
                let text = std::mem::take(&mut search.shown);
                self.search = None;
                return Event::Submit(text);
            }
            other => {
                // Any other key accepts the match into the line and then acts normally.
                let shown = std::mem::take(&mut search.shown);
                self.search = None;
                if !shown.is_empty() {
                    self.set_text(&shown);
                }
                return self.handle(other, hist, env);
            }
        }
        Event::Redraw
    }

    /// The screen update for the current state: `prompt` followed by the line, with the cursor
    /// placed at the editing position.
    pub(crate) fn render(&self, prompt: &str) -> String {
        if let Some(search) = &self.search {
            let query: String = search.query.iter().collect();
            let label = if search.found.is_some() || search.query.is_empty() {
                "reverse-i-search"
            } else {
                "failing reverse-i-search"
            };
            return format!("\r\x1b[K({label})`{query}': {}", search.shown);
        }
        let back = self.buf.len() - self.pos;
        let mut out = format!("\r\x1b[K{prompt}{}", self.text());
        if back > 0 {
            out.push_str(&format!("\x1b[{back}D"));
        }
        out
    }

    /// The final screen update when the line is accepted or cancelled: the whole line, then a
    /// newline, with the cursor moved past the text.
    pub(crate) fn render_final(&self, prompt: &str, suffix: &str) -> String {
        format!("\r\x1b[K{prompt}{}{suffix}\r\n", self.text())
    }
}

/// Find the newest entry matching the query before `before` (default: the newest entry).
fn refind(search: &mut Search, hist: &History, before: Option<usize>) {
    let query: String = search.query.iter().collect();
    let limit = before.unwrap_or(hist.len());
    search.found = hist.search_back(&query, limit);
    search.shown = search
        .found
        .and_then(|i| hist.get(i))
        .unwrap_or("")
        .to_string();
}

#[cfg(test)]
mod tests {
    use super::{Editor, Event};
    use crate::interactive::complete::CompleteEnv;
    use crate::interactive::history::History;
    use crate::term::keys::Key;
    use std::format;
    use std::path::PathBuf;
    use std::string::String;
    use std::vec;
    use std::vec::Vec;

    fn env(dir: PathBuf) -> CompleteEnv {
        CompleteEnv {
            cwd: dir,
            home: None,
            path: Some(String::new()),
            pathext: None,
            windows: cfg!(windows),
        }
    }

    fn history(lines: &[&str]) -> History {
        let mut h = History::default();
        for line in lines {
            h.push(line);
        }
        h
    }

    /// Feed keys and return the last event.
    fn feed(ed: &mut Editor, keys: &[Key], h: &History, e: &CompleteEnv) -> Event {
        let mut last = Event::Redraw;
        for key in keys {
            last = ed.handle(*key, h, e);
        }
        last
    }

    fn typed(s: &str) -> Vec<Key> {
        s.chars().map(Key::Char).collect()
    }

    #[test]
    fn cursor_movement_and_insertion() {
        let (h, e) = (History::default(), env(std::env::temp_dir()));
        let mut ed = Editor::new();
        let mut keys = typed("abc");
        assert_eq!(Editor::new().render_final("$ ", "^C"), "\r\x1b[K$ ^C\r\n");
        keys.extend([Key::Left, Key::Left, Key::Char('x')]);
        feed(&mut ed, &keys, &h, &e);
        assert_eq!(ed.text(), "axbc");
        feed(
            &mut ed,
            &[Key::Home, Key::Delete, Key::End, Key::Backspace],
            &h,
            &e,
        );
        assert_eq!(ed.text(), "xb");
        feed(&mut ed, &[Key::Ctrl('a'), Key::Char('>')], &h, &e);
        assert_eq!(ed.text(), ">xb");
    }

    #[test]
    fn kill_and_word_editing_keys() {
        let (h, e) = (History::default(), env(std::env::temp_dir()));
        let mut ed = Editor::new();
        feed(&mut ed, &typed("echo foo bar"), &h, &e);
        feed(&mut ed, &[Key::Ctrl('w')], &h, &e);
        assert_eq!(ed.text(), "echo foo ");
        feed(&mut ed, &[Key::Ctrl('a'), Key::Ctrl('k')], &h, &e);
        assert_eq!(ed.text(), "");
        feed(&mut ed, &typed("one two"), &h, &e);
        feed(&mut ed, &[Key::Ctrl('u')], &h, &e);
        assert_eq!(ed.text(), "");
        feed(&mut ed, &typed("keep"), &h, &e);
        assert_eq!(feed(&mut ed, &[Key::Ctrl('d')], &h, &e), Event::Redraw);
        assert_eq!(feed(&mut ed, &[Key::Ctrl('c')], &h, &e), Event::Cancel);
        feed(&mut ed, &[Key::Ctrl('u')], &h, &e);
        assert_eq!(feed(&mut ed, &[Key::Ctrl('d')], &h, &e), Event::Eof);
    }

    #[test]
    fn enter_submits_and_arrows_browse_history() {
        let h = history(&["one", "two"]);
        let e = env(std::env::temp_dir());
        let mut ed = Editor::new();
        feed(&mut ed, &typed("pa"), &h, &e);
        feed(&mut ed, &[Key::Up], &h, &e);
        assert_eq!(ed.text(), "two");
        feed(&mut ed, &[Key::Up, Key::Up], &h, &e);
        assert_eq!(ed.text(), "one");
        feed(&mut ed, &[Key::Down], &h, &e);
        assert_eq!(ed.text(), "two");
        feed(&mut ed, &[Key::Down], &h, &e);
        assert_eq!(ed.text(), "pa");
        assert_eq!(
            feed(&mut ed, &[Key::Enter], &h, &e),
            Event::Submit("pa".to_string())
        );
    }

    #[test]
    fn ctrl_r_reverse_search_cycles_and_accepts() {
        let h = history(&["git status", "cargo test", "git log"]);
        let e = env(std::env::temp_dir());
        let mut ed = Editor::new();
        feed(&mut ed, &[Key::Ctrl('r')], &h, &e);
        feed(&mut ed, &typed("git"), &h, &e);
        let shown = ed.render("$ ");
        assert!(
            shown.contains("(reverse-i-search)`git': git log"),
            "{shown}"
        );
        feed(&mut ed, &[Key::Ctrl('r')], &h, &e);
        let shown = ed.render("$ ");
        assert!(shown.contains("git status"), "{shown}");
        assert_eq!(
            feed(&mut ed, &[Key::Enter], &h, &e),
            Event::Submit("git status".to_string())
        );

        feed(&mut ed, &[Key::Ctrl('r'), Key::Char('z')], &h, &e);
        assert!(ed.render("$ ").contains("failing reverse-i-search"));
        feed(&mut ed, &[Key::Escape], &h, &e);
        assert!(!ed.render("$ ").contains("reverse-i-search"));
    }

    #[test]
    fn tab_completes_unique_names_and_lists_ambiguous_ones() {
        let dir = std::env::temp_dir().join(format!("krypton-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        for name in ["alpha.txt", "alphabet.txt", "zeta.txt"] {
            std::fs::write(dir.join(name), "x").expect("write");
        }
        let (h, e) = (History::default(), env(dir.clone()));
        let mut ed = Editor::new();
        feed(&mut ed, &typed("cat zet"), &h, &e);
        feed(&mut ed, &[Key::Tab], &h, &e);
        assert_eq!(ed.text(), "cat zeta.txt ");

        let mut ed = Editor::new();
        feed(&mut ed, &typed("cat al"), &h, &e);
        feed(&mut ed, &[Key::Tab], &h, &e);
        assert_eq!(ed.text(), "cat alpha");
        let event = feed(&mut ed, &[Key::Tab], &h, &e);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            event,
            Event::List(vec!["alpha.txt".to_string(), "alphabet.txt".to_string()])
        );
    }

    #[test]
    fn render_places_the_cursor_before_the_text_after_it() {
        let (h, e) = (History::default(), env(std::env::temp_dir()));
        let mut ed = Editor::new();
        feed(&mut ed, &typed("abcd"), &h, &e);
        feed(&mut ed, &[Key::Left, Key::Left], &h, &e);
        assert_eq!(ed.render("$ "), "\r\x1b[K$ abcd\x1b[2D");
    }
}
