//! Leader check for T50a: a seeded simulated network (drops, delays, reordering, a partition
//! that heals) over a universe of forked blocks from equivocating validators plus invalid blocks;
//! after the network quiets every node must sit on the leader's own fork-choice tip.
#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::needless_range_loop
)]
use assay::Rng;
use aurum::node::{Msg, Node};
use aurum::{Address, Block, Chain, Hash, Tx};
use sphinx::ed25519;
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

/// Valid blocks with their resulting chains, plus invalid blocks.
fn universe(r: &mut Rng, n: usize) -> (Vec<(Block, Chain)>, Vec<Block>) {
    let g = Chain::new(&genesis(), &validators());
    let mut valid: Vec<(Block, Chain)> = Vec::new();
    let mut invalid = Vec::new();
    while valid.len() < n {
        // bias toward recent blocks so branches get long, but fork often
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
            // invalid twin (bad state root, properly signed) and a child of it
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

/// Orphans may wait on a parent nobody has only when that parent is invalid or descends from one.
fn bad_parent(invalid: &[Block], p: &Hash) -> bool {
    invalid
        .iter()
        .any(|b| b.hash() == *p || b.header.parent == *p)
}

struct Net {
    q: VecDeque<(u64, usize, usize, Msg)>, // deliver time, from, to, msg
    now: u64,
}

impl Net {
    fn send(
        &mut self,
        r: &mut Rng,
        from: usize,
        msgs: Vec<Msg>,
        n: usize,
        lossy: bool,
        groups: Option<&[usize]>,
    ) {
        for m in msgs {
            for to in 0..n {
                if to == from
                    || groups.is_some_and(|g| g[to] != g[from])
                    || (lossy && r.chance(1, 5))
                {
                    continue;
                }
                let at = self.now + if lossy { r.below(20) as u64 } else { 1 };
                self.q.push_back((at, from, to, m.clone()));
            }
        }
    }
    fn run(&mut self, r: &mut Rng, nodes: &mut [Node], lossy: bool, groups: Option<&[usize]>) {
        let mut steps = 0;
        while !self.q.is_empty() {
            steps += 1;
            assert!(steps < 2_000_000, "network never quiets");
            // deliver the earliest message; ties in random order (reordering)
            let t = self.q.iter().map(|m| m.0).min().unwrap();
            let idxs: Vec<usize> = (0..self.q.len()).filter(|&i| self.q[i].0 == t).collect();
            let (at, _from, to, m) = self.q.remove(*r.pick(&idxs)).unwrap();
            self.now = at;
            let out = nodes[to].handle(m);
            self.send(r, to, out, nodes.len(), lossy, groups);
        }
    }
}

#[test]
fn network_converges_on_fork_choice() {
    let mut r = Rng::new(0x50A_0001);
    for case in 0..12 {
        let size = 40 + r.below(60);
        let (valid, invalid) = universe(&mut r, size);
        let (h, tip) = best(&valid);
        let n = 3 + r.below(3);
        let mut nodes: Vec<Node> = (0..n)
            .map(|_| Node::new(&genesis(), &validators()))
            .collect();
        let mut all: Vec<Block> = valid
            .iter()
            .map(|v| v.0.clone())
            .chain(invalid.iter().cloned())
            .collect();
        // shuffle: children often arrive before parents
        for i in (1..all.len()).rev() {
            all.swap(i, r.below(i + 1));
        }
        let groups: Vec<usize> = (0..n).map(|i| usize::from(i >= n / 2)).collect();
        let mut net = Net {
            q: VecDeque::new(),
            now: 0,
        };
        let partitioned = r.chance(1, 2);
        for b in all {
            let at = r.below(n);
            let out = nodes[at].handle(Msg::Block(b));
            net.send(&mut r, at, out, n, true, partitioned.then_some(&groups));
            if r.chance(1, 3) {
                net.run(&mut r, &mut nodes, true, partitioned.then_some(&groups));
            }
        }
        net.run(&mut r, &mut nodes, true, partitioned.then_some(&groups));
        // heal: every node announces its tip on a lossless network, repeated until stable
        for _round in 0..4 {
            for i in 0..n {
                let mut m = vec![Msg::Tip {
                    hash: nodes[i].tip(),
                    height: nodes[i].chain().height(),
                }];
                m.extend(nodes[i].missing().into_iter().map(Msg::GetBlock));
                net.send(&mut r, i, m, n, false, None);
            }
            net.run(&mut r, &mut nodes, false, None);
        }
        assert!(
            nodes
                .iter()
                .all(|nd| nd.missing().iter().all(|p| bad_parent(&invalid, p))),
            "case {case}: unresolved orphans"
        );
        let bad: BTreeSet<Hash> = invalid.iter().map(Block::hash).collect();
        for (i, node) in nodes.iter().enumerate() {
            assert_eq!(
                (node.chain().height(), node.tip()),
                (h, tip),
                "case {case}: node {i} of {n} (partitioned {partitioned})"
            );
            assert!(!bad.contains(&node.tip()));
            assert_eq!(
                node.chain().tip().clone(),
                valid.iter().find(|v| v.0.hash() == tip).unwrap().0.header
            );
            let want_root = valid
                .iter()
                .find(|v| v.0.hash() == tip)
                .unwrap()
                .1
                .state_root();
            assert_eq!(
                node.chain().state_root(),
                want_root,
                "case {case}: node {i} state differs"
            );
        }
    }
}

#[test]
fn mempool_follows_the_chosen_chain() {
    let mut r = Rng::new(0x50A_0002);
    for case in 0..30 {
        let mut node = Node::new(&genesis(), &validators());
        let g = Chain::new(&genesis(), &validators());
        // txs into the mempool: one per user, correct nonce 0
        let txs: Vec<Tx> = USERS
            .iter()
            .map(|&u| Tx::signed(&seed(u), addr(11), 5 + u64::from(u), 1, 0, vec![]))
            .collect();
        for t in &txs {
            let out = node.handle(Msg::Tx(t.clone()));
            assert_eq!(out, vec![Msg::Tx(t.clone())], "new tx gossiped once");
            assert!(
                node.handle(Msg::Tx(t.clone())).is_empty(),
                "duplicate tx gossiped"
            );
        }
        assert_eq!(node.pending(), &txs[..]);
        // branch A at height 1 includes a random subset; branch B (2 blocks) includes none
        let pick: Vec<Tx> = txs.iter().filter(|_| r.chance(1, 2)).cloned().collect();
        let p0 = g.expected_proposer();
        let s0 = *VALS.iter().find(|&&v| addr(v) == p0).unwrap();
        let a1 = g.propose(&seed(s0), 5, pick.clone()).unwrap();
        node.handle(Msg::Block(a1.clone()));
        let rest: Vec<Tx> = txs.iter().filter(|t| !pick.contains(t)).cloned().collect();
        assert_eq!(node.pending(), &rest[..], "case {case}: mempool after A");
        let b1 = g.propose(&seed(s0), 6, vec![]).unwrap();
        let mut gb = g.clone();
        gb.append(b1.clone()).unwrap();
        let p1 = gb.expected_proposer();
        let s1 = *VALS.iter().find(|&&v| addr(v) == p1).unwrap();
        let b2 = gb.propose(&seed(s1), 7, vec![]).unwrap();
        node.handle(Msg::Block(b2.clone())); // orphan first
        node.handle(Msg::Block(b1));
        assert_eq!(node.tip(), b2.hash(), "case {case}: longer branch B wins");
        assert_eq!(node.reorgs(), 1, "case {case}: A to B is one reorg");
        let mut want = rest.clone();
        want.extend(pick.iter().cloned());
        let got: BTreeSet<Hash> = node.pending().iter().map(Tx::id).collect();
        assert_eq!(
            got,
            want.iter().map(Tx::id).collect::<BTreeSet<_>>(),
            "case {case}: abandoned txs re-added"
        );
        let prop = node
            .propose(
                &seed(
                    VALS.iter()
                        .copied()
                        .find(|&v| addr(v) == node.chain().expected_proposer())
                        .unwrap(),
                ),
                9,
            )
            .unwrap();
        assert_eq!(
            prop.txs.len(),
            txs.len(),
            "case {case}: proposal takes the whole mempool"
        );
        assert!(node.pending().is_empty());
        assert_eq!(node.tip(), prop.hash());
    }
}

#[test]
fn msg_codec_round_trips_and_hostile_bytes() {
    let mut r = Rng::new(0x50A_0003);
    let (valid, _) = universe(&mut r, 10);
    let mut msgs = vec![
        Msg::GetBlock([7; 32]),
        Msg::Tip {
            hash: [9; 32],
            height: u64::MAX,
        },
    ];
    for (b, _) in &valid {
        msgs.push(Msg::Block(b.clone()));
        msgs.extend(b.txs.iter().cloned().map(Msg::Tx));
    }
    for m in &msgs {
        let e = m.encode();
        assert_eq!(Msg::decode(&e).as_ref(), Ok(m));
        let mut extra = e.clone();
        extra.push(0);
        assert!(Msg::decode(&extra).is_err(), "trailing byte accepted");
        assert!(
            Msg::decode(&e[..r.below(e.len())]).is_err(),
            "truncation accepted"
        );
        for _ in 0..20 {
            let mut x = e.clone();
            let i = r.below(x.len());
            x[i] ^= 1 << r.below(8);
            let _ = Msg::decode(&x);
        }
    }
    for _ in 0..20_000 {
        let _ = Msg::decode(&r.bytes(300));
    }
    // a node fed garbage-derived messages never panics
    let mut node = Node::new(&genesis(), &validators());
    for _ in 0..5000 {
        if let Ok(m) = Msg::decode(&r.bytes(300)) {
            let _ = node.handle(m);
        }
    }
}
