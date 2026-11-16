//! The manifest a tool prints for `--describe`: its commands, arguments and effects (ACI.md, "Manifest").

use crate::json::{obj_vec, Json};

const EFFECTS: [&str; 4] = ["read", "write", "net", "exec"];
const KINDS: [&str; 5] = ["path", "text", "int", "flag", "choice"];

/// The kind of value an argument takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A file or directory path.
    Path,
    /// Free text.
    Text,
    /// A whole number.
    Int,
    /// A switch with no value.
    Flag,
    /// One of the listed `choices`.
    Choice,
}

impl Kind {
    /// The spelling used in the manifest.
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Path => "path",
            Kind::Text => "text",
            Kind::Int => "int",
            Kind::Flag => "flag",
            Kind::Choice => "choice",
        }
    }
}

/// What a command does to the world. Pick the strongest that applies: exec > net > write > read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Changes nothing.
    Read,
    /// Changes files or data.
    Write,
    /// Uses the network.
    Net,
    /// Starts other programs.
    Exec,
}

impl Effect {
    /// The spelling used in the manifest.
    pub fn as_str(&self) -> &'static str {
        match self {
            Effect::Read => "read",
            Effect::Write => "write",
            Effect::Net => "net",
            Effect::Exec => "exec",
        }
    }
}

/// One argument of a command.
pub struct Arg {
    /// Name as written in usage, such as `IMG` or `--limit`.
    pub name: &'static str,
    /// The kind of value it takes.
    pub kind: Kind,
    /// Whether the command fails without it.
    pub required: bool,
    /// The default, written as text.
    pub default: Option<&'static str>,
    /// The allowed values; written only for a `Choice` argument.
    pub choices: &'static [&'static str],
}

/// One subcommand of a tool.
pub struct Command {
    /// The subcommand name.
    pub name: &'static str,
    /// One line on what it does.
    pub summary: &'static str,
    /// What it does to the world.
    pub effect: Effect,
    /// Its arguments.
    pub args: &'static [Arg],
    /// A short description of the data it returns.
    pub returns: &'static str,
    /// A command line that really runs.
    pub example: &'static str,
}

/// A tool's manifest: everything `--describe` prints.
pub struct Manifest {
    /// The tool name.
    pub tool: &'static str,
    /// The tool version.
    pub version: &'static str,
    /// One line on what the tool is.
    pub summary: &'static str,
    /// Every subcommand.
    pub commands: &'static [Command],
}

impl Manifest {
    /// The manifest as JSON, with `"aci": 1`.
    pub fn to_json(&self) -> Json {
        let commands = self.commands.iter().map(command_json).collect();
        obj_vec(vec![
            ("aci", Json::Int(1)),
            ("tool", Json::Str(self.tool.into())),
            ("version", Json::Str(self.version.into())),
            ("summary", Json::Str(self.summary.into())),
            ("commands", Json::Arr(commands)),
        ])
    }
}

fn command_json(c: &Command) -> Json {
    let args = c.args.iter().map(arg_json).collect();
    obj_vec(vec![
        ("name", Json::Str(c.name.into())),
        ("summary", Json::Str(c.summary.into())),
        ("effect", Json::Str(c.effect.as_str().into())),
        ("args", Json::Arr(args)),
        ("returns", Json::Str(c.returns.into())),
        ("example", Json::Str(c.example.into())),
    ])
}

fn arg_json(a: &Arg) -> Json {
    let mut pairs = vec![
        ("name", Json::Str(a.name.into())),
        ("kind", Json::Str(a.kind.as_str().into())),
        ("required", Json::Bool(a.required)),
    ];
    if let Some(d) = a.default {
        pairs.push(("default", Json::Str(d.into())));
    }
    if a.kind == Kind::Choice {
        let choices = a.choices.iter().map(|c| Json::Str((*c).into())).collect();
        pairs.push(("choices", Json::Arr(choices)));
    }
    obj_vec(pairs)
}

/// Check a manifest read from JSON. The first problem found is the error.
pub fn validate_manifest(m: &Json) -> Result<(), String> {
    if !matches!(m, Json::Obj(_)) {
        return Err("manifest is not a JSON object".into());
    }
    if m.get("aci").and_then(Json::as_i64) != Some(1) {
        return Err("\"aci\" must be 1".into());
    }
    for key in ["tool", "version", "summary"] {
        text_field(m, key)?;
    }
    for (i, c) in array_field(m, "commands")?.iter().enumerate() {
        validate_command(c).map_err(|e| format!("commands[{i}]: {e}"))?;
    }
    Ok(())
}

fn validate_command(c: &Json) -> Result<(), String> {
    if !matches!(c, Json::Obj(_)) {
        return Err("command is not a JSON object".into());
    }
    text_field(c, "name")?;
    text_field(c, "summary")?;
    let effect = text_field(c, "effect")?;
    if !EFFECTS.contains(&effect) {
        return Err(format!("unknown effect \"{effect}\""));
    }
    for arg in array_field(c, "args")? {
        validate_arg(arg)?;
    }
    text_field(c, "returns")?;
    text_field(c, "example")?;
    Ok(())
}

fn validate_arg(a: &Json) -> Result<(), String> {
    if !matches!(a, Json::Obj(_)) {
        return Err("argument is not a JSON object".into());
    }
    let name = text_field(a, "name")?;
    let kind = text_field(a, "kind")?;
    if !KINDS.contains(&kind) {
        return Err(format!("{name}: unknown kind \"{kind}\""));
    }
    if field(a, "required")?.as_bool().is_none() {
        return Err(format!("{name}: \"required\" must be a boolean"));
    }
    if a.get("default").is_some() {
        text_field(a, "default").map_err(|e| format!("{name}: {e}"))?;
    }
    if kind == "choice" {
        let choices = array_field(a, "choices").map_err(|e| format!("{name}: {e}"))?;
        if choices.iter().any(|c| c.as_str().is_none()) {
            return Err(format!("{name}: \"choices\" must be strings"));
        }
    }
    Ok(())
}

fn field<'a>(o: &'a Json, key: &str) -> Result<&'a Json, String> {
    o.get(key).ok_or_else(|| format!("missing \"{key}\""))
}

fn text_field<'a>(o: &'a Json, key: &str) -> Result<&'a str, String> {
    field(o, key)?
        .as_str()
        .ok_or_else(|| format!("\"{key}\" must be a string"))
}

fn array_field<'a>(o: &'a Json, key: &str) -> Result<&'a [Json], String> {
    field(o, key)?
        .as_array()
        .ok_or_else(|| format!("\"{key}\" must be an array"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::json::Json;

    const ARGS: &[Arg] = &[
        Arg {
            name: "IMG",
            kind: Kind::Path,
            required: true,
            default: None,
            choices: &[],
        },
        Arg {
            name: "--mode",
            kind: Kind::Choice,
            required: false,
            default: Some("a"),
            choices: &["a", "b"],
        },
    ];
    const COMMANDS: &[Command] = &[Command {
        name: "ls",
        summary: "List entries",
        effect: Effect::Read,
        args: ARGS,
        returns: "items: [{name, kind, size}]",
        example: "t --json ls disk.img",
    }];
    const MANIFEST: Manifest = Manifest {
        tool: "t",
        version: "0.1.0",
        summary: "A test tool",
        commands: COMMANDS,
    };

    const BASE: &str = r#"{"aci":1,"tool":"t","version":"0.1.0","summary":"s","commands":[{"name":"ls","summary":"s","effect":"read","args":[],"returns":"r","example":"t --json ls"}]}"#;

    #[test]
    fn to_json_passes_validation() {
        assert_eq!(validate_manifest(&MANIFEST.to_json()), Ok(()));
        assert_eq!(validate_manifest(&Json::parse(BASE).unwrap()), Ok(()));
    }

    #[test]
    fn validate_manifest_rejects() {
        let bad = [
            BASE.replace("\"commands\":", "\"cmds\":"),
            BASE.replace("\"effect\":\"read\"", "\"effect\":\"delete\""),
            BASE.replace(
                "\"args\":[]",
                "\"args\":[{\"name\":\"X\",\"kind\":\"choice\",\"required\":true}]",
            ),
            BASE.replace("\"aci\":1", "\"aci\":2"),
        ];
        for text in bad {
            let value = Json::parse(&text).unwrap();
            assert!(validate_manifest(&value).is_err(), "accepted: {text}");
        }
    }
}
