//! Executing one action on a computer and rendering what the model reads back.

use std::fs;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::time::Duration;

use aci::envelope::Code;
use aci::json::Json;
use aci::manifest::validate_manifest;

use crate::computer::{self, Computer};
use crate::journal;
use crate::memory::{Kind, Store};
use crate::types::{Action, Error, Level, Observation, Result, RunConfig};

/// Largest file a `write` action accepts, in bytes.
const MAX_WRITE_BYTES: usize = 262_144;
/// Most bytes a `read` action returns.
const MAX_READ_BYTES: u64 = 65_536;
/// Most names a "files here" listing shows.
const MAX_LISTED: usize = 30;
/// Most elements of an array of objects that `render_data` prints.
const MAX_ROWS: usize = 20;

/// Everything an action needs.
#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    /// The computer being driven.
    pub computer: &'a Computer,
    /// The memory store in use (the computer's own, or a shared mind).
    pub mind: &'a Store,
    /// Run settings.
    pub cfg: &'a RunConfig,
    /// The session id written to the journal.
    pub session: &'a str,
}

/// The output of one tool run, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRun {
    /// Process exit code.
    pub exit: i32,
    /// Standard output (lossy UTF-8).
    pub stdout: String,
    /// Standard error (lossy UTF-8).
    pub stderr: String,
    /// True when the tool was killed for taking too long.
    pub timed_out: bool,
    /// Milliseconds the run took.
    pub ms: u64,
}

/// The verbs a model may use at `level`, in prompt order.
pub fn allowed_verbs(level: Level) -> &'static [&'static str] {
    match level {
        Level::Baseline => &["run", "write", "read", "done"],
        Level::Interface => &["run", "help", "write", "read", "done"],
        Level::Memory => &["run", "help", "write", "read", "remember", "recall", "done"],
    }
}

/// Runs one Monolith command line in `c` with `--json` as the first argument (unless the line
/// already starts with `--json` or `--describe`). Returns the exit code, stdout and stderr.
pub fn run_tool(c: &Computer, line: &str, timeout_secs: u64) -> Result<ToolRun> {
    let mut argv = computer::split_command_line(line)?;
    if argv.is_empty() {
        return Err(Error::new(Code::Usage, "no tool given")
            .hint("give a tool name, for example: chronos list"));
    }
    if !matches!(
        argv.get(1).map(String::as_str),
        Some("--json" | "--describe")
    ) {
        argv.insert(1, "--json".to_string());
    }
    let r = c.exec(&argv, Duration::from_secs(timeout_secs))?;
    Ok(ToolRun {
        exit: r.exit,
        stdout: String::from_utf8_lossy(&r.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&r.stderr).into_owned(),
        timed_out: r.timed_out,
        ms: r.ms,
    })
}

/// Runs one Monolith command line in `c`: split, fence check, add `--json` as the first
/// argument, execute. Returns the exit code and standard output as text (lossy UTF-8).
/// Used by `run` actions, suite setup and suite checks.
pub fn run_line(c: &Computer, line: &str, timeout_secs: u64) -> Result<(i32, String)> {
    let r = run_tool(c, line, timeout_secs)?;
    Ok((r.exit, r.stdout))
}

/// Executes one action and records it in the journal. Never fails: an error becomes an
/// observation with `ok: false` whose text says what went wrong and, from level 1, how to fix it.
pub fn execute(ctx: &Ctx<'_>, action: &Action) -> Observation {
    let mut data = vec![
        ("verb".to_string(), Json::Str(action.verb().into())),
        ("arg".to_string(), Json::Str(action.arg().into())),
    ];
    if let Action::Write { content, .. } = action {
        data.push(("content_bytes".into(), int(content.len())));
    }
    log(ctx, "action", Json::Obj(data));
    let mut obs = dispatch(ctx, action);
    obs.text = cut(obs.text, ctx.cfg.obs_cap_bytes);
    log(
        ctx,
        "observation",
        Json::Obj(vec![
            ("ok".into(), Json::Bool(obs.ok)),
            ("text".into(), Json::Str(obs.text.clone())),
        ]),
    );
    obs
}

/// The tool list for the system prompt: one line per tool, `name: summary`, read from each
/// tool's `--describe` (cached in the computer's folder).
pub fn tool_index(c: &Computer) -> Result<String> {
    let lines: Vec<String> = computer::TOOLS
        .iter()
        .filter_map(|name| {
            let m = manifest(c, name).ok()?;
            Some(format!("{name}: {}", text(&m, "summary")))
        })
        .collect();
    if lines.is_empty() {
        return Err(Error::new(
            Code::NotFound,
            format!("no Monolith tools found in {}", c.tools.display()),
        )
        .hint("put the tool binaries in that folder, or set MONOLITH_BIN"));
    }
    Ok(lines.join("\n"))
}

/// The manifest of `tool`, from its cache file or from `TOOL --describe` (which is then cached).
pub fn manifest(c: &Computer, tool: &str) -> Result<Json> {
    if !computer::TOOLS.contains(&tool) {
        return Err(
            Error::new(Code::NotFound, format!("no Monolith tool named `{tool}`"))
                .hint(format!("the tools are: {}", computer::TOOLS.join(", "))),
        );
    }
    let cache = c.dir.join("manifests").join(format!("{tool}.json"));
    if let Some(v) = fs::read_to_string(&cache)
        .ok()
        .and_then(|t| Json::parse(&t).ok())
    {
        return Ok(v);
    }
    let argv = [tool.to_string(), "--describe".to_string()];
    let r = c.exec(&argv, Duration::from_secs(10))?;
    if r.exit != 0 || r.timed_out {
        return Err(Error::new(
            Code::Io,
            format!("`{tool} --describe` failed with exit {}", r.exit),
        ));
    }
    let raw = String::from_utf8_lossy(&r.stdout);
    let v = Json::parse(raw.trim()).map_err(|e| {
        Error::new(
            Code::Invalid,
            format!("the manifest of `{tool}` is not JSON: {e}"),
        )
    })?;
    validate_manifest(&v).map_err(|e| {
        Error::new(
            Code::Invalid,
            format!("the manifest of `{tool}` is not valid: {e}"),
        )
    })?;
    if let Some(dir) = cache.parent() {
        // shortcut: a cache that cannot be written only costs one more --describe run.
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(&cache, v.write());
    Ok(v)
}

/// A compact card for `TOOL` (its commands, one line each) or `TOOL COMMAND` (arguments,
/// what it returns, an example), built from the tool's manifest.
pub fn help_card(c: &Computer, topic: &str) -> Result<String> {
    let words: Vec<&str> = topic.split_whitespace().collect();
    let Some((&tool, rest)) = words.split_first() else {
        return Err(Error::new(Code::Usage, "help needs a tool name").hint("try: help chronos"));
    };
    let m = manifest(c, tool)?;
    let summary = text(&m, "summary");
    let commands = arr(&m, "commands");
    match rest.first() {
        None => {
            let mut out = vec![format!("{tool}: {summary}")];
            for cmd in commands {
                let usage = usage_suffix(arr(cmd, "args"));
                out.push(format!(
                    "  {tool} {}{usage}: {}",
                    text(cmd, "name"),
                    text(cmd, "summary")
                ));
            }
            out.push(format!("Details: help {tool} COMMAND"));
            Ok(out.join("\n"))
        }
        Some(&name) => {
            let Some(cmd) = commands.iter().find(|c| text(c, "name") == name) else {
                let names: Vec<&str> = commands.iter().map(|c| text(c, "name")).collect();
                return Err(Error::new(
                    Code::NotFound,
                    format!("`{tool}` has no command `{name}`"),
                )
                .hint(format!("its commands are: {}", names.join(", "))));
            };
            let mut out = vec![
                format!("{tool} {name}: {}", text(cmd, "summary")),
                format!("usage: {tool} {name}{}", usage_suffix(arr(cmd, "args"))),
            ];
            for a in arr(cmd, "args") {
                out.push(format!("  {}", arg_line(a)));
            }
            out.push(format!("returns: {}", text(cmd, "returns")));
            out.push(format!("example: {}", text(cmd, "example")));
            Ok(out.join("\n"))
        }
    }
}

/// Renders a `--json` envelope's `data` for the model: one `key: value` line per field, strings
/// raw, arrays of objects one compact JSON line per element (at most 20).
pub fn render_data(data: &Json) -> String {
    match data {
        Json::Obj(pairs) => pairs
            .iter()
            .map(|(k, v)| render_field(k, v))
            .collect::<Vec<_>>()
            .join("\n"),
        Json::Str(s) => s.clone(),
        other => other.write(),
    }
}

/// Runs the action without journaling it, and without the output cap.
fn dispatch(ctx: &Ctx<'_>, action: &Action) -> Observation {
    let verb = action.verb();
    let allowed = allowed_verbs(ctx.cfg.level);
    if !allowed.contains(&verb) {
        return bad(format!(
            "action `{verb}` is not available; use one of: {}",
            allowed.join(", ")
        ));
    }
    let c = ctx.computer;
    let shaped = ctx.cfg.level >= Level::Interface;
    match action {
        Action::Run { line } => run(ctx, line, shaped),
        Action::Help { topic } => match help_card(c, topic) {
            Ok(card) => good(card),
            Err(e) => bad(error_text(&e, true)),
        },
        Action::Write { path, content } => write(c, path, content, shaped),
        Action::Read { path } => read(c, path, shaped),
        Action::Remember { text } => {
            match Store::at(&c.dir.join("memory")).add(Kind::Fact, text, false) {
                Ok(id) => good(format!("remembered #{id}")),
                Err(e) => bad(error_text(&e, shaped)),
            }
        }
        Action::Recall { query } => match recall(ctx, query) {
            Ok(found) => good(found),
            Err(e) => bad(error_text(&e, shaped)),
        },
        Action::Done { .. } => good("done"),
    }
}

/// The `run` action: the tool's output, plain or shaped by its envelope.
fn run(ctx: &Ctx<'_>, line: &str, shaped: bool) -> Observation {
    let c = ctx.computer;
    let timeout = ctx.cfg.tool_timeout_secs;
    let r = match run_tool(c, line, timeout) {
        Ok(r) => r,
        Err(e) => return bad(error_text(&e, shaped)),
    };
    if r.timed_out {
        return bad(format!("timed out after {timeout} s"));
    }
    if shaped {
        if let Some(mut obs) = shaped_run(c, line, &r) {
            obs.text = hide_root(c, &obs.text);
            return obs;
        }
    }
    baseline_run(&r)
}

/// The host path of the computer's root shown as `.`: results then name only paths the model
/// can use (an absolute host path would be refused by the fence).
fn hide_root(c: &Computer, text: &str) -> String {
    let root = c.root().display().to_string();
    let mut out = text.to_string();
    for form in [
        root.clone(),
        root.replace("\\", "/"),
        root.replace("/", "\\"),
    ] {
        out = out.replace(&form, ".");
    }
    out
}

/// Raw output: stdout (or stderr when stdout is empty), then `exit N` when the code is not 0.
fn baseline_run(r: &ToolRun) -> Observation {
    let out = r.stdout.trim();
    let text = if out.is_empty() {
        r.stderr.trim().to_string()
    } else {
        out.to_string()
    };
    let text = if r.exit == 0 {
        text
    } else if text.is_empty() {
        format!("exit {}", r.exit)
    } else {
        format!("{text}\nexit {}", r.exit)
    };
    Observation {
        ok: r.exit == 0 && !r.timed_out,
        text,
    }
}

/// The envelope read as a readable result, or `None` when stdout is not an envelope.
fn shaped_run(c: &Computer, line: &str, r: &ToolRun) -> Option<Observation> {
    let env = Json::parse(r.stdout.trim()).ok()?;
    if env.get("ok").and_then(Json::as_bool)? {
        let data = env.get("data").cloned().unwrap_or(Json::Null);
        return Some(good(format!("ok\n{}", render_data(&data))));
    }
    let err = env.get("error")?;
    let code = text(err, "code");
    let mut lines = vec![format!("error {code}: {}", text(err, "message"))];
    if let Some(h) = err.get("hint").and_then(Json::as_str) {
        lines.push(format!("hint: {h}"));
    }
    if code == "usage" || code == "invalid" {
        if let Some(card) = card_for_line(c, line) {
            lines.push(card);
        }
    }
    Some(bad(lines.join("\n")))
}

/// The help card of the command in `line` (the tool card when there is no such command).
fn card_for_line(c: &Computer, line: &str) -> Option<String> {
    let words = computer::split_command_line(line).ok()?;
    let (tool, rest) = words.split_first()?;
    let rest = match rest.first().map(String::as_str) {
        Some("--json") => &rest[1..],
        _ => rest,
    };
    match rest.first() {
        Some(cmd) => help_card(c, &format!("{tool} {cmd}"))
            .or_else(|_| help_card(c, tool))
            .ok(),
        None => help_card(c, tool).ok(),
    }
}

/// The `write` action: refuses large or outside paths, creates the folders, writes the file.
fn write(c: &Computer, path: &str, content: &str, shaped: bool) -> Observation {
    if content.len() > MAX_WRITE_BYTES {
        return bad(format!(
            "the file is {} bytes; the limit is {MAX_WRITE_BYTES} bytes",
            content.len()
        ));
    }
    let target = match c.resolve(path) {
        Ok(p) => p,
        Err(e) => return bad(error_text(&e, shaped)),
    };
    if let Some(parent) = target.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return bad(format!("error: cannot create the folder: {e}"));
        }
    }
    match fs::write(&target, content) {
        Ok(()) => good(format!("wrote {} bytes to {path}", content.len())),
        Err(e) => bad(format!("error: cannot write {path}: {e}")),
    }
}

/// The `read` action: at most 64 KiB, lossy UTF-8; a missing file lists the folder at Interface.
fn read(c: &Computer, path: &str, shaped: bool) -> Observation {
    let target = match c.resolve(path) {
        Ok(p) => p,
        Err(e) => return bad(error_text(&e, shaped)),
    };
    let file = match fs::File::open(&target) {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let mut text = format!("no such file: {path}");
            if shaped {
                if let Some(here) = files_here(c, &target) {
                    text = format!("{text}\nfiles here: {here}");
                }
            }
            return bad(text);
        }
        Err(e) => return bad(format!("error: cannot open {path}: {e}")),
    };
    let mut buf = Vec::new();
    match file.take(MAX_READ_BYTES).read_to_end(&mut buf) {
        Ok(_) => good(String::from_utf8_lossy(&buf).into_owned()),
        Err(e) => bad(format!("error: cannot read {path}: {e}")),
    }
}

/// The names in the nearest existing folder above `target`, inside the computer's root.
fn files_here(c: &Computer, target: &Path) -> Option<String> {
    let root = c.root();
    let mut dir = target.parent();
    while let Some(d) = dir {
        if d.is_dir() {
            let mut names: Vec<String> = fs::read_dir(d)
                .ok()?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names.truncate(MAX_LISTED);
            return Some(if names.is_empty() {
                "(none)".to_string()
            } else {
                names.join(", ")
            });
        }
        if d == root {
            break;
        }
        dir = d.parent();
    }
    None
}

/// The `recall` action: the computer's own store, then the shared mind, without duplicates.
fn recall(ctx: &Ctx<'_>, query: &str) -> Result<String> {
    let kinds = [Kind::Fact, Kind::Lesson, Kind::Skill];
    let own = Store::at(&ctx.computer.dir.join("memory"));
    let mut items = own.search(query, 5, &kinds, false)?;
    for item in ctx.mind.search(query, 5, &kinds, false)? {
        if !items
            .iter()
            .any(|x| x.kind == item.kind && x.text == item.text)
        {
            items.push(item);
        }
    }
    if items.is_empty() {
        return Ok(format!("nothing in memory matches: {query}"));
    }
    Ok(items
        .iter()
        .map(|i| format!("[{}] {}", i.kind.as_str(), i.text))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// One error as observation text: `error: MESSAGE`, plus `hint: HINT` when shaped.
fn error_text(e: &Error, shaped: bool) -> String {
    match (&e.hint, shaped) {
        (Some(h), true) => format!("error: {}\nhint: {h}", e.message),
        _ => format!("error: {}", e.message),
    }
}

/// Cuts `text` to `cap` bytes on a char boundary and says how much was left out.
fn cut(text: String, cap: usize) -> String {
    if text.len() <= cap {
        return text;
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[cut: {} more bytes]", &text[..end], text.len() - end)
}

/// One `key: value` field (several lines for a multi-line string or an array of objects).
fn render_field(key: &str, v: &Json) -> String {
    match v {
        Json::Str(s) if s.contains('\n') => {
            let body: Vec<String> = s
                .trim_end_matches('\n')
                .lines()
                .map(|l| format!("  {l}"))
                .collect();
            format!("{key}:\n{}", body.join("\n"))
        }
        Json::Str(s) => format!("{key}: {s}"),
        Json::Arr(items)
            if !items.is_empty() && items.iter().all(|i| matches!(i, Json::Obj(_))) =>
        {
            let mut lines = vec![format!("{key}:")];
            lines.extend(items.iter().take(MAX_ROWS).map(Json::write));
            if items.len() > MAX_ROWS {
                lines.push(format!("... {} more", items.len() - MAX_ROWS));
            }
            lines.join("\n")
        }
        other => format!("{key}: {}", other.write()),
    }
}

/// `" <NAME> [--flag]"`-style usage text for a command's arguments (empty when none).
fn usage_suffix(args: &[Json]) -> String {
    let tokens: Vec<String> = args.iter().map(usage_token).collect();
    if tokens.is_empty() {
        String::new()
    } else {
        format!(" {}", tokens.join(" "))
    }
}

/// One argument in a usage line.
fn usage_token(a: &Json) -> String {
    let name = text(a, "name");
    let required = flag(a, "required");
    if name.starts_with('-') {
        if text(a, "kind") == "flag" {
            return format!("[{name}]");
        }
        return if required {
            format!("{name} VALUE")
        } else {
            format!("[{name} VALUE]")
        };
    }
    if required {
        format!("<{}>", name.to_uppercase())
    } else {
        format!("[{}]", name.to_uppercase())
    }
}

/// One argument in a command card: `NAME (kind, required, default D, one of a|b)`.
fn arg_line(a: &Json) -> String {
    let mut facts = vec![
        text(a, "kind").to_string(),
        if flag(a, "required") {
            "required".to_string()
        } else {
            "optional".to_string()
        },
    ];
    if let Some(d) = a.get("default") {
        facts.push(format!("default {}", plain(d)));
    }
    let choices = arr(a, "choices");
    if !choices.is_empty() {
        let opts: Vec<String> = choices.iter().map(plain).collect();
        facts.push(format!("one of {}", opts.join("|")));
    }
    format!("{} ({})", text(a, "name"), facts.join(", "))
}

/// A JSON value as plain text: strings raw, anything else compact.
fn plain(v: &Json) -> String {
    match v {
        Json::Str(s) => s.clone(),
        other => other.write(),
    }
}

/// A string field of an object, or `""`.
fn text<'a>(v: &'a Json, key: &str) -> &'a str {
    v.get(key).and_then(Json::as_str).unwrap_or("")
}

/// A boolean field of an object, or `false`.
fn flag(v: &Json, key: &str) -> bool {
    v.get(key).and_then(Json::as_bool).unwrap_or(false)
}

/// An array field of an object, or an empty list.
fn arr<'a>(v: &'a Json, key: &str) -> &'a [Json] {
    v.get(key).and_then(Json::as_array).unwrap_or(&[])
}

/// A whole number as JSON (saturating at `i64::MAX`).
fn int<T: TryInto<i64>>(v: T) -> Json {
    Json::Int(v.try_into().unwrap_or(i64::MAX))
}

/// Adds one entry to the journal. A journal failure never stops an action.
fn log(ctx: &Ctx<'_>, kind: &str, data: Json) {
    let _ = journal::append(ctx.computer, ctx.session, kind, data);
}

/// A successful observation.
fn good(text: impl Into<String>) -> Observation {
    Observation {
        ok: true,
        text: text.into(),
    }
}

/// A failed observation.
fn bad(text: impl Into<String>) -> Observation {
    Observation {
        ok: false,
        text: text.into(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn hide_root_covers_both_slash_styles() {
        let c = Computer {
            name: "c1".into(),
            dir: std::path::PathBuf::from("C:/h/computers/c1"),
            tools: std::path::PathBuf::from("C:/h/bin"),
        };
        let root = c.root().display().to_string();
        let text = format!(
            "a {} b {}/x",
            root.replace("/", "\\"),
            root.replace("\\", "/")
        );
        assert_eq!(hide_root(&c, &text), "a . b ./x");
    }

    #[test]
    fn cut_keeps_char_boundaries() {
        let s = "aé".repeat(10);
        let c = cut(s.clone(), 5);
        assert!(c.starts_with("aéa\n[cut:"), "{c}");
        assert!(c.ends_with("more bytes]"));
        assert_eq!(cut(s.clone(), 1000), s);
    }

    #[test]
    fn render_lists_rows_and_strings() {
        let d = Json::obj([
            ("note", Json::Str("one\ntwo".into())),
            (
                "items",
                Json::Arr(vec![Json::obj([("n", Json::Int(1))]); 21]),
            ),
        ]);
        let text = render_data(&d);
        assert!(text.starts_with("note:\n  one\n  two\nitems:\n{\"n\":1}"));
        assert!(text.ends_with("... 1 more"));
    }
}
