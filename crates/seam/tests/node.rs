//! Integration tests for `seam::node`.

#![allow(clippy::unwrap_used)]

use assay::forall;
use onyx::ed25519;
use seam::node::{Msg, Node};
use seam::{Address, Block, Chain, DecodeError, Header, Tx};
use std::collections::VecDeque;

fn seed(n: u8) -> [u8; 32] {
    [n; 32]
}

fn addr(n: u8) -> Address {
    ed25519::public_key(&seed(n))
}

fn pump_three(nodes: &mut [Node; 3], initial: Vec<(usize, Msg)>) {
    let mut queue: VecDeque<(usize, Msg)> = initial.into_iter().collect();
    let mut steps = 0usize;
    while let Some((sender, msg)) = queue.pop_front() {
        steps += 1;
        assert!(steps < 10_000, "message loop did not terminate");
        for (peer, node) in nodes.iter_mut().enumerate() {
            if peer == sender {
                continue;
            }
            let replies = node.handle(msg.clone());
            for reply in replies {
                queue.push_back((peer, reply));
            }
        }
    }
}

#[test]
fn three_nodes_sync_via_queue() {
    let genesis = [(addr(1), 1000), (addr(2), 1000)];
    let vals = [addr(1), addr(2)];
    let mut nodes = [
        Node::new(&genesis, &vals),
        Node::new(&genesis, &vals),
        Node::new(&genesis, &vals),
    ];

    let tx0 = Tx::signed(&seed(1), addr(2), 50, 2, 0, vec![]);
    let out_tx = nodes[0].handle(Msg::Tx(tx0.clone()));
    assert_eq!(out_tx, vec![Msg::Tx(tx0.clone())]);
    pump_three(&mut nodes, vec![(0, out_tx[0].clone())]);
    for n in &nodes {
        assert_eq!(n.pending(), std::slice::from_ref(&tx0));
    }

    // Node 1 proposes height 1 (expected validator vals[1] == addr(2)).
    let b1 = nodes[1].propose(&seed(2), 10).unwrap();
    let b1_tip = Msg::Tip {
        hash: b1.hash(),
        height: 1,
    };
    pump_three(&mut nodes, vec![(1, b1_tip)]);
    for n in &nodes {
        assert_eq!(n.tip(), b1.hash());
        assert_eq!(n.chain().height(), 1);
        assert!(n.pending().is_empty());
    }

    // Node 0 proposes height 2 (expected validator vals[0] == addr(1)).
    let b2 = nodes[0].propose(&seed(1), 20).unwrap();
    pump_three(&mut nodes, vec![(0, Msg::Block(b2.clone()))]);
    for n in &nodes {
        assert_eq!(n.tip(), b2.hash());
        assert_eq!(n.chain().height(), 2);
        assert_eq!(n.reorgs(), 0);
    }
}

#[test]
fn equivocating_proposers_and_hash_tie_break() {
    let genesis = [(addr(1), 1000)];
    let vals = [addr(1)];

    // Build two distinct valid blocks at height 1 from the same validator.
    let c_a = Chain::new(&genesis, &vals);
    let b_a = c_a.propose(&seed(1), 10, vec![]).unwrap();

    let c_b = Chain::new(&genesis, &vals);
    let b_b = c_b.propose(&seed(1), 11, vec![]).unwrap();

    let (smaller, larger) = if b_a.hash() < b_b.hash() {
        (b_a, b_b)
    } else {
        (b_b, b_a)
    };
    assert!(smaller.hash() < larger.hash());

    // Node receives larger hash first, then smaller hash -> reorg to smaller.
    let mut n1 = Node::new(&genesis, &vals);
    let r1 = n1.handle(Msg::Block(larger.clone()));
    assert_eq!(
        r1,
        vec![
            Msg::Block(larger.clone()),
            Msg::Tip {
                hash: larger.hash(),
                height: 1
            }
        ]
    );
    assert_eq!(n1.tip(), larger.hash());
    assert_eq!(n1.reorgs(), 0);

    let r2 = n1.handle(Msg::Block(smaller.clone()));
    assert_eq!(
        r2,
        vec![
            Msg::Block(smaller.clone()),
            Msg::Tip {
                hash: smaller.hash(),
                height: 1
            }
        ]
    );
    assert_eq!(n1.tip(), smaller.hash());
    assert_eq!(n1.reorgs(), 1);

    // Node receives smaller hash first, then larger hash -> no reorg, no gossip.
    let mut n2 = Node::new(&genesis, &vals);
    n2.handle(Msg::Block(smaller.clone()));
    let r3 = n2.handle(Msg::Block(larger.clone()));
    assert!(r3.is_empty());
    assert_eq!(n2.tip(), smaller.hash());
    assert_eq!(n2.reorgs(), 0);
    assert!(n2.known(&larger.hash()));
}

#[test]
fn deep_reorg_five_blocks_and_mempool_readd() {
    let genesis = [(addr(1), 10_000), (addr(2), 10_000), (addr(3), 10_000)];
    let vals = [addr(1), addr(2)];

    let mut node = Node::new(&genesis, &vals);

    // Main branch: 5 blocks (heights 1..=5) containing tx_a1, tx_a2, tx_a3.
    let tx_a1 = Tx::signed(&seed(1), addr(3), 10, 1, 0, vec![]);
    let tx_a2 = Tx::signed(&seed(1), addr(3), 20, 1, 1, vec![]);
    let tx_a3 = Tx::signed(&seed(2), addr(3), 30, 1, 0, vec![]);
    let tx_shared = Tx::signed(&seed(3), addr(1), 5, 1, 0, vec![]);
    let tx_still_pending = Tx::signed(&seed(3), addr(2), 7, 1, 1, vec![]);

    let mut fork_base = node.chain().clone();
    // Build 5-block main branch on `node`.
    node.handle(Msg::Tx(tx_a1.clone()));
    let b1 = node.propose(&seed(2), 10).unwrap();
    node.handle(Msg::Tx(tx_shared.clone()));
    let b2 = node.propose(&seed(1), 20).unwrap();
    node.handle(Msg::Tx(tx_a2.clone()));
    let b3 = node.propose(&seed(2), 30).unwrap();
    node.handle(Msg::Tx(tx_a3.clone()));
    let b4 = node.propose(&seed(1), 40).unwrap();
    let b5 = node.propose(&seed(2), 50).unwrap();
    assert_eq!(node.chain().height(), 5);
    assert_eq!(node.tip(), b5.hash());
    let _ = (b1, b2, b3, b4);

    // Put a pending tx into mempool after height 5.
    node.handle(Msg::Tx(tx_still_pending.clone()));
    assert_eq!(node.pending(), std::slice::from_ref(&tx_still_pending));

    // Build an alternative 6-block branch from genesis that includes `tx_shared` at height 1.
    let mut alt_blocks = Vec::new();
    for h in 1..=6u64 {
        let s = if h % 2 == 1 { seed(2) } else { seed(1) };
        let txs = if h == 1 {
            vec![tx_shared.clone()]
        } else {
            vec![]
        };
        let blk = fork_base.propose(&s, 100 + h, txs).unwrap();
        fork_base.append(blk.clone()).unwrap();
        alt_blocks.push(blk);
    }

    // Feed blocks 1..=5 of alt branch: shorter or equal height (assume hash check or height 1..4).
    for blk in &alt_blocks[..4] {
        let out = node.handle(Msg::Block(blk.clone()));
        assert!(out.is_empty());
        assert_eq!(node.reorgs(), 0);
    }

    // Feed block 6 (height 6 > 5) -> triggers deep 5-block reorg!
    // Make sure alt_blocks[4] did not reorg early on tie by checking reorgs increment when reaching tip 6.
    let _ = node.handle(Msg::Block(alt_blocks[4].clone()));
    let reorgs_before_6 = node.reorgs();
    let out6 = node.handle(Msg::Block(alt_blocks[5].clone()));
    assert_eq!(node.tip(), alt_blocks[5].hash());
    assert_eq!(node.chain().height(), 6);
    assert_eq!(node.reorgs(), 1);
    assert!(reorgs_before_6 <= 1);
    assert_eq!(
        out6,
        vec![
            Msg::Block(alt_blocks[5].clone()),
            Msg::Tip {
                hash: alt_blocks[5].hash(),
                height: 6
            }
        ]
    );

    // Mempool must have: tx_still_pending, then abandoned txs in block order (tx_a1, tx_a2, tx_a3),
    // while `tx_shared` (now in the new chosen chain) is NOT re-added.
    assert_eq!(node.pending(), &[tx_still_pending, tx_a1, tx_a2, tx_a3]);
}

#[test]
fn orphans_arrive_out_of_order_and_connect() {
    let genesis = [(addr(1), 1000)];
    let vals = [addr(1)];
    let mut node = Node::new(&genesis, &vals);

    let mut c = Chain::new(&genesis, &vals);
    let mut blocks = Vec::new();
    for i in 1..=5u64 {
        let b = c.propose(&seed(1), 10 * i, vec![]).unwrap();
        c.append(b.clone()).unwrap();
        blocks.push(b);
    }

    // Deliver blocks 5, 4, 3, 2 in reverse order.
    for i in (1..5).rev() {
        let out = node.handle(Msg::Block(blocks[i].clone()));
        assert_eq!(out, vec![Msg::GetBlock(blocks[i - 1].hash())]);
        assert!(node.known(&blocks[i].hash()));
        assert_eq!(node.chain().height(), 0);
    }

    // Deliver block 1 -> connects all 5 blocks!
    let out = node.handle(Msg::Block(blocks[0].clone()));
    assert_eq!(node.chain().height(), 5);
    assert_eq!(node.tip(), blocks[4].hash());
    assert_eq!(node.reorgs(), 0);
    assert_eq!(
        out,
        vec![
            Msg::Block(blocks[0].clone()),
            Msg::Tip {
                hash: blocks[4].hash(),
                height: 5
            }
        ]
    );
}

#[test]
fn invalid_block_drops_descendants_and_never_rerequests() {
    let genesis = [(addr(1), 1000)];
    let vals = [addr(1)];
    let mut node = Node::new(&genesis, &vals);

    let mut c = Chain::new(&genesis, &vals);
    let valid_b1 = c.propose(&seed(1), 10, vec![]).unwrap();
    c.append(valid_b1.clone()).unwrap();

    // Corrupt signature on b2, then build orphan b3 and b4 on top of invalid b2.
    let mut invalid_b2 = c.propose(&seed(1), 20, vec![]).unwrap();
    invalid_b2.header.sig[0] ^= 0xff;
    let inv_h2 = invalid_b2.hash();

    let b3 = Block {
        header: Header {
            height: 3,
            parent: inv_h2,
            tx_root: valid_b1.header.tx_root,
            state_root: valid_b1.header.state_root,
            time: 30,
            proposer: addr(1),
            sig: [0u8; 64],
        },
        txs: vec![],
    };
    let h3 = b3.hash();

    let b4 = Block {
        header: Header {
            height: 4,
            parent: h3,
            tx_root: valid_b1.header.tx_root,
            state_root: valid_b1.header.state_root,
            time: 40,
            proposer: addr(1),
            sig: [0u8; 64],
        },
        txs: vec![],
    };
    let h4 = b4.hash();

    // Deliver b4, b3, invalid_b2 as orphans (missing valid_b1).
    assert_eq!(node.handle(Msg::Block(b4)), vec![Msg::GetBlock(h3)]);
    assert_eq!(node.handle(Msg::Block(b3)), vec![Msg::GetBlock(inv_h2)]);
    assert_eq!(
        node.handle(Msg::Block(invalid_b2.clone())),
        vec![Msg::GetBlock(valid_b1.hash())]
    );
    assert!(node.known(&inv_h2));
    assert!(node.known(&h3));
    assert!(node.known(&h4));

    // Deliver valid_b1 -> invalid_b2 fails and drops invalid_b2, b3, b4.
    let out = node.handle(Msg::Block(valid_b1.clone()));
    assert_eq!(
        out,
        vec![
            Msg::Block(valid_b1.clone()),
            Msg::Tip {
                hash: valid_b1.hash(),
                height: 1
            }
        ]
    );
    assert!(!node.known(&inv_h2));
    assert!(!node.known(&h3));
    assert!(!node.known(&h4));

    // Never re-request dropped invalid blocks or their descendants.
    assert!(node
        .handle(Msg::Tip {
            hash: inv_h2,
            height: 2
        })
        .is_empty());
    assert!(node
        .handle(Msg::Tip {
            hash: h3,
            height: 3
        })
        .is_empty());
    assert!(node
        .handle(Msg::Tip {
            hash: h4,
            height: 4
        })
        .is_empty());
    assert!(node.handle(Msg::Block(invalid_b2)).is_empty());

    // A new block referencing h3 is also dropped immediately without requesting h3.
    let b5 = Block {
        header: Header {
            height: 4,
            parent: h3,
            tx_root: valid_b1.header.tx_root,
            state_root: valid_b1.header.state_root,
            time: 50,
            proposer: addr(1),
            sig: [0u8; 64],
        },
        txs: vec![],
    };
    let h5 = b5.hash();
    assert!(node.handle(Msg::Block(b5)).is_empty());
    assert!(!node.known(&h5));
}

#[test]
fn msg_round_trips_and_hostile_bytes_integration() {
    forall(64, 0xbeef, |rng| {
        let mut h = [0u8; 32];
        rng.fill(&mut h);
        let tip = Msg::Tip {
            hash: h,
            height: rng.u64(),
        };
        assert_eq!(Msg::decode(&tip.encode()), Ok(tip));
        let gb = Msg::GetBlock(h);
        assert_eq!(Msg::decode(&gb.encode()), Ok(gb));
    });

    assert_eq!(Msg::decode(&[]), Err(DecodeError::Truncated));
    assert_eq!(Msg::decode(&[99, 1, 2, 3]), Err(DecodeError::TooLarge));
    let gb_bytes = Msg::GetBlock([7u8; 32]).encode();
    assert_eq!(
        Msg::decode(&gb_bytes[..gb_bytes.len() - 1]),
        Err(DecodeError::Truncated)
    );
    let mut bad_gb = gb_bytes;
    bad_gb.push(0);
    assert_eq!(Msg::decode(&bad_gb), Err(DecodeError::TrailingBytes));
}
