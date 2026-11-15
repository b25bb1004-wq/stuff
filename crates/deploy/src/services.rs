//! Service runners for Iridium, Argo Registry, and Aurum nodes.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use argo::registry::Registry;
use aurum::node::net::{Peer, PeerId};
use aurum::node::{Msg, Node};
use aurum::Address;
use iridium::net::Server as IridiumServer;
use mercury::server::{Config as HttpConfig, Router, Server as HttpServer};
use mercury::Response;
use sphinx::{ed25519, sha256};

const STOP_POLL_MS: u64 = 50;
const UDP_POLL_MAX_MS: u64 = 10;
const DNS_RETRY_INTERVAL: Duration = Duration::from_millis(500);
const DEMO_GENESIS_BALANCE: u64 = 1_000_000;

/// Configuration for running an Aurum validator node.
#[derive(Debug, Clone)]
pub struct AurumConfig {
    /// Validator identifier (`1..=validators`).
    pub id: u8,
    /// UDP socket address to bind for peer-to-peer traffic.
    pub listen: String,
    /// HTTP socket address to bind for `GET /health`.
    pub health: String,
    /// Configured remote peers as `(peer_id, host:port)`.
    pub peers: Vec<(u8, String)>,
    /// Total number of validators in genesis.
    pub validators: u8,
}

/// Run an Iridium TCP database server until `stop` becomes true.
pub fn run_iridium(
    listen: &str,
    data: &Path,
    stop: &AtomicBool,
    ready: impl FnOnce(SocketAddr),
) -> Result<(), String> {
    let addr = resolve_first(listen)?;
    fs::create_dir_all(data).map_err(|e| format!("create data dir {}: {e}", data.display()))?;
    let main = data.join("main.db");
    let log = data.join("log.wal");

    let server =
        IridiumServer::start(addr, main, log).map_err(|e| format!("iridium start: {e}"))?;
    ready(server.local_addr());

    while !stop.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(STOP_POLL_MS));
    }
    server.stop();
    Ok(())
}

/// Run an Argo in-memory package registry HTTP server until `stop` becomes true.
pub fn run_registry(
    listen: &str,
    publishers: Vec<[u8; 32]>,
    stop: &AtomicBool,
    ready: impl FnOnce(SocketAddr),
) -> Result<(), String> {
    let registry =
        Registry::start(listen, publishers).map_err(|e| format!("registry start: {e}"))?;
    ready(registry.addr());

    while !stop.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(STOP_POLL_MS));
    }
    registry.stop();
    Ok(())
}

/// Run an Aurum validator node over UDP with a Mercury `/health` HTTP server until `stop` is true.
pub fn run_aurum(
    cfg: &AurumConfig,
    stop: &AtomicBool,
    ready: impl FnOnce(SocketAddr),
) -> Result<(), String> {
    if cfg.validators == 0 {
        return Err("validators must be at least 1".to_string());
    }
    if cfg.id == 0 || cfg.id > cfg.validators {
        return Err(format!(
            "node id {} must be in 1..={}",
            cfg.id, cfg.validators
        ));
    }

    let mut genesis: Vec<(Address, u64)> = Vec::with_capacity(usize::from(cfg.validators));
    let mut validator_addrs: Vec<Address> = Vec::with_capacity(usize::from(cfg.validators));
    for i in 1..=cfg.validators {
        let secret = demo_validator_secret(i);
        let addr = ed25519::public_key(&secret);
        genesis.push((addr, DEMO_GENESIS_BALANCE));
        validator_addrs.push(addr);
    }

    let my_validator_secret = demo_validator_secret(cfg.id);
    let my_validator_addr = ed25519::public_key(&my_validator_secret);
    let my_xenon_secret = demo_xenon_secret(cfg.id);
    let my_seed = fresh_peer_seed()?;

    let listen_addr = resolve_first(&cfg.listen)?;
    let socket =
        UdpSocket::bind(listen_addr).map_err(|e| format!("bind aurum udp {listen_addr}: {e}"))?;
    socket
        .set_nonblocking(true)
        .map_err(|e| format!("set nonblocking udp: {e}"))?;

    let node = Node::new(&genesis, &validator_addrs);
    let mut peer = Peer::new(u32::from(cfg.id), node.clone(), my_xenon_secret, my_seed);

    let mut peer_endpoints: BTreeMap<PeerId, PeerEndpoint> = BTreeMap::new();
    for (pid, spec) in &cfg.peers {
        if *pid == 0 {
            return Err("peer id must be >= 1".to_string());
        }
        if *pid == cfg.id {
            continue;
        }
        let pid_u32 = u32::from(*pid);
        let peer_pub =
            Peer::new(pid_u32, node.clone(), demo_xenon_secret(*pid), [0u8; 32]).public_key();
        peer.add_peer(pid_u32, peer_pub, 0);
        peer_endpoints.insert(
            pid_u32,
            PeerEndpoint {
                spec: spec.clone(),
                addr: resolve_udp(spec),
            },
        );
    }

    let initial_health = format_health(&peer);
    let health_state = Arc::new(RwLock::new(initial_health));
    let health_server = start_health_server(&cfg.health, Arc::clone(&health_state))?;
    ready(health_server.local_addr());

    let start = Instant::now();
    let mut last_dns_retry = Instant::now();
    let mut buf = vec![0u8; 65536];

    while !stop.load(Ordering::Acquire) {
        let mut did_work = false;
        let now_us = u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX);

        if last_dns_retry.elapsed() >= DNS_RETRY_INTERVAL {
            refresh_unresolved_peers(&mut peer_endpoints);
            last_dns_retry = Instant::now();
        }

        for _ in 0..64 {
            match socket.recv_from(&mut buf) {
                Ok((n, src)) => {
                    did_work = true;
                    let sender = find_peer_by_addr(&peer_endpoints, src).or_else(|| {
                        if last_dns_retry.elapsed() >= Duration::from_millis(100) {
                            refresh_all_peers(&mut peer_endpoints);
                            last_dns_retry = Instant::now();
                        }
                        find_peer_by_addr(&peer_endpoints, src)
                    });
                    if let Some(pid) = sender {
                        peer.receive(pid, &buf[..n], now_us);
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                            | io::ErrorKind::ConnectionReset
                    ) =>
                {
                    break;
                }
                Err(_) => break,
            }
        }

        if peer.timeout().is_some_and(|t| now_us >= t) {
            peer.on_timeout(now_us);
            did_work = true;
        }

        let now_secs = unix_timestamp_secs();
        if peer.node().chain().expected_proposer() == my_validator_addr
            && now_secs > peer.node().chain().tip().time
        {
            let mut scratch = peer.node().clone();
            if let Some(block) = scratch.propose(&my_validator_secret, now_secs) {
                peer.inject(Msg::Block(block), now_us);
                did_work = true;
            }
        }

        while let Some((to, dgram)) = peer.poll_transmit(now_us) {
            did_work = true;
            if let Some(ep) = peer_endpoints.get_mut(&to) {
                if ep.addr.is_none() {
                    ep.addr = resolve_udp(&ep.spec);
                }
                if let Some(dest) = ep.addr {
                    let _ = socket.send_to(&dgram, dest);
                }
            }
        }

        update_health_state(&health_state, &peer);

        if !did_work {
            let sleep_ms = peer
                .timeout()
                .map(|t| {
                    let wait_us = t.saturating_sub(now_us);
                    let wait_ms = wait_us.div_ceil(1000);
                    wait_ms.clamp(1, UDP_POLL_MAX_MS)
                })
                .unwrap_or(UDP_POLL_MAX_MS);
            thread::sleep(Duration::from_millis(sleep_ms));
        }
    }

    health_server.stop();
    Ok(())
}

struct PeerEndpoint {
    spec: String,
    addr: Option<SocketAddr>,
}

fn resolve_first(spec: &str) -> Result<SocketAddr, String> {
    if let Ok(sa) = spec.parse::<SocketAddr>() {
        return Ok(sa);
    }
    let mut addrs = spec
        .to_socket_addrs()
        .map_err(|e| format!("resolve {spec}: {e}"))?;
    addrs
        .next()
        .ok_or_else(|| format!("resolve {spec}: no addresses"))
}

fn resolve_udp(spec: &str) -> Option<SocketAddr> {
    if let Ok(sa) = spec.parse::<SocketAddr>() {
        return Some(sa);
    }
    let mut addrs: Vec<SocketAddr> = spec.to_socket_addrs().ok()?.collect();
    // Prefer IPv4 when available so Docker bridge hostnames match IPv4 sockets cleanly.
    if let Some(v4) = addrs.iter().copied().find(SocketAddr::is_ipv4) {
        return Some(v4);
    }
    addrs.pop()
}

fn refresh_unresolved_peers(peers: &mut BTreeMap<PeerId, PeerEndpoint>) {
    for ep in peers.values_mut() {
        if ep.addr.is_none() {
            ep.addr = resolve_udp(&ep.spec);
        }
    }
}

fn refresh_all_peers(peers: &mut BTreeMap<PeerId, PeerEndpoint>) {
    for ep in peers.values_mut() {
        if let Some(sa) = resolve_udp(&ep.spec) {
            ep.addr = Some(sa);
        }
    }
}

fn find_peer_by_addr(peers: &BTreeMap<PeerId, PeerEndpoint>, src: SocketAddr) -> Option<PeerId> {
    let norm_src = normalize_sockaddr(src);
    for (&pid, ep) in peers {
        if let Some(sa) = ep.addr {
            if normalize_sockaddr(sa) == norm_src {
                return Some(pid);
            }
        }
    }
    None
}

fn normalize_sockaddr(sa: SocketAddr) -> SocketAddr {
    match sa {
        SocketAddr::V6(v6) => {
            if let Some(v4) = v6.ip().to_ipv4_mapped() {
                SocketAddr::new(IpAddr::V4(v4), v6.port())
            } else {
                SocketAddr::V6(v6)
            }
        }
        other => other,
    }
}

fn demo_validator_secret(id: u8) -> [u8; 32] {
    sha256::digest(format!("monolith-demo-validator-{id}").as_bytes())
}

fn demo_xenon_secret(id: u8) -> [u8; 32] {
    sha256::digest(format!("monolith-demo-xenon-{id}").as_bytes())
}

/// Handshake RNG seed from the OS CSPRNG; fails closed when there is none.
fn fresh_peer_seed() -> Result<[u8; 32], String> {
    let mut seed = [0u8; 32];
    entropy::fill(&mut seed).map_err(|e| format!("peer seed: {e}"))?;
    Ok(seed)
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1)
        .max(1)
}

fn format_health(peer: &Peer) -> String {
    let height = peer.node().chain().height();
    let tip = to_hex(&peer.node().tip());
    let peers = peer.connected().len();
    format!("height={height} tip={tip} peers={peers}")
}

fn update_health_state(state: &RwLock<String>, peer: &Peer) {
    let next = format_health(peer);
    let mut guard = state
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *guard != next {
        *guard = next;
    }
}

fn start_health_server(listen: &str, state: Arc<RwLock<String>>) -> Result<HttpServer, String> {
    let mut router = Router::new();
    router.route(
        "GET",
        "/health",
        Box::new(move |_req| {
            let body = state
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_bytes()
                .to_vec();
            Response {
                minor: 1,
                status: 200,
                reason: "OK".to_string(),
                headers: vec![
                    (
                        "Content-Length".to_string(),
                        body.len().to_string().into_bytes(),
                    ),
                    (
                        "Content-Type".to_string(),
                        b"text/plain; charset=utf-8".to_vec(),
                    ),
                ],
                body,
            }
        }),
    );

    let cfg = HttpConfig {
        addr: listen.to_string(),
        threads: 2,
        ..HttpConfig::default()
    };
    HttpServer::start(cfg, router).map_err(|e| format!("health server start {listen}: {e}"))
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[usize::from(b >> 4)] as char);
        out.push(HEX[usize::from(b & 0x0f)] as char);
    }
    out
}
