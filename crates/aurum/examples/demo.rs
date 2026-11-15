//! Demo: fund an account, sign transfers, propose and validate a block, prove one transaction
//! is in the block with a Merkle proof, and reject a tampered transaction. Deterministic: fixed
//! seeds, no clock, no network.

use aurum::node::{Msg, Node};
use aurum::{merkle_proof, merkle_root, merkle_verify, Address, BlockError, Chain, Tx, TxError};

/// The address of the key derived from `seed`. Signing any transaction reveals it in `from`.
fn address(seed: &[u8; 32]) -> Address {
    Tx::signed(seed, [0u8; 32], 0, 0, 0, Vec::new()).from
}

fn main() {
    let alice_seed = [1u8; 32];
    let bob_seed = [2u8; 32];
    let validator_seed = [9u8; 32];
    let alice = address(&alice_seed);
    let bob = address(&bob_seed);
    let validator = address(&validator_seed);

    // Genesis credits alice with 1000 and names one validator, who proposes every block.
    let mut chain = Chain::new(&[(alice, 1000)], &[validator]);
    println!(
        "genesis: alice has {:?} (balance, nonce)",
        chain.account(&alice)
    );

    let txs = vec![
        Tx::signed(&alice_seed, bob, 100, 5, 0, b"rent".to_vec()),
        Tx::signed(&alice_seed, bob, 200, 5, 1, Vec::new()),
        Tx::signed(&bob_seed, alice, 50, 1, 0, b"thanks".to_vec()),
    ];
    println!(
        "signed {} transactions, all verify: {}",
        txs.len(),
        txs.iter().all(Tx::verify)
    );

    // Transactions round-trip through their byte encoding.
    let decoded = Tx::decode(&txs[0].encode());
    println!(
        "decode(encode(tx0)) equals tx0: {}",
        decoded.as_ref() == Ok(&txs[0])
    );

    let block = chain
        .propose(&validator_seed, 1, txs.clone())
        .expect("propose");
    chain.append(block.clone()).expect("append");
    println!(
        "block 1 appended: height {}, {} transactions",
        chain.height(),
        block.txs.len()
    );
    println!(
        "alice: {:?}, bob: {:?}",
        chain.account(&alice),
        chain.account(&bob)
    );

    // Merkle proof that transaction 1 is in the block.
    let leaves: Vec<Vec<u8>> = block.txs.iter().map(Tx::encode).collect();
    let root = merkle_root(&leaves);
    let proof = merkle_proof(&leaves, 1);
    let ok = merkle_verify(&root, &leaves[1], 1, leaves.len(), &proof);
    println!(
        "merkle proof for tx 1 ({} hashes) verifies: {ok}",
        proof.len()
    );

    // The same transfer through a node: mempool first, then the node proposes the block.
    let mut node = Node::new(&[(alice, 1000)], &[validator]);
    node.handle(Msg::Tx(txs[0].clone()));
    println!("node mempool: {} transaction(s)", node.pending().len());
    let made = node.propose(&validator_seed, 1).expect("node propose");
    println!(
        "node proposed block at height {} with {} transaction(s)",
        node.chain().height(),
        made.txs.len()
    );

    // A tampered transaction fails its signature check and is refused by the chain.
    let mut bad = txs[0].clone();
    bad.amount = 999;
    println!("tampered tx verifies: {}", bad.verify());
    match chain.propose(&validator_seed, 2, vec![bad]) {
        Err(BlockError::BadTx {
            index,
            error: TxError::BadSignature,
        }) => {
            println!("propose refused tampered tx at index {index}: bad signature");
        }
        other => println!("unexpected result: {other:?}"),
    }
}
