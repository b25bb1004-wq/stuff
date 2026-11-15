//! CSS Syntax Level 3 component value, declaration, and stylesheet parsers.

use super::tokenizer::{tokenize, Token};

/// A component value in a CSS token stream.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A single CSS token.
    Token(Token),
    /// A `{ ... }`, `[ ... ]`, or `( ... )` block.
    Block {
        /// Opening delimiter (`'{'`, `'['`, or `'('`).
        open: char,
        /// Nested component values inside the block.
        items: Vec<Value>,
    },
    /// A `name( ... )` function call.
    Function {
        /// Function name without `(`.
        name: String,
        /// Nested component values inside the parentheses.
        args: Vec<Value>,
    },
}

/// A parsed CSS declaration (`name: value [!important]`).
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    /// Property name as written.
    pub name: String,
    /// Declaration value tokens/blocks, trimmed of leading and trailing whitespace.
    pub value: Vec<Value>,
    /// Whether `!important` was present at the end of the declaration.
    pub important: bool,
}

/// A top-level rule in a CSS stylesheet.
#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    /// A qualified style rule (`prelude { declarations }`).
    Style {
        /// Selector / prelude component values.
        prelude: Vec<Value>,
        /// Parsed declarations inside the `{}` block.
        declarations: Vec<Declaration>,
    },
    /// An at-rule (`@name prelude;` or `@name prelude { block }`).
    At {
        /// At-keyword name without `@`.
        name: String,
        /// Prelude component values.
        prelude: Vec<Value>,
        /// Optional `{}` block contents.
        block: Option<Vec<Value>>,
    },
}

enum Frame {
    Block { open: char, items: Vec<Value> },
    Function { name: String, args: Vec<Value> },
}

impl Frame {
    fn push(&mut self, val: Value) {
        match self {
            Frame::Block { items, .. } => items.push(val),
            Frame::Function { args, .. } => args.push(val),
        }
    }

    fn matches_close(&self, close: &Token) -> bool {
        matches!(
            (self, close),
            (Frame::Block { open: '(', .. }, Token::CloseParen)
                | (Frame::Function { .. }, Token::CloseParen)
                | (Frame::Block { open: '[', .. }, Token::CloseSquare)
                | (Frame::Block { open: '{', .. }, Token::CloseCurly)
        )
    }

    fn into_value(self) -> Value {
        match self {
            Frame::Block { open, items } => Value::Block { open, items },
            Frame::Function { name, args } => Value::Function { name, args },
        }
    }
}

/// Deepest block nesting kept as a tree by [`parse_values`] (tree walks at this depth fit a 2 MiB stack).
const MAX_NEST: usize = 2048;
/// Deepest nesting the stylesheet and declaration parsers clone; deeper clones overflow a 2 MiB stack.
const RULE_NEST: usize = 1024;

/// Parse a CSS string into a nested list of [`Value`]s using an explicit stack.
pub fn parse_values(input: &str) -> Vec<Value> {
    parse_values_capped(input, MAX_NEST)
}

// shortcut: blocks nested deeper than max_nest stay flat tokens, revisit if a tree-walking pass needs deeper input
fn parse_values_capped(input: &str, max_nest: usize) -> Vec<Value> {
    let tokens = tokenize(input);
    let mut root: Vec<Value> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut too_deep = 0usize; // openers past max_nest whose closers are still pending

    for tok in tokens {
        let opens = matches!(
            tok,
            Token::OpenParen | Token::OpenSquare | Token::OpenCurly | Token::Function(_)
        );
        let closes = matches!(
            tok,
            Token::CloseParen | Token::CloseSquare | Token::CloseCurly
        );
        if (opens && stack.len() >= max_nest) || (closes && too_deep > 0) {
            if opens {
                too_deep += 1;
            } else {
                too_deep -= 1;
            }
            let val = Value::Token(tok);
            match stack.last_mut() {
                Some(parent) => parent.push(val),
                None => root.push(val),
            }
            continue;
        }
        match tok {
            Token::OpenParen => stack.push(Frame::Block {
                open: '(',
                items: Vec::new(),
            }),
            Token::OpenSquare => stack.push(Frame::Block {
                open: '[',
                items: Vec::new(),
            }),
            Token::OpenCurly => stack.push(Frame::Block {
                open: '{',
                items: Vec::new(),
            }),
            Token::Function(name) => stack.push(Frame::Function {
                name,
                args: Vec::new(),
            }),
            Token::CloseParen | Token::CloseSquare | Token::CloseCurly => {
                if stack.last().is_some_and(|f| f.matches_close(&tok)) {
                    if let Some(frame) = stack.pop() {
                        let val = frame.into_value();
                        if let Some(parent) = stack.last_mut() {
                            parent.push(val);
                        } else {
                            root.push(val);
                        }
                    }
                } else {
                    let val = Value::Token(tok);
                    if let Some(parent) = stack.last_mut() {
                        parent.push(val);
                    } else {
                        root.push(val);
                    }
                }
            }
            other => {
                let val = Value::Token(other);
                if let Some(parent) = stack.last_mut() {
                    parent.push(val);
                } else {
                    root.push(val);
                }
            }
        }
    }

    while let Some(frame) = stack.pop() {
        let val = frame.into_value();
        if let Some(parent) = stack.last_mut() {
            parent.push(val);
        } else {
            root.push(val);
        }
    }

    root
}

fn is_ws_val(v: &Value) -> bool {
    matches!(v, Value::Token(Token::Whitespace))
}

fn trim_ws(slice: &[Value]) -> Vec<Value> {
    let start = slice
        .iter()
        .position(|v| !is_ws_val(v))
        .unwrap_or(slice.len());
    let end = slice
        .iter()
        .rposition(|v| !is_ws_val(v))
        .map_or(start, |i| i + 1);
    slice[start..end].to_vec()
}

fn parse_single_declaration(entry: &[Value]) -> Option<Declaration> {
    let mut i = 0;
    let name = match entry.get(i) {
        Some(Value::Token(Token::Ident(s))) => {
            i += 1;
            s.clone()
        }
        _ => return None,
    };
    while i < entry.len() && is_ws_val(&entry[i]) {
        i += 1;
    }
    if !matches!(entry.get(i), Some(Value::Token(Token::Colon))) {
        return None;
    }
    i += 1;
    let raw_val = &entry[i..];

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Value,
        Bang,
        Important,
    }

    let mut state = State::Value;
    let mut seen_value = false;
    let mut bang_idx = raw_val.len();

    for (idx, v) in raw_val.iter().enumerate() {
        if is_ws_val(v) {
            continue;
        }
        match state {
            State::Value => {
                if matches!(v, Value::Token(Token::Delim('!'))) {
                    state = State::Bang;
                    bang_idx = idx;
                } else {
                    if matches!(v, Value::Block { open: '{', .. }) && seen_value {
                        return None;
                    }
                    seen_value = true;
                }
            }
            State::Bang => {
                if let Value::Token(Token::Ident(s)) = v {
                    if s.eq_ignore_ascii_case("important") {
                        state = State::Important;
                        continue;
                    }
                }
                if matches!(v, Value::Block { open: '{', .. }) && seen_value {
                    return None;
                }
                state = State::Value;
                seen_value = true;
            }
            State::Important => {
                if matches!(v, Value::Block { open: '{', .. }) && seen_value {
                    return None;
                }
                state = State::Value;
                seen_value = true;
            }
        }
    }

    let (important, slice) = if state == State::Important {
        (true, &raw_val[..bang_idx])
    } else {
        (false, raw_val)
    };

    Some(Declaration {
        name,
        value: trim_ws(slice),
        important,
    })
}

fn parse_declarations_from_values(values: &[Value]) -> Vec<Declaration> {
    let mut out = Vec::new();
    let mut i = 0;

    while i < values.len() {
        while i < values.len()
            && matches!(
                values[i],
                Value::Token(Token::Whitespace | Token::Semicolon)
            )
        {
            i += 1;
        }
        if i >= values.len() {
            break;
        }
        if matches!(values[i], Value::Token(Token::AtKeyword(_))) {
            i += 1;
            while i < values.len() {
                match &values[i] {
                    Value::Token(Token::Semicolon) | Value::Block { open: '{', .. } => {
                        i += 1;
                        break;
                    }
                    _ => i += 1,
                }
            }
            continue;
        }
        let start = i;
        while i < values.len() && !matches!(values[i], Value::Token(Token::Semicolon)) {
            i += 1;
        }
        if let Some(decl) = parse_single_declaration(&values[start..i]) {
            out.push(decl);
        }
        if i < values.len() && matches!(values[i], Value::Token(Token::Semicolon)) {
            i += 1;
        }
    }

    out
}

/// Parse a list of CSS declarations (e.g. a `style` attribute).
pub fn parse_declarations(input: &str) -> Vec<Declaration> {
    let values = parse_values_capped(input, RULE_NEST);
    parse_declarations_from_values(&values)
}

/// Parse a CSS stylesheet into a list of [`Rule`]s.
pub fn parse_stylesheet(input: &str) -> Vec<Rule> {
    let values = parse_values_capped(input, RULE_NEST);
    let mut out = Vec::new();
    let mut i = 0;

    while i < values.len() {
        while i < values.len()
            && matches!(
                values[i],
                Value::Token(Token::Whitespace | Token::Cdo | Token::Cdc)
            )
        {
            i += 1;
        }
        if i >= values.len() {
            break;
        }
        if let Value::Token(Token::AtKeyword(name)) = &values[i] {
            let name = name.clone();
            i += 1;
            let mut prelude = Vec::new();
            let mut block = None;
            while i < values.len() {
                match &values[i] {
                    Value::Token(Token::Semicolon) => {
                        i += 1;
                        break;
                    }
                    Value::Block { open: '{', items } => {
                        block = Some(items.clone());
                        i += 1;
                        break;
                    }
                    other => {
                        prelude.push(other.clone());
                        i += 1;
                    }
                }
            }
            out.push(Rule::At {
                name,
                prelude,
                block,
            });
        } else {
            let mut prelude = Vec::new();
            let mut found_block = None;
            while i < values.len() {
                match &values[i] {
                    Value::Block { open: '{', items } => {
                        found_block = Some(items.clone());
                        i += 1;
                        break;
                    }
                    other => {
                        prelude.push(other.clone());
                        i += 1;
                    }
                }
            }
            if let Some(items) = found_block {
                let declarations = parse_declarations_from_values(&items);
                out.push(Rule::Style {
                    prelude,
                    declarations,
                });
            }
        }
    }

    out
}
