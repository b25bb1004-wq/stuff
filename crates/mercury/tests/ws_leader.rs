//! Leader check for T38: Python as the client. http.client for chunked streams (and a raw
//! HTTP/1.0 read), range requests and chunked request bodies; a raw-socket WebSocket client
//! written from RFC 6455 for handshake, echo (7/16/64-bit lengths), fragmentation with an
//! interleaved ping, close handshake, every protocol error close code and bad handshakes.
//! Plus RFC vectors and decode fuzz in Rust.
#![cfg(feature = "std")]
#![allow(clippy::unwrap_used)]
use assay::Rng;
use mercury::server::{Config, Router, Server};
use mercury::ws::{accept_key, decode_frame, encode_frame, Frame, Message};
use mercury::Response;
use std::process::Command;

const PY: &str = r#"
import sys, socket, os, base64, hashlib, struct, http.client
port = int(sys.argv[1]); data = open(sys.argv[2], "rb").read(); L = len(data)
def ok(name, cond, detail=""): print(("OK " if cond else "FAIL ") + name + ("" if cond else " " + repr(detail)[:400]), flush=True)
count = b"".join(b"%d\n" % i for i in range(100))
def get(path, headers={}, method="GET", body=None, chunked=False):
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
    c.request(method, path, body=body, headers=headers, encode_chunked=chunked)
    r = c.getresponse(); b = r.read(); c.close(); return r, b
r, b = get("/count")
ok("stream chunked", r.status == 200 and (r.getheader("Transfer-Encoding") or "").lower() == "chunked" and b == count, (r.status, r.getheaders(), b[:60]))
s = socket.create_connection(("127.0.0.1", port), 5); s.sendall(b"GET /count HTTP/1.0\r\n\r\n"); buf = b""
while True:
    d = s.recv(65536)
    if not d: break
    buf += d
head, _, body = buf.partition(b"\r\n\r\n")
ok("stream http/1.0 close-delimited", b"chunked" not in head.lower() and body == count, buf[:200])
r, b = get("/body", {"Content-Type": "x"}, "POST", iter([b"abc", b"", b"defgh", b"x" * 5000]), True)
ok("chunked request body", r.status == 200 and b == b"abcdefgh" + b"x" * 5000, (r.status, b[:40]))
r, b = get("/files/blob.bin")
ok("accept-ranges", r.status == 200 and r.getheader("Accept-Ranges") == "bytes" and b == data, (r.status, r.getheaders()))
for rng, st, sl in [("bytes=0-99", 206, (0, 100)), ("bytes=100-", 206, (100, L)), ("bytes=-50", 206, (L - 50, L)),
                    ("bytes=9990-20000", 206, (9990, L)), ("bytes=0-0", 206, (0, 1)), (f"bytes={L-1}-", 206, (L - 1, L)),
                    ("bytes=20000-", 416, None), ("bytes=-0", 416, None), ("bytes=0-1,5-6", 200, (0, L)),
                    ("chars=0-5", 200, (0, L)), ("bytes=5-1", 200, (0, L)), ("bytes=x-y", 200, (0, L))]:
    r, b = get("/files/blob.bin", {"Range": rng})
    cr = r.getheader("Content-Range")
    if st == 206: want_cr = f"bytes {sl[0]}-{sl[1]-1}/{L}"
    elif st == 416: want_cr = f"bytes */{L}"
    else: want_cr = None
    good = r.status == st and cr == want_cr and (sl is None or b == data[sl[0]:sl[1]])
    ok(f"range {rng}", good, (r.status, cr, len(b)))
r, b = get("/files/blob.bin", {"Range": "bytes=10-19"}, "HEAD")
ok("range HEAD", r.status == 206 and b == b"" and r.getheader("Content-Range") == f"bytes 10-19/{L}", (r.status, r.getheaders()))
GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
def connect(path="/echo", key=None, version="13", upgrade="WebSocket"):
    s = socket.create_connection(("127.0.0.1", port), 5)
    key = key if key is not None else base64.b64encode(os.urandom(16)).decode()
    req = f"GET {path} HTTP/1.1\r\nHost: x\r\nUpgrade: {upgrade}\r\nConnection: keep-alive, Upgrade\r\n"
    if key: req += f"Sec-WebSocket-Key: {key}\r\n"
    req += f"Sec-WebSocket-Version: {version}\r\n\r\n"
    s.sendall(req.encode()); buf = b""
    while b"\r\n\r\n" not in buf:
        d = s.recv(4096)
        if not d: break
        buf += d
    head, _, rest = buf.partition(b"\r\n\r\n")
    return s, head.decode(errors="replace"), rest, key
def frame(op, payload, fin=True, mask=True, rsv=0, fake_len=None):
    n = len(payload) if fake_len is None else fake_len; mb = 0x80 if mask else 0
    hdr = bytes([(0x80 if fin else 0) | rsv | op])
    hdr += bytes([mb | n]) if n < 126 else (bytes([mb | 126]) + struct.pack(">H", n) if n < 65536 else bytes([mb | 127]) + struct.pack(">Q", n))
    if not mask: return hdr + payload
    m = os.urandom(4); return hdr + m + bytes(p ^ m[i % 4] for i, p in enumerate(payload))
class Reader:
    def __init__(self, s, buf): self.s, self.buf = s, buf
    def need(self, k):
        while len(self.buf) < k:
            d = self.s.recv(1 << 20)
            if not d: raise EOFError
            self.buf += d
    def frame(self):
        self.need(2); b0, b1 = self.buf[0], self.buf[1]; n = b1 & 127; off = 2
        if n == 126: self.need(4); n = struct.unpack(">H", self.buf[2:4])[0]; off = 4
        elif n == 127: self.need(10); n = struct.unpack(">Q", self.buf[2:10])[0]; off = 10
        self.need(off + n); p = self.buf[off:off + n]; self.buf = self.buf[off + n:]
        return (b0 >> 7, b0 & 15, p, b1 >> 7)
    def eof(self):
        try:
            self.s.settimeout(3); return self.s.recv(1) == b""
        except Exception: return False
s, head, rest, key = connect()
want = base64.b64encode(hashlib.sha1((key + GUID).encode()).digest()).decode()
ok("handshake 101", head.startswith("HTTP/1.1 101") and f"sec-websocket-accept: {want}" in head.lower().replace(want.lower(), want), head)
R = Reader(s, rest)
for name, op, p in [("text", 1, "héllo".encode()), ("binary 200", 2, os.urandom(200)), ("binary 70000", 2, os.urandom(70000)), ("empty text", 1, b"")]:
    s.sendall(frame(op, p)); f = R.frame()
    ok(f"echo {name}", f == (1, op, p, 0), (f[0], f[1], len(f[2]), f[3]))
s.sendall(frame(1, b"Hel", fin=False) + frame(9, b"pp") + frame(0, b"lo"))
f1 = R.frame(); f2 = R.frame()
ok("ping inside fragments", f1 == (1, 10, b"pp", 0) and f2 == (1, 1, b"Hello", 0), (f1, f2))
s.sendall(frame(10, b"unsolicited") + frame(1, b"after"))
f = R.frame(); ok("pong ignored", f == (1, 1, b"after", 0), f)
s.sendall(frame(8, struct.pack(">H", 1000) + b"bye")); f = R.frame()
ok("close echo", f[1] == 8 and f[2][:2] == struct.pack(">H", 1000) and R.eof(), f)
def expect_close(name, payload, code):
    s, head, rest, _ = connect(); R = Reader(s, rest)
    try:
        s.sendall(payload); f = R.frame()
        ok(f"close {code} {name}", f[1] == 8 and f[2][:2] == struct.pack(">H", code) and R.eof(), f)
    except Exception as e: ok(f"close {code} {name}", False, e)
expect_close("unmasked", frame(1, b"hi", mask=False), 1002)
expect_close("rsv1", frame(1, b"hi", rsv=0x40), 1002)
expect_close("opcode 3", frame(3, b"hi"), 1002)
expect_close("fragmented ping", frame(9, b"p", fin=False), 1002)
expect_close("ping 126 bytes", frame(9, b"p" * 126), 1002)
expect_close("lone continuation", frame(0, b"x"), 1002)
expect_close("data inside fragments", frame(1, b"a", fin=False) + frame(1, b"b"), 1002)
expect_close("bad utf-8", frame(1, b"ok\xff"), 1007)
expect_close("bad utf-8 across fragments", frame(1, b"\xe2\x82", fin=False) + frame(0, b"x"), 1007)
expect_close("too big", frame(2, b"z" * 1024, fake_len=(16 << 20) + 1)[:1024 + 14], 1009)
for name, kw, st in [("version 8", {"version": "8"}, "426"), ("no key", {"key": ""}, "400"), ("short key", {"key": "YWJj"}, "400"),
                     ("not websocket", {"upgrade": "h2c"}, "400")]:
    s, head, _, _ = connect(**kw)
    good = head.startswith("HTTP/1.1 " + st) and (st != "426" or "sec-websocket-version: 13" in head.lower())
    ok(f"handshake {name} -> {st}", good, head)
r, b = get("/echo"); ok("plain GET on ws route -> 400", r.status == 400, r.status)
"#;

fn resp(status: u16, body: &[u8]) -> Response {
    Response {
        minor: 1,
        status,
        reason: "OK".into(),
        headers: vec![("Content-Length".into(), body.len().to_string().into_bytes())],
        body: body.to_vec(),
    }
}

#[test]
fn leader_python_client() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("t38");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("www")).unwrap();
    let mut r = Rng::new(0x38);
    let blob: Vec<u8> = (0..10_000).map(|_| r.below(256) as u8).collect();
    std::fs::write(dir.join("www/blob.bin"), &blob).unwrap();
    std::fs::write(dir.join("client.py"), PY).unwrap();
    let mut router = Router::new();
    router.static_dir("/files", dir.join("www"));
    router.route("POST", "/body", Box::new(|q| resp(200, &q.body)));
    router.stream(
        "GET",
        "/count",
        Box::new(|_, w| {
            for i in 0..100 {
                if w(format!("{i}\n").as_bytes()).is_err() || w(b"").is_err() {
                    break;
                }
            }
            200
        }),
    );
    router.websocket(
        "/echo",
        Box::new(|mut ws| {
            while let Ok(m) = ws.recv() {
                match m {
                    Message::Close(_) => break,
                    other => {
                        if ws.send(&other).is_err() {
                            break;
                        }
                    }
                }
            }
        }),
    );
    let srv = Server::start(Config::default(), router).unwrap();
    let o = Command::new("python")
        .arg(dir.join("client.py"))
        .arg(srv.local_addr().port().to_string())
        .arg(dir.join("www/blob.bin"))
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let fails: Vec<&str> = out.lines().filter(|l| !l.starts_with("OK ")).collect();
    let oks = out.lines().filter(|l| l.starts_with("OK ")).count();
    assert!(
        fails.is_empty() && oks == 40 && o.status.success(),
        "{oks} OK, failures:\n{}\nstderr: {}",
        fails.join("\n"),
        String::from_utf8_lossy(&o.stderr)
    );
    srv.stop();
}

#[test]
fn leader_rfc_vectors_and_fuzz() {
    assert_eq!(
        accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
    );
    let hello = Frame {
        fin: true,
        opcode: 1,
        payload: b"Hello".to_vec(),
    };
    assert_eq!(
        encode_frame(&hello, None),
        [0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f]
    );
    let masked = [
        0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58,
    ];
    assert_eq!(encode_frame(&hello, Some([0x37, 0xfa, 0x21, 0x3d])), masked);
    assert_eq!(
        decode_frame(&masked, true, 1 << 20),
        Ok(Some((hello.clone(), 11)))
    );
    assert!(decode_frame(&masked[..6], true, 1 << 20) == Ok(None));
    assert!(decode_frame(&[0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f], true, 1 << 20).is_err());
    let big = Frame {
        fin: true,
        opcode: 2,
        payload: vec![7; 65536],
    };
    assert_eq!(
        &encode_frame(&big, None)[..10],
        &[0x82, 0x7f, 0, 0, 0, 0, 0, 1, 0, 0]
    );
    let mut r = Rng::new(0x38_f022);
    for _ in 0..20_000 {
        let n = r.below(40);
        let mut b: Vec<u8> = (0..n).map(|_| r.below(256) as u8).collect();
        if n > 2 && r.chance(1, 2) {
            b[1] = (b[1] & 0x80) | 127; // huge 64-bit lengths
        }
        let masked = r.chance(1, 2);
        let res = std::panic::catch_unwind(|| decode_frame(&b, masked, 1 << 16));
        assert!(res.is_ok(), "decode_frame panicked on {b:?}");
    }
}
