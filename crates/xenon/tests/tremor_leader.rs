//! Leader check for T34 (xenon protocol core). Copied to crates/xenon/tests/tremor_leader.rs.
//! The handshake model is the leader's own reading of docs/XENON.md section 2 and briefs/T34.md,
//! built from sphinx primitives.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use sphinx::{aead, hkdf, rng::ChaChaRng, sha256, x25519};
use std::collections::{BTreeMap, VecDeque};
use xenon::{Config, Connection, Error, Server, MAX_DATAGRAM};

const MS: u64 = 1000;
const SEC: u64 = 1_000_000;

fn varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}
fn params(c: &Config) -> Vec<u8> {
    let mut p = vec![4];
    for (id, v) in [
        (1, c.max_streams),
        (2, c.stream_window),
        (3, c.conn_window),
        (4, c.idle_timeout_ms),
    ] {
        varint(id, &mut p);
        varint(v, &mut p);
    }
    p
}
fn header(kind: u8, cid: u64, pn: u64) -> Vec<u8> {
    let mut h = vec![kind, 1];
    h.extend_from_slice(&cid.to_le_bytes());
    h.extend_from_slice(&pn.to_le_bytes());
    h
}
fn nonce(pn: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&pn.to_le_bytes());
    n
}
fn expand(prk: &[u8; 32], label: &[u8], h: &[u8; 32]) -> [u8; 32] {
    let mut info = label.to_vec();
    info.extend_from_slice(h);
    let mut k = [0u8; 32];
    hkdf::expand(prk, &info, &mut k).unwrap();
    k
}

struct Model {
    cid: u64,
    initial: Vec<u8>,
    response: Vec<u8>,
    h1: [u8; 32],
    k_c2s: [u8; 32],
}

fn model(
    server_secret: [u8; 32],
    client_seed: [u8; 32],
    server_seed: [u8; 32],
    cc: &Config,
    sc: &Config,
) -> Model {
    let s_pub = x25519::public_key(&server_secret);
    let mut rng = ChaChaRng::from_seed(client_seed);
    let mut e_c = [0u8; 32];
    rng.fill(&mut e_c);
    let cid = rng.next_u64();
    let ec_pub = x25519::public_key(&e_c);
    let mut h = sha256::Sha256::new();
    h.update(b"xenon/1");
    h.update(&s_pub);
    h.update(&ec_pub);
    let h0 = h.finalize();
    let es = x25519::shared_secret(&e_c, &s_pub).unwrap();
    let prk1 = hkdf::extract(b"xenon/1", &es);
    let k_init = expand(&prk1, b"init", &h0);
    let hdr = header(1, cid, 0);
    let mut aad = hdr.clone();
    aad.extend_from_slice(&ec_pub);
    let mut pt = params(cc);
    pt.resize(1134, 0);
    let mut initial = aad.clone();
    initial.extend_from_slice(&aead::seal(&k_init, &nonce(0), &aad, &pt));
    assert_eq!(initial.len(), MAX_DATAGRAM);

    let mut srng = ChaChaRng::from_seed(server_seed);
    let mut e_s = [0u8; 32];
    srng.fill(&mut e_s);
    let es_pub = x25519::public_key(&e_s);
    let ee = x25519::shared_secret(&e_s, &ec_pub).unwrap();
    let mut h = sha256::Sha256::new();
    h.update(&h0);
    h.update(&initial);
    h.update(&es_pub);
    let h1 = h.finalize();
    let prk2 = hkdf::extract(&prk1, &ee);
    let k_c2s = expand(&prk2, b"c2s", &h1);
    let k_s2c = expand(&prk2, b"s2c", &h1);
    let mut aad = header(2, cid, 0);
    aad.extend_from_slice(&es_pub);
    let mut response = aad.clone();
    response.extend_from_slice(&aead::seal(&k_s2c, &nonce(0), &aad, &params(sc)));
    Model {
        cid,
        initial,
        response,
        h1,
        k_c2s,
    }
}

#[test]
fn leader_handshake_known_answer() {
    let custom = Config {
        max_streams: 7,
        stream_window: 1 << 20,
        conn_window: 300,
        idle_timeout_ms: 1234,
        max_half_open: 3,
        max_connections: 256,
    };
    for (cc, sc) in [
        (Config::default(), Config::default()),
        (custom, Config::default()),
        (Config::default(), custom),
    ] {
        let (secret, cseed, sseed) = ([0x22; 32], [0x11; 32], [0x33; 32]);
        let m = model(secret, cseed, sseed, &cc, &sc);
        let mut server = Server::new(secret, sseed, sc);
        assert_eq!(server.public_key(), x25519::public_key(&secret));
        let mut client = Connection::connect(server.public_key(), cseed, cc, 0);
        assert_eq!(client.cid(), m.cid);
        let init = client.poll_transmit(0).unwrap();
        assert!(init == m.initial, "Initial differs from the model");
        assert_eq!(server.handle(&init, 1000), Some(m.cid));
        let resp = server
            .connection(m.cid)
            .unwrap()
            .poll_transmit(1000)
            .unwrap();
        assert!(resp == m.response, "Response differs from the model");
        assert_eq!(
            server.accept(),
            None,
            "server connected before the client's first Data"
        );
        client.handle(&resp, 2000);
        assert!(client.is_connected());
        assert_eq!(client.handshake_hash(), Some(m.h1));
        let data = client.poll_transmit(2000).unwrap();
        assert_eq!(
            &data[..18],
            &header(3, m.cid, 1)[..],
            "client's first Data header"
        );
        let pt = aead::open(&m.k_c2s, &nonce(1), &data[..18], &data[18..]).unwrap();
        assert_eq!(&pt[..2], &[2, 0], "first frame is an ACK of server pn 0");
        assert_eq!(server.handle(&data, 3000), Some(m.cid));
        assert_eq!(server.accept(), Some(m.cid));
        assert_eq!(server.accept(), None);
        assert_eq!(
            server.connection(m.cid).unwrap().handshake_hash(),
            Some(m.h1)
        );
        // A repeat Initial gets the same Response again.
        assert_eq!(server.handle(&init, 4000), Some(m.cid));
        let again = std::iter::from_fn(|| server.connection(m.cid).unwrap().poll_transmit(4000))
            .find(|d| d[0] == 2);
        assert_eq!(
            again.as_deref(),
            Some(&m.response[..]),
            "repeat Initial gets the same Response"
        );
    }
}

fn run_timers(c: &mut Connection, until: u64, sent: &mut Vec<Vec<u8>>) {
    let mut now = 0;
    for _ in 0..1000 {
        while let Some(d) = c.poll_transmit(now) {
            sent.push(d);
        }
        match c.timeout() {
            Some(t) if t <= until && !c.is_closed() => {
                now = now.max(t);
                c.on_timeout(now);
            }
            _ => return,
        }
    }
    panic!("timer loop did not settle");
}

#[test]
fn leader_wrong_key_short_initial_and_resends() {
    let mut server = Server::new([0x22; 32], [0x33; 32], Config::default());
    let wrong = x25519::public_key(&[0x23; 32]);
    let mut client = Connection::connect(wrong, [0x11; 32], Config::default(), 0);
    let mut sent = Vec::new();
    run_timers(&mut client, 600 * SEC, &mut sent);
    assert_eq!(sent.len(), 6, "6 Initials, then give up");
    assert!(
        sent.windows(2).all(|w| w[0] == w[1]),
        "resends are identical"
    );
    for d in &sent {
        assert_eq!(
            server.handle(d, 0),
            None,
            "Initial under a wrong key must be dropped"
        );
    }
    assert!(server.cids().is_empty());
    assert!(!client.is_connected() && client.handshake_hash().is_none());
    assert_eq!(client.error(), Some(Error::Timeout));

    let mut client = Connection::connect(server.public_key(), [0x12; 32], Config::default(), 0);
    let init = client.poll_transmit(0).unwrap();
    assert_eq!(server.handle(&init[..1199], 0), None, "short Initial");
    let mut padded = init.clone();
    padded.push(0);
    assert_eq!(server.handle(&padded, 0), None, "1201-byte Initial");
    let mut flipped = init.clone();
    flipped[600] ^= 1;
    assert_eq!(server.handle(&flipped, 0), None, "corrupt Initial");
    assert!(server.cids().is_empty());
    // Half-open cap.
    let mut small = Server::new(
        [0x22; 32],
        [0x34; 32],
        Config {
            max_half_open: 2,
            max_connections: 256,
            ..Config::default()
        },
    );
    for i in 0..4u8 {
        let mut c = Connection::connect(small.public_key(), [0x40 + i; 32], Config::default(), 0);
        let got = small.handle(&c.poll_transmit(0).unwrap(), 0);
        assert_eq!(got.is_some(), i < 2, "half-open cap, Initial {i}");
    }
    small.on_timeout(11 * SEC);
    assert!(
        small.cids().is_empty(),
        "half-open connections expire after 10 s"
    );
    let mut c = Connection::connect(small.public_key(), [0x50; 32], Config::default(), 11 * SEC);
    assert!(
        small
            .handle(&c.poll_transmit(11 * SEC).unwrap(), 11 * SEC)
            .is_some(),
        "room again after expiry"
    );
}

/// Lossy channel: (deliver time, to_server, bytes).
struct Net {
    r: Rng,
    q: Vec<(u64, bool, Vec<u8>)>,
    drop: usize,
    dup: usize,
    corrupt: usize,
    reorder: usize,
}
impl Net {
    fn send(&mut self, now: u64, to_server: bool, d: Vec<u8>) {
        assert!(d.len() <= MAX_DATAGRAM, "datagram of {} bytes", d.len());
        for copy in 0..2 {
            if copy == 1 && self.r.below(100) >= self.dup {
                break;
            }
            if self.r.below(100) < self.drop {
                continue;
            }
            let mut d = d.clone();
            if self.r.below(100) < self.corrupt && !d.is_empty() {
                let i = self.r.below(d.len());
                d[i] ^= 1 << self.r.below(8);
            }
            let extra = if self.r.below(100) < self.reorder {
                self.r.below(60) as u64 * MS
            } else {
                0
            };
            self.q.push((now + 20 * MS + extra, to_server, d));
        }
    }
    fn next(&self) -> Option<u64> {
        self.q.iter().map(|e| e.0).min()
    }
    fn due(&mut self, now: u64) -> Vec<(bool, Vec<u8>)> {
        let (due, keep): (Vec<_>, Vec<_>) = self.q.drain(..).partition(|e| e.0 <= now);
        self.q = keep;
        let mut due = due;
        due.sort_by_key(|e| e.0);
        due.into_iter().map(|e| (e.1, e.2)).collect()
    }
}

// `before` is bytes in flight before this round's sends: a window cut may leave more in flight
// than the new window (doc section 6 limits sending, not acks), so only growth is gated. After a
// PTO expiry the doc (section 5) allows one or two probe packets on top of the window.
fn invariants(c: &Connection, before: u64, pto: bool, who: &str) {
    let s = c.stats();
    let slack = if pto { 3 * 1200 } else { 1200 };
    if c.is_connected() {
        assert!(
            s.bytes_in_flight <= before || s.bytes_in_flight <= s.cwnd + slack,
            "{who}: {} in flight, cwnd {}",
            s.bytes_in_flight,
            s.cwnd
        );
        assert!(s.cwnd >= 2400, "{who}: cwnd {}", s.cwnd);
    }
    if let Some(e) = c.error() {
        assert!(
            !matches!(
                e,
                Error::Protocol(_) | Error::FlowControl | Error::StreamLimit
            ),
            "{who}: {e:?} on a channel that only drops, delays, duplicates and corrupts"
        );
    }
}

/// Outcome of one simulated echo session: Ok(sim time) when every byte came back, Err(reason).
fn session(seed: u64, loss: usize) -> Result<u64, String> {
    let mut r = Rng::new(seed);
    let mut net = Net {
        r: Rng::new(seed ^ 0x5eed),
        q: Vec::new(),
        drop: loss,
        dup: loss,
        corrupt: loss,
        reorder: loss,
    };
    let mut server = Server::new([0x22; 32], [0x33; 32], Config::default());
    let mut cseed = [0u8; 32];
    r.fill(&mut cseed);
    let mut client = Connection::connect(server.public_key(), cseed, Config::default(), 0);
    let cid = client.cid();
    let sizes = [r.below(200_000), r.below(20_000), 0];
    let sends: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&n| {
            let mut v = vec![0u8; n];
            r.fill(&mut v);
            v
        })
        .collect();
    let mut srv_data = vec![0u8; r.below(60_000)];
    r.fill(&mut srv_data);
    // client state
    let mut ids: Vec<u64> = Vec::new();
    let mut written = [0usize; 3];
    let mut echoed: Vec<Vec<u8>> = vec![Vec::new(); 3];
    let mut srv_stream: Option<u64> = None;
    let mut srv_got = Vec::new();
    // server state
    let mut accepted = false;
    let mut echo: BTreeMap<u64, (VecDeque<u8>, bool)> = BTreeMap::new();
    let mut srv_open: Option<(u64, usize)> = None;
    let mut buf = vec![0u8; 8192];
    let mut now = 0u64;
    let (mut client_pto, mut server_pto) = (false, false);
    for _ in 0..400_000 {
        // Client application.
        if client.is_connected() {
            if ids.is_empty() {
                for _ in 0..3 {
                    ids.push(client.open_stream().map_err(|e| format!("open: {e:?}"))?);
                }
            }
            for i in 0..3 {
                if written[i] < sends[i].len() {
                    written[i] += client
                        .write(ids[i], &sends[i][written[i]..])
                        .map_err(|e| format!("write: {e:?}"))?;
                    if written[i] == sends[i].len() {
                        client.finish(ids[i]).unwrap();
                    }
                } else if written[i] == 0 && sends[i].is_empty() {
                    client.finish(ids[i]).unwrap();
                    written[i] = usize::MAX;
                }
                loop {
                    let n = client
                        .read(ids[i], &mut buf)
                        .map_err(|e| format!("read: {e:?}"))?;
                    if n == 0 {
                        break;
                    }
                    echoed[i].extend_from_slice(&buf[..n]);
                }
                if !sends[i].starts_with(&echoed[i]) {
                    panic!("seed {seed:#x} loss {loss}: stream {i} echoed wrong bytes");
                }
            }
            if srv_stream.is_none() {
                srv_stream = client.accept_stream();
            }
            if let Some(id) = srv_stream {
                loop {
                    let n = client
                        .read(id, &mut buf)
                        .map_err(|e| format!("read: {e:?}"))?;
                    if n == 0 {
                        break;
                    }
                    srv_got.extend_from_slice(&buf[..n]);
                }
                assert!(
                    srv_data.starts_with(&srv_got),
                    "seed {seed:#x}: server stream wrong bytes"
                );
            }
            let done = (0..3).all(|i| client.stream_finished(ids[i]) && echoed[i] == sends[i])
                && srv_stream.is_some_and(|id| client.stream_finished(id))
                && srv_got == srv_data;
            if done {
                return Ok(now);
            }
        }
        // Server application.
        while let Some(c) = server.accept() {
            assert_eq!(c, cid);
            accepted = true;
        }
        if accepted {
            if let Some(conn) = server.connection(cid) {
                while let Some(id) = conn.accept_stream() {
                    echo.insert(id, (VecDeque::new(), false));
                }
                for (&id, (pending, fin_sent)) in echo.iter_mut() {
                    loop {
                        let n = conn
                            .read(id, &mut buf)
                            .map_err(|e| format!("server read: {e:?}"))?;
                        if n == 0 {
                            break;
                        }
                        pending.extend(&buf[..n]);
                    }
                    let (a, _) = pending.as_slices();
                    if !a.is_empty() {
                        let n = conn
                            .write(id, a)
                            .map_err(|e| format!("server write: {e:?}"))?;
                        pending.drain(..n);
                    }
                    if pending.is_empty() && !*fin_sent && conn.stream_finished(id) {
                        conn.finish(id).unwrap();
                        *fin_sent = true;
                    }
                }
                if srv_open.is_none() {
                    srv_open = Some((
                        conn.open_stream()
                            .map_err(|e| format!("server open: {e:?}"))?,
                        0,
                    ));
                }
                if let Some((id, w)) = srv_open.as_mut() {
                    if *w < srv_data.len() {
                        *w += conn
                            .write(*id, &srv_data[*w..])
                            .map_err(|e| format!("server write: {e:?}"))?;
                        if *w == srv_data.len() {
                            conn.finish(*id).unwrap();
                        }
                    } else if srv_data.is_empty() && *w == 0 {
                        conn.finish(*id).unwrap();
                        *w = 1;
                    }
                }
            }
        }
        // Transmit.
        let client_before = client.stats().bytes_in_flight;
        let server_before = server
            .connection(cid)
            .map_or(0, |c| c.stats().bytes_in_flight);
        for _ in 0..64 {
            match client.poll_transmit(now) {
                Some(d) => net.send(now, true, d),
                None => break,
            }
        }
        for c in server.cids() {
            for _ in 0..64 {
                match server.connection(c).unwrap().poll_transmit(now) {
                    Some(d) => net.send(now, false, d),
                    None => break,
                }
            }
        }
        invariants(&client, client_before, client_pto, "client");
        if let Some(conn) = server.connection(cid) {
            invariants(conn, server_before, server_pto, "server");
        }
        (client_pto, server_pto) = (false, false);
        if let Some(e) = client.error() {
            return Err(format!("client closed: {e:?}"));
        }
        // Advance time to the next event.
        let next = [net.next(), client.timeout(), server.timeout()]
            .into_iter()
            .flatten()
            .min();
        let Some(next) = next else {
            return Err("stalled: no packet in flight and no timer".into());
        };
        now = now.max(next);
        if now > 600 * SEC {
            return Err("not done after 600 s of simulated time".into());
        }
        for (to_server, d) in net.due(now) {
            if to_server {
                server.handle(&d, now);
            } else {
                client.handle(&d, now);
            }
        }
        if client.timeout().is_some_and(|t| t <= now) {
            client.on_timeout(now);
            client_pto = true;
        }
        if server.timeout().is_some_and(|t| t <= now) {
            server.on_timeout(now);
            server_pto = true;
        }
    }
    Err("step limit".into())
}

#[test]
fn leader_lossy_channel_simulator() {
    for (loss, seeds, must_finish) in [(0usize, 4u64, 4usize), (5, 6, 6), (15, 6, 6), (30, 8, 3)] {
        let mut finished = 0;
        for s in 0..seeds {
            match session(0x34_51e0_0000 + (loss as u64) * 0x100 + s, loss) {
                Ok(_) => finished += 1,
                Err(e) if loss < 30 => panic!("loss {loss}% seed {s}: {e}"),
                Err(_) => {}
            }
        }
        assert!(
            finished >= must_finish,
            "loss {loss}%: only {finished}/{seeds} sessions finished"
        );
    }
}

/// Connected client and server through a perfect channel, plus every datagram delivered so far.
fn pair(cc: Config, sc: Config) -> (Connection, Server, u64, u64) {
    let mut server = Server::new([0x22; 32], [0x33; 32], sc);
    let mut client = Connection::connect(server.public_key(), [0x11; 32], cc, 0);
    let cid = client.cid();
    let mut now = 0;
    for _ in 0..20 {
        now += MS;
        while let Some(d) = client.poll_transmit(now) {
            server.handle(&d, now);
        }
        if let Some(c) = server.connection(cid) {
            while let Some(d) = c.poll_transmit(now) {
                client.handle(&d, now);
            }
        }
    }
    assert!(client.is_connected() && server.accept() == Some(cid));
    (client, server, cid, now)
}

#[test]
fn leader_replay_and_corruption_change_nothing() {
    let (mut client, mut server, cid, mut now) = pair(Config::default(), Config::default());
    let id = client.open_stream().unwrap();
    client.write(id, b"hello").unwrap();
    now += MS;
    let pkt = client.poll_transmit(now).unwrap();
    assert_eq!(server.handle(&pkt, now), Some(cid));
    let conn = server.connection(cid).unwrap();
    let before = format!("{conn:?}");
    let _ = server.handle(&pkt, now);
    assert_eq!(
        format!("{:?}", server.connection(cid).unwrap()),
        before,
        "replayed packet changed state"
    );
    client.write(id, b" world").unwrap();
    let pkt2 = client.poll_transmit(now).unwrap();
    let mut r = Rng::new(0x34_f11e_0001);
    for _ in 0..200 {
        let mut bad = pkt2.clone();
        let i = r.below(bad.len());
        bad[i] ^= 1 << r.below(8);
        if (2..10).contains(&i) {
            continue; // cid bytes: goes to no connection
        }
        let _ = server.handle(&bad, now);
        assert_eq!(
            format!("{:?}", server.connection(cid).unwrap()),
            before,
            "bit flip at {i} changed state"
        );
    }
    let mut other = pkt2.clone();
    other[2] ^= 0x55;
    assert_eq!(server.handle(&other, now), None, "unknown cid");
    let mut version = pkt2.clone();
    version[1] = 2;
    let _ = server.handle(&version, now);
    assert_eq!(
        format!("{:?}", server.connection(cid).unwrap()),
        before,
        "wrong version changed state"
    );
    assert_eq!(server.handle(&pkt2, now), Some(cid));
    let conn = server.connection(cid).unwrap();
    let s = conn.accept_stream().unwrap();
    assert_eq!(s, id);
    let mut buf = [0u8; 64];
    let n = conn.read(s, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello world");
    // Client side: a replayed server packet.
    let back = conn.poll_transmit(now).or_else(|| {
        conn.write(s, b"x").unwrap();
        conn.poll_transmit(now)
    });
    let back = back.unwrap();
    client.handle(&back, now);
    let before = format!("{client:?}");
    client.handle(&back, now);
    assert_eq!(
        format!("{client:?}"),
        before,
        "client: replay changed state"
    );
}

#[test]
fn leader_limits_close_and_idle() {
    let small = Config {
        max_streams: 2,
        ..Config::default()
    };
    let (mut client, mut server, cid, mut now) = pair(Config::default(), small);
    client.open_stream().unwrap();
    client.open_stream().unwrap();
    assert_eq!(client.open_stream(), Err(Error::StreamLimit));
    assert_eq!(
        client.write(1_000_001, b"x"),
        Err(Error::Protocol("unknown stream"))
    );
    client.close(4, "bye");
    for _ in 0..10 {
        now += MS;
        while let Some(d) = client.poll_transmit(now) {
            server.handle(&d, now);
        }
    }
    let want = Some(Error::Closed {
        code: 4,
        reason: "bye".into(),
    });
    assert_eq!(
        server.connection(cid).map(|c| c.error()),
        Some(want.clone()),
        "peer sees the close"
    );
    assert_eq!(client.error(), want);

    // Idle timeout: the smaller of the two params, no packet sent.
    let (mut client, mut server, cid, now) = pair(
        Config {
            idle_timeout_ms: 2000,
            ..Config::default()
        },
        Config::default(),
    );
    let mut t = now;
    let mut sent = 0;
    while !client.is_closed() {
        let next = client
            .timeout()
            .expect("an open connection always has a timer");
        assert!(next >= t);
        t = next;
        client.on_timeout(t);
        while client.poll_transmit(t).is_some() {
            sent += 1;
        }
        assert!(t < now + 10 * SEC, "idle timeout did not fire");
    }
    assert!(
        t >= now + 2 * SEC - 30 * MS && t <= now + 3 * SEC,
        "idle at {t}"
    );
    assert_eq!(client.error(), Some(Error::Timeout));
    let _ = sent;
    while server.cids().contains(&cid) {
        let st = server.timeout().expect("server timer");
        server.on_timeout(st);
        if let Some(c) = server.connection(cid) {
            while c.poll_transmit(st).is_some() {}
        }
        assert!(st < now + 10 * SEC, "server idle timeout did not fire");
    }
}

#[test]
fn leader_fuzz_never_panics() {
    let mut r = Rng::new(0x34_f022_0001);
    let (mut client, mut server, cid, now) = pair(Config::default(), Config::default());
    let id = client.open_stream().unwrap();
    client.write(id, &[7u8; 3000]).unwrap();
    let mut real = Vec::new();
    while let Some(d) = client.poll_transmit(now) {
        real.push(d);
    }
    let mut fresh = Connection::connect(server.public_key(), [0x66; 32], Config::default(), 0);
    for i in 0..3000 {
        let mut d = match r.below(4) {
            0 => r.bytes(1300),
            1 => {
                let mut d = real[r.below(real.len())].clone();
                for _ in 0..1 + r.below(4) {
                    let j = r.below(d.len());
                    d[j] = r.u64() as u8;
                }
                d
            }
            2 => {
                let mut d = header([1u8, 2, 3][r.below(3)], cid, r.u64() % 4);
                d.extend(r.bytes(1200));
                d
            }
            _ => {
                let d = &real[r.below(real.len())];
                d[..r.below(d.len())].to_vec()
            }
        };
        if i % 7 == 0 {
            d.truncate(r.below(30));
        }
        let _ = server.handle(&d, now);
        client.handle(&d, now);
        fresh.handle(&d, now);
    }
    // The real packets still work afterwards.
    for d in &real {
        server.handle(d, now);
    }
    let conn = server.connection(cid).unwrap();
    let s = conn.accept_stream().unwrap();
    let mut buf = vec![0u8; 4000];
    let mut got = 0;
    loop {
        let n = conn.read(s, &mut buf[got..]).unwrap();
        if n == 0 {
            break;
        }
        got += n;
    }
    assert_eq!(&buf[..got], &[7u8; 3000][..]);
}
