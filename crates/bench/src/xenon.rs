//! Benchmarks for `xenon`: loopback bulk transfer and small-message round trips.
//!
//! For `bulk_transfer`, `ops` is reported in whole megabytes (`bytes / 1_000_000`, rounded down),
//! so `Sample::rate()` directly yields `MB/s` (10^6 bytes per second).

use std::io::{Cursor, ErrorKind};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use sphinx::{sha256, x25519};
use xenon::{recv_file, send_file, Config, Connection, Lossy, Server, MAX_DATAGRAM};

use crate::{measure, Opts, Sample};

/// Run all `xenon` benchmarks.
pub fn run(opts: &Opts) -> Result<Vec<Sample>, String> {
    Ok(vec![bench_bulk_transfer(opts)?, bench_round_trip(opts)?])
}

fn bench_bulk_transfer(opts: &Opts) -> Result<Sample, String> {
    let scale = usize::try_from(opts.scale.max(1)).map_err(|_| "scale overflow".to_string())?;
    let total_bytes = (4usize * 1024 * 1024)
        .checked_mul(scale)
        .ok_or_else(|| "bulk_transfer size overflow".to_string())?;
    let ops_mb = (total_bytes as u64) / 1_000_000;

    let mut payload = vec![0u8; total_bytes];
    for (i, b) in payload.iter_mut().enumerate() {
        *b = (i.wrapping_mul(131) ^ (i >> 8)) as u8;
    }
    let expected_hash = sha256::digest(&payload);

    let cfg = Config {
        stream_window: 65_536,
        conn_window: 65_536,
        ..Config::default()
    };
    let secret = [0x11u8; 32];
    let pubkey = x25519::public_key(&secret);
    let timeout = Duration::from_secs(30);

    measure("xenon", "bulk_transfer", "MB/s", opts.budget, || {
        let srv_sock =
            UdpSocket::bind("127.0.0.1:0").map_err(|e| format!("bind server failed: {e}"))?;
        let srv_addr = srv_sock
            .local_addr()
            .map_err(|e| format!("server local_addr failed: {e}"))?;
        let cli_sock =
            UdpSocket::bind("127.0.0.1:0").map_err(|e| format!("bind client failed: {e}"))?;

        let srv_handle = thread::Builder::new()
            .name("bench-xenon-recv".to_string())
            .spawn(move || {
                let mut sink = std::io::sink();
                recv_file(
                    srv_sock,
                    secret,
                    [0x22u8; 32],
                    cfg,
                    Lossy::default(),
                    &mut sink,
                    timeout,
                )
            })
            .map_err(|e| format!("spawn recv_file thread failed: {e}"))?;

        let mut cursor = Cursor::new(payload.as_slice());
        let send_res = send_file(
            cli_sock,
            srv_addr,
            pubkey,
            [0x33u8; 32],
            cfg,
            Lossy::default(),
            &mut cursor,
            total_bytes as u64,
            timeout,
        );
        let recv_res = srv_handle
            .join()
            .map_err(|_| "recv_file thread panicked".to_string())?;

        let sent_hash = send_res.map_err(|e| format!("send_file failed: {e}"))?;
        let (recv_len, recv_hash) = recv_res.map_err(|e| format!("recv_file failed: {e}"))?;
        if recv_len != total_bytes as u64
            || sent_hash != expected_hash
            || recv_hash != expected_hash
        {
            return Err("bulk_transfer hash or length mismatch".to_string());
        }
        Ok(ops_mb)
    })
}

fn now_us(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn run_echo_loop(
    socket: UdpSocket,
    secret: [u8; 32],
    cfg: Config,
    stop: &AtomicBool,
    start: Instant,
) -> Result<(), String> {
    let _ = socket.set_read_timeout(Some(Duration::from_millis(1)));
    let mut server = Server::new(secret, [0x44u8; 32], cfg);
    let mut peer: Option<SocketAddr> = None;
    let mut streams: Vec<u64> = Vec::new();
    let mut recv_buf = [0u8; MAX_DATAGRAM];
    let mut tmp = [0u8; 4096];

    while !stop.load(Ordering::Acquire) {
        let cids = server.cids();
        for cid in cids {
            if let Some(conn) = server.connection(cid) {
                if conn.is_connected() {
                    while let Some(sid) = conn.accept_stream() {
                        streams.push(sid);
                    }
                    for &sid in &streams {
                        while let Ok(n) = conn.read(sid, &mut tmp) {
                            if n == 0 {
                                break;
                            }
                            let mut off = 0usize;
                            while off < n {
                                match conn.write(sid, &tmp[off..n]) {
                                    Ok(0) | Err(_) => break,
                                    Ok(w) => off += w,
                                }
                            }
                        }
                    }
                }
                let now = now_us(start);
                while let Some(pkt) = conn.poll_transmit(now) {
                    if let Some(addr) = peer {
                        socket
                            .send_to(&pkt, addr)
                            .map_err(|e| format!("server send_to failed: {e}"))?;
                    }
                }
            }
        }

        match socket.recv_from(&mut recv_buf) {
            Ok((n, addr)) => {
                if server.handle(&recv_buf[..n], now_us(start)).is_some() {
                    peer = Some(addr);
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset
                ) => {}
            Err(e) => return Err(format!("server recv_from failed: {e}")),
        }

        let now = now_us(start);
        if server.timeout().is_some_and(|t| now >= t) {
            server.on_timeout(now);
        }
    }
    Ok(())
}

fn pump_client(
    conn: &mut Connection,
    sock: &UdpSocket,
    srv_addr: SocketAddr,
    start: Instant,
) -> Result<(), String> {
    let now = now_us(start);
    while let Some(pkt) = conn.poll_transmit(now) {
        sock.send_to(&pkt, srv_addr)
            .map_err(|e| format!("client send_to failed: {e}"))?;
    }
    Ok(())
}

fn bench_round_trip(opts: &Opts) -> Result<Sample, String> {
    let cfg = Config::default();
    let secret = [0x55u8; 32];
    let pubkey = x25519::public_key(&secret);

    let srv_sock =
        UdpSocket::bind("127.0.0.1:0").map_err(|e| format!("bind echo server failed: {e}"))?;
    let srv_addr = srv_sock
        .local_addr()
        .map_err(|e| format!("echo server local_addr failed: {e}"))?;
    let cli_sock =
        UdpSocket::bind("127.0.0.1:0").map_err(|e| format!("bind echo client failed: {e}"))?;
    let _ = cli_sock.set_read_timeout(Some(Duration::from_millis(5)));

    let start = Instant::now();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let srv_stop = std::sync::Arc::clone(&stop);
    let srv_handle = thread::Builder::new()
        .name("bench-xenon-echo".to_string())
        .spawn(move || run_echo_loop(srv_sock, secret, cfg, &srv_stop, start))
        .map_err(|e| format!("spawn echo server failed: {e}"))?;

    let mut conn = Connection::connect(pubkey, [0x66u8; 32], cfg, now_us(start));
    let mut recv_buf = [0u8; MAX_DATAGRAM];
    let hs_deadline = Instant::now() + Duration::from_secs(5);
    while !conn.is_connected() {
        if let Some(err) = conn.error() {
            stop.store(true, Ordering::Release);
            let _ = srv_handle.join();
            return Err(format!("handshake failed: {err}"));
        }
        if Instant::now() >= hs_deadline {
            stop.store(true, Ordering::Release);
            let _ = srv_handle.join();
            return Err("handshake timed out".to_string());
        }
        pump_client(&mut conn, &cli_sock, srv_addr, start)?;
        match cli_sock.recv_from(&mut recv_buf) {
            Ok((n, _)) => conn.handle(&recv_buf[..n], now_us(start)),
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset
                ) => {}
            Err(e) => {
                stop.store(true, Ordering::Release);
                let _ = srv_handle.join();
                return Err(format!("handshake recv_from failed: {e}"));
            }
        }
        let now = now_us(start);
        if conn.timeout().is_some_and(|t| now >= t) {
            conn.on_timeout(now);
        }
    }

    let sid = conn
        .open_stream()
        .map_err(|e| format!("open_stream failed: {e}"))?;
    let mut seq = 1u64;
    const BATCH: u64 = 100;

    let res = measure("xenon", "round_trip", "rt/s", opts.budget, || {
        for _ in 0..BATCH {
            let mut msg = [0xA5u8; 64];
            msg[..8].copy_from_slice(&seq.to_le_bytes());
            seq = seq.wrapping_add(1);

            let mut written = 0usize;
            while written < msg.len() {
                let w = conn
                    .write(sid, &msg[written..])
                    .map_err(|e| format!("stream write failed: {e}"))?;
                written += w;
                if w == 0 {
                    pump_client(&mut conn, &cli_sock, srv_addr, start)?;
                    if let Ok((n, _)) = cli_sock.recv_from(&mut recv_buf) {
                        conn.handle(&recv_buf[..n], now_us(start));
                    }
                }
            }
            pump_client(&mut conn, &cli_sock, srv_addr, start)?;

            let mut reply = [0u8; 64];
            let mut read_len = 0usize;
            let rt_deadline = Instant::now() + Duration::from_secs(5);
            while read_len < reply.len() {
                let n = conn
                    .read(sid, &mut reply[read_len..])
                    .map_err(|e| format!("stream read failed: {e}"))?;
                read_len += n;
                if read_len == reply.len() {
                    break;
                }
                pump_client(&mut conn, &cli_sock, srv_addr, start)?;
                if Instant::now() >= rt_deadline {
                    return Err("round_trip timed out".to_string());
                }
                match cli_sock.recv_from(&mut recv_buf) {
                    Ok((rn, _)) => conn.handle(&recv_buf[..rn], now_us(start)),
                    Err(e)
                        if matches!(
                            e.kind(),
                            ErrorKind::WouldBlock
                                | ErrorKind::TimedOut
                                | ErrorKind::ConnectionReset
                        ) => {}
                    Err(e) => return Err(format!("client recv_from failed: {e}")),
                }
                let now = now_us(start);
                if conn.timeout().is_some_and(|t| now >= t) {
                    conn.on_timeout(now);
                }
            }

            if reply != msg {
                return Err("round_trip echo mismatch".to_string());
            }
        }
        pump_client(&mut conn, &cli_sock, srv_addr, start)?;
        Ok(BATCH)
    });

    conn.close(0, "");
    let _ = pump_client(&mut conn, &cli_sock, srv_addr, start);
    stop.store(true, Ordering::Release);
    let srv_res = srv_handle
        .join()
        .map_err(|_| "echo server thread panicked".to_string())?;
    srv_res?;
    res
}
