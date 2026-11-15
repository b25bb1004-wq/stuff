//! Leader check for T50b: peers gossip over xenon in a simulated lossy, duplicating, reordering
//! network with a partition that heals and a late joiner; every peer must end on the leader's own
//! fork-choice tip with the same state root. Garbage datagrams, garbage and oversize streams from
//! a fake peer, and determinism.
#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::needless_range_loop,
    clippy::type_complexity
)]
use assay::Rng;
use sphinx::ed25519;
use aurum::node::net::{Peer, PeerId, MAX_MSG, SYNC_INTERVAL_US};
use aurum::node::{Msg, Node};
use aurum::{Address, Block, Chain, Hash, Tx};
use std::collections::{BTreeSet, VecDeque};

fn seed(n: u8) -> [u8; 32] {
    [n; 32]
}

fn addr(n: u8) -> Address {
    ed25519::public_key(&seed(n))
}

const VALS: [u8; 3] = [1, 2, 3];
const USERS: [u8; 4] = [11, 12, 13, 14];

fn genesis() -> Vec<(Address, u64)> {
    USERS.iter().map(|&u| (addr(u), 1_000_000)).collect()
}

fn validators() -> Vec<Address> {
    VALS.iter().map(|&v| addr(v)).collect()
}

/// Valid blocks with their resulting chains, plus invalid blocks (bad state root twins and a
/// child of each).
fn universe(r: &mut Rng, n: usize) -> (Vec<(Block, Chain)>, Vec<Block>) {
    let g = Chain::new(&genesis(), &validators());
    let mut valid: Vec<(Block, Chain)> = Vec::new();
    let mut invalid = Vec::new();
    while valid.len() < n {
        let parent: Chain = if valid.is_empty() || r.chance(1, 12) {
            g.clone()
        } else {
            let lo = valid.len().saturating_sub(8);
            valid[lo + r.below(valid.len() - lo)].1.clone()
        };
        let who = parent.expected_proposer();
        let vs = *VALS.iter().find(|&&v| addr(v) == who).unwrap();
        let mut txs = Vec::new();
        for &u in USERS.iter() {
            if r.chance(1, 3) {
                let (_, nonce) = parent.account(&addr(u));
                let to = addr(*r.pick(&USERS));
                txs.push(Tx::signed(
                    &seed(u),
                    to,
                    1 + r.below(50) as u64,
                    1,
                    nonce,
                    vec![],
                ));
            }
        }
        let time = parent.tip().time + 1 + r.below(5) as u64;
        let b = parent.propose(&seed(vs), time, txs).unwrap();
        let mut c = parent.clone();
        c.append(b.clone()).unwrap();
        if r.chance(1, 10) {
            let mut bad = b.clone();
            bad.header.state_root[0] ^= 0x5a;
            bad.header.sign(&seed(vs));
            let mut child = bad.clone();
            child.txs.clear();
            child.header.parent = bad.hash();
            child.header.height += 1;
            child.header.time += 1;
            child.header.tx_root = aurum::merkle_root(&[]);
            let nxt = c.expected_proposer();
            let ns = *VALS.iter().find(|&&v| addr(v) == nxt).unwrap_or(&vs);
            child.header.proposer = addr(ns);
            child.header.sign(&seed(ns));
            invalid.push(bad);
            invalid.push(child);
        }
        valid.push((b, c));
    }
    (valid, invalid)
}

fn best(valid: &[(Block, Chain)]) -> (u64, Hash) {
    valid
        .iter()
        .map(|(b, _)| (b.header.height, b.hash()))
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
        .unwrap()
}

fn bad_parent(invalid: &[Block], p: &Hash) -> bool {
    invalid
        .iter()
        .any(|b| b.hash() == *p || b.header.parent == *p)
}

const LATE_T: u64 = 2_500_000;
const END: u64 = 9_000_000;

struct Sim {
    q: Vec<(u64, PeerId, PeerId, Vec<u8>)>, // deliver at, from, to, bytes
    now: u64,
    loss: u64, // per mille
    groups: Option<Vec<usize>>,
    heal_at: u64,
    sent: u64,
    capture: Vec<Vec<u8>>,
}

impl Sim {
    fn send(&mut self, r: &mut Rng, from: PeerId, to: PeerId, d: Vec<u8>) {
        self.sent += 1;
        if self.capture.len() < 300 && r.chance(1, 10) {
            self.capture.push(d.clone());
        }
        if let Some(g) = &self.groups {
            if self.now < self.heal_at && g[from as usize] != g[to as usize] {
                return;
            }
        }
        if (r.below(1000) as u64) < self.loss {
            return;
        }
        let copies = if r.chance(1, 50) { 2 } else { 1 };
        for _ in 0..copies {
            let at = self.now + 100 + r.below(15_000) as u64;
            self.q.push((at, from, to, d.clone()));
        }
    }
}

fn pump(sim: &mut Sim, r: &mut Rng, peers: &mut [Peer]) {
    for i in 0..peers.len() {
        let mut k = 0;
        while let Some((to, d)) = peers[i].poll_transmit(sim.now) {
            k += 1;
            assert!(k < 200_000, "peer {i}: poll_transmit never drains");
            assert!(
                (to as usize) < peers.len() && to as usize != i,
                "peer {i} sent to {to}"
            );
            sim.send(r, i as PeerId, to, d);
        }
    }
}

fn key(i: usize) -> [u8; 32] {
    [100 + i as u8; 32]
}

struct Run {
    peers: Vec<Peer>,
    sent: u64,
    capture: Vec<Vec<u8>>,
}

/// One simulated run; `inj` holds (time, peer, message) local injections.
fn simulate(
    r: &mut Rng,
    n: usize,
    mut inj: Vec<(u64, usize, Msg)>,
    partitioned: bool,
    late: Option<usize>,
    noise: bool,
) -> Run {
    let mut peers: Vec<Peer> = (0..n)
        .map(|i| {
            Peer::new(
                i as PeerId,
                Node::new(&genesis(), &validators()),
                key(i),
                [200 + i as u8; 32],
            )
        })
        .collect();
    let pubs: Vec<[u8; 32]> = peers.iter().map(Peer::public_key).collect();
    for i in 0..n {
        for j in 0..n {
            if i != j && late != Some(i) && late != Some(j) {
                peers[i].add_peer(j as PeerId, pubs[j], 0);
            }
        }
    }
    inj.sort_by_key(|e| e.0);
    let mut inj = VecDeque::from(inj);
    let mut sim = Sim {
        q: Vec::new(),
        now: 0,
        loss: 100,
        groups: partitioned.then(|| (0..n).map(|i| usize::from(i >= n / 2)).collect()),
        heal_at: 2_000_000,
        sent: 0,
        capture: Vec::new(),
    };
    let mut joined = late.is_none();
    pump(&mut sim, r, &mut peers);
    let mut steps = 0u64;
    loop {
        steps += 1;
        assert!(
            steps < 4_000_000,
            "simulation stuck at {} us (timer not advancing?)",
            sim.now
        );
        let tq = sim.q.iter().map(|e| e.0).min();
        let tp = peers.iter().filter_map(Peer::timeout).min();
        let ti = inj.front().map(|e| e.0);
        let tl = (!joined).then_some(LATE_T);
        let Some(next) = [tq, tp, ti, tl].into_iter().flatten().min() else {
            break;
        };
        if next > END {
            break;
        }
        sim.now = sim.now.max(next);
        let now = sim.now;
        while inj.front().is_some_and(|e| e.0 <= now) {
            let (_, at, m) = inj.pop_front().unwrap();
            peers[at].inject(m, now);
        }
        if !joined && now >= LATE_T {
            joined = true;
            let l = late.unwrap();
            for j in 0..n {
                if j != l {
                    peers[j].add_peer(l as PeerId, pubs[l], now);
                    peers[l].add_peer(j as PeerId, pubs[j], now);
                }
            }
        }
        for p in peers.iter_mut() {
            if p.timeout().is_some_and(|t| t <= now) {
                p.on_timeout(now);
            }
        }
        // deliver every datagram due now, ties in random order (reordering)
        loop {
            let due: Vec<usize> = (0..sim.q.len()).filter(|&i| sim.q[i].0 <= now).collect();
            if due.is_empty() {
                break;
            }
            let (_, from, to, d) = sim.q.swap_remove(*r.pick(&due));
            peers[to as usize].receive(from, &d, now);
        }
        if noise && r.chance(1, 40) {
            let to = r.below(n);
            let from = if r.chance(1, 4) {
                77
            } else {
                r.below(n) as PeerId
            };
            let mut g = if !sim.capture.is_empty() && r.chance(1, 2) {
                r.pick(&sim.capture).clone()
            } else {
                r.bytes(1300)
            };
            if !g.is_empty() {
                match r.below(3) {
                    0 => g.truncate(r.below(g.len())),
                    1 => {
                        let i = r.below(g.len());
                        g[i] ^= 1 << r.below(8);
                    }
                    _ => {}
                }
            }
            peers[to].receive(from, &g, now);
        }
        pump(&mut sim, r, &mut peers);
    }
    Run {
        peers,
        sent: sim.sent,
        capture: sim.capture,
    }
}

fn injections(
    r: &mut Rng,
    valid: &[(Block, Chain)],
    invalid: &[Block],
    n: usize,
    late: Option<usize>,
) -> Vec<(u64, usize, Msg)> {
    let mut v = Vec::new();
    for b in valid.iter().map(|x| &x.0).chain(invalid.iter()) {
        let mut at = r.below(n);
        if late == Some(at) {
            at = (at + 1) % n;
        }
        v.push((r.below(1_500_000) as u64, at, Msg::Block(b.clone())));
    }
    v
}

fn assert_converged(run: &Run, valid: &[(Block, Chain)], invalid: &[Block], tag: &str) {
    let (h, tip) = best(valid);
    let want_root = valid
        .iter()
        .find(|v| v.0.hash() == tip)
        .unwrap()
        .1
        .state_root();
    let bad: BTreeSet<Hash> = invalid.iter().map(Block::hash).collect();
    let n = run.peers.len();
    for (i, p) in run.peers.iter().enumerate() {
        let others: Vec<PeerId> = (0..n as PeerId).filter(|&j| j != i as PeerId).collect();
        assert_eq!(p.connected(), others, "{tag}: peer {i} links");
        assert_eq!(
            (p.node().chain().height(), p.node().tip()),
            (h, tip),
            "{tag}: peer {i} of {n} tip"
        );
        assert_eq!(
            p.node().chain().state_root(),
            want_root,
            "{tag}: peer {i} state"
        );
        assert!(!bad.contains(&p.node().tip()));
        assert!(
            p.node().missing().iter().all(|m| bad_parent(invalid, m)),
            "{tag}: peer {i} unresolved orphans"
        );
    }
}

#[test]
fn peers_converge_through_loss_partition_and_late_join() {
    let mut r = Rng::new(0x50B_0001);
    for case in 0..6 {
        let size = 30 + r.below(50);
        let (valid, invalid) = universe(&mut r, size);
        let n = 3 + r.below(4);
        // cover each feature at least twice over the six cases
        let partitioned = case % 2 == 0;
        let late = (case % 3 != 0).then(|| r.below(n));
        let noise = case >= 2;
        let inj = injections(&mut r, &valid, &invalid, n, late);
        let run = simulate(&mut r, n, inj, partitioned, late, noise);
        let tag =
            format!("case {case} n {n} partitioned {partitioned} late {late:?} noise {noise}");
        assert_converged(&run, &valid, &invalid, &tag);
        // sync keeps every link busy after the late join: one tip each way per interval at least
        let ticks = (END - LATE_T) / SYNC_INTERVAL_US;
        assert!(
            run.sent >= ticks * (n * (n - 1)) as u64,
            "{tag}: only {} datagrams in {ticks} sync intervals",
            run.sent
        );
        assert!(!run.capture.is_empty());
    }
}

#[test]
fn deterministic_runs() {
    let mut r = Rng::new(0x50B_0002);
    let (valid, invalid) = universe(&mut r, 40);
    let inj = injections(&mut r, &valid, &invalid, 4, Some(3));
    let s = r.u64();
    let a = simulate(&mut Rng::new(s), 4, inj.clone(), true, Some(3), true);
    let b = simulate(&mut Rng::new(s), 4, inj, true, Some(3), true);
    assert_eq!(a.sent, b.sent, "datagram counts differ");
    assert_eq!(a.capture, b.capture, "datagrams differ");
    for (x, y) in a.peers.iter().zip(&b.peers) {
        assert_eq!(x.node().tip(), y.node().tip());
    }
    assert_converged(&a, &valid, &invalid, "determinism run");
}

/// A fake peer (a raw xenon server with id 9, dialled by real peer 0) sends a garbage stream,
/// then a stream larger than MAX_MSG, then a valid block on the same link: the block must still
/// be accepted and the link must stay up.
#[test]
fn garbage_and_oversize_streams_are_dropped() {
    let mut r = Rng::new(0x50B_0003);
    let (valid, _) = universe(&mut r, 2);
    let first = valid[0].0.clone();
    let cfg = xenon::Config {
        max_streams: 1 << 40,
        ..xenon::Config::default()
    };
    let mut fake = xenon::Server::new([7; 32], [8; 32], cfg);
    let mut p = Peer::new(0, Node::new(&genesis(), &validators()), key(0), [200; 32]);
    p.add_peer(9, fake.public_key(), 0);
    let mut now = 0u64;
    let mut cid = None;
    let mut stage = 0;
    let (mut big, mut written, mut stalls) = (0u64, 0usize, 0u32);
    let chunk = vec![0xA5u8; 16 * 1024];
    for _ in 0..400_000 {
        now += 200;
        if p.timeout().is_some_and(|t| t <= now) {
            p.on_timeout(now);
        }
        if fake.timeout().is_some_and(|t| t <= now) {
            fake.on_timeout(now);
        }
        while let Some((to, d)) = p.poll_transmit(now) {
            assert_eq!(to, 9);
            if let Some(c) = fake.handle(&d, now) {
                cid.get_or_insert(c);
            }
        }
        while fake.accept().is_some() {}
        if let Some(conn) = cid.and_then(|c| fake.connection(c)) {
            if conn.is_connected() {
                match stage {
                    0 => {
                        let s = conn.open_stream().unwrap();
                        let g = r.bytes(5000);
                        conn.write(s, &g).unwrap();
                        conn.finish(s).unwrap();
                        stage = 1;
                    }
                    1 => {
                        big = conn.open_stream().unwrap();
                        stage = 2;
                    }
                    2 => match conn.write(big, &chunk) {
                        Ok(0) => {
                            stalls += 1;
                            if stalls > 20_000 {
                                stage = 3; // the peer stopped reading: it must have reset
                            }
                        }
                        Ok(k) => {
                            written += k;
                            stalls = 0;
                            if written > MAX_MSG + chunk.len() {
                                let _ = conn.finish(big);
                                stage = 3;
                            }
                        }
                        Err(_) => stage = 3, // reset by the peer
                    },
                    3 => {
                        let s = conn.open_stream().unwrap();
                        let e = Msg::Block(first.clone()).encode();
                        assert_eq!(conn.write(s, &e).unwrap(), e.len());
                        conn.finish(s).unwrap();
                        stage = 4;
                    }
                    _ => {}
                }
            }
            while let Some(d) = conn.poll_transmit(now) {
                p.receive(9, &d, now);
            }
        }
        if stage == 4 && p.node().known(&first.hash()) {
            break;
        }
    }
    assert_eq!(
        stage, 4,
        "fake link stuck at stage {stage} (written {written})"
    );
    assert!(
        p.node().known(&first.hash()),
        "valid block after garbage and oversize streams not accepted"
    );
    assert_eq!(p.node().tip(), first.hash());
    assert_eq!(p.connected(), vec![9]);
    // random datagrams from known and unknown ids never panic
    for _ in 0..5000 {
        let from = *r.pick(&[9, 3, 77]);
        let d = r.bytes(1500);
        p.receive(from, &d, now);
    }
}
