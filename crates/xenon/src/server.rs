//! Xenon server endpoint managing multiple connections and half-open limits.

use crate::conn::Connection;
use crate::crypto;
use crate::wire::{Header, Params, HEADER_LEN, PKT_DATA, PKT_INITIAL, VERSION};
use crate::{Config, MAX_DATAGRAM};
use sphinx::rng::ChaChaRng;
use sphinx::x25519;
use std::collections::{BTreeMap, VecDeque};

/// Server-side endpoint accepting Xenon connections.
pub struct Server {
    secret: [u8; 32],
    public_key: [u8; 32],
    rng: ChaChaRng,
    config: Config,
    connections: BTreeMap<u64, Connection>,
    accept_queue: VecDeque<u64>,
}

impl Server {
    /// Create a new server with static X25519 `secret`, RNG `seed`, and `config`.
    pub fn new(secret: [u8; 32], seed: [u8; 32], config: Config) -> Server {
        Server {
            secret,
            public_key: x25519::public_key(&secret),
            rng: ChaChaRng::from_seed(seed),
            config,
            connections: BTreeMap::new(),
            accept_queue: VecDeque::new(),
        }
    }

    /// The server's static X25519 public key.
    pub fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    /// Feed an incoming datagram at time `now` (microseconds); returns the `cid` or `None` if dropped.
    pub fn handle(&mut self, datagram: &[u8], now: u64) -> Option<u64> {
        if datagram.len() < HEADER_LEN || datagram.len() > MAX_DATAGRAM {
            return None;
        }
        let hdr = Header::decode(datagram)?;
        if hdr.version != VERSION {
            return None;
        }

        match hdr.pkt_type {
            PKT_INITIAL => self.handle_initial(hdr, datagram, now),
            PKT_DATA => self.handle_data(hdr.cid, datagram, now),
            _ => None,
        }
    }

    fn handle_initial(&mut self, hdr: Header, datagram: &[u8], now: u64) -> Option<u64> {
        if datagram.len() != MAX_DATAGRAM || hdr.pn != 0 {
            return None;
        }
        if let Some(conn) = self.connections.get_mut(&hdr.cid) {
            if conn.matches_known_initial(datagram) {
                conn.requeue_response();
                return Some(hdr.cid);
            }
            return None;
        }

        let half_open = self
            .connections
            .values()
            .filter(|c| c.is_half_open())
            .count();
        if half_open >= self.config.max_half_open {
            return None;
        }

        let opened = crypto::open_initial(&self.secret, &self.public_key, datagram)?;
        let mut e_s = [0u8; 32];
        self.rng.fill(&mut e_s);
        let local_params = Params::from_config(&self.config);
        let (keys, resp) = crypto::build_response(&opened, datagram, e_s, local_params)?;

        let conn = Connection::new_server_half_open(
            opened.cid,
            self.config,
            opened.peer_params,
            keys,
            datagram.to_vec(),
            resp,
            now,
        );
        self.connections.insert(opened.cid, conn);
        Some(opened.cid)
    }

    fn handle_data(&mut self, cid: u64, datagram: &[u8], now: u64) -> Option<u64> {
        let conn = self.connections.get_mut(&cid)?;
        let was_half_open = conn.is_half_open();
        if !conn.handle_checked(datagram, now) {
            return None;
        }
        if was_half_open && conn.is_connected() {
            self.accept_queue.push_back(cid);
        }
        Some(cid)
    }

    /// Borrow a connection by `cid`.
    pub fn connection(&mut self, cid: u64) -> Option<&mut Connection> {
        self.connections.get_mut(&cid)
    }

    /// Pop the next newly connected `cid`, each once.
    pub fn accept(&mut self) -> Option<u64> {
        self.accept_queue.pop_front()
    }

    /// Live connection IDs in ascending order.
    pub fn cids(&self) -> Vec<u64> {
        self.connections
            .iter()
            .filter(|(_, c)| !c.is_closed())
            .map(|(&cid, _)| cid)
            .collect()
    }

    /// Earliest timer across all live connections, if any.
    pub fn timeout(&self) -> Option<u64> {
        self.connections
            .values()
            .filter_map(Connection::timeout)
            .min()
    }

    /// Run due timers across all connections and remove closed or expired half-open ones.
    pub fn on_timeout(&mut self, now: u64) {
        for conn in self.connections.values_mut() {
            conn.on_timeout(now);
        }
        self.connections.retain(|_, c| !c.is_closed());
    }
}
