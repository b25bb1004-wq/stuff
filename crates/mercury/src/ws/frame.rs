//! WebSocket frame encoding and decoding (RFC 6455 Section 5).

use alloc::vec::Vec;

/// A single decoded or outgoing WebSocket frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Whether this is the final fragment in a message.
    pub fin: bool,
    /// Four-bit frame opcode (`0x0`..=`0xF`).
    pub opcode: u8,
    /// Unmasked frame payload bytes.
    pub payload: Vec<u8>,
}

/// Errors returned by WebSocket frame decoding and connection operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsError {
    /// A WebSocket framing or protocol invariant was violated.
    Protocol,
    /// A frame or reassembled message exceeded the configured size limit.
    TooBig,
    /// A text or close reason payload contained invalid UTF-8.
    Utf8,
    /// The WebSocket connection has been closed.
    Closed,
    /// An underlying I/O error occurred on the socket.
    Io,
}

/// One frame from the start of `buf`: `Ok(None)` when more bytes are needed; `masked` says
/// whether the frame must be masked (server reading a client: `true`) and is enforced.
pub fn decode_frame(
    buf: &[u8],
    masked: bool,
    max_payload: usize,
) -> Result<Option<(Frame, usize)>, WsError> {
    if buf.len() < 2 {
        return Ok(None);
    }

    let b0 = buf[0];
    let b1 = buf[1];

    let fin = (b0 & 0x80) != 0;
    let rsv = b0 & 0x70;
    if rsv != 0 {
        return Err(WsError::Protocol);
    }

    let opcode = b0 & 0x0F;
    if !matches!(opcode, 0x0 | 0x1 | 0x2 | 0x8 | 0x9 | 0xA) {
        return Err(WsError::Protocol);
    }

    let is_control = (opcode & 0x08) != 0;
    if is_control && !fin {
        return Err(WsError::Protocol);
    }

    let is_masked = (b1 & 0x80) != 0;
    if is_masked != masked {
        return Err(WsError::Protocol);
    }

    let len7 = b1 & 0x7F;
    if is_control && len7 > 125 {
        return Err(WsError::Protocol);
    }

    let mut offset: usize = 2;
    let payload_len: usize = match len7 {
        0..=125 => usize::from(len7),
        126 => {
            if buf.len() < 4 {
                return Ok(None);
            }
            let raw = u16::from_be_bytes([buf[2], buf[3]]);
            if raw < 126 {
                return Err(WsError::Protocol);
            }
            offset = 4;
            usize::from(raw)
        }
        _ => {
            if buf.len() < 10 {
                return Ok(None);
            }
            let raw = u64::from_be_bytes([
                buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8], buf[9],
            ]);
            if (raw >> 63) != 0 || raw <= 65535 {
                return Err(WsError::Protocol);
            }
            offset = 10;
            usize::try_from(raw).map_err(|_| WsError::TooBig)?
        }
    };

    if payload_len > max_payload {
        return Err(WsError::TooBig);
    }

    let mask_key = if is_masked {
        let Some(key_end) = offset.checked_add(4) else {
            return Err(WsError::TooBig);
        };
        if buf.len() < key_end {
            return Ok(None);
        }
        let key = [
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ];
        offset = key_end;
        Some(key)
    } else {
        None
    };

    let total_len = offset.checked_add(payload_len).ok_or(WsError::TooBig)?;
    if buf.len() < total_len {
        return Ok(None);
    }

    let mut payload = buf[offset..total_len].to_vec();
    if let Some(key) = mask_key {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= key[i & 3];
        }
    }

    Ok(Some((
        Frame {
            fin,
            opcode,
            payload,
        },
        total_len,
    )))
}

/// Encode `f` into wire bytes using the shortest length form, optionally masked with `mask`.
pub fn encode_frame(f: &Frame, mask: Option<[u8; 4]>) -> Vec<u8> {
    let len = f.payload.len();
    let header_len =
        2 + if len <= 125 {
            0
        } else if len <= 65535 {
            2
        } else {
            8
        } + if mask.is_some() { 4 } else { 0 };

    let mut out = Vec::with_capacity(header_len + len);
    let b0 = if f.fin { 0x80 } else { 0x00 } | (f.opcode & 0x0F);
    out.push(b0);

    let mask_bit = if mask.is_some() { 0x80 } else { 0x00 };
    if len <= 125 {
        out.push(mask_bit | (len as u8));
    } else if len <= 65535 {
        out.push(mask_bit | 126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(mask_bit | 127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }

    if let Some(key) = mask {
        out.extend_from_slice(&key);
        for (i, &b) in f.payload.iter().enumerate() {
            out.push(b ^ key[i & 3]);
        }
    } else {
        out.extend_from_slice(&f.payload);
    }

    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use assay::{forall, hex};

    #[test]
    fn rfc6455_5_7_examples() {
        // 1. Single-frame unmasked text "Hello"
        let f1 = Frame {
            fin: true,
            opcode: 0x1,
            payload: b"Hello".to_vec(),
        };
        let w1 = hex("81 05 48 65 6c 6c 6f");
        assert_eq!(encode_frame(&f1, None), w1);
        assert_eq!(
            decode_frame(&w1, false, 1024).unwrap(),
            Some((f1, w1.len()))
        );

        // 2. Single-frame masked text "Hello"
        let f2 = Frame {
            fin: true,
            opcode: 0x1,
            payload: b"Hello".to_vec(),
        };
        let mask = [0x37, 0xfa, 0x21, 0x3d];
        let w2 = hex("81 85 37 fa 21 3d 7f 9f 4d 51 58");
        assert_eq!(encode_frame(&f2, Some(mask)), w2);
        assert_eq!(decode_frame(&w2, true, 1024).unwrap(), Some((f2, w2.len())));

        // 3. Fragmented unmasked text "Hel" + "lo"
        let f3a = Frame {
            fin: false,
            opcode: 0x1,
            payload: b"Hel".to_vec(),
        };
        let w3a = hex("01 03 48 65 6c");
        assert_eq!(encode_frame(&f3a, None), w3a);
        assert_eq!(
            decode_frame(&w3a, false, 1024).unwrap(),
            Some((f3a, w3a.len()))
        );

        let f3b = Frame {
            fin: true,
            opcode: 0x0,
            payload: b"lo".to_vec(),
        };
        let w3b = hex("80 02 6c 6f");
        assert_eq!(encode_frame(&f3b, None), w3b);
        assert_eq!(
            decode_frame(&w3b, false, 1024).unwrap(),
            Some((f3b, w3b.len()))
        );

        // 4. Unmasked Ping "Hello" and masked Pong "Hello"
        let ping = Frame {
            fin: true,
            opcode: 0x9,
            payload: b"Hello".to_vec(),
        };
        let w_ping = hex("89 05 48 65 6c 6c 6f");
        assert_eq!(encode_frame(&ping, None), w_ping);
        assert_eq!(
            decode_frame(&w_ping, false, 1024).unwrap(),
            Some((ping, w_ping.len()))
        );

        let pong = Frame {
            fin: true,
            opcode: 0xA,
            payload: b"Hello".to_vec(),
        };
        let w_pong = hex("8a 85 37 fa 21 3d 7f 9f 4d 51 58");
        assert_eq!(encode_frame(&pong, Some(mask)), w_pong);
        assert_eq!(
            decode_frame(&w_pong, true, 1024).unwrap(),
            Some((pong, w_pong.len()))
        );

        // 5. 256-byte binary message in a single unmasked frame
        let data256 = alloc::vec![0xAB; 256];
        let f5 = Frame {
            fin: true,
            opcode: 0x2,
            payload: data256.clone(),
        };
        let mut w5 = hex("82 7e 01 00");
        w5.extend_from_slice(&data256);
        assert_eq!(encode_frame(&f5, None), w5);
        assert_eq!(
            decode_frame(&w5, false, 1024).unwrap(),
            Some((f5, w5.len()))
        );

        // 6. 65536-byte binary message in a single unmasked frame
        let data64k = alloc::vec![0xCD; 65536];
        let f6 = Frame {
            fin: true,
            opcode: 0x2,
            payload: data64k.clone(),
        };
        let mut w6 = hex("82 7f 00 00 00 00 00 01 00 00");
        w6.extend_from_slice(&data64k);
        assert_eq!(encode_frame(&f6, None), w6);
        assert_eq!(
            decode_frame(&w6, false, 100_000).unwrap(),
            Some((f6, w6.len()))
        );
    }

    #[test]
    fn roundtrip_and_fuzz_never_panics() {
        const VALID_OPCODES: [u8; 6] = [0x0, 0x1, 0x2, 0x8, 0x9, 0xA];
        forall(1000, 0x5EED_1234, |rng| {
            let opcode = *rng.pick(&VALID_OPCODES);
            let is_control = (opcode & 0x08) != 0;
            let fin = if is_control { true } else { rng.chance(1, 2) };
            let max_len = if is_control { 125 } else { 4096 };
            let payload = rng.bytes(max_len);
            let frame = Frame {
                fin,
                opcode,
                payload,
            };
            let mask = if rng.chance(1, 2) {
                let mut k = [0u8; 4];
                rng.fill(&mut k);
                Some(k)
            } else {
                None
            };
            let wire = encode_frame(&frame, mask);
            let decoded = decode_frame(&wire, mask.is_some(), 4096).unwrap().unwrap();
            assert_eq!(decoded, (frame, wire.len()));

            // Random garbage bytes must never panic.
            let garbage = rng.bytes(256);
            let _ = decode_frame(&garbage, rng.chance(1, 2), 4096);
        });
    }
}
