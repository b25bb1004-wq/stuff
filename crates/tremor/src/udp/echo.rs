//! Echo server and client over Tremor UDP.

use super::{map_conn_err, now_us, wait_datagram, LossySender};
use crate::{Config, Connection, Lossy, Server, UdpError, MAX_DATAGRAM};
use std::collections::BTreeMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

struct EchoStream {
    id: u64,
    pending: Vec<u8>,
    client_finished: bool,
    server_finished: bool,
}

impl EchoStream {
    fn new(id: u64) -> Self {
        Self {
            id,
            pending: Vec::new(),
            client_finished: false,
            server_finished: false,
        }
    }
}

/// Run a multi-client Tremor echo server until `stop` becomes `true`.
pub fn echo_server(
    socket: UdpSocket,
    secret: [u8; 32],
    seed: [u8; 32],
    config: Config,
    lossy: Lossy,
    stop: &AtomicBool,
) -> Result<(), UdpError> {
    let start = Instant::now();
    let mut server = Server::new(secret, seed, config);
    let mut sender = LossySender::new(lossy);
    let mut peers: BTreeMap<u64, SocketAddr> = BTreeMap::new();
    let mut conn_streams: BTreeMap<u64, Vec<EchoStream>> = BTreeMap::new();
    let mut recv_buf = [0u8; MAX_DATAGRAM];
    let mut tmp = [0u8; 8192];
    let mut last_timeout = None;

    while !stop.load(Ordering::Acquire) {
        let cids = server.cids();
        for cid in cids {
            let Some(conn) = server.connection(cid) else {
                continue;
            };
            if conn.error().is_some() {
                conn_streams.remove(&cid);
            } else if conn.is_connected() {
                let streams = conn_streams.entry(cid).or_default();
                while let Some(id) = conn.accept_stream() {
                    streams.push(EchoStream::new(id));
                }
                for s in streams.iter_mut() {
                    if !s.client_finished {
                        while let Ok(n) = conn.read(s.id, &mut tmp) {
                            if n == 0 {
                                break;
                            }
                            s.pending.extend_from_slice(&tmp[..n]);
                        }
                        if conn.stream_finished(s.id) {
                            s.client_finished = true;
                        }
                    }
                    while !s.pending.is_empty() {
                        match conn.write(s.id, &s.pending) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                s.pending.drain(..n);
                            }
                        }
                    }
                    if s.client_finished && s.pending.is_empty() && !s.server_finished {
                        let _ = conn.finish(s.id);
                        s.server_finished = true;
                    }
                }
                streams.retain(|s| !s.server_finished);
            }

            let now = now_us(start);
            while let Some(dgram) = conn.poll_transmit(now) {
                if let Some(&addr) = peers.get(&cid) {
                    sender.send(&socket, addr, dgram)?;
                }
            }
        }

        let now = now_us(start);
        if let Some((n, addr)) = wait_datagram(
            &socket,
            &mut recv_buf,
            server.timeout(),
            None,
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
            conn_streams.retain(|cid, _| server.connection(*cid).is_some());
        }
    }

    sender.flush(&socket);
    Ok(())
}

/// Connect to `server`, echo `data` over one stream, and return the echoed bytes.
#[allow(clippy::too_many_arguments)]
pub fn echo(
    socket: UdpSocket,
    server: SocketAddr,
    server_key: [u8; 32],
    seed: [u8; 32],
    config: Config,
    lossy: Lossy,
    data: &[u8],
    timeout: std::time::Duration,
) -> Result<Vec<u8>, UdpError> {
    let start = Instant::now();
    let deadline_us = u64::try_from(timeout.as_micros()).unwrap_or(u64::MAX);
    let mut conn = Connection::connect(server_key, seed, config, 0);
    let mut sender = LossySender::new(lossy);
    let mut stream_id: Option<u64> = None;
    let mut written = 0usize;
    let mut finished_send = false;
    let mut out = Vec::with_capacity(data.len());
    let mut recv_buf = [0u8; MAX_DATAGRAM];
    let mut tmp = [0u8; 8192];
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

            while written < data.len() {
                let n = conn.write(sid, &data[written..]).map_err(map_conn_err)?;
                if n == 0 {
                    break;
                }
                written += n;
            }

            if written == data.len() && !finished_send {
                conn.finish(sid).map_err(map_conn_err)?;
                finished_send = true;
            }

            loop {
                let n = conn.read(sid, &mut tmp).map_err(map_conn_err)?;
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&tmp[..n]);
            }

            if finished_send && conn.stream_finished(sid) {
                conn.close(0, "");
                let now = now_us(start);
                while let Some(dgram) = conn.poll_transmit(now) {
                    sender.send(&socket, server, dgram)?;
                }
                sender.flush(&socket);
                return Ok(out);
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
