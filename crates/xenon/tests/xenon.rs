//! Integration, protocol-model, lossy-channel simulator, and fuzz tests for Xenon.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use sphinx::{aead, hkdf, sha256::Sha256, x25519};
use std::collections::{BTreeMap, BinaryHeap};
use xenon::{Config, Connection, Error, Server, MAX_DATAGRAM};

#[derive(Clone, Eq, PartialEq)]
struct InFlight {
    deliver_at: u64,
    seq: u64,
    to_server: bool,
    data: Vec<u8>,
}

impl Ord for InFlight {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        other
            .deliver_at
            .cmp(&self.deliver_at)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for InFlight {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy)]
struct ChannelCfg {
    loss_pct: u64,
    reorder_pct: u64,
    dup_pct: u64,
    corrupt_pct: u64,
    base_delay_us: u64,
}

fn check_invariants(c: &Connection, prev_bif: u64, lossless: bool) {
    let st = c.stats();
    assert!(st.cwnd >= 2400, "cwnd dropped below 2400: {}", st.cwnd);
    if lossless || prev_bif <= st.cwnd {
        assert!(
            st.bytes_in_flight <= st.cwnd + MAX_DATAGRAM as u64,
            "bytes_in_flight {} > cwnd {} + 1200",
            st.bytes_in_flight,
            st.cwnd
        );
    }
}

fn enqueue_pkt(
    q: &mut BinaryHeap<InFlight>,
    rng: &mut Rng,
    now: u64,
    seq: &mut u64,
    to_server: bool,
    pkt: Vec<u8>,
    ch: ChannelCfg,
) {
    assert!(pkt.len() <= MAX_DATAGRAM);
    if rng.chance(ch.loss_pct, 100) {
        return;
    }
    let copies = if rng.chance(ch.dup_pct, 100) { 2 } else { 1 };
    for _ in 0..copies {
        let mut data = pkt.clone();
        if rng.chance(ch.corrupt_pct, 100) && !data.is_empty() {
            let idx = rng.below(data.len());
            data[idx] ^= 1 << rng.below(8);
        }
        let extra = if rng.chance(ch.reorder_pct, 100) {
            rng.range(1_000, 40_000)
        } else {
            0
        };
        *seq += 1;
        q.push(InFlight {
            deliver_at: now + ch.base_delay_us + extra,
            seq: *seq,
            to_server,
            data,
        });
    }
}

fn step_pair(
    client: &mut Connection,
    server: &mut Server,
    now: u64,
    q: &mut BinaryHeap<InFlight>,
    rng: &mut Rng,
    seq: &mut u64,
    ch: ChannelCfg,
) {
    let lossless = ch.loss_pct == 0 && ch.corrupt_pct == 0;
    loop {
        let prev = client.stats().bytes_in_flight;
        let Some(pkt) = client.poll_transmit(now) else {
            break;
        };
        check_invariants(client, prev, lossless);
        enqueue_pkt(q, rng, now, seq, true, pkt, ch);
    }
    for cid in server.cids() {
        if let Some(sc) = server.connection(cid) {
            loop {
                let prev = sc.stats().bytes_in_flight;
                let Some(pkt) = sc.poll_transmit(now) else {
                    break;
                };
                check_invariants(sc, prev, lossless);
                enqueue_pkt(q, rng, now, seq, false, pkt, ch);
            }
        }
    }
}

fn next_event_time(client: &Connection, server: &Server, q: &BinaryHeap<InFlight>) -> Option<u64> {
    [
        client.timeout(),
        server.timeout(),
        q.peek().map(|p| p.deliver_at),
    ]
    .into_iter()
    .flatten()
    .min()
}

#[test]
fn handshake_and_model_cross_check() {
    let srv_sec = [0x21u8; 32];
    let srv_seed = [0x31u8; 32];
    let cli_seed = [0x41u8; 32];
    let cfg = Config::default();

    let mut server = Server::new(srv_sec, srv_seed, cfg);
    let srv_pub = server.public_key();
    assert_eq!(srv_pub, x25519::public_key(&srv_sec));

    let mut client = Connection::connect(srv_pub, cli_seed, cfg, 1000);
    assert!(!client.is_connected());
    assert!(!client.is_closed());
    assert_eq!(client.handshake_hash(), None);

    let init = client.poll_transmit(1000).unwrap();
    assert_eq!(init.len(), MAX_DATAGRAM);

    // Independent model check of Initial and k_init
    let mut c_rng = sphinx::rng::ChaChaRng::from_seed(cli_seed);
    let mut e_c = [0u8; 32];
    c_rng.fill(&mut e_c);
    let cid = c_rng.next_u64();
    assert_eq!(client.cid(), cid);
    let pub_e_c = x25519::public_key(&e_c);
    assert_eq!(&init[18..50], &pub_e_c);
    let mut h0_hasher = Sha256::new();
    h0_hasher.update(b"xenon/1");
    h0_hasher.update(&srv_pub);
    h0_hasher.update(&pub_e_c);
    let h0 = h0_hasher.finalize();
    let es = x25519::shared_secret(&e_c, &srv_pub).unwrap();
    let prk1 = hkdf::extract(b"xenon/1", &es);
    let mut k_init = [0u8; 32];
    let mut info = b"init".to_vec();
    info.extend_from_slice(&h0);
    hkdf::expand(&prk1, &info, &mut k_init).unwrap();
    let pt_init = aead::open(&k_init, &[0u8; 12], &init[..50], &init[50..]).unwrap();
    assert_eq!(pt_init.len(), 1134);

    // Server handles Initial
    assert_eq!(server.handle(&init, 2000), Some(cid));
    assert_eq!(server.accept(), None);
    assert_eq!(server.cids(), vec![cid]);

    let resp = server.connection(cid).unwrap().poll_transmit(2000).unwrap();
    assert!(resp.len() < init.len());

    // Client opens Response and sends ACK for server pn 0
    client.handle(&resp, 3000);
    assert!(client.is_connected());
    let h1 = client.handshake_hash().unwrap();
    assert_eq!(server.connection(cid).unwrap().handshake_hash(), Some(h1));

    let first_data = client.poll_transmit(3000).unwrap();
    assert_eq!(first_data[0], 3);
    assert_eq!(
        u64::from_le_bytes(first_data[10..18].try_into().unwrap()),
        1
    );

    // Verify first_data opens under model's k_c2s
    let pub_e_s: [u8; 32] = resp[18..50].try_into().unwrap();
    let ee = x25519::shared_secret(&e_c, &pub_e_s).unwrap();
    let mut h1_hasher = Sha256::new();
    h1_hasher.update(&h0);
    h1_hasher.update(&init);
    h1_hasher.update(&pub_e_s);
    assert_eq!(h1_hasher.finalize(), h1);
    let prk2 = hkdf::extract(&prk1, &ee);
    let mut k_c2s = [0u8; 32];
    let mut c2s_info = b"c2s".to_vec();
    c2s_info.extend_from_slice(&h1);
    hkdf::expand(&prk2, &c2s_info, &mut k_c2s).unwrap();
    let mut nonce1 = [0u8; 12];
    nonce1[4..12].copy_from_slice(&1u64.to_le_bytes());
    let first_pt = aead::open(&k_c2s, &nonce1, &first_data[..18], &first_data[18..]).unwrap();
    assert!(!first_pt.is_empty());

    assert_eq!(server.handle(&first_data, 4000), Some(cid));
    assert!(server.connection(cid).unwrap().is_connected());
    assert_eq!(server.accept(), Some(cid));
    assert_eq!(server.accept(), None);
}

#[test]
fn wrong_server_key_resends_6_initials_then_times_out() {
    let srv_sec = [0x11u8; 32];
    let wrong_pub = x25519::public_key(&[0x22u8; 32]);
    let mut server = Server::new(srv_sec, [1; 32], Config::default());
    let mut client = Connection::connect(wrong_pub, [2; 32], Config::default(), 0);

    let mut sent = Vec::new();
    let mut now = 0;
    for _ in 0..6 {
        let pkt = client.poll_transmit(now).unwrap();
        assert_eq!(server.handle(&pkt, now), None);
        sent.push(pkt);
        let t = client.timeout().unwrap();
        now = t;
        client.on_timeout(now);
    }
    for pkt in &sent[1..] {
        assert_eq!(pkt, &sent[0]);
    }
    assert_eq!(sent.len(), 6);
    assert!(!client.is_connected());
    assert!(client.is_closed());
    assert_eq!(client.error(), Some(Error::Timeout));
    assert_eq!(client.poll_transmit(now), None);
}

#[test]
fn server_drops_short_low_order_and_half_open_cap_and_resends_repeat() {
    let cfg = Config {
        max_half_open: 2,
        ..Config::default()
    };
    let mut server = Server::new([3; 32], [4; 32], cfg);
    let pubk = server.public_key();

    let mut c1 = Connection::connect(pubk, [10; 32], cfg, 0);
    let i1 = c1.poll_transmit(0).unwrap();
    assert_eq!(server.handle(&i1[..1199], 0), None);

    // Low-order E_c (all zeros)
    let mut low = i1.clone();
    low[18..50].fill(0);
    assert_eq!(server.handle(&low, 0), None);

    // Accept up to max_half_open = 2
    assert_eq!(server.handle(&i1, 0), Some(c1.cid()));
    let r1 = server
        .connection(c1.cid())
        .unwrap()
        .poll_transmit(0)
        .unwrap();

    // Repeat Initial queues identical Response
    assert_eq!(server.handle(&i1, 100), Some(c1.cid()));
    let r1_again = server
        .connection(c1.cid())
        .unwrap()
        .poll_transmit(100)
        .unwrap();
    assert_eq!(r1, r1_again);

    let mut c2 = Connection::connect(pubk, [11; 32], cfg, 0);
    let i2 = c2.poll_transmit(0).unwrap();
    assert_eq!(server.handle(&i2, 0), Some(c2.cid()));

    // 3rd half-open is dropped
    let mut c3 = Connection::connect(pubk, [12; 32], cfg, 0);
    let i3 = c3.poll_transmit(0).unwrap();
    assert_eq!(server.handle(&i3, 0), None);

    // Half-open expires after 10s
    let exp = server.timeout().unwrap();
    assert_eq!(exp, 10_000_000);
    server.on_timeout(exp);
    assert!(server.cids().is_empty());

    // Now c3 is accepted
    assert_eq!(server.handle(&i3, exp), Some(c3.cid()));
}

#[test]
fn replayed_and_corrupted_packets_leave_debug_unchanged() {
    let mut server = Server::new([5; 32], [6; 32], Config::default());
    let mut client = Connection::connect(server.public_key(), [7; 32], Config::default(), 0);
    let init = client.poll_transmit(0).unwrap();
    let cid = server.handle(&init, 0).unwrap();
    let resp = server.connection(cid).unwrap().poll_transmit(0).unwrap();
    client.handle(&resp, 100);
    let d1 = client.poll_transmit(100).unwrap();
    server.handle(&d1, 200);

    let sid = client.open_stream().unwrap();
    client.write(sid, b"hello xenon").unwrap();
    let d2 = client.poll_transmit(300).unwrap();
    assert_eq!(server.handle(&d2, 400), Some(cid));

    let dbg_cli = format!("{client:?}");
    let dbg_srv = format!("{:?}", server.connection(cid).unwrap());

    // Replay Response and Data packet
    client.handle(&resp, 999);
    assert_eq!(server.handle(&d2, 999), None);

    // Bit-flipped Data packet
    let mut bad = d2.clone();
    bad[20] ^= 0x80;
    client.handle(&bad, 999);
    assert_eq!(server.handle(&bad, 999), None);

    assert_eq!(format!("{client:?}"), dbg_cli);
    assert_eq!(format!("{:?}", server.connection(cid).unwrap()), dbg_srv);
}

#[test]
fn lossy_channel_multi_stream_bidirectional_byte_exact() {
    let profiles = [
        ChannelCfg {
            loss_pct: 0,
            reorder_pct: 0,
            dup_pct: 0,
            corrupt_pct: 0,
            base_delay_us: 5_000,
        },
        ChannelCfg {
            loss_pct: 15,
            reorder_pct: 20,
            dup_pct: 20,
            corrupt_pct: 15,
            base_delay_us: 8_000,
        },
        ChannelCfg {
            loss_pct: 30,
            reorder_pct: 30,
            dup_pct: 30,
            corrupt_pct: 30,
            base_delay_us: 10_000,
        },
    ];

    for (p_idx, ch) in profiles.into_iter().enumerate() {
        forall(3, 100 + p_idx as u64, |rng| {
            let cfg = Config {
                stream_window: 16_384,
                conn_window: 32_768,
                idle_timeout_ms: 60_000,
                ..Config::default()
            };

            let mut srv_sec = [0u8; 32];
            let mut srv_seed = [0u8; 32];
            let mut cli_seed = [0u8; 32];
            rng.fill(&mut srv_sec);
            rng.fill(&mut srv_seed);
            rng.fill(&mut cli_seed);

            let mut server = Server::new(srv_sec, srv_seed, cfg);
            let mut client = Connection::connect(server.public_key(), cli_seed, cfg, 0);
            let mut q = BinaryHeap::new();
            let mut seq = 0u64;
            let mut now = 0u64;

            let mut cli_tx: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
            let mut srv_tx: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
            let mut cli_sent_pos: BTreeMap<u64, usize> = BTreeMap::new();
            let mut srv_sent_pos: BTreeMap<u64, usize> = BTreeMap::new();
            let mut cli_rx: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
            let mut srv_rx: BTreeMap<u64, Vec<u8>> = BTreeMap::new();

            for idx in 0..3 {
                let len = 8_000 + rng.below(12_000);
                let mut buf = vec![0u8; len];
                rng.fill(&mut buf);
                cli_tx.insert(idx * 2, buf);
            }
            for idx in 0..2 {
                let len = 6_000 + rng.below(10_000);
                let mut buf = vec![0u8; len];
                rng.fill(&mut buf);
                srv_tx.insert(idx * 2 + 1, buf);
            }

            let mut cli_opened = false;
            let mut srv_opened = false;
            let mut srv_cid = None;

            for _ in 0..80_000 {
                if client.is_connected() && !cli_opened {
                    for &sid in cli_tx.keys() {
                        assert_eq!(client.open_stream().unwrap(), sid);
                        cli_sent_pos.insert(sid, 0);
                    }
                    cli_opened = true;
                }
                if let Some(cid) = server.accept() {
                    srv_cid = Some(cid);
                }
                if let Some(cid) = srv_cid {
                    let Some(sc) = server.connection(cid) else {
                        break;
                    };
                    if sc.is_connected() && !srv_opened {
                        for &sid in srv_tx.keys() {
                            assert_eq!(sc.open_stream().unwrap(), sid);
                            srv_sent_pos.insert(sid, 0);
                        }
                        srv_opened = true;
                    }
                }

                // Feed outgoing writes and drain incoming reads
                if client.is_connected() {
                    for (&sid, data) in &cli_tx {
                        let pos = cli_sent_pos.get_mut(&sid).unwrap();
                        if *pos < data.len() {
                            let n = client.write(sid, &data[*pos..]).unwrap();
                            *pos += n;
                            if *pos == data.len() {
                                client.finish(sid).unwrap();
                            }
                        }
                    }
                    while let Some(sid) = client.accept_stream() {
                        cli_rx.insert(sid, Vec::new());
                    }
                    for (&sid, out) in &mut cli_rx {
                        let mut tmp = [0u8; 2048];
                        loop {
                            let n = client.read(sid, &mut tmp).unwrap();
                            if n == 0 {
                                break;
                            }
                            out.extend_from_slice(&tmp[..n]);
                        }
                    }
                }
                if let Some(cid) = srv_cid {
                    let sc = server.connection(cid).unwrap();
                    if sc.is_connected() {
                        for (&sid, data) in &srv_tx {
                            let pos = srv_sent_pos.get_mut(&sid).unwrap();
                            if *pos < data.len() {
                                let n = sc.write(sid, &data[*pos..]).unwrap();
                                *pos += n;
                                if *pos == data.len() {
                                    sc.finish(sid).unwrap();
                                }
                            }
                        }
                        while let Some(sid) = sc.accept_stream() {
                            srv_rx.insert(sid, Vec::new());
                        }
                        for (&sid, out) in &mut srv_rx {
                            let mut tmp = [0u8; 2048];
                            loop {
                                let n = sc.read(sid, &mut tmp).unwrap();
                                if n == 0 {
                                    break;
                                }
                                out.extend_from_slice(&tmp[..n]);
                            }
                        }
                    }
                }

                step_pair(&mut client, &mut server, now, &mut q, rng, &mut seq, ch);

                let all_cli_done = srv_cid.is_some_and(|cid| {
                    let sc = server.connection(cid).unwrap();
                    cli_tx.keys().all(|&id| sc.stream_finished(id))
                });
                let all_srv_done = srv_tx.keys().all(|&id| client.stream_finished(id));
                if all_cli_done && all_srv_done {
                    break;
                }
                if client.is_closed() {
                    break;
                }

                let Some(t) = next_event_time(&client, &server, &q) else {
                    break;
                };
                now = t;
                while let Some(top) = q.peek() {
                    if top.deliver_at > now {
                        break;
                    }
                    let pkt = q.pop().unwrap();
                    if pkt.to_server {
                        server.handle(&pkt.data, now);
                    } else {
                        client.handle(&pkt.data, now);
                    }
                }
                if client.timeout().is_some_and(|d| now >= d) {
                    client.on_timeout(now);
                }
                if server.timeout().is_some_and(|d| now >= d) {
                    server.on_timeout(now);
                }
            }

            let srv_alive = srv_cid.is_some_and(|cid| server.connection(cid).is_some());
            if p_idx <= 1 {
                assert_eq!(client.error(), None);
                assert!(srv_alive);
            }
            if client.error().is_none() && srv_alive {
                for (sid, want) in &cli_tx {
                    assert_eq!(srv_rx.get(sid).unwrap(), want);
                }
                for (sid, want) in &srv_tx {
                    assert_eq!(cli_rx.get(sid).unwrap(), want);
                }
            }
        });
    }
}

#[test]
fn stream_limits_reset_close_and_idle_timeout() {
    let cfg = Config {
        max_streams: 2,
        idle_timeout_ms: 500,
        ..Config::default()
    };
    let mut server = Server::new([8; 32], [9; 32], cfg);
    let mut client = Connection::connect(server.public_key(), [10; 32], cfg, 0);

    let mut q = BinaryHeap::new();
    let mut seq = 0;
    let ch = ChannelCfg {
        loss_pct: 0,
        reorder_pct: 0,
        dup_pct: 0,
        corrupt_pct: 0,
        base_delay_us: 1_000,
    };
    let mut now = 0;
    while !client.is_connected() || server.accept().is_none() {
        step_pair(
            &mut client,
            &mut server,
            now,
            &mut q,
            &mut Rng::new(1),
            &mut seq,
            ch,
        );
        now = next_event_time(&client, &server, &q).unwrap();
        while q.peek().is_some_and(|p| p.deliver_at <= now) {
            let pkt = q.pop().unwrap();
            if pkt.to_server {
                server.handle(&pkt.data, now);
            } else {
                client.handle(&pkt.data, now);
            }
        }
        if client.timeout().is_some_and(|d| now >= d) {
            client.on_timeout(now);
        }
        if server.timeout().is_some_and(|d| now >= d) {
            server.on_timeout(now);
        }
    }

    let cid = server.cids()[0];
    let s0 = client.open_stream().unwrap();
    let s2 = client.open_stream().unwrap();
    assert_eq!(client.open_stream(), Err(Error::StreamLimit));
    assert_eq!(
        client.write(99, b"x"),
        Err(Error::Protocol("unknown stream"))
    );

    client.write(s0, b"partial").unwrap();
    client.reset(s0, 42).unwrap();
    assert_eq!(
        client.write(s0, b"more"),
        Err(Error::Protocol("unknown stream"))
    );
    client.finish(s2).unwrap();

    for _ in 0..20 {
        step_pair(
            &mut client,
            &mut server,
            now,
            &mut q,
            &mut Rng::new(1),
            &mut seq,
            ch,
        );
        let Some(t) = next_event_time(&client, &server, &q) else {
            break;
        };
        if t > now + 100_000 {
            break;
        }
        now = t;
        while q.peek().is_some_and(|p| p.deliver_at <= now) {
            let pkt = q.pop().unwrap();
            if pkt.to_server {
                server.handle(&pkt.data, now);
            } else {
                client.handle(&pkt.data, now);
            }
        }
        if client.timeout().is_some_and(|d| now >= d) {
            client.on_timeout(now);
        }
        if server.timeout().is_some_and(|d| now >= d) {
            server.on_timeout(now);
        }
    }

    {
        let sc = server.connection(cid).unwrap();
        assert_eq!(sc.accept_stream(), Some(s0));
        assert_eq!(sc.accept_stream(), Some(s2));
        let mut buf = [0u8; 32];
        assert_eq!(
            sc.read(s0, &mut buf),
            Err(Error::Closed {
                code: 42,
                reason: String::new()
            })
        );
        assert!(!sc.stream_finished(s0));
        assert!(sc.stream_finished(s2));
    }

    // Idle timeout test: no packets sent on timeout
    let idle_at = client.timeout().unwrap();
    client.on_timeout(idle_at);
    assert_eq!(client.error(), Some(Error::Timeout));
    assert!(client.is_closed());
    assert_eq!(client.poll_transmit(idle_at), None);
    assert_eq!(client.open_stream(), Err(Error::Timeout));

    // Close and draining test on a fresh connection
    let mut client2 = Connection::connect(server.public_key(), [20; 32], cfg, 0);
    let i2 = client2.poll_transmit(0).unwrap();
    let cid2 = server.handle(&i2, 0).unwrap();
    let r2 = server.connection(cid2).unwrap().poll_transmit(0).unwrap();
    client2.handle(&r2, 10);
    let d1 = client2.poll_transmit(10).unwrap();
    server.handle(&d1, 20);
    assert_eq!(server.accept(), Some(cid2));

    client2.close(4, "app done");
    assert_eq!(
        client2.error(),
        Some(Error::Closed {
            code: 4,
            reason: "app done".to_owned()
        })
    );
    assert!(!client2.is_closed());
    let close_pkt = client2.poll_transmit(30).unwrap();
    server.handle(&close_pkt, 40);
    {
        let sc2 = server.connection(cid2).unwrap();
        assert_eq!(
            sc2.error(),
            Some(Error::Closed {
                code: 4,
                reason: "app done".to_owned()
            })
        );
        assert!(sc2.is_closed());
        assert_eq!(sc2.poll_transmit(40), None);
    }
    let drain_end = client2.timeout().unwrap();
    client2.on_timeout(drain_end);
    assert!(client2.is_closed());
}

#[test]
fn fuzzed_datagrams_never_panic() {
    let mut server = Server::new([0x33; 32], [0x44; 32], Config::default());
    let mut client = Connection::connect(server.public_key(), [0x55; 32], Config::default(), 0);
    let init = client.poll_transmit(0).unwrap();
    let cid = server.handle(&init, 0).unwrap();
    let resp = server.connection(cid).unwrap().poll_transmit(0).unwrap();
    client.handle(&resp, 10);

    forall(300, 777, |r| {
        let mut pkt = r.bytes(1300);
        if r.chance(1, 3) && pkt.len() >= 18 {
            pkt[1] = 1;
            pkt[2..10].copy_from_slice(&cid.to_le_bytes());
        }
        client.handle(&pkt, 100);
        server.handle(&pkt, 100);
    });
}

#[test]
fn small_flow_control_windows_and_1mib_write_cap() {
    let cfg = Config {
        stream_window: 1000,
        conn_window: 1500,
        ..Config::default()
    };
    let mut server = Server::new([0x71; 32], [0x72; 32], cfg);
    let mut client = Connection::connect(server.public_key(), [0x73; 32], cfg, 0);
    let mut q = BinaryHeap::new();
    let mut seq = 0;
    let ch = ChannelCfg {
        loss_pct: 0,
        reorder_pct: 0,
        dup_pct: 0,
        corrupt_pct: 0,
        base_delay_us: 1_000,
    };
    let mut now = 0;
    while !client.is_connected() || server.accept().is_none() {
        step_pair(
            &mut client,
            &mut server,
            now,
            &mut q,
            &mut Rng::new(2),
            &mut seq,
            ch,
        );
        now = next_event_time(&client, &server, &q).unwrap();
        while q.peek().is_some_and(|p| p.deliver_at <= now) {
            let pkt = q.pop().unwrap();
            if pkt.to_server {
                server.handle(&pkt.data, now);
            } else {
                client.handle(&pkt.data, now);
            }
        }
    }

    let cid = server.cids()[0];
    let s0 = client.open_stream().unwrap();
    // 1 MiB write cap check
    let big = vec![0x5au8; 1_048_576 + 100];
    let taken = client.write(s0, &big).unwrap();
    assert_eq!(taken, 1_048_576);
    assert_eq!(client.write(s0, &[1, 2, 3]).unwrap(), 0);
    client.finish(s0).unwrap();

    let mut received = Vec::new();
    for _ in 0..10_000 {
        let sc = server.connection(cid).unwrap();
        let mut buf = [0u8; 512];
        if sc.accept_stream().is_some() || !received.is_empty() || sc.stream_finished(s0) {
            loop {
                let n = sc.read(s0, &mut buf).unwrap();
                if n == 0 {
                    break;
                }
                received.extend_from_slice(&buf[..n]);
            }
        }
        if sc.stream_finished(s0) {
            break;
        }
        step_pair(
            &mut client,
            &mut server,
            now,
            &mut q,
            &mut Rng::new(2),
            &mut seq,
            ch,
        );
        let Some(t) = next_event_time(&client, &server, &q) else {
            break;
        };
        now = t;
        while q.peek().is_some_and(|p| p.deliver_at <= now) {
            let pkt = q.pop().unwrap();
            if pkt.to_server {
                server.handle(&pkt.data, now);
            } else {
                client.handle(&pkt.data, now);
            }
        }
        if client.timeout().is_some_and(|d| now >= d) {
            client.on_timeout(now);
        }
        if server.timeout().is_some_and(|d| now >= d) {
            server.on_timeout(now);
        }
    }
    assert_eq!(received.len(), 1_048_576);
    assert!(received.iter().all(|&b| b == 0x5a));
}

#[test]
fn ack_and_pto_probes_work_when_window_full() {
    let mut server = Server::new([0x41; 32], [0x42; 32], Config::default());
    let mut client = Connection::connect(server.public_key(), [0x43; 32], Config::default(), 0);
    let init = client.poll_transmit(0).unwrap();
    let cid = server.handle(&init, 10_000).unwrap();
    let resp = server
        .connection(cid)
        .unwrap()
        .poll_transmit(10_000)
        .unwrap();
    client.handle(&resp, 20_000);
    let first = client.poll_transmit(20_000).unwrap();
    server.handle(&first, 30_000);
    assert_eq!(server.accept(), Some(cid));

    // Fill the client's entire cwnd (12000 bytes = 10 * 1200) and drop those packets.
    let s_cli = client.open_stream().unwrap();
    client.write(s_cli, &vec![0x11; 32_768]).unwrap();
    let mut now = 40_000u64;
    let mut sent = 0;
    while let Some(d) = client.poll_transmit(now) {
        assert_eq!(d.len(), MAX_DATAGRAM);
        sent += 1;
    }
    assert_eq!(sent, 10);
    assert_eq!(client.stats().bytes_in_flight, 12_000);
    assert_eq!(client.stats().cwnd, 12_000);

    // Server sends an ack-eliciting stream packet to client while client's window is full.
    let s_srv = server.connection(cid).unwrap().open_stream().unwrap();
    server
        .connection(cid)
        .unwrap()
        .write(s_srv, b"ping")
        .unwrap();
    let srv_pkt = server.connection(cid).unwrap().poll_transmit(now).unwrap();
    client.handle(&srv_pkt, now + 5_000);
    now += 5_000;

    // Client's next timer must be the 25 ms ACK deadline (well before idle timeout).
    let ack_due = client.timeout().unwrap();
    assert!(ack_due <= now + 25_000);
    now = ack_due;
    client.on_timeout(now);

    // Client must emit an ACK-only packet even with bytes_in_flight == cwnd, without changing bif.
    let ack_pkt = client.poll_transmit(now).expect("ACK-only packet expected");
    assert_eq!(client.stats().bytes_in_flight, 12_000);
    assert_eq!(client.poll_transmit(now), None);
    assert_eq!(server.handle(&ack_pkt, now + 1_000), Some(cid));

    // With packets still in flight, timeout() must return the PTO deadline (before 30s idle).
    let pto_due = client.timeout().unwrap();
    assert!(pto_due < now + 5_000_000);
    now = pto_due;
    client.on_timeout(now);

    // After PTO expiry, timeout() remains armed and PTO probes go out even with full window.
    assert!(client.timeout().is_some());
    let probe1 = client.poll_transmit(now).expect("first PTO probe expected");
    assert!(client.stats().bytes_in_flight > 12_000);
    let probe2 = client
        .poll_transmit(now)
        .expect("second PTO probe expected");
    assert_eq!(server.handle(&probe1, now + 1_000), Some(cid));
    assert_eq!(server.handle(&probe2, now + 1_000), Some(cid));
}
