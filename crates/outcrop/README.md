# outcrop

outcrop is Strata's HTTP/1.1 layer. Its core parses and writes HTTP messages without doing any I/O,
so the same code can sit behind any transport. The optional `std` feature (on by default) adds a
threaded server, a router and a WebSocket connection type, so a Strata program can serve files and
WebSocket clients with no external crates.

## Features

- Parse one HTTP/1.1 or HTTP/1.0 request or response from the front of a buffer; the byte count used is returned.
- Partial input is reported as `Error::Incomplete`, so a caller can read more and try again.
- Bodies by `Content-Length` or chunked transfer coding; `Limits` caps head size, header count and body size.
- Write requests and responses to wire bytes, adding `Content-Length` when the body needs it.
- The core is `#![no_std]` and uses `alloc`; `#![forbid(unsafe_code)]` holds throughout.
- Server: threaded on the geyser pool, keep-alive, pipelined requests, read timeouts, a per-connection
  request cap, `503` above `max_connections`, and a graceful `stop`.
- Router: exact paths, prefix routes ending in `*`, streamed responses, WebSocket upgrade routes and
  static directory mounts for `GET` and `HEAD`. Static files reject path traversal and honour single `Range` requests.
- WebSocket (RFC 6455): accept-key derivation with our own SHA-1 and Base64, frame encode and decode,
  and a `WebSocket` that answers pings, echoes close frames and reassembles fragments.

## Public API

| Item | What it does |
| --- | --- |
| `parse_request(buf, &Limits)` | Parse one request; returns `(Request, bytes_used)` |
| `parse_response(buf, &Limits, head_only)` | Parse one response; returns `(Response, bytes_used)` |
| `write_request(&Request)` / `write_response(&Response)` | Serialize to wire bytes |
| `Limits`, `Request`, `Response`, `Error` | Core types |
| `server::Config`, `server::Server` | Configure, start, inspect (`stats`, `local_addr`) and `stop` the server |
| `server::Router` | `route`, `websocket`, `stream`, `static_dir` |
| `ws::accept_key`, `ws::encode_frame`, `ws::decode_frame` | WebSocket key and frame helpers |
| `ws::WebSocket` (std only) | `recv` and `send` messages on an upgraded connection |

## Demo

```
cargo run -p outcrop --example demo
```

Real output:

```
parsed GET /hello?x=1 (HTTP/1.1), 44 of 44 bytes used
partial head gives: Err(Incomplete)
server bound to 127.0.0.1 on an ephemeral port
client got status 200 OK
body: you asked for /hello
server counted 1 request(s) on 1 connection(s)
server stopped
accept key for RFC 6455 example: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=
text frame "Hi" encodes to: 81 02 48 69
```

The demo binds 127.0.0.1 on port 0 and makes no outside connections.

## Tests

```
cargo test -p outcrop
```

The crate gate is `bash tools/check.sh outcrop` from the repository root.
