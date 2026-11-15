//! Benchmarks for `aurum`: Merkle trees, signature verification, and block execution.

use aurum::{merkle_proof, merkle_root, merkle_verify, Address, Chain, Tx};
use sphinx::ed25519;

use crate::{measure, Opts, Sample};

/// Run all `aurum` benchmarks.
pub fn run(opts: &Opts) -> Result<Vec<Sample>, String> {
    Ok(vec![
        bench_merkle_root(opts)?,
        bench_merkle_proof(opts)?,
        bench_tx_verify(opts)?,
        bench_chain_blocks(opts)?,
    ])
}

fn make_leaves(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| {
            let mut leaf = vec![0u8; 64];
            leaf[..8].copy_from_slice(&(i as u64).to_le_bytes());
            for (j, b) in leaf[8..].iter_mut().enumerate() {
                *b = ((i + j) as u8).wrapping_mul(17);
            }
            leaf
        })
        .collect()
}

fn bench_merkle_root(opts: &Opts) -> Result<Sample, String> {
    let leaves = make_leaves(1024);
    let expected = merkle_root(&leaves);

    measure("aurum", "merkle_root", "roots/s", opts.budget, || {
        let got = merkle_root(std::hint::black_box(&leaves));
        if got != expected {
            return Err("merkle_root mismatch".to_string());
        }
        Ok(1)
    })
}

fn bench_merkle_proof(opts: &Opts) -> Result<Sample, String> {
    let leaves = make_leaves(1024);
    let root = merkle_root(&leaves);
    let mut idx = 0usize;

    measure("aurum", "merkle_proof", "proofs/s", opts.budget, || {
        let proof = merkle_proof(std::hint::black_box(&leaves), idx);
        let leaf = leaves
            .get(idx)
            .ok_or_else(|| "leaf index out of range".to_string())?;
        if !merkle_verify(&root, leaf, idx, leaves.len(), &proof) {
            return Err(format!("merkle_verify failed at index {idx}"));
        }
        idx = (idx + 1) % leaves.len();
        Ok(1)
    })
}

fn seed_for(idx: u64) -> [u8; 32] {
    let mut seed = [0u8; 32];
    seed[..8].copy_from_slice(&idx.to_le_bytes());
    seed[8..16].copy_from_slice(&idx.wrapping_add(1).to_le_bytes());
    seed
}

fn bench_tx_verify(opts: &Opts) -> Result<Sample, String> {
    const BATCH: usize = 16;
    let to_seed = seed_for(999);
    let to_addr = ed25519::public_key(&to_seed);

    let txs: Vec<Tx> = (0..BATCH)
        .map(|i| {
            let s = seed_for((i + 1) as u64);
            Tx::signed(&s, to_addr, 100, 1, 0, vec![i as u8; 16])
        })
        .collect();

    measure("aurum", "tx_verify", "tx/s", opts.budget, || {
        for tx in &txs {
            if !std::hint::black_box(tx).verify() {
                return Err("tx signature verification failed".to_string());
            }
        }
        Ok(BATCH as u64)
    })
}

fn bench_chain_blocks(opts: &Opts) -> Result<Sample, String> {
    const TXS_PER_BLOCK: usize = 100;
    const CYCLE_BLOCKS: usize = 4;

    let val_seed = seed_for(10_000);
    let val_addr = ed25519::public_key(&val_seed);
    let recip_seed = seed_for(20_000);
    let recip_addr = ed25519::public_key(&recip_seed);

    let sender_seeds: Vec<[u8; 32]> = (1..=TXS_PER_BLOCK).map(|i| seed_for(i as u64)).collect();
    let genesis: Vec<(Address, u64)> = sender_seeds
        .iter()
        .map(|s| (ed25519::public_key(s), 1_000_000))
        .collect();

    let base_chain = Chain::new(&genesis, &[val_addr]);
    let mut block_txs = Vec::with_capacity(CYCLE_BLOCKS);
    for nonce in 0..CYCLE_BLOCKS {
        let mut txs = Vec::with_capacity(TXS_PER_BLOCK);
        for s in &sender_seeds {
            txs.push(Tx::signed(s, recip_addr, 10, 1, nonce as u64, Vec::new()));
        }
        block_txs.push(txs);
    }

    let mut chain = base_chain.clone();
    let mut step = 0usize;
    let mut time = 1u64;

    measure("aurum", "chain_blocks", "blocks/s", opts.budget, || {
        let txs = block_txs
            .get(step)
            .ok_or_else(|| "block_txs index out of range".to_string())?
            .clone();
        let block = chain
            .propose(&val_seed, time, txs)
            .map_err(|e| format!("chain propose failed: {e:?}"))?;
        chain
            .append(block)
            .map_err(|e| format!("chain append failed: {e:?}"))?;

        let expected_height = (step + 1) as u64;
        let expected_bal = expected_height.saturating_mul((TXS_PER_BLOCK as u64) * 10);
        if chain.height() != expected_height || chain.account(&recip_addr).0 != expected_bal {
            return Err("chain state verification mismatch".to_string());
        }

        time = time.saturating_add(1);
        step += 1;
        if step == CYCLE_BLOCKS {
            chain = base_chain.clone();
            step = 0;
            time = 1;
        }
        Ok(1)
    })
}
