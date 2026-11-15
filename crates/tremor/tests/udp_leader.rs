//! Leader check for T35: UDP driver, lossy link, echo and file transfer.
#![allow(clippy::unwrap_used, clippy::type_complexity)]
use assay::Rng;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tremor::*;

fn sock() -> UdpSocket {
    UdpSocket::bind("127.0.0.1:0").unwrap()
}

fn key(seed: u8) -> ([u8; 32], [u8; 32]) {
    let secret = [seed; 32];
    let public = Server::new(secret, [1; 32], Config::default()).public_key();
    (secret, public)
}

fn start_echo(
    lossy: Lossy,
) -> (
    SocketAddr,
    [u8; 32],
    Arc<AtomicBool>,
    thread::JoinHandle<()>,
) {
    let (secret, public) = key(9);
    let s = sock();
    let addr = s.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let st = stop.clone();
    let h = thread::spawn(move || {
        echo_server(s, secret, [2; 32], Config::default(), lossy, &st).unwrap();
    });
    (addr, public, stop, h)
}

fn payload(r: &mut Rng, n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    r.fill(&mut v);
    v
}

/// A fresh client seed: equal seeds would mean equal connection ids.
fn seed32(r: &mut Rng) -> [u8; 32] {
    let mut s = [0u8; 32];
    r.fill(&mut s);
    s
}

#[test]
fn echo_random_sizes_and_loss() {
    let mut r = Rng::new(0x35_1eed_0001);
    for round in 0..6u64 {
        let lossy = Lossy {
            drop_per_mille: r.below(80) as u32,
            dup_per_mille: r.below(40) as u32,
            delay_per_mille: r.below(80) as u32,
            seed: r.u64() | 1,
        };
        let (addr, public, stop, h) = start_echo(lossy);
        for _ in 0..3 {
            let n = [0, 1, 1199, 1200, 1201, 4096, 70_000][r.below(7)] + r.below(3);
            let data = payload(&mut r, n);
            let cl = Lossy {
                seed: r.u64() | 1,
                ..lossy
            };
            let got = echo(
                sock(),
                addr,
                public,
                seed32(&mut r),
                Config::default(),
                cl,
                &data,
                Duration::from_secs(30),
            )
            .unwrap();
            assert_eq!(got, data, "round {round} n {n}");
        }
        stop.store(true, Ordering::SeqCst);
        h.join().unwrap();
    }
}

#[test]
fn eight_concurrent_clients() {
    let lossy = Lossy {
        drop_per_mille: 30,
        dup_per_mille: 10,
        delay_per_mille: 30,
        seed: 77,
    };
    let (addr, public, stop, h) = start_echo(lossy);
    let hs: Vec<_> = (0..8u8)
        .map(|i| {
            thread::spawn(move || {
                let mut r = Rng::new(0x35_c0c0 + u64::from(i));
                let data = payload(&mut r, 20_000 + usize::from(i) * 777);
                let cl = Lossy {
                    seed: 100 + u64::from(i),
                    ..lossy
                };
                let got = echo(
                    sock(),
                    addr,
                    public,
                    [i + 40; 32],
                    Config::default(),
                    cl,
                    &data,
                    Duration::from_secs(60),
                )
                .unwrap();
                assert_eq!(got, data, "client {i}");
            })
        })
        .collect();
    for t in hs {
        t.join().unwrap();
    }
    stop.store(true, Ordering::SeqCst);
    h.join().unwrap();
}

#[test]
fn wrong_key_and_dead_server_time_out() {
    let (addr, _public, stop, h) = start_echo(Lossy::default());
    let (_, wrong) = key(10);
    let t = Instant::now();
    let res = echo(
        sock(),
        addr,
        wrong,
        [5; 32],
        Config::default(),
        Lossy::default(),
        b"hi",
        Duration::from_secs(2),
    );
    assert!(res.is_err(), "wrong key must fail");
    assert!(
        t.elapsed() < Duration::from_secs(6),
        "wrong key took {:?}",
        t.elapsed()
    );
    stop.store(true, Ordering::SeqCst);
    h.join().unwrap();
    let dead = sock().local_addr().unwrap(); // bound then dropped: nobody listens
    let t = Instant::now();
    let res = send_file(
        sock(),
        dead,
        wrong,
        [6; 32],
        Config::default(),
        Lossy::default(),
        &mut &b"abc"[..],
        3,
        Duration::from_secs(2),
    );
    assert!(res.is_err(), "dead server must fail");
    assert!(
        t.elapsed() < Duration::from_secs(6),
        "dead server took {:?}",
        t.elapsed()
    );
}

/// Drives a raw client that announces `len` bytes but sends only `send`.
fn short_client(addr: SocketAddr, public: [u8; 32], len: u64, send: usize) {
    let s = sock();
    s.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
    let start = Instant::now();
    let now = || start.elapsed().as_micros() as u64;
    let mut c = Connection::connect(public, [8; 32], Config::default(), now());
    let mut buf = vec![0u8; 2048];
    let mut sid = None;
    let mut body = len.to_le_bytes().to_vec();
    body.extend(std::iter::repeat_n(7u8, send));
    let mut off = 0;
    let mut finished = false;
    while start.elapsed() < Duration::from_secs(8) && !c.is_closed() {
        while let Some(d) = c.poll_transmit(now()) {
            s.send_to(&d, addr).unwrap();
        }
        if let Ok((n, _)) = s.recv_from(&mut buf) {
            c.handle(&buf[..n], now());
        }
        if c.timeout().is_some_and(|t| t <= now()) {
            c.on_timeout(now());
        }
        if c.is_connected() && sid.is_none() {
            sid = Some(c.open_stream().unwrap());
        }
        if let Some(id) = sid {
            if off < body.len() {
                off += c.write(id, &body[off..]).unwrap_or(0);
            } else if !finished {
                c.finish(id).unwrap();
                finished = true;
            }
        }
    }
}

#[test]
fn short_stream_is_bad_frame() {
    let (secret, public) = key(11);
    let s = sock();
    let addr = s.local_addr().unwrap();
    let h = thread::spawn(move || {
        let mut out = Vec::new();
        recv_file(
            s,
            secret,
            [3; 32],
            Config::default(),
            Lossy::default(),
            &mut out,
            Duration::from_secs(10),
        )
    });
    short_client(addr, public, 1000, 400);
    let res = h.join().unwrap();
    assert!(matches!(res, Err(UdpError::BadFrame)), "got {res:?}");
}

fn transfer(n: usize, lossy: Lossy, secs: u64) {
    let (secret, public) = key(12);
    let s = sock();
    let addr = s.local_addr().unwrap();
    let h = thread::spawn(move || {
        let mut out = Vec::new();
        let r = recv_file(
            s,
            secret,
            [4; 32],
            Config::default(),
            Lossy { seed: 991, ..lossy },
            &mut out,
            Duration::from_secs(secs),
        )
        .unwrap();
        (r, out)
    });
    let mut r = Rng::new(0x35_f11e);
    let data = payload(&mut r, n);
    let hash = send_file(
        sock(),
        addr,
        public,
        [5; 32],
        Config::default(),
        lossy,
        &mut &data[..],
        n as u64,
        Duration::from_secs(secs),
    )
    .unwrap();
    let ((len, rh), out) = h.join().unwrap();
    assert_eq!(len, n as u64);
    assert_eq!(rh, hash);
    assert!(out == data, "payload differs");
}

#[test]
fn lossy_file_4mb() {
    transfer(
        4 << 20,
        Lossy {
            drop_per_mille: 20,
            dup_per_mille: 10,
            delay_per_mille: 20,
            seed: 5,
        },
        120,
    );
}

#[test]
#[ignore = "release only: cargo test --release -p tremor --test udp_leader -- --ignored"]
fn lossy_file_100mb() {
    transfer(
        100 << 20,
        Lossy {
            drop_per_mille: 20,
            dup_per_mille: 0,
            delay_per_mille: 0,
            seed: 6,
        },
        900,
    );
}
