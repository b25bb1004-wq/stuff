# seam

seam is the chain layer of Strata. It holds blocks, a Merkle tree over transactions, signed
transfers, account state, and the rules that decide whether a block can extend the chain. It is
written for Strata's own zero-dependency stack, so it uses only the standard library and the
other Strata crates.

The reason for the crate is to keep chain rules in one place. A block is accepted only if its
height, parent, time, proposer, signature, transactions, and resulting state root all check out,
and a failed check leaves the chain unchanged. Peers exchange blocks and transactions through
the sans-IO `node` module, so the same rules apply to every block a node receives.

## Features

- Block headers and blocks, with byte encoding, hashing, and ed25519 header signatures.
- Signed transfers with amount, fee, nonce, and up to `MAX_DATA` bytes of payload.
- An RFC 6962 Merkle tree: roots, audit paths, and path verification.
- Account state of balances and nonces, with a state root for each block.
- Chain validation: `Chain::propose` builds a block and `Chain::append` checks one before it
  becomes the tip. Proposers rotate through the validator list by height.
- Contract addresses and receipts, plus read access to contract code and storage.
- A node that keeps a block tree, an orphan pool, a mempool, and fork choice, and counts reorgs.
- Sync and gossip messages (`Msg`) with byte encoding, and a sans-IO peer over Tremor links.

## Public API

| Item | What it does |
| --- | --- |
| `Chain::new(genesis, validators)` | Start a chain from balances and a validator list |
| `Chain::propose(seed, time, txs)` | Build the next block signed by the key of `seed` |
| `Chain::append(block)` | Validate a block and make it the tip |
| `Chain::account(addr)` | `(balance, nonce)` of an address |
| `Tx::signed(seed, to, amount, fee, nonce, data)` | Build and sign a transfer |
| `Tx::verify()`, `Tx::encode()`, `Tx::decode(bytes)` | Check a signature and round-trip bytes |
| `Block::encode()`, `Block::decode(bytes)` | Round-trip a whole block |
| `merkle_root`, `merkle_proof`, `merkle_verify` | Build a root, an audit path, and check a leaf |
| `node::Node::new`, `handle`, `propose` | Keep a node's mempool and tree; propose a block |
| `node::net::Peer` | Sans-IO gossip peer over Tremor links |

Errors are `TxError`, `BlockError`, and `DecodeError`.

## Demo

The demo funds one account, signs three transfers, proposes and validates a block, proves a
transaction is in the block, and shows a tampered transaction being refused. It opens no
sockets.

```
cargo run -p seam --example demo
```

Output:

```
genesis: alice has (1000, 0) (balance, nonce)
signed 3 transactions, all verify: true
decode(encode(tx0)) equals tx0: true
block 1 appended: height 1, 3 transactions
alice: (740, 2), bob: (249, 1)
merkle proof for tx 1 (2 hashes) verifies: true
node mempool: 1 transaction(s)
node proposed block at height 1 with 1 transaction(s)
tampered tx verifies: false
propose refused tampered tx at index 0: bad signature
```

## Tests

```
cargo test -p seam
```

Unit tests are in the source modules and in `crates/seam/src/node/tests.rs`. Integration tests
are in `crates/seam/tests`.
