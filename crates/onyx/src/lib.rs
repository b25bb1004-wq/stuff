//! onyx: hashes, HMAC, HKDF, ChaCha20, Poly1305, the ChaCha20-Poly1305 AEAD and a
//! constant-time compare. Standard library only; `no_std` with `alloc` outside tests.
#![forbid(unsafe_code)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod aead;
pub mod chacha20;
pub mod ct;
pub mod hkdf;
pub mod hmac;
pub mod poly1305;
pub mod sha256;
pub mod sha512;

/// Errors returned by the fallible functions in this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The authentication tag did not match.
    BadTag,
    /// An input or output had an invalid length.
    BadLength,
}
