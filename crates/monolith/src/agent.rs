//! The agent loop: build the prompt, ask the model for one action, run it, repeat.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aci::envelope::Code;
use aci::json::Json;

use crate::actions::{self, allowed_verbs, Ctx};
use crate::computer::{self, Computer};
use crate::journal;
use crate::llm::{self, ChatRequest, Message, Reply, Role};
use crate::memory::{Item, Kind, Store};
use crate::types::{Action, CmdOut, Error, Level, Outcome, Result, RunConfig, Stop};

/// Most characters of all message contents before the context is cut.
const CONTEXT_LIMIT: usize = 30_000;
/// How many of the newest `Result:` messages keep their full text.
const KEEP_RESULTS: usize = 4;
/// How many bytes of an older `Result:` message are kept.
const CUT_RESULT_BYTES: usize = 300;
/// Suffix of a cut `Result:` message (also how a cut one is recognised).
const CUT_MARK: &str = " [older result cut]";
/// Repeats of the same action before the harness says so.
const REPEAT_LIMIT: u32 = 3;
/// Seconds a model call may take.
const MODEL_TIMEOUT_SECS: u64 = 300;

/// The action key used to spot repeats: verb, argument, content.
type Key = (String, String, String);

/// The JSON schema the model's reply must follow at `level`.
pub fn action_schema(level: Level) -> Json {
    let verbs: Vec<Json> = allowed_verbs(level)
        .iter()
        .map(|v| Json::Str((*v).into()))
        .collect();
    Json::obj([
        ("type", js("object")),
        (
            "properties",
            Json::obj([
                (
                    "thought",
                    Json::obj([("type", js("string")), ("maxLength", Json::Int(200))]),
                ),
                (
                    "action",
                    Json::obj([("type", js("string")), ("enum", Json::Arr(verbs))]),
                ),
                ("arg", Json::obj([("type", js("string"))])),
                ("content", Json::obj([("type", js("string"))])),
            ]),
        ),
        (
            "required",
            Json::Arr(vec![js("thought"), js("action"), js("arg")]),
        ),
    ])
}

/// The system prompt for this context (level, tool list, lessons for `task`).
pub fn system_prompt(ctx: &Ctx<'_>, task: &str) -> String {
    let level = ctx.cfg.level;
    let verbs = allowed_verbs(level);
    let mut sections = vec![
        "You are an AI with your own computer. It holds only the Monolith tools listed below: there is no shell, no Python and no internet. Work in small steps, one action per reply, and look at each result before the next step.".to_string(),
        format!(
            "Reply with ONE JSON object and nothing else: {{\"thought\": \"<under 25 words>\", \"action\": \"{}\", \"arg\": \"...\", \"content\": \"...\"}}",
            verbs.join("|")
        ),
    ];
    let mut actions_text = vec!["Actions:".to_string()];
    actions_text.extend(verbs.iter().map(|v| action_help(v).to_string()));
    sections.push(actions_text.join("\n"));
    let index = actions::tool_index(ctx.computer).unwrap_or_else(|_| "(no tools found)".into());
    sections.push(format!("Tools:\n{index}"));
    if level >= Level::Interface {
        sections.push(EXAMPLE.to_string());
    }
    if level >= Level::Memory {
        let mut lessons = ctx.cfg.extra_lessons.clone();
        lessons.extend(texts(ctx.mind.search(task, 6, &[Kind::Lesson], true)));
        lessons.truncate(6);
        push_list(
            &mut sections,
            "Lessons from earlier work (checked, follow them):",
            &lessons,
        );
        let skills = texts(ctx.mind.search(task, 3, &[Kind::Skill], true));
        push_list(&mut sections, "Skills that worked before:", &skills);
        let own = Store::at(&ctx.computer.dir.join("memory"));
        let facts = texts(own.search(task, 5, &[Kind::Fact], false));
        push_list(
            &mut sections,
            "What you remembered on this computer:",
            &facts,
        );
    }
    sections.join("\n\n")
}

/// Lets the model work on `task` until it says `done`, the step limit is reached or the model
/// server fails. Every turn is journaled.
pub fn run_task(ctx: &Ctx<'_>, task: &str) -> Outcome {
    let cfg = ctx.cfg;
    let level = cfg.level;
    let mut out = Outcome {
        answer: None,
        stop: Stop::StepLimit,
        steps: 0,
        invalid_actions: 0,
        failed_actions: 0,
        good_commands: Vec::new(),
        prompt_tokens: 0,
        completion_tokens: 0,
        model_ms: 0,
    };
    log(
        ctx,
        "task",
        Json::obj([
            ("prompt", js(task)),
            ("level", Json::Int(i64::from(level.number()))),
        ]),
    );
    let schema = action_schema(level);
    let mut msgs = vec![
        Message {
            role: Role::System,
            content: system_prompt(ctx, task),
        },
        Message {
            role: Role::User,
            content: format!("Task: {task}"),
        },
    ];
    let mut last: Option<Key> = None;
    let mut streak = 0u32;
    let stop = loop {
        if out.steps >= cfg.max_steps {
            break Stop::StepLimit;
        }
        compact(&mut msgs, level);
        let turn = out.steps + 1;
        let reply = match ask(ctx, &msgs, &schema, turn) {
            Ok(r) => r,
            Err(message) => break Stop::ModelError(message),
        };
        out.steps = turn;
        out.prompt_tokens += reply.prompt_tokens;
        out.completion_tokens += reply.completion_tokens;
        out.model_ms += reply.ms;
        log(
            ctx,
            "model",
            Json::obj([
                ("content", js(&reply.content)),
                ("prompt_tokens", int(reply.prompt_tokens)),
                ("completion_tokens", int(reply.completion_tokens)),
                ("ms", int(reply.ms)),
            ]),
        );
        msgs.push(Message {
            role: Role::Assistant,
            content: reply.content.clone(),
        });
        let action = match parse_turn(&reply.content, level) {
            Ok(a) => a,
            Err(reason) => {
                out.invalid_actions += 1;
                last = None;
                streak = 0;
                msgs.push(Message {
                    role: Role::User,
                    content: format!(
                        "Invalid reply: {reason}. Reply with one JSON object: {{\"thought\": \"...\", \"action\": \"...\", \"arg\": \"...\"}}"
                    ),
                });
                continue;
            }
        };
        let key: Key = (
            action.verb().to_string(),
            action.arg().to_string(),
            content_of(&action).to_string(),
        );
        if last.as_ref() == Some(&key) {
            streak += 1;
        } else {
            streak = 1;
            last = Some(key);
        }
        let obs = actions::execute(ctx, &action);
        if !obs.ok {
            out.failed_actions += 1;
        }
        if let (true, Action::Run { line }) = (obs.ok, &action) {
            out.good_commands.push(line.clone());
        }
        if let Action::Done { answer } = &action {
            out.answer = Some(answer.clone());
            break Stop::Done;
        }
        let mut text = obs.text;
        if level >= Level::Interface && streak >= REPEAT_LIMIT {
            text.push_str(
                "\nYou have done exactly this 3 times with the same result. Do something different: use help, or finish with done.",
            );
        }
        msgs.push(Message {
            role: Role::User,
            content: format!("Result: {text}"),
        });
    };
    let stop_name = match &stop {
        Stop::Done => "done",
        Stop::StepLimit => "step_limit",
        Stop::ModelError(_) => "model_error",
    };
    log(
        ctx,
        "done",
        Json::obj([
            ("answer", out.answer.as_deref().map_or(Json::Null, js)),
            ("stop", js(stop_name)),
            ("steps", int(out.steps)),
        ]),
    );
    out.stop = stop;
    out
}

/// After a failed task: asks the model for one short lesson (what to do differently), given the
/// task and the session's journal. `None` if the model gives nothing usable.
pub fn reflect(ctx: &Ctx<'_>, task: &str, failed_checks: &[String]) -> Option<String> {
    let mut user = format!("Task: {task}\n\nSteps taken:\n");
    user.push_str(&session_steps(ctx).join("\n"));
    user.push_str("\n\nChecks that failed:\n");
    user.push_str(&failed_checks.join("\n"));
    let schema = Json::obj([
        ("type", js("object")),
        (
            "properties",
            Json::obj([(
                "lesson",
                Json::obj([("type", js("string")), ("maxLength", Json::Int(300))]),
            )]),
        ),
        ("required", Json::Arr(vec![js("lesson")])),
    ]);
    let msgs = [
        Message {
            role: Role::System,
            content: REFLECT_SYSTEM.to_string(),
        },
        Message {
            role: Role::User,
            content: user,
        },
    ];
    let cfg = ctx.cfg;
    let req = ChatRequest {
        url: &cfg.model_url,
        model: &cfg.model,
        messages: &msgs,
        max_tokens: 120,
        temperature: cfg.temperature,
        seed: cfg.seed,
        json_schema: Some(&schema),
        thinking: cfg.thinking,
        timeout: Duration::from_secs(MODEL_TIMEOUT_SECS),
    };
    let reply = llm::chat(&req).ok()?;
    let v = Json::parse(reply.content.trim()).ok()?;
    let lesson = v
        .get("lesson")
        .and_then(Json::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    log(ctx, "reflect", Json::obj([("lesson", js(&lesson))]));
    (lesson.chars().count() >= 15).then_some(lesson)
}

/// Parses the flags of `run` (all except `--task`) into `cfg`. Returns the arguments it did not
/// recognise, in order.
pub fn parse_run_flags(args: &[String], cfg: &mut RunConfig) -> Result<Vec<String>> {
    let mut left = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--level" => cfg.level = Level::parse(flag_value(args, &mut i)?)?,
            "--model-url" => cfg.model_url = flag_value(args, &mut i)?.to_string(),
            "--max-steps" => cfg.max_steps = number(flag_value(args, &mut i)?, "--max-steps")?,
            "--mind" => cfg.mind_dir = Some(PathBuf::from(flag_value(args, &mut i)?)),
            "--seed" => cfg.seed = number(flag_value(args, &mut i)?, "--seed")?,
            "--temp" => {
                let t: f64 = number(flag_value(args, &mut i)?, "--temp")?;
                if !t.is_finite() {
                    return Err(Error::new(Code::Usage, "--temp needs a finite number"));
                }
                cfg.temperature = t;
            }
            "--freeze" => cfg.freeze_memory = true,
            "--thinking" => cfg.thinking = true,
            _ => left.push(args[i].clone()),
        }
        i += 1;
    }
    Ok(left)
}

/// CLI: `run NAME --task TEXT [--level N] [--model-url URL] [--max-steps N] [--mind DIR]
/// [--freeze] [--seed N] [--temp X] [--thinking]`.
pub fn cmd(args: &[String]) -> Result<CmdOut> {
    let mut cfg = RunConfig::default();
    let left = parse_run_flags(args, &mut cfg)?;
    let mut name: Option<String> = None;
    let mut task: Option<String> = None;
    let mut i = 0;
    while i < left.len() {
        let word = left[i].as_str();
        if word == "--task" {
            task = Some(flag_value(&left, &mut i)?.to_string());
        } else if word.starts_with("--") {
            return Err(Error::new(Code::Usage, format!("unknown flag `{word}`")).hint(USAGE));
        } else if name.is_none() {
            name = Some(left[i].clone());
        } else {
            return Err(Error::new(Code::Usage, format!("unexpected `{word}`")).hint(USAGE));
        }
        i += 1;
    }
    let (Some(name), Some(task)) = (name, task) else {
        return Err(
            Error::new(Code::Usage, "run needs a computer name and --task TEXT").hint(USAGE),
        );
    };
    let c = Computer::open(&computer::home(), &name)?;
    let mind_dir = cfg.mind_dir.clone().unwrap_or_else(|| c.dir.join("memory"));
    let mind = Store::at(&mind_dir);
    let session = format!(
        "s{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis())
    );
    let ctx = Ctx {
        computer: &c,
        mind: &mind,
        cfg: &cfg,
        session: &session,
    };
    let out = run_task(&ctx, &task);
    Ok(report(&out, &session))
}

/// Usage hint for `run`.
const USAGE: &str = "run NAME --task TEXT [--level 0|1|2] [--model-url URL] [--max-steps N] [--mind DIR] [--freeze] [--seed N] [--temp X] [--thinking]";

/// The system prompt of `reflect`.
const REFLECT_SYSTEM: &str = "You review a failed attempt by an AI that uses the Monolith tools. Write ONE lesson: a general rule about using the tools that would have avoided the failure. One sentence, under 40 words, no names of files or values from this task. Reply as {\"lesson\": \"...\"}.";

/// The worked example for Interface and above (a different task).
const EXAMPLE: &str = "Example (a different task):\n\
{\"thought\": \"I need the exact arguments of chronos first.\", \"action\": \"help\", \"arg\": \"chronos\"}\n\
Result: chronos: keeps notes in a store\n  chronos init: makes the store\nDetails: help chronos COMMAND\n\
{\"thought\": \"The store is missing, so I create it.\", \"action\": \"run\", \"arg\": \"chronos init\"}\n\
Result: ok\n\
{\"thought\": \"The store is ready and the task is complete.\", \"action\": \"done\", \"arg\": \"the notes store is ready\"}\n\
Result: done";

/// One line about an action in the system prompt.
fn action_help(verb: &str) -> &'static str {
    match verb {
        "run" => "run: arg is one tool command line, for example: chronos init. Paths are relative to your computer's root.",
        "help" => "help: arg is TOOL or \"TOOL COMMAND\"; shows the exact arguments. Use it before your first use of a tool.",
        "write" => "write: arg is a file path, content is the whole file text.",
        "read" => "read: arg is a file path.",
        "remember" => "remember: arg is a fact to keep for later sessions on this computer. You will not see this conversation again.",
        "recall" => "recall: arg is what to look for in your memory.",
        _ => "done: arg is your final answer. Use it as soon as the task is complete and include anything the task asked you to report.",
    }
}

/// Asks the model for the next turn. One retry after 2 s; a second failure is the error text.
fn ask(
    ctx: &Ctx<'_>,
    msgs: &[Message],
    schema: &Json,
    turn: u32,
) -> std::result::Result<Reply, String> {
    let cfg = ctx.cfg;
    let call = || {
        llm::chat(&ChatRequest {
            url: &cfg.model_url,
            model: &cfg.model,
            messages: msgs,
            max_tokens: cfg.max_tokens,
            temperature: cfg.temperature,
            seed: cfg.seed.wrapping_add(i64::from(turn)),
            json_schema: Some(schema),
            thinking: cfg.thinking,
            timeout: Duration::from_secs(MODEL_TIMEOUT_SECS),
        })
    };
    match call() {
        Ok(r) => Ok(r),
        Err(_) => {
            thread::sleep(Duration::from_secs(2));
            call().map_err(|e| e.to_string())
        }
    }
}

/// The action in a model reply, or the reason it is not usable at `level`.
fn parse_turn(content: &str, level: Level) -> std::result::Result<Action, String> {
    let (_, action) = Action::from_json(content).map_err(|e| e.message)?;
    let allowed = allowed_verbs(level);
    if !allowed.contains(&action.verb()) {
        return Err(format!(
            "`{}` is not an action here; use one of: {}",
            action.verb(),
            allowed.join(", ")
        ));
    }
    Ok(action)
}

/// The content of a write action, or empty text.
fn content_of(action: &Action) -> &str {
    match action {
        Action::Write { content, .. } => content,
        _ => "",
    }
}

/// Keeps the prompt under [`CONTEXT_LIMIT`]: at Interface and above, older results are cut to
/// their first bytes; then the oldest assistant and user pair is dropped until it fits.
fn compact(msgs: &mut Vec<Message>, level: Level) {
    if size(msgs) <= CONTEXT_LIMIT {
        return;
    }
    if level >= Level::Interface {
        let results: Vec<usize> = msgs
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == Role::User && m.content.starts_with("Result: "))
            .map(|(i, _)| i)
            .collect();
        let older = results.len().saturating_sub(KEEP_RESULTS);
        for &i in &results[..older] {
            let m = &mut msgs[i];
            if m.content.len() > CUT_RESULT_BYTES && !m.content.ends_with(CUT_MARK) {
                m.content = format!("{}{CUT_MARK}", head(&m.content, CUT_RESULT_BYTES));
            }
        }
    }
    while size(msgs) > CONTEXT_LIMIT && msgs.len() >= 4 {
        msgs.drain(2..4);
    }
}

/// Total bytes of all message contents.
fn size(msgs: &[Message]) -> usize {
    msgs.iter().map(|m| m.content.len()).sum()
}

/// The first `max` bytes of `s`, cut back to a char boundary.
fn head(s: &str, max: usize) -> &str {
    let mut end = max.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// This session's actions as `verb arg -> first 200 bytes of the result`, from the journal.
fn session_steps(ctx: &Ctx<'_>) -> Vec<String> {
    let entries = journal::read(ctx.computer).unwrap_or_default();
    let mut pending: Option<String> = None;
    let mut lines = Vec::new();
    for e in entries.iter().filter(|e| e.session == ctx.session) {
        let field = |k: &str| {
            e.data
                .get(k)
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string()
        };
        match e.kind.as_str() {
            "action" => pending = Some(format!("{} {}", field("verb"), field("arg"))),
            "observation" => {
                if let Some(step) = pending.take() {
                    lines.push(format!("{step} -> {}", head(&field("text"), 200)));
                }
            }
            _ => {}
        }
    }
    lines
}

/// Texts of search results, or none when the search fails.
fn texts(found: Result<Vec<Item>>) -> Vec<String> {
    found
        .map(|items| items.into_iter().map(|i| i.text).collect())
        .unwrap_or_default()
}

/// Adds a `heading` and one `- item` line per item, unless there are no items.
fn push_list(sections: &mut Vec<String>, heading: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    let body: Vec<String> = items.iter().map(|t| format!("- {t}")).collect();
    sections.push(format!("{heading}\n{}", body.join("\n")));
}

/// The CLI data and text for a finished run.
fn report(out: &Outcome, session: &str) -> CmdOut {
    let (stop, error) = match &out.stop {
        Stop::Done => ("done", None),
        Stop::StepLimit => ("step_limit", None),
        Stop::ModelError(m) => ("model_error", Some(m.clone())),
    };
    let mut pairs = vec![
        (
            "answer".to_string(),
            out.answer.as_deref().map_or(Json::Null, js),
        ),
        ("stop".to_string(), js(stop)),
    ];
    if let Some(e) = &error {
        pairs.push(("error".into(), js(e)));
    }
    let rest = [
        ("steps", int(out.steps)),
        ("invalid_actions", int(out.invalid_actions)),
        ("failed_actions", int(out.failed_actions)),
        ("prompt_tokens", int(out.prompt_tokens)),
        ("completion_tokens", int(out.completion_tokens)),
        ("model_ms", int(out.model_ms)),
        ("session", js(session)),
    ];
    pairs.extend(rest.into_iter().map(|(k, v)| (k.to_string(), v)));
    let headline = out
        .answer
        .clone()
        .unwrap_or_else(|| format!("(no answer: {stop})"));
    let text = format!(
        "{headline}\nsteps {}, invalid {}, failed {}, tokens {}+{}",
        out.steps,
        out.invalid_actions,
        out.failed_actions,
        out.prompt_tokens,
        out.completion_tokens
    );
    CmdOut {
        data: Json::Obj(pairs),
        text,
        exit: if matches!(out.stop, Stop::Done) { 0 } else { 1 },
    }
}

/// Parses a number flag value.
fn number<T: std::str::FromStr>(text: &str, flag: &str) -> Result<T> {
    text.parse::<T>()
        .map_err(|_| Error::new(Code::Usage, format!("{flag} needs a number, not `{text}`")))
}

/// The value after the flag at `args[*i]`; advances `i` past it.
fn flag_value<'a>(args: &'a [String], i: &mut usize) -> Result<&'a str> {
    let flag = args[*i].as_str();
    match args.get(*i + 1) {
        Some(v) => {
            *i += 1;
            Ok(v.as_str())
        }
        None => Err(Error::new(Code::Usage, format!("{flag} needs a value")).hint(USAGE)),
    }
}

/// Journals one entry. A journal failure never stops a run.
fn log(ctx: &Ctx<'_>, kind: &str, data: Json) {
    let _ = journal::append(ctx.computer, ctx.session, kind, data);
}

/// A string as JSON.
fn js(s: &str) -> Json {
    Json::Str(s.to_string())
}

/// A whole number as JSON (saturating at `i64::MAX`).
fn int<T: TryInto<i64>>(v: T) -> Json {
    Json::Int(v.try_into().unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn head_respects_char_boundaries() {
        assert_eq!(head("aé", 2), "a");
        assert_eq!(head("abc", 10), "abc");
    }
}
