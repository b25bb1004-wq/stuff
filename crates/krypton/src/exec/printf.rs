//! POSIX `printf` builtin formatter.

use std::format;
use std::prelude::rust_2021::*;

use super::redir::{FdTable, IoStreams};

/// Largest width or precision honored, so a huge request cannot exhaust memory.
/// Larger requests are an error (exit 1), never silent truncation.
const MAX_FIELD: u64 = 1 << 20;

fn unescape_bytes(s: &str, stop_on_c: bool) -> (Vec<u8>, bool) {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b != b'\\' {
            out.push(b);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&esc) = bytes.get(i) else {
            out.push(b'\\');
            break;
        };
        i += 1;
        match esc {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'\'' => out.push(b'\''),
            b'c' if stop_on_c => return (out, true),
            b'0'..=b'7' => {
                let mut val = u32::from(esc - b'0');
                let mut count = 1;
                while count < 3 {
                    if let Some(&d @ b'0'..=b'7') = bytes.get(i) {
                        val = val * 8 + u32::from(d - b'0');
                        i += 1;
                        count += 1;
                    } else {
                        break;
                    }
                }
                out.push((val & 0xff) as u8);
            }
            b'x' => {
                let mut val = 0u32;
                let mut count = 0;
                while count < 2 {
                    if let Some(&d) = bytes.get(i) {
                        if let Some(hex) = (d as char).to_digit(16) {
                            val = val * 16 + hex;
                            i += 1;
                            count += 1;
                            continue;
                        }
                    }
                    break;
                }
                if count > 0 {
                    out.push((val & 0xff) as u8);
                } else {
                    out.push(b'\\');
                    out.push(b'x');
                }
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    (out, false)
}

/// Value of a `'c` or `"c` character-constant argument.
fn quoted_char(s: &str) -> Option<char> {
    let rest = s.strip_prefix('\'').or_else(|| s.strip_prefix('"'))?;
    rest.chars().next()
}

/// Integer argument: decimal, octal (leading 0), hex (0x), or a character constant. Empty is 0.
fn parse_int(s: &str) -> Option<i64> {
    if s.is_empty() {
        return Some(0);
    }
    if let Some(ch) = quoted_char(s) {
        return Some(i64::from(u32::from(ch)));
    }
    let trimmed = s.trim();
    let (neg, body) = if let Some(r) = trimmed.strip_prefix('-') {
        (true, r)
    } else if let Some(r) = trimmed.strip_prefix('+') {
        (false, r)
    } else {
        (false, trimmed)
    };
    let val = if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16)
    } else if body.starts_with('0') && body.len() > 1 {
        i64::from_str_radix(&body[1..], 8)
    } else {
        body.parse::<i64>()
    };
    val.ok().map(|v| if neg { v.wrapping_neg() } else { v })
}

/// Floating-point argument. Empty is 0.0; a character constant gives its code.
fn parse_float(s: &str) -> Option<f64> {
    if s.is_empty() {
        return Some(0.0);
    }
    if let Some(ch) = quoted_char(s) {
        return Some(f64::from(u32::from(ch)));
    }
    s.trim().parse::<f64>().ok()
}

/// Parsed `%` conversion flags, width and precision.
#[derive(Default)]
struct Spec {
    left: bool,
    zero: bool,
    plus: bool,
    space: bool,
    alt: bool,
    width: usize,
    prec: Option<usize>,
}

fn check_field(n: u64, fds: &mut FdTable, io: &mut IoStreams<'_>, status: &mut i32) -> usize {
    if n > MAX_FIELD {
        fds.write_err(
            io,
            "krypton: printf: width or precision exceeds the 1048576 (1 MiB) limit\n",
        );
        *status = 1;
        return 0;
    }
    usize::try_from(n).unwrap_or(0)
}

/// Pad `body` (with an optional sign or `0x` prefix) to the spec's width.
fn assemble(prefix: &str, body: &str, sp: &Spec, zero_ok: bool) -> String {
    let len = prefix.chars().count() + body.chars().count();
    let pad = sp.width.saturating_sub(len);
    if sp.left {
        format!("{prefix}{body}{}", " ".repeat(pad))
    } else if zero_ok {
        format!("{prefix}{}{body}", "0".repeat(pad))
    } else {
        format!("{}{prefix}{body}", " ".repeat(pad))
    }
}

fn fmt_int(sp: &Spec, conv: u8, n: i64) -> String {
    let (neg, mut digits) = match conv {
        b'd' | b'i' => (n < 0, n.unsigned_abs().to_string()),
        b'u' => (false, (n as u64).to_string()),
        b'o' => (false, format!("{:o}", n as u64)),
        b'x' => (false, format!("{:x}", n as u64)),
        _ => (false, format!("{:X}", n as u64)),
    };
    if sp.prec == Some(0) && n == 0 {
        digits.clear();
    }
    let min = sp.prec.unwrap_or(0);
    if digits.len() < min {
        digits = format!("{}{digits}", "0".repeat(min - digits.len()));
    }
    let mut prefix = String::new();
    if matches!(conv, b'd' | b'i') {
        if neg {
            prefix.push('-');
        } else if sp.plus {
            prefix.push('+');
        } else if sp.space {
            prefix.push(' ');
        }
    }
    if sp.alt {
        match conv {
            b'o' if !digits.starts_with('0') => digits.insert(0, '0'),
            b'x' if n != 0 => prefix.push_str("0x"),
            b'X' if n != 0 => prefix.push_str("0X"),
            _ => {}
        }
    }
    let zero_ok = sp.zero && !sp.left && sp.prec.is_none();
    assemble(&prefix, &digits, sp, zero_ok)
}

/// Decimal exponent of `a` after rounding to `prec` fractional digits in scientific form.
fn exp10(a: f64, prec: usize) -> i32 {
    let s = format!("{:.*e}", prec, a);
    s.split_once('e')
        .and_then(|(_, e)| e.parse::<i32>().ok())
        .unwrap_or(0)
}

/// C-style `%e` body: mantissa, `e`, sign, and at least two exponent digits.
fn sci(a: f64, prec: usize, alt: bool) -> String {
    let s = format!("{:.*e}", prec, a);
    let mant = s.split_once('e').map_or(s.as_str(), |(m, _)| m);
    let e = exp10(a, prec);
    let dot = if alt && prec == 0 { "." } else { "" };
    format!(
        "{mant}{dot}e{}{:02}",
        if e < 0 { '-' } else { '+' },
        e.unsigned_abs()
    )
}

fn fixed(a: f64, prec: usize, alt: bool) -> String {
    let dot = if alt && prec == 0 { "." } else { "" };
    format!("{:.*}{dot}", prec, a)
}

/// Drop trailing zeros (and a bare point) from the fraction, keeping any exponent.
fn strip_zeros(s: &str) -> String {
    let (num, tail) = match s.find('e') {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    };
    if !num.contains('.') {
        return s.to_string();
    }
    let trimmed = num.trim_end_matches('0').trim_end_matches('.');
    format!("{trimmed}{tail}")
}

fn general(a: f64, prec: usize, alt: bool) -> String {
    let p = prec.max(1);
    let x = exp10(a, p - 1);
    let p_i = i32::try_from(p).unwrap_or(i32::MAX);
    let s = if x < -4 || x >= p_i {
        sci(a, p - 1, alt)
    } else {
        let frac = p_i - 1 - x;
        fixed(a, usize::try_from(frac).unwrap_or(0), alt)
    };
    if alt {
        s
    } else {
        strip_zeros(&s)
    }
}

fn fmt_float(sp: &Spec, conv: u8, v: f64) -> String {
    let neg = v.is_sign_negative() && !v.is_nan();
    let a = v.abs();
    let prec = sp.prec.unwrap_or(6);
    let body = if a.is_nan() {
        "nan".to_string()
    } else if a.is_infinite() {
        "inf".to_string()
    } else {
        match conv.to_ascii_lowercase() {
            b'f' => fixed(a, prec, sp.alt),
            b'e' => sci(a, prec, sp.alt),
            _ => general(a, prec, sp.alt),
        }
    };
    let body = if conv.is_ascii_uppercase() {
        body.to_ascii_uppercase()
    } else {
        body
    };
    let mut prefix = String::new();
    if neg {
        prefix.push('-');
    } else if sp.plus {
        prefix.push('+');
    } else if sp.space {
        prefix.push(' ');
    }
    let zero_ok = sp.zero && !sp.left && a.is_finite();
    assemble(&prefix, &body, sp, zero_ok)
}

/// Read the next argument, or "" once the arguments run out.
fn next_arg<'a>(rest: &'a [String], idx: &mut usize) -> &'a str {
    let s = rest.get(*idx).map_or("", String::as_str);
    *idx += 1;
    s
}

fn int_arg(arg: &str, fds: &mut FdTable, io: &mut IoStreams<'_>, status: &mut i32) -> i64 {
    if let Some(v) = parse_int(arg) {
        return v;
    }
    fds.write_err(io, &format!("krypton: printf: {arg}: invalid number\n"));
    *status = 1;
    0
}

fn float_arg(arg: &str, fds: &mut FdTable, io: &mut IoStreams<'_>, status: &mut i32) -> f64 {
    if let Some(v) = parse_float(arg) {
        return v;
    }
    fds.write_err(io, &format!("krypton: printf: {arg}: invalid number\n"));
    *status = 1;
    0.0
}

/// Execute `printf` builtin.
pub(crate) fn builtin_printf(args: &[String], fds: &mut FdTable, io: &mut IoStreams<'_>) -> i32 {
    let Some(fmt) = args.first() else {
        fds.write_err(io, "krypton: printf: usage: printf format [arguments]\n");
        return 1;
    };
    let rest = &args[1..];
    let mut arg_idx = 0usize;
    let mut out = Vec::new();
    let mut status = 0;
    let fmt_bytes = fmt.as_bytes();

    loop {
        let start_idx = arg_idx;
        let mut i = 0usize;
        let mut saw_spec = false;

        while let Some(&b) = fmt_bytes.get(i) {
            if b == b'\\' {
                let mut j = i + 1;
                if let Some(&esc) = fmt_bytes.get(j) {
                    j += 1;
                    if esc == b'0' || (b'1'..=b'7').contains(&esc) {
                        let mut count = 1;
                        while count < 3 {
                            if matches!(fmt_bytes.get(j), Some(b'0'..=b'7')) {
                                j += 1;
                                count += 1;
                            } else {
                                break;
                            }
                        }
                    } else if esc == b'x' {
                        let mut count = 0;
                        while count < 2 {
                            if let Some(&d) = fmt_bytes.get(j) {
                                if (d as char).is_ascii_hexdigit() {
                                    j += 1;
                                    count += 1;
                                    continue;
                                }
                            }
                            break;
                        }
                    }
                }
                if let Ok(seg) = std::str::from_utf8(&fmt_bytes[i..j]) {
                    let (unescaped, _) = unescape_bytes(seg, false);
                    out.extend_from_slice(&unescaped);
                }
                i = j;
                continue;
            }

            if b != b'%' {
                out.push(b);
                i += 1;
                continue;
            }

            i += 1;
            if fmt_bytes.get(i) == Some(&b'%') {
                out.push(b'%');
                i += 1;
                continue;
            }

            let mut sp = Spec::default();
            while let Some(&fb) = fmt_bytes.get(i) {
                match fb {
                    b'-' => sp.left = true,
                    b'0' => sp.zero = true,
                    b'+' => sp.plus = true,
                    b' ' => sp.space = true,
                    b'#' => sp.alt = true,
                    _ => break,
                }
                i += 1;
            }

            if fmt_bytes.get(i) == Some(&b'*') {
                i += 1;
                let w = int_arg(next_arg(rest, &mut arg_idx), fds, io, &mut status);
                if w < 0 {
                    sp.left = true;
                }
                sp.width = check_field(w.unsigned_abs(), fds, io, &mut status);
            } else {
                let mut w = 0u64;
                while let Some(&wb @ b'0'..=b'9') = fmt_bytes.get(i) {
                    w = w.saturating_mul(10).saturating_add(u64::from(wb - b'0'));
                    i += 1;
                }
                sp.width = check_field(w, fds, io, &mut status);
            }

            if fmt_bytes.get(i) == Some(&b'.') {
                i += 1;
                if fmt_bytes.get(i) == Some(&b'*') {
                    i += 1;
                    let p = int_arg(next_arg(rest, &mut arg_idx), fds, io, &mut status);
                    sp.prec = u64::try_from(p)
                        .ok()
                        .map(|p| check_field(p, fds, io, &mut status));
                } else {
                    let mut p = 0u64;
                    while let Some(&pb @ b'0'..=b'9') = fmt_bytes.get(i) {
                        p = p.saturating_mul(10).saturating_add(u64::from(pb - b'0'));
                        i += 1;
                    }
                    sp.prec = Some(check_field(p, fds, io, &mut status));
                }
            }

            let Some(&spec) = fmt_bytes.get(i) else {
                break;
            };
            i += 1;
            saw_spec = true;

            match spec {
                b's' | b'b' | b'c' => {
                    let arg = next_arg(rest, &mut arg_idx);
                    let mut stop_all = false;
                    let text = match spec {
                        b's' => arg.to_string(),
                        b'b' => {
                            let (unescaped, stop) = unescape_bytes(arg, true);
                            // `\c` ends all output, including the rest of the format.
                            stop_all = stop;
                            String::from_utf8_lossy(&unescaped).into_owned()
                        }
                        _ => arg.chars().next().map(String::from).unwrap_or_default(),
                    };
                    let text = match sp.prec {
                        Some(p) if spec == b's' => text.chars().take(p).collect(),
                        _ => text,
                    };
                    out.extend_from_slice(assemble("", &text, &sp, false).as_bytes());
                    if stop_all {
                        let _ = fds.write_fd(1, io, &out);
                        return status;
                    }
                }
                b'd' | b'i' | b'u' | b'x' | b'X' | b'o' => {
                    let n = int_arg(next_arg(rest, &mut arg_idx), fds, io, &mut status);
                    out.extend_from_slice(fmt_int(&sp, spec, n).as_bytes());
                }
                b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => {
                    let v = float_arg(next_arg(rest, &mut arg_idx), fds, io, &mut status);
                    out.extend_from_slice(fmt_float(&sp, spec, v).as_bytes());
                }
                other => {
                    out.push(b'%');
                    out.push(other);
                }
            }
        }

        if !saw_spec || arg_idx >= rest.len() || arg_idx == start_idx {
            break;
        }
    }

    if fds.write_fd(1, io, &out).is_err() {
        return 1;
    }
    status
}
