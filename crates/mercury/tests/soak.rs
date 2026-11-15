//! Fuzz soak tests for mercury parsers and decoders.

#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use assay::Rng;
use mercury::ws::{accept_key, decode_frame, encode_frame, Frame};
use mercury::{
    parse_request, parse_response, write_request, write_response, Limits, Request, Response,
};

const DEFAULT_SEED: u64 = 0x05EE_D79A;
const DEFAULT_MS: u64 = 300;
const MAX_BUF: usize = 4096;
const MAX_ALLOC_LIMIT: usize = 1024 * 1024;

fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(raw) => {
            let s = raw.trim().replace('_', "");
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).unwrap_or(DEFAULT_SEED)
            } else {
                s.parse::<u64>().unwrap_or(DEFAULT_SEED)
            }
        }
        Err(_) => DEFAULT_SEED,
    }
}

fn soak_budget() -> Duration {
    let ms = std::env::var("STRATA_SOAK_MS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MS);
    Duration::from_millis(ms)
}

fn mutate(rng: &mut Rng, base: &[u8], other: &[u8]) -> Vec<u8> {
    let mut out = base.to_vec();
    let ops = 1 + rng.below(4);
    for _ in 0..ops {
        match rng.below(6) {
            0 if !out.is_empty() => {
                // Bit flip
                let idx = rng.below(out.len());
                let bit = 1u8 << rng.below(8);
                out[idx] ^= bit;
            }
            1 if !out.is_empty() => {
                // Truncate
                let new_len = rng.below(out.len() + 1);
                out.truncate(new_len);
            }
            2 if !other.is_empty() => {
                // Splice from other
                let a = rng.below(other.len());
                let b = a + rng.below(other.len() - a + 1);
                let pos = rng.below(out.len() + 1);
                let slice = &other[a..b];
                if out.len() + slice.len() <= MAX_BUF {
                    out.splice(pos..pos, slice.iter().copied());
                }
            }
            3 if !out.is_empty() => {
                // Repeat a chunk
                let a = rng.below(out.len());
                let b = a + rng.below((out.len() - a).min(64) + 1);
                let chunk = out[a..b].to_vec();
                let pos = rng.below(out.len() + 1);
                if out.len() + chunk.len() <= MAX_BUF {
                    out.splice(pos..pos, chunk);
                }
            }
            4 if out.len() > 1 => {
                // Delete a chunk
                let a = rng.below(out.len());
                let b = a + rng.below(out.len() - a + 1);
                out.drain(a..b);
            }
            _ => {
                // Insert random bytes
                let extra = rng.bytes(32);
                let pos = rng.below(out.len() + 1);
                if out.len() + extra.len() <= MAX_BUF {
                    out.splice(pos..pos, extra);
                }
            }
        }
    }
    out
}

fn random_limits(rng: &mut Rng) -> Limits {
    if rng.chance(1, 4) {
        Limits {
            max_head: 1 + rng.below(512),
            max_headers: rng.below(16),
            max_body: rng.below(4096),
        }
    } else {
        Limits {
            max_head: 16 * 1024,
            max_headers: 100,
            max_body: MAX_ALLOC_LIMIT,
        }
    }
}

fn random_headers(rng: &mut Rng) -> Vec<(String, Vec<u8>)> {
    const NAMES: &[&str] = &[
        "Host",
        "Connection",
        "User-Agent",
        "Accept",
        "X-Custom",
        "Upgrade",
    ];
    let count = rng.below(6);
    let mut headers = Vec::with_capacity(count);
    for _ in 0..count {
        let name = (*rng.pick(NAMES)).to_string();
        let val = match name.as_str() {
            "Connection" => (*rng.pick(&[b"close".as_slice(), b"keep-alive", b"Upgrade"])).to_vec(),
            "Host" => b"example.com:8080".to_vec(),
            _ => {
                let len = rng.below(24);
                (0..len)
                    .map(|_| b'a' + (rng.below(26) as u8))
                    .collect::<Vec<u8>>()
            }
        };
        headers.push((name, val));
    }
    headers
}

fn gen_request_bytes(rng: &mut Rng) -> Vec<u8> {
    if rng.chance(1, 5) {
        let chunk = rng.bytes(64);
        let mut wire = format!(
            "POST /upload HTTP/1.1\r\nHost: example.com\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
            chunk.len()
        )
        .into_bytes();
        wire.extend_from_slice(&chunk);
        if !chunk.is_empty() {
            wire.extend_from_slice(b"\r\n0\r\n\r\n");
        } else {
            wire.extend_from_slice(b"\r\n");
        }
        return wire;
    }

    let method = (*rng.pick(&["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS"])).to_string();
    let target = (*rng.pick(&["/", "/index.html", "/api/v1/items?q=test", "*"])).to_string();
    let minor = rng.below(2) as u8;
    let headers = random_headers(rng);
    let body = if method == "GET" || method == "HEAD" {
        Vec::new()
    } else {
        rng.bytes(256)
    };
    let req = Request {
        method,
        target,
        minor,
        headers,
        body,
    };
    write_request(&req)
}

fn gen_response_bytes(rng: &mut Rng) -> Vec<u8> {
    if rng.chance(1, 5) {
        let chunk = rng.bytes(64);
        let mut wire = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
            chunk.len()
        )
        .into_bytes();
        wire.extend_from_slice(&chunk);
        if !chunk.is_empty() {
            wire.extend_from_slice(b"\r\n0\r\n\r\n");
        } else {
            wire.extend_from_slice(b"\r\n");
        }
        return wire;
    }

    let status = *rng.pick(&[101u16, 200, 204, 304, 400, 404, 500]);
    let reason = (*rng.pick(&["OK", "", "Not Found", "Bad Request"])).to_string();
    let minor = rng.below(2) as u8;
    let headers = random_headers(rng);
    let body = if (100..=199).contains(&status) || status == 204 || status == 304 {
        Vec::new()
    } else {
        rng.bytes(256)
    };
    let resp = Response {
        minor,
        status,
        reason,
        headers,
        body,
    };
    write_response(&resp)
}

fn gen_frame_bytes(rng: &mut Rng) -> Vec<u8> {
    let opcode = *rng.pick(&[0x0u8, 0x1, 0x2, 0x8, 0x9, 0xA]);
    let is_control = (opcode & 0x08) != 0;
    let fin = if is_control { true } else { rng.chance(1, 2) };
    let max_len = if is_control { 125 } else { 300 };
    let payload = rng.bytes(max_len);
    let mask = if rng.chance(1, 2) {
        let mut m = [0u8; 4];
        rng.fill(&mut m);
        Some(m)
    } else {
        None
    };
    encode_frame(
        &Frame {
            fin,
            opcode,
            payload,
        },
        mask,
    )
}

#[test]
fn soak_parse_request() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x1111_1111_1111_1111);
    let start = Instant::now();
    let mut cases = 0u64;

    while start.elapsed() < budget || cases < 10 {
        let buf = if cases.is_multiple_of(2) {
            rng.bytes(MAX_BUF)
        } else {
            let a = gen_request_bytes(&mut rng);
            let b = gen_request_bytes(&mut rng);
            mutate(&mut rng, &a, &b)
        };
        let lim = random_limits(&mut rng);
        let t0 = Instant::now();
        let _ = parse_request(&buf, &lim);
        assert!(t0.elapsed() < Duration::from_secs(1));
        cases += 1;
    }

    println!("soak target=parse_request seed={seed:#x} cases={cases}");
}

#[test]
fn soak_parse_response() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x2222_2222_2222_2222);
    let start = Instant::now();
    let mut cases = 0u64;

    while start.elapsed() < budget || cases < 10 {
        let buf = if cases.is_multiple_of(2) {
            rng.bytes(MAX_BUF)
        } else {
            let a = gen_response_bytes(&mut rng);
            let b = gen_response_bytes(&mut rng);
            mutate(&mut rng, &a, &b)
        };
        let lim = random_limits(&mut rng);
        let head_only = rng.chance(1, 3);
        let t0 = Instant::now();
        let _ = parse_response(&buf, &lim, head_only);
        assert!(t0.elapsed() < Duration::from_secs(1));
        cases += 1;
    }

    println!("soak target=parse_response seed={seed:#x} cases={cases}");
}

#[test]
fn soak_decode_frame() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x3333_3333_3333_3333);
    let start = Instant::now();
    let mut cases = 0u64;

    while start.elapsed() < budget || cases < 10 {
        let buf = if cases.is_multiple_of(2) {
            rng.bytes(MAX_BUF)
        } else {
            let a = gen_frame_bytes(&mut rng);
            let b = gen_frame_bytes(&mut rng);
            mutate(&mut rng, &a, &b)
        };
        let masked = rng.chance(1, 2);
        let max_payload = if rng.chance(1, 4) {
            rng.below(512)
        } else {
            MAX_ALLOC_LIMIT
        };
        let t0 = Instant::now();
        let _ = decode_frame(&buf, masked, max_payload);
        assert!(t0.elapsed() < Duration::from_secs(1));
        cases += 1;
    }

    println!("soak target=decode_frame seed={seed:#x} cases={cases}");
}

#[test]
fn soak_accept_key() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x4444_4444_4444_4444);
    let start = Instant::now();
    let mut cases = 0u64;

    while start.elapsed() < budget || cases < 10 {
        let raw = if cases.is_multiple_of(2) {
            rng.bytes(256)
        } else {
            let valid = b"dGhlIHNhbXBsZSBub25jZQ==";
            mutate(&mut rng, valid, valid)
        };
        let s = String::from_utf8_lossy(&raw);
        let t0 = Instant::now();
        let out = accept_key(&s);
        assert!(!out.is_empty());
        assert!(t0.elapsed() < Duration::from_secs(1));
        cases += 1;
    }

    println!("soak target=accept_key seed={seed:#x} cases={cases}");
}
