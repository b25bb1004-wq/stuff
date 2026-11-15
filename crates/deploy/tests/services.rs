//! Integration tests for deploy services, health probes, CLI parsers, and Unix SIGTERM.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use ::iridium::net::Client as IridiumClient;
use deploy::cli::{aurum, healthcheck, iridium, registry};
use deploy::{probe, run_aurum, run_iridium, run_registry, AurumConfig};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "monolith_deploy_{tag}_{}_{:?}",
        std::process::id(),
        thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn closed_port_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().to_string()
}

fn free_udp_ports(count: usize) -> Vec<u16> {
    let mut sockets = Vec::with_capacity(count);
    let mut ports = Vec::with_capacity(count);
    for _ in 0..count {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        ports.push(sock.local_addr().unwrap().port());
        sockets.push(sock);
    }
    ports
}

#[test]
fn iridium_persists_across_restart_and_stops_promptly() {
    let data_dir = temp_dir("iridium_persist");

    let stop1 = Arc::new(AtomicBool::new(false));
    let stop1_t = Arc::clone(&stop1);
    let dir1 = data_dir.clone();
    let (tx1, rx1) = mpsc::channel::<SocketAddr>();
    let h1 = thread::spawn(move || {
        run_iridium("127.0.0.1:0", &dir1, &stop1_t, move |addr| {
            let _ = tx1.send(addr);
        })
    });

    let addr1 = rx1.recv_timeout(Duration::from_secs(3)).unwrap();
    probe::tcp(&addr1.to_string(), Duration::from_secs(2)).unwrap();

    {
        let mut client = IridiumClient::connect(addr1).unwrap();
        client
            .execute("CREATE TABLE items (id INT, name TEXT)")
            .unwrap()
            .unwrap();
        client
            .execute("INSERT INTO items VALUES (1, 'alpha')")
            .unwrap()
            .unwrap();
    }

    let stop_start = Instant::now();
    stop1.store(true, Ordering::Release);
    h1.join().unwrap().unwrap();
    assert!(stop_start.elapsed() < Duration::from_secs(2));

    let stop2 = Arc::new(AtomicBool::new(false));
    let stop2_t = Arc::clone(&stop2);
    let dir2 = data_dir.clone();
    let (tx2, rx2) = mpsc::channel::<SocketAddr>();
    let h2 = thread::spawn(move || {
        run_iridium("127.0.0.1:0", &dir2, &stop2_t, move |addr| {
            let _ = tx2.send(addr);
        })
    });

    let addr2 = rx2.recv_timeout(Duration::from_secs(3)).unwrap();
    {
        let mut client = IridiumClient::connect(addr2).unwrap();
        let rows = client
            .execute("SELECT id, name FROM items")
            .unwrap()
            .unwrap();
        let debug = format!("{rows:?}");
        assert!(debug.contains("alpha"), "expected persisted row: {debug}");
    }

    let stop2_start = Instant::now();
    stop2.store(true, Ordering::Release);
    h2.join().unwrap().unwrap();
    assert!(stop2_start.elapsed() < Duration::from_secs(2));

    let _ = fs::remove_dir_all(&data_dir);
}

#[test]
fn registry_serves_index_and_stops_promptly() {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = Arc::clone(&stop);
    let (tx, rx) = mpsc::channel::<SocketAddr>();
    let handle = thread::spawn(move || {
        run_registry("127.0.0.1:0", Vec::new(), &stop_t, move |addr| {
            let _ = tx.send(addr);
        })
    });

    let addr = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let body = probe::http(&addr.to_string(), "/index", Duration::from_secs(2)).unwrap();
    assert!(body.is_empty());

    let t0 = Instant::now();
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
    assert!(t0.elapsed() < Duration::from_secs(2));
}

fn parse_health(body: &str) -> Option<(u64, String, usize)> {
    let mut height = None;
    let mut tip = None;
    let mut peers = None;
    for part in body.split_ascii_whitespace() {
        if let Some(v) = part.strip_prefix("height=") {
            height = v.parse::<u64>().ok();
        } else if let Some(v) = part.strip_prefix("tip=") {
            tip = Some(v.to_string());
        } else if let Some(v) = part.strip_prefix("peers=") {
            peers = v.parse::<usize>().ok();
        }
    }
    Some((height?, tip?, peers?))
}

#[test]
fn aurum_three_nodes_converge_and_stop_promptly() {
    let ports = free_udp_ports(3);
    let addrs: Vec<String> = ports.iter().map(|p| format!("127.0.0.1:{p}")).collect();

    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();
    let mut health_addrs = Vec::new();

    for i in 0..3usize {
        let id = u8::try_from(i + 1).unwrap();
        let peers: Vec<(u8, String)> = (0..3usize)
            .filter(|&j| j != i)
            .map(|j| (u8::try_from(j + 1).unwrap(), addrs[j].clone()))
            .collect();
        let cfg = AurumConfig {
            id,
            listen: addrs[i].clone(),
            health: "127.0.0.1:0".to_string(),
            peers,
            validators: 3,
        };
        let stop_t = Arc::clone(&stop);
        let (tx, rx) = mpsc::channel::<SocketAddr>();
        handles.push(thread::spawn(move || {
            run_aurum(&cfg, &stop_t, move |addr| {
                let _ = tx.send(addr);
            })
        }));
        health_addrs.push(rx.recv_timeout(Duration::from_secs(3)).unwrap().to_string());
    }

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut converged = false;
    while Instant::now() < deadline {
        let mut parsed = Vec::new();
        for ha in &health_addrs {
            if let Ok(body) = probe::http(ha, "/health", Duration::from_secs(1)) {
                if let Some(info) = parse_health(&body) {
                    parsed.push(info);
                }
            }
        }
        if parsed.len() == 3 {
            let h0 = parsed[0].0;
            let t0 = &parsed[0].1;
            if parsed
                .iter()
                .all(|(h, t, _)| *h >= 3 && t == t0 && *h == h0)
            {
                converged = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(200));
    }

    let stop_start = Instant::now();
    stop.store(true, Ordering::Release);
    for h in handles {
        h.join().unwrap().unwrap();
    }
    assert!(stop_start.elapsed() < Duration::from_secs(2));
    assert!(converged, "3 aurum nodes did not converge to height >= 3");
}

#[test]
fn probes_fail_quickly_on_closed_port() {
    let addr = closed_port_addr();
    let timeout = Duration::from_millis(300);

    let t0 = Instant::now();
    assert!(probe::tcp(&addr, timeout).is_err());
    assert!(t0.elapsed() < Duration::from_secs(2));

    let t1 = Instant::now();
    assert!(probe::http(&addr, "/health", timeout).is_err());
    assert!(t1.elapsed() < Duration::from_secs(2));
}

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|x| (*x).to_string()).collect()
}

#[test]
fn cli_flag_parsers_good_and_bad_cases() {
    let ir_ok = iridium::parse_args(&s(&["--listen", "0.0.0.0:7400", "--data", "/data"])).unwrap();
    assert_eq!(ir_ok.listen, "0.0.0.0:7400");
    assert_eq!(ir_ok.data, PathBuf::from("/data"));
    assert!(iridium::parse_args(&s(&["--listen", "0.0.0.0:7400"])).is_err());
    assert!(iridium::parse_args(&s(&["--listen", "0.0.0.0:7400", "--bogus"])).is_err());
    assert!(iridium::parse_args(&s(&["--listen"])).is_err());

    let pub_hex = "ab".repeat(32);
    let reg_ok =
        registry::parse_args(&s(&["--listen", "0.0.0.0:7500", "--publisher", &pub_hex])).unwrap();
    assert_eq!(reg_ok.publishers, vec![[0xab; 32]]);
    assert!(registry::parse_args(&s(&["--listen", "0.0.0.0:7500", "--publisher", "zz"])).is_err());
    assert!(registry::parse_args(&s(&[])).is_err());

    let au_ok = aurum::parse_args(&s(&[
        "--id",
        "1",
        "--listen",
        "0.0.0.0:7700",
        "--health",
        "0.0.0.0:7600",
        "--validators",
        "3",
        "--peer",
        "2@aurum2:7700",
        "--peer",
        "3@aurum3:7700",
    ]))
    .unwrap();
    assert_eq!(au_ok.id, 1);
    assert_eq!(au_ok.validators, 3);
    assert_eq!(au_ok.peers.len(), 2);
    assert!(aurum::parse_args(&s(&[
        "--id",
        "4",
        "--listen",
        "0.0.0.0:7700",
        "--health",
        "0.0.0.0:7600",
        "--validators",
        "3",
    ]))
    .is_err());
    assert!(aurum::parse_args(&s(&[
        "--id",
        "1",
        "--listen",
        "0.0.0.0:7700",
        "--health",
        "0.0.0.0:7600",
        "--validators",
        "3",
        "--peer",
        "badpeer",
    ]))
    .is_err());

    assert_eq!(
        healthcheck::parse_args(&s(&["tcp", "127.0.0.1:7400"])).unwrap(),
        healthcheck::Target::Tcp {
            addr: "127.0.0.1:7400".to_string(),
        }
    );
    assert_eq!(
        healthcheck::parse_args(&s(&["http", "127.0.0.1:7500", "/index"])).unwrap(),
        healthcheck::Target::Http {
            addr: "127.0.0.1:7500".to_string(),
            path: "/index".to_string(),
        }
    );
    assert!(healthcheck::parse_args(&s(&["tcp"])).is_err());
    assert!(healthcheck::parse_args(&s(&["udp", "127.0.0.1:1"])).is_err());
}

#[cfg(unix)]
#[test]
fn iridium_binary_stops_cleanly_on_sigterm() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    extern "C" {
        fn kill(pid: std::ffi::c_int, sig: std::ffi::c_int) -> std::ffi::c_int;
    }

    let data_dir = temp_dir("iridium_sigterm");
    let mut child = Command::new(env!("CARGO_BIN_EXE_iridium-server"))
        .arg("--listen")
        .arg("127.0.0.1:0")
        .arg("--data")
        .arg(&data_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).unwrap();
    assert!(bytes > 0 && line.contains("listening"), "got line: {line}");

    let pid = std::ffi::c_int::try_from(child.id()).unwrap();
    const SIGTERM: std::ffi::c_int = 15;
    // SAFETY: `pid` is the live OS process ID of the child we just spawned.
    let rc = unsafe { kill(pid, SIGTERM) };
    assert_eq!(rc, 0);

    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st;
        }
        assert!(
            Instant::now() < deadline,
            "iridium-server did not exit within 3s"
        );
        thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(status.code(), Some(0));
    let _ = fs::remove_dir_all(&data_dir);
}
