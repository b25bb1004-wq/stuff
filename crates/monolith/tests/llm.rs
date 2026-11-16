//! Tests for the model client against a fake llama-server on 127.0.0.1.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use aci::envelope::Code;
use aci::json::Json;
use monolith::llm::{chat, ping, ChatRequest, Message, Role};

/// Starts a fake server on a free loopback port. It accepts one connection, reads the request,
/// writes each part (pausing `gap` between parts), keeps the socket open for `hold`, and sends the
/// request text back through the channel.
fn serve(parts: Vec<Vec<u8>>, gap: Duration, hold: Duration) -> (u16, Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = tx.send(read_request(&mut stream));
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                thread::sleep(gap);
            }
            if stream.write_all(part).is_err() {
                return;
            }
        }
        thread::sleep(hold);
    });
    (port, rx)
}

/// Reads the request head, then `Content-Length` bytes of body.
fn read_request(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return String::from_utf8_lossy(&buf).into_owned(),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let len: usize = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while buf.len() < head_end + len {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// A complete response with `Content-Length`.
fn reply(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// The value of a request header (case-insensitive name).
fn header(request: &str, name: &str) -> Option<String> {
    request
        .split("\r\n")
        .skip(1)
        .take_while(|l| !l.is_empty())
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
}

/// The parsed JSON body of a captured request.
fn body_of(request: &str) -> Json {
    Json::parse(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn messages() -> Vec<Message> {
    vec![
        Message {
            role: Role::System,
            content: "be brief".into(),
        },
        Message {
            role: Role::User,
            content: "hi".into(),
        },
        Message {
            role: Role::Assistant,
            content: "hello".into(),
        },
        Message {
            role: Role::User,
            content: "q".into(),
        },
    ]
}

fn schema() -> Json {
    Json::obj([
        ("type", Json::Str("object".into())),
        ("required", Json::Arr(vec![Json::Str("tool".into())])),
    ])
}

fn request<'a>(url: &'a str, msgs: &'a [Message], schema: Option<&'a Json>) -> ChatRequest<'a> {
    ChatRequest {
        url,
        model: "local",
        messages: msgs,
        max_tokens: 64,
        temperature: 0.6,
        seed: 7,
        json_schema: schema,
        thinking: false,
        timeout: Duration::from_secs(5),
    }
}

const OK_BODY: &str = r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#;

#[test]
fn content_length_reply_and_request_shape() {
    let (port, rx) = serve(
        vec![reply("200 OK", OK_BODY)],
        Duration::ZERO,
        Duration::ZERO,
    );
    let url = format!("http://127.0.0.1:{port}");
    let s = schema();
    let msgs = messages();
    let r = chat(&request(&url, &msgs, Some(&s))).unwrap();
    assert_eq!(r.content, "ok");
    assert_eq!(r.prompt_tokens, 12);
    assert_eq!(r.completion_tokens, 3);

    let sent = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        sent.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"),
        "{sent}"
    );
    assert_eq!(header(&sent, "connection").unwrap(), "close");
    let body_text = sent.split_once("\r\n\r\n").unwrap().1;
    assert_eq!(
        header(&sent, "content-length").unwrap(),
        body_text.len().to_string()
    );

    let b = body_of(&sent);
    assert_eq!(b.get("cache_prompt"), Some(&Json::Bool(true)));
    assert_eq!(b.get("stream"), Some(&Json::Bool(false)));
    assert_eq!(b.get("top_k"), Some(&Json::Int(20)));
    assert_eq!(
        b.get("chat_template_kwargs")
            .and_then(|k| k.get("enable_thinking")),
        Some(&Json::Bool(false))
    );
    let pairs: Vec<(&str, &str)> = b
        .get("messages")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|m| {
            (
                m.get("role").and_then(Json::as_str).unwrap(),
                m.get("content").and_then(Json::as_str).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("system", "be brief"),
            ("user", "hi"),
            ("assistant", "hello"),
            ("user", "q")
        ]
    );
    assert_eq!(
        b.get("response_format")
            .and_then(|f| f.get("json_schema"))
            .and_then(|j| j.get("schema")),
        Some(&s)
    );
}

#[test]
fn no_schema_means_no_response_format_and_thinking_flag_is_sent() {
    let (port, rx) = serve(
        vec![reply("200 OK", OK_BODY)],
        Duration::ZERO,
        Duration::ZERO,
    );
    let url = format!("http://127.0.0.1:{port}");
    let msgs = messages();
    let req = ChatRequest {
        thinking: true,
        ..request(&url, &msgs, None)
    };
    chat(&req).unwrap();
    let b = body_of(&rx.recv_timeout(Duration::from_secs(5)).unwrap());
    assert!(b.get("response_format").is_none());
    assert_eq!(
        b.get("chat_template_kwargs")
            .and_then(|k| k.get("enable_thinking")),
        Some(&Json::Bool(true))
    );
}

#[test]
fn chunked_reply_in_small_writes_with_extensions_and_trailers() {
    let json = r#"{"choices":[{"message":{"content":"chunked reply"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#;
    let pieces = [&json[..20], &json[20..45], &json[45..]];
    let mut parts = vec![
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec(),
    ];
    for (i, p) in pieces.iter().enumerate() {
        let ext = if i == 0 { ";ext=1" } else { "" };
        parts.push(format!("{:x}{ext}\r\n", p.len()).into_bytes());
        parts.push(format!("{p}\r\n").into_bytes());
    }
    parts.push(b"0\r\nX-Trailer: yes\r\n\r\n".to_vec());
    let (port, _rx) = serve(parts, Duration::from_millis(30), Duration::ZERO);
    let url = format!("http://127.0.0.1:{port}");
    let msgs = messages();
    let r = chat(&request(&url, &msgs, None)).unwrap();
    assert_eq!(r.content, "chunked reply");
    assert_eq!(r.prompt_tokens, 5);
    assert_eq!(r.completion_tokens, 2);
}

#[test]
fn reply_without_length_or_chunks_is_read_to_end_of_stream() {
    let raw = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{OK_BODY}");
    let (port, _rx) = serve(vec![raw.into_bytes()], Duration::ZERO, Duration::ZERO);
    let url = format!("http://127.0.0.1:{port}");
    let msgs = messages();
    let r = chat(&request(&url, &msgs, None)).unwrap();
    assert_eq!(r.content, "ok");
    assert_eq!(r.completion_tokens, 3);
}

#[test]
fn escapes_and_non_ascii_round_trip() {
    let body = r#"{"choices":[{"message":{"content":"line one\nsay \"hi\" café é end"}}]}"#;
    let (port, _rx) = serve(vec![reply("200 OK", body)], Duration::ZERO, Duration::ZERO);
    let url = format!("http://127.0.0.1:{port}");
    let msgs = messages();
    let r = chat(&request(&url, &msgs, None)).unwrap();
    assert_eq!(r.content, "line one\nsay \"hi\" café é end");
}

#[test]
fn http_500_is_io_with_status_in_message() {
    let (port, _rx) = serve(
        vec![reply("500 Internal Server Error", "boom")],
        Duration::ZERO,
        Duration::ZERO,
    );
    let url = format!("http://127.0.0.1:{port}");
    let msgs = messages();
    let e = chat(&request(&url, &msgs, None)).unwrap_err();
    assert_eq!(e.code, Code::Io);
    assert!(e.message.contains("500"), "{}", e.message);
    assert!(e.message.contains("boom"), "{}", e.message);
}

#[test]
fn non_json_200_is_invalid() {
    let (port, _rx) = serve(
        vec![reply("200 OK", "not json")],
        Duration::ZERO,
        Duration::ZERO,
    );
    let url = format!("http://127.0.0.1:{port}");
    let msgs = messages();
    let e = chat(&request(&url, &msgs, None)).unwrap_err();
    assert_eq!(e.code, Code::Invalid);
    assert!(e.message.contains("not json"), "{}", e.message);
}

#[test]
fn empty_choices_is_invalid() {
    let (port, _rx) = serve(
        vec![reply("200 OK", r#"{"choices":[]}"#)],
        Duration::ZERO,
        Duration::ZERO,
    );
    let url = format!("http://127.0.0.1:{port}");
    let msgs = messages();
    let e = chat(&request(&url, &msgs, None)).unwrap_err();
    assert_eq!(e.code, Code::Invalid);
}

#[test]
fn loopback_rule_and_connection_errors_need_no_server() {
    let msgs = messages();
    for url in [
        "http://example.com:8080",
        "http://10.0.0.5:8080",
        "http://192.168.1.2",
    ] {
        let e = chat(&request(url, &msgs, None)).unwrap_err();
        assert_eq!(e.code, Code::Denied, "{url}");
        assert!(
            e.hint.unwrap().contains("must run on this machine"),
            "{url}"
        );
    }
    let e = chat(&request("https://127.0.0.1:8080", &msgs, None)).unwrap_err();
    assert_eq!(e.code, Code::Unsupported);

    // Port 1 on loopback has nothing listening.
    let req = ChatRequest {
        timeout: Duration::from_secs(1),
        ..request("http://127.0.0.1:1", &msgs, None)
    };
    let e = chat(&req).unwrap_err();
    assert_eq!(e.code, Code::Io);
    assert!(e.hint.unwrap().contains("curl http://127.0.0.1:1/health"));
}

#[test]
fn silent_server_times_out_quickly() {
    let (port, _rx) = serve(vec![], Duration::ZERO, Duration::from_secs(10));
    let url = format!("http://127.0.0.1:{port}");
    let msgs = messages();
    let req = ChatRequest {
        timeout: Duration::from_millis(300),
        ..request(&url, &msgs, None)
    };
    let started = Instant::now();
    let e = chat(&req).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(e.code, Code::Io);
    assert!(e.message.contains("timed out"), "{}", e.message);
}

#[test]
fn base_path_is_part_of_the_request_line() {
    let (port, rx) = serve(
        vec![reply("200 OK", OK_BODY)],
        Duration::ZERO,
        Duration::ZERO,
    );
    let url = format!("http://127.0.0.1:{port}/api/");
    let msgs = messages();
    chat(&request(&url, &msgs, None)).unwrap();
    let sent = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        sent.starts_with("POST /api/v1/chat/completions HTTP/1.1\r\n"),
        "{sent}"
    );
}

#[test]
fn ping_ok_and_loading_503() {
    let (port, rx) = serve(vec![reply("200 OK", "{}")], Duration::ZERO, Duration::ZERO);
    ping(&format!("http://127.0.0.1:{port}"), Duration::from_secs(5)).unwrap();
    let sent = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(sent.starts_with("GET /health HTTP/1.1\r\n"), "{sent}");

    let (port, _rx) = serve(
        vec![reply("503 Service Unavailable", "loading")],
        Duration::ZERO,
        Duration::ZERO,
    );
    let e = ping(&format!("http://127.0.0.1:{port}"), Duration::from_secs(5)).unwrap_err();
    assert_eq!(e.code, Code::Io);
    assert!(e.message.contains("503"), "{}", e.message);
}
