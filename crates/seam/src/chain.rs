//! The chain: genesis, validator rotation, block proposal, and validated append.

use crate::block::{Block, Header};
use crate::contract::Receipt;
use crate::merkle::merkle_root;
use crate::state::State;
use crate::tx::Tx;
use crate::{Address, BlockError, Hash, MAX_TXS};
use onyx::ed25519;

/// An in-memory chain of blocks with its account state.
#[derive(Debug, Clone)]
pub struct Chain {
    genesis: Block,
    /// Blocks after genesis; `blocks[i]` has height `i + 1`.
    blocks: Vec<Block>,
    /// Receipts for each block after genesis; `receipts[i]` corresponds to `blocks[i]`.
    receipts: Vec<Vec<Receipt>>,
    state: State,
    validators: Vec<Address>,
}

impl Chain {
    /// A chain whose genesis credits `genesis` balances and rotates proposers through
    /// `validators`, which must be non-empty.
    pub fn new(genesis: &[(Address, u64)], validators: &[Address]) -> Chain {
        let state = State::from_genesis(genesis);
        let header = Header {
            height: 0,
            parent: [0u8; 32],
            tx_root: merkle_root(&[]),
            state_root: state.root(),
            time: 0,
            proposer: [0u8; 32],
            sig: [0u8; 64],
        };
        Chain {
            genesis: Block {
                header,
                txs: Vec::new(),
            },
            blocks: Vec::new(),
            receipts: Vec::new(),
            state,
            validators: validators.to_vec(),
        }
    }

    /// The genesis block.
    pub fn genesis(&self) -> &Block {
        &self.genesis
    }

    /// The header of the tip block.
    pub fn tip(&self) -> &Header {
        &self.tip_block().header
    }

    /// The height of the tip block.
    pub fn height(&self) -> u64 {
        u64::try_from(self.blocks.len()).unwrap_or(u64::MAX)
    }

    /// The block at `height`, if the chain has reached it.
    pub fn block(&self, height: u64) -> Option<&Block> {
        if height == 0 {
            return Some(&self.genesis);
        }
        usize::try_from(height - 1)
            .ok()
            .and_then(|index| self.blocks.get(index))
    }

    /// `(balance, nonce)` of `address`; `(0, 0)` if unknown.
    pub fn account(&self, address: &Address) -> (u64, u64) {
        self.state.account(address)
    }

    /// Transaction receipts for the block at `height`; `None` for genesis or unknown height.
    pub fn receipts(&self, height: u64) -> Option<&[Receipt]> {
        if height == 0 {
            return None;
        }
        usize::try_from(height - 1)
            .ok()
            .and_then(|index| self.receipts.get(index).map(Vec::as_slice))
    }

    /// The Mica source code deployed at `contract`, if any.
    pub fn code(&self, contract: &Address) -> Option<&str> {
        self.state.code(contract)
    }

    /// The value stored under `key` in `contract`'s storage, if present.
    pub fn storage(&self, contract: &Address, key: &[u8]) -> Option<&[u8]> {
        self.state.storage(contract, key)
    }

    /// The Merkle root of the current account state.
    pub fn state_root(&self) -> Hash {
        self.state.root()
    }

    /// The validator allowed to propose the next block.
    pub fn expected_proposer(&self) -> Address {
        let count = u64::try_from(self.validators.len()).unwrap_or(u64::MAX);
        if count == 0 {
            return [0u8; 32];
        }
        let slot = self.height().wrapping_add(1) % count;
        usize::try_from(slot)
            .ok()
            .and_then(|index| self.validators.get(index))
            .copied()
            .unwrap_or([0u8; 32])
    }

    /// Build the next block from `txs`, proposed by the key of `seed`. Does not check that the
    /// proposer is expected or that `time` is new; fails only with `TooLarge` or the first bad
    /// transaction.
    pub fn propose(&self, seed: &[u8; 32], time: u64, txs: Vec<Tx>) -> Result<Block, BlockError> {
        if txs.len() > MAX_TXS {
            return Err(BlockError::TooLarge);
        }
        let proposer = ed25519::public_key(seed);
        let mut state = self.state.clone();
        apply_all(&mut state, &txs, &proposer)?;
        let mut header = Header {
            height: self.height().wrapping_add(1),
            parent: self.tip().hash(),
            tx_root: tx_root(&txs),
            state_root: state.root(),
            time,
            proposer,
            sig: [0u8; 64],
        };
        header.sign(seed);
        Ok(Block { header, txs })
    }

    /// Validate `block` and make it the tip. Checks run in a fixed order and the first failure
    /// is returned with the chain unchanged.
    pub fn append(&mut self, block: Block) -> Result<(), BlockError> {
        let header = &block.header;
        let tip = self.tip();
        if header.height != self.height().wrapping_add(1) {
            return Err(BlockError::BadHeight);
        }
        if header.parent != tip.hash() {
            return Err(BlockError::BadParent);
        }
        if header.time <= tip.time {
            return Err(BlockError::BadTime);
        }
        if header.proposer != self.expected_proposer() {
            return Err(BlockError::WrongProposer);
        }
        if !header.verify() {
            return Err(BlockError::BadSignature);
        }
        if block.txs.len() > MAX_TXS {
            return Err(BlockError::TooLarge);
        }
        if header.tx_root != tx_root(&block.txs) {
            return Err(BlockError::BadTxRoot);
        }
        let mut state = self.state.clone();
        let receipts = apply_all(&mut state, &block.txs, &header.proposer)?;
        if header.state_root != state.root() {
            return Err(BlockError::BadStateRoot);
        }
        self.state = state;
        self.blocks.push(block);
        self.receipts.push(receipts);
        Ok(())
    }

    /// The tip block; genesis when no block has been appended.
    fn tip_block(&self) -> &Block {
        self.blocks.last().unwrap_or(&self.genesis)
    }
}

/// Merkle root of the transaction encodings.
fn tx_root(txs: &[Tx]) -> Hash {
    let leaves: Vec<Vec<u8>> = txs.iter().map(Tx::encode).collect();
    merkle_root(&leaves)
}

/// Apply `txs` in order, reporting the first failure with its index.
fn apply_all(
    state: &mut State,
    txs: &[Tx],
    proposer: &Address,
) -> Result<Vec<Receipt>, BlockError> {
    let mut receipts = Vec::with_capacity(txs.len());
    for (index, tx) in txs.iter().enumerate() {
        let receipt = state
            .apply(tx, proposer)
            .map_err(|error| BlockError::BadTx { index, error })?;
        receipts.push(receipt);
    }
    Ok(receipts)
}
