//! Xenon sans-I/O connection state machine.

use crate::crypto::{self, ClientHandshake, SessionKeys};
use crate::recovery::{LossRecovery, ReplayWindow};
use crate::stream::Stream;
use crate::wire::{self, Frame, Header, Params, HEADER_LEN, PKT_DATA, PKT_RESPONSE, VERSION};
use crate::{Config, Error, Stats, MAX_DATAGRAM};
use sphinx::rng::ChaChaRng;
use std::collections::{BTreeMap, VecDeque};

pub(crate) const HANDSHAKE_PROBE_US: u64 = 300_000;
pub(crate) const MAX_INITIAL_TRIES: u32 = 6;
pub(crate) const HALF_OPEN_TIMEOUT_US: u64 = 10_000_000;

#[derive(Debug, Clone)]
pub(crate) enum Phase {
    ClientHandshake {
        hs: ClientHandshake,
        initial_queued: bool,
        initial_sends: u32,
        probe_deadline: Option<u64>,
    },
    ServerHalfOpen {
        initial_datagram: Vec<u8>,
        response_datagram: Vec<u8>,
        response_queued: bool,
        half_open_deadline: u64,
    },
    Connected {
        initial_datagram: Option<Vec<u8>>,
        response_datagram: Option<Vec<u8>>,
        response_queued: bool,
    },
    Draining {
        close_code: u64,
        close_reason: String,
        close_queued: bool,
        drain_until: u64,
        armed_on_send: bool,
    },
    Closed,
}

/// A Xenon transport connection (sans-I/O state machine).
#[derive(Debug, Clone)]
pub struct Connection {
    pub(crate) is_server: bool,
    pub(crate) cid: u64,
    pub(crate) local_cfg: Config,
    pub(crate) peer_params: Params,
    pub(crate) keys: Option<SessionKeys>,
    pub(crate) phase: Phase,
    pub(crate) error: Option<Error>,
    pub(crate) last_now: u64,

    // Packet numbers & recovery
    pub(crate) next_pn: u64,
    pub(crate) recv_window: ReplayWindow,
    pub(crate) recovery: LossRecovery,
    pub(crate) pto_probe_pending: u32,

    // Delayed ACK & idle timers
    pub(crate) ack_eliciting_unacked: u32,
    pub(crate) ack_immediate: bool,
    pub(crate) ack_deadline: Option<u64>,
    pub(crate) largest_recv_at: u64,
    pub(crate) idle_deadline: Option<u64>,
    pub(crate) sent_since_recv: bool,
    pub(crate) sent_packets: u64,

    // Streams & connection-level flow control
    pub(crate) streams: BTreeMap<u64, Stream>,
    pub(crate) next_local_stream_id: u64,
    pub(crate) local_streams_opened: u64,
    pub(crate) next_expected_peer_stream: u64,
    pub(crate) accept_queue: VecDeque<u64>,
    pub(crate) stream_rr_cursor: usize,

    pub(crate) peer_conn_max_data: u64,
    pub(crate) conn_sent_total: u64,
    pub(crate) local_conn_max_data: u64,
    pub(crate) acked_local_conn_max_data: u64,
    pub(crate) conn_recv_total: u64,
    pub(crate) conn_read_total: u64,
    pub(crate) need_max_data: bool,
}

impl Connection {
    /// Start a new client connection to `server_key` using `seed`, `config`, and time `now`.
    pub fn connect(server_key: [u8; 32], seed: [u8; 32], config: Config, now: u64) -> Connection {
        let mut rng = ChaChaRng::from_seed(seed);
        let mut e_c = [0u8; 32];
        rng.fill(&mut e_c);
        let cid = rng.next_u64();
        let local_params = Params::from_config(&config);

        match crypto::build_initial(e_c, cid, &server_key, local_params) {
            Some(hs) => Self::new_base(
                false,
                cid,
                config,
                now,
                Phase::ClientHandshake {
                    hs,
                    initial_queued: true,
                    initial_sends: 0,
                    probe_deadline: None,
                },
            ),
            None => {
                let mut conn = Self::new_base(false, cid, config, now, Phase::Closed);
                conn.error = Some(Error::Protocol("weak server key"));
                conn
            }
        }
    }

    pub(crate) fn new_server_half_open(
        cid: u64,
        config: Config,
        peer_params: Params,
        keys: SessionKeys,
        initial_datagram: Vec<u8>,
        response_datagram: Vec<u8>,
        now: u64,
    ) -> Self {
        let mut conn = Self::new_base(
            true,
            cid,
            config,
            now,
            Phase::ServerHalfOpen {
                initial_datagram,
                response_datagram,
                response_queued: true,
                half_open_deadline: now.saturating_add(HALF_OPEN_TIMEOUT_US),
            },
        );
        conn.keys = Some(keys);
        conn.peer_params = peer_params;
        conn.peer_conn_max_data = peer_params.conn_window;
        conn
    }

    fn new_base(is_server: bool, cid: u64, config: Config, now: u64, phase: Phase) -> Self {
        let local_stream_start = u64::from(is_server);
        let peer_stream_start = u64::from(!is_server);
        Self {
            is_server,
            cid,
            local_cfg: config,
            peer_params: Params::from_config(&Config::default()),
            keys: None,
            phase,
            error: None,
            last_now: now,
            next_pn: 1,
            recv_window: ReplayWindow::default(),
            recovery: LossRecovery::default(),
            pto_probe_pending: 0,
            ack_eliciting_unacked: 0,
            ack_immediate: false,
            ack_deadline: None,
            largest_recv_at: now,
            idle_deadline: None,
            sent_since_recv: false,
            sent_packets: 0,
            streams: BTreeMap::new(),
            next_local_stream_id: local_stream_start,
            local_streams_opened: 0,
            next_expected_peer_stream: peer_stream_start,
            accept_queue: VecDeque::new(),
            stream_rr_cursor: 0,
            peer_conn_max_data: config.conn_window,
            conn_sent_total: 0,
            local_conn_max_data: config.conn_window,
            acked_local_conn_max_data: config.conn_window,
            conn_recv_total: 0,
            conn_read_total: 0,
            need_max_data: false,
        }
    }

    /// The 64-bit connection ID.
    pub fn cid(&self) -> u64 {
        self.cid
    }

    /// Feed an incoming UDP datagram at time `now` (microseconds).
    pub fn handle(&mut self, datagram: &[u8], now: u64) {
        self.handle_checked(datagram, now);
    }

    /// Process an incoming datagram; returns `false` if dropped with zero state mutation.
    pub(crate) fn handle_checked(&mut self, datagram: &[u8], now: u64) -> bool {
        if datagram.len() < HEADER_LEN || datagram.len() > MAX_DATAGRAM {
            return false;
        }
        let Some(hdr) = Header::decode(datagram) else {
            return false;
        };
        if hdr.version != VERSION || hdr.cid != self.cid {
            return false;
        }

        match &self.phase {
            Phase::ClientHandshake { hs, .. } => {
                if hdr.pkt_type != PKT_RESPONSE || hdr.pn != 0 {
                    return false;
                }
                let Some((keys, peer_params)) = crypto::open_response(hs, self.cid, datagram)
                else {
                    return false;
                };
                self.finish_client_handshake(keys, peer_params, now);
                true
            }
            Phase::ServerHalfOpen { .. } | Phase::Connected { .. } => {
                if hdr.pkt_type != PKT_DATA || hdr.pn == 0 {
                    return false;
                }
                if self.recv_window.is_seen_or_old(hdr.pn) {
                    return false;
                }
                let Some(keys) = self.keys else {
                    return false;
                };
                let recv_key = if self.is_server {
                    &keys.k_c2s
                } else {
                    &keys.k_s2c
                };
                let Some(pt) = crypto::open_data(recv_key, datagram, hdr.pn) else {
                    return false;
                };
                if matches!(self.phase, Phase::ServerHalfOpen { .. }) {
                    self.promote_server_connected(now);
                }
                self.on_authenticated_data(hdr.pn, &pt, now);
                true
            }
            Phase::Draining { .. } => {
                if hdr.pkt_type != PKT_DATA || hdr.pn == 0 {
                    return false;
                }
                if self.recv_window.is_seen_or_old(hdr.pn) {
                    return false;
                }
                let Some(keys) = self.keys else {
                    return false;
                };
                let recv_key = if self.is_server {
                    &keys.k_c2s
                } else {
                    &keys.k_s2c
                };
                let Some(pt) = crypto::open_data(recv_key, datagram, hdr.pn) else {
                    return false;
                };
                self.recv_window.mark_seen(hdr.pn);
                self.last_now = now;
                let has_close = wire::decode_frames(&pt)
                    .is_some_and(|fs| fs.iter().any(|f| matches!(f, Frame::Close { .. })));
                if !has_close {
                    if let Phase::Draining { close_queued, .. } = &mut self.phase {
                        *close_queued = true;
                    }
                }
                true
            }
            Phase::Closed => false,
        }
    }

    fn finish_client_handshake(&mut self, keys: SessionKeys, peer_params: Params, now: u64) {
        self.keys = Some(keys);
        self.peer_params = peer_params;
        self.peer_conn_max_data = peer_params.conn_window;
        self.phase = Phase::Connected {
            initial_datagram: None,
            response_datagram: None,
            response_queued: false,
        };
        self.last_now = now;
        self.recv_window.mark_seen(0);
        self.largest_recv_at = now;
        self.ack_immediate = true;
        self.ack_eliciting_unacked = 1;
        self.reset_idle_timer(now);
    }

    fn promote_server_connected(&mut self, now: u64) {
        let old = core::mem::replace(&mut self.phase, Phase::Closed);
        if let Phase::ServerHalfOpen {
            initial_datagram,
            response_datagram,
            ..
        } = old
        {
            self.phase = Phase::Connected {
                initial_datagram: Some(initial_datagram),
                response_datagram: Some(response_datagram),
                response_queued: false,
            };
        }
        self.reset_idle_timer(now);
    }

    pub(crate) fn idle_timeout_us(&self) -> u64 {
        self.local_cfg
            .idle_timeout_ms
            .min(self.peer_params.idle_timeout_ms)
            .saturating_mul(1_000)
    }

    pub(crate) fn reset_idle_timer(&mut self, now: u64) {
        self.idle_deadline = Some(now.saturating_add(self.idle_timeout_us()));
        self.sent_since_recv = false;
    }

    /// Poll the next outgoing UDP datagram at time `now` (microseconds).
    pub fn poll_transmit(&mut self, now: u64) -> Option<Vec<u8>> {
        self.last_now = now;
        self.poll_transmit_inner(now)
    }

    /// Absolute timestamp (microseconds) of the next timer event, if any.
    pub fn timeout(&self) -> Option<u64> {
        match &self.phase {
            Phase::ClientHandshake { probe_deadline, .. } => *probe_deadline,
            Phase::ServerHalfOpen {
                half_open_deadline, ..
            } => Some(*half_open_deadline),
            Phase::Connected { .. } => [
                self.idle_deadline,
                self.ack_deadline,
                self.recovery.loss_deadline,
                self.recovery.pto_deadline,
            ]
            .into_iter()
            .flatten()
            .min(),
            Phase::Draining { drain_until, .. } => Some(*drain_until),
            Phase::Closed => None,
        }
    }

    /// True once the handshake is complete and the connection is not closed/closing.
    pub fn is_connected(&self) -> bool {
        matches!(self.phase, Phase::Connected { .. })
    }

    /// True once closed and done draining (nothing more to send).
    pub fn is_closed(&self) -> bool {
        matches!(self.phase, Phase::Closed)
    }

    /// The error that closed this connection, or `None` while open.
    pub fn error(&self) -> Option<Error> {
        self.error.clone()
    }

    /// The 32-byte handshake transcript hash `h1` once known.
    pub fn handshake_hash(&self) -> Option<[u8; 32]> {
        self.keys.map(|k| k.h1)
    }

    /// Close the connection cleanly or with an application error code and reason.
    pub fn close(&mut self, code: u64, reason: &str) {
        if self.error.is_some() {
            return;
        }
        self.enter_draining(
            Error::Closed {
                code,
                reason: reason.to_owned(),
            },
            code,
            reason,
        );
    }

    pub(crate) fn enter_draining(&mut self, err: Error, wire_code: u64, wire_reason: &str) {
        self.error = Some(err);
        if self.keys.is_some() && !matches!(self.phase, Phase::ServerHalfOpen { .. }) {
            let drain_until = self
                .last_now
                .saturating_add(self.recovery.base_pto().saturating_mul(3));
            self.phase = Phase::Draining {
                close_code: wire_code,
                close_reason: wire_reason.to_owned(),
                close_queued: true,
                drain_until,
                armed_on_send: false,
            };
        } else {
            self.phase = Phase::Closed;
        }
        self.recovery.bytes_in_flight = 0;
        self.recovery.sent.clear();
    }

    /// Current connection statistics.
    pub fn stats(&self) -> Stats {
        Stats {
            sent_packets: self.sent_packets,
            lost_packets: self.recovery.lost_packets,
            cwnd: self.recovery.cwnd,
            bytes_in_flight: self.recovery.bytes_in_flight,
            srtt_us: self.recovery.srtt_us,
        }
    }

    pub(crate) fn is_peer_stream(&self, id: u64) -> bool {
        let parity = id & 1;
        if self.is_server {
            parity == 0
        } else {
            parity == 1
        }
    }

    pub(crate) fn is_half_open(&self) -> bool {
        matches!(self.phase, Phase::ServerHalfOpen { .. })
    }

    pub(crate) fn matches_known_initial(&self, datagram: &[u8]) -> bool {
        match &self.phase {
            Phase::ServerHalfOpen {
                initial_datagram, ..
            } => initial_datagram.as_slice() == datagram,
            Phase::Connected {
                initial_datagram: Some(init),
                ..
            } => init.as_slice() == datagram,
            _ => false,
        }
    }

    pub(crate) fn requeue_response(&mut self) {
        match &mut self.phase {
            Phase::ServerHalfOpen {
                response_queued, ..
            } => *response_queued = true,
            Phase::Connected {
                response_datagram: Some(_),
                response_queued,
                ..
            } => *response_queued = true,
            _ => {}
        }
    }
}
