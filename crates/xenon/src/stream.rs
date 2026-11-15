//! Per-stream send/receive buffers, reassembly, and stream-level flow control.

use crate::Error;
use std::collections::{BTreeMap, VecDeque};

/// Maximum unacknowledged bytes accepted by `write` per stream (1 MiB).
pub(crate) const MAX_STREAM_UNACKED: usize = 1_048_576;

/// State of one bidirectional Xenon stream.
#[derive(Debug, Clone)]
pub(crate) struct Stream {
    // Send side
    pub(crate) send_open: bool,
    pub(crate) send_base: u64,
    pub(crate) send_buf: VecDeque<u8>,
    pub(crate) send_next: u64,
    pub(crate) send_acked: BTreeMap<u64, u64>,
    pub(crate) fin_offset: Option<u64>,
    pub(crate) fin_sent: bool,
    pub(crate) fin_acked: bool,
    pub(crate) send_reset: Option<u64>,
    pub(crate) need_send_reset: bool,
    pub(crate) reset_acked: bool,
    pub(crate) lost_chunks: VecDeque<(u64, usize, bool)>,
    pub(crate) peer_max_data: u64,

    // Receive side
    pub(crate) recv_read_pos: u64,
    pub(crate) max_recv_offset: u64,
    pub(crate) recv_buf: BTreeMap<u64, Vec<u8>>,
    pub(crate) final_size: Option<u64>,
    pub(crate) recv_reset: Option<u64>,
    pub(crate) local_max_data: u64,
    pub(crate) acked_local_max_data: u64,
    pub(crate) need_max_stream_data: bool,
    pub(crate) recv_window_step: u64,
}

impl Stream {
    pub(crate) fn new(local_window: u64, peer_window: u64) -> Self {
        Self {
            send_open: true,
            send_base: 0,
            send_buf: VecDeque::new(),
            send_next: 0,
            send_acked: BTreeMap::new(),
            fin_offset: None,
            fin_sent: false,
            fin_acked: false,
            send_reset: None,
            need_send_reset: false,
            reset_acked: false,
            lost_chunks: VecDeque::new(),
            peer_max_data: peer_window,
            recv_read_pos: 0,
            max_recv_offset: 0,
            recv_buf: BTreeMap::new(),
            final_size: None,
            recv_reset: None,
            local_max_data: local_window,
            acked_local_max_data: local_window,
            need_max_stream_data: false,
            recv_window_step: local_window,
        }
    }

    /// Total bytes accepted from the local application so far.
    pub(crate) fn write_pos(&self) -> u64 {
        self.send_base + self.send_buf.len() as u64
    }

    /// Append up to 1 MiB unacknowledged bytes to the send buffer.
    pub(crate) fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        if !self.send_open {
            return Err(Error::Protocol("unknown stream"));
        }
        let avail = MAX_STREAM_UNACKED.saturating_sub(self.send_buf.len());
        let take = data.len().min(avail);
        if let Some(chunk) = data.get(..take) {
            self.send_buf.extend(chunk);
        }
        Ok(take)
    }

    /// Mark the send side finished at the current write position.
    pub(crate) fn finish(&mut self) -> Result<(), Error> {
        if self.send_reset.is_some() {
            return Err(Error::Protocol("unknown stream"));
        }
        if self.fin_offset.is_none() {
            self.fin_offset = Some(self.write_pos());
            self.send_open = false;
        }
        Ok(())
    }

    /// Abandon the send side with `code`.
    pub(crate) fn reset(&mut self, code: u64) -> Result<(), Error> {
        if self.send_reset.is_some() {
            return Ok(());
        }
        self.send_open = false;
        self.send_reset = Some(code);
        self.need_send_reset = true;
        self.lost_chunks.clear();
        self.send_buf.clear();
        Ok(())
    }

    /// Copy a slice `[offset..offset+len]` from `send_buf`, trimming any already-acked prefix.
    pub(crate) fn slice_for_retransmit(
        &self,
        offset: u64,
        len: usize,
        fin: bool,
    ) -> (u64, Vec<u8>, bool) {
        let end = offset.saturating_add(len as u64);
        let actual_start = offset.max(self.send_base);
        if actual_start >= end {
            return (end, Vec::new(), fin);
        }
        let rel_start = (actual_start - self.send_base) as usize;
        let rel_end = ((end - self.send_base) as usize).min(self.send_buf.len());
        let mut out = Vec::with_capacity(rel_end.saturating_sub(rel_start));
        for idx in rel_start..rel_end {
            if let Some(&b) = self.send_buf.get(idx) {
                out.push(b);
            }
        }
        (actual_start, out, fin)
    }

    /// Copy up to `len` new unsent bytes starting at `send_next`.
    pub(crate) fn take_new_send(&mut self, max_bytes: usize) -> (u64, Vec<u8>, bool) {
        let offset = self.send_next;
        let rel_start = offset.saturating_sub(self.send_base) as usize;
        let avail = self.send_buf.len().saturating_sub(rel_start);
        let take = avail.min(max_bytes);
        let mut out = Vec::with_capacity(take);
        for idx in rel_start..rel_start + take {
            if let Some(&b) = self.send_buf.get(idx) {
                out.push(b);
            }
        }
        self.send_next += take as u64;
        let fin = self.fin_offset == Some(self.send_next) && !self.fin_sent;
        if fin {
            self.fin_sent = true;
        }
        (offset, out, fin)
    }

    /// Record an acknowledged range `[offset..offset+len]` and optional `fin`.
    pub(crate) fn mark_send_acked(&mut self, offset: u64, len: usize, fin: bool) {
        if fin {
            self.fin_acked = true;
        }
        if len == 0 {
            return;
        }
        let end = offset.saturating_add(len as u64);
        if end <= self.send_base {
            return;
        }
        let mut new_start = offset.max(self.send_base);
        let mut new_end = end;

        let to_merge: Vec<(u64, u64)> = self
            .send_acked
            .iter()
            .filter(|(&s, &e)| e >= new_start && s <= new_end)
            .map(|(&s, &e)| (s, e))
            .collect();
        for (s, e) in to_merge {
            self.send_acked.remove(&s);
            new_start = new_start.min(s);
            new_end = new_end.max(e);
        }
        self.send_acked.insert(new_start, new_end);

        while let Some((&s, &e)) = self.send_acked.iter().next() {
            if s > self.send_base {
                break;
            }
            self.send_acked.remove(&s);
            if e > self.send_base {
                let advance = (e - self.send_base) as usize;
                let drain = advance.min(self.send_buf.len());
                self.send_buf.drain(..drain);
                self.send_base += drain as u64;
            }
        }
    }

    /// Handle incoming `STREAM` frame data. Returns `Err((wire_code, msg))` on error.
    pub(crate) fn on_recv_data(
        &mut self,
        offset: u64,
        data: &[u8],
        fin: bool,
        conn_recv_total: &mut u64,
        local_conn_max_data: u64,
    ) -> Result<(), (u64, &'static str)> {
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or((1u64, "stream offset overflow"))?;

        if let Some(fs) = self.final_size {
            if end > fs {
                return Err((1, "data past fin"));
            }
            if fin && end != fs {
                return Err((1, "fin size changed"));
            }
        } else if fin {
            if end < self.max_recv_offset {
                return Err((1, "fin before received data"));
            }
            self.final_size = Some(end);
        }

        if end > self.local_max_data {
            return Err((2, "stream flow control exceeded"));
        }
        if end > self.max_recv_offset {
            let delta = end - self.max_recv_offset;
            let new_conn = conn_recv_total
                .checked_add(delta)
                .ok_or((2u64, "connection flow control exceeded"))?;
            if new_conn > local_conn_max_data {
                return Err((2, "connection flow control exceeded"));
            }
            *conn_recv_total = new_conn;
            self.max_recv_offset = end;
        }

        if self.recv_reset.is_some() || data.is_empty() {
            return Ok(());
        }
        self.insert_recv_chunk(offset, data);
        Ok(())
    }

    /// Handle incoming `RESET_STREAM` frame. Returns `Err((wire_code, msg))` on error.
    pub(crate) fn on_recv_reset(
        &mut self,
        code: u64,
        final_size: u64,
        conn_recv_total: &mut u64,
        local_conn_max_data: u64,
    ) -> Result<(), (u64, &'static str)> {
        if final_size < self.max_recv_offset {
            return Err((1, "reset final size below received data"));
        }
        if let Some(fs) = self.final_size {
            if fs != final_size {
                return Err((1, "reset final size mismatch"));
            }
        }
        if final_size > self.local_max_data {
            return Err((2, "reset stream flow control exceeded"));
        }
        if final_size > self.max_recv_offset {
            let delta = final_size - self.max_recv_offset;
            let new_conn = conn_recv_total
                .checked_add(delta)
                .ok_or((2u64, "reset connection flow control exceeded"))?;
            if new_conn > local_conn_max_data {
                return Err((2, "reset connection flow control exceeded"));
            }
            *conn_recv_total = new_conn;
            self.max_recv_offset = final_size;
        }
        self.final_size = Some(final_size);
        if self.recv_reset.is_none() {
            self.recv_reset = Some(code);
            self.recv_buf.clear();
        }
        Ok(())
    }

    fn insert_recv_chunk(&mut self, offset: u64, data: &[u8]) {
        let end = offset + data.len() as u64;
        let mut cur = offset.max(self.recv_read_pos);
        if cur >= end {
            return;
        }
        if let Some((&prev_k, prev_v)) = self.recv_buf.range(..=cur).next_back() {
            let prev_end = prev_k + prev_v.len() as u64;
            if prev_end > cur {
                cur = prev_end;
                if cur >= end {
                    return;
                }
            }
        }
        while cur < end {
            let next_existing = self
                .recv_buf
                .range(cur..end)
                .next()
                .map(|(&k, v)| (k, v.len()));
            match next_existing {
                Some((next_k, next_len)) => {
                    if next_k > cur {
                        let lo = (cur - offset) as usize;
                        let hi = (next_k - offset) as usize;
                        if let Some(part) = data.get(lo..hi) {
                            self.push_disjoint(cur, part);
                        }
                    }
                    cur = next_k + next_len as u64;
                }
                None => {
                    let lo = (cur - offset) as usize;
                    if let Some(part) = data.get(lo..) {
                        self.push_disjoint(cur, part);
                    }
                    break;
                }
            }
        }
    }

    fn push_disjoint(&mut self, offset: u64, part: &[u8]) {
        if part.is_empty() {
            return;
        }
        if let Some((&prev_k, prev_v)) = self.recv_buf.range(..offset).next_back() {
            if prev_k + prev_v.len() as u64 == offset {
                if let Some(v) = self.recv_buf.get_mut(&prev_k) {
                    v.extend_from_slice(part);
                    return;
                }
            }
        }
        self.recv_buf.insert(offset, part.to_vec());
    }

    /// Read contiguous bytes into `buf`, advancing the stream window if half-consumed.
    pub(crate) fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        if let Some(code) = self.recv_reset {
            return Err(Error::Closed {
                code,
                reason: String::new(),
            });
        }
        let mut copied = 0;
        while copied < buf.len() {
            let Some((&start, _)) = self.recv_buf.iter().next() else {
                break;
            };
            if start > self.recv_read_pos {
                break;
            }
            let Some(mut chunk) = self.recv_buf.remove(&start) else {
                break;
            };
            let take = (buf.len() - copied).min(chunk.len());
            if let (Some(dst), Some(src)) = (buf.get_mut(copied..copied + take), chunk.get(..take))
            {
                dst.copy_from_slice(src);
            }
            copied += take;
            self.recv_read_pos += take as u64;
            if take < chunk.len() {
                chunk.drain(..take);
                self.recv_buf.insert(self.recv_read_pos, chunk);
            }
        }
        if self.final_size.is_none()
            && self.local_max_data.saturating_sub(self.recv_read_pos) <= self.recv_window_step / 2
        {
            let new_limit = self.recv_read_pos.saturating_add(self.recv_window_step);
            if new_limit > self.local_max_data {
                self.local_max_data = new_limit;
                self.need_max_stream_data = true;
            }
        }
        Ok(copied)
    }

    /// True if FIN has been received and all bytes have been read.
    pub(crate) fn is_finished(&self) -> bool {
        self.recv_reset.is_none() && self.final_size == Some(self.recv_read_pos)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn reassembly_and_flow_control_window() {
        let mut s = Stream::new(100, 100);
        let mut conn_rx = 0;
        s.on_recv_data(5, b"world", true, &mut conn_rx, 1000)
            .unwrap();
        let mut out = [0u8; 10];
        assert_eq!(s.read(&mut out).unwrap(), 0);
        s.on_recv_data(0, b"hello", false, &mut conn_rx, 1000)
            .unwrap();
        assert_eq!(s.read(&mut out).unwrap(), 10);
        assert_eq!(&out, b"helloworld");
        assert!(s.is_finished());
        assert_eq!(conn_rx, 10);
    }

    #[test]
    fn write_caps_unacked_at_1mib() {
        let mut s = Stream::new(262_144, 262_144);
        let big = vec![0xab; MAX_STREAM_UNACKED + 500];
        let n = s.write(&big).unwrap();
        assert_eq!(n, MAX_STREAM_UNACKED);
        assert_eq!(s.write(&[1, 2, 3]).unwrap(), 0);
        s.mark_send_acked(0, 100, false);
        assert_eq!(s.write(&[1, 2, 3]).unwrap(), 3);
    }
}
