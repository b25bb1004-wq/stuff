# Tremor design (D2)
v1, written by the leader on 2026-11-15. Encrypted, reliable, multi-stream transport over UDP.
Decides the handshake, packet format, streams, loss recovery and congestion control.
Tasks: T34 protocol core (no I/O inside), T35 UDP transport and tools. Crypto comes from onyx
(`x25519`, `hkdf`, `aead` ChaCha20-Poly1305, `sha256`). Not compatible with QUIC or Noise; it
borrows their ideas, and says so.

## 1. Shape
- One UDP datagram carries one packet, at most 1200 bytes. All integers little-endian unless said.
  `varint` = unsigned LEB128, at most 10 bytes, minimal.
- The core is a sans-I/O state machine: the caller feeds datagrams and the time (u64
  microseconds) in, and takes datagrams and the next timer out. No clock, socket or thread inside.
- A client knows the server's static X25519 public key `S` in advance (like a pinned key). v1
  authenticates the server only; applications authenticate clients over the channel, bound to
  the handshake hash `h1` (exported).

## 2. Handshake (1 round trip)
Client: ephemeral `e_c`, `E_c = x25519::public_key(e_c)`, random connection id `cid` (u64).
- `h0 = sha256(b"tremor/1" || S || E_c)`, `es = DH(e_c, S)` (server: `DH(s, E_c)`),
  `prk1 = hkdf::extract(b"tremor/1", es)`, `k_init = expand(prk1, b"init" || h0, 32)`.
- **Initial** (client to server): header, `E_c`, then `aead::seal(k_init, nonce(0), aad = header || E_c,
  params || zero padding)` with the padding making the datagram exactly 1200 bytes. A server drops
  any Initial shorter than 1200 bytes (limits amplification: its reply is never larger).
- Server: ephemeral `e_s`, `E_s`. `ee = DH(e_s, E_c)`, `h1 = sha256(h0 || initial datagram || E_s)`,
  `prk2 = hkdf::extract(prk1, ee)`, `k_c2s = expand(prk2, b"c2s" || h1, 32)`,
  `k_s2c = expand(prk2, b"s2c" || h1, 32)`.
- **Response** (server to client): header (packet number 0), `E_s`, then
  `seal(k_s2c, nonce(0), aad = header || E_s, params)`. Only a holder of `s` can produce it
  (es), and `ee` gives forward secrecy.
- The client is connected when the Response opens. The server is connected when the first Data
  packet under `k_c2s` opens. A replayed Initial makes the server send a Response nobody can use;
  half-open connections are capped (default 256) and expire after 10 s.
- The client resends the same Initial on its probe timer (section 5, starting at 300 ms) and gives
  up with `Error::Timeout` after 6 tries. A server that gets a repeat Initial for a known `cid`
  resends the same Response.
- Params (inside the sealed parts): varint count of (id varint, value varint) pairs: 1 max
  open streams (default 100), 2 initial stream window (default 262144), 3 initial connection
  window (default 1048576), 4 idle timeout ms (default 30000). Unknown ids are ignored.

## 3. Packets
- Header (always plaintext, always part of the AEAD's associated data): type u8 (1 Initial,
  2 Response, 3 Data), version u8 (1), `cid` u64, packet number u64. 18 bytes.
- Data packet: header, then `seal(key of this direction, nonce(pn), aad = header, frames)`.
  `nonce(pn)` = 4 zero bytes then pn as u64 little-endian. Each direction counts its packet
  numbers from 0 (the Response is server pn 0), strictly increasing, never reused; a connection
  closes before 2^62.
- The receiver drops silently (no reply, no state change) any packet that fails to open, has an
  unknown `cid`, a wrong version or type, or a packet number already seen or older than the 1024
  most recent. Hostile bytes never panic.
- Frames (a sequence; each starts with its type byte):
  `0x00` PADDING (the rest of the plaintext is zero) · `0x01` PING ·
  `0x02` ACK: largest acked varint, ack delay µs varint, range count varint, first range length
  varint, then (gap varint, length varint) per extra range, at most 32 ranges, as QUIC ·
  `0x03` STREAM: flags u8 (bit 0 = fin), stream id varint, offset varint, length varint, data ·
  `0x04` MAX_DATA varint · `0x05` MAX_STREAM_DATA stream id varint, limit varint ·
  `0x06` RESET_STREAM stream id varint, code varint, final size varint ·
  `0x07` CLOSE code varint, reason length varint, UTF-8 reason.
  An unknown frame type or a malformed frame closes the connection with code 1 (protocol error).

## 4. Streams and flow control
- Bidirectional, ordered, reliable byte streams. The client opens even ids, the server odd ids,
  each side in increasing order; opening beyond the peer's max open streams is a protocol error.
- `fin` marks the final size; data past it, or a changed final size, is a protocol error.
  `RESET_STREAM` abandons a stream in one direction.
- Credit-based flow control per stream and per connection, in bytes of offset. A receiver sends
  `MAX_*` when the application has read half its window; a sender never exceeds the limits.

## 5. Loss recovery (after RFC 9002)
- Packets with any frame other than ACK and PADDING are ack-eliciting. The receiver acks after 2
  ack-eliciting packets or 25 ms (max ack delay), whichever comes first.
- RTT: `latest_rtt` from the newest acked packet minus ack delay (not below min_rtt);
  `srtt`, `rttvar` as RFC 6298; initial `srtt` 100 ms.
- A packet is lost when a packet at least 3 numbers higher is acked, or when it was sent more
  than `9/8 * max(srtt, latest_rtt)` before a later acked packet. Lost frames are sent again in
  new packets; packets are never resent as they were.
- Probe timeout `PTO = srtt + max(4 * rttvar, 1 ms) + 25 ms`, doubled after each consecutive
  expiry; on expiry send one or two ack-eliciting packets (new data or PING).
- Idle timeout: the smaller of the two params; then the connection is gone with no packet.

## 6. Congestion control (NewReno, bytes)
Initial window 12000 (10 * 1200), minimum 2400. Slow start until the first loss; on a loss,
`window = max(window / 2, 2400)` once per recovery period (losses of packets sent before the
period started do not cut again); then congestion avoidance adds `1200 * acked / window` per ack.
Persistent congestion (all packets lost across `3 * PTO`) resets the window to the minimum. Bytes
in flight never exceed the window, except one probe packet on PTO.

## 7. Closing and errors
`CLOSE` ends the connection; the sender keeps answering with the same CLOSE for `3 * PTO`
(draining), the receiver stops sending at once. Codes: 0 no error, 1 protocol error, 2 flow
control, 3 stream limit, 4 application (reason from the app). `Error`: `Timeout`, `Closed { code,
reason }`, `Protocol(&'static str)`, `FlowControl`, `StreamLimit`, `TooLarge`.

## 8. What the leader will check
The simulator in T34: two endpoints through a channel with loss, reordering, duplication and
corruption each up to 30 percent; every stream arrives byte-exact or the connection reports an
error, never wrong data. Plus: known-answer handshake keys from a Python model of section 2,
a wrong server key never connects, replayed and corrupted packets change nothing, the window
never exceeds its limit, and fuzzed datagrams never panic. T35: a 100 MB file over a lossy local
link arrives with the same hash.
