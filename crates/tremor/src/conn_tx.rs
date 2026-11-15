//! Tremor outgoing packet construction and frame packing.

use crate::conn::{Connection, Phase, HANDSHAKE_PROBE_US};
use crate::crypto;
use crate::recovery::{SentItem, SentPacket, MAX_PN};
use crate::wire::{Frame, HEADER_LEN, TAG_LEN};
use crate::{Error, MAX_DATAGRAM};

impl Connection {
    pub(crate) fn poll_transmit_inner(&mut self, now: u64) -> Option<Vec<u8>> {
        match &mut self.phase {
            Phase::ClientHandshake {
                hs,
                initial_queued,
                initial_sends,
                probe_deadline,
            } => {
                if !*initial_queued {
                    return None;
                }
                *initial_queued = false;
                let shift = (*initial_sends).min(10);
                *initial_sends += 1;
                *probe_deadline = Some(now.saturating_add(HANDSHAKE_PROBE_US << shift));
                self.sent_packets += 1;
                Some(hs.initial_datagram.clone())
            }
            Phase::ServerHalfOpen {
                response_datagram,
                response_queued,
                ..
            } => {
                if !*response_queued {
                    return None;
                }
                *response_queued = false;
                self.sent_packets += 1;
                Some(response_datagram.clone())
            }
            Phase::Draining {
                close_code,
                close_reason,
                close_queued,
                drain_until,
                armed_on_send,
            } => {
                if !*close_queued {
                    return None;
                }
                *close_queued = false;
                if !*armed_on_send {
                    *drain_until = now.saturating_add(self.recovery.base_pto().saturating_mul(3));
                    *armed_on_send = true;
                }
                if self.next_pn >= MAX_PN {
                    self.phase = Phase::Closed;
                    return None;
                }
                let keys = self.keys?;
                let send_key = if self.is_server {
                    &keys.k_s2c
                } else {
                    &keys.k_c2s
                };
                let mut pt = Vec::new();
                Frame::Close {
                    code: *close_code,
                    reason: close_reason.clone(),
                }
                .encode(&mut pt);
                pt.truncate(MAX_DATAGRAM - HEADER_LEN - TAG_LEN);
                let pn = self.next_pn;
                self.next_pn += 1;
                self.sent_packets += 1;
                Some(crypto::seal_data(send_key, self.cid, pn, &pt))
            }
            Phase::Connected {
                response_datagram,
                response_queued,
                ..
            } => {
                if *response_queued {
                    if let Some(resp) = response_datagram.clone() {
                        *response_queued = false;
                        self.sent_packets += 1;
                        return Some(resp);
                    }
                }
                self.build_data_packet(now)
            }
            Phase::Closed => None,
        }
    }

    fn build_data_packet(&mut self, now: u64) -> Option<Vec<u8>> {
        if self.next_pn >= MAX_PN {
            self.error = Some(Error::Protocol("packet number limit"));
            self.phase = Phase::Closed;
            return None;
        }
        let keys = self.keys?;
        let send_key = if self.is_server {
            &keys.k_s2c
        } else {
            &keys.k_c2s
        };

        if self.pto_probe_pending > 0 && !self.has_pending_ack_eliciting() {
            let probe_items = self.recovery.oldest_in_flight_items();
            self.requeue_items(probe_items);
        }

        let is_pto = self.pto_probe_pending > 0;
        let window_room = self
            .recovery
            .cwnd
            .saturating_sub(self.recovery.bytes_in_flight)
            .min(MAX_DATAGRAM as u64) as usize;
        let can_ack_elicit = is_pto || window_room >= 64;
        let need_ack = self.ack_immediate
            || self.ack_eliciting_unacked >= 2
            || self.ack_deadline.is_some_and(|d| now >= d);
        let has_work = self.has_pending_ack_eliciting() || is_pto;

        if !(can_ack_elicit && has_work) && !need_ack {
            return None;
        }

        let max_pt = MAX_DATAGRAM.saturating_sub(HEADER_LEN + TAG_LEN);
        let elicit_pt = if is_pto {
            if self.recovery.bytes_in_flight <= self.recovery.cwnd {
                max_pt
            } else {
                let probe_room = self
                    .recovery
                    .cwnd
                    .saturating_add(MAX_DATAGRAM as u64)
                    .saturating_sub(self.recovery.bytes_in_flight)
                    .min(MAX_DATAGRAM as u64) as usize;
                max_pt.min(probe_room.saturating_sub(HEADER_LEN + TAG_LEN))
            }
        } else {
            max_pt.min(window_room.saturating_sub(HEADER_LEN + TAG_LEN))
        };

        let mut pt = Vec::new();
        let mut items = Vec::new();
        let mut sent_ping = false;
        let ack_buf = self.encode_pending_ack(now);

        if need_ack {
            if let Some(ref abuf) = ack_buf {
                if abuf.len() <= max_pt {
                    pt.extend_from_slice(abuf);
                    self.clear_pending_ack();
                }
            }
            if can_ack_elicit && has_work {
                self.pack_ack_eliciting_frames(elicit_pt, &mut pt, &mut items);
                if items.is_empty() && is_pto && pt.len() < max_pt {
                    Frame::Ping.encode(&mut pt);
                    sent_ping = true;
                }
            }
        } else {
            let reserve = ack_buf.as_ref().map_or(0, |abuf| {
                if abuf.len() + 32 <= elicit_pt {
                    abuf.len()
                } else {
                    0
                }
            });
            self.pack_ack_eliciting_frames(elicit_pt - reserve, &mut pt, &mut items);
            if items.is_empty() && is_pto && pt.len() < max_pt {
                Frame::Ping.encode(&mut pt);
                sent_ping = true;
            }
            if pt.is_empty() {
                return None;
            }
            if let Some(abuf) = ack_buf {
                if pt.len() + abuf.len() <= elicit_pt {
                    pt.extend_from_slice(&abuf);
                    self.clear_pending_ack();
                }
            }
        }

        if pt.is_empty() {
            return None;
        }

        let ack_eliciting = !items.is_empty() || sent_ping;
        let pn = self.next_pn;
        self.next_pn += 1;
        self.sent_packets += 1;
        let datagram = crypto::seal_data(send_key, self.cid, pn, &pt);

        if ack_eliciting {
            self.recovery.on_packet_sent(SentPacket {
                pn,
                sent_at: now,
                bytes: datagram.len() as u64,
                items,
            });
            if self.pto_probe_pending > 0 {
                self.pto_probe_pending -= 1;
            }
            if !self.sent_since_recv {
                self.idle_deadline = Some(now.saturating_add(self.idle_timeout_us()));
                self.sent_since_recv = true;
            }
        }
        Some(datagram)
    }

    fn encode_pending_ack(&self, now: u64) -> Option<Vec<u8>> {
        let (largest, ranges) = self.recv_window.ack_ranges()?;
        let mut fbuf = Vec::new();
        Frame::Ack {
            largest,
            delay_us: now.saturating_sub(self.largest_recv_at),
            ranges,
        }
        .encode(&mut fbuf);
        Some(fbuf)
    }

    fn clear_pending_ack(&mut self) {
        self.ack_immediate = false;
        self.ack_eliciting_unacked = 0;
        self.ack_deadline = None;
    }

    fn has_pending_ack_eliciting(&self) -> bool {
        if self.need_max_data {
            return true;
        }
        let conn_avail = self.peer_conn_max_data.saturating_sub(self.conn_sent_total);
        self.streams.values().any(|s| {
            s.need_max_stream_data
                || s.need_send_reset
                || !s.lost_chunks.is_empty()
                || (s.send_reset.is_none()
                    && ((s.send_next < s.write_pos()
                        && s.send_next < s.peer_max_data
                        && conn_avail > 0)
                        || (s.fin_offset == Some(s.send_next) && !s.fin_sent)))
        })
    }

    fn pack_ack_eliciting_frames(
        &mut self,
        max_pt: usize,
        pt: &mut Vec<u8>,
        items: &mut Vec<SentItem>,
    ) {
        if self.need_max_data {
            let mut fbuf = Vec::new();
            Frame::MaxData(self.local_conn_max_data).encode(&mut fbuf);
            if pt.len() + fbuf.len() <= max_pt {
                pt.extend_from_slice(&fbuf);
                items.push(SentItem::MaxData(self.local_conn_max_data));
                self.need_max_data = false;
            }
        }

        let ids: Vec<u64> = self.streams.keys().copied().collect();
        let count = ids.len();
        if count == 0 {
            return;
        }
        let start = self.stream_rr_cursor % count;

        for idx in 0..count {
            let id = ids[(start + idx) % count];
            self.pack_stream_control(id, max_pt, pt, items);
        }
        for idx in 0..count {
            let id = ids[(start + idx) % count];
            self.pack_stream_retransmits(id, max_pt, pt, items);
        }
        for idx in 0..count {
            let id = ids[(start + idx) % count];
            self.pack_stream_new_data(id, max_pt, pt, items);
        }
        self.stream_rr_cursor = (start + 1) % count;
    }

    fn pack_stream_control(
        &mut self,
        id: u64,
        max_pt: usize,
        pt: &mut Vec<u8>,
        items: &mut Vec<SentItem>,
    ) {
        let Some(s) = self.streams.get_mut(&id) else {
            return;
        };
        if s.need_max_stream_data {
            let mut fbuf = Vec::new();
            Frame::MaxStreamData {
                id,
                limit: s.local_max_data,
            }
            .encode(&mut fbuf);
            if pt.len() + fbuf.len() <= max_pt {
                pt.extend_from_slice(&fbuf);
                items.push(SentItem::MaxStreamData {
                    id,
                    limit: s.local_max_data,
                });
                s.need_max_stream_data = false;
            }
        }
        if s.need_send_reset {
            if let Some(code) = s.send_reset {
                let mut fbuf = Vec::new();
                Frame::ResetStream {
                    id,
                    code,
                    final_size: s.send_next,
                }
                .encode(&mut fbuf);
                if pt.len() + fbuf.len() <= max_pt {
                    pt.extend_from_slice(&fbuf);
                    items.push(SentItem::ResetStream(id));
                    s.need_send_reset = false;
                }
            }
        }
    }

    fn pack_stream_retransmits(
        &mut self,
        id: u64,
        max_pt: usize,
        pt: &mut Vec<u8>,
        items: &mut Vec<SentItem>,
    ) {
        let Some(s) = self.streams.get_mut(&id) else {
            return;
        };
        if s.send_reset.is_some() {
            s.lost_chunks.clear();
            return;
        }
        while let Some((off, len, fin)) = s.lost_chunks.pop_front() {
            let (actual_off, data, actual_fin) = s.slice_for_retransmit(off, len, fin);
            if data.is_empty() && (!actual_fin || s.fin_acked) {
                continue;
            }
            let rem = max_pt.saturating_sub(pt.len());
            let hdr0 = Frame::stream_header_len(id, actual_off, 0);
            if rem < hdr0 || (rem == hdr0 && !data.is_empty()) {
                s.lost_chunks
                    .push_front((actual_off, data.len(), actual_fin));
                break;
            }
            let mut budget = rem - hdr0;
            if budget >= 128 {
                budget -= 1;
            }
            if data.len() <= budget {
                let dlen = data.len();
                Frame::Stream {
                    id,
                    offset: actual_off,
                    fin: actual_fin,
                    data,
                }
                .encode(pt);
                items.push(SentItem::Stream {
                    id,
                    offset: actual_off,
                    len: dlen,
                    fin: actual_fin,
                });
            } else {
                let split = budget;
                let head = data[..split].to_vec();
                let tail_off = actual_off + split as u64;
                let tail_len = data.len() - split;
                s.lost_chunks.push_front((tail_off, tail_len, actual_fin));
                Frame::Stream {
                    id,
                    offset: actual_off,
                    fin: false,
                    data: head,
                }
                .encode(pt);
                items.push(SentItem::Stream {
                    id,
                    offset: actual_off,
                    len: split,
                    fin: false,
                });
                break;
            }
        }
    }

    fn pack_stream_new_data(
        &mut self,
        id: u64,
        max_pt: usize,
        pt: &mut Vec<u8>,
        items: &mut Vec<SentItem>,
    ) {
        let conn_avail = self.peer_conn_max_data.saturating_sub(self.conn_sent_total);
        let Some(s) = self.streams.get_mut(&id) else {
            return;
        };
        if s.send_reset.is_some() {
            return;
        }
        let unsent = s.write_pos().saturating_sub(s.send_next);
        let stream_avail = s.peer_max_data.saturating_sub(s.send_next);
        let allowed = unsent.min(stream_avail).min(conn_avail);
        let fin_ready = s.fin_offset == Some(s.send_next) && !s.fin_sent;
        if allowed == 0 && !fin_ready {
            return;
        }
        let rem = max_pt.saturating_sub(pt.len());
        let hdr0 = Frame::stream_header_len(id, s.send_next, 0);
        if rem < hdr0 || (rem == hdr0 && allowed > 0) {
            return;
        }
        let mut budget = rem - hdr0;
        if budget >= 128 {
            budget -= 1;
        }
        let take = (allowed as usize).min(budget);
        let (off, data, fin) = s.take_new_send(take);
        let dlen = data.len();
        self.conn_sent_total = self.conn_sent_total.saturating_add(dlen as u64);
        Frame::Stream {
            id,
            offset: off,
            fin,
            data,
        }
        .encode(pt);
        items.push(SentItem::Stream {
            id,
            offset: off,
            len: dlen,
            fin,
        });
    }
}
