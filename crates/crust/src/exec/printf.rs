//! POSIX `printf` builtin formatter.

use std::format;
use std::prelude::rust_2021::*;

use super::redir::{FdTable, IoStreams};

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

fn parse_printf_int(s: &str) -> Result<i64, String> {
    if s.is_empty() {
        return Ok(0);
    }
    if let Some(rest) = s.strip_prefix('\'').or_else(|| s.strip_prefix('"')) {
        if let Some(ch) = rest.chars().next() {
            return Ok(i64::from(u32::from(ch)));
        }
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
    match val {
        Ok(v) => Ok(if neg { v.wrapping_neg() } else { v }),
        Err(_) => Err(format!("invalid number: {s}")),
    }
}

fn pad_field(s: &str, width: usize, left: bool, pad_char: char) -> String {
    let char_len = s.chars().count();
    if char_len >= width {
        return s.to_string();
    }
    let pad_len = width - char_len;
    let pad: String = std::iter::repeat_n(pad_char, pad_len).collect();
    if left {
        format!("{s}{pad}")
    } else if pad_char == '0' && s.starts_with('-') {
        format!("-{pad}{}", &s[1..])
    } else {
        format!("{pad}{s}")
    }
}

/// Execute `printf` builtin.
pub(crate) fn builtin_printf(args: &[String], fds: &mut FdTable, io: &mut IoStreams<'_>) -> i32 {
    let Some(fmt) = args.first() else {
        fds.write_err(io, "crust: printf: usage: printf format [arguments]\n");
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
                            if let Some(&d @ b'0'..=b'7') = fmt_bytes.get(j) {
                                let _ = d;
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

            let mut left = false;
            let mut zero_pad = false;
            while let Some(&fb) = fmt_bytes.get(i) {
                match fb {
                    b'-' => {
                        left = true;
                        i += 1;
                    }
                    b'0' => {
                        zero_pad = true;
                        i += 1;
                    }
                    b'+' | b' ' | b'#' => {
                        i += 1;
                    }
                    _ => break,
                }
            }
            if left {
                zero_pad = false;
            }

            let mut width = 0usize;
            while let Some(&wb @ b'0'..=b'9') = fmt_bytes.get(i) {
                width = width
                    .saturating_mul(10)
                    .saturating_add(usize::from(wb - b'0'));
                i += 1;
            }

            if fmt_bytes.get(i) == Some(&b'.') {
                i += 1;
                while let Some(&pb @ b'0'..=b'9') = fmt_bytes.get(i) {
                    let _ = pb;
                    i += 1;
                }
            }

            let Some(&spec) = fmt_bytes.get(i) else {
                break;
            };
            i += 1;
            saw_spec = true;

            let arg = rest.get(arg_idx).map(String::as_str).unwrap_or("");
            arg_idx += 1;

            match spec {
                b's' => {
                    let formatted = pad_field(arg, width, left, ' ');
                    out.extend_from_slice(formatted.as_bytes());
                }
                b'b' => {
                    let (unescaped, stop) = unescape_bytes(arg, true);
                    let s = String::from_utf8_lossy(&unescaped);
                    let formatted = pad_field(&s, width, left, ' ');
                    out.extend_from_slice(formatted.as_bytes());
                    if stop {
                        let _ = fds.write_fd(1, io, &out);
                        return status;
                    }
                }
                b'c' => {
                    let ch = arg.chars().next().unwrap_or('\0');
                    let s = if ch == '\0' {
                        String::new()
                    } else {
                        ch.to_string()
                    };
                    let formatted = pad_field(&s, width, left, ' ');
                    out.extend_from_slice(formatted.as_bytes());
                }
                b'd' | b'i' | b'x' | b'X' | b'o' => {
                    let n = match parse_printf_int(arg) {
                        Ok(v) => v,
                        Err(e) => {
                            fds.write_err(io, &format!("crust: printf: {e}\n"));
                            status = 1;
                            0
                        }
                    };
                    let raw = match spec {
                        b'd' | b'i' => format!("{n}"),
                        b'x' => format!("{:x}", n as u64),
                        b'X' => format!("{:X}", n as u64),
                        b'o' => format!("{:o}", n as u64),
                        _ => unreachable!(),
                    };
                    let pad_ch = if zero_pad { '0' } else { ' ' };
                    let formatted = pad_field(&raw, width, left, pad_ch);
                    out.extend_from_slice(formatted.as_bytes());
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
