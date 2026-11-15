//! Network messages for block gossip, tip announcements, block requests, and transactions.

use crate::block::Block;
use crate::codec::Reader;
use crate::tx::Tx;
use crate::{DecodeError, Hash};

/// Tag bytes for encoded `Msg` variants.
const TAG_BLOCK: u8 = 0;
const TAG_TIP: u8 = 1;
const TAG_GET_BLOCK: u8 = 2;
const TAG_TX: u8 = 3;

/// Plain-value sync and gossip messages exchanged between `Node` instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// A full block.
    Block(Block),
    /// A tip announcement with block hash and chain height.
    Tip {
        /// Hash of the announced tip header.
        hash: Hash,
        /// Height of the announced tip block.
        height: u64,
    },
    /// Request for the block with the given header hash.
    GetBlock(Hash),
    /// A signed transaction for gossip and mempool inclusion.
    Tx(Tx),
}

impl Msg {
    /// Encode this message as a tagged byte vector.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Msg::Block(block) => {
                let body = block.encode();
                let mut out = Vec::with_capacity(1 + body.len());
                out.push(TAG_BLOCK);
                out.extend_from_slice(&body);
                out
            }
            Msg::Tip { hash, height } => {
                let mut out = Vec::with_capacity(1 + 32 + 8);
                out.push(TAG_TIP);
                out.extend_from_slice(hash);
                out.extend_from_slice(&height.to_le_bytes());
                out
            }
            Msg::GetBlock(hash) => {
                let mut out = Vec::with_capacity(1 + 32);
                out.push(TAG_GET_BLOCK);
                out.extend_from_slice(hash);
                out
            }
            Msg::Tx(tx) => {
                let body = tx.encode();
                let mut out = Vec::with_capacity(1 + body.len());
                out.push(TAG_TX);
                out.extend_from_slice(&body);
                out
            }
        }
    }

    /// Decode a message from bytes; trailing bytes or unknown tags are errors.
    pub fn decode(b: &[u8]) -> Result<Msg, DecodeError> {
        let Some((&tag, rest)) = b.split_first() else {
            return Err(DecodeError::Truncated);
        };
        match tag {
            TAG_BLOCK => Block::decode(rest).map(Msg::Block),
            TAG_TIP => {
                let mut reader = Reader::new(rest);
                let hash = reader.array()?;
                let height = reader.u64()?;
                reader.finish()?;
                Ok(Msg::Tip { hash, height })
            }
            TAG_GET_BLOCK => {
                let mut reader = Reader::new(rest);
                let hash = reader.array()?;
                reader.finish()?;
                Ok(Msg::GetBlock(hash))
            }
            TAG_TX => Tx::decode(rest).map(Msg::Tx),
            _ => Err(DecodeError::TooLarge),
        }
    }
}
