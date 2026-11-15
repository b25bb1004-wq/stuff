//! aurum: blocks, an RFC 6962 Merkle tree, signed transfers, account state and chain validation.
#![forbid(unsafe_code)]

mod block;
mod chain;
mod codec;
mod contract;
mod merkle;
pub mod node;
mod state;
mod tx;

pub use block::{Block, Header};
pub use chain::Chain;
pub use contract::{contract_address, Receipt, DEPLOY, FUEL_PER_FEE};
pub use merkle::{merkle_proof, merkle_root, merkle_verify};
pub use tx::Tx;

/// A 32-byte SHA-256 digest.
pub type Hash = [u8; 32];
/// An account address: an ed25519 public key.
pub type Address = [u8; 32];
/// Largest length of `Tx::data`, in bytes.
pub const MAX_DATA: usize = 1024;
/// Largest number of transactions in one block.
pub const MAX_TXS: usize = 1000;

/// Why a byte string could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The input ended before a field was complete.
    Truncated,
    /// Bytes remain after the last field.
    TrailingBytes,
    /// A length or count field is above its limit.
    TooLarge,
}

/// Why a transaction cannot be applied to the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxError {
    /// `data` is longer than `MAX_DATA`.
    TooLarge,
    /// The signature does not match `from`.
    BadSignature,
    /// The nonce is not the sender's next nonce.
    BadNonce,
    /// `amount + fee` overflows or exceeds the sender's balance.
    InsufficientFunds,
    /// A balance or nonce would overflow `u64`.
    Overflow,
}

/// Why a block cannot be appended to the chain (or proposed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// The height is not the tip height plus one.
    BadHeight,
    /// The parent hash is not the tip hash.
    BadParent,
    /// The time is not after the tip time.
    BadTime,
    /// The proposer is not the expected validator for this height.
    WrongProposer,
    /// The header signature does not verify.
    BadSignature,
    /// More than `MAX_TXS` transactions.
    TooLarge,
    /// `tx_root` does not match the transactions.
    BadTxRoot,
    /// Transaction `index` failed with `error`.
    BadTx {
        /// Position of the failing transaction in the block.
        index: usize,
        /// The reason it failed.
        error: TxError,
    },
    /// `state_root` does not match the state after the transactions.
    BadStateRoot,
}
