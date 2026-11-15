//! Fuzz soak tests for Xenon `Connection` and `Server`.
#![allow(clippy::unwrap_used)]

use assay::Rng;
use std::time::{Duration, Instant};
use xenon::{Config, Connection, Server, MAX_DATAGRAM};

const DEFAULT_SEED: u64 = 0x05EE_D799;
const DEFAULT_SOAK_MS: u64 = 300;
const MAX_INPUT_LEN: usize = 2048;

fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(val) => {
            let trimmed = val.trim();
            if let Some(hex) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
            {
                u64::from_str_radix(&hex.replace('_', ""), 16).unwrap_or(DEFAULT_SEED)
            } else {
                trimmed
                    .replace('_', "")
                    .parse::<u64>()
                    .unwrap_or(DEFAULT_SEED)
            }
        }
        Err(_) => DEFAULT_SEED,
    }
}

fn soak_budget() -> Duration {
    let ms = std::env::var("STRATA_SOAK_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_SOAK_MS);
    Duration::from_millis(ms)
}

fn key_from_rng(rng: &mut Rng) -> [u8; 32] {
    let mut k = [0u8; 32];
    rng.fill(&mut k);
    k
}

struct Corpus {
    datagrams: Vec<Vec<u8>>,
    server_secret: [u8; 32],
    server_pub: [u8; 32],
    client_cid: u64,
}

fn capture_real_exchange(rng: &mut Rng) -> Corpus {
    let mut datagrams = Vec::new();
    let server_secret = key_from_rng(rng);
    let server_seed = key_from_rng(rng);
    let client_seed = key_from_rng(rng);
    let mut now = 1_000_000u64;

    let mut server = Server::new(server_secret, server_seed, Config::default());
    let server_pub = server.public_key();
    let mut client = Connection::connect(server_pub, client_seed, Config::default(), now);
    let client_cid = client.cid();

    if let Some(init) = client.poll_transmit(now) {
        datagrams.push(init.clone());
        server.handle(&init, now);
    }

    if let Some(sc) = server.connection(client_cid) {
        if let Some(resp) = sc.poll_transmit(now) {
            datagrams.push(resp.clone());
            client.handle(&resp, now);
        }
    }

    while let Some(pkt) = client.poll_transmit(now) {
        datagrams.push(pkt.clone());
        server.handle(&pkt, now);
    }

    assert!(client.is_connected(), "captured client must be connected");
    assert_eq!(server.accept(), Some(client_cid));

    // Stream round-trip: client -> server -> client
    let payload = b"xenon-soak-roundtrip-payload-0123456789abcdef";
    let sid = client.open_stream().unwrap();
    let written = client.write(sid, payload).unwrap();
    assert_eq!(written, payload.len());
    client.finish(sid).unwrap();

    now += 10_000;
    while let Some(pkt) = client.poll_transmit(now) {
        datagrams.push(pkt.clone());
        server.handle(&pkt, now);
    }

    let sc = server.connection(client_cid).unwrap();
    let accepted_sid = sc.accept_stream().unwrap();
    let mut recv_buf = vec![0u8; payload.len() + 16];
    let mut got = Vec::new();
    loop {
        let n = sc.read(accepted_sid, &mut recv_buf).unwrap();
        if n == 0 {
            break;
        }
        got.extend_from_slice(&recv_buf[..n]);
    }
    assert_eq!(got, payload);
    assert!(sc.stream_finished(accepted_sid));

    sc.write(accepted_sid, &got).unwrap();
    sc.finish(accepted_sid).unwrap();

    now += 10_000;
    while let Some(pkt) = sc.poll_transmit(now) {
        datagrams.push(pkt.clone());
        client.handle(&pkt, now);
    }

    let mut client_got = Vec::new();
    loop {
        let n = client.read(sid, &mut recv_buf).unwrap();
        if n == 0 {
            break;
        }
        client_got.extend_from_slice(&recv_buf[..n]);
    }
    assert_eq!(client_got, payload);
    assert!(client.stream_finished(sid));

    client.close(0, "done");
    if let Some(close_pkt) = client.poll_transmit(now) {
        datagrams.push(close_pkt.clone());
        server.handle(&close_pkt, now);
    }

    Corpus {
        datagrams,
        server_secret,
        server_pub,
        client_cid,
    }
}

fn gen_input(rng: &mut Rng, corpus: &[Vec<u8>], target_cid: Option<u64>) -> Vec<u8> {
    if rng.chance(1, 2) || corpus.is_empty() {
        let max_len = rng.pick(&[17usize, 64, MAX_DATAGRAM, 1500]);
        return rng.bytes(*max_len);
    }

    let base = rng.pick(corpus);
    let mut out = match rng.below(4) {
        // Bit flips
        0 => {
            let mut v = base.clone();
            if !v.is_empty() {
                let flips = 1 + rng.below(8);
                for _ in 0..flips {
                    let idx = rng.below(v.len());
                    let bit = 1u8 << rng.below(8);
                    v[idx] ^= bit;
                }
            }
            v
        }
        // Truncation
        1 => {
            let new_len = rng.below(base.len() + 1);
            base[..new_len].to_vec()
        }
        // Splice two valid datagrams
        2 => {
            let other = rng.pick(corpus);
            let split_a = rng.below(base.len() + 1);
            let split_b = rng.below(other.len() + 1);
            let mut v = base[..split_a].to_vec();
            v.extend_from_slice(&other[split_b..]);
            v.truncate(MAX_INPUT_LEN);
            v
        }
        // Repeated chunks
        _ => {
            let mut v = Vec::new();
            if !base.is_empty() {
                let start = rng.below(base.len());
                let end = start + 1 + rng.below(base.len() - start);
                let chunk = &base[start..end];
                let reps = 2 + rng.below(3);
                for _ in 0..reps {
                    v.extend_from_slice(chunk);
                    if v.len() >= MAX_INPUT_LEN {
                        break;
                    }
                }
                v.truncate(MAX_INPUT_LEN);
            }
            v
        }
    };

    // Occasionally patch CID so packets reach deeper phase/AEAD/frame checks.
    if let Some(cid) = target_cid {
        if out.len() >= 10 && rng.chance(1, 2) {
            out[2..10].copy_from_slice(&cid.to_le_bytes());
        }
    }
    out
}

fn verify_pair_roundtrip(
    client: &mut Connection,
    server: &mut Server,
    cid: u64,
    now: &mut u64,
    rng: &mut Rng,
) {
    if !client.is_connected() {
        return;
    }
    let Ok(sid) = client.open_stream() else {
        return;
    };
    let msg = rng.bytes(256);
    if client.write(sid, &msg).is_err() {
        return;
    }
    let _ = client.finish(sid);

    for _ in 0..8 {
        *now += 5_000;
        while let Some(pkt) = client.poll_transmit(*now) {
            server.handle(&pkt, *now);
        }
        if let Some(sc) = server.connection(cid) {
            while let Some(pkt) = sc.poll_transmit(*now) {
                client.handle(&pkt, *now);
            }
        }
    }

    if let Some(sc) = server.connection(cid) {
        if let Some(s_sid) = sc.accept_stream() {
            let mut buf = vec![0u8; msg.len() + 16];
            let mut got = Vec::new();
            while let Ok(n) = sc.read(s_sid, &mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            assert_eq!(got, msg, "roundtrip mismatch on connected pair");
        }
    }
}

fn soak_connection(seed: u64, budget: Duration, corpus: &Corpus) -> u64 {
    let mut rng = Rng::new(seed ^ 0xC0AA_EC71);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while start.elapsed() < budget {
        let case_start = Instant::now();
        let phase_pick = rng.below(4);
        let mut now = 2_000_000u64;

        match phase_pick {
            // Phase 0: ClientHandshake
            0 => {
                let mut client = Connection::connect(
                    corpus.server_pub,
                    key_from_rng(&mut rng),
                    Config::default(),
                    now,
                );
                for _ in 0..8 {
                    let pkt = gen_input(&mut rng, &corpus.datagrams, Some(client.cid()));
                    client.handle(&pkt, now);
                    let _ = client.poll_transmit(now);
                    if let Some(t) = client.timeout() {
                        now = now.max(t);
                        client.on_timeout(now);
                    }
                    let _ = client.stats();
                }
                assert!(client.is_connected() || client.is_closed() || client.error().is_none());
            }
            // Phase 1: Connected (client & server)
            1 => {
                let mut server = Server::new(
                    corpus.server_secret,
                    key_from_rng(&mut rng),
                    Config::default(),
                );
                let mut client = Connection::connect(
                    server.public_key(),
                    key_from_rng(&mut rng),
                    Config::default(),
                    now,
                );
                let cid = client.cid();
                if let Some(init) = client.poll_transmit(now) {
                    server.handle(&init, now);
                }
                if let Some(sc) = server.connection(cid) {
                    if let Some(resp) = sc.poll_transmit(now) {
                        client.handle(&resp, now);
                    }
                }
                while let Some(ack) = client.poll_transmit(now) {
                    server.handle(&ack, now);
                }

                for _ in 0..8 {
                    let pkt = gen_input(&mut rng, &corpus.datagrams, Some(cid));
                    client.handle(&pkt, now);
                    let _ = client.poll_transmit(now);
                    if let Some(sc) = server.connection(cid) {
                        let pkt2 = gen_input(&mut rng, &corpus.datagrams, Some(cid));
                        sc.handle(&pkt2, now);
                        let _ = sc.poll_transmit(now);
                    }
                    now += 1_000;
                }

                if client.is_connected() {
                    verify_pair_roundtrip(&mut client, &mut server, cid, &mut now, &mut rng);
                } else {
                    assert!(client.is_closed() || client.error().is_some());
                }
            }
            // Phase 2: Draining
            2 => {
                let mut client = Connection::connect(
                    corpus.server_pub,
                    key_from_rng(&mut rng),
                    Config::default(),
                    now,
                );
                client.close(42, "soak-close");
                for _ in 0..6 {
                    let pkt = gen_input(&mut rng, &corpus.datagrams, Some(client.cid()));
                    client.handle(&pkt, now);
                    let _ = client.poll_transmit(now);
                    now += 50_000;
                    client.on_timeout(now);
                }
                assert!(client.is_closed() || client.error().is_some());
            }
            // Phase 3: Closed (including weak server key)
            _ => {
                let mut client =
                    Connection::connect([0u8; 32], key_from_rng(&mut rng), Config::default(), now);
                assert!(client.is_closed());
                for _ in 0..4 {
                    let pkt = gen_input(&mut rng, &corpus.datagrams, Some(client.cid()));
                    client.handle(&pkt, now);
                    assert!(client.poll_transmit(now).is_none());
                    client.on_timeout(now);
                    assert!(client.is_closed());
                }
            }
        }

        assert!(
            case_start.elapsed() < Duration::from_secs(1),
            "connection soak case took >= 1s"
        );
        cases += 1;
    }

    cases
}

fn soak_server(seed: u64, budget: Duration, corpus: &Corpus) -> u64 {
    let mut rng = Rng::new(seed ^ 0x5E47_E400);
    let start = Instant::now();
    let mut cases: u64 = 0;

    let cfg = Config {
        max_half_open: 8,
        max_connections: 256,
        ..Config::default()
    };
    let mut server = Server::new(corpus.server_secret, key_from_rng(&mut rng), cfg);
    let mut now = 5_000_000u64;

    while start.elapsed() < budget {
        let case_start = Instant::now();
        let target_cid = if rng.chance(1, 3) {
            Some(corpus.client_cid)
        } else {
            None
        };
        let pkt = gen_input(&mut rng, &corpus.datagrams, target_cid);
        if let Some(cid) = server.handle(&pkt, now) {
            if let Some(sc) = server.connection(cid) {
                let _ = sc.poll_transmit(now);
            }
        }

        while server.accept().is_some() {}
        let _ = server.cids();

        if rng.chance(1, 16) {
            if let Some(t) = server.timeout() {
                now = now.max(t);
            } else {
                now += 100_000;
            }
            server.on_timeout(now);
        } else {
            now += 1_000;
        }

        // Periodically verify the server still completes a clean handshake and round-trip.
        if cases.is_multiple_of(64) {
            let mut client = Connection::connect(
                server.public_key(),
                key_from_rng(&mut rng),
                Config::default(),
                now,
            );
            let cid = client.cid();
            if let Some(init) = client.poll_transmit(now) {
                server.handle(&init, now);
            }
            if let Some(sc) = server.connection(cid) {
                if let Some(resp) = sc.poll_transmit(now) {
                    client.handle(&resp, now);
                }
            }
            while let Some(ack) = client.poll_transmit(now) {
                server.handle(&ack, now);
            }
            if client.is_connected() {
                verify_pair_roundtrip(&mut client, &mut server, cid, &mut now, &mut rng);
                client.close(0, "bye");
                if let Some(cp) = client.poll_transmit(now) {
                    server.handle(&cp, now);
                }
            }
        }

        assert!(
            case_start.elapsed() < Duration::from_secs(1),
            "server soak case took >= 1s"
        );
        cases += 1;
    }

    cases
}

#[test]
fn soak_all_targets() {
    let seed = soak_seed();
    let budget = soak_budget();

    let mut corpus_rng = Rng::new(seed);
    let corpus = capture_real_exchange(&mut corpus_rng);

    let conn_cases = soak_connection(seed, budget, &corpus);
    println!("soak target=Connection seed={seed:#x} cases={conn_cases}");
    assert!(conn_cases > 0);

    let server_cases = soak_server(seed, budget, &corpus);
    println!("soak target=Server seed={seed:#x} cases={server_cases}");
    assert!(server_cases > 0);
}
