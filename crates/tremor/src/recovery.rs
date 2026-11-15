//! Anti-replay window, RTT/loss detection, ACK range building, and NewReno congestion control.

use crate::wire::MAX_ACK_RANGES;
use std::collections::BTreeMap;

pub(crate) const INITIAL_CWND: u64 = 12_000;
pub(crate) const MIN_CWND: u64 = 2_400;
pub(crate) const INITIAL_SRTT_US: u64 = 100_000;
pub(crate) const INITIAL_RTTVAR_US: u64 = 50_000;
pub(crate) const MAX_ACK_DELAY_US: u64 = 25_000;
pub(crate) const MAX_PN: u64 = 1u64 << 62;

/// 1024-packet sliding anti-replay window.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReplayWindow {
    largest: Option<u64>,
    bitmap: [u64; 16],
}

impl ReplayWindow {
    /// Returns true if `pn` is out of range, already seen, or older than the 1024 most recent.
    pub(crate) fn is_seen_or_old(&self, pn: u64) -> bool {
        if pn >= MAX_PN {
            return true;
        }
        let Some(max) = self.largest else {
            return false;
        };
        if pn > max {
            return false;
        }
        let diff = max - pn;
        if diff >= 1024 {
            return true;
        }
        let word = (diff / 64) as usize;
        let bit = diff % 64;
        self.bitmap.get(word).is_some_and(|w| (w >> bit) & 1 != 0)
    }

    /// Record `pn` as received.
    pub(crate) fn mark_seen(&mut self, pn: u64) {
        let Some(max) = self.largest else {
            self.largest = Some(pn);
            self.bitmap[0] = 1;
            return;
        };
        if pn > max {
            let shift = pn - max;
            self.shift_left(shift);
            self.bitmap[0] |= 1;
            self.largest = Some(pn);
        } else {
            let diff = max - pn;
            if diff < 1024 {
                let word = (diff / 64) as usize;
                let bit = diff % 64;
                if let Some(slot) = self.bitmap.get_mut(word) {
                    *slot |= 1u64 << bit;
                }
            }
        }
    }

    fn shift_left(&mut self, shift: u64) {
        if shift >= 1024 {
            self.bitmap = [0; 16];
            return;
        }
        let word_shift = (shift / 64) as usize;
        let bit_shift = (shift % 64) as u32;
        let mut next = [0u64; 16];
        for (i, slot) in next.iter_mut().enumerate().skip(word_shift) {
            let mut val = self.bitmap[i - word_shift] << bit_shift;
            if bit_shift > 0 && i > word_shift {
                val |= self.bitmap[i - word_shift - 1] >> (64 - bit_shift);
            }
            *slot = val;
        }
        self.bitmap = next;
    }

    /// Build up to 32 descending inclusive ACK ranges `(start, end)`.
    pub(crate) fn ack_ranges(&self) -> Option<(u64, Vec<(u64, u64)>)> {
        let max = self.largest?;
        let mut ranges = Vec::new();
        let limit = max.min(1023);
        let mut cur_end: Option<u64> = None;
        let mut cur_start: u64 = 0;

        for d in 0..=limit {
            let word = (d / 64) as usize;
            let bit = d % 64;
            let seen = (self.bitmap[word] >> bit) & 1 != 0;
            let pn = max - d;
            if seen {
                if cur_end.is_none() {
                    cur_end = Some(pn);
                }
                cur_start = pn;
            } else if let Some(end) = cur_end.take() {
                ranges.push((cur_start, end));
                if ranges.len() >= MAX_ACK_RANGES {
                    break;
                }
            }
        }
        if let Some(end) = cur_end {
            if ranges.len() < MAX_ACK_RANGES {
                ranges.push((cur_start, end));
            }
        }
        if ranges.is_empty() {
            None
        } else {
            Some((max, ranges))
        }
    }
}

/// Items carried in a sent ack-eliciting Data packet for loss recovery.
#[derive(Debug, Clone)]
pub(crate) enum SentItem {
    Stream {
        id: u64,
        offset: u64,
        len: usize,
        fin: bool,
    },
    MaxData(u64),
    MaxStreamData {
        id: u64,
        limit: u64,
    },
    ResetStream(u64),
}

/// Metadata for one unacknowledged ack-eliciting packet.
#[derive(Debug, Clone)]
pub(crate) struct SentPacket {
    pub(crate) pn: u64,
    pub(crate) sent_at: u64,
    pub(crate) bytes: u64,
    pub(crate) items: Vec<SentItem>,
}

/// Outcome of processing an ACK or loss-timer tick.
#[derive(Debug, Default)]
pub(crate) struct RecoveryEvents {
    pub(crate) acked_items: Vec<SentItem>,
    pub(crate) lost_items: Vec<SentItem>,
    pub(crate) lost_count: u64,
    pub(crate) any_acked: bool,
}

/// RTT estimator, loss detector, PTO state, and NewReno congestion controller.
#[derive(Debug, Clone)]
pub(crate) struct LossRecovery {
    pub(crate) srtt_us: u64,
    pub(crate) rttvar_us: u64,
    pub(crate) min_rtt_us: u64,
    pub(crate) has_rtt_sample: bool,
    pub(crate) latest_rtt_us: u64,
    pub(crate) cwnd: u64,
    pub(crate) ssthresh: u64,
    pub(crate) ca_bytes: u64,
    pub(crate) bytes_in_flight: u64,
    pub(crate) recovery_end_pn: Option<u64>,
    pub(crate) pto_count: u32,
    pub(crate) largest_acked: Option<u64>,
    pub(crate) loss_deadline: Option<u64>,
    pub(crate) pto_deadline: Option<u64>,
    pub(crate) lost_packets: u64,
    pub(crate) sent: BTreeMap<u64, SentPacket>,
    consecutive_loss_span: Option<(u64, u64)>,
}

impl Default for LossRecovery {
    fn default() -> Self {
        Self {
            srtt_us: INITIAL_SRTT_US,
            rttvar_us: INITIAL_RTTVAR_US,
            min_rtt_us: u64::MAX,
            has_rtt_sample: false,
            latest_rtt_us: INITIAL_SRTT_US,
            cwnd: INITIAL_CWND,
            ssthresh: u64::MAX,
            ca_bytes: 0,
            bytes_in_flight: 0,
            recovery_end_pn: None,
            pto_count: 0,
            largest_acked: None,
            loss_deadline: None,
            pto_deadline: None,
            lost_packets: 0,
            sent: BTreeMap::new(),
            consecutive_loss_span: None,
        }
    }
}

impl LossRecovery {
    /// Base PTO = srtt + max(4 * rttvar, 1 ms) + 25 ms.
    pub(crate) fn base_pto(&self) -> u64 {
        self.srtt_us
            .saturating_add((4 * self.rttvar_us).max(1_000))
            .saturating_add(MAX_ACK_DELAY_US)
    }

    /// Current backed-off PTO duration.
    pub(crate) fn current_pto(&self) -> u64 {
        self.base_pto() << self.pto_count.min(10)
    }

    /// Record a newly transmitted ack-eliciting Data packet.
    pub(crate) fn on_packet_sent(&mut self, pkt: SentPacket) {
        self.bytes_in_flight = self.bytes_in_flight.saturating_add(pkt.bytes);
        let sent_at = pkt.sent_at;
        let pn = pkt.pn;
        self.sent.insert(pn, pkt);
        self.pto_deadline = Some(sent_at.saturating_add(self.current_pto()));
    }

    /// Process an incoming ACK frame at `now`.
    pub(crate) fn on_ack(
        &mut self,
        largest: u64,
        ack_delay_us: u64,
        ranges: &[(u64, u64)],
        now: u64,
        next_pn: u64,
    ) -> RecoveryEvents {
        let mut events = RecoveryEvents::default();
        let mut newly_acked = Vec::new();
        let mut largest_newly_acked: Option<SentPacket> = None;

        for &(start, end) in ranges {
            let pns: Vec<u64> = self.sent.range(start..=end).map(|(&k, _)| k).collect();
            for pn in pns {
                if let Some(pkt) = self.sent.remove(&pn) {
                    if largest_newly_acked
                        .as_ref()
                        .is_none_or(|cur| pkt.pn > cur.pn)
                    {
                        largest_newly_acked = Some(pkt.clone());
                    }
                    newly_acked.push(pkt);
                }
            }
        }

        if newly_acked.is_empty() {
            return events;
        }

        events.any_acked = true;
        self.consecutive_loss_span = None;
        self.pto_count = 0;

        if self.largest_acked.is_none_or(|cur| largest > cur) {
            self.largest_acked = Some(largest);
        }

        if let Some(ref newest) = largest_newly_acked {
            if newest.pn == largest {
                self.update_rtt(now.saturating_sub(newest.sent_at), ack_delay_us);
            }
        }

        for pkt in &newly_acked {
            self.bytes_in_flight = self.bytes_in_flight.saturating_sub(pkt.bytes);
            events.acked_items.extend(pkt.items.iter().cloned());
        }

        self.detect_losses(now, next_pn, &mut events);

        for pkt in newly_acked {
            self.on_packet_acked_cc(pkt.pn, pkt.bytes);
        }

        self.refresh_pto_timer();
        events
    }

    fn update_rtt(&mut self, latest_rtt: u64, ack_delay_us: u64) {
        self.latest_rtt_us = latest_rtt;
        if latest_rtt < self.min_rtt_us {
            self.min_rtt_us = latest_rtt;
        }
        let delay = ack_delay_us.min(MAX_ACK_DELAY_US);
        let adj = latest_rtt.saturating_sub(delay).max(self.min_rtt_us);
        if !self.has_rtt_sample {
            self.srtt_us = latest_rtt;
            self.rttvar_us = latest_rtt / 2;
            self.has_rtt_sample = true;
        } else {
            let diff = self.srtt_us.abs_diff(adj);
            self.rttvar_us = (3 * self.rttvar_us + diff) / 4;
            self.srtt_us = (7 * self.srtt_us + adj) / 8;
        }
    }

    fn on_packet_acked_cc(&mut self, pn: u64, acked_bytes: u64) {
        if let Some(end_pn) = self.recovery_end_pn {
            if pn <= end_pn {
                return;
            }
            self.recovery_end_pn = None;
        }
        if self.cwnd < self.ssthresh {
            self.cwnd = self.cwnd.saturating_add(acked_bytes);
        } else {
            self.ca_bytes = self
                .ca_bytes
                .saturating_add(1200u64.saturating_mul(acked_bytes));
            let old_cwnd = self.cwnd.max(1);
            let inc = self.ca_bytes / old_cwnd;
            if inc > 0 {
                self.cwnd = self.cwnd.saturating_add(inc);
                self.ca_bytes %= old_cwnd;
            }
        }
    }

    /// Run packet-threshold and time-threshold loss detection.
    pub(crate) fn detect_losses(&mut self, now: u64, next_pn: u64, events: &mut RecoveryEvents) {
        let Some(largest_acked) = self.largest_acked else {
            self.loss_deadline = None;
            return;
        };

        let loss_delay = ((9 * self.srtt_us.max(self.latest_rtt_us)) / 8).max(1_000);
        let lost_send_time = now.saturating_sub(loss_delay);

        let mut lost_pns = Vec::new();
        let mut earliest_pending: Option<u64> = None;

        for (&pn, pkt) in &self.sent {
            if pn >= largest_acked {
                continue;
            }
            if largest_acked - pn >= 3 || pkt.sent_at <= lost_send_time {
                lost_pns.push(pn);
            } else {
                let t = pkt.sent_at.saturating_add(loss_delay);
                earliest_pending = Some(earliest_pending.map_or(t, |cur| cur.min(t)));
            }
        }
        self.loss_deadline = earliest_pending;

        if lost_pns.is_empty() {
            return;
        }

        let mut lost_packets = Vec::with_capacity(lost_pns.len());
        for pn in lost_pns {
            if let Some(pkt) = self.sent.remove(&pn) {
                self.bytes_in_flight = self.bytes_in_flight.saturating_sub(pkt.bytes);
                self.lost_packets += 1;
                events.lost_count += 1;
                lost_packets.push(pkt);
            }
        }

        self.on_packets_lost_cc(&lost_packets, next_pn);
        for pkt in lost_packets {
            events.lost_items.extend(pkt.items);
        }
        self.refresh_pto_timer();
    }

    fn on_packets_lost_cc(&mut self, lost: &[SentPacket], next_pn: u64) {
        let pc_duration = 3 * self.base_pto();
        for pkt in lost {
            let in_recovery = self.recovery_end_pn.is_some_and(|end_pn| pkt.pn <= end_pn);
            if !in_recovery {
                self.recovery_end_pn = Some(next_pn.saturating_sub(1));
                self.cwnd = (self.cwnd / 2).max(MIN_CWND);
                self.ssthresh = self.cwnd;
                self.ca_bytes = 0;
            }
            if self.has_rtt_sample {
                let (min_t, max_t) = match self.consecutive_loss_span {
                    Some((a, b)) => (a.min(pkt.sent_at), b.max(pkt.sent_at)),
                    None => (pkt.sent_at, pkt.sent_at),
                };
                self.consecutive_loss_span = Some((min_t, max_t));
                if max_t.saturating_sub(min_t) >= pc_duration {
                    self.cwnd = MIN_CWND;
                    self.recovery_end_pn = None;
                }
            }
        }
    }

    /// Refresh PTO deadline based on the newest in-flight packet.
    pub(crate) fn refresh_pto_timer(&mut self) {
        self.pto_deadline = self
            .sent
            .values()
            .next_back()
            .map(|p| p.sent_at.saturating_add(self.current_pto()));
    }

    /// Items from the oldest in-flight packet that carries frames (for PTO probe retransmission).
    pub(crate) fn oldest_in_flight_items(&self) -> Vec<SentItem> {
        self.sent
            .values()
            .find(|p| !p.items.is_empty())
            .map(|p| p.items.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn replay_window_tracks_1024_and_ranges() {
        let mut rw = ReplayWindow::default();
        assert!(!rw.is_seen_or_old(0));
        rw.mark_seen(0);
        assert!(rw.is_seen_or_old(0));

        rw.mark_seen(5);
        rw.mark_seen(3);
        let (largest, ranges) = rw.ack_ranges().unwrap();
        assert_eq!(largest, 5);
        assert_eq!(ranges, vec![(5, 5), (3, 3), (0, 0)]);

        rw.mark_seen(1024);
        assert!(rw.is_seen_or_old(0));
        assert!(!rw.is_seen_or_old(1));
        assert!(rw.is_seen_or_old(1024));
    }

    #[test]
    fn newreno_single_cut_per_recovery_and_persistent_congestion() {
        let mut lr = LossRecovery::default();
        for pn in 1..=5 {
            lr.on_packet_sent(SentPacket {
                pn,
                sent_at: pn * 1000,
                bytes: 1200,
                items: vec![],
            });
        }
        assert_eq!(lr.bytes_in_flight, 6000);
        // Ack pn 4 and 5 -> pn 1 and 2 lost by 3-packet threshold
        let ev = lr.on_ack(5, 0, &[(4, 5)], 50_000, 6);
        assert_eq!(ev.lost_count, 2);
        // Only one cut: 12000 -> 6000
        assert_eq!(lr.cwnd, 6000);
        // Time-threshold loss of pn 3 (sent before recovery started) must not cut again
        let mut ev2 = RecoveryEvents::default();
        lr.detect_losses(500_000, 6, &mut ev2);
        assert_eq!(ev2.lost_count, 1);
        assert_eq!(lr.cwnd, 6000);
    }
}
