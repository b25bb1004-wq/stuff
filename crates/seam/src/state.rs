//! Account and contract state: balances, nonces, code, storage, and Merkle state root.

use crate::contract::{
    contract_address, execute_contract, Contract, Receipt, DEPLOY, FUEL_PER_FEE,
};
use crate::merkle::merkle_root;
use crate::tx::Tx;
use crate::{Address, Hash, TxError, MAX_DATA};
use std::collections::BTreeMap;

/// Every account's `(balance, nonce)` and deployed contracts, keyed by address.
#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    accounts: BTreeMap<Address, (u64, u64)>,
    contracts: BTreeMap<Address, Contract>,
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

    /// The Mica source code of `contract`, if deployed.
    pub(crate) fn code(&self, contract: &Address) -> Option<&str> {
        self.contracts.get(contract).map(|c| c.source.as_str())
    }

    /// The value stored under `key` in `contract`'s storage, if present.
    pub(crate) fn storage(&self, contract: &Address, key: &[u8]) -> Option<&[u8]> {
        self.contracts
            .get(contract)
            .and_then(|c| c.storage.get(key).map(Vec::as_slice))
    }

    /// The Merkle root over all non-zero account leaves and contract leaves, sorted by address.
    pub(crate) fn root(&self) -> Hash {
        let mut entries: Vec<(Address, u8, Vec<u8>)> =
            Vec::with_capacity(self.accounts.len() + self.contracts.len());
        for (address, &(balance, nonce)) in &self.accounts {
            if (balance, nonce) == (0, 0) {
                continue;
            }
            let mut leaf = Vec::with_capacity(48);
            leaf.extend_from_slice(address);
            leaf.extend_from_slice(&balance.to_le_bytes());
            leaf.extend_from_slice(&nonce.to_le_bytes());
            entries.push((*address, 0, leaf));
        }
        for (address, contract) in &self.contracts {
            entries.push((*address, 1, contract.state_leaf(address)));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let leaves: Vec<Vec<u8>> = entries.into_iter().map(|(_, _, leaf)| leaf).collect();
        merkle_root(&leaves)
    }

    /// Apply one transaction, paying the fee to `proposer`. On error the state may be partly
    /// changed, so callers apply transactions to a copy and discard it on failure.
    pub(crate) fn apply(&mut self, tx: &Tx, proposer: &Address) -> Result<Receipt, TxError> {
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

        if tx.to == DEPLOY && !tx.data.is_empty() {
            self.credit(proposer, tx.fee)?;
            return self.apply_deploy(tx, nonce);
        }
        if tx.to != DEPLOY && self.contracts.contains_key(&tx.to) {
            self.credit(proposer, tx.fee)?;
            return self.apply_call(tx);
        }

        self.credit(&tx.to, tx.amount)?;
        self.credit(proposer, tx.fee)?;
        Ok(Receipt {
            ok: true,
            fuel_used: 0,
            events: Vec::new(),
            contract: None,
        })
    }

    /// Deploy a contract from `tx.data`; on invalid source, refund `tx.amount` to sender.
    fn apply_deploy(&mut self, tx: &Tx, nonce: u64) -> Result<Receipt, TxError> {
        let Some(contract) = Contract::compile(&tx.data) else {
            self.credit(&tx.from, tx.amount)?;
            return Ok(Receipt {
                ok: false,
                fuel_used: 0,
                events: Vec::new(),
                contract: None,
            });
        };
        let addr = contract_address(&tx.from, nonce);
        self.credit(&addr, tx.amount)?;
        self.contracts.insert(addr, contract);
        Ok(Receipt {
            ok: true,
            fuel_used: 0,
            events: Vec::new(),
            contract: Some(addr),
        })
    }

    /// Call an existing contract at `tx.to`; on trap, drop writes/events and refund `tx.amount`.
    fn apply_call(&mut self, tx: &Tx) -> Result<Receipt, TxError> {
        self.credit(&tx.to, tx.amount)?;
        let fuel = tx.fee.saturating_mul(FUEL_PER_FEE);
        let Some(contract) = self.contracts.get(&tx.to) else {
            return Ok(Receipt {
                ok: false,
                fuel_used: 0,
                events: Vec::new(),
                contract: None,
            });
        };
        let outcome = execute_contract(contract, tx.from, tx.amount, &tx.data, fuel);
        if outcome.ok {
            if let Some(contract) = self.contracts.get_mut(&tx.to) {
                contract.storage.extend(outcome.writes);
            }
            return Ok(Receipt {
                ok: true,
                fuel_used: outcome.fuel_used,
                events: outcome.events,
                contract: None,
            });
        }
        self.debit(&tx.to, tx.amount)?;
        self.credit(&tx.from, tx.amount)?;
        Ok(Receipt {
            ok: false,
            fuel_used: outcome.fuel_used,
            events: Vec::new(),
            contract: None,
        })
    }

    /// Add `amount` to the balance of `address`.
    fn credit(&mut self, address: &Address, amount: u64) -> Result<(), TxError> {
        let (balance, nonce) = self.account(address);
        let total = balance.checked_add(amount).ok_or(TxError::Overflow)?;
        self.accounts.insert(*address, (total, nonce));
        Ok(())
    }

    /// Subtract `amount` from the balance of `address`.
    fn debit(&mut self, address: &Address, amount: u64) -> Result<(), TxError> {
        let (balance, nonce) = self.account(address);
        let left = balance
            .checked_sub(amount)
            .ok_or(TxError::InsufficientFunds)?;
        self.accounts.insert(*address, (left, nonce));
        Ok(())
    }
}
