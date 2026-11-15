#![allow(clippy::unwrap_used)]

use super::*;
use crate::tx::Tx;
use crate::DecodeError;
use assay::forall;
use onyx::ed25519;

fn seed(n: u8) -> [u8; 32] {
    [n; 32]
}

fn addr(n: u8) -> Address {
    ed25519::public_key(&seed(n))
}

#[test]
fn msg_round_trips_and_hostile_bytes() {
    let s1 = seed(1);
    let s2 = seed(2);
    let mut chain = Chain::new(&[(addr(1), 1000)], &[addr(1)]);
    let tx = Tx::signed(&s1, addr(2), 10, 1, 0, b"hi".to_vec());
    let blk = chain.propose(&s1, 5, vec![tx.clone()]).unwrap();
    chain.append(blk.clone()).unwrap();

    let msgs = [
        Msg::Block(blk.clone()),
        Msg::Tip {
            hash: blk.hash(),
            height: 1,
        },
        Msg::GetBlock(blk.hash()),
        Msg::Tx(tx.clone()),
    ];
    for m in &msgs {
        let enc = m.encode();
        assert_eq!(Msg::decode(&enc), Ok(m.clone()));
    }

    assert_eq!(Msg::decode(&[]), Err(DecodeError::Truncated));
    assert_eq!(Msg::decode(&[4]), Err(DecodeError::TooLarge));
    assert_eq!(Msg::decode(&[255, 0, 1]), Err(DecodeError::TooLarge));

    for m in &msgs {
        let enc = m.encode();
        if enc.len() > 1 {
            assert_eq!(
                Msg::decode(&enc[..enc.len() - 1]),
                Err(DecodeError::Truncated)
            );
        }
        let mut with_trailing = enc.clone();
        with_trailing.push(0);
        assert_eq!(Msg::decode(&with_trailing), Err(DecodeError::TrailingBytes));
    }

    let _ = s2;
}

#[test]
fn msg_property_round_trip() {
    forall(64, 0x5ea1, |rng| {
        let mut h = [0u8; 32];
        rng.fill(&mut h);
        let height = rng.u64();
        let tip = Msg::Tip { hash: h, height };
        assert_eq!(Msg::decode(&tip.encode()), Ok(tip));

        let gb = Msg::GetBlock(h);
        assert_eq!(Msg::decode(&gb.encode()), Ok(gb));

        let mut s = [0u8; 32];
        rng.fill(&mut s);
        let mut to = [0u8; 32];
        rng.fill(&mut to);
        let data = rng.bytes(64);
        let tx = Tx::signed(&s, to, rng.u64(), rng.u64(), rng.u64(), data);
        let m_tx = Msg::Tx(tx);
        assert_eq!(Msg::decode(&m_tx.encode()), Ok(m_tx));
    });
}

#[test]
fn orphan_pool_evicts_oldest_at_256() {
    let vals = [addr(1)];
    let mut node = Node::new(&[(addr(1), 1000)], &vals);

    // Create 257 orphan blocks on distinct unknown parents.
    let mut first_hash = [0u8; 32];
    let mut hashes = Vec::new();
    for i in 0..257u64 {
        let mut chain = Chain::new(&[(addr(1), 1000)], &vals);
        // Make a distinct fake parent chain by varying block 1's timestamp.
        let b1 = chain.propose(&seed(1), 10 + i, vec![]).unwrap();
        chain.append(b1).unwrap();
        // Propose b2 whose parent is b1 (unknown to `node`).
        let b2 = chain.propose(&seed(1), 1000 + i, vec![]).unwrap();
        let h2 = b2.hash();
        if i == 0 {
            first_hash = h2;
        }
        hashes.push(h2);
        let out = node.handle(Msg::Block(b2));
        assert!(matches!(out.as_slice(), [Msg::GetBlock(_)]));
    }

    assert!(!node.known(&first_hash));
    for h in &hashes[1..] {
        assert!(node.known(h));
    }
}

#[test]
fn propose_checks_validator_time_and_skips_bad_txs() {
    let vals = [addr(1), addr(2)];
    let mut node = Node::new(&[(addr(1), 100), (addr(2), 100)], &vals);

    // Height 1 expects validator[1] == addr(2).
    assert!(node.propose(&seed(1), 10).is_none());
    // Time 0 is not > genesis time 0.
    assert!(node.propose(&seed(2), 0).is_none());

    let bad_nonce_tx = Tx::signed(&seed(1), addr(2), 10, 1, 5, vec![]);
    let good_tx = Tx::signed(&seed(1), addr(2), 10, 1, 0, vec![]);
    let too_rich_tx = Tx::signed(&seed(2), addr(1), 1000, 1, 0, vec![]);

    assert_eq!(node.handle(Msg::Tx(bad_nonce_tx.clone())).len(), 1);
    // Duplicate is ignored.
    assert!(node.handle(Msg::Tx(bad_nonce_tx.clone())).is_empty());
    assert_eq!(node.handle(Msg::Tx(good_tx.clone())).len(), 1);
    assert_eq!(node.handle(Msg::Tx(too_rich_tx.clone())).len(), 1);

    let blk = node.propose(&seed(2), 10).unwrap();
    assert_eq!(blk.txs, vec![good_tx.clone()]);
    assert_eq!(node.chain().height(), 1);
    assert_eq!(node.pending(), &[bad_nonce_tx, too_rich_tx]);
    // Already-included tx is not re-added to mempool.
    assert!(node.handle(Msg::Tx(good_tx)).is_empty());
}
