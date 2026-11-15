//! Logos smart contracts executed on Elysium with buffered storage and events.

use crate::merkle::merkle_root;
use crate::{Address, Hash};
use cobalt::Compiled;
use elysium::{Heap, Host, Limits, Obj, Trap, Value, Vm};
use sphinx::sha256;
use std::collections::BTreeMap;

/// Sentinel recipient address that deploys a contract when `data` is non-empty.
pub const DEPLOY: Address = [0; 32];

/// Elysium fuel units granted per unit of transaction fee.
pub const FUEL_PER_FEE: u64 = 1000;

/// Maximum heap size in bytes for a single contract call (1 MiB).
const CONTRACT_MAX_HEAP_BYTES: usize = 1024 * 1024;

/// Maximum call frame depth for a single contract call.
const CONTRACT_MAX_DEPTH: u32 = 64;

/// Host imports permitted for on-chain contracts.
const ALLOWED_IMPORTS: [&str; 6] = [
    "chain.get",
    "chain.set",
    "chain.caller",
    "chain.emit",
    "chain.input",
    "chain.value",
];

/// Derive the contract address for `deployer` at `nonce`.
pub fn contract_address(deployer: &Address, nonce: u64) -> Address {
    let mut buf = Vec::with_capacity(13 + 32 + 8);
    buf.extend_from_slice(b"aurum-contract");
    buf.extend_from_slice(deployer);
    buf.extend_from_slice(&nonce.to_le_bytes());
    sha256::digest(&buf)
}

/// Execution outcome of a single transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// Whether the transfer, deploy, or contract call succeeded.
    pub ok: bool,
    /// Elysium fuel consumed by the transaction (0 for transfers and deploys).
    pub fuel_used: u64,
    /// Events emitted via `chain.emit` during a successful call.
    pub events: Vec<String>,
    /// Address of the newly deployed contract, if a deploy succeeded.
    pub contract: Option<Address>,
}

/// A deployed Logos contract with its source, compiled bytecode, and storage.
#[derive(Debug, Clone)]
pub(crate) struct Contract {
    pub(crate) source: String,
    pub(crate) compiled: Compiled,
    pub(crate) storage: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl Contract {
    /// Compile `data` as a UTF-8 Logos contract, returning `None` on any validation failure.
    pub(crate) fn compile(data: &[u8]) -> Option<Contract> {
        let source = std::str::from_utf8(data).ok()?;
        let ast = logos::parser::parse(source).ok()?;
        let ir = logos::check::check(&ast).ok()?;
        let compiled = cobalt::compile(&ir);
        let main_idx = compiled.main?;
        let main_fn = compiled.module.funcs.get(main_idx as usize)?;
        if main_fn.params != 0 {
            return None;
        }
        for imp in &compiled.module.imports {
            if !ALLOWED_IMPORTS.contains(&imp.as_str()) {
                return None;
            }
        }
        if Vm::new(compiled.module.clone(), &ALLOWED_IMPORTS).is_err() {
            return None;
        }
        Some(Contract {
            source: String::from(source),
            compiled,
            storage: BTreeMap::new(),
        })
    }

    /// Merkle root of this contract's key-value storage.
    pub(crate) fn storage_root(&self) -> Hash {
        let mut leaves = Vec::with_capacity(self.storage.len());
        for (key, value) in &self.storage {
            let Ok(klen) = u32::try_from(key.len()) else {
                continue;
            };
            let mut leaf = Vec::with_capacity(4 + key.len() + value.len());
            leaf.extend_from_slice(&klen.to_le_bytes());
            leaf.extend_from_slice(key);
            leaf.extend_from_slice(value);
            leaves.push(leaf);
        }
        merkle_root(&leaves)
    }

    /// The 96-byte state-root leaf `address || sha256(source) || storage_root`.
    pub(crate) fn state_leaf(&self, address: &Address) -> Vec<u8> {
        let mut leaf = Vec::with_capacity(96);
        leaf.extend_from_slice(address);
        leaf.extend_from_slice(&sha256::digest(self.source.as_bytes()));
        leaf.extend_from_slice(&self.storage_root());
        leaf
    }
}

/// Outcome of executing a contract's `main` function.
pub(crate) struct CallOutcome {
    pub(crate) ok: bool,
    pub(crate) fuel_used: u64,
    pub(crate) writes: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(crate) events: Vec<String>,
}

/// Elysium host implementation for on-chain contract calls.
struct ChainHost<'a> {
    committed: &'a BTreeMap<Vec<u8>, Vec<u8>>,
    writes: BTreeMap<Vec<u8>, Vec<u8>>,
    events: Vec<String>,
    caller: Address,
    input: &'a [u8],
    value: u64,
}

impl ChainHost<'_> {
    /// Extract a byte slice from a Elysium `[byte]` argument.
    fn get_bytes(arg: Option<Value>, heap: &Heap) -> Result<&[u8], Trap> {
        let Some(Value::Ref(h)) = arg else {
            return Err(Trap::TypeError);
        };
        match heap.get(h)? {
            Obj::Bytes(bytes) => Ok(bytes.as_slice()),
            _ => Err(Trap::TypeError),
        }
    }

    /// Extract a string slice from a Elysium `str` argument.
    fn get_str(arg: Option<Value>, heap: &Heap) -> Result<&str, Trap> {
        let Some(Value::Ref(h)) = arg else {
            return Err(Trap::TypeError);
        };
        match heap.get(h)? {
            Obj::Str(s) => Ok(s.as_str()),
            _ => Err(Trap::TypeError),
        }
    }
}

impl Host for ChainHost<'_> {
    fn call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Result<Value, Trap> {
        match name {
            "chain.get" => {
                let key = Self::get_bytes(args.first().copied(), heap)?;
                let found = self
                    .writes
                    .get(key)
                    .or_else(|| self.committed.get(key))
                    .cloned();
                match found {
                    Some(val) => {
                        let bytes_h = heap.alloc(Obj::Bytes(val))?;
                        let opt_h = heap.alloc(Obj::Variant {
                            tag: 1,
                            fields: vec![Value::Ref(bytes_h)],
                        })?;
                        Ok(Value::Ref(opt_h))
                    }
                    None => {
                        let opt_h = heap.alloc(Obj::Variant {
                            tag: 0,
                            fields: Vec::new(),
                        })?;
                        Ok(Value::Ref(opt_h))
                    }
                }
            }
            "chain.set" => {
                let key = Self::get_bytes(args.first().copied(), heap)?.to_vec();
                let val = Self::get_bytes(args.get(1).copied(), heap)?.to_vec();
                self.writes.insert(key, val);
                Ok(Value::Unit)
            }
            "chain.caller" => {
                let h = heap.alloc(Obj::Bytes(self.caller.to_vec()))?;
                Ok(Value::Ref(h))
            }
            "chain.emit" => {
                let event = Self::get_str(args.first().copied(), heap)?;
                self.events.push(String::from(event));
                Ok(Value::Unit)
            }
            "chain.input" => {
                let h = heap.alloc(Obj::Bytes(self.input.to_vec()))?;
                Ok(Value::Ref(h))
            }
            "chain.value" => {
                let val = i64::try_from(self.value).map_err(|_| Trap::Overflow)?;
                Ok(Value::Int(val))
            }
            other => Err(Trap::Host(String::from(other))),
        }
    }
}

/// Execute `contract`'s `main` function on Elysium with `fuel` and buffered host state.
pub(crate) fn execute_contract(
    contract: &Contract,
    caller: Address,
    value: u64,
    input: &[u8],
    fuel: u64,
) -> CallOutcome {
    let Some(main_idx) = contract.compiled.main else {
        return CallOutcome {
            ok: false,
            fuel_used: 0,
            writes: BTreeMap::new(),
            events: Vec::new(),
        };
    };
    let Some(main_fn) = contract.compiled.module.funcs.get(main_idx as usize) else {
        return CallOutcome {
            ok: false,
            fuel_used: 0,
            writes: BTreeMap::new(),
            events: Vec::new(),
        };
    };
    let Ok(mut vm) = Vm::new(contract.compiled.module.clone(), &ALLOWED_IMPORTS) else {
        return CallOutcome {
            ok: false,
            fuel_used: 0,
            writes: BTreeMap::new(),
            events: Vec::new(),
        };
    };
    let mut host = ChainHost {
        committed: &contract.storage,
        writes: BTreeMap::new(),
        events: Vec::new(),
        caller,
        input,
        value,
    };
    let limits = Limits {
        fuel,
        max_heap_bytes: CONTRACT_MAX_HEAP_BYTES,
        max_depth: CONTRACT_MAX_DEPTH,
    };
    let ok = vm.call(&main_fn.name, &[], &mut host, limits).is_ok();
    let fuel_used = vm.fuel_used();
    if ok {
        CallOutcome {
            ok: true,
            fuel_used,
            writes: host.writes,
            events: host.events,
        }
    } else {
        CallOutcome {
            ok: false,
            fuel_used,
            writes: BTreeMap::new(),
            events: Vec::new(),
        }
    }
}
