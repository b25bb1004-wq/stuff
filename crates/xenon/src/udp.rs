//! UDP driver, lossy link simulation, echo, and file-transfer helpers for Xenon.

mod echo;
mod file;

pub use echo::{echo, echo_server};
pub use file::{recv_file, send_file};

use crate::Error;
use core::fmt;
use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const MIN_WAIT_US: u64 = 1_000;
const MAX_WAIT_US: u64 = 50_000;

/// Simulated packet loss, duplication, and reordering for outgoing UDP datagrams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Lossy {
    /// Probability (per mille, 0..=1000) to drop an outgoing datagram.
    pub drop_per_mille: u32,
    /// Probability (per mille, 0..=1000) to duplicate an outgoing datagram.
    pub dup_per_mille: u32,
    /// Probability (per mille, 0..=1000) to delay an outgoing datagram by 1 to 3 packets.
    pub delay_per_mille: u32,
    /// Deterministic PRNG seed; `0` disables all simulated loss, duplication, and delay.
    pub seed: u64,
}

/// Errors returned by Xenon UDP driver operations.
#[derive(Debug)]
pub enum UdpError {
    /// Underlying socket or I/O error kind.
    Io(ErrorKind),
    /// Transport connection error.
    Conn(Error),
    /// Operation or handshake did not complete within the allowed timeout.
    TimedOut,
    /// File transfer SHA-256 verification failed.
    BadHash,
    /// Malformed or truncated file transfer framing.
    BadFrame,
}

impl fmt::Display for UdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UdpError::Io(kind) => write!(f, "i/o error: {kind:?}"),
            UdpError::Conn(err) => write!(f, "connection error: {err}"),
            UdpError::TimedOut => write!(f, "timed out"),
            UdpError::BadHash => write!(f, "sha256 mismatch"),
            UdpError::BadFrame => write!(f, "bad file transfer frame"),
        }
    }
}

impl std::error::Error for UdpError {}

pub(crate) fn map_conn_err(err: Error) -> UdpError {
    match err {
        Error::Timeout => UdpError::TimedOut,
        other => UdpError::Conn(other),
    }
}

pub(crate) fn now_us(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

pub(crate) fn wait_datagram(
    socket: &UdpSocket,
    buf: &mut [u8],
    next_timer: Option<u64>,
    deadline_us: Option<u64>,
    now: u64,
    last_timeout: &mut Option<u64>,
) -> Result<Option<(usize, SocketAddr)>, UdpError> {
    let mut wait_us = next_timer.map_or(MAX_WAIT_US, |t| t.saturating_sub(now));
    if let Some(dl) = deadline_us {
        wait_us = wait_us.min(dl.saturating_sub(now));
    }
    let wait_us = wait_us.clamp(MIN_WAIT_US, MAX_WAIT_US);
    if *last_timeout != Some(wait_us) {
        socket
            .set_read_timeout(Some(Duration::from_micros(wait_us)))
            .map_err(|e| UdpError::Io(e.kind()))?;
        *last_timeout = Some(wait_us);
    }

    match socket.recv_from(buf) {
        Ok((n, addr)) => Ok(Some((n, addr))),
        Err(e)
            if matches!(
                e.kind(),
                ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset
            ) =>
        {
            Ok(None)
        }
        Err(e) => Err(UdpError::Io(e.kind())),
    }
}

struct Xorshift64(u64);

impl Xorshift64 {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_per_mille(&mut self) -> u32 {
        (self.next_u64() % 1000) as u32
    }
}

struct HeldPacket {
    addr: SocketAddr,
    data: Vec<u8>,
    wait: usize,
}

pub(crate) struct LossySender {
    lossy: Lossy,
    rng: Option<Xorshift64>,
    held: VecDeque<HeldPacket>,
}

impl LossySender {
    pub(crate) fn new(lossy: Lossy) -> Self {
        let rng = if lossy.seed == 0 {
            None
        } else {
            Some(Xorshift64(lossy.seed))
        };
        Self {
            lossy,
            rng,
            held: VecDeque::new(),
        }
    }

    pub(crate) fn send(
        &mut self,
        socket: &UdpSocket,
        addr: SocketAddr,
        data: Vec<u8>,
    ) -> Result<(), UdpError> {
        let Some(rng) = self.rng.as_mut() else {
            socket
                .send_to(&data, addr)
                .map_err(|e| UdpError::Io(e.kind()))?;
            return Ok(());
        };

        if rng.next_per_mille() < self.lossy.drop_per_mille {
            return Ok(());
        }
        if rng.next_per_mille() < self.lossy.dup_per_mille {
            socket
                .send_to(&data, addr)
                .map_err(|e| UdpError::Io(e.kind()))?;
            socket
                .send_to(&data, addr)
                .map_err(|e| UdpError::Io(e.kind()))?;
            self.release_ready(socket)?;
            return Ok(());
        }
        if rng.next_per_mille() < self.lossy.delay_per_mille {
            let wait = 1 + (rng.next_u64() % 3) as usize;
            self.held.push_back(HeldPacket { addr, data, wait });
            return Ok(());
        }

        socket
            .send_to(&data, addr)
            .map_err(|e| UdpError::Io(e.kind()))?;
        self.release_ready(socket)?;
        Ok(())
    }

    fn release_ready(&mut self, socket: &UdpSocket) -> Result<(), UdpError> {
        let mut i = 0;
        while i < self.held.len() {
            if self.held[i].wait <= 1 {
                if let Some(pkt) = self.held.remove(i) {
                    socket
                        .send_to(&pkt.data, pkt.addr)
                        .map_err(|e| UdpError::Io(e.kind()))?;
                }
            } else {
                self.held[i].wait -= 1;
                i += 1;
            }
        }
        Ok(())
    }

    pub(crate) fn flush(&mut self, socket: &UdpSocket) {
        while let Some(pkt) = self.held.pop_front() {
            let _ = socket.send_to(&pkt.data, pkt.addr);
        }
    }
}
