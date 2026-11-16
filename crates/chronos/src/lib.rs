//! Git SHA-256 compatible object store, index, commits, branches and checkout.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod diff;
mod format;
#[cfg(feature = "std")]
pub mod git;
mod id;
mod index;
mod merge;
#[cfg(feature = "std")]
pub mod remote;
mod repo;
mod status;
mod store;

pub use diff::{diff, lines, unified, Edit};
pub use format::{
    decode_commit, decode_tree, encode_commit, encode_tree, Commit, Mode, Signature, TreeEntry,
};
pub use id::{from_hex, hash_object, to_hex, Error, Id, Kind};
pub use merge::{merge3, MergeError, Merged};
pub use repo::Repo;
pub use status::Change;
#[cfg(feature = "std")]
pub use store::DirStore;
pub use store::{validate_entry_name, Files, MemStore, Store};
