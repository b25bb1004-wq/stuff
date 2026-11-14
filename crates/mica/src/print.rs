//! Canonical source text for a syntax tree. Parenthesises only where precedence needs it.
use crate::ast::{
    Arm, BinOp, Block, Expr, FnBody, FnDef, Ident, Item, Module, Param, Pattern, Stmt, Type,
    TypeBody, UnOp,
};
use alloc::string::{String, ToString};
use core::fmt::Write;

/// Prints a module as canonical Mica source text, ending with a newline.
pub fn print(module: &Module) -> String {
    let mut o = Out {
        s: String::new(),
        indent: 0,
    };
    o.module(module);
    o.s
}

/// Precedence of the postfix level, the tightest operator position a call base may need.
const POSTFIX: u8 = 12;
/// Precedence of unary operators.
const UNARY: u8 = 11;
/// Precedence of primary expressions (never parenthesised).
const PRIMARY: u8 = 13;

struct Out {
    s: String,
    indent: usize,
}

fn bin_info(op: BinOp) -> (u8, &'static str) {
    match op {
        BinOp::Or => (1, "||"),
        BinOp::And => (2, "&&"),
        BinOp::Eq => (3, "=="),
        BinOp::Ne => (3, "!="),
        BinOp::Lt => (3, "<"),
        BinOp::Le => (3, "<="),
        BinOp::Gt => (3, ">"),
        BinOp::Ge => (3, ">="),
        BinOp::BitOr => (5, "|"),
        BinOp::BitXor => (6, "^"),
        BinOp::BitAnd => (7, "&"),
        BinOp::Shl => (8, "<<"),
        BinOp::Shr => (8, ">>"),
        BinOp::Add => (9, "+"),
        BinOp::Sub => (9, "-"),
        BinOp::AddWrap => (9, "+%"),
        BinOp::SubWrap => (9, "-%"),
        BinOp::Mul => (10, "*"),
        BinOp::Div => (10, "/"),
        BinOp::Rem => (10, "%"),
        BinOp::MulWrap => (10, "*%"),
    }
}

/// Precedence level of an expression, matching the parser's levels.
fn prec(e: &Expr) -> u8 {
    match e {
        Expr::Pipe { .. } => 0,
        Expr::Binary { op, .. } => bin_info(*op).0,
        Expr::Range { .. } => 4,
        Expr::Unary { .. } => UNARY,
        Expr::Int { value, .. } if *value < 0 => UNARY,
        Expr::Call { .. } | Expr::Index { .. } | Expr::Field { .. } | Expr::TupleField { .. } => {
            POSTFIX
        }
        _ => PRIMARY,
    }
}

/// True when a bare record value sits where the parser would refuse it in a head.
fn has_bare_record(e: &Expr) -> bool {
    match e {
        Expr::Record { .. } => true,
        Expr::Unary { operand, .. } => has_bare_record(operand),
        Expr::Binary { lhs, rhs, .. } | Expr::Pipe { lhs, rhs, .. } => {
            has_bare_record(lhs) || has_bare_record(rhs)
        }
        Expr::Range { start, end, .. } => has_bare_record(start) || has_bare_record(end),
        Expr::Call { callee: base, .. }
        | Expr::Index { base, .. }
        | Expr::Field { base, .. }
        | Expr::TupleField { base, .. } => has_bare_record(base),
        _ => false,
    }
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn byte_lit(b: u8) -> String {
    let mut out = String::from("b'");
    match b {
        b'\'' => out.push_str("\\'"),
        b'\\' => out.push_str("\\\\"),
        b'\n' => out.push_str("\\n"),
        b'\t' => out.push_str("\\t"),
        b'\r' => out.push_str("\\r"),
        0 => out.push_str("\\0"),
        0x20..=0x7e => out.push(char::from(b)),
        _ => {
            let _ = write!(out, "\\x{:02x}", b);
        }
    }
    out.push('\'');
    out
}

impl Out {
    fn nl(&mut self) {
        self.s.push('\n');
        for _ in 0..self.indent {
            self.s.push_str("    ");
        }
    }

    fn seq<T>(&mut self, items: &[T], sep: &str, f: impl Fn(&mut Self, &T)) {
        for (i, x) in items.iter().enumerate() {
            if i > 0 {
                self.s.push_str(sep);
            }
            f(self, x);
        }
    }

    fn module(&mut self, m: &Module) {
        for (i, item) in m.items.iter().enumerate() {
            if i > 0 {
                self.s.push('\n');
            }
            self.item(item);
            self.s.push('\n');
        }
    }

    fn item(&mut self, it: &Item) {
        match it {
            Item::Type(t) => {
                self.s.push_str("type ");
                self.s.push_str(&t.name.name);
                self.s.push_str(" = ");
                match &t.body {
                    TypeBody::Record(fields) => {
                        self.s.push_str("{ ");
                        self.seq(&fields[..], ", ", |o, f| {
                            o.s.push_str(&f.name.name);
                            o.s.push_str(": ");
                            o.ty(&f.ty);
                        });
                        self.s.push_str(" }");
                    }
                    TypeBody::Variants(vs) => self.seq(&vs[..], " | ", |o, v| {
                        o.s.push_str(&v.name.name);
                        if !v.fields.is_empty() {
                            o.s.push('(');
                            o.seq(&v.fields[..], ", ", |o, t| o.ty(t));
                            o.s.push(')');
                        }
                    }),
                }
            }
            Item::Const(c) => {
                self.s.push_str("const ");
                self.s.push_str(&c.name.name);
                self.s.push_str(": ");
                self.ty(&c.ty);
                self.s.push_str(" = ");
                self.expr(&c.value, 0);
            }
            Item::Layer(l) => {
                self.s.push_str("layer ");
                self.s.push_str(&l.name.name);
            }
            Item::Fn(f) => self.fn_def(f),
            Item::Test(t) => {
                self.s.push_str("test ");
                self.s.push_str(&quote(&t.name));
                self.s.push(' ');
                self.block(&t.body);
            }
            Item::Prop(p) => {
                self.s.push_str("prop ");
                self.s.push_str(&quote(&p.name));
                self.s.push_str(" (");
                self.params(&p.params);
                self.s.push(')');
                if let Some(c) = &p.cond {
                    self.s.push_str(" where ");
                    self.head(c);
                }
                self.s.push(' ');
                self.block(&p.body);
            }
        }
    }

    fn params(&mut self, ps: &[Param]) {
        self.seq(ps, ", ", |o, p| {
            o.s.push_str(&p.name.name);
            o.s.push_str(": ");
            o.ty(&p.ty);
        });
    }

    fn uses(&mut self, uses: &[Ident]) {
        if uses.is_empty() {
            return;
        }
        self.s.push_str(" uses ");
        self.seq(uses, ", ", |o, u| o.s.push_str(&u.name));
    }

    fn fn_def(&mut self, f: &FnDef) {
        self.s.push_str("fn ");
        self.s.push_str(&f.name.name);
        self.s.push('(');
        self.params(&f.params);
        self.s.push(')');
        if let Some(r) = &f.ret {
            self.s.push_str(" -> ");
            self.ty(r);
        }
        self.uses(&f.uses);
        match &f.body {
            FnBody::Block(b) => {
                self.s.push(' ');
                self.block(b);
            }
            FnBody::Extern { path, .. } => {
                self.s.push_str(" = extern ");
                self.s.push_str(&quote(path));
            }
        }
    }

    fn block(&mut self, b: &Block) {
        if b.stmts.is_empty() {
            self.s.push_str("{}");
            return;
        }
        self.s.push('{');
        self.indent += 1;
        for st in &b.stmts {
            self.nl();
            self.stmt(st);
        }
        self.indent -= 1;
        self.nl();
        self.s.push('}');
    }

    fn stmt(&mut self, st: &Stmt) {
        match st {
            Stmt::Let {
                mutable,
                name,
                ty,
                value,
                ..
            } => {
                self.s.push_str(if *mutable { "var " } else { "let " });
                self.s.push_str(&name.name);
                if let Some(t) = ty {
                    self.s.push_str(": ");
                    self.ty(t);
                }
                self.s.push_str(" = ");
                self.expr(value, 0);
            }
            Stmt::Return { value, .. } => {
                self.s.push_str("return");
                if let Some(v) = value {
                    self.s.push(' ');
                    self.expr(v, 0);
                }
            }
            Stmt::Break { .. } => self.s.push_str("break"),
            Stmt::Continue { .. } => self.s.push_str("continue"),
            Stmt::While { cond, body, .. } => {
                self.s.push_str("while ");
                self.head(cond);
                self.s.push(' ');
                self.block(body);
            }
            Stmt::For {
                var, iter, body, ..
            } => {
                self.s.push_str("for ");
                self.s.push_str(&var.name);
                self.s.push_str(" in ");
                self.head(iter);
                self.s.push(' ');
                self.block(body);
            }
            Stmt::Assert { cond, msg, .. } => {
                self.s.push_str("assert ");
                self.expr(cond, 0);
                if let Some(m) = msg {
                    self.s.push_str(", ");
                    self.expr(m, 0);
                }
            }
            Stmt::Assign { target, value, .. } => {
                self.expr(target, 0);
                self.s.push_str(" = ");
                self.expr(value, 0);
            }
            Stmt::Expr { expr, .. } => self.expr(expr, 0),
        }
    }

    /// Prints an expression in a head position, wrapping it when a bare record would appear.
    fn head(&mut self, e: &Expr) {
        if has_bare_record(e) {
            self.s.push('(');
            self.expr(e, 0);
            self.s.push(')');
        } else {
            self.expr(e, 0);
        }
    }

    fn expr(&mut self, e: &Expr, min: u8) {
        let paren = prec(e) < min;
        if paren {
            self.s.push('(');
        }
        self.expr_inner(e);
        if paren {
            self.s.push(')');
        }
    }

    fn expr_inner(&mut self, e: &Expr) {
        match e {
            Expr::Int { value, .. } => self.s.push_str(&value.to_string()),
            Expr::Byte { value, .. } => self.s.push_str(&byte_lit(*value)),
            Expr::Str { value, .. } => self.s.push_str(&quote(value)),
            Expr::Bool { value, .. } => self.s.push_str(if *value { "true" } else { "false" }),
            Expr::None { .. } => self.s.push_str("none"),
            Expr::Some { value, .. } => {
                self.s.push_str("some(");
                self.expr(value, 0);
                self.s.push(')');
            }
            Expr::Name(id) => self.s.push_str(&id.name),
            Expr::Variant { name, args, .. } => {
                self.s.push_str(&name.name);
                if !args.is_empty() {
                    self.s.push('(');
                    self.seq(&args[..], ", ", |o, a| o.expr(a, 0));
                    self.s.push(')');
                }
            }
            Expr::Record { name, fields, .. } => {
                self.s.push_str(&name.name);
                self.s.push_str(" { ");
                self.seq(&fields[..], ", ", |o, f| {
                    o.s.push_str(&f.name.name);
                    o.s.push_str(": ");
                    o.expr(&f.value, 0);
                });
                self.s.push_str(" }");
            }
            Expr::List { items, .. } => {
                self.s.push('[');
                self.seq(&items[..], ", ", |o, a| o.expr(a, 0));
                self.s.push(']');
            }
            Expr::Repeat { value, count, .. } => {
                self.s.push('[');
                self.expr(value, 0);
                self.s.push_str("; ");
                self.expr(count, 0);
                self.s.push(']');
            }
            Expr::Tuple { items, .. } => {
                self.s.push('(');
                self.seq(&items[..], ", ", |o, a| o.expr(a, 0));
                self.s.push(')');
            }
            Expr::Unit { .. } => self.s.push_str("()"),
            Expr::Unary { op, operand, .. } => {
                self.s.push_str(match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "!",
                });
                self.expr(operand, UNARY);
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                let (p, text) = bin_info(*op);
                // Comparisons do not chain, so both of their sides need the tighter level.
                let lhs_min = if p == 3 { p + 1 } else { p };
                self.expr(lhs, lhs_min);
                self.s.push(' ');
                self.s.push_str(text);
                self.s.push(' ');
                self.expr(rhs, p + 1);
            }
            Expr::Pipe { lhs, rhs, .. } => {
                self.expr(lhs, 0);
                self.s.push_str(" |> ");
                self.expr(rhs, POSTFIX);
            }
            Expr::Range { start, end, .. } => {
                self.expr(start, 5);
                self.s.push_str(" .. ");
                self.expr(end, 5);
            }
            Expr::Call { callee, args, .. } => {
                self.expr(callee, POSTFIX);
                self.s.push('(');
                self.seq(&args[..], ", ", |o, a| o.expr(a, 0));
                self.s.push(')');
            }
            Expr::Index { base, index, .. } => {
                self.expr(base, POSTFIX);
                self.s.push('[');
                self.expr(index, 0);
                self.s.push(']');
            }
            Expr::Field { base, name, .. } => {
                self.expr(base, POSTFIX);
                self.s.push('.');
                self.s.push_str(&name.name);
            }
            Expr::TupleField { base, index, .. } => {
                self.expr(base, POSTFIX);
                self.s.push('.');
                self.s.push_str(&index.to_string());
            }
            Expr::If {
                cond, then, els, ..
            } => {
                self.s.push_str("if ");
                self.head(cond);
                self.s.push(' ');
                self.block(then);
                if let Some(e) = els {
                    self.s.push_str(" else ");
                    match &**e {
                        Expr::Block(b) => self.block(b),
                        other => self.expr(other, 0),
                    }
                }
            }
            Expr::Match { subject, arms, .. } => {
                self.s.push_str("match ");
                self.head(subject);
                self.s.push_str(" {");
                self.indent += 1;
                for arm in arms {
                    self.nl();
                    self.arm(arm);
                }
                self.indent -= 1;
                self.nl();
                self.s.push('}');
            }
            Expr::Block(b) => self.block(b),
        }
    }

    fn arm(&mut self, arm: &Arm) {
        self.pattern(&arm.pattern);
        self.s.push_str(" => ");
        self.expr(&arm.body, 0);
        self.s.push(',');
    }

    fn pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Wild { .. } => self.s.push('_'),
            Pattern::Bind(id) => self.s.push_str(&id.name),
            Pattern::Int { value, .. } => self.s.push_str(&value.to_string()),
            Pattern::Byte { value, .. } => self.s.push_str(&byte_lit(*value)),
            Pattern::Str { value, .. } => self.s.push_str(&quote(value)),
            Pattern::Bool { value, .. } => self.s.push_str(if *value { "true" } else { "false" }),
            Pattern::None { .. } => self.s.push_str("none"),
            Pattern::Some { inner, .. } => {
                self.s.push_str("some(");
                self.pattern(inner);
                self.s.push(')');
            }
            Pattern::Variant { name, args, .. } => {
                self.s.push_str(&name.name);
                if !args.is_empty() {
                    self.s.push('(');
                    self.seq(&args[..], ", ", |o, a| o.pattern(a));
                    self.s.push(')');
                }
            }
            Pattern::Tuple { items, .. } => {
                self.s.push('(');
                self.seq(&items[..], ", ", |o, a| o.pattern(a));
                self.s.push(')');
            }
        }
    }

    fn ty(&mut self, t: &Type) {
        match t {
            Type::Int { .. } => self.s.push_str("int"),
            Type::Bool { .. } => self.s.push_str("bool"),
            Type::Byte { .. } => self.s.push_str("byte"),
            Type::Str { .. } => self.s.push_str("str"),
            Type::Unit { .. } => self.s.push_str("unit"),
            Type::Named(id) => self.s.push_str(&id.name),
            Type::List { elem, .. } => {
                self.s.push('[');
                self.ty(elem);
                self.s.push(']');
            }
            Type::Tuple { items, .. } => {
                self.s.push('(');
                self.seq(&items[..], ", ", |o, a| o.ty(a));
                self.s.push(')');
            }
            Type::Option { inner, .. } => {
                self.ty(inner);
                self.s.push('?');
            }
            Type::Fn {
                params, ret, uses, ..
            } => {
                self.s.push_str("fn(");
                self.seq(&params[..], ", ", |o, a| o.ty(a));
                self.s.push_str(") -> ");
                self.ty(ret);
                self.uses(uses);
            }
        }
    }
}
