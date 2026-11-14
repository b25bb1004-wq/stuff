//! PNG image decoding and encoding.

mod decode;
mod encode;
mod filter;

#[cfg(test)]
mod tests;

pub use decode::decode_png;
pub use encode::encode_png;

/// Errors returned by PNG decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PngError {
    /// Missing or invalid 8-byte PNG signature.
    Signature,
    /// Malformed, truncated, or out-of-order chunk framing.
    Chunk,
    /// Chunk CRC-32 mismatch.
    Crc,
    /// Missing or invalid IHDR header fields.
    Header,
    /// Unsupported critical chunk.
    Unsupported,
    /// Corrupt compressed image stream, filter, or palette index.
    Data,
    /// Image dimensions exceed `max_pixels` or allocation limits.
    TooLarge,
}
