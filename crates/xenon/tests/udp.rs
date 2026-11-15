//! Integration tests for Xenon UDP driver, lossy link, echo, and file transfer.
#![allow(clippy::unwrap_used)]

use assay::Rng;
use sphinx::sha256::Sha256;
use sphinx::x25519;
use std::io::Cursor;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use xenon::{echo, echo_server, recv_file, send_file, Config, Lossy, UdpError};

fn bind_lo() -> (UdpSocket, SocketAddr) {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = sock.local_addr().unwrap();
    (sock, addr)
}

fn fill_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let mut out = vec![0u8; len];
    rng.fill(&mut out);
    out
}

fn seed32(v: u8) -> [u8; 32] {
    [v; 32]
}

fn spawn_echo_server(
    secret: [u8; 32],
    lossy: Lossy,
) -> (SocketAddr, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let (sock, addr) = bind_lo();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_srv = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        echo_server(
            sock,
            secret,
            seed32(11),
            Config::default(),
            lossy,
            &stop_srv,
        )
        .unwrap();
    });
    (addr, stop, handle)
}

#[test]
fn echo_sizes_no_loss_and_stop() {
    let secret = seed32(1);
    let pubkey = x25519::public_key(&secret);
    let (addr, stop, handle) = spawn_echo_server(secret, Lossy::default());

    for (idx, &len) in [0usize, 1, 1200, 100_000].iter().enumerate() {
        let payload = fill_bytes(100 + idx as u64, len);
        let (csock, _) = bind_lo();
        let got = echo(
            csock,
            addr,
            pubkey,
            seed32(20 + idx as u8),
            Config::default(),
            Lossy::default(),
            &payload,
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(got, payload, "mismatch for len={len}");
    }

    stop.store(true, Ordering::Release);
    handle.join().unwrap();
}

#[test]
fn echo_1mb_lossy() {
    let secret = seed32(2);
    let pubkey = x25519::public_key(&secret);
    let srv_loss = Lossy {
        drop_per_mille: 50,
        dup_per_mille: 20,
        delay_per_mille: 50,
        seed: 42,
    };
    let cli_loss = Lossy {
        drop_per_mille: 50,
        dup_per_mille: 20,
        delay_per_mille: 50,
        seed: 99,
    };
    let (addr, stop, handle) = spawn_echo_server(secret, srv_loss);

    let payload = fill_bytes(777, 1_000_000);
    let (csock, _) = bind_lo();
    let got = echo(
        csock,
        addr,
        pubkey,
        seed32(3),
        Config::default(),
        cli_loss,
        &payload,
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(got, payload);

    stop.store(true, Ordering::Release);
    handle.join().unwrap();
}

#[test]
fn three_clients_concurrent_echo() {
    let secret = seed32(4);
    let pubkey = x25519::public_key(&secret);
    let (addr, stop, handle) = spawn_echo_server(secret, Lossy::default());

    let mut clients = Vec::new();
    for i in 0..3u64 {
        let payload = fill_bytes(500 + i, 50_000 + (i as usize) * 7_000);
        clients.push(thread::spawn(move || {
            let (csock, _) = bind_lo();
            let got = echo(
                csock,
                addr,
                pubkey,
                seed32(40 + i as u8),
                Config::default(),
                Lossy::default(),
                &payload,
                Duration::from_secs(15),
            )
            .unwrap();
            assert_eq!(got, payload);
        }));
    }
    for c in clients {
        c.join().unwrap();
    }

    stop.store(true, Ordering::Release);
    handle.join().unwrap();
}

#[test]
fn wrong_server_key_and_dead_server_timeout() {
    let secret = seed32(5);
    let mut bad_pubkey = x25519::public_key(&secret);
    bad_pubkey[0] ^= 0xff;

    let (addr, stop, handle) = spawn_echo_server(secret, Lossy::default());
    let (csock, _) = bind_lo();
    let timeout = Duration::from_millis(450);
    let started = Instant::now();
    let res = echo(
        csock,
        addr,
        bad_pubkey,
        seed32(6),
        Config::default(),
        Lossy::default(),
        b"hello",
        timeout,
    );
    assert!(matches!(res, Err(UdpError::TimedOut | UdpError::Conn(_))));
    assert!(started.elapsed() < Duration::from_secs(2));

    stop.store(true, Ordering::Release);
    handle.join().unwrap();

    // Also verify dead server times out cleanly.
    let (dead_sock, dead_addr) = bind_lo();
    drop(dead_sock);
    let (csock2, _) = bind_lo();
    let res2 = echo(
        csock2,
        dead_addr,
        x25519::public_key(&secret),
        seed32(7),
        Config::default(),
        Lossy::default(),
        b"hello",
        Duration::from_millis(350),
    );
    assert!(matches!(res2, Err(UdpError::TimedOut | UdpError::Conn(_))));
}

#[test]
fn file_transfer_8mb_lossy() {
    let secret = seed32(8);
    let pubkey = x25519::public_key(&secret);
    let srv_loss = Lossy {
        drop_per_mille: 50,
        dup_per_mille: 20,
        delay_per_mille: 50,
        seed: 123,
    };
    let cli_loss = Lossy {
        drop_per_mille: 50,
        dup_per_mille: 20,
        delay_per_mille: 50,
        seed: 456,
    };

    let (ssock, saddr) = bind_lo();
    let srv_thread = thread::spawn(move || {
        let mut received = Vec::new();
        let (len, hash) = recv_file(
            ssock,
            secret,
            seed32(9),
            Config::default(),
            srv_loss,
            &mut received,
            Duration::from_secs(40),
        )
        .unwrap();
        (len, hash, received)
    });

    let payload = fill_bytes(999, 8 * 1024 * 1024);
    let expected_hash = {
        let mut h = Sha256::new();
        h.update(&payload);
        h.finalize()
    };

    let (csock, _) = bind_lo();
    let mut cursor = Cursor::new(&payload);
    let sent_hash = send_file(
        csock,
        saddr,
        pubkey,
        seed32(10),
        Config::default(),
        cli_loss,
        &mut cursor,
        payload.len() as u64,
        Duration::from_secs(40),
    )
    .unwrap();
    assert_eq!(sent_hash, expected_hash);

    let (recv_len, recv_hash, received) = srv_thread.join().unwrap();
    assert_eq!(recv_len, payload.len() as u64);
    assert_eq!(recv_hash, expected_hash);
    assert_eq!(received.len(), payload.len());
    assert_eq!(received, payload);
}

#[test]
fn short_stream_to_recv_file_rejected() {
    let secret = seed32(12);
    let pubkey = x25519::public_key(&secret);
    let (ssock, saddr) = bind_lo();
    let srv_thread = thread::spawn(move || {
        let mut out = Vec::new();
        recv_file(
            ssock,
            secret,
            seed32(13),
            Config::default(),
            Lossy::default(),
            &mut out,
            Duration::from_secs(5),
        )
    });

    // Send a header claiming 1000 bytes, but only send 10 bytes and finish.
    let mut bad_payload = Vec::new();
    bad_payload.extend_from_slice(&1000u64.to_le_bytes());
    bad_payload.extend_from_slice(&[0xabu8; 10]);

    let (csock, _) = bind_lo();
    let _ = echo(
        csock,
        saddr,
        pubkey,
        seed32(14),
        Config::default(),
        Lossy::default(),
        &bad_payload,
        Duration::from_millis(500),
    );

    let res = srv_thread.join().unwrap();
    assert!(matches!(res, Err(UdpError::BadFrame)));
}
