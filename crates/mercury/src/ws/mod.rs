//! WebSocket protocol support (RFC 6455): key derivation, framing, and connections.

mod crypto;
mod frame;
#[cfg(feature = "std")]
mod socket;

use alloc::string::String;
use alloc::vec::Vec;

pub use crypto::accept_key;
#[cfg(feature = "std")]
pub(crate) use crypto::base64_decode;
pub use frame::{decode_frame, encode_frame, Frame, WsError};
#[cfg(feature = "std")]
pub use socket::WebSocket;

/// A high-level WebSocket message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// UTF-8 text message.
    Text(String),
    /// Arbitrary binary message.
    Binary(Vec<u8>),
    /// Control ping frame payload.
    Ping(Vec<u8>),
    /// Control pong frame payload.
    Pong(Vec<u8>),
    /// Close frame with optional status code and UTF-8 reason.
    Close(Option<(u16, String)>),
}
