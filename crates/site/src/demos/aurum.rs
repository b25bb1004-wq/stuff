//! Aurum signed-transfer, block proposal, and RFC 6962 Merkle proof demo.

use std::fmt::Write as _;

use aurum::{merkle_proof, merkle_root, merkle_verify, Address, Chain, Tx};
use sphinx::ed25519;

use super::{check_input, hex_lower, Demo, Output};

const MAX_LINES: usize = 256;
const GENESIS_BALANCE: u64 = 1_000;
const BLOCK_COUNT: usize = 3;

const ACCOUNTS: [(&str, [u8; 32]); 3] = [
    ("alice", [0xA1; 32]),
    ("bob", [0xB2; 32]),
    ("carol", [0xC3; 32]),
];

const DEFAULT_TXS: &str = "\
alice bob 100
alice carol 25
bob carol 40
carol alice 15";

/// Registered Aurum blockchain demo.
pub const DEMO: Demo = Demo {
    id: "aurum",
    title: "Aurum Chain",
    default_input: DEFAULT_TXS,
    run,
};

struct TransferSpec {
    from_idx: usize,
    to_idx: usize,
    amount: u64,
}

/// Build a 3-block Aurum demo chain from `from to amount` transfer lines and verify a Merkle proof.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let specs = parse_transfers(input)?;

    let keys: [Address; 3] = [
        ed25519::public_key(&ACCOUNTS[0].1),
        ed25519::public_key(&ACCOUNTS[1].1),
        ed25519::public_key(&ACCOUNTS[2].1),
    ];
    let genesis: Vec<(Address, u64)> = keys.iter().map(|&k| (k, GENESIS_BALANCE)).collect();
    let mut chain = Chain::new(&genesis, &keys);

    let mut nonces = [0u64; 3];
    let mut signed_txs = Vec::with_capacity(specs.len());
    for spec in specs {
        let sender_idx = spec.from_idx;
        let nonce = nonces[sender_idx];
        nonces[sender_idx] = nonce.saturating_add(1);
        signed_txs.push(Tx::signed(
            &ACCOUNTS[sender_idx].1,
            keys[spec.to_idx],
            spec.amount,
            0,
            nonce,
            Vec::new(),
        ));
    }

    let buckets = distribute_into_blocks(signed_txs);
    let mut out = String::new();
    out.push_str("demo keys:\n");
    for (idx, (name, _)) in ACCOUNTS.iter().enumerate() {
        let _ = writeln!(out, "  {name}: {}", short_hex(&keys[idx]));
    }

    let mut proof_target: Option<(u64, Vec<Vec<u8>>, [u8; 32])> = None;
    for (blk_idx, block_txs) in buckets.into_iter().enumerate() {
        let height = (blk_idx + 1) as u64;
        let slot = usize::try_from(height % (ACCOUNTS.len() as u64)).unwrap_or(0);
        let proposer_name = ACCOUNTS[slot].0;
        let proposer_seed = &ACCOUNTS[slot].1;
        let block = chain
            .propose(proposer_seed, height, block_txs)
            .map_err(|e| format!("block #{height} propose failed: {e:?}"))?;
        let block_hash = block.header.hash();
        let tx_root = block.header.tx_root;
        let tx_count = block.txs.len();
        if proof_target.is_none() && tx_count > 0 {
            let leaves: Vec<Vec<u8>> = block.txs.iter().map(Tx::encode).collect();
            proof_target = Some((height, leaves, tx_root));
        }
        chain
            .append(block)
            .map_err(|e| format!("block #{height} append failed: {e:?}"))?;
        let _ = writeln!(
            out,
            "block #{height} (proposer {proposer_name}): hash={} tx_root={} txs={tx_count}",
            short_hex(&block_hash),
            short_hex(&tx_root)
        );
    }

    if let Some((height, leaves, expected_root)) = proof_target {
        let computed_root = merkle_root(&leaves);
        let proof = merkle_proof(&leaves, 0);
        let verified = merkle_verify(&computed_root, &leaves[0], 0, leaves.len(), &proof);
        let _ = writeln!(
            out,
            "merkle root (block #{height}): {} (matches header: {})",
            short_hex(&computed_root),
            computed_root == expected_root
        );
        let _ = writeln!(
            out,
            "inclusion proof tx[0] (siblings={}, leaves={}): verified={verified}",
            proof.len(),
            leaves.len()
        );
    }

    out.push_str("final balances:\n");
    for (idx, (name, _)) in ACCOUNTS.iter().enumerate() {
        let (bal, nonce) = chain.account(&keys[idx]);
        let _ = writeln!(out, "  {name}: {bal} (nonce {nonce})");
    }

    Ok(Output::Text(out))
}

fn parse_transfers(input: &str) -> Result<Vec<TransferSpec>, String> {
    let mut out = Vec::new();
    let mut count = 0usize;
    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        count += 1;
        if count > MAX_LINES {
            return Err(format!("line {line_no}: exceeds {MAX_LINES} lines"));
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(format!(
                "line {line_no}: expected `from to amount`, got `{line}`"
            ));
        }
        let from_idx = find_account(parts[0]).ok_or_else(|| {
            format!(
                "line {line_no}: unknown sender `{}` (use alice, bob, carol)",
                parts[0]
            )
        })?;
        let to_idx = find_account(parts[1]).ok_or_else(|| {
            format!(
                "line {line_no}: unknown recipient `{}` (use alice, bob, carol)",
                parts[1]
            )
        })?;
        let amount = parts[2]
            .parse::<u64>()
            .map_err(|_| format!("line {line_no}: invalid amount `{}`", parts[2]))?;
        if amount == 0 {
            return Err(format!("line {line_no}: amount must be positive"));
        }
        out.push(TransferSpec {
            from_idx,
            to_idx,
            amount,
        });
    }
    if out.is_empty() {
        return Err(String::from("empty transfer script"));
    }
    Ok(out)
}

fn find_account(name: &str) -> Option<usize> {
    ACCOUNTS.iter().position(|(n, _)| *n == name)
}

fn distribute_into_blocks(txs: Vec<Tx>) -> [Vec<Tx>; BLOCK_COUNT] {
    let mut buckets: [Vec<Tx>; BLOCK_COUNT] = [Vec::new(), Vec::new(), Vec::new()];
    let total = txs.len();
    for (i, tx) in txs.into_iter().enumerate() {
        let b = if total <= BLOCK_COUNT {
            i
        } else {
            (i * BLOCK_COUNT) / total
        };
        buckets[b.min(BLOCK_COUNT - 1)].push(tx);
    }
    buckets
}

fn short_hex(bytes: &[u8; 32]) -> String {
    let full = hex_lower(bytes);
    full[..16].to_owned()
}
