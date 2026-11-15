//! Reliable file transfer client and server over Xenon UDP.

use super::{map_conn_err, now_us, wait_datagram, LossySender};
use crate::{Config, Connection, Error, Lossy, Server, UdpError, MAX_DATAGRAM};
use sphinx::sha256::Sha256;
use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

struct RecvState {
    cid: u64,
    stream_id: Option<u64>,
    header: [u8; 8],
    header_len: usize,
    expected_len: Option<u64>,
    received: u64,
    hasher: Sha256,
    digest: Option<[u8; 32]>,
    reply_written: usize,
    reply_finished: bool,
    reply_flushed: bool,
}

impl RecvState {
    fn new(cid: u64) -> Self {
        Self {
            cid,
            stream_id: None,
            header: [0u8; 8],
            header_len: 0,
            expected_len: None,
            received: 0,
            hasher: Sha256::new(),
            digest: None,
            reply_written: 0,
            reply_finished: false,
            reply_flushed: false,
        }
    }

    fn process(&mut self, conn: &mut Connection, out: &mut dyn Write) -> Result<bool, UdpError> {
        if let Some(err) = conn.error() {
            if self.reply_finished && matches!(err, Error::Closed { code: 0, .. }) {
                return Ok(true);
            }
            return Err(map_conn_err(err));
        }
        if !conn.is_connected() {
            return Ok(false);
        }

        let sid = match self.stream_id {
            Some(id) => id,
            None => {
                let Some(id) = conn.accept_stream() else {
                    return Ok(false);
                };
                self.stream_id = Some(id);
                id
            }
        };

        let mut tmp = [0u8; 8192];
        if !self.reply_finished {
            loop {
                let n = conn.read(sid, &mut tmp).map_err(map_conn_err)?;
                if n == 0 {
                    break;
                }
                let mut pos = 0usize;
                if self.header_len < 8 {
                    let take = (8 - self.header_len).min(n);
                    self.header[self.header_len..self.header_len + take]
                        .copy_from_slice(&tmp[..take]);
                    self.header_len += take;
                    pos += take;
                    if self.header_len == 8 {
                        self.expected_len = Some(u64::from_le_bytes(self.header));
                    }
                }
                if pos < n {
                    let expected = self.expected_len.unwrap_or(0);
                    let chunk = &tmp[pos..n];
                    let next_total = self.received.saturating_add(chunk.len() as u64);
                    if next_total > expected {
                        return Err(UdpError::BadFrame);
                    }
                    out.write_all(chunk).map_err(|e| UdpError::Io(e.kind()))?;
                    self.hasher.update(chunk);
                    self.received = next_total;
                }
            }

            if conn.stream_finished(sid) {
                let Some(expected) = self.expected_len else {
                    return Err(UdpError::BadFrame);
                };
                if self.received != expected {
                    return Err(UdpError::BadFrame);
                }
                if self.digest.is_none() {
                    out.flush().map_err(|e| UdpError::Io(e.kind()))?;
                    self.digest = Some(core::mem::take(&mut self.hasher).finalize());
                }
                if let Some(digest) = self.digest {
                    while self.reply_written < 32 {
                        let w = conn
                            .write(sid, &digest[self.reply_written..])
                            .map_err(map_conn_err)?;
                        if w == 0 {
                            break;
                        }
                        self.reply_written += w;
                    }
                    if self.reply_written == 32 {
                        conn.finish(sid).map_err(map_conn_err)?;
                        self.reply_finished = true;
                    }
                }
            }
        }

        if self.reply_finished && self.reply_flushed && conn.stats().bytes_in_flight == 0 {
            return Ok(true);
        }
        Ok(false)
    }
}

/// Receive one file from a single client, write it to `out`, and reply with its SHA-256 hash.
pub fn recv_file(
    socket: UdpSocket,
    secret: [u8; 32],
    seed: [u8; 32],
    config: Config,
    lossy: Lossy,
    out: &mut dyn Write,
    timeout: Duration,
) -> Result<(u64, [u8; 32]), UdpError> {
    let start = Instant::now();
    let deadline_us = u64::try_from(timeout.as_micros()).unwrap_or(u64::MAX);
    let mut server = Server::new(secret, seed, config);
    let mut sender = LossySender::new(lossy);
    let mut peers: BTreeMap<u64, SocketAddr> = BTreeMap::new();
    let mut state: Option<RecvState> = None;
    let mut recv_buf = [0u8; MAX_DATAGRAM];
    let mut last_timeout = None;

    loop {
        if state.is_none() {
            if let Some(cid) = server.accept() {
                state = Some(RecvState::new(cid));
            }
        }

        if let Some(st) = state.as_mut() {
            let done = match server.connection(st.cid) {
                Some(conn) => st.process(conn, out)?,
                None => {
                    if st.reply_finished {
                        true
                    } else {
                        return Err(UdpError::TimedOut);
                    }
                }
            };
            if done {
                if let Some(conn) = server.connection(st.cid) {
                    conn.close(0, "");
                    let now = now_us(start);
                    while let Some(dgram) = conn.poll_transmit(now) {
                        if let Some(&addr) = peers.get(&st.cid) {
                            sender.send(&socket, addr, dgram)?;
                        }
                    }
                }
                sender.flush(&socket);
                let len = st.expected_len.ok_or(UdpError::BadFrame)?;
                let hash = st.digest.ok_or(UdpError::BadFrame)?;
                return Ok((len, hash));
            }
        }

        let cids = server.cids();
        for cid in cids {
            if let Some(conn) = server.connection(cid) {
                let now = now_us(start);
                while let Some(dgram) = conn.poll_transmit(now) {
                    if let Some(&addr) = peers.get(&cid) {
                        sender.send(&socket, addr, dgram)?;
                    }
                }
                if let Some(st) = state.as_mut() {
                    if st.cid == cid && st.reply_finished {
                        st.reply_flushed = true;
                    }
                }
            }
        }

        let now = now_us(start);
        if now >= deadline_us {
            sender.flush(&socket);
            return Err(UdpError::TimedOut);
        }

        if let Some((n, addr)) = wait_datagram(
            &socket,
            &mut recv_buf,
            server.timeout(),
            Some(deadline_us),
            now,
            &mut last_timeout,
        )? {
            if let Some(cid) = server.handle(&recv_buf[..n], now_us(start)) {
                peers.insert(cid, addr);
            }
        }

        let now = now_us(start);
        if server.timeout().is_some_and(|t| now >= t) {
            server.on_timeout(now);
            peers.retain(|cid, _| server.connection(*cid).is_some());
        }
    }
}

/// Send `len` bytes from `input` to `server` and verify the returned SHA-256 hash.
#[allow(clippy::too_many_arguments)]
pub fn send_file(
    socket: UdpSocket,
    server: SocketAddr,
    server_key: [u8; 32],
    seed: [u8; 32],
    config: Config,
    lossy: Lossy,
    input: &mut dyn Read,
    len: u64,
    timeout: Duration,
) -> Result<[u8; 32], UdpError> {
    let start = Instant::now();
    let deadline_us = u64::try_from(timeout.as_micros()).unwrap_or(u64::MAX);
    let mut conn = Connection::connect(server_key, seed, config, 0);
    let mut sender = LossySender::new(lossy);
    let mut stream_id: Option<u64> = None;

    let header = len.to_le_bytes();
    let mut header_sent = 0usize;
    let mut payload_sent = 0u64;
    let mut chunk = [0u8; 16_384];
    let mut chunk_pos = 0usize;
    let mut chunk_len = 0usize;
    let mut hasher = Sha256::new();
    let mut expected_hash: Option<[u8; 32]> = None;
    let mut finished_send = false;
    let mut reply = Vec::with_capacity(32);

    let mut recv_buf = [0u8; MAX_DATAGRAM];
    let mut tmp = [0u8; 1024];
    let mut last_timeout = None;

    loop {
        if let Some(err) = conn.error() {
            sender.flush(&socket);
            return Err(map_conn_err(err));
        }

        if conn.is_connected() {
            let sid = match stream_id {
                Some(id) => id,
                None => {
                    let id = conn.open_stream().map_err(map_conn_err)?;
                    stream_id = Some(id);
                    id
                }
            };

            while header_sent < 8 {
                let w = conn
                    .write(sid, &header[header_sent..])
                    .map_err(map_conn_err)?;
                if w == 0 {
                    break;
                }
                header_sent += w;
            }

            if header_sent == 8 {
                while payload_sent < len || chunk_pos < chunk_len {
                    if chunk_pos == chunk_len && payload_sent < len {
                        let want = (len - payload_sent).min(chunk.len() as u64) as usize;
                        match input.read(&mut chunk[..want]) {
                            Ok(0) => {
                                sender.flush(&socket);
                                return Err(UdpError::Io(ErrorKind::UnexpectedEof));
                            }
                            Ok(n) => {
                                hasher.update(&chunk[..n]);
                                chunk_pos = 0;
                                chunk_len = n;
                            }
                            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                            Err(e) => {
                                sender.flush(&socket);
                                return Err(UdpError::Io(e.kind()));
                            }
                        }
                    }
                    let w = conn
                        .write(sid, &chunk[chunk_pos..chunk_len])
                        .map_err(map_conn_err)?;
                    if w == 0 {
                        break;
                    }
                    chunk_pos += w;
                    payload_sent += w as u64;
                }
            }

            if header_sent == 8 && payload_sent == len && chunk_pos == chunk_len && !finished_send {
                expected_hash = Some(core::mem::take(&mut hasher).finalize());
                conn.finish(sid).map_err(map_conn_err)?;
                finished_send = true;
            }

            loop {
                let n = conn.read(sid, &mut tmp).map_err(map_conn_err)?;
                if n == 0 {
                    break;
                }
                reply.extend_from_slice(&tmp[..n]);
                if reply.len() > 32 {
                    sender.flush(&socket);
                    return Err(UdpError::BadFrame);
                }
            }

            if finished_send && conn.stream_finished(sid) {
                let got: [u8; 32] = reply.try_into().map_err(|_| UdpError::BadFrame)?;
                let want = expected_hash.ok_or(UdpError::BadFrame)?;
                if got != want {
                    sender.flush(&socket);
                    return Err(UdpError::BadHash);
                }
                conn.close(0, "");
                let now = now_us(start);
                while let Some(dgram) = conn.poll_transmit(now) {
                    sender.send(&socket, server, dgram)?;
                }
                sender.flush(&socket);
                return Ok(want);
            }
        }

        let now = now_us(start);
        while let Some(dgram) = conn.poll_transmit(now) {
            sender.send(&socket, server, dgram)?;
        }

        let now = now_us(start);
        if now >= deadline_us {
            sender.flush(&socket);
            return Err(UdpError::TimedOut);
        }

        if let Some((n, _)) = wait_datagram(
            &socket,
            &mut recv_buf,
            conn.timeout(),
            Some(deadline_us),
            now,
            &mut last_timeout,
        )? {
            conn.handle(&recv_buf[..n], now_us(start));
        }

        let now = now_us(start);
        if conn.timeout().is_some_and(|t| now >= t) {
            conn.on_timeout(now);
        }
    }
}
