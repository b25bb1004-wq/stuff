//! The envelope every `--json` run prints, plus paging, the text cap and the flag helpers (ACI.md, "Envelope").

use crate::json::{lossy, obj_vec, Json};
use std::ffi::OsString;

/// Text values are cut at this many bytes.
pub const TEXT_CAP: usize = 16384;
/// Page size when `--limit` is not given.
pub const DEFAULT_LIMIT: usize = 100;
/// Largest page size `--limit` may ask for.
pub const MAX_LIMIT: usize = 1000;

/// The error code of a failed envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// Bad arguments or an unknown command.
    Usage,
    /// The named thing does not exist.
    NotFound,
    /// The thing already exists.
    Exists,
    /// The input is wrong.
    Invalid,
    /// Another process holds it.
    Locked,
    /// Permission was refused.
    Denied,
    /// A read or write failed.
    Io,
    /// The request is not supported here.
    Unsupported,
    /// The change conflicts with the current state.
    Conflict,
    /// A check or challenge must be answered first.
    Challenge,
    /// A bug in the tool.
    Internal,
}

/// Every error code, in ACI.md order.
const ALL_CODES: [Code; 11] = [
    Code::Usage,
    Code::NotFound,
    Code::Exists,
    Code::Invalid,
    Code::Locked,
    Code::Denied,
    Code::Io,
    Code::Unsupported,
    Code::Conflict,
    Code::Challenge,
    Code::Internal,
];

impl Code {
    /// The spelling used in the envelope.
    pub fn as_str(&self) -> &'static str {
        match self {
            Code::Usage => "usage",
            Code::NotFound => "not_found",
            Code::Exists => "exists",
            Code::Invalid => "invalid",
            Code::Locked => "locked",
            Code::Denied => "denied",
            Code::Io => "io",
            Code::Unsupported => "unsupported",
            Code::Conflict => "conflict",
            Code::Challenge => "challenge",
            Code::Internal => "internal",
        }
    }
}

/// A success line: `{"ok":true,"tool":..,"cmd":..,"data":..}` and a newline.
pub fn ok(tool: &str, cmd: &str, data: Json) -> String {
    let mut line = Json::obj([
        ("ok", Json::Bool(true)),
        ("tool", Json::Str(tool.into())),
        ("cmd", Json::Str(cmd.into())),
        ("data", data),
    ])
    .write();
    line.push('\n');
    line
}

/// A failure line with `code`, `message` and, when given, a `hint`, and a newline.
pub fn err(tool: &str, cmd: &str, code: Code, message: &str, hint: Option<&str>) -> String {
    let mut error = vec![
        ("code", Json::Str(code.as_str().into())),
        ("message", Json::Str(message.into())),
    ];
    if let Some(h) = hint {
        error.push(("hint", Json::Str(h.into())));
    }
    let mut line = Json::obj([
        ("ok", Json::Bool(false)),
        ("tool", Json::Str(tool.into())),
        ("cmd", Json::Str(cmd.into())),
        ("error", obj_vec(error)),
    ])
    .write();
    line.push('\n');
    line
}

/// One page of a list. `items` is already sliced; `next_offset` appears only when `more` is true.
pub fn page(items: Vec<Json>, total: usize, offset: usize, limit: usize) -> Json {
    let next = offset.saturating_add(limit.max(1));
    let more = next < total;
    let mut pairs = vec![
        ("items", Json::Arr(items)),
        ("total", int(total)),
        ("more", Json::Bool(more)),
    ];
    if more {
        pairs.push(("next_offset", int(next)));
    }
    obj_vec(pairs)
}

fn int(n: usize) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

/// Text fields for a byte value, cut at `TEXT_CAP`. A cut value also gets `truncated` and the full `bytes`;
/// invalid UTF-8 gets `lossy`.
pub fn capped(bytes: &[u8]) -> Vec<(String, Json)> {
    let (mut text, is_lossy) = lossy(bytes);
    let truncated = text.len() > TEXT_CAP;
    if truncated {
        let mut cut = TEXT_CAP;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    let mut fields = vec![("text".to_string(), Json::Str(text))];
    if truncated {
        fields.push(("truncated".to_string(), Json::Bool(true)));
        fields.push(("bytes".to_string(), int(bytes.len())));
    }
    if is_lossy {
        fields.push(("lossy".to_string(), Json::Bool(true)));
    }
    fields
}

/// Remove a leading `--json` and report whether it was there. A `--json` later on is left alone.
pub fn take_json_flag(args: &mut Vec<OsString>) -> bool {
    if args.first().and_then(|a| a.to_str()) == Some("--json") {
        args.remove(0);
        true
    } else {
        false
    }
}

/// True when the arguments are exactly `--describe`.
pub fn take_describe_flag(args: &[OsString]) -> bool {
    matches!(args, [only] if only.to_str() == Some("--describe"))
}

/// Check an envelope read from JSON. The first problem found is the error.
pub fn validate_envelope(e: &Json) -> Result<(), String> {
    if !matches!(e, Json::Obj(_)) {
        return Err("envelope is not a JSON object".into());
    }
    let ok = e
        .get("ok")
        .and_then(Json::as_bool)
        .ok_or_else(|| "\"ok\" must be a boolean".to_string())?;
    for key in ["tool", "cmd"] {
        text_field(e, key)?;
    }
    if ok {
        e.get("data")
            .ok_or_else(|| "success needs \"data\"".to_string())?;
        return Ok(());
    }
    let error = e
        .get("error")
        .ok_or_else(|| "failure needs \"error\"".to_string())?;
    if !matches!(error, Json::Obj(_)) {
        return Err("\"error\" must be an object".into());
    }
    let code = text_field(error, "code")?;
    if !ALL_CODES.iter().any(|c| c.as_str() == code) {
        return Err(format!("unknown error code \"{code}\""));
    }
    text_field(error, "message")?;
    if error.get("hint").is_some() {
        text_field(error, "hint")?;
    }
    Ok(())
}

fn text_field<'a>(o: &'a Json, key: &str) -> Result<&'a str, String> {
    o.get(key)
        .ok_or_else(|| format!("missing \"{key}\""))?
        .as_str()
        .ok_or_else(|| format!("\"{key}\" must be a string"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::json::Json;
    use std::ffi::OsString;

    #[test]
    fn written_envelopes_validate() {
        let good = ok("hades", "ls", Json::obj([("items", Json::Arr(vec![]))]));
        assert!(good.ends_with('\n'));
        let parsed = Json::parse(good.trim_end()).unwrap();
        assert_eq!(validate_envelope(&parsed), Ok(()));

        let hinted = err(
            "hades",
            "ls",
            Code::NotFound,
            "no such path: /x",
            Some("run: hades --json ls disk.img /"),
        );
        assert_eq!(
            validate_envelope(&Json::parse(hinted.trim_end()).unwrap()),
            Ok(())
        );
        let bare = err("aci", "", Code::Usage, "unknown command", None);
        assert_eq!(
            validate_envelope(&Json::parse(bare.trim_end()).unwrap()),
            Ok(())
        );
    }

    #[test]
    fn validate_envelope_rejects() {
        let bad = [
            r#"{"ok":true,"tool":"t","cmd":"c"}"#,
            r#"{"ok":false,"tool":"t","cmd":"c","error":{"code":"oops","message":"m"}}"#,
            r#"[1,2]"#,
            r#"{"ok":false,"tool":"t","cmd":"c","error":{"code":"usage"}}"#,
        ];
        for text in bad {
            let value = Json::parse(text).unwrap();
            assert!(validate_envelope(&value).is_err(), "accepted: {text}");
        }
        let good = Json::parse(r#"{"ok":true,"tool":"t","cmd":"c","data":{}}"#).unwrap();
        assert_eq!(validate_envelope(&good), Ok(()));
    }

    #[test]
    fn capped_truncates_at_text_cap() {
        let big = vec![b'x'; 20000];
        let obj = Json::Obj(capped(&big));
        assert_eq!(
            obj.get("text").and_then(Json::as_str).map(str::len),
            Some(TEXT_CAP)
        );
        assert_eq!(obj.get("truncated"), Some(&Json::Bool(true)));
        assert_eq!(obj.get("bytes"), Some(&Json::Int(20000)));
        assert_eq!(obj.get("lossy"), None);
    }

    #[test]
    fn capped_leaves_small_text_alone() {
        assert_eq!(
            capped(b"hi"),
            vec![("text".to_string(), Json::Str("hi".into()))]
        );
        let bad = Json::Obj(capped(&[0x61, 0xff]));
        assert_eq!(bad.get("lossy"), Some(&Json::Bool(true)));
    }

    #[test]
    fn page_sets_more_and_next_offset() {
        let first = page(vec![Json::Int(0); 100], 250, 100, 100);
        assert_eq!(first.get("more"), Some(&Json::Bool(true)));
        assert_eq!(first.get("next_offset"), Some(&Json::Int(200)));
        let last = page(vec![Json::Int(0); 50], 250, 200, 100);
        assert_eq!(last.get("more"), Some(&Json::Bool(false)));
        assert_eq!(last.get("next_offset"), None);
    }

    #[test]
    fn take_json_flag_only_leading() {
        let mut a: Vec<OsString> = vec!["--json".into(), "ls".into()];
        assert!(take_json_flag(&mut a));
        assert_eq!(a, vec![OsString::from("ls")]);

        let mut b: Vec<OsString> = vec!["ls".into()];
        assert!(!take_json_flag(&mut b));
        assert_eq!(b, vec![OsString::from("ls")]);

        let mut c: Vec<OsString> = vec!["ls".into(), "--json".into()];
        assert!(!take_json_flag(&mut c));
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn take_describe_flag_needs_only_that_argument() {
        assert!(take_describe_flag(&[OsString::from("--describe")]));
        assert!(!take_describe_flag(&[
            OsString::from("--describe"),
            OsString::from("x")
        ]));
        assert!(!take_describe_flag(&[]));
    }
}
