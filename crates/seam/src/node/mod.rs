//! Node state: block tree, orphan pool, fork choice, reorgs, mempool, and message handling.

mod msg;
pub mod net;

#[cfg(test)]
mod tests;

pub use msg::Msg;

use crate::block::Block;
use crate::chain::Chain;
use crate::tx::Tx;
use crate::{Address, BlockError, Hash, MAX_TXS};
use onyx::ed25519;
use std::collections::{HashMap, HashSet, VecDeque};

/// Maximum number of orphan blocks retained before evicting the oldest.
const MAX_ORPHANS: usize = 256;

/// A validated block together with the chain state ending at that block.
#[derive(Debug, Clone)]
struct ValidBlock {
    block: Block,
    chain: Chain,
}

/// A blockchain node maintaining a tree of valid blocks, an orphan pool, and a mempool.
#[derive(Debug, Clone)]
pub struct Node {
    tip: Hash,
    chosen: Chain,
    chosen_txs: HashSet<Hash>,
    valid: HashMap<Hash, ValidBlock>,
    orphans: HashMap<Hash, Block>,
    orphan_order: VecDeque<Hash>,
    invalid: HashSet<Hash>,
    mempool: Vec<Tx>,
    mempool_ids: HashSet<Hash>,
    reorgs: u64,
}

impl Node {
    /// Create a node initialized at genesis with the given balances and validator set.
    pub fn new(genesis: &[(Address, u64)], validators: &[Address]) -> Node {
        let chain = Chain::new(genesis, validators);
        let genesis_block = chain.genesis().clone();
        let tip = genesis_block.hash();
        let mut valid = HashMap::new();
        valid.insert(
            tip,
            ValidBlock {
                block: genesis_block,
                chain: chain.clone(),
            },
        );
        Node {
            tip,
            chosen: chain,
            chosen_txs: HashSet::new(),
            valid,
            orphans: HashMap::new(),
            orphan_order: VecDeque::new(),
            invalid: HashSet::new(),
            mempool: Vec::new(),
            mempool_ids: HashSet::new(),
            reorgs: 0,
        }
    }

    /// The currently chosen best chain.
    pub fn chain(&self) -> &Chain {
        &self.chosen
    }

    /// Hash of the chosen chain's tip header.
    pub fn tip(&self) -> Hash {
        self.tip
    }

    /// True if the block with hash `h` is stored (either valid or orphan).
    pub fn known(&self, h: &Hash) -> bool {
        self.valid.contains_key(h) || self.orphans.contains_key(h)
    }

    /// Parents of stored orphans that are neither known nor invalid, sorted and deduplicated.
    /// A sync loop re-requests these with `GetBlock`, so a lost request is never fatal.
    pub fn missing(&self) -> Vec<Hash> {
        let mut m: Vec<Hash> = self
            .orphans
            .values()
            .map(|b| b.header.parent)
            .filter(|p| !self.known(p) && !self.invalid.contains(p))
            .collect();
        m.sort_unstable();
        m.dedup();
        m
    }

    /// Pending transactions in the mempool, in arrival order without duplicates.
    pub fn pending(&self) -> &[Tx] {
        &self.mempool
    }

    /// Handle an incoming sync/gossip message, returning replies and gossip to broadcast.
    pub fn handle(&mut self, msg: Msg) -> Vec<Msg> {
        match msg {
            Msg::Block(block) => self.handle_block(block),
            Msg::Tip { hash, .. } => {
                if self.known(&hash) || self.invalid.contains(&hash) {
                    Vec::new()
                } else {
                    vec![Msg::GetBlock(hash)]
                }
            }
            Msg::GetBlock(hash) => match self.get_known_block(&hash) {
                Some(block) => vec![Msg::Block(block)],
                None => Vec::new(),
            },
            Msg::Tx(tx) => self.handle_tx(tx),
        }
    }

    /// Propose a new block on the node's own tip using valid mempool txs, append it, and return it.
    pub fn propose(&mut self, seed: &[u8; 32], time: u64) -> Option<Block> {
        let proposer = ed25519::public_key(seed);
        if proposer != self.chosen.expected_proposer() || time <= self.chosen.tip().time {
            return None;
        }
        let mut cursor = 0usize;
        let mut batch = Vec::with_capacity(self.mempool.len().min(MAX_TXS));
        Self::fill_batch(&self.mempool, &mut cursor, &mut batch);
        let block = loop {
            match self.chosen.propose(seed, time, batch.clone()) {
                Ok(block) => break block,
                Err(BlockError::BadTx { index, .. }) => {
                    if index >= batch.len() {
                        return None;
                    }
                    batch.remove(index);
                    Self::fill_batch(&self.mempool, &mut cursor, &mut batch);
                }
                Err(_) => return None,
            }
        };
        let mut next_chain = self.chosen.clone();
        if next_chain.append(block.clone()).is_err() {
            return None;
        }
        let hash = block.hash();
        self.valid.insert(
            hash,
            ValidBlock {
                block: block.clone(),
                chain: next_chain,
            },
        );
        self.connect_orphans_from(hash);
        self.update_tip();
        Some(block)
    }

    /// Number of times the chosen tip moved to a non-descendant block.
    pub fn reorgs(&self) -> u64 {
        self.reorgs
    }

    /// Append mempool txs from `cursor` into `batch` until `batch` reaches `MAX_TXS`.
    fn fill_batch(mempool: &[Tx], cursor: &mut usize, batch: &mut Vec<Tx>) {
        while batch.len() < MAX_TXS {
            let Some(tx) = mempool.get(*cursor) else {
                break;
            };
            *cursor = cursor.saturating_add(1);
            batch.push(tx.clone());
        }
    }

    /// Retrieve a clone of a known block (valid or orphan).
    fn get_known_block(&self, hash: &Hash) -> Option<Block> {
        if let Some(vb) = self.valid.get(hash) {
            return Some(vb.block.clone());
        }
        self.orphans.get(hash).cloned()
    }

    /// Process an incoming transaction, adding to mempool and gossiping once if new.
    fn handle_tx(&mut self, tx: Tx) -> Vec<Msg> {
        let id = tx.id();
        if self.mempool_ids.contains(&id) || self.chosen_txs.contains(&id) {
            return Vec::new();
        }
        self.mempool_ids.insert(id);
        self.mempool.push(tx.clone());
        vec![Msg::Tx(tx)]
    }

    /// Process an incoming block: validate, connect orphans, or buffer as an orphan.
    fn handle_block(&mut self, block: Block) -> Vec<Msg> {
        let hash = block.hash();
        if self.known(&hash) || self.invalid.contains(&hash) {
            return Vec::new();
        }
        let parent = block.header.parent;
        if self.invalid.contains(&parent) {
            self.mark_invalid_with_descendants(hash);
            return Vec::new();
        }
        if let Some(parent_chain) = self.valid.get(&parent).map(|v| &v.chain) {
            let mut next_chain = parent_chain.clone();
            if next_chain.append(block.clone()).is_err() {
                self.mark_invalid_with_descendants(hash);
                return Vec::new();
            }
            self.valid.insert(
                hash,
                ValidBlock {
                    block: block.clone(),
                    chain: next_chain,
                },
            );
            self.connect_orphans_from(hash);
            if self.update_tip() {
                return vec![
                    Msg::Block(block),
                    Msg::Tip {
                        hash: self.tip,
                        height: self.chosen.height(),
                    },
                ];
            }
            return Vec::new();
        }
        match self.missing_ancestor(&parent) {
            AncestorStatus::Missing(missing) => {
                if missing == hash {
                    self.mark_invalid_with_descendants(hash);
                    return Vec::new();
                }
                self.insert_orphan(hash, block);
                vec![Msg::GetBlock(missing)]
            }
            AncestorStatus::Invalid => {
                self.mark_invalid_with_descendants(hash);
                Vec::new()
            }
            AncestorStatus::Cycle => Vec::new(),
        }
    }

    /// Follow orphan parents up to the missing root ancestor, detecting invalid ancestors or cycles.
    fn missing_ancestor(&self, start_parent: &Hash) -> AncestorStatus {
        let mut cur = *start_parent;
        let mut seen = HashSet::new();
        while let Some(orphan) = self.orphans.get(&cur) {
            if !seen.insert(cur) {
                return AncestorStatus::Cycle;
            }
            cur = orphan.header.parent;
            if self.invalid.contains(&cur) {
                return AncestorStatus::Invalid;
            }
        }
        if self.invalid.contains(&cur) {
            AncestorStatus::Invalid
        } else {
            AncestorStatus::Missing(cur)
        }
    }

    /// Insert an orphan block, evicting the oldest orphan if the pool is at capacity.
    fn insert_orphan(&mut self, hash: Hash, block: Block) {
        if self.orphans.contains_key(&hash) {
            return;
        }
        while self.orphans.len() >= MAX_ORPHANS {
            let Some(oldest) = self.orphan_order.pop_front() else {
                break;
            };
            self.orphans.remove(&oldest);
        }
        self.orphans.insert(hash, block);
        self.orphan_order.push_back(hash);
    }

    /// Remove and return all orphan blocks whose parent hash equals `parent`.
    fn take_orphans_with_parent(&mut self, parent: &Hash) -> Vec<(Hash, Block)> {
        let matching: Vec<Hash> = self
            .orphan_order
            .iter()
            .copied()
            .filter(|h| {
                self.orphans
                    .get(h)
                    .is_some_and(|b| b.header.parent == *parent)
            })
            .collect();
        let mut out = Vec::with_capacity(matching.len());
        if matching.is_empty() {
            return out;
        }
        let match_set: HashSet<Hash> = matching.iter().copied().collect();
        self.orphan_order.retain(|h| !match_set.contains(h));
        for h in matching {
            if let Some(b) = self.orphans.remove(&h) {
                out.push((h, b));
            }
        }
        out
    }

    /// Repeatedly validate and connect any orphans descending from `root_hash`.
    fn connect_orphans_from(&mut self, root_hash: Hash) {
        let mut queue = VecDeque::new();
        queue.push_back(root_hash);
        while let Some(parent_hash) = queue.pop_front() {
            let children = self.take_orphans_with_parent(&parent_hash);
            for (child_hash, child_block) in children {
                let Some(parent_chain) = self.valid.get(&parent_hash).map(|v| &v.chain) else {
                    continue;
                };
                let mut next_chain = parent_chain.clone();
                if next_chain.append(child_block.clone()).is_ok() {
                    self.valid.insert(
                        child_hash,
                        ValidBlock {
                            block: child_block,
                            chain: next_chain,
                        },
                    );
                    queue.push_back(child_hash);
                } else {
                    self.mark_invalid_with_descendants(child_hash);
                }
            }
        }
    }

    /// Mark `root` and every orphan descendant of `root` as invalid and drop them from orphans.
    fn mark_invalid_with_descendants(&mut self, root: Hash) {
        let mut queue = VecDeque::new();
        queue.push_back(root);
        while let Some(bad_hash) = queue.pop_front() {
            if !self.invalid.insert(bad_hash) {
                continue;
            }
            for (child_hash, _) in self.take_orphans_with_parent(&bad_hash) {
                queue.push_back(child_hash);
            }
        }
    }

    /// Re-evaluate fork choice; if tip changed, count reorgs, update mempool, and return true.
    fn update_tip(&mut self) -> bool {
        let mut best_hash = self.tip;
        let mut best_height = self.chosen.height();
        for (h, vb) in &self.valid {
            let height = vb.chain.height();
            if height > best_height || (height == best_height && *h < best_hash) {
                best_height = height;
                best_hash = *h;
            }
        }
        if best_hash == self.tip {
            return false;
        }
        let Some(new_chain) = self.valid.get(&best_hash).map(|v| v.chain.clone()) else {
            return false;
        };
        let old_height = self.chosen.height();
        let is_descendant = new_chain.block(old_height).map(Block::hash) == Some(self.tip);
        if !is_descendant {
            self.reorgs = self.reorgs.saturating_add(1);
        }
        let max_common = old_height.min(new_chain.height());
        let mut lca_height = 0u64;
        for h in (1..=max_common).rev() {
            let old_h = self.chosen.block(h).map(Block::hash);
            let new_h = new_chain.block(h).map(Block::hash);
            if old_h.is_some() && old_h == new_h {
                lca_height = h;
                break;
            }
        }
        for h in lca_height.saturating_add(1)..=old_height {
            if let Some(blk) = self.chosen.block(h) {
                for tx in &blk.txs {
                    self.chosen_txs.remove(&tx.id());
                }
            }
        }
        for h in lca_height.saturating_add(1)..=new_chain.height() {
            if let Some(blk) = new_chain.block(h) {
                for tx in &blk.txs {
                    self.chosen_txs.insert(tx.id());
                }
            }
        }
        let chosen_txs = &self.chosen_txs;
        let mempool_ids = &mut self.mempool_ids;
        self.mempool.retain(|tx| {
            let id = tx.id();
            if chosen_txs.contains(&id) {
                mempool_ids.remove(&id);
                false
            } else {
                true
            }
        });
        for h in lca_height.saturating_add(1)..=old_height {
            if let Some(blk) = self.chosen.block(h) {
                for tx in &blk.txs {
                    let id = tx.id();
                    if !self.chosen_txs.contains(&id) && self.mempool_ids.insert(id) {
                        self.mempool.push(tx.clone());
                    }
                }
            }
        }
        self.tip = best_hash;
        self.chosen = new_chain;
        true
    }
}

/// Result of tracing an orphan's ancestor chain.
enum AncestorStatus {
    Missing(Hash),
    Invalid,
    Cycle,
}
