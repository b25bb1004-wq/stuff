//! POSIX shell arithmetic evaluator (`$((...))`).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::types::{Env, ExpandError};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ATok {
    Num(i64),
    Ident(String),
    LParen,
    RParen,
    Question,
    Colon,
    Tilde,
    Bang,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Shl,
    Shr,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    BitAnd,
    BitXor,
    BitOr,
    LogAnd,
    LogOr,
    Assign(AssignOp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    BitAnd,
    BitXor,
    BitOr,
}

fn parse_int_literal(bytes: &[u8], start: usize) -> Result<(i64, usize), ExpandError> {
    let mut i = start;
    if bytes[i] == b'0' && i + 1 < bytes.len() && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
        i += 2;
        if i >= bytes.len() || !bytes[i].is_ascii_hexdigit() {
            return Err(ExpandError::Arith(String::from("invalid hex literal")));
        }
        let mut val: u64 = 0;
        while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
            let d = match bytes[i] {
                b'0'..=b'9' => (bytes[i] - b'0') as u64,
                b'a'..=b'f' => (bytes[i] - b'a' + 10) as u64,
                b'A'..=b'F' => (bytes[i] - b'A' + 10) as u64,
                _ => 0,
            };
            val = val.wrapping_mul(16).wrapping_add(d);
            i += 1;
        }
        if i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            return Err(ExpandError::Arith(String::from("invalid integer suffix")));
        }
        return Ok((val as i64, i));
    }

    if bytes[i] == b'0' {
        let mut val: u64 = 0;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            if bytes[i] > b'7' {
                return Err(ExpandError::Arith(String::from("invalid octal digit")));
            }
            val = val.wrapping_mul(8).wrapping_add((bytes[i] - b'0') as u64);
            i += 1;
        }
        if i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
            return Err(ExpandError::Arith(String::from("invalid integer suffix")));
        }
        return Ok((val as i64, i));
    }

    let mut val: u64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        val = val.wrapping_mul(10).wrapping_add((bytes[i] - b'0') as u64);
        i += 1;
    }
    if i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        return Err(ExpandError::Arith(String::from("invalid integer suffix")));
    }
    Ok((val as i64, i))
}

fn tokenize(expr: &str) -> Result<Vec<ATok>, ExpandError> {
    let bytes = expr.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b.is_ascii_digit() {
            let (n, next_i) = parse_int_literal(bytes, i)?;
            toks.push(ATok::Num(n));
            i = next_i;
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = String::from(&expr[start..i]);
            toks.push(ATok::Ident(ident));
            continue;
        }

        let rest = &bytes[i..];
        let (tok, len) = if rest.starts_with(b"<<=") {
            (ATok::Assign(AssignOp::Shl), 3)
        } else if rest.starts_with(b">>=") {
            (ATok::Assign(AssignOp::Shr), 3)
        } else if rest.starts_with(b"==") {
            (ATok::Eq, 2)
        } else if rest.starts_with(b"!=") {
            (ATok::Ne, 2)
        } else if rest.starts_with(b"<=") {
            (ATok::Le, 2)
        } else if rest.starts_with(b">=") {
            (ATok::Ge, 2)
        } else if rest.starts_with(b"<<") {
            (ATok::Shl, 2)
        } else if rest.starts_with(b">>") {
            (ATok::Shr, 2)
        } else if rest.starts_with(b"&&") {
            (ATok::LogAnd, 2)
        } else if rest.starts_with(b"||") {
            (ATok::LogOr, 2)
        } else if rest.starts_with(b"+=") {
            (ATok::Assign(AssignOp::Add), 2)
        } else if rest.starts_with(b"-=") {
            (ATok::Assign(AssignOp::Sub), 2)
        } else if rest.starts_with(b"*=") {
            (ATok::Assign(AssignOp::Mul), 2)
        } else if rest.starts_with(b"/=") {
            (ATok::Assign(AssignOp::Div), 2)
        } else if rest.starts_with(b"%=") {
            (ATok::Assign(AssignOp::Rem), 2)
        } else if rest.starts_with(b"&=") {
            (ATok::Assign(AssignOp::BitAnd), 2)
        } else if rest.starts_with(b"^=") {
            (ATok::Assign(AssignOp::BitXor), 2)
        } else if rest.starts_with(b"|=") {
            (ATok::Assign(AssignOp::BitOr), 2)
        } else {
            let t = match b {
                b'(' => ATok::LParen,
                b')' => ATok::RParen,
                b'?' => ATok::Question,
                b':' => ATok::Colon,
                b'~' => ATok::Tilde,
                b'!' => ATok::Bang,
                b'+' => ATok::Plus,
                b'-' => ATok::Minus,
                b'*' => ATok::Star,
                b'/' => ATok::Slash,
                b'%' => ATok::Percent,
                b'<' => ATok::Lt,
                b'>' => ATok::Gt,
                b'&' => ATok::BitAnd,
                b'^' => ATok::BitXor,
                b'|' => ATok::BitOr,
                b'=' => ATok::Assign(AssignOp::Set),
                _ => {
                    return Err(ExpandError::Arith(format!(
                        "unexpected character: {}",
                        b as char
                    )))
                }
            };
            (t, 1)
        };
        toks.push(tok);
        i += len;
    }

    Ok(toks)
}

fn parse_env_int(raw: &str) -> Result<i64, ExpandError> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(0);
    }
    let bytes = s.as_bytes();
    let (neg, idx) = if bytes[0] == b'-' {
        (true, 1)
    } else if bytes[0] == b'+' {
        (false, 1)
    } else {
        (false, 0)
    };
    if idx >= bytes.len() || !bytes[idx].is_ascii_digit() {
        return Err(ExpandError::Arith(format!("invalid integer value: {s}")));
    }
    let (val, end) = parse_int_literal(bytes, idx)?;
    if end != bytes.len() {
        return Err(ExpandError::Arith(format!("invalid integer value: {s}")));
    }
    Ok(if neg { val.wrapping_neg() } else { val })
}

struct ArithParser<'a> {
    toks: Vec<ATok>,
    pos: usize,
    env: &'a mut dyn Env,
    depth: usize,
}

impl<'a> ArithParser<'a> {
    fn peek(&self) -> Option<&ATok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<ATok> {
        let tok = self.toks.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn enter(&mut self) -> Result<(), ExpandError> {
        self.depth += 1;
        if self.depth > 256 {
            return Err(ExpandError::Arith(String::from(
                "expression nesting too deep",
            )));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn read_var(&self, name: &str) -> Result<i64, ExpandError> {
        match self.env.get(name) {
            None => Ok(0),
            Some(s) => parse_env_int(&s),
        }
    }

    fn expr(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        self.assign(eval)
    }

    fn assign(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        self.enter()?;
        let res = (|| {
            let (lhs_val, lvalue) = self.cond(eval)?;
            if let Some(ATok::Assign(op)) = self.peek().cloned() {
                let Some(name) = lvalue else {
                    return Err(ExpandError::Arith(String::from(
                        "assignment target must be a variable",
                    )));
                };
                self.next();
                let (rhs_val, _) = self.assign(eval)?;
                if !eval {
                    return Ok((0, None));
                }
                let new_val = match op {
                    AssignOp::Set => rhs_val,
                    AssignOp::Add => lhs_val.wrapping_add(rhs_val),
                    AssignOp::Sub => lhs_val.wrapping_sub(rhs_val),
                    AssignOp::Mul => lhs_val.wrapping_mul(rhs_val),
                    AssignOp::Div => {
                        if rhs_val == 0 {
                            return Err(ExpandError::Arith(String::from("division by zero")));
                        }
                        lhs_val.wrapping_div(rhs_val)
                    }
                    AssignOp::Rem => {
                        if rhs_val == 0 {
                            return Err(ExpandError::Arith(String::from("division by zero")));
                        }
                        lhs_val.wrapping_rem(rhs_val)
                    }
                    AssignOp::Shl => lhs_val.wrapping_shl((rhs_val & 63) as u32),
                    AssignOp::Shr => lhs_val.wrapping_shr((rhs_val & 63) as u32),
                    AssignOp::BitAnd => lhs_val & rhs_val,
                    AssignOp::BitXor => lhs_val ^ rhs_val,
                    AssignOp::BitOr => lhs_val | rhs_val,
                };
                self.env.set(&name, &new_val.to_string());
                Ok((new_val, None))
            } else {
                Ok((lhs_val, lvalue))
            }
        })();
        self.leave();
        res
    }

    fn cond(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (cond_val, lvalue) = self.log_or(eval)?;
        if matches!(self.peek(), Some(ATok::Question)) {
            self.next();
            let take_then = eval && cond_val != 0;
            let (then_val, _) = self.expr(take_then)?;
            if !matches!(self.next(), Some(ATok::Colon)) {
                return Err(ExpandError::Arith(String::from(
                    "expected ':' in ternary expression",
                )));
            }
            let take_else = eval && cond_val == 0;
            let (else_val, _) = self.cond(take_else)?;
            let out = if !eval {
                0
            } else if cond_val != 0 {
                then_val
            } else {
                else_val
            };
            Ok((out, None))
        } else {
            Ok((cond_val, lvalue))
        }
    }

    fn log_or(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (mut val, mut lval) = self.log_and(eval)?;
        while matches!(self.peek(), Some(ATok::LogOr)) {
            self.next();
            let eval_rhs = eval && val == 0;
            let (rhs, _) = self.log_and(eval_rhs)?;
            val = if eval && (val != 0 || rhs != 0) { 1 } else { 0 };
            lval = None;
        }
        Ok((val, lval))
    }

    fn log_and(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (mut val, mut lval) = self.bit_or(eval)?;
        while matches!(self.peek(), Some(ATok::LogAnd)) {
            self.next();
            let eval_rhs = eval && val != 0;
            let (rhs, _) = self.bit_or(eval_rhs)?;
            val = if eval && val != 0 && rhs != 0 { 1 } else { 0 };
            lval = None;
        }
        Ok((val, lval))
    }

    fn bit_or(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (mut val, mut lval) = self.bit_xor(eval)?;
        while matches!(self.peek(), Some(ATok::BitOr)) {
            self.next();
            let (rhs, _) = self.bit_xor(eval)?;
            val |= rhs;
            lval = None;
        }
        Ok((val, lval))
    }

    fn bit_xor(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (mut val, mut lval) = self.bit_and(eval)?;
        while matches!(self.peek(), Some(ATok::BitXor)) {
            self.next();
            let (rhs, _) = self.bit_and(eval)?;
            val ^= rhs;
            lval = None;
        }
        Ok((val, lval))
    }

    fn bit_and(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (mut val, mut lval) = self.equality(eval)?;
        while matches!(self.peek(), Some(ATok::BitAnd)) {
            self.next();
            let (rhs, _) = self.equality(eval)?;
            val &= rhs;
            lval = None;
        }
        Ok((val, lval))
    }

    fn equality(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (mut val, mut lval) = self.relational(eval)?;
        while let Some(tok) = self.peek().cloned() {
            let is_eq = match tok {
                ATok::Eq => true,
                ATok::Ne => false,
                _ => break,
            };
            self.next();
            let (rhs, _) = self.relational(eval)?;
            val = if (val == rhs) == is_eq { 1 } else { 0 };
            lval = None;
        }
        Ok((val, lval))
    }

    fn relational(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (mut val, mut lval) = self.shift(eval)?;
        while let Some(tok) = self.peek().cloned() {
            match tok {
                ATok::Lt | ATok::Le | ATok::Gt | ATok::Ge => {
                    self.next();
                    let (rhs, _) = self.shift(eval)?;
                    let ok = match tok {
                        ATok::Lt => val < rhs,
                        ATok::Le => val <= rhs,
                        ATok::Gt => val > rhs,
                        ATok::Ge => val >= rhs,
                        _ => false,
                    };
                    val = i64::from(ok);
                    lval = None;
                }
                _ => break,
            }
        }
        Ok((val, lval))
    }

    fn shift(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (mut val, mut lval) = self.additive(eval)?;
        while let Some(tok) = self.peek().cloned() {
            match tok {
                ATok::Shl => {
                    self.next();
                    let (rhs, _) = self.additive(eval)?;
                    val = val.wrapping_shl((rhs & 63) as u32);
                    lval = None;
                }
                ATok::Shr => {
                    self.next();
                    let (rhs, _) = self.additive(eval)?;
                    val = val.wrapping_shr((rhs & 63) as u32);
                    lval = None;
                }
                _ => break,
            }
        }
        Ok((val, lval))
    }

    fn additive(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (mut val, mut lval) = self.multiplicative(eval)?;
        while let Some(tok) = self.peek().cloned() {
            match tok {
                ATok::Plus => {
                    self.next();
                    let (rhs, _) = self.multiplicative(eval)?;
                    val = val.wrapping_add(rhs);
                    lval = None;
                }
                ATok::Minus => {
                    self.next();
                    let (rhs, _) = self.multiplicative(eval)?;
                    val = val.wrapping_sub(rhs);
                    lval = None;
                }
                _ => break,
            }
        }
        Ok((val, lval))
    }

    fn multiplicative(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        let (mut val, mut lval) = self.unary(eval)?;
        while let Some(tok) = self.peek().cloned() {
            match tok {
                ATok::Star => {
                    self.next();
                    let (rhs, _) = self.unary(eval)?;
                    val = val.wrapping_mul(rhs);
                    lval = None;
                }
                ATok::Slash => {
                    self.next();
                    let (rhs, _) = self.unary(eval)?;
                    if eval {
                        if rhs == 0 {
                            return Err(ExpandError::Arith(String::from("division by zero")));
                        }
                        val = val.wrapping_div(rhs);
                    }
                    lval = None;
                }
                ATok::Percent => {
                    self.next();
                    let (rhs, _) = self.unary(eval)?;
                    if eval {
                        if rhs == 0 {
                            return Err(ExpandError::Arith(String::from("division by zero")));
                        }
                        val = val.wrapping_rem(rhs);
                    }
                    lval = None;
                }
                _ => break,
            }
        }
        Ok((val, lval))
    }

    fn unary(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        self.enter()?;
        let res = match self.peek().cloned() {
            Some(ATok::Plus) => {
                self.next();
                let (v, _) = self.unary(eval)?;
                Ok((v, None))
            }
            Some(ATok::Minus) => {
                self.next();
                let (v, _) = self.unary(eval)?;
                Ok((v.wrapping_neg(), None))
            }
            Some(ATok::Bang) => {
                self.next();
                let (v, _) = self.unary(eval)?;
                Ok((if v == 0 { 1 } else { 0 }, None))
            }
            Some(ATok::Tilde) => {
                self.next();
                let (v, _) = self.unary(eval)?;
                Ok((!v, None))
            }
            _ => self.primary(eval),
        };
        self.leave();
        res
    }

    fn primary(&mut self, eval: bool) -> Result<(i64, Option<String>), ExpandError> {
        match self.next() {
            Some(ATok::Num(n)) => Ok((n, None)),
            Some(ATok::Ident(name)) => {
                let val = if eval { self.read_var(&name)? } else { 0 };
                Ok((val, Some(name)))
            }
            Some(ATok::LParen) => {
                let (val, _) = self.expr(eval)?;
                if !matches!(self.next(), Some(ATok::RParen)) {
                    return Err(ExpandError::Arith(String::from("missing ')'")));
                }
                Ok((val, None))
            }
            _ => Err(ExpandError::Arith(String::from("expected operand"))),
        }
    }
}

/// Evaluate a POSIX shell arithmetic expression (`i64`, wrapping overflow).
pub fn arith(expr: &str, env: &mut dyn Env) -> Result<i64, ExpandError> {
    let toks = tokenize(expr)?;
    if toks.is_empty() {
        return Ok(0);
    }
    let mut parser = ArithParser {
        toks,
        pos: 0,
        env,
        depth: 0,
    };
    let (val, _) = parser.expr(true)?;
    if parser.pos != parser.toks.len() {
        return Err(ExpandError::Arith(String::from(
            "unexpected trailing token",
        )));
    }
    Ok(val)
}
