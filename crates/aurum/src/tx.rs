//! Signed transfer transactions and their byte encoding.

use crate::codec::{len_u32, to_usize, Reader};
use crate::{Address, DecodeError, Hash, MAX_DATA};
use sphinx::{ed25519, sha256};

/// Domain separator prefixed to the body before signing.
const TX_DOMAIN: &[u8] = b"aurum-tx\0";

/// A signed transfer of `amount` from `from` to `to`, paying `fee` to the block proposer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tx {
    /// The sender, the public key that signed the transaction.
    pub from: Address,
    /// The recipient.
    pub to: Address,
    /// The amount transferred.
    pub amount: u64,
    /// The fee paid to the block proposer.
    pub fee: u64,
    /// The sender's nonce; must equal the sender's next nonce.
    pub nonce: u64,
    /// Opaque payload, at most `MAX_DATA` bytes.
    pub data: Vec<u8>,
    /// The ed25519 signature over the domain-tagged body.
    pub sig: [u8; 64],
}

impl Tx {
    /// A transaction from the key derived from `seed`, signed with it.
    pub fn signed(
        seed: &[u8; 32],
        to: Address,
        amount: u64,
        fee: u64,
        nonce: u64,
        data: Vec<u8>,
    ) -> Tx {
        let mut tx = Tx {
            from: ed25519::public_key(seed),
            to,
            amount,
            fee,
            nonce,
            data,
            sig: [0u8; 64],
        };
        tx.sig = ed25519::sign(seed, &tx.signing_message());
        tx
    }

    /// The unsigned encoding: `from to amount fee nonce len data`.
    pub fn body(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(92 + self.data.len());
        out.extend_from_slice(&self.from);
        out.extend_from_slice(&self.to);
        out.extend_from_slice(&self.amount.to_le_bytes());
        out.extend_from_slice(&self.fee.to_le_bytes());
        out.extend_from_slice(&self.nonce.to_le_bytes());
        out.extend_from_slice(&len_u32(self.data.len()).to_le_bytes());
        out.extend_from_slice(&self.data);
        out
    }

    /// The full encoding: `body || sig`.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.body();
        out.extend_from_slice(&self.sig);
        out
    }

    /// Decode exactly one transaction; trailing bytes are an error.
    pub fn decode(bytes: &[u8]) -> Result<Tx, DecodeError> {
        let mut reader = Reader::new(bytes);
        let tx = Tx::read(&mut reader)?;
        reader.finish()?;
        Ok(tx)
    }

    /// The transaction id: `sha256(encode)`.
    pub fn id(&self) -> Hash {
        sha256::digest(&self.encode())
    }

    /// True when `sig` is a valid signature by `from` over the body.
    pub fn verify(&self) -> bool {
        ed25519::verify(&self.from, &self.signing_message(), &self.sig).is_ok()
    }

    /// The bytes that are signed: the domain tag followed by the body.
    fn signing_message(&self) -> Vec<u8> {
        let mut message = TX_DOMAIN.to_vec();
        message.extend_from_slice(&self.body());
        message
    }

    /// Read one transaction from a shared reader, for use inside a block.
    pub(crate) fn read(reader: &mut Reader) -> Result<Tx, DecodeError> {
        let from = reader.array()?;
        let to = reader.array()?;
        let amount = reader.u64()?;
        let fee = reader.u64()?;
        let nonce = reader.u64()?;
        let len = to_usize(reader.u32()?);
        if len > MAX_DATA {
            return Err(DecodeError::TooLarge);
        }
        let data = reader.take(len)?.to_vec();
        let sig = reader.array()?;
        Ok(Tx {
            from,
            to,
            amount,
            fee,
            nonce,
            data,
            sig,
        })
    }
}
