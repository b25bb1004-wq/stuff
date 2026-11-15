# tremor

tremor is an encrypted, reliable, multi-stream transport protocol, written as a
sans-I/O core. The `Connection` and `Server` types take datagrams in and return
datagrams out, with the caller supplying the time, so the protocol logic makes no
socket calls and reads no clock. A UDP driver module sits on top for the common
cases: an echo client and server, and reliable file transfer.

Why it exists: a transport that does no I/O of its own can be tested
deterministically, with simulated loss and time, before it is wired to a real
socket.

## Features

- Encrypted packets, with a handshake that uses X25519 keys from a server public key.
- Multiple bidirectional streams per connection, opened with `open_stream` and
  accepted with `accept_stream`.
- Per-stream and connection-wide flow control windows.
- Loss detection and retransmission, with a congestion window and an RTT estimate
  reported through `stats`.
- Idle timeout and a limit on half-open connections on the server.
- `Lossy` simulation (drop, duplicate and delay per mille, seeded) for the UDP driver.
- UDP helpers: `echo`, `echo_server`, `send_file` and `recv_file`. File transfer
  checks a SHA-256 hash at the end.

## Public API

| Item | Purpose |
| --- | --- |
| `Connection::connect(key, seed, config, now)` | Start a client connection to a server public key. |
| `Connection::handle`, `poll_transmit`, `timeout`, `on_timeout` | Feed datagrams in, take datagrams out, run timers. |
| `open_stream`, `accept_stream`, `write`, `read`, `finish`, `reset` | Stream I/O. |
| `is_connected`, `is_closed`, `error`, `close`, `stats`, `cid` | Connection state and metrics. |
| `Server::new(secret, seed, config)`, `public_key` | Create a server endpoint. |
| `Server::handle`, `connection`, `accept`, `cids`, `timeout`, `on_timeout` | Route datagrams to connections. |
| `echo_server`, `echo` | UDP echo server and client. |
| `send_file`, `recv_file` | UDP file transfer. |
| `Config`, `Stats`, `Lossy`, `Error`, `UdpError`, `MAX_DATAGRAM` | Settings, metrics and error types. |

## Demo

```
cargo run -p tremor --example demo
```

Output (program stdout):

```
handshake connected: true after 1 rounds
server accepted connection 8343168226643167888
client wrote 17 of 17 bytes on stream 0
server saw stream 0
server read 17 bytes: "hello over tremor"
client read back 17 bytes: "hello over tremor"
echo matches: true
client sent 3 packets, cwnd 12062 bytes
```

The demo runs the client and server in one process and passes datagrams by hand
with `poll_transmit` and `handle`. Fixed seeds make the connection id and the
counters the same on every run. The handshake shows as connected after one
round, and the server then accepts the connection a few rounds later.

## Tests

```
cargo test -p tremor
```
