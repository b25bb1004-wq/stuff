//! Source line preprocessing, comment stripping, label extraction, and tokenization.

use std::collections::HashMap;

use crate::macros::{expand_macro_body, parse_macro_header, MacroDef, OpenMacro};
use crate::AsmError;

/// A label attached to a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LabelDef {
    Named(String),
    Numeric(u64),
}

/// One preprocessed statement line ready for Pass 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StmtLine {
    pub line: usize,
    pub labels: Vec<LabelDef>,
    pub text: String,
}

/// Strip `#` comments outside `"..."` and `'...'` literals.
pub(crate) fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_dq = false;
    let mut in_sq = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_dq {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_dq = false;
            }
        } else if in_sq {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_sq = false;
            }
        } else {
            match b {
                b'#' => return &line[..i],
                b'"' => in_dq = true,
                b'\'' => in_sq = true,
                _ => {}
            }
        }
        i += 1;
    }
    line
}

pub(crate) fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || matches!(b, b'_' | b'.' | b'$')
}

pub(crate) fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'$')
}

pub(crate) fn is_valid_ident(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && is_ident_start(b[0]) && b[1..].iter().copied().all(is_ident_cont)
}

/// Peel all leading `name:` and `N:` labels from a comment-stripped line.
pub(crate) fn extract_labels(mut s: &str, line: usize) -> Result<(Vec<LabelDef>, &str), AsmError> {
    let mut labels = Vec::new();
    loop {
        s = s.trim_start();
        let bytes = s.as_bytes();
        if bytes.is_empty() {
            break;
        }
        if bytes[0].is_ascii_digit() {
            let mut end = 1;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            let after = s[end..].trim_start();
            if let Some(rest) = after.strip_prefix(':') {
                let num: u64 = s[..end]
                    .parse()
                    .map_err(|_| AsmError::new(line, "invalid numeric label"))?;
                labels.push(LabelDef::Numeric(num));
                s = rest;
                continue;
            }
            break;
        }
        if is_ident_start(bytes[0]) {
            let mut end = 1;
            while end < bytes.len() && is_ident_cont(bytes[end]) {
                end += 1;
            }
            let after = s[end..].trim_start();
            if let Some(rest) = after.strip_prefix(':') {
                labels.push(LabelDef::Named(s[..end].to_string()));
                s = rest;
                continue;
            }
            break;
        }
        break;
    }
    Ok((labels, s.trim()))
}

/// Split a head token (mnemonic or directive) from the rest of the statement.
pub(crate) fn split_head(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.find(|c: char| c.is_ascii_whitespace()) {
        Some(idx) => (&s[..idx], s[idx..].trim()),
        None => (s, ""),
    }
}

/// Split comma-separated operands outside `(...)`, `"..."`, and `'...'`.
pub(crate) fn split_operands(s: &str, line: usize) -> Result<Vec<&str>, AsmError> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut in_dq = false;
    let mut in_sq = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_dq {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_dq = false;
            }
        } else if in_sq {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_sq = false;
            }
        } else {
            match b {
                b'"' => in_dq = true,
                b'\'' => in_sq = true,
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => {
                    let piece = s[start..i].trim();
                    if piece.is_empty() {
                        return Err(AsmError::new(line, "empty operand"));
                    }
                    parts.push(piece);
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }
    let last = s[start..].trim();
    if last.is_empty() {
        return Err(AsmError::new(line, "trailing comma in operands"));
    }
    parts.push(last);
    Ok(parts)
}

/// Parse a `"..."` string literal with escapes `\n \t \r \0 \\ \" \'`.
pub(crate) fn parse_string_lit(s: &str, line: usize) -> Result<Vec<u8>, AsmError> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'"') {
        return Err(AsmError::new(line, "expected string literal"));
    }
    let mut out = Vec::new();
    let mut i = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            if i + 1 != bytes.len() {
                return Err(AsmError::new(
                    line,
                    "unexpected trailing characters after string",
                ));
            }
            return Ok(out);
        }
        if b == b'\\' {
            i += 1;
            if i >= bytes.len() {
                return Err(AsmError::new(line, "unterminated string"));
            }
            let esc = match bytes[i] {
                b'n' => b'\n',
                b't' => b'\t',
                b'r' => b'\r',
                b'0' => b'\0',
                b'\\' => b'\\',
                b'"' => b'"',
                b'\'' => b'\'',
                other => {
                    return Err(AsmError::new(
                        line,
                        format!("unknown string escape '\\{}'", other as char),
                    ));
                }
            };
            out.push(esc);
        } else {
            out.push(b);
        }
        i += 1;
    }
    Err(AsmError::new(line, "unterminated string"))
}

/// Preprocess source text: collect macros, expand invocations, and strip comments.
pub(crate) fn preprocess(src: &str) -> (Vec<StmtLine>, Option<AsmError>) {
    let mut macros: HashMap<String, MacroDef> = HashMap::new();
    let mut out = Vec::new();
    let mut in_macro: Option<OpenMacro> = None;

    for (idx, raw_line) in src.lines().enumerate() {
        let line_no = idx + 1;
        let line_no_cr = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let stripped = strip_comment(line_no_cr);
        let trimmed = stripped.trim();

        if let Some(mut om) = in_macro.take() {
            let (head, _) = split_head(trimmed);
            if head == ".macro" {
                om.nest += 1;
                om.body.push(stripped.to_string());
                in_macro = Some(om);
            } else if head == ".endm" {
                if om.nest > 0 {
                    om.nest -= 1;
                    om.body.push(stripped.to_string());
                    in_macro = Some(om);
                } else {
                    macros.insert(
                        om.name,
                        MacroDef {
                            params: om.params,
                            body: om.body,
                        },
                    );
                }
            } else {
                om.body.push(stripped.to_string());
                in_macro = Some(om);
            }
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        let (labels, rest) = match extract_labels(stripped, line_no) {
            Ok(v) => v,
            Err(e) => return (out, Some(e)),
        };
        if labels.is_empty() && rest.is_empty() {
            continue;
        }

        if !rest.is_empty() {
            let (head, tail) = split_head(rest);
            if head == ".macro" {
                if !labels.is_empty() {
                    out.push(StmtLine {
                        line: line_no,
                        labels,
                        text: String::new(),
                    });
                }
                match parse_macro_header(tail, line_no) {
                    Ok((name, params)) => {
                        in_macro = Some(OpenMacro {
                            start_line: line_no,
                            name,
                            params,
                            body: Vec::new(),
                            nest: 0,
                        });
                    }
                    Err(e) => return (out, Some(e)),
                }
                continue;
            }
            if head == ".endm" {
                return (out, Some(AsmError::new(line_no, "unexpected .endm")));
            }
            if let Some(mac) = macros.get(head) {
                if !labels.is_empty() {
                    out.push(StmtLine {
                        line: line_no,
                        labels,
                        text: String::new(),
                    });
                }
                let args = match split_operands(tail, line_no) {
                    Ok(a) => a,
                    Err(e) => return (out, Some(e)),
                };
                if args.len() != mac.params.len() {
                    return (
                        out,
                        Some(AsmError::new(
                            line_no,
                            format!(
                                "macro '{head}' expects {} arguments, got {}",
                                mac.params.len(),
                                args.len()
                            ),
                        )),
                    );
                }
                if let Err(e) = expand_macro_body(mac, &args, line_no, &macros, &mut out, 0) {
                    return (out, Some(e));
                }
                continue;
            }
        }

        out.push(StmtLine {
            line: line_no,
            labels,
            text: rest.to_string(),
        });
    }

    if let Some(om) = in_macro {
        return (out, Some(AsmError::new(om.start_line, "missing .endm")));
    }

    (out, None)
}
