//! Account state: balances and nonces, with a Merkle state root.

use crate::merkle::merkle_root;
use crate::tx::Tx;
use crate::{Address, Hash, TxError, MAX_DATA};
use std::collections::BTreeMap;

/// Every account's `(balance, nonce)`, keyed by address.
#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    accounts: BTreeMap<Address, (u64, u64)>,
}

impl State {
    /// The genesis state. A repeated address adds up; saturates instead of overflowing.
    pub(crate) fn from_genesis(allocs: &[(Address, u64)]) -> State {
        let mut state = State::default();
        for (address, amount) in allocs {
            let (balance, nonce) = state.account(address);
            state
                .accounts
                .insert(*address, (balance.saturating_add(*amount), nonce));
        }
        state
    }

    /// `(balance, nonce)` of `address`; `(0, 0)` if unknown.
    pub(crate) fn account(&self, address: &Address) -> (u64, u64) {
        self.accounts.get(address).copied().unwrap_or((0, 0))
    }

    /// The Merkle root over the 48-byte leaves of every account that is not `(0, 0)`.
    pub(crate) fn root(&self) -> Hash {
        let mut leaves = Vec::with_capacity(self.accounts.len());
        for (address, &(balance, nonce)) in &self.accounts {
            if (balance, nonce) == (0, 0) {
                continue;
            }
            let mut leaf = Vec::with_capacity(48);
            leaf.extend_from_slice(address);
            leaf.extend_from_slice(&balance.to_le_bytes());
            leaf.extend_from_slice(&nonce.to_le_bytes());
            leaves.push(leaf);
        }
        merkle_root(&leaves)
    }

    /// Apply one transaction, paying the fee to `proposer`. On error the state may be partly
    /// changed, so callers apply transactions to a copy and discard it on failure.
    pub(crate) fn apply(&mut self, tx: &Tx, proposer: &Address) -> Result<(), TxError> {
        if tx.data.len() > MAX_DATA {
            return Err(TxError::TooLarge);
        }
        if !tx.verify() {
            return Err(TxError::BadSignature);
        }
        let (balance, nonce) = self.account(&tx.from);
        if tx.nonce != nonce {
            return Err(TxError::BadNonce);
        }
        let left = tx
            .amount
            .checked_add(tx.fee)
            .and_then(|total| balance.checked_sub(total))
            .ok_or(TxError::InsufficientFunds)?;
        let next_nonce = nonce.checked_add(1).ok_or(TxError::Overflow)?;
        self.accounts.insert(tx.from, (left, next_nonce));
        self.credit(&tx.to, tx.amount)?;
        self.credit(proposer, tx.fee)
    }

    /// Add `amount` to the balance of `address`.
    fn credit(&mut self, address: &Address, amount: u64) -> Result<(), TxError> {
        let (balance, nonce) = self.account(address);
        let total = balance.checked_add(amount).ok_or(TxError::Overflow)?;
        self.accounts.insert(*address, (total, nonce));
        Ok(())
    }
}
