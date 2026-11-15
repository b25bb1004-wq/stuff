//! Block headers, blocks, and their byte encodings.

use crate::codec::{len_u32, to_usize, Reader};
use crate::tx::Tx;
use crate::{Address, DecodeError, Hash, MAX_TXS};
use onyx::{ed25519, sha256};

/// Domain separator prefixed to the unsigned header before signing.
const HEADER_DOMAIN: &[u8] = b"seam-blk";

/// A block header. The proposer signs it; `sig` covers the first 144 encoded bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Height; the genesis block is height 0.
    pub height: u64,
    /// Hash of the parent header.
    pub parent: Hash,
    /// Merkle root of the transaction encodings.
    pub tx_root: Hash,
    /// Root of the account state after the block's transactions.
    pub state_root: Hash,
    /// Block time, strictly increasing along the chain.
    pub time: u64,
    /// The validator that proposed the block.
    pub proposer: Address,
    /// The proposer's signature over the header.
    pub sig: [u8; 64],
}

impl Header {
    /// The 208-byte encoding.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.unsigned();
        out.extend_from_slice(&self.sig);
        out
    }

    /// The header hash: `sha256(encode)`.
    pub fn hash(&self) -> Hash {
        sha256::digest(&self.encode())
    }

    /// Set `sig` to the proposer signature made with `seed`. The proposer field is untouched.
    pub fn sign(&mut self, seed: &[u8; 32]) {
        self.sig = ed25519::sign(seed, &self.signing_message());
    }

    /// True when `sig` is a valid signature by `proposer`.
    pub fn verify(&self) -> bool {
        ed25519::verify(&self.proposer, &self.signing_message(), &self.sig).is_ok()
    }

    /// The first 144 encoded bytes, everything except the signature.
    fn unsigned(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(208);
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.parent);
        out.extend_from_slice(&self.tx_root);
        out.extend_from_slice(&self.state_root);
        out.extend_from_slice(&self.time.to_le_bytes());
        out.extend_from_slice(&self.proposer);
        out
    }

    /// The bytes that are signed: the domain tag followed by the unsigned header.
    fn signing_message(&self) -> Vec<u8> {
        let mut message = HEADER_DOMAIN.to_vec();
        message.extend_from_slice(&self.unsigned());
        message
    }

    /// Read one header from a shared reader.
    fn read(reader: &mut Reader) -> Result<Header, DecodeError> {
        Ok(Header {
            height: reader.u64()?,
            parent: reader.array()?,
            tx_root: reader.array()?,
            state_root: reader.array()?,
            time: reader.u64()?,
            proposer: reader.array()?,
            sig: reader.array()?,
        })
    }
}

/// A header and the transactions it commits to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The signed header.
    pub header: Header,
    /// The transactions, applied in order.
    pub txs: Vec<Tx>,
}

impl Block {
    /// `header(208) count(u32) tx encodings`.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.header.encode();
        out.extend_from_slice(&len_u32(self.txs.len()).to_le_bytes());
        for tx in &self.txs {
            out.extend_from_slice(&tx.encode());
        }
        out
    }

    /// Decode exactly one block; trailing bytes are an error.
    pub fn decode(bytes: &[u8]) -> Result<Block, DecodeError> {
        let mut reader = Reader::new(bytes);
        let header = Header::read(&mut reader)?;
        let count = to_usize(reader.u32()?);
        if count > MAX_TXS {
            return Err(DecodeError::TooLarge);
        }
        let mut txs = Vec::with_capacity(count);
        for _ in 0..count {
            txs.push(Tx::read(&mut reader)?);
        }
        reader.finish()?;
        Ok(Block { header, txs })
    }

    /// The block hash, which is the header hash.
    pub fn hash(&self) -> Hash {
        self.header.hash()
    }
}
