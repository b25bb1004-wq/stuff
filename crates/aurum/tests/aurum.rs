//! Tests for aurum: RFC 6962 vectors, proofs, encodings, decoding, and chain validation.
#![allow(clippy::unwrap_used)]

use assay::{forall, hex, to_hex};
use aurum::{
    merkle_proof, merkle_root, merkle_verify, Address, Block, BlockError, Chain, DecodeError, Hash,
    Tx, TxError, MAX_DATA, MAX_TXS,
};
use sphinx::{ed25519, sha256};
use std::collections::BTreeMap;

/// RFC 6962 test leaves, in order.
fn rfc_leaves() -> Vec<Vec<u8>> {
    [
        "",
        "00",
        "10",
        "2021",
        "3031",
        "40414243",
        "5051525354555657",
        "606162636465666768696a6b6c6d6e6f",
    ]
    .iter()
    .map(|s| hex(s))
    .collect()
}

/// Roots of the first 1 to 8 RFC 6962 leaves.
const RFC_ROOTS: [&str; 8] = [
    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
    "fac54203e7cc696cf0dfcb42c92a1d9dbaf70ad9e621f4bd8d98662f00e3c125",
    "aeb6bcfe274b70a14fb067a5e5578264db0fa9b51af5e0ba159158f329e06e77",
    "d37ee418976dd95753c1c73862b9398fa2a2cf9b4ff0fdfe8b30cd95209614b7",
    "4e3bbb1f7b478dcfe71fb631631519a3bca12c9aefca1612bfce4c13a86264d4",
    "76e67dadbcdf1e10e1b74ddc608abd2f98dfb16fbce75277b5232a127f2087ef",
    "ddb89be403809e325750d3d263cd78929c2942b7942a34b77e122c9594a74c8c",
    "5dc9da79a70659a9ad559cb701ded9a2ab9d823aad2f4960cfe370eff4604328",
];

/// Distinct leaves for an `n`-leaf tree.
fn leaves_of(n: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| vec![i as u8, (i >> 8) as u8, 0xA5])
        .collect()
}

fn seed(i: u8) -> [u8; 32] {
    [i; 32]
}

/// Six funded accounts (seeds 1 to 6) and three validators (the first three keys).
fn fixture() -> (Chain, Vec<[u8; 32]>) {
    let seeds: Vec<[u8; 32]> = (1..=6).map(seed).collect();
    let keys: Vec<Address> = seeds.iter().map(ed25519::public_key).collect();
    let genesis: Vec<(Address, u64)> = keys.iter().map(|k| (*k, 1000)).collect();
    (Chain::new(&genesis, &keys[..3]), seeds)
}

fn keys_of(seeds: &[[u8; 32]]) -> Vec<Address> {
    seeds.iter().map(ed25519::public_key).collect()
}

/// The seed of the validator expected to propose the next block.
fn proposer_seed(chain: &Chain, seeds: &[[u8; 32]]) -> [u8; 32] {
    let expected = chain.expected_proposer();
    *seeds
        .iter()
        .find(|s| ed25519::public_key(s) == expected)
        .unwrap()
}

/// A block with `txs` committed to, proposed by `seed` at `time`, with an unchecked state root.
fn signed_block(seed: &[u8; 32], chain: &Chain, time: u64, txs: Vec<Tx>) -> Block {
    let leaves: Vec<Vec<u8>> = txs.iter().map(Tx::encode).collect();
    let mut header = chain.propose(seed, time, Vec::new()).unwrap().header;
    header.tx_root = merkle_root(&leaves);
    header.state_root = [0u8; 32];
    header.sign(seed);
    Block { header, txs }
}

/// Everything an append failure must leave unchanged.
fn snapshot(chain: &Chain, keys: &[Address]) -> (u64, Hash, Hash, Vec<(u64, u64)>) {
    (
        chain.height(),
        chain.tip().hash(),
        chain.state_root(),
        keys.iter().map(|k| chain.account(k)).collect(),
    )
}

fn expect_append_error(chain: &mut Chain, keys: &[Address], block: Block, want: BlockError) {
    let before = snapshot(chain, keys);
    assert_eq!(chain.append(block), Err(want));
    assert_eq!(
        snapshot(chain, keys),
        before,
        "chain changed after {want:?}"
    );
}

#[test]
fn rfc6962_vector_roots() {
    let leaves = rfc_leaves();
    for (n, want) in RFC_ROOTS.iter().enumerate() {
        assert_eq!(
            to_hex(&merkle_root(&leaves[..n + 1])),
            *want,
            "n = {}",
            n + 1
        );
    }
}

#[test]
fn empty_tree_root_is_sha256_of_nothing() {
    assert_eq!(merkle_root(&[]), sha256::digest(b""));
    assert!(merkle_proof(&[], 0).is_empty());
}

#[test]
fn proofs_verify_for_every_index_of_1_to_40_leaves() {
    for n in 1..=40 {
        let leaves = leaves_of(n);
        let root = merkle_root(&leaves);
        for i in 0..n {
            let proof = merkle_proof(&leaves, i);
            assert!(proof.len() <= 6, "n={n} i={i}");
            assert!(
                merkle_verify(&root, &leaves[i], i, n, &proof),
                "n={n} i={i}"
            );
        }
        assert!(merkle_proof(&leaves, n).is_empty());
        assert!(!merkle_verify(&root, &leaves[0], n, n, &[]));
    }
}

#[test]
fn tampered_proofs_and_leaves_are_rejected() {
    for n in [2usize, 3, 5, 8, 13, 33, 40] {
        let leaves = leaves_of(n);
        let root = merkle_root(&leaves);
        for i in 0..n {
            let proof = merkle_proof(&leaves, i);
            let mut bad_leaf = leaves[i].clone();
            bad_leaf[0] ^= 1;
            assert!(!merkle_verify(&root, &bad_leaf, i, n, &proof));
            for k in 0..proof.len() {
                let mut bad = proof.clone();
                bad[k][5] ^= 0x80;
                assert!(
                    !merkle_verify(&root, &leaves[i], i, n, &bad),
                    "n={n} i={i} k={k}"
                );
            }
            if let Some((_, short)) = proof.split_last() {
                assert!(!merkle_verify(&root, &leaves[i], i, n, short));
            }
            let mut long = proof.clone();
            long.push([7u8; 32]);
            assert!(!merkle_verify(&root, &leaves[i], i, n, &long));
            assert!(!merkle_verify(&root, &leaves[i], (i + 1) % n, n, &proof));
        }
    }
}

#[test]
fn tx_encoding_is_byte_exact() {
    let s = seed(1);
    let to = [9u8; 32];
    let tx = Tx::signed(&s, to, 0x0102, 3, 4, vec![0xAA, 0xBB]);
    let enc = tx.encode();
    assert_eq!(enc.len(), 92 + 2 + 64);
    assert_eq!(&enc[..32], ed25519::public_key(&s).as_slice());
    assert_eq!(&enc[32..64], to.as_slice());
    assert_eq!(&enc[64..72], 0x0102u64.to_le_bytes().as_slice());
    assert_eq!(&enc[72..80], 3u64.to_le_bytes().as_slice());
    assert_eq!(&enc[80..88], 4u64.to_le_bytes().as_slice());
    assert_eq!(&enc[88..92], 2u32.to_le_bytes().as_slice());
    assert_eq!(&enc[92..94], [0xAA, 0xBB].as_slice());
    assert_eq!(&enc[94..], tx.sig.as_slice());
    assert_eq!(tx.body().as_slice(), &enc[..94]);
    assert_eq!(tx.id(), sha256::digest(&enc));
    assert!(tx.verify());
    assert_eq!(Tx::decode(&enc), Ok(tx));
}

#[test]
fn tampered_tx_fails_verification() {
    let tx = Tx::signed(&seed(2), [1u8; 32], 5, 1, 0, b"hi".to_vec());
    let mut t = tx.clone();
    t.amount += 1;
    assert!(!t.verify());
    let mut t = tx.clone();
    t.data.push(0);
    assert!(!t.verify());
    let mut t = tx.clone();
    t.sig[0] ^= 1;
    assert!(!t.verify());
    let mut t = tx.clone();
    t.from = [3u8; 32];
    assert!(!t.verify());
    assert!(tx.verify());
}

#[test]
fn block_encoding_round_trips_and_is_byte_exact() {
    let (chain, seeds) = fixture();
    let p = proposer_seed(&chain, &seeds);
    let keys = keys_of(&seeds);
    let txs = vec![
        Tx::signed(&seeds[0], keys[1], 10, 1, 0, vec![1, 2, 3]),
        Tx::signed(&seeds[0], keys[2], 0, 0, 1, Vec::new()),
    ];
    let block = chain.propose(&p, 1, txs.clone()).unwrap();
    let enc = block.encode();
    let tx_bytes: usize = txs.iter().map(|t| t.encode().len()).sum();
    assert_eq!(block.header.encode().len(), 208);
    assert_eq!(enc.len(), 208 + 4 + tx_bytes);
    assert_eq!(&enc[..208], block.header.encode().as_slice());
    assert_eq!(&enc[208..212], 2u32.to_le_bytes().as_slice());
    assert_eq!(
        &enc[212..],
        [txs[0].encode(), txs[1].encode()].concat().as_slice()
    );
    assert_eq!(Block::decode(&enc), Ok(block.clone()));
    assert_eq!(block.hash(), block.header.hash());
    assert!(block.header.verify());
    assert_eq!(block.header.proposer, keys[p_index(&seeds, &p)]);
}

fn p_index(seeds: &[[u8; 32]], p: &[u8; 32]) -> usize {
    seeds.iter().position(|s| s == p).unwrap()
}

#[test]
fn header_sign_sets_sig_only() {
    let (chain, seeds) = fixture();
    let p = proposer_seed(&chain, &seeds);
    let mut header = chain.propose(&p, 1, Vec::new()).unwrap().header;
    let proposer = header.proposer;
    header.sign(&seeds[5]);
    assert_eq!(header.proposer, proposer);
    assert!(!header.verify());
    header.sign(&p);
    assert!(header.verify());
}

#[test]
fn decode_errors_are_reported() {
    let (chain, seeds) = fixture();
    let p = proposer_seed(&chain, &seeds);
    let keys = keys_of(&seeds);

    let tx = Tx::signed(&seeds[0], keys[1], 1, 0, 0, vec![7; 3]);
    let enc = tx.encode();
    for n in 0..enc.len() {
        assert_eq!(
            Tx::decode(&enc[..n]),
            Err(DecodeError::Truncated),
            "prefix {n}"
        );
    }
    let mut long = enc.clone();
    long.push(0);
    assert_eq!(Tx::decode(&long), Err(DecodeError::TrailingBytes));
    assert_eq!(Tx::decode(&[]), Err(DecodeError::Truncated));

    let max = Tx::signed(&seeds[0], keys[1], 1, 0, 0, vec![5; MAX_DATA]);
    assert_eq!(Tx::decode(&max.encode()), Ok(max));
    let big = Tx::signed(&seeds[0], keys[1], 1, 0, 0, vec![0; MAX_DATA + 1]);
    assert_eq!(Tx::decode(&big.encode()), Err(DecodeError::TooLarge));
    let mut cut = enc[..88].to_vec();
    cut.extend_from_slice(&((MAX_DATA + 1) as u32).to_le_bytes());
    assert_eq!(Tx::decode(&cut), Err(DecodeError::TooLarge));

    let block = chain.propose(&p, 1, vec![tx.clone()]).unwrap();
    let benc = block.encode();
    for n in 0..benc.len() {
        assert_eq!(
            Block::decode(&benc[..n]),
            Err(DecodeError::Truncated),
            "prefix {n}"
        );
    }
    let mut trailing = benc.clone();
    trailing.push(9);
    assert_eq!(Block::decode(&trailing), Err(DecodeError::TrailingBytes));
    assert_eq!(Block::decode(&[]), Err(DecodeError::Truncated));

    let mut over = block.header.encode();
    over.extend_from_slice(&((MAX_TXS + 1) as u32).to_le_bytes());
    assert_eq!(Block::decode(&over), Err(DecodeError::TooLarge));
    let mut at_limit = block.header.encode();
    at_limit.extend_from_slice(&(MAX_TXS as u32).to_le_bytes());
    assert_eq!(Block::decode(&at_limit), Err(DecodeError::Truncated));
    let mut oversized_tx = block.header.encode();
    oversized_tx.extend_from_slice(&1u32.to_le_bytes());
    oversized_tx.extend_from_slice(&big.encode());
    assert_eq!(Block::decode(&oversized_tx), Err(DecodeError::TooLarge));
}

#[test]
fn random_transfers_match_a_simple_model() {
    forall(4, 0x5EA3, |rng| {
        let seeds: Vec<[u8; 32]> = (1..=8).map(seed).collect();
        let keys = keys_of(&seeds);
        let genesis: Vec<(Address, u64)> = keys.iter().map(|k| (*k, 500)).collect();
        let validators = keys[..3].to_vec();
        let mut chain = Chain::new(&genesis, &validators);
        let mut model: BTreeMap<Address, (u64, u64)> =
            genesis.iter().map(|(a, b)| (*a, (*b, 0))).collect();
        let mut time = 0u64;

        for _ in 0..30 {
            let expected = chain.expected_proposer();
            let slot = (chain.height() as usize + 1) % validators.len();
            assert_eq!(expected, validators[slot]);
            let p = proposer_seed(&chain, &seeds);

            let mut txs = Vec::new();
            for _ in 0..rng.below(6) {
                let from_i = rng.below(seeds.len());
                let from = keys[from_i];
                let to = keys[rng.below(keys.len())];
                let (balance, nonce) = model_get(&model, &from);
                let amount = rng.range(0, 60).min(balance);
                let fee = rng.range(0, 4).min(balance - amount);
                let data = if rng.chance(1, 4) {
                    rng.bytes(20)
                } else {
                    Vec::new()
                };
                let tx = Tx::signed(&seeds[from_i], to, amount, fee, nonce, data);
                model_apply(&mut model, &tx, &expected);
                txs.push(tx);
            }

            time += 1 + rng.below(3) as u64;
            let block = chain.propose(&p, time, txs).unwrap();
            chain.append(block.clone()).unwrap();
            assert_eq!(chain.block(chain.height()), Some(&block));
            assert_eq!(chain.state_root(), model_root(&model));
            for k in &keys {
                assert_eq!(chain.account(k), model_get(&model, k));
            }
            assert_eq!(Block::decode(&block.encode()).unwrap(), block);
        }
        assert_eq!(chain.height(), 30);
        assert_eq!(chain.genesis().header.height, 0);
    });
}

fn model_get(model: &BTreeMap<Address, (u64, u64)>, address: &Address) -> (u64, u64) {
    model.get(address).copied().unwrap_or((0, 0))
}

/// The same rules as the chain: debit amount+fee and bump the nonce, credit amount, credit fee.
fn model_apply(model: &mut BTreeMap<Address, (u64, u64)>, tx: &Tx, proposer: &Address) {
    let (balance, nonce) = model_get(model, &tx.from);
    model.insert(tx.from, (balance - tx.amount - tx.fee, nonce + 1));
    let (to_balance, to_nonce) = model_get(model, &tx.to);
    model.insert(tx.to, (to_balance + tx.amount, to_nonce));
    let (pb, pn) = model_get(model, proposer);
    model.insert(*proposer, (pb + tx.fee, pn));
}

fn model_root(model: &BTreeMap<Address, (u64, u64)>) -> Hash {
    let leaves: Vec<Vec<u8>> = model
        .iter()
        .filter(|(_, v)| **v != (0, 0))
        .map(|(a, (b, n))| {
            [
                a.as_slice(),
                b.to_le_bytes().as_slice(),
                n.to_le_bytes().as_slice(),
            ]
            .concat()
        })
        .collect();
    merkle_root(&leaves)
}

#[test]
fn every_append_error_is_reported_and_leaves_the_chain_unchanged() {
    let (mut chain, seeds) = fixture();
    let keys = keys_of(&seeds);
    let p = proposer_seed(&chain, &seeds);
    let good_tx = Tx::signed(&seeds[0], keys[1], 10, 1, 0, Vec::new());
    let good = chain.propose(&p, 1, vec![good_tx.clone()]).unwrap();

    let mut case = good.clone();
    case.header.height = 2;
    expect_append_error(&mut chain, &keys, case, BlockError::BadHeight);

    let mut case = good.clone();
    case.header.parent = [1u8; 32];
    expect_append_error(&mut chain, &keys, case, BlockError::BadParent);

    let stale = chain.propose(&p, 0, Vec::new()).unwrap();
    expect_append_error(&mut chain, &keys, stale, BlockError::BadTime);

    let outsider = chain.propose(&seeds[5], 1, Vec::new()).unwrap();
    expect_append_error(&mut chain, &keys, outsider, BlockError::WrongProposer);

    let mut case = good.clone();
    case.header.sig[0] ^= 1;
    expect_append_error(&mut chain, &keys, case, BlockError::BadSignature);

    let mut case = good.clone();
    case.header.time = 2;
    expect_append_error(&mut chain, &keys, case, BlockError::BadSignature);

    let huge = signed_block(&p, &chain, 1, vec![good_tx.clone(); MAX_TXS + 1]);
    expect_append_error(&mut chain, &keys, huge, BlockError::TooLarge);

    let mut case = good.clone();
    case.txs[0] = Tx::signed(&seeds[0], keys[1], 11, 1, 0, Vec::new());
    expect_append_error(&mut chain, &keys, case, BlockError::BadTxRoot);

    let bad_nonce = Tx::signed(&seeds[0], keys[1], 1, 0, 5, Vec::new());
    let block = signed_block(&p, &chain, 1, vec![good_tx.clone(), bad_nonce]);
    let want = BlockError::BadTx {
        index: 1,
        error: TxError::BadNonce,
    };
    expect_append_error(&mut chain, &keys, block, want);

    let mut case = good.clone();
    case.header.state_root = [1u8; 32];
    case.header.sign(&p);
    expect_append_error(&mut chain, &keys, case, BlockError::BadStateRoot);

    // Precedence: height before parent, signature before size, tx root before transactions.
    let mut case = good.clone();
    case.header.height = 9;
    case.header.parent = [1u8; 32];
    expect_append_error(&mut chain, &keys, case, BlockError::BadHeight);

    let mut case = signed_block(&p, &chain, 1, vec![good_tx.clone(); MAX_TXS + 1]);
    case.header.sig = [0u8; 64];
    expect_append_error(&mut chain, &keys, case, BlockError::BadSignature);

    let bad_nonce = Tx::signed(&seeds[0], keys[1], 1, 0, 5, Vec::new());
    let mut case = signed_block(&p, &chain, 1, vec![good_tx.clone()]);
    case.txs = vec![bad_nonce];
    expect_append_error(&mut chain, &keys, case, BlockError::BadTxRoot);

    let outsider_fake = signed_block(&seeds[5], &chain, 1, vec![good_tx.clone()]);
    let mut case = outsider_fake;
    case.header.sig = [0u8; 64];
    expect_append_error(&mut chain, &keys, case, BlockError::WrongProposer);

    // The chain still accepts the good block afterwards.
    chain.append(good.clone()).unwrap();
    assert_eq!(chain.height(), 1);
    assert_eq!(chain.block(1), Some(&good));
    assert_eq!(chain.tip().hash(), good.hash());
    assert_eq!(chain.account(&keys[1]), (1011, 0));
}

#[test]
fn propose_reports_the_first_bad_transaction() {
    let (chain, seeds) = fixture();
    let keys = keys_of(&seeds);
    let p = proposer_seed(&chain, &seeds);
    let good = Tx::signed(&seeds[0], keys[1], 10, 1, 0, Vec::new());

    let poor = Tx::signed(&seeds[0], keys[1], 10_000, 0, 0, Vec::new());
    assert_eq!(
        chain.propose(&p, 1, vec![poor]).map(|_| ()),
        Err(BlockError::BadTx {
            index: 0,
            error: TxError::InsufficientFunds
        })
    );
    let wrap = Tx::signed(&seeds[0], keys[1], u64::MAX, 1, 0, Vec::new());
    assert_eq!(
        chain.propose(&p, 1, vec![wrap]).map(|_| ()),
        Err(BlockError::BadTx {
            index: 0,
            error: TxError::InsufficientFunds
        })
    );
    let big = Tx::signed(&seeds[0], keys[1], 1, 0, 1, vec![0; MAX_DATA + 1]);
    assert_eq!(
        chain.propose(&p, 1, vec![good.clone(), big]).map(|_| ()),
        Err(BlockError::BadTx {
            index: 1,
            error: TxError::TooLarge
        })
    );
    let mut forged = good.clone();
    forged.sig[3] ^= 1;
    assert_eq!(
        chain.propose(&p, 1, vec![good.clone(), forged]).map(|_| ()),
        Err(BlockError::BadTx {
            index: 1,
            error: TxError::BadSignature
        })
    );
    assert_eq!(
        chain
            .propose(&p, 1, vec![good.clone(); MAX_TXS + 1])
            .map(|_| ()),
        Err(BlockError::TooLarge)
    );

    let genesis = [(keys[0], 10u64), (keys[1], u64::MAX)];
    let full = Chain::new(&genesis, &keys[..1]);
    let to_full = Tx::signed(&seeds[0], keys[1], 1, 0, 0, Vec::new());
    assert_eq!(
        full.propose(&seeds[0], 1, vec![to_full]).map(|_| ()),
        Err(BlockError::BadTx {
            index: 0,
            error: TxError::Overflow
        })
    );
}

#[test]
fn random_bytes_never_panic_the_decoders() {
    forall(3000, 0xD3C0, |rng| {
        let bytes = rng.bytes(400);
        let _ = Tx::decode(&bytes);
        let _ = Block::decode(&bytes);
    });
}

#[test]
fn mutated_block_encodings_never_panic_and_decode_canonically() {
    let (chain, seeds) = fixture();
    let p = proposer_seed(&chain, &seeds);
    let keys = keys_of(&seeds);
    let txs = vec![
        Tx::signed(&seeds[0], keys[1], 10, 1, 0, vec![4; 9]),
        Tx::signed(&seeds[0], keys[2], 3, 0, 1, Vec::new()),
    ];
    let base = chain.propose(&p, 1, txs).unwrap().encode();
    forall(2000, 0xF022, |rng| {
        let mut bytes = base.clone();
        for _ in 0..1 + rng.below(4) {
            let i = rng.below(bytes.len());
            bytes[i] = rng.u64() as u8;
        }
        if rng.chance(1, 4) {
            let n = rng.below(bytes.len());
            bytes.truncate(n);
        }
        if let Ok(block) = Block::decode(&bytes) {
            assert_eq!(block.encode(), bytes);
        }
    });
}
