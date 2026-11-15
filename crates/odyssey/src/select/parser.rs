//! CSS Selectors Level 4 subset parser.

use crate::css::{tokenize, Token};

/// Longest selector list accepted, in bytes; longer input is a parse error.
const MAX_SELECTOR_BYTES: usize = 1024;
/// Most compound selectors allowed in one complex selector (bounds matching work and recursion).
const MAX_COMPOUNDS: usize = 32;

/// Error returned when parsing an invalid selector string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypeSel {
    Tag(String),
    Universal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttrOp {
    Eq,
    Includes,
    DashMatch,
    Prefix,
    Suffix,
    Substring,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Simple {
    Id(String),
    Class(String),
    AttrPresence(String),
    AttrMatch {
        name: String,
        op: AttrOp,
        value: String,
    },
    Root,
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    Empty,
    NthChild(i32, i32),
    NthLastChild(i32, i32),
    NthOfType(i32, i32),
    NthLastOfType(i32, i32),
    Not(Box<Compound>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Compound {
    pub(crate) type_sel: Option<TypeSel>,
    pub(crate) simples: Vec<Simple>,
}

/// A parsed complex CSS selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    pub(crate) first: Compound,
    pub(crate) rest: Vec<(Combinator, Compound)>,
}

impl Selector {
    /// Return the Selectors Level 4 specificity `(id, class_attr_pseudo, type)`.
    pub fn specificity(&self) -> (u32, u32, u32) {
        let mut spec = compound_specificity(&self.first);
        for (_, comp) in &self.rest {
            let s = compound_specificity(comp);
            spec.0 += s.0;
            spec.1 += s.1;
            spec.2 += s.2;
        }
        spec
    }
}

fn compound_specificity(c: &Compound) -> (u32, u32, u32) {
    let mut spec = (0u32, 0u32, 0u32);
    if matches!(c.type_sel, Some(TypeSel::Tag(_))) {
        spec.2 += 1;
    }
    for s in &c.simples {
        match s {
            Simple::Id(_) => spec.0 += 1,
            Simple::Not(inner) => {
                let ispec = compound_specificity(inner);
                spec.0 += ispec.0;
                spec.1 += ispec.1;
                spec.2 += ispec.2;
            }
            _ => spec.1 += 1,
        }
    }
    spec
}

/// Parse a comma-separated list of CSS selectors.
pub fn parse_selectors(input: &str) -> Result<Vec<Selector>, SelectorError> {
    if input.len() > MAX_SELECTOR_BYTES {
        return Err(SelectorError);
    }
    let tokens = tokenize(input);
    let mut pos = 0usize;
    let mut selectors = Vec::new();

    loop {
        skip_ws(&tokens, &mut pos);
        if pos >= tokens.len() {
            return Err(SelectorError);
        }
        selectors.push(parse_complex(&tokens, &mut pos)?);
        skip_ws(&tokens, &mut pos);
        if pos >= tokens.len() {
            break;
        }
        if matches!(tokens.get(pos), Some(Token::Comma)) {
            pos += 1;
        } else {
            return Err(SelectorError);
        }
    }

    Ok(selectors)
}

fn skip_ws(tokens: &[Token], pos: &mut usize) {
    while matches!(tokens.get(*pos), Some(Token::Whitespace)) {
        *pos += 1;
    }
}

fn parse_complex(tokens: &[Token], pos: &mut usize) -> Result<Selector, SelectorError> {
    let first = parse_compound(tokens, pos, false)?;
    let mut rest = Vec::new();

    loop {
        let had_ws = matches!(tokens.get(*pos), Some(Token::Whitespace));
        skip_ws(tokens, pos);
        if *pos >= tokens.len() || matches!(tokens.get(*pos), Some(Token::Comma)) {
            break;
        }
        let comb = match tokens.get(*pos) {
            Some(Token::Delim('>')) => {
                *pos += 1;
                skip_ws(tokens, pos);
                Combinator::Child
            }
            Some(Token::Delim('+')) => {
                *pos += 1;
                skip_ws(tokens, pos);
                Combinator::NextSibling
            }
            Some(Token::Delim('~')) => {
                *pos += 1;
                skip_ws(tokens, pos);
                Combinator::SubsequentSibling
            }
            _ if had_ws => Combinator::Descendant,
            _ => return Err(SelectorError),
        };
        if rest.len() + 2 > MAX_COMPOUNDS {
            return Err(SelectorError);
        }
        let next_comp = parse_compound(tokens, pos, false)?;
        rest.push((comb, next_comp));
    }

    Ok(Selector { first, rest })
}

fn parse_compound(
    tokens: &[Token],
    pos: &mut usize,
    in_not: bool,
) -> Result<Compound, SelectorError> {
    let mut type_sel = None;
    let mut simples = Vec::new();

    match tokens.get(*pos) {
        Some(Token::Ident(name)) => {
            type_sel = Some(TypeSel::Tag(name.to_ascii_lowercase()));
            *pos += 1;
        }
        Some(Token::Delim('*')) => {
            type_sel = Some(TypeSel::Universal);
            *pos += 1;
        }
        _ => {}
    }

    while *pos < tokens.len() {
        match tokens.get(*pos) {
            Some(Token::Hash { value, id: true }) => {
                simples.push(Simple::Id(value.clone()));
                *pos += 1;
            }
            Some(Token::Hash { id: false, .. }) => return Err(SelectorError),
            Some(Token::Delim('.')) => {
                *pos += 1;
                let Some(Token::Ident(cls)) = tokens.get(*pos) else {
                    return Err(SelectorError);
                };
                simples.push(Simple::Class(cls.clone()));
                *pos += 1;
            }
            Some(Token::OpenSquare) => {
                simples.push(parse_attr(tokens, pos)?);
            }
            Some(Token::Colon) => {
                simples.push(parse_pseudo(tokens, pos, in_not)?);
            }
            Some(Token::Whitespace | Token::Comma | Token::CloseParen) => break,
            Some(Token::Delim('>' | '+' | '~')) => break,
            _ => return Err(SelectorError),
        }
    }

    if type_sel.is_none() && simples.is_empty() {
        return Err(SelectorError);
    }

    Ok(Compound { type_sel, simples })
}

fn parse_attr(tokens: &[Token], pos: &mut usize) -> Result<Simple, SelectorError> {
    *pos += 1; // skip '['
    skip_ws(tokens, pos);
    let Some(Token::Ident(name)) = tokens.get(*pos) else {
        return Err(SelectorError);
    };
    let attr_name = name.to_ascii_lowercase();
    *pos += 1;
    skip_ws(tokens, pos);

    if matches!(tokens.get(*pos), Some(Token::CloseSquare)) {
        *pos += 1;
        return Ok(Simple::AttrPresence(attr_name));
    }

    let op = match tokens.get(*pos) {
        Some(Token::Delim('=')) => {
            *pos += 1;
            AttrOp::Eq
        }
        Some(Token::Delim(c @ ('~' | '|' | '^' | '$' | '*'))) => {
            let c = *c;
            *pos += 1;
            if !matches!(tokens.get(*pos), Some(Token::Delim('='))) {
                return Err(SelectorError);
            }
            *pos += 1;
            match c {
                '~' => AttrOp::Includes,
                '|' => AttrOp::DashMatch,
                '^' => AttrOp::Prefix,
                '$' => AttrOp::Suffix,
                '*' => AttrOp::Substring,
                _ => return Err(SelectorError),
            }
        }
        _ => return Err(SelectorError),
    };

    skip_ws(tokens, pos);
    let value = match tokens.get(*pos) {
        Some(Token::Ident(v) | Token::Str(v)) => {
            *pos += 1;
            v.clone()
        }
        _ => return Err(SelectorError),
    };
    skip_ws(tokens, pos);
    if !matches!(tokens.get(*pos), Some(Token::CloseSquare)) {
        return Err(SelectorError);
    }
    *pos += 1;
    Ok(Simple::AttrMatch {
        name: attr_name,
        op,
        value,
    })
}

fn parse_pseudo(tokens: &[Token], pos: &mut usize, in_not: bool) -> Result<Simple, SelectorError> {
    *pos += 1; // skip ':'
    match tokens.get(*pos) {
        Some(Token::Ident(name)) => {
            *pos += 1;
            match name.to_ascii_lowercase().as_str() {
                "root" => Ok(Simple::Root),
                "first-child" => Ok(Simple::FirstChild),
                "last-child" => Ok(Simple::LastChild),
                "only-child" => Ok(Simple::OnlyChild),
                "first-of-type" => Ok(Simple::FirstOfType),
                "last-of-type" => Ok(Simple::LastOfType),
                "empty" => Ok(Simple::Empty),
                _ => Err(SelectorError),
            }
        }
        Some(Token::Function(name)) => {
            let lower = name.to_ascii_lowercase();
            *pos += 1;
            if lower == "not" {
                if in_not {
                    return Err(SelectorError);
                }
                skip_ws(tokens, pos);
                let inner = parse_compound(tokens, pos, true)?;
                skip_ws(tokens, pos);
                if !matches!(tokens.get(*pos), Some(Token::CloseParen)) {
                    return Err(SelectorError);
                }
                *pos += 1;
                return Ok(Simple::Not(Box::new(inner)));
            }
            let start = *pos;
            let mut depth = 1usize;
            while *pos < tokens.len() && depth > 0 {
                match tokens[*pos] {
                    Token::Function(_) | Token::OpenParen => depth += 1,
                    Token::CloseParen => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                *pos += 1;
            }
            if depth != 0 {
                return Err(SelectorError);
            }
            let args = &tokens[start..*pos];
            *pos += 1; // skip ')'
            let (a, b) = parse_an_plus_b(args)?;
            match lower.as_str() {
                "nth-child" => Ok(Simple::NthChild(a, b)),
                "nth-last-child" => Ok(Simple::NthLastChild(a, b)),
                "nth-of-type" => Ok(Simple::NthOfType(a, b)),
                "nth-last-of-type" => Ok(Simple::NthLastOfType(a, b)),
                _ => Err(SelectorError),
            }
        }
        _ => Err(SelectorError),
    }
}

fn parse_an_plus_b(tokens: &[Token]) -> Result<(i32, i32), SelectorError> {
    let mut s = String::new();
    let mut prev_ws = false;
    let mut prev_was_sign_or_num = false;
    for tok in tokens {
        match tok {
            Token::Whitespace => {
                prev_ws = true;
                if !s.ends_with(' ') && !s.is_empty() {
                    s.push(' ');
                }
            }
            Token::Ident(id) => {
                if prev_ws && prev_was_sign_or_num && id.to_ascii_lowercase().starts_with('n') {
                    return Err(SelectorError);
                }
                prev_ws = false;
                prev_was_sign_or_num = false;
                s.push_str(id);
            }
            Token::Number { value, int: true } => {
                if *value < i32::MIN as f64 || *value > i32::MAX as f64 {
                    return Err(SelectorError);
                }
                prev_ws = false;
                prev_was_sign_or_num = true;
                let n = *value as i32;
                if n >= 0 && s.trim_end().ends_with(['n', 'N']) {
                    s.push('+');
                }
                s.push_str(&n.to_string());
            }
            Token::Dimension {
                value,
                int: true,
                unit,
            } => {
                if *value < i32::MIN as f64 || *value > i32::MAX as f64 {
                    return Err(SelectorError);
                }
                prev_ws = false;
                prev_was_sign_or_num = false;
                s.push_str(&(*value as i32).to_string());
                s.push_str(unit);
            }
            Token::Delim(c @ ('+' | '-')) => {
                prev_ws = false;
                prev_was_sign_or_num = true;
                s.push(*c);
            }
            _ => return Err(SelectorError),
        }
    }
    parse_an_plus_b_str(s.trim())
}

fn parse_an_plus_b_str(raw: &str) -> Result<(i32, i32), SelectorError> {
    let lower = raw.to_ascii_lowercase();
    if lower == "odd" {
        return Ok((2, 1));
    }
    if lower == "even" {
        return Ok((2, 0));
    }
    let compact: String = lower.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if compact.is_empty() {
        return Err(SelectorError);
    }
    if let Some(n_pos) = compact.find('n') {
        let a_str = &compact[..n_pos];
        let a = match a_str {
            "" | "+" => 1,
            "-" => -1,
            _ => a_str.parse::<i32>().map_err(|_| SelectorError)?,
        };
        let b_str = &compact[n_pos + 1..];
        let b = if b_str.is_empty() {
            0
        } else {
            if !b_str.starts_with('+') && !b_str.starts_with('-') {
                return Err(SelectorError);
            }
            let rest = &b_str[1..];
            if rest.is_empty() || !rest.bytes().all(|c| c.is_ascii_digit()) {
                return Err(SelectorError);
            }
            b_str.parse::<i32>().map_err(|_| SelectorError)?
        };
        Ok((a, b))
    } else {
        let b = compact.parse::<i32>().map_err(|_| SelectorError)?;
        Ok((0, b))
    }
}
