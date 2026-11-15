//! Fuzz soak for seam's decoders, Merkle verifier, message decoder and chain import.
//! `STRATA_SOAK_MS` sets the budget per target (default 300 ms); `STRATA_SOAK_SEED` sets the seed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use assay::Rng;
use onyx::ed25519;
use seam::node::Msg;
use seam::{
    merkle_proof, merkle_root, merkle_verify, Address, Block, BlockError, Chain, Hash, Header, Tx,
    MAX_DATA,
};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

const DEFAULT_SEED: u64 = 0x05EE_D79D;
const DEFAULT_MS: u64 = 300;
/// Hard limit for one case, as the brief requires.
const CASE_LIMIT: Duration = Duration::from_secs(1);

fn env_u64(name: &str, default: u64) -> u64 {
    let Ok(text) = std::env::var(name) else {
        return default;
    };
    let clean: String = text.chars().filter(|c| *c != '_').collect();
    let parsed = match clean
        .strip_prefix("0x")
        .or_else(|| clean.strip_prefix("0X"))
    {
        Some(digits) => u64::from_str_radix(digits, 16),
        None => clean.parse(),
    };
    parsed.unwrap_or_else(|_| panic!("{name} must be a u64, got {text:?}"))
}

/// Run `case` on fresh random streams until `budget` is spent (at least one case).
fn soak(name: &str, seed: u64, budget: Duration, mut case: impl FnMut(&mut Rng)) {
    let mut master = Rng::new(seed);
    let start = Instant::now();
    let mut cases: u64 = 0;
    while cases == 0 || start.elapsed() < budget {
        let case_seed = master.u64();
        let began = Instant::now();
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| case(&mut Rng::new(case_seed)))) {
            eprintln!("soak: {name} panicked on case {cases}, case seed {case_seed:#018x}");
            resume_unwind(payload);
        }
        let took = began.elapsed();
        assert!(
            took < CASE_LIMIT,
            "soak: {name} case {cases} (case seed {case_seed:#018x}) took {took:?}"
        );
        cases += 1;
    }
    println!("soak: {name} seed={seed:#x} cases={cases}");
}

fn run(name: &str, case: impl FnMut(&mut Rng)) {
    let seed = env_u64("STRATA_SOAK_SEED", DEFAULT_SEED);
    let ms = env_u64("STRATA_SOAK_MS", DEFAULT_MS);
    soak(name, seed, Duration::from_millis(ms), case);
}

fn bytes32(rng: &mut Rng) -> [u8; 32] {
    let mut out = [0u8; 32];
    rng.fill(&mut out);
    out
}

fn flip_bit(rng: &mut Rng, bytes: &mut [u8]) {
    if !bytes.is_empty() {
        let i = rng.below(bytes.len());
        bytes[i] ^= 1u8 << rng.below(8);
    }
}

/// Pure noise half the time, text-like noise otherwise.
fn noise(rng: &mut Rng) -> Vec<u8> {
    if rng.chance(1, 2) {
        return rng.bytes(4096);
    }
    let alphabet: &[u8] = b"0123456789abcdefABCDEF: \n\t\0\xff";
    (0..rng.below(512)).map(|_| *rng.pick(alphabet)).collect()
}

/// A mutated copy of `valid`: bit flips, truncation, a splice with `other`, a repeated chunk,
/// or appended noise.
fn mutate(rng: &mut Rng, valid: &[u8], other: &[u8]) -> Vec<u8> {
    let mut out = valid.to_vec();
    match rng.below(5) {
        0 => {
            for _ in 0..=rng.below(4) {
                flip_bit(rng, &mut out);
            }
        }
        1 => {
            let keep = rng.below(out.len() + 1);
            out.truncate(keep);
        }
        2 => {
            let keep = rng.below(out.len() + 1);
            let from = rng.below(other.len() + 1);
            out.truncate(keep);
            out.extend_from_slice(&other[from..]);
        }
        3 => {
            if !out.is_empty() {
                let start = rng.below(out.len());
                let end = start + 1 + rng.below(out.len() - start);
                let chunk = out[start..end].to_vec();
                let at = rng.below(out.len() + 1);
                let tail = out.split_off(at);
                out.extend_from_slice(&chunk);
                out.extend_from_slice(&tail);
            }
        }
        _ => out.extend(rng.bytes(64)),
    }
    out
}

fn small_data(rng: &mut Rng) -> Vec<u8> {
    let len = if rng.chance(1, 8) {
        MAX_DATA
    } else {
        rng.below(64)
    };
    let mut data = vec![0u8; len];
    rng.fill(&mut data);
    data
}

/// A random transaction with a valid signature and arbitrary amounts.
fn signed_tx(rng: &mut Rng) -> Tx {
    let seed = bytes32(rng);
    let to = bytes32(rng);
    let (amount, fee, nonce) = (rng.u64(), rng.u64(), rng.u64());
    Tx::signed(&seed, to, amount, fee, nonce, small_data(rng))
}

/// A block with random header fields and up to three signed transactions. Not chain-valid.
fn random_block(rng: &mut Rng) -> Block {
    let count = rng.below(4);
    let txs: Vec<Tx> = (0..count).map(|_| signed_tx(rng)).collect();
    let mut sig = [0u8; 64];
    rng.fill(&mut sig);
    let header = Header {
        height: rng.u64(),
        parent: bytes32(rng),
        tx_root: bytes32(rng),
        state_root: bytes32(rng),
        time: rng.u64(),
        proposer: bytes32(rng),
        sig,
    };
    Block { header, txs }
}

/// Genesis and keys for a throwaway chain: one validator and one funded sender.
struct World {
    genesis: Vec<(Address, u64)>,
    validators: Vec<Address>,
    proposer: [u8; 32],
    sender: [u8; 32],
}

fn world(rng: &mut Rng) -> World {
    let proposer = bytes32(rng);
    let sender = bytes32(rng);
    World {
        genesis: vec![(ed25519::public_key(&sender), 1_000_000)],
        validators: vec![ed25519::public_key(&proposer)],
        proposer,
        sender,
    }
}

/// Blocks proposed one after another on a live chain, so each is valid on the one before.
fn valid_blocks(rng: &mut Rng, w: &World, count: usize) -> Vec<Block> {
    let mut chain = Chain::new(&w.genesis, &w.validators);
    let sender_addr = ed25519::public_key(&w.sender);
    let mut out = Vec::with_capacity(count);
    for height in 1..=count {
        let nonce = chain.account(&sender_addr).1;
        let mut txs: Vec<Tx> = Vec::new();
        for k in 0..rng.below(3) {
            let to = bytes32(rng);
            let (amount, fee) = (rng.range(0, 100), rng.range(0, 5));
            let data = small_data(rng);
            txs.push(Tx::signed(
                &w.sender,
                to,
                amount,
                fee,
                nonce + k as u64,
                data,
            ));
        }
        let block = chain
            .propose(&w.proposer, height as u64, txs)
            .expect("proposal from valid txs");
        chain.append(block.clone()).expect("proposed block appends");
        out.push(block);
    }
    out
}

fn block_target(rng: &mut Rng) {
    let block = if rng.chance(1, 2) {
        random_block(rng)
    } else {
        let w = world(rng);
        valid_blocks(rng, &w, 1).remove(0)
    };
    let enc = block.encode();
    assert_eq!(Block::decode(&enc).expect("own encoding decodes"), block);
    let other = random_block(rng).encode();
    let bytes = if rng.chance(1, 2) {
        noise(rng)
    } else {
        mutate(rng, &enc, &other)
    };
    if let Ok(decoded) = Block::decode(&bytes) {
        assert_eq!(decoded.encode(), bytes, "accepted block is not canonical");
    }
}

fn tx_target(rng: &mut Rng) {
    let tx = signed_tx(rng);
    let enc = tx.encode();
    assert_eq!(Tx::decode(&enc).expect("own encoding decodes"), tx);
    assert!(tx.verify(), "a freshly signed tx must verify");
    let other_tx = signed_tx(rng);
    let other = other_tx.encode();
    let bytes = if rng.chance(1, 2) {
        noise(rng)
    } else {
        mutate(rng, &enc, &other)
    };
    if let Ok(decoded) = Tx::decode(&bytes) {
        assert_eq!(decoded.encode(), bytes, "accepted tx is not canonical");
        // A splice can reproduce the other, validly signed, transaction: that one may verify.
        if decoded != tx && decoded != other_tx {
            assert!(!decoded.verify(), "altered tx still verifies");
        }
    }
}

fn msg_target(rng: &mut Rng) {
    let msg = match rng.below(4) {
        0 => Msg::Block(random_block(rng)),
        1 => Msg::Tip {
            hash: bytes32(rng),
            height: rng.u64(),
        },
        2 => Msg::GetBlock(bytes32(rng)),
        _ => Msg::Tx(signed_tx(rng)),
    };
    let enc = msg.encode();
    assert_eq!(Msg::decode(&enc).expect("own encoding decodes"), msg);
    let other = signed_tx(rng).encode();
    let bytes = if rng.chance(1, 2) {
        noise(rng)
    } else {
        mutate(rng, &enc, &other)
    };
    if let Ok(decoded) = Msg::decode(&bytes) {
        assert_eq!(decoded.encode(), bytes, "accepted message is not canonical");
    }
}

fn merkle_target(rng: &mut Rng) {
    let count = rng.below(65);
    let leaves: Vec<Vec<u8>> = (0..count).map(|_| rng.bytes(96)).collect();
    let root = merkle_root(&leaves);
    let index = rng.below(count + 2); // sometimes out of range
    let proof = merkle_proof(&leaves, index);
    if index < count {
        let leaf = &leaves[index];
        assert!(
            merkle_verify(&root, leaf, index, count, &proof),
            "honest proof rejected"
        );
        match rng.below(4) {
            0 => {
                let mut longer = leaf.clone();
                longer.push(rng.u64() as u8);
                assert!(
                    !merkle_verify(&root, &longer, index, count, &proof),
                    "tampered leaf accepted"
                );
            }
            1 if !proof.is_empty() => {
                let mut bad = proof.clone();
                let k = rng.below(bad.len());
                let mut node = bad[k];
                flip_bit(rng, &mut node);
                bad[k] = node;
                assert!(
                    !merkle_verify(&root, leaf, index, count, &bad),
                    "tampered proof accepted"
                );
            }
            2 => {
                let mut bad_root = root;
                flip_bit(rng, &mut bad_root);
                assert!(
                    !merkle_verify(&bad_root, leaf, index, count, &proof),
                    "tampered root accepted"
                );
            }
            _ => {}
        }
    } else {
        assert!(proof.is_empty(), "proof for an out-of-range index");
        assert!(!merkle_verify(&root, &[], index, count, &proof));
    }
    // Junk proofs and wild indexes must return a bool, never panic.
    let junk: Vec<Hash> = (0..rng.below(12)).map(|_| bytes32(rng)).collect();
    let _ = merkle_verify(
        &root,
        &[1u8, 2, 3],
        rng.u64() as usize,
        rng.below(70),
        &junk,
    );
    let _ = merkle_verify(&root, &[], usize::MAX, usize::MAX, &junk);
}

fn chain_target(rng: &mut Rng) {
    let w = world(rng);
    let count = 1 + rng.below(4);
    let good = valid_blocks(rng, &w, count);
    // good[k] is the next block after a chain holding good[..k].
    let k = rng.below(good.len());
    let prefix = |n: usize| {
        let mut chain = Chain::new(&w.genesis, &w.validators);
        for block in &good[..n] {
            chain.append(block.clone()).expect("prefix is valid");
        }
        chain
    };

    // The untouched next block must be accepted, so the corruption checks below mean something.
    let mut chain = prefix(k);
    chain.append(good[k].clone()).expect("valid next block");
    assert_eq!(chain.height(), k as u64 + 1);

    // Reordered: a block one height too high or too low is refused with BadHeight.
    let neighbours = [k.checked_sub(1), Some(k + 1)];
    for other in neighbours.into_iter().flatten().filter_map(|i| good.get(i)) {
        let mut chain = prefix(k);
        assert_eq!(
            chain.append(other.clone()),
            Err(BlockError::BadHeight),
            "out-of-order block accepted"
        );
        assert_eq!(chain.height(), k as u64);
    }

    // Corrupted: one flipped bit makes the next block fail, and the chain does not move.
    let mut bytes = good[k].encode();
    flip_bit(rng, &mut bytes);
    if let Ok(block) = Block::decode(&bytes) {
        assert_eq!(block.encode(), bytes, "accepted block is not canonical");
        let mut chain = prefix(k);
        let before = (chain.height(), chain.tip().hash(), chain.state_root());
        assert!(chain.append(block).is_err(), "corrupted block accepted");
        assert_eq!(
            before,
            (chain.height(), chain.tip().hash(), chain.state_root())
        );
    }

    // Pure garbage that happens to decode must not panic the importer.
    if let Ok(block) = Block::decode(&noise(rng)) {
        let _ = prefix(k).append(block);
    }
}

#[test]
fn soak_block_decode() {
    run("block_decode", block_target);
}

#[test]
fn soak_tx_decode() {
    run("tx_decode", tx_target);
}

#[test]
fn soak_msg_decode() {
    run("msg_decode", msg_target);
}

#[test]
fn soak_merkle_tamper() {
    run("merkle_tamper", merkle_target);
}

#[test]
fn soak_chain_import() {
    run("chain_import", chain_target);
}
