//! Tests for the exam runner: selftest, model runs, learning, resume, chains, filters, the CLI
//! and the report. A fake tool (a copy of this crate's binary) and a fake model server.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use aci::envelope::Code;
use aci::json::Json;
use monolith::eval::{self, Options};
use monolith::memory::{Kind, Store};
use monolith::types::{Level, RunConfig};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const EXAMPLE: &str = "== a1-make-file
prompt: Write the number 4471 into out.txt and register a computer named box1.
setup: write seed.txt
| hello
check: read out.txt => contains 4471
check: chronos list => contains \"total\":1
check: answer => contains 4471
solution: write out.txt
| 4471
solution: run chronos new box1
solution: done 4471
";

/// Where a test works: a base folder, the run folder under it and a tool folder.
struct Rig {
    base: PathBuf,
    tools: PathBuf,
}

/// A new, empty folder under the system temp dir.
fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "monolith-eval-{tag}-{}-{n}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A rig whose tool folder holds this crate's binary as `chronos`.
fn rig(tag: &str) -> Rig {
    let base = temp_dir(tag);
    let tools = temp_dir("tools");
    let exe = format!("chronos{}", std::env::consts::EXE_SUFFIX);
    fs::copy(env!("CARGO_BIN_EXE_monolith"), tools.join(exe)).unwrap();
    Rig { base, tools }
}

/// Writes a suite file `NAME.tasks` in the rig's base folder.
fn suite(r: &Rig, name: &str, text: &str) -> PathBuf {
    let p = r.base.join(format!("{name}.tasks"));
    fs::write(&p, text).unwrap();
    p
}

/// Run options for the rig: run folder `base/out`, the rig's tools.
fn options(r: &Rig, suites: Vec<PathBuf>, selftest: bool, cfg: RunConfig) -> Options {
    Options {
        suites,
        out: r.base.join("out"),
        selftest,
        only: Vec::new(),
        limit: None,
        cfg,
        tools: Some(r.tools.clone()),
    }
}

/// Run settings at `level`, with no model.
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

/// The result lines of a run folder, parsed.
fn lines(out: &Path) -> Vec<Json> {
    fs::read_to_string(out.join("results.jsonl"))
        .unwrap()
        .lines()
        .map(|l| Json::parse(l).unwrap())
        .collect()
}

/// A string field of a record.
fn sf(j: &Json, key: &str) -> String {
    j.get(key).and_then(Json::as_str).unwrap_or("").to_string()
}

/// A number field of a record.
fn nf(j: &Json, key: &str) -> i64 {
    j.get(key).and_then(Json::as_i64).unwrap()
}

/// A true/false field of a record.
fn bf(j: &Json, key: &str) -> bool {
    j.get(key).and_then(Json::as_bool).unwrap()
}

/// The lessons and skills a run folder's mind holds.
fn mind_items(out: &Path) -> Vec<monolith::memory::Item> {
    Store::at(&out.join("mind")).list().unwrap()
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

#[test]
fn selftest_passes_the_example_task() {
    let r = rig("selftest-pass");
    let s = suite(&r, "a1", EXAMPLE);
    let opts = options(&r, vec![s], true, cfg(Level::Memory));
    let sum = eval::run(&opts).unwrap();
    assert_eq!((sum.tasks, sum.passed), (1, 1));
    let rs = lines(&opts.out);
    assert_eq!(rs.len(), 1);
    assert_eq!(sf(&rs[0], "status"), "pass");
    let report = fs::read_to_string(opts.out.join("report.md")).unwrap();
    assert!(report.contains("a1"), "{report}");
}

#[test]
fn selftest_catches_bad_tasks() {
    let r = rig("selftest-bad");
    let text = "== b1-already
prompt: Say x.
setup: write seed.txt
| x
check: read seed.txt => contains x
solution: done x

== b2-wrong-solution
prompt: Write 4471 into out.txt.
check: read out.txt => contains 4471
solution: write out.txt
| 1234
solution: done 4471

== b3-setup-fails
prompt: Say y.
setup: run chronos nope
check: answer => contains y
solution: done y
";
    let s = suite(&r, "bad", text);
    let opts = options(&r, vec![s], true, cfg(Level::Memory));
    let sum = eval::run(&opts).unwrap();
    assert_eq!((sum.tasks, sum.passed, sum.setup_errors), (3, 0, 1));
    let rs = lines(&opts.out);
    assert_eq!(sf(&rs[0], "id"), "b1-already");
    assert_eq!(sf(&rs[0], "class"), "selftest");
    let first = rs[0].get("failed_checks").and_then(Json::as_array).unwrap();
    assert_eq!(
        first[0].as_str(),
        Some("checks already pass before the solution")
    );
    assert_eq!(sf(&rs[1], "id"), "b2-wrong-solution");
    assert_eq!(sf(&rs[1], "status"), "fail");
    assert_eq!(sf(&rs[1], "class"), "selftest");
    assert_eq!(sf(&rs[2], "status"), "setup_error");
    assert_eq!(sf(&rs[2], "class"), "setup_error");
}

#[test]
fn model_run_passes_and_sums_tokens() {
    let (url, seen) = fake_model(&[
        r#"{"action":"write","arg":"out.txt","content":"4471"}"#,
        r#"{"action":"run","arg":"chronos new box1"}"#,
        r#"{"action":"done","arg":"4471"}"#,
    ]);
    let r = rig("model-pass");
    let s = suite(&r, "a1", EXAMPLE);
    let opts = options(&r, vec![s], false, model_cfg(Level::Interface, &url));
    let sum = eval::run(&opts).unwrap();
    assert_eq!(sum.passed, 1);
    let rec = &lines(&opts.out)[0];
    assert_eq!(sf(rec, "status"), "pass");
    assert_eq!(sf(rec, "class"), "ok");
    assert_eq!(nf(rec, "steps"), 3);
    assert_eq!(nf(rec, "prompt_tokens"), 30);
    assert_eq!(nf(rec, "completion_tokens"), 15);
    assert_eq!(seen.lock().unwrap().len(), 3);
}

#[test]
fn model_run_fails_at_interface_without_reflection() {
    let (url, seen) = fake_model(&[r#"{"action":"done","arg":"no idea"}"#]);
    let r = rig("model-fail");
    let s = suite(&r, "a1", EXAMPLE);
    let opts = options(&r, vec![s], false, model_cfg(Level::Interface, &url));
    let sum = eval::run(&opts).unwrap();
    assert_eq!(sum.passed, 0);
    let rec = &lines(&opts.out)[0];
    assert_eq!(sf(rec, "status"), "fail");
    assert_eq!(sf(rec, "class"), "wrong_result");
    assert!(!rec
        .get("failed_checks")
        .and_then(Json::as_array)
        .unwrap()
        .is_empty());
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(mind_items(&opts.out).is_empty());
}

#[test]
fn failed_run_learns_a_verified_lesson_from_a_retry() {
    let lesson = r#"{"lesson":"Write the requested file before answering the task."}"#;
    let (url, seen) = fake_model(&[
        r#"{"action":"done","arg":"no idea"}"#,
        lesson,
        r#"{"action":"write","arg":"out.txt","content":"4471"}"#,
        r#"{"action":"run","arg":"chronos new box1"}"#,
        r#"{"action":"done","arg":"4471"}"#,
    ]);
    let r = rig("learn-pass");
    let s = suite(&r, "a1", EXAMPLE);
    let opts = options(&r, vec![s], false, model_cfg(Level::Memory, &url));
    let sum = eval::run(&opts).unwrap();
    assert_eq!((sum.passed, sum.passed_after_retry), (0, 1));
    let rec = &lines(&opts.out)[0];
    assert_eq!(sf(rec, "status"), "fail");
    assert!(bf(rec, "retried"));
    assert!(bf(rec, "retry_pass"));
    assert_eq!(
        sf(rec, "lesson"),
        "Write the requested file before answering the task."
    );
    let items = mind_items(&opts.out);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, Kind::Lesson);
    assert!(items[0].verified);
    assert_eq!(
        items[0].text,
        "Write the requested file before answering the task."
    );
    assert_eq!(seen.lock().unwrap().len(), 5);
}

#[test]
fn lesson_stays_unverified_when_the_retry_fails() {
    let lesson = r#"{"lesson":"Write the requested file before answering the task."}"#;
    let (url, seen) = fake_model(&[
        r#"{"action":"done","arg":"no idea"}"#,
        lesson,
        r#"{"action":"done","arg":"no idea"}"#,
    ]);
    let r = rig("learn-fail");
    let s = suite(&r, "a1", EXAMPLE);
    let opts = options(&r, vec![s], false, model_cfg(Level::Memory, &url));
    let sum = eval::run(&opts).unwrap();
    assert_eq!(sum.passed_after_retry, 0);
    let rec = &lines(&opts.out)[0];
    assert!(bf(rec, "retried"));
    assert!(!bf(rec, "retry_pass"));
    let items = mind_items(&opts.out);
    assert_eq!(items.len(), 1);
    assert!(!items[0].verified);
    assert_eq!(seen.lock().unwrap().len(), 3);
}

#[test]
fn frozen_memory_writes_nothing() {
    let (url, seen) = fake_model(&[r#"{"action":"done","arg":"no idea"}"#]);
    let r = rig("frozen-fail");
    let s = suite(&r, "a1", EXAMPLE);
    let mut c = model_cfg(Level::Memory, &url);
    c.freeze_memory = true;
    let opts = options(&r, vec![s], false, c);
    eval::run(&opts).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(mind_items(&opts.out).is_empty());

    let (url, _seen) = fake_model(&[
        r#"{"action":"write","arg":"out.txt","content":"4471"}"#,
        r#"{"action":"run","arg":"chronos new box1"}"#,
        r#"{"action":"done","arg":"4471"}"#,
    ]);
    let r = rig("frozen-pass");
    let s = suite(&r, "a1", EXAMPLE);
    let mut c = model_cfg(Level::Memory, &url);
    c.freeze_memory = true;
    let opts = options(&r, vec![s], false, c);
    eval::run(&opts).unwrap();
    assert_eq!(sf(&lines(&opts.out)[0], "status"), "pass");
    assert!(mind_items(&opts.out).is_empty());
}

#[test]
fn clean_pass_stores_a_verified_skill() {
    let (url, _seen) = fake_model(&[
        r#"{"action":"write","arg":"out.txt","content":"4471"}"#,
        r#"{"action":"run","arg":"chronos new box1"}"#,
        r#"{"action":"done","arg":"4471"}"#,
    ]);
    let r = rig("skill");
    let s = suite(&r, "a1", EXAMPLE);
    let opts = options(&r, vec![s], false, model_cfg(Level::Memory, &url));
    eval::run(&opts).unwrap();
    let items = mind_items(&opts.out);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, Kind::Skill);
    assert!(items[0].verified);
    assert!(
        items[0].text.contains("chronos new box1"),
        "{}",
        items[0].text
    );
}

#[test]
fn resume_skips_finished_tasks() {
    let (url, seen) = fake_model(&[
        r#"{"action":"write","arg":"out.txt","content":"4471"}"#,
        r#"{"action":"run","arg":"chronos new box1"}"#,
        r#"{"action":"done","arg":"4471"}"#,
    ]);
    let r = rig("resume");
    let s = suite(&r, "a1", EXAMPLE);
    let opts = options(&r, vec![s], false, model_cfg(Level::Interface, &url));
    eval::run(&opts).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 3);
    let sum = eval::run(&opts).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 3);
    assert_eq!(lines(&opts.out).len(), 1);
    assert_eq!(sum.tasks, 1);
}

#[test]
fn named_computer_is_shared_by_tasks() {
    let text = "== c1-write-seed
prompt: Write the seed.
computer: chain1
check: read seed.txt => contains 4471
solution: write seed.txt
| 4471
solution: done ok

== c2-use-seed
prompt: Register a box on the chain computer.
computer: chain1
check: read seed.txt => contains 4471
check: chronos list => contains \"total\":1
solution: run chronos new box1
solution: done ok

== c3-fresh
prompt: Check a fresh computer.
check: read seed.txt => contains 4471
solution: done ok
";
    let r = rig("chain");
    let s = suite(&r, "chain", text);
    let opts = options(&r, vec![s], true, cfg(Level::Memory));
    let sum = eval::run(&opts).unwrap();
    assert_eq!(sum.passed, 2);
    let rs = lines(&opts.out);
    assert_eq!(sf(&rs[0], "status"), "pass");
    assert_eq!(sf(&rs[1], "status"), "pass");
    assert_eq!(sf(&rs[1], "computer"), "chain1");
    assert_eq!(sf(&rs[2], "status"), "fail");
}

#[test]
fn only_and_limit_filter_tasks_and_duplicates_are_invalid() {
    let text = "== f1
prompt: one
check: answer => contains x
solution: done x

== f2
prompt: two
check: answer => contains x
solution: done x

== f3
prompt: three
check: answer => contains x
solution: done x
";
    let r = rig("filters");
    let s = suite(&r, "f", text);
    let mut opts = options(&r, vec![s.clone()], true, cfg(Level::Memory));
    opts.only = vec!["f1".into(), "f3".into()];
    let sum = eval::run(&opts).unwrap();
    assert_eq!(sum.tasks, 2);
    let ids: Vec<String> = lines(&opts.out).iter().map(|j| sf(j, "id")).collect();
    assert_eq!(ids, vec!["f1", "f3"]);

    let mut opts = options(&r, vec![s.clone()], true, cfg(Level::Memory));
    opts.out = r.base.join("limited");
    opts.limit = Some(1);
    assert_eq!(eval::run(&opts).unwrap().tasks, 1);

    let dup = suite(
        &r,
        "dup",
        "== f1\nprompt: again\ncheck: answer => contains x\nsolution: done x\n",
    );
    let opts = options(&r, vec![s, dup], true, cfg(Level::Memory));
    let e = eval::run(&opts).unwrap_err();
    assert_eq!(e.code, Code::Invalid);
}

#[test]
fn cmd_rejects_bad_arguments() {
    let r = rig("cmd");
    let s = suite(&r, "a1", EXAMPLE).to_string_lossy().to_string();
    let out = r.base.join("out").to_string_lossy().to_string();
    let args = |v: &[&str]| -> Vec<String> { v.iter().map(|x| x.to_string()).collect() };
    let e = eval::cmd(&args(&["--out", &out])).unwrap_err();
    assert_eq!(e.code, Code::Usage);
    assert!(e.hint.is_some());
    let e = eval::cmd(&args(&["--suite", &s])).unwrap_err();
    assert_eq!(e.code, Code::Usage);
    assert!(e.hint.is_some());
    let e = eval::cmd(&args(&["--suite", &s, "--out", &out, "--bogus"])).unwrap_err();
    assert_eq!(e.code, Code::Usage);
    assert!(e.hint.is_some());
}

#[test]
fn report_shows_pass_percent_and_failed_ids() {
    let text = "== p1
prompt: pass one
check: answer => contains x
solution: done x

== f1
prompt: fail one
check: answer => contains x
solution: done y
";
    let r = rig("report");
    let s = suite(&r, "mix", text);
    let opts = options(&r, vec![s], true, cfg(Level::Memory));
    let sum = eval::run(&opts).unwrap();
    assert!(
        sum.report.contains("| total | 2 | 1 | 50 | 1 |"),
        "{}",
        sum.report
    );
    assert!(sum.report.contains("Failed tasks:"));
    assert!(sum.report.contains("f1 (selftest):"), "{}", sum.report);
    assert!(!sum.report.contains("p1 ("));
    let on_disk = fs::read_to_string(opts.out.join("report.md")).unwrap();
    assert_eq!(on_disk, sum.report);
}
