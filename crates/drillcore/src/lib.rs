//! Git SHA-256 compatible object store, index, commits, branches and checkout.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod format;
mod id;
mod index;
mod repo;
mod store;

pub use format::{
    decode_commit, decode_tree, encode_commit, encode_tree, Commit, Mode, Signature, TreeEntry,
};
pub use id::{from_hex, hash_object, to_hex, Error, Id, Kind};
pub use repo::Repo;
#[cfg(feature = "std")]
pub use store::DirStore;
pub use store::{Files, MemStore, Store};
