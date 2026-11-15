//! Benchmarks for `mercury`: HTTP/1.1 parsing, serialization, and keep-alive server.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mercury::server::{Config, Router, Server};
use mercury::{parse_request, parse_response, write_response, Error, Limits, Response};

use crate::{measure, Opts, Sample};

const REQUEST_BYTES: &[u8] = concat!(
    "GET /api/v1/items/42?filter=active HTTP/1.1\r\n",
    "Host: api.example.internal:8080\r\n",
    "User-Agent: monolith-bench/1.0\r\n",
    "Accept: application/json\r\n",
    "Accept-Encoding: gzip, br\r\n",
    "Accept-Language: en-US,en\r\n",
    "Connection: keep-alive\r\n",
    "Cache-Control: no-cache\r\n",
    "Authorization: Bearer tok_0123456789abcdef01234\r\n",
    "X-Request-Id: req-0011223344556\r\n",
    "X-Forwarded-For: 192.0.2.10\r\n",
    "X-Client-Version: 2026.11\r\n",
    "If-None-Match: \"v1\"\r\n",
    "\r\n",
)
.as_bytes();

/// Run all `mercury` benchmarks.
pub fn run(opts: &Opts) -> Result<Vec<Sample>, String> {
    Ok(vec![
        bench_parse_request(opts)?,
        bench_write_response(opts)?,
        bench_server_keepalive(opts)?,
    ])
}

fn bench_parse_request(opts: &Opts) -> Result<Sample, String> {
    if REQUEST_BYTES.len() != 400 {
        return Err(format!(
            "mercury parse_request fixture expected 400 bytes, got {}",
            REQUEST_BYTES.len()
        ));
    }
    let limits = Limits::default();
    const BATCH: u64 = 128;

    measure("mercury", "parse_request", "req/s", opts.budget, || {
        let raw = std::hint::black_box(REQUEST_BYTES);
        for _ in 0..BATCH {
            let (req, used) =
                parse_request(raw, &limits).map_err(|e| format!("parse_request failed: {e:?}"))?;
            if used != raw.len()
                || req.method != "GET"
                || req.headers.len() != 12
                || !req.body.is_empty()
            {
                return Err("parse_request verification mismatch".to_string());
            }
            let _ = std::hint::black_box(req);
        }
        Ok(BATCH)
    })
}

fn bench_write_response(opts: &Opts) -> Result<Sample, String> {
    let resp = Response {
        minor: 1,
        status: 200,
        reason: "OK".to_string(),
        headers: vec![
            (
                "Content-Type".to_string(),
                b"application/octet-stream".to_vec(),
            ),
            ("Server".to_string(), b"mercury".to_vec()),
        ],
        body: vec![b'A'; 1024],
    };

    let sample_wire = write_response(&resp);
    let (parsed, used) = parse_response(&sample_wire, &Limits::default(), false)
        .map_err(|e| format!("write_response self-check parse failed: {e:?}"))?;
    if used != sample_wire.len() || parsed.status != 200 || parsed.body.len() != 1024 {
        return Err("write_response self-check mismatch".to_string());
    }
    let expected_len = sample_wire.len();
    const BATCH: u64 = 128;

    measure("mercury", "write_response", "resp/s", opts.budget, || {
        let r = std::hint::black_box(&resp);
        for _ in 0..BATCH {
            let wire = write_response(r);
            if wire.len() != expected_len
                || !wire.starts_with(b"HTTP/1.1 200 OK\r\n")
                || !wire.ends_with(&r.body)
            {
                return Err("write_response verification mismatch".to_string());
            }
            let _ = std::hint::black_box(wire);
        }
        Ok(BATCH)
    })
}

enum ClientCmd {
    Batch(u64),
    Stop,
}

fn run_client_worker(
    mut stream: TcpStream,
    cmd_rx: mpsc::Receiver<ClientCmd>,
    done_tx: mpsc::Sender<Result<u64, String>>,
) {
    let req = b"GET /ping HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n";
    let limits = Limits::default();
    let mut buf = [0u8; 4096];

    while let Ok(cmd) = cmd_rx.recv() {
        let count = match cmd {
            ClientCmd::Batch(n) => n,
            ClientCmd::Stop => break,
        };
        let mut res = Ok(count);
        for _ in 0..count {
            if let Err(e) = stream.write_all(req) {
                res = Err(format!("client write failed: {e}"));
                break;
            }
            let mut filled = 0usize;
            loop {
                if filled == buf.len() {
                    res = Err("response exceeded buffer".to_string());
                    break;
                }
                match stream.read(&mut buf[filled..]) {
                    Ok(0) => {
                        res = Err("unexpected EOF from server".to_string());
                        break;
                    }
                    Ok(n) => {
                        filled += n;
                        match parse_response(&buf[..filled], &limits, false) {
                            Ok((resp, used)) => {
                                if used != filled || resp.status != 200 || resp.body != b"pong" {
                                    res = Err(format!(
                                        "server response mismatch: status={} body_len={}",
                                        resp.status,
                                        resp.body.len()
                                    ));
                                }
                                break;
                            }
                            Err(Error::Incomplete) => continue,
                            Err(e) => {
                                res = Err(format!("client parse_response failed: {e:?}"));
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        res = Err(format!("client read failed: {e}"));
                        break;
                    }
                }
            }
            if res.is_err() {
                break;
            }
        }
        if done_tx.send(res).is_err() {
            break;
        }
    }
}

fn bench_server_keepalive(opts: &Opts) -> Result<Sample, String> {
    let mut router = Router::new();
    router.route(
        "GET",
        "/ping",
        Box::new(|_| Response {
            minor: 1,
            status: 200,
            reason: "OK".to_string(),
            headers: Vec::new(),
            body: b"pong".to_vec(),
        }),
    );

    let cfg = Config {
        addr: "127.0.0.1:0".to_string(),
        threads: 4,
        read_timeout_ms: 5000,
        keep_alive_max: u32::MAX,
        max_connections: 64,
        ..Config::default()
    };
    let srv = Server::start(cfg, router).map_err(|e| format!("server start failed: {e}"))?;
    let addr = srv.local_addr();

    const THREADS: usize = 4;
    const PER_THREAD: u64 = 25;

    let mut cmd_txs = Vec::with_capacity(THREADS);
    let mut handles = Vec::with_capacity(THREADS);
    let (done_tx, done_rx) = mpsc::channel();

    for idx in 0..THREADS {
        let stream =
            TcpStream::connect(addr).map_err(|e| format!("client {idx} connect failed: {e}"))?;
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let thread_done_tx = done_tx.clone();
        let handle = thread::Builder::new()
            .name(format!("bench-mercury-{idx}"))
            .spawn(move || run_client_worker(stream, cmd_rx, thread_done_tx))
            .map_err(|e| format!("spawn client {idx} failed: {e}"))?;
        cmd_txs.push(cmd_tx);
        handles.push(handle);
    }
    drop(done_tx);

    let mut total_sent = 0u64;
    let res = measure("mercury", "server_keepalive", "req/s", opts.budget, || {
        for tx in &cmd_txs {
            tx.send(ClientCmd::Batch(PER_THREAD))
                .map_err(|_| "worker channel closed".to_string())?;
        }
        let mut batch_ops = 0u64;
        for _ in 0..THREADS {
            let worker_res = done_rx
                .recv()
                .map_err(|_| "worker result channel closed".to_string())??;
            batch_ops = batch_ops.saturating_add(worker_res);
        }
        total_sent = total_sent.saturating_add(batch_ops);
        Ok(batch_ops)
    });

    for tx in cmd_txs {
        let _ = tx.send(ClientCmd::Stop);
    }
    for h in handles {
        let _ = h.join();
    }

    let stats = srv.stats();
    srv.stop();

    if stats.rejected != 0 {
        return Err(format!("server rejected {} connections", stats.rejected));
    }
    if stats.requests < total_sent {
        return Err(format!(
            "server recorded {} requests, expected at least {}",
            stats.requests, total_sent
        ));
    }

    res
}
