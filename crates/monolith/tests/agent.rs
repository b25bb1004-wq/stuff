//! Tests for the action executor and the agent loop: a fake tool (a copy of this crate's own
//! binary, which speaks the same `--describe` / `--json` protocol) and a fake model server.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aci::envelope::Code;
use aci::json::Json;
use monolith::actions::{self, Ctx};
use monolith::agent;
use monolith::computer::Computer;
use monolith::journal;
use monolith::memory::{Kind, Store};
use monolith::types::{Action, Level, Observation, RunConfig, Stop};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A new, empty folder under the system temp dir.
fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "monolith-agent-{tag}-{}-{n}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A computer `c1` whose tools are a folder with this crate's binary as `chronos`.
fn rig(tag: &str) -> Computer {
    let home = temp_dir(tag);
    let tools = temp_dir("tools");
    let exe = format!("chronos{}", std::env::consts::EXE_SUFFIX);
    fs::copy(env!("CARGO_BIN_EXE_monolith"), tools.join(exe)).unwrap();
    let mut c = Computer::create(&home, "c1").unwrap();
    c.tools = tools;
    c
}

/// Run settings at `level` with no model (tests that need one set `model_url`).
fn cfg(level: Level) -> RunConfig {
    RunConfig {
        level,
        ..RunConfig::default()
    }
}

/// Run settings at `level` that talk to the fake model at `url`.
fn model_cfg(level: Level, url: &str) -> RunConfig {
    RunConfig {
        level,
        model_url: url.to_string(),
        ..RunConfig::default()
    }
}

/// True when any of the request bodies contains `needle`.
fn all_contain(bodies: &[String], needle: &str) -> bool {
    bodies.iter().any(|b| b.contains(needle))
}

/// Runs one action at `level` against `c`, with a fresh mind store.
fn act(c: &Computer, level: Level, action: &Action) -> Observation {
    let mind = Store::at(&temp_dir("mind"));
    let cfg = cfg(level);
    let ctx = Ctx {
        computer: c,
        mind: &mind,
        cfg: &cfg,
        session: "t-act",
    };
    actions::execute(&ctx, action)
}

fn run(line: &str) -> Action {
    Action::Run { line: line.into() }
}

fn lines_of(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// Reads one HTTP request (head and `Content-Length` body) and returns the body.
fn read_request(stream: &mut TcpStream) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return String::new();
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let len = head
        .lines()
        .find_map(|l| {
            let l = l.to_ascii_lowercase();
            l.strip_prefix("content-length:")
                .and_then(|v| v.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    while buf.len() < head_end + len {
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let end = (head_end + len).min(buf.len());
    String::from_utf8_lossy(&buf[head_end..end]).into_owned()
}

/// A fake llama-server: answers request i with `script[i]` as the message content (the last
/// script line again if there are more requests). Returns its URL and the request bodies seen.
fn fake_model(script: &[&str]) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let script: Vec<String> = script.iter().map(|s| s.to_string()).collect();
    thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { break };
            let body = read_request(&mut stream);
            log.lock().unwrap().push(body);
            let content = script.get(i).or(script.last()).cloned().unwrap_or_default();
            let reply = format!(
                "{{\"choices\":[{{\"message\":{{\"content\":{}}}}}],\"usage\":{{\"prompt_tokens\":10,\"completion_tokens\":5}}}}",
                Json::Str(content).write()
            );
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.len(),
                reply
            );
            let _ = stream.write_all(resp.as_bytes());
            if i + 1 >= script.len() {
                break;
            }
        }
    });
    (url, seen)
}

/// The URL of a loopback port that nothing listens on.
fn closed_port_url() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    format!("http://{addr}")
}

#[test]
fn run_line_adds_json_unless_asked() {
    let c = rig("runline");
    let (code, out) = actions::run_line(&c, "chronos list", 60).unwrap();
    assert_eq!(code, 0);
    let env = Json::parse(out.trim()).unwrap();
    assert_eq!(env.get("ok").and_then(Json::as_bool), Some(true));
    let d = actions::run_tool(&c, "chronos --describe", 60).unwrap();
    assert_eq!(d.exit, 0);
    let m = Json::parse(d.stdout.trim()).unwrap();
    assert!(m.get("aci").is_some());
    assert!(m.get("ok").is_none());
}

#[test]
fn tool_index_manifest_and_help_cards() {
    let c = rig("index");
    let idx = actions::tool_index(&c).unwrap();
    let lines = lines_of(&idx);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("chronos: "));
    assert!(c.dir.join("manifests").join("chronos.json").is_file());

    let card = actions::help_card(&c, "chronos").unwrap();
    assert!(card
        .lines()
        .any(|l| l.contains("new") && l.contains("<NAME>")));
    let log = actions::help_card(&c, "chronos log").unwrap();
    assert!(log.contains("--limit"));
    assert!(log.contains("example:"));

    let e = actions::help_card(&c, "bash").unwrap_err();
    assert_eq!(e.code, Code::NotFound);
    assert!(e.hint.is_some());
    let e = actions::help_card(&c, "chronos nope").unwrap_err();
    assert_eq!(e.code, Code::NotFound);
    assert!(e.hint.is_some());
}

#[test]
fn execute_at_baseline_gives_raw_output() {
    let c = rig("baseline");
    let o = act(&c, Level::Baseline, &run("chronos list"));
    assert!(o.ok);
    assert!(o.text.starts_with("{\"ok\":true"), "{}", o.text);
    let refused = act(
        &c,
        Level::Baseline,
        &Action::Help {
            topic: "chronos".into(),
        },
    );
    assert!(!refused.ok);
    assert!(refused.text.contains("not available"));
}

#[test]
fn execute_at_interface_shapes_output_and_teaches() {
    let c = rig("interface");
    let o = act(&c, Level::Interface, &run("chronos list"));
    assert!(o.ok);
    assert!(o.text.starts_with("ok"), "{}", o.text);
    assert!(lines_of(&o.text).contains(&"total: 0"));

    let o = act(&c, Level::Interface, &run("chronos nope"));
    assert!(!o.ok);
    assert!(o.text.contains("error usage"), "{}", o.text);

    let o = act(&c, Level::Interface, &run("chronos new \"bad name\""));
    assert!(!o.ok);
    assert!(o.text.contains("error invalid"), "{}", o.text);
    assert!(o.text.contains("usage: chronos new <NAME>"), "{}", o.text);

    let o = act(&c, Level::Interface, &run("bash x"));
    assert!(!o.ok);
    assert!(o.text.contains("chronos"), "{}", o.text);
}

#[test]
fn fence_refuses_outside_paths() {
    let c = rig("fence");
    let o = act(&c, Level::Interface, &run("chronos new ../x"));
    assert!(!o.ok);
    assert!(o.text.contains(".."), "{}", o.text);
    let o = act(
        &c,
        Level::Interface,
        &Action::Write {
            path: "../x".into(),
            content: "no".into(),
        },
    );
    assert!(!o.ok);
    assert!(!c.dir.join("x").exists());
    assert!(!c.root().parent().unwrap().join("x").exists());
}

#[test]
fn write_then_read_round_trip() {
    let c = rig("files");
    let w = act(
        &c,
        Level::Baseline,
        &Action::Write {
            path: "notes/a.txt".into(),
            content: "hello\nworld\n".into(),
        },
    );
    assert!(w.ok);
    assert_eq!(w.text, "wrote 12 bytes to notes/a.txt");
    let r = act(
        &c,
        Level::Baseline,
        &Action::Read {
            path: "notes/a.txt".into(),
        },
    );
    assert!(r.ok);
    assert_eq!(r.text, "hello\nworld\n");
}

#[test]
fn read_of_missing_file_lists_folder_at_interface() {
    let c = rig("missing");
    fs::write(c.root().join("alpha.txt"), "a").unwrap();
    let o = act(
        &c,
        Level::Interface,
        &Action::Read {
            path: "nope.txt".into(),
        },
    );
    assert!(!o.ok);
    assert!(o.text.contains("no such file: nope.txt"));
    assert!(o.text.contains("files here: alpha.txt"), "{}", o.text);
    let plain = act(
        &c,
        Level::Baseline,
        &Action::Read {
            path: "nope.txt".into(),
        },
    );
    assert!(!plain.text.contains("files here"));
}

#[test]
fn long_observation_is_cut() {
    let c = rig("cut");
    fs::write(c.root().join("long.txt"), "x".repeat(1000)).unwrap();
    let mind = Store::at(&temp_dir("mind"));
    let mut cfg = cfg(Level::Baseline);
    cfg.obs_cap_bytes = 100;
    let ctx = Ctx {
        computer: &c,
        mind: &mind,
        cfg: &cfg,
        session: "t-cut",
    };
    let o = actions::execute(
        &ctx,
        &Action::Read {
            path: "long.txt".into(),
        },
    );
    assert!(o.text.contains("[cut:"));
    assert!(o.text.len() < 200);
}

#[test]
fn remember_then_recall_finds_the_fact() {
    let c = rig("memory");
    let r = act(
        &c,
        Level::Memory,
        &Action::Remember {
            text: "the server uses port 8473".into(),
        },
    );
    assert!(r.ok);
    assert_eq!(r.text, "remembered #1");
    let stored = Store::at(&c.dir.join("memory")).list().unwrap();
    assert_eq!(stored.len(), 1);

    let o = act(
        &c,
        Level::Memory,
        &Action::Recall {
            query: "server port".into(),
        },
    );
    assert!(o.ok);
    assert!(
        o.text.contains("[fact] the server uses port 8473"),
        "{}",
        o.text
    );

    let none = act(
        &c,
        Level::Memory,
        &Action::Recall {
            query: "zebra crossing".into(),
        },
    );
    assert!(none.ok);
    assert!(none.text.contains("nothing in memory"));
}

#[test]
fn run_task_end_to_end() {
    let c = rig("e2e");
    let (url, bodies) = fake_model(&[
        r#"{"thought":"learn the tool","action":"help","arg":"chronos"}"#,
        r#"{"thought":"make a computer","action":"run","arg":"chronos new box1"}"#,
        r#"{"thought":"check it","action":"run","arg":"chronos list"}"#,
        r#"{"thought":"finished","action":"done","arg":"one computer"}"#,
    ]);
    let mind = Store::at(&c.dir.join("memory"));
    let cfg = model_cfg(Level::Memory, &url);
    let ctx = Ctx {
        computer: &c,
        mind: &mind,
        cfg: &cfg,
        session: "t-run",
    };
    let out = agent::run_task(&ctx, "make one computer and list it");
    assert_eq!(out.stop, Stop::Done);
    assert_eq!(out.answer.as_deref(), Some("one computer"));
    assert_eq!(out.steps, 4);
    assert_eq!(out.invalid_actions, 0);
    assert_eq!(out.failed_actions, 0);
    assert_eq!(
        out.good_commands,
        vec!["chronos new box1".to_string(), "chronos list".to_string()]
    );
    assert_eq!(out.prompt_tokens, 40);
    assert_eq!(out.completion_tokens, 20);

    let seen = bodies.lock().unwrap().clone();
    assert_eq!(seen.len(), 4);
    assert!(seen[2].contains("Result:"));
    assert!(seen[2].contains("make a computer"));

    let kinds: Vec<String> = journal::read(&c)
        .unwrap()
        .into_iter()
        .filter(|e| e.session == "t-run")
        .map(|e| e.kind)
        .collect();
    let expected = [
        "task",
        "model",
        "action",
        "observation",
        "model",
        "action",
        "observation",
        "model",
        "action",
        "observation",
        "model",
        "action",
        "observation",
        "done",
    ];
    assert_eq!(kinds, expected);
}

#[test]
fn invalid_replies_are_counted_and_explained() {
    let c = rig("invalid");
    let (url, bodies) = fake_model(&[
        "not json",
        r#"{"thought":"x","action":"fly","arg":"y"}"#,
        r#"{"thought":"stop","action":"done","arg":"gave up"}"#,
    ]);
    let mind = Store::at(&c.dir.join("memory"));
    let cfg = model_cfg(Level::Memory, &url);
    let ctx = Ctx {
        computer: &c,
        mind: &mind,
        cfg: &cfg,
        session: "t-bad",
    };
    let out = agent::run_task(&ctx, "anything");
    assert_eq!(out.stop, Stop::Done);
    assert_eq!(out.invalid_actions, 2);
    assert_eq!(out.steps, 3);
    let seen = bodies.lock().unwrap().clone();
    assert!(seen[1].contains("Invalid reply"));
}

#[test]
fn step_limit_and_repeat_guard() {
    let c = rig("steps");
    let same = r#"{"thought":"again","action":"run","arg":"chronos list"}"#;
    let (url, bodies) = fake_model(&[same, same, same, same, same]);
    let mind = Store::at(&c.dir.join("memory"));
    let mut cfg = model_cfg(Level::Interface, &url);
    cfg.max_steps = 5;
    let ctx = Ctx {
        computer: &c,
        mind: &mind,
        cfg: &cfg,
        session: "t-loop",
    };
    let out = agent::run_task(&ctx, "list forever");
    assert_eq!(out.stop, Stop::StepLimit);
    assert_eq!(out.steps, 5);
    let seen = bodies.lock().unwrap().clone();
    assert!(all_contain(&seen[4..], "exactly this 3 times"));
}

#[test]
fn model_down_is_a_model_error_quickly() {
    let c = rig("down");
    let mind = Store::at(&c.dir.join("memory"));
    let cfg = model_cfg(Level::Memory, &closed_port_url());
    let ctx = Ctx {
        computer: &c,
        mind: &mind,
        cfg: &cfg,
        session: "t-down",
    };
    let started = Instant::now();
    let out = agent::run_task(&ctx, "anything");
    assert!(matches!(out.stop, Stop::ModelError(_)));
    assert!(started.elapsed() < Duration::from_secs(15));
}

#[test]
fn system_prompt_depends_on_level_and_memory() {
    let c = rig("prompt");
    let mind = Store::at(&temp_dir("mind"));
    mind.add(
        Kind::Lesson,
        "Read the help card of a command before using it.",
        true,
    )
    .unwrap();
    mind.add(
        Kind::Lesson,
        "Read the help card before using chronos commands.",
        false,
    )
    .unwrap();
    let task = "read the help card of chronos";

    let base_cfg = cfg(Level::Baseline);
    let base = agent::system_prompt(
        &Ctx {
            computer: &c,
            mind: &mind,
            cfg: &base_cfg,
            session: "p",
        },
        task,
    );
    assert!(!base.lines().any(|l| l.starts_with("help:")));
    assert!(!base.contains("Example"));

    let interface_cfg = cfg(Level::Interface);
    let iface = agent::system_prompt(
        &Ctx {
            computer: &c,
            mind: &mind,
            cfg: &interface_cfg,
            session: "p",
        },
        task,
    );
    assert!(iface.lines().any(|l| l.starts_with("help:")));
    assert!(iface.contains("Example (a different task):"));

    let mut memory_cfg = cfg(Level::Memory);
    memory_cfg.extra_lessons = vec!["Operator rule: check arguments first.".into()];
    let mem = agent::system_prompt(
        &Ctx {
            computer: &c,
            mind: &mind,
            cfg: &memory_cfg,
            session: "p",
        },
        task,
    );
    assert!(mem.contains("Lessons from earlier work (checked, follow them):"));
    assert!(mem.contains("- Read the help card of a command before using it."));
    assert!(!mem.contains("- Read the help card before using chronos commands."));
    let extra = mem.find("- Operator rule").unwrap();
    let verified = mem.find("- Read the help card of a command").unwrap();
    assert!(extra < verified);
}

#[test]
fn context_guard_keeps_the_prompt_small() {
    let c = rig("context");
    fs::write(c.root().join("big.txt"), "a".repeat(15_000)).unwrap();
    let mut script: Vec<&str> = vec![r#"{"thought":"read","action":"read","arg":"big.txt"}"#; 8];
    script.push(r#"{"thought":"done","action":"done","arg":"read it all"}"#);
    let (url, bodies) = fake_model(&script);
    let mind = Store::at(&c.dir.join("memory"));
    let mut cfg = model_cfg(Level::Memory, &url);
    cfg.obs_cap_bytes = 20_000;
    let ctx = Ctx {
        computer: &c,
        mind: &mind,
        cfg: &cfg,
        session: "t-ctx",
    };
    let out = agent::run_task(&ctx, "read a big file many times");
    assert_eq!(out.stop, Stop::Done);
    let seen = bodies.lock().unwrap().clone();
    assert_eq!(seen.len(), 9);
    assert!(seen.iter().all(|b| b.len() < 40_000));
}

#[test]
fn schema_and_run_flags() {
    let schema = agent::action_schema(Level::Baseline);
    let verbs: Vec<&str> = schema
        .get("properties")
        .and_then(|p| p.get("action"))
        .and_then(|a| a.get("enum"))
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .filter_map(Json::as_str)
        .collect();
    assert_eq!(verbs, ["run", "write", "read", "done"]);

    let args: Vec<String> = [
        "--level",
        "1",
        "--model-url",
        "http://127.0.0.1:9",
        "--max-steps",
        "7",
        "--mind",
        "/tmp/mind",
        "--seed",
        "42",
        "--temp",
        "0.2",
        "--freeze",
        "--thinking",
        "--task",
        "hello",
        "box",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut cfg = RunConfig::default();
    let left = agent::parse_run_flags(&args, &mut cfg).unwrap();
    assert_eq!(left, ["--task", "hello", "box"]);
    assert_eq!(cfg.level, Level::Interface);
    assert_eq!(cfg.model_url, "http://127.0.0.1:9");
    assert_eq!(cfg.max_steps, 7);
    assert_eq!(cfg.mind_dir, Some(PathBuf::from("/tmp/mind")));
    assert_eq!(cfg.seed, 42);
    assert!((cfg.temperature - 0.2).abs() < 1e-9);
    assert!(cfg.freeze_memory);
    assert!(cfg.thinking);

    let unknown: Vec<String> = ["box", "--bogus"].iter().map(|s| s.to_string()).collect();
    let left = agent::parse_run_flags(&unknown, &mut RunConfig::default()).unwrap();
    assert_eq!(left, ["box", "--bogus"]);

    for bad in [["--seed", "x"], ["--max-steps", "-1"], ["--level", "9"]] {
        let args: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
        let e = agent::parse_run_flags(&args, &mut RunConfig::default()).unwrap_err();
        assert_eq!(e.code, Code::Usage, "{bad:?}");
    }
    let missing: Vec<String> = vec!["--level".into()];
    assert_eq!(
        agent::parse_run_flags(&missing, &mut RunConfig::default())
            .unwrap_err()
            .code,
        Code::Usage
    );
}

#[test]
fn reflect_returns_a_usable_lesson_only() {
    let c = rig("reflect");
    let mind = Store::at(&c.dir.join("memory"));
    let good = "Always read the help card of a command before using it.";
    let reply = format!("{{\"lesson\":\"{good}\"}}");
    let (url, bodies) = fake_model(&[reply.as_str()]);
    let cfg = model_cfg(Level::Memory, &url);
    let ctx = Ctx {
        computer: &c,
        mind: &mind,
        cfg: &cfg,
        session: "t-reflect",
    };
    let checks = vec!["the file exists".to_string()];
    assert_eq!(
        agent::reflect(&ctx, "make a box", &checks).as_deref(),
        Some(good)
    );
    let seen = bodies.lock().unwrap().clone();
    assert!(seen[0].contains("Checks that failed"));

    let (url, _) = fake_model(&["{\"lesson\":\"no\"}"]);
    let cfg = model_cfg(Level::Memory, &url);
    let ctx = Ctx {
        computer: &c,
        mind: &mind,
        cfg: &cfg,
        session: "t-reflect",
    };
    assert_eq!(agent::reflect(&ctx, "make a box", &checks), None);
}

#[test]
fn render_data_and_cut_helpers_are_public() {
    let data = Json::obj([("total", Json::Int(0))]);
    assert_eq!(actions::render_data(&data), "total: 0");
}
