//! Wire format encoding and decoding for Xenon packets, parameters, and frames.

use crate::Config;

/// Packet type bytes.
pub(crate) const PKT_INITIAL: u8 = 1;
pub(crate) const PKT_RESPONSE: u8 = 2;
pub(crate) const PKT_DATA: u8 = 3;

/// Protocol version byte.
pub(crate) const VERSION: u8 = 1;

/// Fixed plaintext header length in bytes.
pub(crate) const HEADER_LEN: usize = 18;

/// Poly1305 tag length in bytes.
pub(crate) const TAG_LEN: usize = 16;

/// Initial packet plaintext size (1200 - 18 header - 32 E_c - 16 tag).
pub(crate) const INITIAL_PLAINTEXT_LEN: usize = 1134;

/// Maximum number of ACK ranges encoded in an ACK frame.
pub(crate) const MAX_ACK_RANGES: usize = 32;

/// Decoded 18-byte packet header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) pkt_type: u8,
    pub(crate) version: u8,
    pub(crate) cid: u64,
    pub(crate) pn: u64,
}

impl Header {
    /// Encode header into 18 bytes.
    pub(crate) fn encode(self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0] = self.pkt_type;
        out[1] = self.version;
        out[2..10].copy_from_slice(&self.cid.to_le_bytes());
        out[10..18].copy_from_slice(&self.pn.to_le_bytes());
        out
    }

    /// Decode an 18-byte header from the front of `buf`.
    pub(crate) fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() < HEADER_LEN {
            return None;
        }
        let cid_bytes: [u8; 8] = buf.get(2..10)?.try_into().ok()?;
        let pn_bytes: [u8; 8] = buf.get(10..18)?.try_into().ok()?;
        Some(Header {
            pkt_type: *buf.first()?,
            version: *buf.get(1)?,
            cid: u64::from_le_bytes(cid_bytes),
            pn: u64::from_le_bytes(pn_bytes),
        })
    }
}

/// Build the 12-byte AEAD nonce for packet number `pn`.
pub(crate) fn nonce(pn: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..12].copy_from_slice(&pn.to_le_bytes());
    n
}

/// Encode `v` as minimal unsigned LEB128.
pub(crate) fn encode_varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

/// Byte length of `v` as unsigned LEB128.
pub(crate) fn varint_len(mut v: u64) -> usize {
    let mut n = 1;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}

/// Decode a minimal unsigned LEB128 integer (at most 10 bytes, fitting in `u64`).
pub(crate) fn decode_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut val = 0u64;
    for i in 0..10 {
        let b = *buf.get(*pos)?;
        *pos += 1;
        if i > 0 && b == 0 {
            return None;
        }
        if i == 9 && (b & 0xfe) != 0 {
            return None;
        }
        val |= u64::from(b & 0x7f) << (i * 7);
        if (b & 0x80) == 0 {
            return Some(val);
        }
    }
    None
}

/// Transport parameters exchanged in the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Params {
    pub(crate) max_streams: u64,
    pub(crate) stream_window: u64,
    pub(crate) conn_window: u64,
    pub(crate) idle_timeout_ms: u64,
}

impl Params {
    pub(crate) fn from_config(cfg: &Config) -> Self {
        Self {
            max_streams: cfg.max_streams,
            stream_window: cfg.stream_window,
            conn_window: cfg.conn_window,
            idle_timeout_ms: cfg.idle_timeout_ms,
        }
    }

    pub(crate) fn encode(self, out: &mut Vec<u8>) {
        encode_varint(4, out);
        encode_varint(1, out);
        encode_varint(self.max_streams, out);
        encode_varint(2, out);
        encode_varint(self.stream_window, out);
        encode_varint(3, out);
        encode_varint(self.conn_window, out);
        encode_varint(4, out);
        encode_varint(self.idle_timeout_ms, out);
    }

    pub(crate) fn decode(buf: &[u8], allow_trailing: bool) -> Option<Self> {
        let mut p = Self::from_config(&Config::default());
        let mut pos = 0;
        let count = decode_varint(buf, &mut pos)?;
        let rem = buf.len().saturating_sub(pos);
        if count > (rem as u64) / 2 {
            return None;
        }
        for _ in 0..count {
            let id = decode_varint(buf, &mut pos)?;
            let val = decode_varint(buf, &mut pos)?;
            match id {
                1 => p.max_streams = val,
                2 => p.stream_window = val,
                3 => p.conn_window = val,
                4 => p.idle_timeout_ms = val,
                _ => {}
            }
        }
        if !allow_trailing && pos != buf.len() {
            return None;
        }
        Some(p)
    }
}

/// Decoded Data packet frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Frame {
    Padding,
    Ping,
    Ack {
        largest: u64,
        delay_us: u64,
        /// Inclusive `(start, end)` ranges sorted descending.
        ranges: Vec<(u64, u64)>,
    },
    Stream {
        id: u64,
        offset: u64,
        fin: bool,
        data: Vec<u8>,
    },
    MaxData(u64),
    MaxStreamData {
        id: u64,
        limit: u64,
    },
    ResetStream {
        id: u64,
        code: u64,
        final_size: u64,
    },
    Close {
        code: u64,
        reason: String,
    },
}

impl Frame {
    /// Whether this frame is ack-eliciting (everything except PADDING and ACK).
    pub(crate) fn is_ack_eliciting(&self) -> bool {
        !matches!(self, Frame::Padding | Frame::Ack { .. })
    }

    /// Encode this frame onto `out`.
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Frame::Padding => out.push(0x00),
            Frame::Ping => out.push(0x01),
            Frame::Ack {
                largest,
                delay_us,
                ranges,
            } => {
                if let Some(&(first_start, first_end)) = ranges.first() {
                    out.push(0x02);
                    encode_varint(*largest, out);
                    encode_varint(*delay_us, out);
                    let extra = ranges.len().saturating_sub(1);
                    encode_varint(extra as u64, out);
                    encode_varint(first_end.saturating_sub(first_start), out);
                    for pair in ranges.windows(2) {
                        let prev_start = pair[0].0;
                        let (cur_start, cur_end) = pair[1];
                        let gap = prev_start.saturating_sub(cur_end).saturating_sub(2);
                        let len = cur_end.saturating_sub(cur_start);
                        encode_varint(gap, out);
                        encode_varint(len, out);
                    }
                }
            }
            Frame::Stream {
                id,
                offset,
                fin,
                data,
            } => {
                out.push(0x03);
                out.push(u8::from(*fin));
                encode_varint(*id, out);
                encode_varint(*offset, out);
                encode_varint(data.len() as u64, out);
                out.extend_from_slice(data);
            }
            Frame::MaxData(limit) => {
                out.push(0x04);
                encode_varint(*limit, out);
            }
            Frame::MaxStreamData { id, limit } => {
                out.push(0x05);
                encode_varint(*id, out);
                encode_varint(*limit, out);
            }
            Frame::ResetStream {
                id,
                code,
                final_size,
            } => {
                out.push(0x06);
                encode_varint(*id, out);
                encode_varint(*code, out);
                encode_varint(*final_size, out);
            }
            Frame::Close { code, reason } => {
                out.push(0x07);
                encode_varint(*code, out);
                encode_varint(reason.len() as u64, out);
                out.extend_from_slice(reason.as_bytes());
            }
        }
    }

    /// Encoded byte size of a Stream frame with `data_len` payload bytes.
    pub(crate) fn stream_header_len(id: u64, offset: u64, data_len: usize) -> usize {
        2 + varint_len(id) + varint_len(offset) + varint_len(data_len as u64)
    }
}

/// Decode all frames in a decrypted Data packet payload.
pub(crate) fn decode_frames(buf: &[u8]) -> Option<Vec<Frame>> {
    let mut frames = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let ty = *buf.get(pos)?;
        pos += 1;
        let frame = match ty {
            0x00 => Frame::Padding,
            0x01 => Frame::Ping,
            0x02 => decode_ack(buf, &mut pos)?,
            0x03 => decode_stream(buf, &mut pos)?,
            0x04 => Frame::MaxData(decode_varint(buf, &mut pos)?),
            0x05 => Frame::MaxStreamData {
                id: decode_varint(buf, &mut pos)?,
                limit: decode_varint(buf, &mut pos)?,
            },
            0x06 => Frame::ResetStream {
                id: decode_varint(buf, &mut pos)?,
                code: decode_varint(buf, &mut pos)?,
                final_size: decode_varint(buf, &mut pos)?,
            },
            0x07 => {
                let code = decode_varint(buf, &mut pos)?;
                let len_u64 = decode_varint(buf, &mut pos)?;
                let len = usize::try_from(len_u64).ok()?;
                let end = pos.checked_add(len)?;
                let slice = buf.get(pos..end)?;
                let reason = core::str::from_utf8(slice).ok()?.to_owned();
                pos = end;
                Frame::Close { code, reason }
            }
            _ => return None,
        };
        frames.push(frame);
    }
    Some(frames)
}

fn decode_ack(buf: &[u8], pos: &mut usize) -> Option<Frame> {
    let largest = decode_varint(buf, pos)?;
    let delay_us = decode_varint(buf, pos)?;
    let extra_u64 = decode_varint(buf, pos)?;
    if extra_u64 > MAX_ACK_RANGES as u64 {
        return None;
    }
    let extra = extra_u64 as usize;
    let first_len = decode_varint(buf, pos)?;
    let mut smallest = largest.checked_sub(first_len)?;
    let mut ranges = Vec::with_capacity(extra + 1);
    ranges.push((smallest, largest));
    for _ in 0..extra {
        let gap = decode_varint(buf, pos)?;
        let len = decode_varint(buf, pos)?;
        let cur_end = smallest.checked_sub(gap)?.checked_sub(2)?;
        let cur_start = cur_end.checked_sub(len)?;
        ranges.push((cur_start, cur_end));
        smallest = cur_start;
    }
    Some(Frame::Ack {
        largest,
        delay_us,
        ranges,
    })
}

fn decode_stream(buf: &[u8], pos: &mut usize) -> Option<Frame> {
    let flags = *buf.get(*pos)?;
    *pos += 1;
    if (flags & !0x01) != 0 {
        return None;
    }
    let fin = (flags & 0x01) != 0;
    let id = decode_varint(buf, pos)?;
    let offset = decode_varint(buf, pos)?;
    let len_u64 = decode_varint(buf, pos)?;
    let len = usize::try_from(len_u64).ok()?;
    offset.checked_add(len_u64)?;
    let end = pos.checked_add(len)?;
    let data = buf.get(*pos..end)?.to_vec();
    *pos = end;
    Some(Frame::Stream {
        id,
        offset,
        fin,
        data,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::forall;

    #[test]
    fn varint_round_trip_and_non_minimal_reject() {
        for v in [
            0u64,
            1,
            127,
            128,
            16383,
            16384,
            u64::from(u32::MAX),
            u64::MAX,
        ] {
            let mut enc = Vec::new();
            encode_varint(v, &mut enc);
            assert_eq!(enc.len(), varint_len(v));
            let mut pos = 0;
            assert_eq!(decode_varint(&enc, &mut pos), Some(v));
            assert_eq!(pos, enc.len());
        }
        // Non-minimal zero
        let mut p = 0;
        assert_eq!(decode_varint(&[0x80, 0x00], &mut p), None);
        // Overflow 10th byte
        let mut p = 0;
        assert_eq!(
            decode_varint(
                &[0x80; 9].into_iter().chain([0x02]).collect::<Vec<_>>(),
                &mut p
            ),
            None
        );
    }

    #[test]
    fn frame_round_trip_property() {
        forall(100, 17, |r| {
            let frames = vec![
                Frame::Ping,
                Frame::Ack {
                    largest: 100,
                    delay_us: 2500,
                    ranges: vec![(95, 100), (80, 90), (0, 50)],
                },
                Frame::Stream {
                    id: r.range(0, 200),
                    offset: r.range(0, 1_000_000),
                    fin: r.chance(1, 2),
                    data: r.bytes(100),
                },
                Frame::MaxData(r.u64() >> 2),
                Frame::MaxStreamData {
                    id: 4,
                    limit: 524_288,
                },
                Frame::ResetStream {
                    id: 2,
                    code: 7,
                    final_size: 42,
                },
                Frame::Close {
                    code: 4,
                    reason: "bye".to_owned(),
                },
            ];
            let mut buf = Vec::new();
            for f in &frames {
                f.encode(&mut buf);
            }
            assert_eq!(decode_frames(&buf), Some(frames));
        });
    }
}
