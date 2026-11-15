//! Xenon: encrypted, reliable, multi-stream transport protocol core (sans-I/O).
#![forbid(unsafe_code)]

mod conn;
mod conn_io;
mod conn_tx;
mod crypto;
mod recovery;
mod server;
mod stream;
mod udp;
mod wire;

pub use conn::Connection;
pub use server::Server;
pub use udp::*;

use core::fmt;

/// Maximum UDP datagram payload size in bytes.
pub const MAX_DATAGRAM: usize = 1200;

/// Errors returned by Xenon connection and stream operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Handshake or idle timeout expired.
    Timeout,
    /// Connection or stream was closed/reset with a code and reason.
    Closed {
        /// Application or wire error code.
        code: u64,
        /// Human-readable reason string.
        reason: String,
    },
    /// Peer violated the Xenon wire protocol or local API precondition.
    Protocol(&'static str),
    /// Peer violated flow control limits.
    FlowControl,
    /// Local or peer stream count exceeded `max_streams`.
    StreamLimit,
    /// Input or message exceeded size bounds.
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Timeout => write!(f, "connection timed out"),
            Error::Closed { code, reason } => {
                if reason.is_empty() {
                    write!(f, "closed (code {code})")
                } else {
                    write!(f, "closed (code {code}): {reason}")
                }
            }
            Error::Protocol(msg) => write!(f, "protocol error: {msg}"),
            Error::FlowControl => write!(f, "flow control error"),
            Error::StreamLimit => write!(f, "stream limit exceeded"),
            Error::TooLarge => write!(f, "too large"),
        }
    }
}

impl std::error::Error for Error {}

/// Transport configuration parameters for `Connection` and `Server`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Maximum streams a peer may open.
    pub max_streams: u64,
    /// Initial per-stream receive window in bytes.
    pub stream_window: u64,
    /// Initial connection-wide receive window in bytes.
    pub conn_window: u64,
    /// Idle timeout in milliseconds.
    pub idle_timeout_ms: u64,
    /// Maximum concurrent half-open connections on a server.
    pub max_half_open: usize,
    /// Maximum total connections (half-open or live) a server holds; new Initials beyond it are refused.
    pub max_connections: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_streams: 100,
            stream_window: 262_144,
            conn_window: 1_048_576,
            idle_timeout_ms: 30_000,
            max_half_open: 256,
            max_connections: 256,
        }
    }
}

/// Snapshot of connection packet, loss, congestion window, and RTT metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Total datagrams transmitted by this connection.
    pub sent_packets: u64,
    /// Total Data packets declared lost.
    pub lost_packets: u64,
    /// Current congestion window in bytes.
    pub cwnd: u64,
    /// Current unacknowledged bytes in flight.
    pub bytes_in_flight: u64,
    /// Current smoothed RTT estimate in microseconds.
    pub srtt_us: u64,
}
