//! Sans-IO node gossip and block sync over Xenon encrypted streams.

use crate::node::{Msg, Node};
use sphinx::sha256;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use xenon::{Config, Connection, Server};

/// Logical network identifier for a peer.
pub type PeerId = u32;
/// Sync loop period in microseconds.
pub const SYNC_INTERVAL_US: u64 = 100_000;
/// Redial a client link still not connected this long after its last dial.
pub const REDIAL_US: u64 = 2_000_000;
/// Maximum encoded message payload accepted on a single stream; larger payloads are dropped.
pub const MAX_MSG: usize = 4 << 20;

const MAX_PENDING_GOSSIP: usize = 10_000;
const STREAM_RESET_CODE: u64 = 1;
const READ_CHUNK: usize = 4096;

/// Buffered bytes and finished state for an accepted incoming stream.
#[derive(Debug, Clone)]
enum RecvStream {
    Open(Vec<u8>),
    Dropped,
}

/// Per-link outgoing queues, active stream write state, and incoming stream buffers.
#[derive(Debug, Clone, Default)]
struct LinkIo {
    pending_gossip: VecDeque<Vec<u8>>,
    send_queue: VecDeque<Vec<u8>>,
    active_send: Option<(u64, Vec<u8>, usize)>,
    recv_streams: BTreeMap<u64, RecvStream>,
}

fn push_bounded(queue: &mut VecDeque<Vec<u8>>, bytes: Vec<u8>) {
    if queue.len() >= MAX_PENDING_GOSSIP {
        queue.pop_front();
    }
    queue.push_back(bytes);
}

impl LinkIo {
    fn reset_transport(&mut self) {
        self.send_queue.clear();
        self.active_send = None;
        self.recv_streams.clear();
    }

    fn pump_connected(&mut self, conn: &mut Connection) {
        while let Some(bytes) = self.pending_gossip.pop_front() {
            push_bounded(&mut self.send_queue, bytes);
        }
        self.flush_outgoing(conn);
    }

    fn flush_outgoing(&mut self, conn: &mut Connection) {
        if !conn.is_connected() {
            return;
        }
        loop {
            if self.active_send.is_none() {
                let Some(front) = self.send_queue.front() else {
                    break;
                };
                if front.len() > MAX_MSG {
                    self.send_queue.pop_front();
                    continue;
                }
                match conn.open_stream() {
                    Ok(sid) => {
                        if let Some(bytes) = self.send_queue.pop_front() {
                            self.active_send = Some((sid, bytes, 0));
                        }
                    }
                    Err(_) => break,
                }
            }
            let Some((sid, bytes, offset)) = self.active_send.as_mut() else {
                break;
            };
            if *offset < bytes.len() {
                match conn.write(*sid, &bytes[*offset..]) {
                    Ok(0) => break,
                    Ok(n) => {
                        *offset = offset.saturating_add(n);
                    }
                    Err(_) => {
                        self.active_send = None;
                        break;
                    }
                }
            }
            if *offset >= bytes.len() {
                let _ = conn.finish(*sid);
                self.active_send = None;
            }
        }
    }

    fn drain_streams(&mut self, conn: &mut Connection) -> Vec<Msg> {
        while let Some(sid) = conn.accept_stream() {
            self.recv_streams
                .entry(sid)
                .or_insert_with(|| RecvStream::Open(Vec::new()));
        }
        let sids: Vec<u64> = self.recv_streams.keys().copied().collect();
        let mut out = Vec::new();
        let mut tmp = [0u8; READ_CHUNK];
        for sid in sids {
            let Some(state) = self.recv_streams.get_mut(&sid) else {
                continue;
            };
            let mut remove = false;
            let mut reset_stream = false;
            loop {
                match conn.read(sid, &mut tmp) {
                    Ok(0) => break,
                    Ok(n) => match state {
                        RecvStream::Open(buf) => {
                            if buf.len().saturating_add(n) > MAX_MSG {
                                *state = RecvStream::Dropped;
                                reset_stream = true;
                            } else {
                                buf.extend_from_slice(&tmp[..n]);
                            }
                        }
                        RecvStream::Dropped => {}
                    },
                    Err(_) => {
                        remove = true;
                        break;
                    }
                }
            }
            if reset_stream {
                let _ = conn.reset(sid, STREAM_RESET_CODE);
            }
            if remove {
                self.recv_streams.remove(&sid);
                continue;
            }
            if conn.stream_finished(sid) {
                if let Some(RecvStream::Open(buf)) = self.recv_streams.remove(&sid) {
                    match Msg::decode(&buf) {
                        Ok(msg) => out.push(msg),
                        Err(_) => {
                            let _ = conn.reset(sid, STREAM_RESET_CODE);
                        }
                    }
                }
            }
        }
        out
    }
}

/// Outbound client connection state for a peer with `peer_id > self.id`.
#[derive(Debug, Clone)]
struct ClientLink {
    key: [u8; 32],
    attempt: u64,
    dialed_at: u64,
    conn: Connection,
    io: LinkIo,
}

/// Inbound server-side state for a peer with `peer_id < self.id`.
#[derive(Debug, Clone, Default)]
struct ServerPeer {
    cid: Option<u64>,
    connected: bool,
    io: LinkIo,
}

/// A Monolith node wrapped with sans-IO Xenon peer links, gossip broadcasting, and periodic sync.
pub struct Peer {
    id: PeerId,
    node: Node,
    seed: [u8; 32],
    config: Config,
    server: Server,
    clients: BTreeMap<PeerId, ClientLink>,
    server_peers: BTreeMap<PeerId, ServerPeer>,
    cid_to_peer: BTreeMap<u64, PeerId>,
    next_sync: u64,
}

impl Peer {
    /// Create a peer with logical `id`, initial `node`, Xenon `secret`, and RNG `seed`.
    pub fn new(id: PeerId, node: Node, secret: [u8; 32], seed: [u8; 32]) -> Peer {
        let config = Config {
            max_streams: 1_000_000,
            stream_window: MAX_MSG as u64,
            conn_window: (MAX_MSG as u64).saturating_mul(4),
            ..Config::default()
        };
        let server_seed = derive_seed(b"aurum-net-server\0", &[u64::from(id)], &seed);
        Peer {
            id,
            node,
            seed,
            config,
            server: Server::new(secret, server_seed, config),
            clients: BTreeMap::new(),
            server_peers: BTreeMap::new(),
            cid_to_peer: BTreeMap::new(),
            next_sync: SYNC_INTERVAL_US,
        }
    }

    /// Return this peer's Xenon static X25519 public key derived from `secret`.
    pub fn public_key(&self) -> [u8; 32] {
        self.server.public_key()
    }

    /// Register another peer; the lower `PeerId` dials while the higher listens on its server.
    pub fn add_peer(&mut self, id: PeerId, key: [u8; 32], now: u64) {
        if id == self.id {
            return;
        }
        if self.id < id {
            if self.clients.contains_key(&id) {
                return;
            }
            let link_seed = derive_seed(
                b"aurum-net-link\0",
                &[u64::from(self.id), u64::from(id), 0],
                &self.seed,
            );
            let conn = Connection::connect(key, link_seed, self.config, now);
            self.clients.insert(
                id,
                ClientLink {
                    key,
                    attempt: 0,
                    dialed_at: now,
                    conn,
                    io: LinkIo::default(),
                },
            );
        } else {
            self.server_peers.entry(id).or_default();
        }
    }

    /// Borrow the underlying blockchain `Node`.
    pub fn node(&self) -> &Node {
        &self.node
    }

    /// Inject a local message into the node as if received, broadcasting any replies.
    pub fn inject(&mut self, msg: Msg, _now: u64) {
        let replies = self.node.handle(msg);
        self.broadcast(replies);
    }

    /// Feed a datagram received from `from` at time `now` (microseconds).
    pub fn receive(&mut self, from: PeerId, datagram: &[u8], now: u64) {
        if from == self.id {
            return;
        }
        let msgs = if self.id < from {
            self.receive_client(from, datagram, now)
        } else {
            self.receive_server(from, datagram, now)
        };
        for msg in msgs {
            let replies = self.node.handle(msg);
            self.broadcast(replies);
        }
    }

    /// Poll the next outgoing `(PeerId, datagram)` pair at time `now` (microseconds).
    pub fn poll_transmit(&mut self, now: u64) -> Option<(PeerId, Vec<u8>)> {
        for (&peer_id, client) in &mut self.clients {
            if client.conn.is_connected() {
                client.io.pump_connected(&mut client.conn);
            }
            if let Some(dgram) = client.conn.poll_transmit(now) {
                return Some((peer_id, dgram));
            }
        }
        for cid in self.server.cids() {
            let Some(&peer_id) = self.cid_to_peer.get(&cid) else {
                continue;
            };
            let (Some(sp), Some(conn)) = (
                self.server_peers.get_mut(&peer_id),
                self.server.connection(cid),
            ) else {
                continue;
            };
            if sp.cid == Some(cid) {
                sp.connected = conn.is_connected();
                if sp.connected {
                    sp.io.pump_connected(conn);
                }
            }
            let dgram = conn.poll_transmit(now);
            if sp.cid == Some(cid) {
                sp.connected = conn.is_connected();
            }
            if let Some(d) = dgram {
                return Some((peer_id, d));
            }
        }
        None
    }

    /// Return the earliest Xenon connection/server timer or next sync tick timestamp.
    pub fn timeout(&self) -> Option<u64> {
        let client_min = self.clients.values().filter_map(|c| c.conn.timeout()).min();
        [Some(self.next_sync), client_min, self.server.timeout()]
            .into_iter()
            .flatten()
            .min()
    }

    /// Drive Xenon timers for all connections and the server, then run the sync tick if due.
    pub fn on_timeout(&mut self, now: u64) {
        for (&peer_id, client) in &mut self.clients {
            client.conn.on_timeout(now);
            // xenon backs off to 9 s between handshake tries; after a partition heals,
            // a fresh dial every REDIAL_US reconnects far sooner (sync ticks drive this check)
            let stalled =
                !client.conn.is_connected() && now >= client.dialed_at.saturating_add(REDIAL_US);
            if client.conn.is_closed() || stalled {
                client.dialed_at = now;
                client.io.reset_transport();
                client.attempt = client.attempt.saturating_add(1);
                let link_seed = derive_seed(
                    b"aurum-net-link\0",
                    &[u64::from(self.id), u64::from(peer_id), client.attempt],
                    &self.seed,
                );
                client.conn = Connection::connect(client.key, link_seed, self.config, now);
            }
        }
        self.server.on_timeout(now);
        let live_cids: BTreeSet<u64> = self.server.cids().into_iter().collect();
        self.cid_to_peer.retain(|cid, _| live_cids.contains(cid));
        for sp in self.server_peers.values_mut() {
            if let Some(cid) = sp.cid {
                if !live_cids.contains(&cid) {
                    sp.cid = None;
                    sp.connected = false;
                    sp.io.reset_transport();
                } else {
                    sp.connected = self
                        .server
                        .connection(cid)
                        .is_some_and(|c| c.is_connected());
                }
            }
        }
        if now >= self.next_sync {
            self.next_sync = now.saturating_add(SYNC_INTERVAL_US);
            self.run_sync_tick();
        }
    }

    /// Return sorted `PeerId`s of all peers whose Xenon handshake is currently complete.
    pub fn connected(&self) -> Vec<PeerId> {
        let mut out: Vec<PeerId> = self
            .clients
            .iter()
            .filter(|(_, c)| c.conn.is_connected())
            .map(|(&id, _)| id)
            .chain(
                self.server_peers
                    .iter()
                    .filter(|(_, s)| s.connected)
                    .map(|(&id, _)| id),
            )
            .collect();
        out.sort_unstable();
        out
    }

    fn receive_client(&mut self, from: PeerId, datagram: &[u8], now: u64) -> Vec<Msg> {
        let Some(client) = self.clients.get_mut(&from) else {
            return Vec::new();
        };
        client.conn.handle(datagram, now);
        if client.conn.is_connected() {
            client.io.pump_connected(&mut client.conn);
        }
        client.io.drain_streams(&mut client.conn)
    }

    fn receive_server(&mut self, from: PeerId, datagram: &[u8], now: u64) -> Vec<Msg> {
        if !self.server_peers.contains_key(&from) {
            return Vec::new();
        }
        let Some(cid) = self.server.handle(datagram, now) else {
            return Vec::new();
        };
        if let Some(sp) = self.server_peers.get_mut(&from) {
            if sp.cid != Some(cid) {
                if let Some(old_cid) = sp.cid {
                    sp.io.reset_transport();
                    sp.connected = false;
                    if let Some(old_conn) = self.server.connection(old_cid) {
                        old_conn.close(0, "superseded");
                    }
                }
                sp.cid = Some(cid);
                self.cid_to_peer.insert(cid, from);
            }
        }
        while let Some(accepted_cid) = self.server.accept() {
            let Some(&peer_id) = self.cid_to_peer.get(&accepted_cid) else {
                continue;
            };
            if let Some(sp) = self.server_peers.get_mut(&peer_id) {
                if sp.cid == Some(accepted_cid) {
                    sp.connected = true;
                    if let Some(conn) = self.server.connection(accepted_cid) {
                        sp.io.pump_connected(conn);
                    }
                }
            }
        }
        let (Some(sp), Some(conn)) = (
            self.server_peers.get_mut(&from),
            self.server.connection(cid),
        ) else {
            return Vec::new();
        };
        if sp.cid != Some(cid) {
            return Vec::new();
        }
        sp.connected = conn.is_connected();
        if sp.connected {
            sp.io.pump_connected(conn);
        }
        sp.io.drain_streams(conn)
    }

    fn broadcast(&mut self, msgs: Vec<Msg>) {
        for msg in msgs {
            self.send_to_peers(msg.encode(), true);
        }
    }

    fn run_sync_tick(&mut self) {
        let tip = Msg::Tip {
            hash: self.node.tip(),
            height: self.node.chain().height(),
        }
        .encode();
        self.send_to_peers(tip, false);
        for hash in self.node.missing() {
            self.send_to_peers(Msg::GetBlock(hash).encode(), false);
        }
    }

    fn send_to_peers(&mut self, bytes: Vec<u8>, queue_if_disconnected: bool) {
        if bytes.len() > MAX_MSG {
            return;
        }
        for client in self.clients.values_mut() {
            if client.conn.is_connected() {
                push_bounded(&mut client.io.send_queue, bytes.clone());
                client.io.flush_outgoing(&mut client.conn);
            } else if queue_if_disconnected {
                push_bounded(&mut client.io.pending_gossip, bytes.clone());
            }
        }
        for sp in self.server_peers.values_mut() {
            if sp.connected {
                push_bounded(&mut sp.io.send_queue, bytes.clone());
                if let Some(conn) = sp.cid.and_then(|cid| self.server.connection(cid)) {
                    sp.io.flush_outgoing(conn);
                }
            } else if queue_if_disconnected {
                push_bounded(&mut sp.io.pending_gossip, bytes.clone());
            }
        }
    }
}

fn derive_seed(tag: &[u8], parts: &[u64], seed: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(tag.len() + parts.len() * 8 + 32);
    buf.extend_from_slice(tag);
    for &p in parts {
        buf.extend_from_slice(&p.to_le_bytes());
    }
    buf.extend_from_slice(seed);
    sha256::digest(&buf)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::Address;
    use sphinx::ed25519;

    fn addr(b: u8) -> Address {
        ed25519::public_key(&[b; 32])
    }

    fn make_node() -> Node {
        Node::new(&[(addr(1), 1000)], &[addr(1)])
    }

    fn pump_all(peers: &mut [&mut Peer], now: u64) {
        for _ in 0..32 {
            let mut moved = false;
            for i in 0..peers.len() {
                while let Some((to, dgram)) = peers[i].poll_transmit(now) {
                    let from = peers[i].id;
                    if let Some(target) = peers.iter_mut().find(|p| p.id == to) {
                        target.receive(from, &dgram, now);
                    }
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
    }

    #[test]
    fn handshake_unknown_peer_drop_and_sorted_connected() {
        let mut p1 = Peer::new(1, make_node(), [11; 32], [21; 32]);
        let mut p2 = Peer::new(2, make_node(), [12; 32], [22; 32]);
        let mut p3 = Peer::new(3, make_node(), [13; 32], [23; 32]);

        p1.add_peer(1, p1.public_key(), 0);
        p1.add_peer(3, p3.public_key(), 0);
        p1.add_peer(2, p2.public_key(), 0);
        p2.add_peer(1, p1.public_key(), 0);
        p3.add_peer(1, p1.public_key(), 0);

        // Unknown sender datagram is ignored without panicking.
        p1.receive(99, &[0u8; 64], 0);
        assert!(p1.connected().is_empty());

        pump_all(&mut [&mut p1, &mut p2, &mut p3], 100);
        assert_eq!(p1.connected(), vec![2, 3]);
        assert_eq!(p2.connected(), vec![1]);
        assert_eq!(p3.connected(), vec![1]);
    }

    #[test]
    fn pending_gossip_caps_at_10000_dropping_oldest() {
        let mut io = LinkIo::default();
        for i in 0..10_050u32 {
            push_bounded(&mut io.pending_gossip, i.to_le_bytes().to_vec());
        }
        assert_eq!(io.pending_gossip.len(), 10_000);
        assert_eq!(io.pending_gossip.front().unwrap(), &50u32.to_le_bytes());
        assert_eq!(io.pending_gossip.back().unwrap(), &10_049u32.to_le_bytes());
    }

    #[test]
    fn sync_tick_timing_and_oversize_or_corrupt_stream_dropped() {
        let mut p1 = Peer::new(1, make_node(), [31; 32], [41; 32]);
        let mut p2 = Peer::new(2, make_node(), [32; 32], [42; 32]);
        p1.add_peer(2, p2.public_key(), 0);
        p2.add_peer(1, p1.public_key(), 0);
        pump_all(&mut [&mut p1, &mut p2], 10);

        // Before SYNC_INTERVAL_US, on_timeout does not fire sync.
        p1.on_timeout(SYNC_INTERVAL_US - 1);
        assert!(p1.poll_transmit(SYNC_INTERVAL_US - 1).is_none());

        // At SYNC_INTERVAL_US, p1 sends Msg::Tip.
        p1.on_timeout(SYNC_INTERVAL_US);
        assert!(p1.poll_transmit(SYNC_INTERVAL_US).is_some());

        // Inject invalid stream payload and oversize stream from p1's client conn directly.
        let conn = &mut p1.clients.get_mut(&2).unwrap().conn;
        let bad_sid = conn.open_stream().unwrap();
        assert_eq!(conn.write(bad_sid, &[255, 1, 2, 3]).unwrap(), 4);
        conn.finish(bad_sid).unwrap();
        let big_sid = conn.open_stream().unwrap();
        let big_chunk = vec![0xAAu8; 64 * 1024];
        let mut sent = 0usize;
        while sent <= MAX_MSG {
            pump_all(&mut [&mut p1, &mut p2], SYNC_INTERVAL_US + 1000);
            let conn = &mut p1.clients.get_mut(&2).unwrap().conn;
            let n = conn.write(big_sid, &big_chunk).unwrap();
            if n > 0 {
                sent += n;
            }
        }
        p1.clients
            .get_mut(&2)
            .unwrap()
            .conn
            .finish(big_sid)
            .unwrap();
        pump_all(&mut [&mut p1, &mut p2], SYNC_INTERVAL_US + 2000);
        // Connection remains healthy after bad and oversize streams.
        assert_eq!(p2.connected(), vec![1]);
    }
}
