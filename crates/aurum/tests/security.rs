//! Security tests covering decoding, signatures, Merkle proofs, contracts, networking, and fork choice.
#![allow(clippy::unwrap_used)]

use aurum::node::net::{self, Peer};
use aurum::node::{Msg, Node};
use aurum::{
    contract_address, merkle_proof, merkle_root, merkle_verify, Block, Chain, DecodeError, Header,
    Tx, DEPLOY, MAX_DATA, MAX_TXS,
};
use sphinx::ed25519;
use std::time::{Duration, Instant};
use xenon::{Config, Connection};

fn seed(b: u8) -> [u8; 32] {
    let mut s = [0u8; 32];
    s[0] = b;
    s[31] = b.wrapping_mul(13).wrapping_add(5);
    s
}

fn addr(b: u8) -> [u8; 32] {
    ed25519::public_key(&seed(b))
}

#[test]
fn decode_rejects_oversized_truncated_and_trailing_bytes() {
    // Empty and truncated inputs never panic.
    assert_eq!(Block::decode(&[]), Err(DecodeError::Truncated));
    assert_eq!(Tx::decode(&[]), Err(DecodeError::Truncated));
    assert_eq!(Msg::decode(&[]), Err(DecodeError::Truncated));
    assert_eq!(Msg::decode(&[99, 1, 2]), Err(DecodeError::TooLarge));

    // Craft a Tx header claiming u32::MAX data bytes: must reject TooLarge without allocating.
    let mut hostile_tx = vec![0u8; 32 + 32 + 8 + 8 + 8];
    hostile_tx.extend_from_slice(&u32::MAX.to_le_bytes());
    hostile_tx.extend_from_slice(&[0u8; 64]);
    assert_eq!(Tx::decode(&hostile_tx), Err(DecodeError::TooLarge));

    // Claiming MAX_DATA + 1 bytes must also fail with TooLarge.
    let mut over_data_tx = vec![0u8; 32 + 32 + 8 + 8 + 8];
    over_data_tx.extend_from_slice(&((MAX_DATA as u32) + 1).to_le_bytes());
    assert_eq!(Tx::decode(&over_data_tx), Err(DecodeError::TooLarge));

    // Craft a Block header claiming u32::MAX transactions: must reject TooLarge without allocating.
    let mut hostile_blk = vec![0u8; 208];
    hostile_blk.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(Block::decode(&hostile_blk), Err(DecodeError::TooLarge));

    let mut over_txs_blk = vec![0u8; 208];
    over_txs_blk.extend_from_slice(&((MAX_TXS as u32) + 1).to_le_bytes());
    assert_eq!(Block::decode(&over_txs_blk), Err(DecodeError::TooLarge));

    // Trailing byte on valid encodings must fail.
    let tx = Tx::signed(&seed(1), addr(2), 10, 1, 0, b"ok".to_vec());
    let mut tx_bytes = tx.encode();
    tx_bytes.push(0xAA);
    assert_eq!(Tx::decode(&tx_bytes), Err(DecodeError::TrailingBytes));

    let chain = Chain::new(&[(addr(1), 1000)], &[addr(1)]);
    let blk = chain.propose(&seed(1), 1, vec![tx]).unwrap();
    let mut blk_bytes = blk.encode();
    blk_bytes.push(0xBB);
    assert_eq!(Block::decode(&blk_bytes), Err(DecodeError::TrailingBytes));
}

#[test]
fn signature_checks_reject_forged_and_zero_keys() {
    // Zero public key and zero signature must not verify.
    let zero_tx = Tx {
        from: [0u8; 32],
        to: addr(2),
        amount: 10,
        fee: 1,
        nonce: 0,
        data: Vec::new(),
        sig: [0u8; 64],
    };
    assert!(!zero_tx.verify());

    let zero_hdr = Header {
        height: 1,
        parent: [0u8; 32],
        tx_root: merkle_root(&[]),
        state_root: [0u8; 32],
        time: 1,
        proposer: [0u8; 32],
        sig: [0u8; 64],
    };
    assert!(!zero_hdr.verify());

    // Mutating any signed field invalidates signature.
    let mut tx = Tx::signed(&seed(1), addr(2), 10, 1, 0, b"hi".to_vec());
    assert!(tx.verify());
    tx.amount = 11;
    assert!(!tx.verify());
    tx.amount = 10;
    tx.sig[0] ^= 1;
    assert!(!tx.verify());
}

#[test]
fn merkle_verify_rejects_hostile_proofs() {
    let leaves: Vec<Vec<u8>> = (0..7u8).map(|i| vec![i; 16]).collect();
    let root = merkle_root(&leaves);

    assert!(!merkle_verify(&root, &leaves[0], 0, 0, &[]));
    assert!(!merkle_verify(&root, &leaves[0], 7, 7, &[]));

    for (idx, leaf) in leaves.iter().enumerate() {
        let proof = merkle_proof(&leaves, idx);
        assert!(merkle_verify(&root, leaf, idx, leaves.len(), &proof));

        // Truncated proof must fail.
        if !proof.is_empty() {
            assert!(!merkle_verify(
                &root,
                leaf,
                idx,
                leaves.len(),
                &proof[..proof.len() - 1]
            ));
        }
        // Extended proof (including huge hostile slice) must fail.
        let mut extended = proof.clone();
        extended.push([0x42u8; 32]);
        assert!(!merkle_verify(&root, leaf, idx, leaves.len(), &extended));

        let mut huge = proof.clone();
        huge.resize(10_000, [0u8; 32]);
        assert!(!merkle_verify(&root, leaf, idx, leaves.len(), &huge));

        // Wrong leaf or wrong index or wrong count must fail.
        let mut bad_leaf = leaf.clone();
        bad_leaf[0] ^= 1;
        assert!(!merkle_verify(&root, &bad_leaf, idx, leaves.len(), &proof));
        assert!(!merkle_verify(&root, leaf, idx + 1, leaves.len(), &proof));
        assert!(!merkle_verify(&root, leaf, idx, leaves.len() + 2, &proof));
    }
    // At index 6, changing count from 7 to 8 flips the sibling direction and must fail.
    let proof6 = merkle_proof(&leaves, 6);
    assert!(!merkle_verify(&root, &leaves[6], 6, 8, &proof6));

    // Second-preimage attempt: present an inner node hash pair with 0x01 prefix as a leaf.
    let two = vec![b"left".to_vec(), b"right".to_vec()];
    let two_root = merkle_root(&two);
    let l0 = sphinx::sha256::digest(&[&[0x00][..], b"left"].concat());
    let l1 = sphinx::sha256::digest(&[&[0x00][..], b"right"].concat());
    let mut forged_inner = Vec::with_capacity(65);
    forged_inner.push(0x01);
    forged_inner.extend_from_slice(&l0);
    forged_inner.extend_from_slice(&l1);
    assert!(!merkle_verify(&two_root, &forged_inner, 0, 1, &[]));
}

#[test]
fn fork_choice_rejects_forged_replayed_and_cyclic_blocks() {
    let vals = [addr(1), addr(2)];
    let mut node = Node::new(&[(addr(1), 1000)], &vals);
    let c0 = node.chain().clone();

    // Valid block 1 from validator 2.
    let b1 = c0.propose(&seed(2), 10, Vec::new()).unwrap();
    let h1 = b1.hash();
    assert!(!node.handle(Msg::Block(b1.clone())).is_empty());
    assert_eq!(node.tip(), h1);

    // Replayed block 1 is ignored.
    assert!(node.handle(Msg::Block(b1.clone())).is_empty());
    assert_eq!(node.chain().height(), 1);

    // Wrong proposer at height 2 (expects addr(1), propose with seed(2)).
    let wrong_prop = node.chain().propose(&seed(2), 20, Vec::new()).unwrap();
    assert!(node.handle(Msg::Block(wrong_prop)).is_empty());
    assert_eq!(node.tip(), h1);

    // Non-increasing timestamp at height 2.
    let bad_time = node.chain().propose(&seed(1), 10, Vec::new()).unwrap();
    assert!(node.handle(Msg::Block(bad_time)).is_empty());
    assert_eq!(node.tip(), h1);

    // Forged signature at height 2.
    let mut bad_sig = node.chain().propose(&seed(1), 20, Vec::new()).unwrap();
    bad_sig.header.sig[0] ^= 0xff;
    assert!(node.handle(Msg::Block(bad_sig)).is_empty());
    assert_eq!(node.tip(), h1);

    // Self-parent orphan cycle is rejected.
    let mut self_loop = node.chain().propose(&seed(1), 30, Vec::new()).unwrap();
    self_loop.header.parent = [0xAA; 32];
    let mut two_cycle_a = self_loop.clone();
    let mut two_cycle_b = self_loop;
    two_cycle_b.header.time = 31;
    two_cycle_a.header.parent = two_cycle_b.hash();
    two_cycle_b.header.parent = two_cycle_a.hash();
    let _ = node.handle(Msg::Block(two_cycle_a.clone()));
    let _ = node.handle(Msg::Block(two_cycle_b.clone()));
    assert_eq!(node.tip(), h1);
}

#[test]
fn node_handle_tx_accepts_forged_signature_and_oversized_data() {
    let mut node = Node::new(&[(addr(1), 1000)], &[addr(1)]);

    let mut forged = Tx::signed(&seed(1), addr(2), 10, 1, 0, b"hi".to_vec());
    forged.sig[0] ^= 0x01;
    let out = node.handle(Msg::Tx(forged));
    assert!(out.is_empty(), "forged tx must not be gossiped");
    assert!(
        node.pending().is_empty(),
        "forged tx must not enter mempool"
    );

    let oversized = Tx::signed(&seed(1), addr(2), 10, 1, 0, vec![0xAA; MAX_DATA + 1]);
    let out2 = node.handle(Msg::Tx(oversized));
    assert!(out2.is_empty(), "oversized tx must not be gossiped");
    assert!(
        node.pending().is_empty(),
        "oversized tx must not enter mempool"
    );
}

#[test]
fn contract_host_unbounded_writes_and_events_allocation() {
    let alice_seed = seed(2);
    let val_seed = seed(1);
    let alice = ed25519::public_key(&alice_seed);
    let val = ed25519::public_key(&val_seed);
    let mut chain = Chain::new(&[(alice, 1_000_000_000)], &[val]);

    // Build a 16 KiB string inside the 1 MiB VM heap, then emit it 4200 times (~67.2 MiB)
    // using only fee=500 (500_000 fuel).
    let src = r#"
fn emit(e: str) uses chain = extern "chain.emit"

fn main() uses chain {
    var b: [byte] = []
    var j = 0
    while j < 16384 {
        push(b, byte(65))
        j = j + 1
    }
    let s = match text(b) { none => "", some(t) => t }
    var i = 0
    while i < 4200 {
        emit(s)
        i = i + 1
    }
}
"#;
    assert!(src.len() <= MAX_DATA);
    let c_addr = contract_address(&alice, 0);
    let deploy = Tx::signed(&alice_seed, DEPLOY, 0, 10, 0, src.as_bytes().to_vec());
    let call = Tx::signed(&alice_seed, c_addr, 0, 500, 1, Vec::new());
    let blk = chain.propose(&val_seed, 1, vec![deploy, call]).unwrap();
    chain.append(blk).unwrap();

    let rs = chain.receipts(1).unwrap();
    assert!(rs[0].ok, "contract deploy must succeed");
    let total_event_bytes: usize = rs[1].events.iter().map(String::len).sum();
    assert!(
        !rs[1].ok && total_event_bytes < 64 * 1024 * 1024,
        "contract allocated {total_event_bytes} event bytes on host (ok={})",
        rs[1].ok
    );

    // Also verify chain.set write-count / write-bytes cap traps cleanly.
    let set_src = r#"
fn set(k: [byte], v: [byte]) uses chain = extern "chain.set"

fn enc(n: int) -> [byte] {
    var o: [byte] = []
    var i = 0
    while i < 8 { push(o, byte((n >> (i * 8)) & 255)); i = i + 1 }
    o
}

fn main() uses chain {
    var v: [byte] = []
    var j = 0
    while j < 4096 {
        push(v, byte(66))
        j = j + 1
    }
    var i = 0
    while i < 512 {
        set(enc(i), v)
        i = i + 1
    }
}
"#;
    assert!(set_src.len() <= MAX_DATA);
    let c2_addr = contract_address(&alice, 2);
    let deploy2 = Tx::signed(&alice_seed, DEPLOY, 0, 10, 2, set_src.as_bytes().to_vec());
    let call2 = Tx::signed(&alice_seed, c2_addr, 0, 1000, 3, Vec::new());
    let blk2 = chain.propose(&val_seed, 2, vec![deploy2, call2]).unwrap();
    chain.append(blk2).unwrap();
    let rs2 = chain.receipts(2).unwrap();
    assert!(rs2[0].ok, "set contract deploy must succeed");
    assert!(
        !rs2[1].ok,
        "contract exceeding host write limit (2 MiB) must trap"
    );
}

#[test]
fn contract_infinite_loop_with_huge_fee_hangs() {
    let alice_seed = seed(2);
    let val_seed = seed(1);
    let alice = ed25519::public_key(&alice_seed);
    let val = ed25519::public_key(&val_seed);
    let mut chain = Chain::new(&[(alice, 1_000_000_000)], &[val]);

    let src = "fn main() { while true { } }";
    let c_addr = contract_address(&alice, 0);
    let deploy = Tx::signed(&alice_seed, DEPLOY, 0, 10, 0, src.as_bytes().to_vec());
    // 100_000 fee -> 100_000_000 fuel without a per-call cap.
    let call = Tx::signed(&alice_seed, c_addr, 0, 100_000, 1, Vec::new());

    let start = Instant::now();
    let blk = chain.propose(&val_seed, 1, vec![deploy, call]).unwrap();
    chain.append(blk).unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(1),
        "contract call took {:?} (>= 1s)",
        elapsed
    );
    assert!(!chain.receipts(1).unwrap()[1].ok);
}

#[test]
fn node_propose_quadratic_bad_nonce_mempool_hangs() {
    let val_seed = seed(1);
    let alice_seed = seed(2);
    let alice = ed25519::public_key(&alice_seed);
    let val = ed25519::public_key(&val_seed);
    let mut node = Node::new(&[(alice, 1_000_000)], &[val]);

    // 30 valid txs followed by 100 bad-nonce txs (~3000 ed25519 checks on unfixed code).
    for n in 0..30u64 {
        let tx = Tx::signed(&alice_seed, addr(3), 1, 0, n, Vec::new());
        node.handle(Msg::Tx(tx));
    }
    for n in 5000..5100u64 {
        let tx = Tx::signed(&alice_seed, addr(3), 1, 0, n, Vec::new());
        node.handle(Msg::Tx(tx));
    }

    let start = Instant::now();
    let blk = node.propose(&val_seed, 10).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(blk.txs.len(), 30);
    assert!(
        elapsed < Duration::from_secs(1),
        "node.propose took {:?} (>= 1s)",
        elapsed
    );
}

#[test]
fn net_concurrent_unfinished_streams_exceed_limit() {
    let node = Node::new(&[(addr(1), 1000)], &[addr(1)]);
    let mut server_peer = Peer::new(2, node, [22; 32], [33; 32]);
    let cfg = Config {
        max_streams: 1_000_000,
        stream_window: net::MAX_MSG as u64,
        conn_window: (net::MAX_MSG as u64).saturating_mul(4),
        ..Config::default()
    };
    let mut client_conn = Connection::connect(server_peer.public_key(), [44; 32], cfg, 0);
    server_peer.add_peer(1, [0u8; 32], 0);

    // Complete Xenon handshake.
    for step in 0..16 {
        let now = step * 100;
        while let Some(dgram) = client_conn.poll_transmit(now) {
            server_peer.receive(1, &dgram, now);
        }
        while let Some((_, dgram)) = server_peer.poll_transmit(now) {
            client_conn.handle(&dgram, now);
        }
        if client_conn.is_connected() && server_peer.connected() == vec![1] {
            break;
        }
    }
    assert!(client_conn.is_connected());

    // Build 60 distinct ~1.13 MiB Msg::Block payloads (total ~67.8 MiB > 64 MiB).
    let tx = Tx {
        from: addr(9),
        to: addr(8),
        amount: 1,
        fee: 0,
        nonce: 0,
        data: vec![0xAB; MAX_DATA],
        sig: [0u8; 64],
    };
    let base_hdr = Header {
        height: 1,
        parent: [0x11; 32],
        tx_root: [0u8; 32],
        state_root: [0u8; 32],
        time: 1,
        proposer: addr(1),
        sig: [0u8; 64],
    };
    let mut streams = Vec::new();
    let mut now = 2000u64;
    for i in 0..60u64 {
        let mut hdr = base_hdr.clone();
        hdr.time = i + 1;
        let blk = Block {
            header: hdr,
            txs: vec![tx.clone(); MAX_TXS],
        };
        let hash = blk.hash();
        let bytes = Msg::Block(blk).encode();
        let sid = client_conn.open_stream().unwrap();
        // Send everything except the last byte so all 60 streams remain unfinished concurrently.
        let mut off = 0usize;
        let mut stalls = 0usize;
        let target = bytes.len() - 1;
        while off < target {
            match client_conn.write(sid, &bytes[off..target]) {
                Ok(0) => {
                    stalls += 1;
                    if stalls > 32 {
                        break;
                    }
                }
                Ok(n) => {
                    off += n;
                    stalls = 0;
                }
                Err(_) => break,
            }
            now += 10;
            while let Some(dgram) = client_conn.poll_transmit(now) {
                server_peer.receive(1, &dgram, now);
            }
            while let Some((_, dgram)) = server_peer.poll_transmit(now) {
                client_conn.handle(&dgram, now);
            }
        }
        streams.push((sid, hash, bytes[target]));
    }

    // Now finish all 60 streams and pump.
    for (sid, _, last_byte) in &streams {
        let _ = client_conn.write(*sid, &[*last_byte]);
        let _ = client_conn.finish(*sid);
    }
    for _ in 0..64 {
        now += 10;
        let mut moved = false;
        while let Some(dgram) = client_conn.poll_transmit(now) {
            server_peer.receive(1, &dgram, now);
            moved = true;
        }
        while let Some((_, dgram)) = server_peer.poll_transmit(now) {
            client_conn.handle(&dgram, now);
            moved = true;
        }
        if !moved {
            break;
        }
    }

    let known_count = streams
        .iter()
        .filter(|(_, h, _)| server_peer.node().known(h))
        .count();
    assert!(
        (1..=20).contains(&known_count),
        "expected bounded concurrent stream buffering (1..=20), got {known_count}/60"
    );
}
