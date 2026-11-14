//! Python-compatible recursive-descent regex parser.

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::class::{AnchorKind, Ast, CharClass};
use super::RegexError;

pub(crate) struct ParsedPattern {
    pub(crate) ast: Ast,
    pub(crate) num_captures: usize,
}

pub(crate) fn parse_pattern(pattern: &str) -> Result<ParsedPattern, RegexError> {
    let mut parser = Parser {
        src: pattern,
        pos: 0,
        num_captures: 0,
    };
    let ast = parser.parse_alt(false)?;
    if parser.pos != parser.src.len() {
        return Err(RegexError {
            pos: parser.pos,
            msg: "unbalanced parenthesis",
        });
    }
    Ok(ParsedPattern {
        ast,
        num_captures: parser.num_captures,
    })
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    num_captures: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.src.get(self.pos..)?.chars().next()
    }

    fn next_char(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn parse_alt(&mut self, in_group: bool) -> Result<Ast, RegexError> {
        let mut branches = Vec::new();
        branches.push(self.parse_concat(in_group)?);
        while self.peek() == Some('|') {
            self.next_char();
            branches.push(self.parse_concat(in_group)?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap_or(Ast::Empty))
        } else {
            Ok(Ast::Alt(branches))
        }
    }

    fn parse_concat(&mut self, in_group: bool) -> Result<Ast, RegexError> {
        let mut items = Vec::new();
        while let Some(ch) = self.peek() {
            if ch == '|' {
                break;
            }
            if ch == ')' {
                if in_group {
                    break;
                }
                return Err(RegexError {
                    pos: self.pos,
                    msg: "unbalanced parenthesis",
                });
            }
            let item_pos = self.pos;
            let atom = match ch {
                '*' | '+' | '?' => {
                    return Err(RegexError {
                        pos: item_pos,
                        msg: "nothing to repeat",
                    });
                }
                '{' => {
                    if self.try_parse_brace_quantifier()?.is_some() {
                        return Err(RegexError {
                            pos: item_pos,
                            msg: "nothing to repeat",
                        });
                    }
                    self.next_char();
                    Ast::Literal('{')
                }
                '(' => self.parse_group()?,
                '[' => self.parse_class()?,
                '.' => {
                    self.next_char();
                    Ast::Class(CharClass::dot())
                }
                '^' => {
                    self.next_char();
                    Ast::Anchor(AnchorKind::Start)
                }
                '$' => {
                    self.next_char();
                    Ast::Anchor(AnchorKind::End)
                }
                '\\' => self.parse_escape_outside()?,
                _ => {
                    self.next_char();
                    Ast::Literal(ch)
                }
            };

            let node = self.parse_optional_quantifier(atom)?;
            items.push(node);
        }

        match items.len() {
            0 => Ok(Ast::Empty),
            1 => Ok(items.pop().unwrap_or(Ast::Empty)),
            _ => Ok(Ast::Concat(items)),
        }
    }

    fn parse_optional_quantifier(&mut self, atom: Ast) -> Result<Ast, RegexError> {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.next_char();
                (0u16, None)
            }
            Some('+') => {
                self.next_char();
                (1u16, None)
            }
            Some('?') => {
                self.next_char();
                (0u16, Some(1u16))
            }
            Some('{') => match self.try_parse_brace_quantifier()? {
                Some((min, max)) => (min, max),
                None => return Ok(atom),
            },
            _ => return Ok(atom),
        };
        let greedy = if self.peek() == Some('?') {
            self.next_char();
            false
        } else {
            true
        };
        Ok(Ast::Repeat {
            body: Box::new(atom),
            min,
            max,
            greedy,
        })
    }

    /// If `self.pos` starts a `{n}`, `{n,}`, `{,m}`, `{n,m}` quantifier, consumes it and returns
    /// `Ok(Some((min, max)))`. If it is literal `{` (like `{foo}` or `{,}` or unclosed), leaves
    /// `self.pos` unchanged and returns `Ok(None)`. Returns `Err` if count > 1000 or min > max.
    fn try_parse_brace_quantifier(&mut self) -> Result<Option<(u16, Option<u16>)>, RegexError> {
        let start = self.pos;
        let bytes = self.src.as_bytes();
        let mut idx = start + 1;

        let min_start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        let has_min = idx > min_start;
        let min_slice = &self.src[min_start..idx];

        let (has_comma, has_max, max_slice) = if idx < bytes.len() && bytes[idx] == b',' {
            idx += 1;
            let max_start = idx;
            while idx < bytes.len() && bytes[idx].is_ascii_digit() {
                idx += 1;
            }
            (true, idx > max_start, &self.src[max_start..idx])
        } else {
            (false, false, "")
        };

        if idx >= bytes.len() || bytes[idx] != b'}' || (!has_min && !has_max) {
            return Ok(None);
        }
        idx += 1;

        let min = if has_min {
            parse_repeat_count(min_slice, start)?
        } else {
            0
        };
        let max = if !has_comma {
            Some(min)
        } else if has_max {
            let m = parse_repeat_count(max_slice, start)?;
            if min > m {
                return Err(RegexError {
                    pos: start,
                    msg: "min repeat greater than max repeat",
                });
            }
            Some(m)
        } else {
            None
        };

        self.pos = idx;
        Ok(Some((min, max)))
    }

    fn parse_group(&mut self) -> Result<Ast, RegexError> {
        let open_pos = self.pos;
        self.next_char(); // '('
        let capture = if self
            .src
            .get(self.pos..)
            .is_some_and(|s| s.starts_with("?:"))
        {
            self.pos += 2;
            None
        } else if self.peek() == Some('?') {
            return Err(RegexError {
                pos: self.pos,
                msg: "unsupported group syntax",
            });
        } else {
            self.num_captures += 1;
            Some(self.num_captures)
        };

        let body = self.parse_alt(true)?;
        if self.peek() != Some(')') {
            return Err(RegexError {
                pos: open_pos,
                msg: "unbalanced parenthesis",
            });
        }
        self.next_char(); // ')'
        Ok(Ast::Group {
            capture,
            body: Box::new(body),
        })
    }

    fn parse_escape_outside(&mut self) -> Result<Ast, RegexError> {
        let esc_pos = self.pos;
        self.next_char(); // '\'
        let Some(ch) = self.next_char() else {
            return Err(RegexError {
                pos: esc_pos,
                msg: "bad escape (end of pattern)",
            });
        };
        match ch {
            'n' => Ok(Ast::Literal('\n')),
            't' => Ok(Ast::Literal('\t')),
            'r' => Ok(Ast::Literal('\r')),
            'd' => Ok(Ast::Class(CharClass::digit())),
            'D' => Ok(Ast::Class(CharClass::non_digit())),
            'w' => Ok(Ast::Class(CharClass::word())),
            'W' => Ok(Ast::Class(CharClass::non_word())),
            's' => Ok(Ast::Class(CharClass::space())),
            'S' => Ok(Ast::Class(CharClass::non_space())),
            'b' => Ok(Ast::Anchor(AnchorKind::WordBoundary)),
            'B' => Ok(Ast::Anchor(AnchorKind::NonWordBoundary)),
            c if c.is_ascii_alphanumeric() => Err(RegexError {
                pos: esc_pos,
                msg: "bad escape",
            }),
            c => Ok(Ast::Literal(c)),
        }
    }

    fn parse_class(&mut self) -> Result<Ast, RegexError> {
        let open_pos = self.pos;
        self.next_char(); // '['
        let negated = if self.peek() == Some('^') {
            self.next_char();
            true
        } else {
            false
        };

        let mut ranges: Vec<(u32, u32)> = Vec::new();
        let mut is_first = true;

        loop {
            let Some(ch) = self.peek() else {
                return Err(RegexError {
                    pos: open_pos,
                    msg: "unterminated character set",
                });
            };
            if ch == ']' && !is_first {
                self.next_char();
                break;
            }
            is_first = false;
            let item_pos = self.pos;
            let first = self.parse_class_item()?;
            if self.peek() == Some('-') && !self.next_after_dash_is_close() {
                if self.pos + 1 >= self.src.len() {
                    return Err(RegexError {
                        pos: open_pos,
                        msg: "unterminated character set",
                    });
                }
                self.next_char(); // '-'
                let second = self.parse_class_item()?;
                match (first, second) {
                    (ClassItem::Char(lo), ClassItem::Char(hi)) if lo <= hi => {
                        ranges.push((lo as u32, hi as u32));
                    }
                    _ => {
                        return Err(RegexError {
                            pos: item_pos,
                            msg: "bad character range",
                        });
                    }
                }
            } else {
                match first {
                    ClassItem::Char(c) => ranges.push((c as u32, c as u32)),
                    ClassItem::Class(cc) => ranges.extend(cc.ranges),
                }
            }
        }

        Ok(Ast::Class(CharClass::from_ranges(negated, ranges)))
    }

    fn next_after_dash_is_close(&self) -> bool {
        self.src
            .get(self.pos + 1..)
            .is_some_and(|s| s.starts_with(']'))
    }

    fn parse_class_item(&mut self) -> Result<ClassItem, RegexError> {
        let pos = self.pos;
        let Some(ch) = self.next_char() else {
            return Err(RegexError {
                pos,
                msg: "unterminated character set",
            });
        };
        if ch != '\\' {
            return Ok(ClassItem::Char(ch));
        }
        let Some(esc) = self.next_char() else {
            return Err(RegexError {
                pos,
                msg: "bad escape (end of pattern)",
            });
        };
        match esc {
            'n' => Ok(ClassItem::Char('\n')),
            't' => Ok(ClassItem::Char('\t')),
            'r' => Ok(ClassItem::Char('\r')),
            'b' => Ok(ClassItem::Char('\x08')),
            'd' => Ok(ClassItem::Class(CharClass::digit())),
            'D' => Ok(ClassItem::Class(CharClass::non_digit())),
            'w' => Ok(ClassItem::Class(CharClass::word())),
            'W' => Ok(ClassItem::Class(CharClass::non_word())),
            's' => Ok(ClassItem::Class(CharClass::space())),
            'S' => Ok(ClassItem::Class(CharClass::non_space())),
            c if c.is_ascii_alphanumeric() => Err(RegexError {
                pos,
                msg: "bad escape",
            }),
            c => Ok(ClassItem::Char(c)),
        }
    }
}

enum ClassItem {
    Char(char),
    Class(CharClass),
}

fn parse_repeat_count(s: &str, err_pos: usize) -> Result<u16, RegexError> {
    let mut val = 0u32;
    for b in s.bytes() {
        val = val.saturating_mul(10).saturating_add(u32::from(b - b'0'));
        if val > 1000 {
            return Err(RegexError {
                pos: err_pos,
                msg: "repetition count above 1000",
            });
        }
    }
    Ok(val as u16)
}
