//! Xenon connection frame handling, timers, and stream API.

use crate::conn::{Connection, Phase, MAX_INITIAL_TRIES};
use crate::recovery::{RecoveryEvents, SentItem, MAX_ACK_DELAY_US};
use crate::stream::Stream;
use crate::wire::{self, Frame};
use crate::Error;

impl Connection {
    /// Open a new outgoing bidirectional stream.
    pub fn open_stream(&mut self) -> Result<u64, Error> {
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        if !self.is_connected() {
            return Err(Error::Protocol("not connected"));
        }
        if self.local_streams_opened >= self.peer_params.max_streams {
            return Err(Error::StreamLimit);
        }
        let id = self.next_local_stream_id;
        self.next_local_stream_id = self.next_local_stream_id.saturating_add(2);
        self.local_streams_opened += 1;
        self.streams.insert(
            id,
            Stream::new(self.local_cfg.stream_window, self.peer_params.stream_window),
        );
        Ok(id)
    }

    /// Pop the next stream opened by the peer, in stream-ID order.
    pub fn accept_stream(&mut self) -> Option<u64> {
        self.accept_queue.pop_front()
    }

    /// Write bytes to stream `id`, returning the number of bytes accepted.
    pub fn write(&mut self, id: u64, data: &[u8]) -> Result<usize, Error> {
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        if !self.is_connected() {
            return Err(Error::Protocol("not connected"));
        }
        let Some(stream) = self.streams.get_mut(&id) else {
            return Err(Error::Protocol("unknown stream"));
        };
        stream.write(data)
    }

    /// Mark the send side of stream `id` finished (FIN will be sent after buffered bytes).
    pub fn finish(&mut self, id: u64) -> Result<(), Error> {
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        if !self.is_connected() {
            return Err(Error::Protocol("not connected"));
        }
        let Some(stream) = self.streams.get_mut(&id) else {
            return Err(Error::Protocol("unknown stream"));
        };
        stream.finish()
    }

    /// Read available contiguous bytes from stream `id` into `buf`.
    pub fn read(&mut self, id: u64, buf: &mut [u8]) -> Result<usize, Error> {
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        if !self.is_connected() {
            return Err(Error::Protocol("not connected"));
        }
        let Some(stream) = self.streams.get_mut(&id) else {
            return Err(Error::Protocol("unknown stream"));
        };
        let n = stream.read(buf)?;
        self.conn_read_total = self.conn_read_total.saturating_add(n as u64);
        if self
            .local_conn_max_data
            .saturating_sub(self.conn_read_total)
            <= self.local_cfg.conn_window / 2
        {
            let new_limit = self
                .conn_read_total
                .saturating_add(self.local_cfg.conn_window);
            if new_limit > self.local_conn_max_data {
                self.local_conn_max_data = new_limit;
                self.need_max_data = true;
            }
        }
        Ok(n)
    }

    /// True if FIN was received on stream `id` and every byte has been read.
    pub fn stream_finished(&self, id: u64) -> bool {
        self.streams.get(&id).is_some_and(Stream::is_finished)
    }

    /// Abandon the send side of stream `id` with `code`.
    pub fn reset(&mut self, id: u64, code: u64) -> Result<(), Error> {
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        if !self.is_connected() {
            return Err(Error::Protocol("not connected"));
        }
        let Some(stream) = self.streams.get_mut(&id) else {
            return Err(Error::Protocol("unknown stream"));
        };
        stream.reset(code)
    }

    pub(crate) fn on_authenticated_data(&mut self, pn: u64, pt: &[u8], now: u64) {
        self.recv_window.mark_seen(pn);
        self.last_now = now;
        self.largest_recv_at = now;
        self.reset_idle_timer(now);

        let Some(frames) = wire::decode_frames(pt) else {
            self.enter_draining(Error::Protocol("malformed frame"), 1, "malformed frame");
            return;
        };

        if frames.iter().any(Frame::is_ack_eliciting) {
            self.ack_eliciting_unacked += 1;
            if self.ack_eliciting_unacked >= 2 {
                self.ack_immediate = true;
                self.ack_deadline = None;
            } else if self.ack_deadline.is_none() {
                self.ack_deadline = Some(now.saturating_add(MAX_ACK_DELAY_US));
            }
        }

        for frame in frames {
            if !self.apply_frame(frame, now) {
                return;
            }
        }
    }

    fn apply_frame(&mut self, frame: Frame, now: u64) -> bool {
        match frame {
            Frame::Padding | Frame::Ping => true,
            Frame::Ack {
                largest,
                delay_us,
                ranges,
            } => {
                let ev = self
                    .recovery
                    .on_ack(largest, delay_us, &ranges, now, self.next_pn);
                self.apply_recovery_events(ev);
                true
            }
            Frame::Stream {
                id,
                offset,
                fin,
                data,
            } => {
                if let Err((wcode, msg)) = self.ensure_stream_for_frame(id) {
                    self.close_wire_err(wcode, msg);
                    return false;
                }
                let Some(stream) = self.streams.get_mut(&id) else {
                    return false;
                };
                if let Err((wcode, msg)) = stream.on_recv_data(
                    offset,
                    &data,
                    fin,
                    &mut self.conn_recv_total,
                    self.local_conn_max_data,
                ) {
                    self.close_wire_err(wcode, msg);
                    return false;
                }
                true
            }
            Frame::MaxData(limit) => {
                if limit > self.peer_conn_max_data {
                    self.peer_conn_max_data = limit;
                }
                true
            }
            Frame::MaxStreamData { id, limit } => {
                if let Err((wcode, msg)) = self.ensure_stream_for_frame(id) {
                    self.close_wire_err(wcode, msg);
                    return false;
                }
                if let Some(s) = self.streams.get_mut(&id) {
                    if limit > s.peer_max_data {
                        s.peer_max_data = limit;
                    }
                }
                true
            }
            Frame::ResetStream {
                id,
                code,
                final_size,
            } => {
                if let Err((wcode, msg)) = self.ensure_stream_for_frame(id) {
                    self.close_wire_err(wcode, msg);
                    return false;
                }
                let Some(stream) = self.streams.get_mut(&id) else {
                    return false;
                };
                if let Err((wcode, msg)) = stream.on_recv_reset(
                    code,
                    final_size,
                    &mut self.conn_recv_total,
                    self.local_conn_max_data,
                ) {
                    self.close_wire_err(wcode, msg);
                    return false;
                }
                true
            }
            Frame::Close { code, reason } => {
                self.error = Some(Error::Closed { code, reason });
                self.phase = Phase::Closed;
                self.recovery.bytes_in_flight = 0;
                self.recovery.sent.clear();
                false
            }
        }
    }

    fn ensure_stream_for_frame(&mut self, id: u64) -> Result<(), (u64, &'static str)> {
        if self.is_peer_stream(id) {
            if id / 2 >= self.local_cfg.max_streams {
                return Err((3, "stream limit exceeded"));
            }
            while self.next_expected_peer_stream <= id {
                let sid = self.next_expected_peer_stream;
                self.next_expected_peer_stream = self.next_expected_peer_stream.saturating_add(2);
                self.streams.insert(
                    sid,
                    Stream::new(self.local_cfg.stream_window, self.peer_params.stream_window),
                );
                self.accept_queue.push_back(sid);
            }
            Ok(())
        } else if self.streams.contains_key(&id) {
            Ok(())
        } else {
            Err((1, "unknown stream"))
        }
    }

    fn close_wire_err(&mut self, wire_code: u64, msg: &'static str) {
        let err = match wire_code {
            2 => Error::FlowControl,
            3 => Error::StreamLimit,
            _ => Error::Protocol(msg),
        };
        self.enter_draining(err, wire_code, msg);
    }

    pub(crate) fn run_loss_detection(&mut self, now: u64) {
        let mut ev = RecoveryEvents::default();
        self.recovery.detect_losses(now, self.next_pn, &mut ev);
        self.apply_recovery_events(ev);
    }

    fn apply_recovery_events(&mut self, ev: RecoveryEvents) {
        if ev.any_acked {
            self.pto_probe_pending = 0;
        }
        for item in ev.acked_items {
            match item {
                SentItem::Stream {
                    id,
                    offset,
                    len,
                    fin,
                } => {
                    if let Some(s) = self.streams.get_mut(&id) {
                        s.mark_send_acked(offset, len, fin);
                    }
                }
                SentItem::MaxData(lim) => {
                    if lim > self.acked_local_conn_max_data {
                        self.acked_local_conn_max_data = lim;
                    }
                }
                SentItem::MaxStreamData { id, limit } => {
                    if let Some(s) = self.streams.get_mut(&id) {
                        if limit > s.acked_local_max_data {
                            s.acked_local_max_data = limit;
                        }
                    }
                }
                SentItem::ResetStream(id) => {
                    if let Some(s) = self.streams.get_mut(&id) {
                        s.reset_acked = true;
                    }
                }
            }
        }
        self.requeue_items(ev.lost_items);
    }

    pub(crate) fn requeue_items(&mut self, items: Vec<SentItem>) {
        for item in items {
            match item {
                SentItem::Stream {
                    id,
                    offset,
                    len,
                    fin,
                } => {
                    if let Some(s) = self.streams.get_mut(&id) {
                        if s.send_reset.is_none() {
                            s.lost_chunks.push_back((offset, len, fin));
                        }
                    }
                }
                SentItem::MaxData(_) => {
                    if self.local_conn_max_data > self.acked_local_conn_max_data {
                        self.need_max_data = true;
                    }
                }
                SentItem::MaxStreamData { id, .. } => {
                    if let Some(s) = self.streams.get_mut(&id) {
                        if s.local_max_data > s.acked_local_max_data {
                            s.need_max_stream_data = true;
                        }
                    }
                }
                SentItem::ResetStream(id) => {
                    if let Some(s) = self.streams.get_mut(&id) {
                        if !s.reset_acked {
                            s.need_send_reset = true;
                        }
                    }
                }
            }
        }
    }

    /// Run due timers at time `now` (microseconds).
    pub fn on_timeout(&mut self, now: u64) {
        self.last_now = now;
        match &mut self.phase {
            Phase::ClientHandshake {
                initial_queued,
                initial_sends,
                probe_deadline,
                ..
            } => {
                if let Some(d) = *probe_deadline {
                    if now >= d {
                        if *initial_sends >= MAX_INITIAL_TRIES {
                            self.error = Some(Error::Timeout);
                            self.phase = Phase::Closed;
                        } else {
                            *initial_queued = true;
                            *probe_deadline = None;
                        }
                    }
                }
            }
            Phase::ServerHalfOpen {
                half_open_deadline, ..
            } => {
                if now >= *half_open_deadline {
                    self.error = Some(Error::Timeout);
                    self.phase = Phase::Closed;
                }
            }
            Phase::Connected { .. } => {
                if self.idle_deadline.is_some_and(|d| now >= d) {
                    self.error = Some(Error::Timeout);
                    self.phase = Phase::Closed;
                    self.recovery.bytes_in_flight = 0;
                    self.recovery.sent.clear();
                    return;
                }
                if self.ack_deadline.is_some_and(|d| now >= d) {
                    self.ack_immediate = true;
                    self.ack_deadline = None;
                }
                if self.recovery.loss_deadline.is_some_and(|d| now >= d) {
                    self.run_loss_detection(now);
                }
                if self.recovery.pto_deadline.is_some_and(|d| now >= d) {
                    if !self.recovery.sent.is_empty() {
                        self.recovery.pto_count = self.recovery.pto_count.saturating_add(1);
                        self.pto_probe_pending = 2;
                        self.recovery.pto_deadline =
                            Some(now.saturating_add(self.recovery.current_pto()));
                    } else {
                        self.recovery.pto_deadline = None;
                    }
                }
            }
            Phase::Draining { drain_until, .. } => {
                if now >= *drain_until {
                    self.phase = Phase::Closed;
                    self.recovery.bytes_in_flight = 0;
                    self.recovery.sent.clear();
                }
            }
            Phase::Closed => {}
        }
    }
}
