//! Security and trust-boundary tests for Xenon.
#![allow(clippy::unwrap_used)]

use std::time::Instant;
use xenon::{Config, Connection, Error, Server, MAX_DATAGRAM};

fn test_keys() -> ([u8; 32], [u8; 32], [u8; 32]) {
    let mut secret = [0u8; 32];
    secret[0] = 7;
    secret[31] = 1;
    let mut s_seed = [0u8; 32];
    s_seed[0] = 9;
    let mut c_seed = [0u8; 32];
    c_seed[0] = 11;
    (secret, s_seed, c_seed)
}

fn complete_handshake(
    server: &mut Server,
    client_seed: [u8; 32],
    cfg: Config,
    now: u64,
) -> (Connection, u64) {
    let mut client = Connection::connect(server.public_key(), client_seed, cfg, now);
    let init = client.poll_transmit(now).unwrap();
    let cid = server.handle(&init, now).unwrap();
    let resp = server.connection(cid).unwrap().poll_transmit(now).unwrap();
    client.handle(&resp, now);
    assert!(client.is_connected());
    // Client sends first Data packet (contains ACK for Response) to complete handshake on server.
    let first_data = client.poll_transmit(now).unwrap();
    assert_eq!(server.handle(&first_data, now), Some(cid));
    assert!(server.connection(cid).unwrap().is_connected());
    assert_eq!(server.accept(), Some(cid));
    (client, cid)
}

#[test]
fn forged_and_corrupted_handshakes_rejected() {
    let (secret, s_seed, c_seed) = test_keys();
    let mut server = Server::new(secret, s_seed, Config::default());
    let mut client = Connection::connect(server.public_key(), c_seed, Config::default(), 0);
    let valid_init = client.poll_transmit(0).unwrap();
    assert_eq!(valid_init.len(), MAX_DATAGRAM);

    // Wrong version byte
    let mut bad_ver = valid_init.clone();
    bad_ver[1] = 2;
    assert_eq!(server.handle(&bad_ver, 1), None);

    // Non-zero packet number in Initial
    let mut bad_pn = valid_init.clone();
    bad_pn[10] = 1;
    assert_eq!(server.handle(&bad_pn, 1), None);

    // Corrupted tag in Initial
    let mut bad_tag = valid_init.clone();
    bad_tag[MAX_DATAGRAM - 1] ^= 0xff;
    assert_eq!(server.handle(&bad_tag, 1), None);

    // Mutated CID (bound in AAD)
    let mut bad_cid = valid_init.clone();
    bad_cid[2] ^= 0x01;
    assert_eq!(server.handle(&bad_cid, 1), None);

    // Low-order / all-zero ephemeral client key
    let mut zero_ec = valid_init.clone();
    zero_ec[18..50].fill(0);
    assert_eq!(server.handle(&zero_ec, 1), None);

    // Forged PKT_RESPONSE sent to server
    let mut fake_resp = valid_init.clone();
    fake_resp[0] = 2;
    assert_eq!(server.handle(&fake_resp, 1), None);

    // Weak all-zero server key on client connect
    let weak_client = Connection::connect([0u8; 32], c_seed, Config::default(), 0);
    assert!(weak_client.is_closed());
    assert!(matches!(weak_client.error(), Some(Error::Protocol(_))));

    // Server had zero connections created from all forged packets
    assert!(server.cids().is_empty());
}

#[test]
fn forged_and_mismatched_response_rejected_by_client() {
    let (secret, s_seed, c_seed) = test_keys();
    let mut server = Server::new(secret, s_seed, Config::default());
    let mut client = Connection::connect(server.public_key(), c_seed, Config::default(), 0);
    let init = client.poll_transmit(0).unwrap();
    let cid = server.handle(&init, 0).unwrap();
    let valid_resp = server.connection(cid).unwrap().poll_transmit(0).unwrap();

    // Truncated response
    client.handle(&valid_resp[..valid_resp.len() - 1], 1);
    assert!(!client.is_connected());

    // Wrong CID in response
    let mut wrong_cid = valid_resp.clone();
    wrong_cid[2] ^= 0x01;
    client.handle(&wrong_cid, 1);
    assert!(!client.is_connected());

    // Non-zero pn in response
    let mut bad_pn = valid_resp.clone();
    bad_pn[10] = 1;
    client.handle(&bad_pn, 1);
    assert!(!client.is_connected());

    // Corrupted ciphertext/tag in response
    let mut bad_tag = valid_resp.clone();
    let last = bad_tag.len() - 1;
    bad_tag[last] ^= 0x80;
    client.handle(&bad_tag, 1);
    assert!(!client.is_connected());

    // All-zero ephemeral server key
    let mut zero_es = valid_resp.clone();
    zero_es[18..50].fill(0);
    client.handle(&zero_es, 1);
    assert!(!client.is_connected());

    // Valid response still succeeds after all forged attempts
    client.handle(&valid_resp, 2);
    assert!(client.is_connected());

    // Replayed response after connected is ignored
    client.handle(&valid_resp, 3);
    assert!(client.is_connected());
}

#[test]
fn truncated_oversized_and_unknown_cid_packets_dropped() {
    let (secret, s_seed, c_seed) = test_keys();
    let mut server = Server::new(secret, s_seed, Config::default());
    let (mut client, cid) = complete_handshake(&mut server, c_seed, Config::default(), 0);

    let sid = client.open_stream().unwrap();
    assert_eq!(client.write(sid, b"ping").unwrap(), 4);
    let data_pkt = client.poll_transmit(10).unwrap();

    // Empty and short datagrams (< HEADER_LEN)
    for len in [0usize, 1, 17] {
        assert_eq!(server.handle(&data_pkt[..len], 20), None);
        client.handle(&data_pkt[..len], 20);
    }

    // Oversized datagrams (> MAX_DATAGRAM)
    let oversized = vec![0u8; MAX_DATAGRAM + 1];
    assert_eq!(server.handle(&oversized, 20), None);
    client.handle(&oversized, 20);

    // Truncated Data packet (missing tag / 1 byte short)
    assert_eq!(server.handle(&data_pkt[..data_pkt.len() - 1], 20), None);

    // Unknown connection ID for Data packet
    let mut unknown_cid = data_pkt.clone();
    unknown_cid[2] ^= 0x42;
    assert_eq!(server.handle(&unknown_cid, 20), None);

    // Data packet with pn == 0
    let mut pn_zero = data_pkt.clone();
    pn_zero[10..18].fill(0);
    assert_eq!(server.handle(&pn_zero, 20), None);

    // Data packet with pn >= MAX_PN (1 << 62) and u64::MAX
    let mut pn_max = data_pkt.clone();
    pn_max[10..18].copy_from_slice(&(1u64 << 62).to_le_bytes());
    assert_eq!(server.handle(&pn_max, 20), None);
    pn_max[10..18].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(server.handle(&pn_max, 20), None);

    // Legitimate packet still works
    assert_eq!(server.handle(&data_pkt, 20), Some(cid));
    // Replay of legitimate packet is rejected
    assert_eq!(server.handle(&data_pkt, 21), None);
}

#[test]
fn no_amplification_from_unauthenticated_or_initial_packets() {
    let (secret, s_seed, c_seed) = test_keys();
    let mut server = Server::new(secret, s_seed, Config::default());
    let mut client = Connection::connect(server.public_key(), c_seed, Config::default(), 0);
    let init = client.poll_transmit(0).unwrap();

    // Short Initial produces no connection and no reply
    assert_eq!(server.handle(&init[..MAX_DATAGRAM - 1], 0), None);
    assert!(server.cids().is_empty());

    // Valid 1200-byte Initial produces a much smaller Response
    let cid = server.handle(&init, 0).unwrap();
    let resp = server.connection(cid).unwrap().poll_transmit(0).unwrap();
    assert!(resp.len() < init.len());
    assert!(server.connection(cid).unwrap().poll_transmit(0).is_none());
}

#[test]
fn half_open_flood_bounded_and_expires_cleanly() {
    let (secret, s_seed, _) = test_keys();
    let cfg = Config {
        max_half_open: 16,
        ..Config::default()
    };
    let mut server = Server::new(secret, s_seed, cfg);
    let pub_key = server.public_key();

    let start = Instant::now();
    for i in 0u64..100 {
        let mut seed = [0u8; 32];
        seed[..8].copy_from_slice(&(i + 1).to_le_bytes());
        let mut c = Connection::connect(pub_key, seed, Config::default(), 100);
        let init = c.poll_transmit(100).unwrap();
        let res = server.handle(&init, 100);
        if i < 16 {
            assert!(res.is_some());
        } else {
            assert_eq!(res, None);
        }
    }
    assert!(start.elapsed().as_secs_f64() < 1.0);
    assert_eq!(server.cids().len(), 16);

    // After half-open timeout (10s), expired entries are evicted and new ones succeed
    let deadline = server.timeout().unwrap();
    server.on_timeout(deadline);
    assert!(server.cids().is_empty());

    let mut fresh = Connection::connect(pub_key, [99u8; 32], Config::default(), deadline);
    let init = fresh.poll_transmit(deadline).unwrap();
    assert!(server.handle(&init, deadline).is_some());
    assert_eq!(server.cids().len(), 1);
}

#[test]
fn hostile_extreme_peer_config_params_do_not_panic_or_allocate() {
    let (secret, s_seed, c_seed) = test_keys();
    let mut server = Server::new(secret, s_seed, Config::default());
    let hostile_cfg = Config {
        max_streams: u64::MAX,
        stream_window: u64::MAX,
        conn_window: u64::MAX,
        idle_timeout_ms: u64::MAX,
        max_half_open: usize::MAX,
    };
    let (mut client, cid) = complete_handshake(&mut server, c_seed, hostile_cfg, 0);

    let s_conn = server.connection(cid).unwrap();
    let sid = s_conn.open_stream().unwrap();
    assert_eq!(s_conn.write(sid, b"ok").unwrap(), 2);
    let pkt = s_conn.poll_transmit(10).unwrap();
    client.handle(&pkt, 20);
    let mut buf = [0u8; 16];
    let c_sid = client.accept_stream().unwrap();
    assert_eq!(client.read(c_sid, &mut buf).unwrap(), 2);
    assert_eq!(&buf[..2], b"ok");
}

#[test]
fn large_payload_respects_stream_and_conn_flow_control() {
    let (secret, s_seed, c_seed) = test_keys();
    let cfg = Config {
        max_streams: 2,
        stream_window: 1024,
        conn_window: 1536,
        idle_timeout_ms: 30_000,
        max_half_open: 16,
    };
    let mut server = Server::new(secret, s_seed, cfg);
    let (mut client, cid) = complete_handshake(&mut server, c_seed, cfg, 0);

    let s0 = client.open_stream().unwrap();
    let s1 = client.open_stream().unwrap();
    assert_eq!(client.open_stream(), Err(Error::StreamLimit));

    let big = vec![0x5a; 2048];
    let n0 = client.write(s0, &big).unwrap();
    let n1 = client.write(s1, &big).unwrap();
    assert_eq!(n0, 2048);
    assert_eq!(n1, 2048);

    // Pump packets without reading on server: server buffers must stay within conn_window
    for t in 1..20 {
        while let Some(pkt) = client.poll_transmit(t * 1000) {
            server.handle(&pkt, t * 1000);
        }
        while let Some(pkt) = server.connection(cid).unwrap().poll_transmit(t * 1000) {
            client.handle(&pkt, t * 1000);
        }
    }
    assert!(server.connection(cid).unwrap().is_connected());
    let mut tmp = [0u8; 4096];
    let r0 = server
        .connection(cid)
        .unwrap()
        .read(0, &mut tmp)
        .unwrap_or(0);
    let r1 = server
        .connection(cid)
        .unwrap()
        .read(2, &mut tmp)
        .unwrap_or(0);
    assert!(r0 <= 1024);
    assert!(r1 <= 1024);
    assert!(r0 + r1 <= 1536);
}

#[test]
fn reordered_duplicate_and_corrupted_data_packets() {
    let (secret, s_seed, c_seed) = test_keys();
    let mut server = Server::new(secret, s_seed, Config::default());
    let (mut client, cid) = complete_handshake(&mut server, c_seed, Config::default(), 0);

    let sid = client.open_stream().unwrap();
    let payload = vec![0xab; 3000];
    assert_eq!(client.write(sid, &payload).unwrap(), 3000);
    client.finish(sid).unwrap();

    let mut pkts = Vec::new();
    while let Some(p) = client.poll_transmit(10) {
        pkts.push(p);
    }
    assert!(pkts.len() >= 3);

    // Deliver in reverse order, with a bit-flipped copy before each
    for p in pkts.iter().rev() {
        let mut bad = p.clone();
        bad[20] ^= 0x01;
        assert_eq!(server.handle(&bad, 20), None);
        assert_eq!(server.handle(p, 20), Some(cid));
        // Duplicate is dropped
        assert_eq!(server.handle(p, 21), None);
    }

    let sc = server.connection(cid).unwrap();
    let rsid = sc.accept_stream().unwrap();
    let mut out = vec![0u8; 4096];
    let n = sc.read(rsid, &mut out).unwrap();
    assert_eq!(n, 3000);
    assert_eq!(&out[..n], payload.as_slice());
    assert!(sc.stream_finished(rsid));
}

#[test]
fn rtt_arithmetic_overflow_panics_on_large_timestamp() {
    let (secret, s_seed, c_seed) = test_keys();
    let mut server = Server::new(secret, s_seed, Config::default());
    let (mut client, cid) = complete_handshake(&mut server, c_seed, Config::default(), 0);

    // Client sends 1st ack-eliciting packet at t = 0
    let sid = client.open_stream().unwrap();
    assert_eq!(client.write(sid, b"hello").unwrap(), 5);
    let data_pkt1 = client.poll_transmit(0).unwrap();
    assert_eq!(server.handle(&data_pkt1, 0), Some(cid));

    // Trigger ACK on server
    server.on_timeout(30_000);
    let ack_pkt1 = server
        .connection(cid)
        .unwrap()
        .poll_transmit(30_000)
        .unwrap();

    // First ACK at a huge timestamp exercises detect_losses (9 * srtt) and base_pto (4 * rttvar)
    let t_huge = u64::MAX - 10_000_000;
    client.handle(&ack_pkt1, t_huge);
    assert!(client.stats().srtt_us > 0);

    // Second packet + ACK at huge timestamp exercises update_rtt (3 * rttvar, 7 * srtt)
    assert_eq!(client.write(sid, b"world").unwrap(), 5);
    let data_pkt2 = client.poll_transmit(0).unwrap();
    assert_eq!(server.handle(&data_pkt2, 30_000), Some(cid));
    server.on_timeout(60_000);
    let ack_pkt2 = server
        .connection(cid)
        .unwrap()
        .poll_transmit(60_000)
        .unwrap();
    client.handle(&ack_pkt2, u64::MAX - 5_000_000);

    // Closing + transmitting Close frame exercises 3 * base_pto() in Draining
    client.close(0, "done");
    let close_pkt = client.poll_transmit(u64::MAX - 1_000_000).unwrap();
    assert_eq!(server.handle(&close_pkt, 60_001), Some(cid));
    assert!(server.connection(cid).unwrap().is_closed());
}

#[test]
fn modified_initial_with_existing_cid_rejected_after_connected() {
    let (secret, s_seed, c_seed) = test_keys();
    let mut server = Server::new(secret, s_seed, Config::default());
    let mut client = Connection::connect(server.public_key(), c_seed, Config::default(), 0);
    let init = client.poll_transmit(0).unwrap();
    let cid = server.handle(&init, 0).unwrap();
    let resp = server.connection(cid).unwrap().poll_transmit(0).unwrap();
    client.handle(&resp, 10);
    let first_data = client.poll_transmit(10).unwrap();
    assert_eq!(server.handle(&first_data, 20), Some(cid));
    assert!(server.connection(cid).unwrap().is_connected());
    assert!(server.connection(cid).unwrap().poll_transmit(20).is_none());

    // Forged/modified Initial reusing the active CID must be rejected
    let mut forged_same_cid =
        Connection::connect(server.public_key(), [77u8; 32], Config::default(), 0)
            .poll_transmit(0)
            .unwrap();
    forged_same_cid[2..10].copy_from_slice(&cid.to_le_bytes());
    assert_eq!(server.handle(&forged_same_cid, 30), None);
    assert!(server.connection(cid).unwrap().poll_transmit(30).is_none());
}
