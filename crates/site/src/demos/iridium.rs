//! Iridium SQL in-memory database demo.

use std::fmt::Write as _;

use iridium::sql::{Db, Value};
use iridium::MemStorage;

use super::{check_input, hex_lower, Demo, Output};

const MAX_STATEMENTS: usize = 256;
const MAX_DISPLAY_ROWS: usize = 256;

const DEFAULT_SQL: &str = "\
CREATE TABLE layers (id INT PRIMARY KEY, name TEXT, depth INT);
INSERT INTO layers VALUES (1, 'Titan', 100), (2, 'Iridium', 200), (3, 'Hades', 300);
SELECT name, depth FROM layers WHERE depth >= 150 ORDER BY depth DESC;
UPDATE layers SET depth = depth + 10 WHERE id = 1;
SELECT COUNT(*) AS total, SUM(depth) AS sum_depth FROM layers;";

/// Registered Iridium SQL demo.
pub const DEMO: Demo = Demo {
    id: "iridium",
    title: "Iridium SQL",
    default_input: DEFAULT_SQL,
    run,
};

/// Execute a semicolon-separated SQL script on an in-memory Iridium `Db`.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let stmts = split_statements(input);
    if stmts.is_empty() {
        return Err(String::from("empty SQL script"));
    }
    if stmts.len() > MAX_STATEMENTS {
        return Err(format!("too many statements (limit {MAX_STATEMENTS})"));
    }

    let mut db = Db::open(MemStorage::new(), MemStorage::new())
        .map_err(|e| format!("failed to open in-memory db: {e}"))?;

    let mut out = String::new();
    for (idx, stmt) in stmts.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let _ = writeln!(out, "> {stmt}");
        let rows = db
            .execute(stmt)
            .map_err(|e| format!("statement {}: {e}", idx + 1))?;
        if rows.columns.is_empty() {
            let _ = writeln!(out, "ok ({} changes)", rows.changes);
        } else {
            format_table(&mut out, &rows.columns, &rows.rows);
        }
    }
    Ok(Output::Text(out))
}

fn split_statements(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_quote = false;
    let mut in_line_comment = false;

    while i < bytes.len() {
        let b = bytes[i];
        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_quote {
            if b == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                in_quote = false;
            }
            i += 1;
            continue;
        }
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            in_line_comment = true;
            i += 2;
            continue;
        }
        if b == b'\'' {
            in_quote = true;
            i += 1;
            continue;
        }
        if b == b';' {
            let part = input[start..i].trim();
            if !part.is_empty() {
                out.push(part);
            }
            start = i + 1;
        }
        i += 1;
    }

    let tail = input[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn format_table(out: &mut String, cols: &[String], rows: &[Vec<Value>]) {
    let shown = rows.len().min(MAX_DISPLAY_ROWS);
    let mut widths: Vec<usize> = cols.iter().map(|c| c.chars().count()).collect();
    let mut rendered: Vec<Vec<String>> = Vec::with_capacity(shown);

    for row in rows.iter().take(shown) {
        let mut cells = Vec::with_capacity(cols.len());
        for (i, val) in row.iter().enumerate() {
            let s = format_value(val);
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(s.chars().count());
            }
            cells.push(s);
        }
        rendered.push(cells);
    }

    write_row(out, cols.iter().map(String::as_str), &widths);
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            out.push_str("-+-");
        }
        for _ in 0..*w {
            out.push('-');
        }
    }
    out.push('\n');

    for row in &rendered {
        write_row(out, row.iter().map(String::as_str), &widths);
    }
    if rows.len() > shown {
        let _ = writeln!(out, "... ({} more rows)", rows.len() - shown);
    }
}

fn write_row<'a>(out: &mut String, cells: impl Iterator<Item = &'a str>, widths: &[usize]) {
    for (i, (cell, width)) in cells.zip(widths.iter()).enumerate() {
        if i > 0 {
            out.push_str(" | ");
        }
        let len = cell.chars().count();
        out.push_str(cell);
        for _ in len..*width {
            out.push(' ');
        }
    }
    out.push('\n');
}

fn format_value(val: &Value) -> String {
    match val {
        Value::Null => String::from("NULL"),
        Value::Int(n) => n.to_string(),
        Value::Text(s) => s.clone(),
        Value::Blob(b) => {
            if b.len() > 32 {
                format!("0x{}...", hex_lower(&b[..32]))
            } else {
                format!("0x{}", hex_lower(b))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn split_statements_respects_quotes_and_comments() {
        let sql = "SELECT 'a;b'; -- ignore; this\nSELECT 1;";
        assert_eq!(
            split_statements(sql),
            vec!["SELECT 'a;b'", "-- ignore; this\nSELECT 1"]
        );
    }
}
