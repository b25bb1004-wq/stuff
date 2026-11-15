//! Macro definition and recursive expansion with parameter substitution.

use std::collections::HashMap;

use crate::lexer::{
    extract_labels, is_ident_cont, is_ident_start, is_valid_ident, split_head, split_operands,
    strip_comment, StmtLine,
};
use crate::AsmError;

#[derive(Debug, Clone)]
pub(crate) struct MacroDef {
    pub params: Vec<String>,
    pub body: Vec<String>,
}

pub(crate) struct OpenMacro {
    pub start_line: usize,
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<String>,
    pub nest: usize,
}

pub(crate) fn parse_macro_header(
    rest: &str,
    line: usize,
) -> Result<(String, Vec<String>), AsmError> {
    let (name, param_str) = split_head(rest);
    if !is_valid_ident(name) {
        return Err(AsmError::new(line, format!("invalid macro name '{name}'")));
    }
    let param_str = param_str.strip_prefix(',').unwrap_or(param_str).trim();
    let mut params = Vec::new();
    if !param_str.is_empty() {
        let raw_params: Vec<&str> = if param_str.contains(',') {
            split_operands(param_str, line)?
        } else {
            param_str.split_whitespace().collect()
        };
        for raw in raw_params {
            if !is_valid_ident(raw) {
                return Err(AsmError::new(
                    line,
                    format!("invalid macro parameter '{raw}'"),
                ));
            }
            params.push(raw.to_string());
        }
    }
    Ok((name.to_string(), params))
}

fn substitute_params(body_line: &str, params: &[String], args: &[&str]) -> String {
    let mut out = String::with_capacity(body_line.len());
    let bytes = body_line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if i + 2 < bytes.len() && bytes[i + 1] == b'(' && bytes[i + 2] == b')' {
                i += 3;
                continue;
            }
            let start = i + 1;
            if start < bytes.len() && is_ident_start(bytes[start]) {
                let mut end = start + 1;
                while end < bytes.len() && is_ident_cont(bytes[end]) {
                    end += 1;
                }
                let ident = &body_line[start..end];
                if let Some(idx) = params.iter().position(|p| p == ident) {
                    out.push_str(args[idx]);
                    i = end;
                    continue;
                }
            }
        }
        if let Some(ch) = body_line[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    out
}

/// Most statements a whole source may expand to through macros.
const MAX_EXPANDED_STMTS: usize = 1_000_000;
/// Longest line a macro substitution may produce.
const MAX_EXPANDED_LINE: usize = 1 << 20;

pub(crate) fn expand_macro_body(
    mac: &MacroDef,
    args: &[&str],
    invoke_line: usize,
    macros: &HashMap<String, MacroDef>,
    out: &mut Vec<StmtLine>,
    depth: usize,
) -> Result<(), AsmError> {
    if depth > 64 {
        return Err(AsmError::new(
            invoke_line,
            "macro expansion recursion depth exceeded",
        ));
    }
    for raw in &mac.body {
        // depth is capped, but nested macros can still double the work at every level
        if out.len() >= MAX_EXPANDED_STMTS {
            return Err(AsmError::new(invoke_line, "macro expansion too large"));
        }
        let sub = substitute_params(raw, &mac.params, args);
        if sub.len() > MAX_EXPANDED_LINE {
            return Err(AsmError::new(invoke_line, "macro expansion line too long"));
        }
        let stripped = strip_comment(&sub);
        let (labels, rest) = extract_labels(stripped, invoke_line)?;
        if labels.is_empty() && rest.is_empty() {
            continue;
        }
        if !rest.is_empty() {
            let (head, tail) = split_head(rest);
            if let Some(nested) = macros.get(head) {
                if !labels.is_empty() {
                    out.push(StmtLine {
                        line: invoke_line,
                        labels,
                        text: String::new(),
                    });
                }
                let nested_args = split_operands(tail, invoke_line)?;
                if nested_args.len() != nested.params.len() {
                    return Err(AsmError::new(
                        invoke_line,
                        format!(
                            "macro '{head}' expects {} arguments, got {}",
                            nested.params.len(),
                            nested_args.len()
                        ),
                    ));
                }
                expand_macro_body(nested, &nested_args, invoke_line, macros, out, depth + 1)?;
                continue;
            }
        }
        out.push(StmtLine {
            line: invoke_line,
            labels,
            text: rest.to_string(),
        });
    }
    Ok(())
}
