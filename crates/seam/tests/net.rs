//! Integration tests for `seam::node::net` gossip and sync over simulated Tremor links.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use onyx::ed25519;
use seam::node::net::{Peer, PeerId, SYNC_INTERVAL_US};
use seam::node::{Msg, Node};
use seam::{Address, Block, Chain, Hash, Tx};
use std::collections::BTreeMap;

fn seed_u8(n: u8) -> [u8; 32] {
    [n; 32]
}

fn addr(n: u8) -> Address {
    ed25519::public_key(&seed_u8(n))
}

#[derive(Clone, Copy)]
struct NetProfile {
    loss_num: u64,
    loss_den: u64,
    delay_min: u64,
    delay_span: u64,
}

struct SimNet {
    peers: BTreeMap<PeerId, Peer>,
    in_flight: BTreeMap<(u64, u64), (PeerId, PeerId, Vec<u8>)>,
    seq: u64,
    now: u64,
    rng: Rng,
    profile: NetProfile,
    sent_counts: BTreeMap<PeerId, usize>,
    partitioned: Option<(Vec<PeerId>, Vec<PeerId>)>,
}

impl SimNet {
    fn new(seed: u64, profile: NetProfile) -> Self {
        Self {
            peers: BTreeMap::new(),
            in_flight: BTreeMap::new(),
            seq: 0,
            now: 0,
            rng: Rng::new(seed),
            profile,
            sent_counts: BTreeMap::new(),
            partitioned: None,
        }
    }

    fn add_peer_full_mesh(&mut self, id: PeerId, peer: Peer) {
        let new_key = peer.public_key();
        let existing: Vec<(PeerId, [u8; 32])> = self
            .peers
            .iter()
            .map(|(&pid, p)| (pid, p.public_key()))
            .collect();
        let mut peer = peer;
        for (pid, pkey) in &existing {
            peer.add_peer(*pid, *pkey, self.now);
        }
        for (&_, p) in &mut self.peers {
            p.add_peer(id, new_key, self.now);
        }
        self.peers.insert(id, peer);
        self.sent_counts.entry(id).or_insert(0);
    }

    fn is_blocked(&self, a: PeerId, b: PeerId) -> bool {
        let Some((side_a, side_b)) = &self.partitioned else {
            return false;
        };
        (side_a.contains(&a) && side_b.contains(&b)) || (side_b.contains(&a) && side_a.contains(&b))
    }

    fn drain_transmits(&mut self) {
        let ids: Vec<PeerId> = self.peers.keys().copied().collect();
        for from in ids {
            while let Some((to, dgram)) = self.peers.get_mut(&from).unwrap().poll_transmit(self.now)
            {
                *self.sent_counts.entry(from).or_insert(0) += 1;
                if self.is_blocked(from, to) {
                    continue;
                }
                if self.profile.loss_den > 0
                    && self
                        .rng
                        .chance(self.profile.loss_num, self.profile.loss_den)
                {
                    continue;
                }
                let delay = self.profile.delay_min + self.rng.range(0, self.profile.delay_span);
                let deliver_at = self.now.saturating_add(delay);
                let seq = self.seq;
                self.seq += 1;
                self.in_flight.insert((deliver_at, seq), (from, to, dgram));
            }
        }
    }

    fn run_until(&mut self, until: u64) {
        self.drain_transmits();
        loop {
            let next_pkt = self.in_flight.keys().next().map(|&(t, _)| t);
            let next_timer = self.peers.values().filter_map(Peer::timeout).min();
            let next_t = [next_pkt, next_timer].into_iter().flatten().min();
            let Some(t) = next_t else {
                break;
            };
            if t > until {
                break;
            }
            self.now = self.now.max(t);

            while let Some((&(deliver_at, _), _)) = self.in_flight.first_key_value() {
                if deliver_at > self.now {
                    break;
                }
                let (_, (from, to, dgram)) = self.in_flight.pop_first().unwrap();
                if let Some(peer) = self.peers.get_mut(&to) {
                    peer.receive(from, &dgram, self.now);
                }
            }

            let ids: Vec<PeerId> = self.peers.keys().copied().collect();
            for id in ids {
                if let Some(peer) = self.peers.get_mut(&id) {
                    if peer.timeout().is_some_and(|pt| pt <= self.now) {
                        peer.on_timeout(self.now);
                    }
                }
            }

            self.drain_transmits();
        }
        self.now = self.now.max(until);
    }

    fn all_converged(&self, expected_tip: Hash, expected_state_root: Hash) -> bool {
        self.peers.values().all(|p| {
            p.node().tip() == expected_tip && p.node().chain().state_root() == expected_state_root
        })
    }
}

fn make_genesis() -> (Vec<(Address, u64)>, Vec<Address>) {
    let genesis = vec![(addr(1), 100_000), (addr(2), 100_000), (addr(3), 100_000)];
    let validators = vec![addr(1), addr(2), addr(3)];
    (genesis, validators)
}

fn make_forked_chains(
    genesis: &[(Address, u64)],
    validators: &[Address],
) -> (Vec<Block>, Vec<Block>, Chain) {
    // Losing fork: height 2
    let mut fork_a = Chain::new(genesis, validators);
    let tx_a1 = Tx::signed(&seed_u8(1), addr(2), 50, 1, 0, b"forkA".to_vec());
    let b1_a = fork_a.propose(&seed_u8(2), 10, vec![tx_a1]).unwrap();
    fork_a.append(b1_a.clone()).unwrap();
    let b2_a = fork_a.propose(&seed_u8(3), 20, vec![]).unwrap();
    fork_a.append(b2_a.clone()).unwrap();

    // Winning fork: height 4
    let mut fork_b = Chain::new(genesis, validators);
    let tx_b1 = Tx::signed(&seed_u8(1), addr(3), 75, 2, 0, b"forkB1".to_vec());
    let b1_b = fork_b.propose(&seed_u8(2), 12, vec![tx_b1]).unwrap();
    fork_b.append(b1_b.clone()).unwrap();
    let tx_b2 = Tx::signed(&seed_u8(2), addr(1), 30, 1, 0, b"forkB2".to_vec());
    let b2_b = fork_b.propose(&seed_u8(3), 22, vec![tx_b2]).unwrap();
    fork_b.append(b2_b.clone()).unwrap();
    let b3_b = fork_b.propose(&seed_u8(1), 32, vec![]).unwrap();
    fork_b.append(b3_b.clone()).unwrap();
    let b4_b = fork_b.propose(&seed_u8(2), 42, vec![]).unwrap();
    fork_b.append(b4_b.clone()).unwrap();

    (vec![b1_a, b2_a], vec![b1_b, b2_b, b3_b, b4_b], fork_b)
}

fn build_network(peer_ids: &[PeerId], sim_seed: u64, profile: NetProfile) -> SimNet {
    let (genesis, validators) = make_genesis();
    let mut net = SimNet::new(sim_seed, profile);
    for (idx, &pid) in peer_ids.iter().enumerate() {
        let s = (idx as u8) + 10;
        let peer = Peer::new(
            pid,
            Node::new(&genesis, &validators),
            seed_u8(s),
            seed_u8(s + 50),
        );
        net.add_peer_full_mesh(pid, peer);
    }
    net
}

#[test]
fn forked_blocks_converge_under_loss_delay_and_reordering() {
    let (genesis, validators) = make_genesis();
    let (fork_a, fork_b, winner) = make_forked_chains(&genesis, &validators);
    let peer_ids = [10, 20, 30, 40, 50];
    let profile = NetProfile {
        loss_num: 1,
        loss_den: 5, // 20% packet loss
        delay_min: 1_000,
        delay_span: 40_000, // high jitter => packet reordering
    };
    let mut net = build_network(&peer_ids, 0xBEEF_1001, profile);

    // Let peers handshake partially or inject immediately before/after handshake.
    net.run_until(50_000);

    // Inject competing fork blocks across random peers, including out-of-order orphans.
    let mut rng = Rng::new(0xCAFE_1234);
    for blk in &fork_a {
        let &pid = rng.pick(&peer_ids);
        net.peers
            .get_mut(&pid)
            .unwrap()
            .inject(Msg::Block(blk.clone()), net.now);
    }
    for blk in fork_b.iter().rev() {
        let &pid = rng.pick(&peer_ids);
        net.peers
            .get_mut(&pid)
            .unwrap()
            .inject(Msg::Block(blk.clone()), net.now);
    }

    net.run_until(4_000_000);

    assert!(net.all_converged(winner.tip().hash(), winner.state_root()));
    for &pid in &peer_ids {
        assert_eq!(net.peers[&pid].connected().len(), peer_ids.len() - 1);
    }
}

#[test]
fn late_joining_peer_catches_up_via_sync() {
    let (genesis, validators) = make_genesis();
    let (_fork_a, fork_b, winner) = make_forked_chains(&genesis, &validators);
    let initial_ids = [1, 2, 3];
    let profile = NetProfile {
        loss_num: 1,
        loss_den: 10,
        delay_min: 2_000,
        delay_span: 20_000,
    };
    let mut net = build_network(&initial_ids, 0xBEEF_2002, profile);
    net.run_until(500_000);

    for blk in &fork_b {
        net.peers
            .get_mut(&1)
            .unwrap()
            .inject(Msg::Block(blk.clone()), net.now);
    }
    net.run_until(3_000_000);
    assert!(net.all_converged(winner.tip().hash(), winner.state_root()));

    // Add peer 4 late at t = 3s; it starts at genesis and must sync all 4 blocks.
    let late_peer = Peer::new(
        4,
        Node::new(&genesis, &validators),
        seed_u8(94),
        seed_u8(95),
    );
    net.add_peer_full_mesh(4, late_peer);
    net.run_until(6_000_000);

    assert!(net.all_converged(winner.tip().hash(), winner.state_root()));
    assert_eq!(net.peers[&4].node().chain().height(), 4);
}

#[test]
fn partition_heals_and_reorgs_to_winning_fork() {
    let (genesis, validators) = make_genesis();
    let (fork_a, fork_b, winner) = make_forked_chains(&genesis, &validators);
    let profile = NetProfile {
        loss_num: 1,
        loss_den: 10,
        delay_min: 1_000,
        delay_span: 15_000,
    };
    let mut net = build_network(&[1, 2, 3, 4], 0xBEEF_3003, profile);
    net.run_until(400_000);

    // Partition {1, 2} from {3, 4} for 2.5 seconds (longer than idle/handshake probes if any).
    net.partitioned = Some((vec![1, 2], vec![3, 4]));
    for blk in &fork_a {
        net.peers
            .get_mut(&1)
            .unwrap()
            .inject(Msg::Block(blk.clone()), net.now);
    }
    for blk in &fork_b {
        net.peers
            .get_mut(&3)
            .unwrap()
            .inject(Msg::Block(blk.clone()), net.now);
    }
    net.run_until(3_000_000);
    assert_eq!(net.peers[&1].node().chain().height(), 2);
    assert_eq!(net.peers[&3].node().chain().height(), 4);

    // Heal partition and let sync + reorg converge everyone to fork_b.
    net.partitioned = None;
    net.run_until(6_000_000);
    assert!(net.all_converged(winner.tip().hash(), winner.state_root()));
    assert!(net.peers[&1].node().reorgs() >= 1);
}

#[test]
fn hostile_datagrams_never_panic_and_peers_converge() {
    let (genesis, validators) = make_genesis();
    let (_fork_a, fork_b, winner) = make_forked_chains(&genesis, &validators);
    let peer_ids = [1, 2, 3];
    let profile = NetProfile {
        loss_num: 0,
        loss_den: 1,
        delay_min: 1_000,
        delay_span: 10_000,
    };
    let mut net = build_network(&peer_ids, 0xBEEF_4004, profile);
    net.run_until(300_000);

    // Capture real datagrams while gossiping blocks, then feed truncated/bit-flipped/random bytes.
    for blk in &fork_b {
        net.peers
            .get_mut(&1)
            .unwrap()
            .inject(Msg::Block(blk.clone()), net.now);
    }
    net.drain_transmits();
    let captured: Vec<(PeerId, PeerId, Vec<u8>)> =
        net.in_flight.values().take(8).cloned().collect();

    forall(64, 0xBAD_DA7A, |rng| {
        let &target = rng.pick(&peer_ids);
        let from = if rng.chance(1, 3) {
            999 // unknown peer id
        } else {
            *rng.pick(&peer_ids)
        };
        let garbage = rng.bytes(1400);
        net.peers
            .get_mut(&target)
            .unwrap()
            .receive(from, &garbage, net.now);

        if let Some((c_from, c_to, real)) = captured.first() {
            if !real.is_empty() {
                let cut = rng.below(real.len());
                net.peers
                    .get_mut(c_to)
                    .unwrap()
                    .receive(*c_from, &real[..cut], net.now);

                let mut flipped = real.clone();
                let idx = rng.below(flipped.len());
                flipped[idx] ^= 1u8 << (rng.below(8) as u8);
                net.peers
                    .get_mut(c_to)
                    .unwrap()
                    .receive(*c_from, &flipped, net.now);
            }
        }
    });

    net.run_until(3_000_000);
    assert!(net.all_converged(winner.tip().hash(), winner.state_root()));
}

#[test]
fn simulation_is_deterministic() {
    fn run_once(seed: u64) -> (Vec<Hash>, Vec<Hash>, BTreeMap<PeerId, usize>) {
        let (genesis, validators) = make_genesis();
        let (fork_a, fork_b, _) = make_forked_chains(&genesis, &validators);
        let peer_ids = [2, 5, 9, 14];
        let profile = NetProfile {
            loss_num: 1,
            loss_den: 6,
            delay_min: 1_500,
            delay_span: 25_000,
        };
        let mut net = build_network(&peer_ids, seed, profile);

        // Inject one message before handshake finishes (tests pending queue + flush).
        net.peers
            .get_mut(&2)
            .unwrap()
            .inject(Msg::Block(fork_a[0].clone()), 0);
        net.run_until(300_000);

        for (i, blk) in fork_b.iter().enumerate() {
            let pid = peer_ids[i % peer_ids.len()];
            net.peers
                .get_mut(&pid)
                .unwrap()
                .inject(Msg::Block(blk.clone()), net.now);
        }
        net.run_until(3_500_000);

        let tips = peer_ids
            .iter()
            .map(|id| net.peers[id].node().tip())
            .collect();
        let roots = peer_ids
            .iter()
            .map(|id| net.peers[id].node().chain().state_root())
            .collect();
        (tips, roots, net.sent_counts)
    }

    let r1 = run_once(0xD37E_5EED);
    let r2 = run_once(0xD37E_5EED);
    assert_eq!(r1, r2);
    assert!(r1.0.windows(2).all(|w| w[0] == w[1]));
    assert!(r1.1.windows(2).all(|w| w[0] == w[1]));
    assert!(r1.2.values().all(|&c| c > 0));
    const { assert!(SYNC_INTERVAL_US == 100_000) };
}
