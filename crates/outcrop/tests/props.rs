//! Property, fuzz, prefix, and limit tests for outcrop.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use outcrop::{
    parse_request, parse_response, write_request, write_response, Error, Limits, Request, Response,
};

#[test]
fn every_prefix_is_incomplete() {
    let lim = Limits::default();
    let reqs: &[&[u8]] = &[
        b"GET / HTTP/1.1\r\nHost: a\r\n\r\n",
        b"POST /x HTTP/1.0\nContent-Length: 5\n\nhello",
        b"PUT /c HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3;x=1\r\nabc\r\n0\r\nX-T: 1\r\n\r\n",
    ];
    for msg in reqs {
        for len in 0..msg.len() {
            assert_eq!(
                parse_request(&msg[..len], &lim),
                Err(Error::Incomplete),
                "prefix len {len} of req"
            );
        }
        assert!(parse_request(msg, &lim).is_ok());
    }

    let resps: &[(&[u8], bool)] = &[
        (b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody", false),
        (b"HTTP/1.0 204\n\n", false),
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhi\r\n0\r\n\r\n",
            false,
        ),
        (b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\n", true),
    ];
    for &(msg, head_only) in resps {
        for len in 0..msg.len() {
            assert_eq!(
                parse_response(&msg[..len], &lim, head_only),
                Err(Error::Incomplete),
                "prefix len {len} of resp"
            );
        }
        assert!(parse_response(msg, &lim, head_only).is_ok());
    }
}

#[test]
fn limits_enforced_before_allocating() {
    let lim = Limits {
        max_head: 24,
        max_headers: 2,
        max_body: 5,
    };

    // HeadTooLarge as soon as max_head bytes arrive without blank line
    assert_eq!(
        parse_request(b"GET /very-long-path-here HTTP/1.1\r\n\r\n", &lim),
        Err(Error::HeadTooLarge)
    );
    assert_eq!(
        parse_request(b"012345678901234567890123", &lim),
        Err(Error::HeadTooLarge)
    );
    assert_eq!(
        parse_response(b"HTTP/1.1 200 OK\r\nX-Long: 123456\r\n\r\n", &lim, false),
        Err(Error::HeadTooLarge)
    );

    // TooManyHeaders
    let big_head_lim = Limits {
        max_head: 1024,
        max_headers: 2,
        max_body: 5,
    };
    assert_eq!(
        parse_request(
            b"GET / HTTP/1.1\r\nA: 1\r\nB: 2\r\nC: 3\r\n\r\n",
            &big_head_lim
        ),
        Err(Error::TooManyHeaders)
    );

    // BodyTooLarge on Content-Length without needing the body bytes
    assert_eq!(
        parse_request(
            b"POST / HTTP/1.1\r\nContent-Length: 6\r\n\r\n",
            &big_head_lim
        ),
        Err(Error::BodyTooLarge)
    );
    assert_eq!(
        parse_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\n",
            &big_head_lim,
            false
        ),
        Err(Error::BodyTooLarge)
    );

    // BodyTooLarge on chunked size without needing chunk data
    assert_eq!(
        parse_request(
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n6\r\n",
            &big_head_lim
        ),
        Err(Error::BodyTooLarge)
    );
    // Cumulative chunked body > max_body
    assert_eq!(
        parse_request(
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n3\r\n",
            &big_head_lim
        ),
        Err(Error::BodyTooLarge)
    );
}

#[test]
fn header_lookup_and_keep_alive() {
    let lim = Limits::default();
    let (req, _) = parse_request(
        b"GET / HTTP/1.1\r\nX-Custom: first\r\nx-custom: second\r\nConnection: keep-alive\r\n\r\n",
        &lim,
    )
    .unwrap();
    assert_eq!(req.header("x-custom"), Some(b"first".as_slice()));
    assert_eq!(req.header("X-CUSTOM"), Some(b"first".as_slice()));
    assert_eq!(req.header("missing"), None);
    assert!(req.keep_alive());

    let (req_close, _) =
        parse_request(b"GET / HTTP/1.1\r\nConnection: foo, Close\r\n\r\n", &lim).unwrap();
    assert!(!req_close.keep_alive());

    let (req_10_default, _) = parse_request(b"GET / HTTP/1.0\r\n\r\n", &lim).unwrap();
    assert!(!req_10_default.keep_alive());

    let (req_10_ka, _) =
        parse_request(b"GET / HTTP/1.0\r\nConnection: Keep-Alive\r\n\r\n", &lim).unwrap();
    assert!(req_10_ka.keep_alive());

    let (resp, _) = parse_response(
        b"HTTP/1.1 200 OK\r\nServer: outcrop\r\nContent-Length: 0\r\n\r\n",
        &lim,
        false,
    )
    .unwrap();
    assert_eq!(resp.header("server"), Some(b"outcrop".as_slice()));
}

fn gen_token(r: &mut Rng, min_len: usize, max_len: usize) -> String {
    const TCHARS: &[u8] =
        b"!#$%&'*+-.^_`|~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let len = min_len + r.below(max_len - min_len + 1);
    (0..len).map(|_| *r.pick(TCHARS) as char).collect()
}

fn gen_vchar_str(r: &mut Rng, min_len: usize, max_len: usize) -> String {
    let len = min_len + r.below(max_len - min_len + 1);
    (0..len)
        .map(|_| (0x21 + r.below(0x7E - 0x21 + 1)) as u8 as char)
        .collect()
}

fn gen_headers(r: &mut Rng, max_count: usize) -> Vec<(String, Vec<u8>)> {
    let count = r.below(max_count + 1);
    let mut headers = Vec::with_capacity(count);
    for _ in 0..count {
        let mut name = gen_token(r, 1, 12);
        if name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("transfer-encoding")
        {
            name.push_str("-x");
        }
        let val_len = r.below(24);
        let mut val = Vec::with_capacity(val_len);
        for i in 0..val_len {
            // Avoid leading/trailing whitespace so trim_ows is an identity
            let b = if i == 0 || i + 1 == val_len {
                (0x21 + r.below(0x7E - 0x21 + 1)) as u8
            } else {
                (0x20 + r.below(0x7E - 0x20 + 1)) as u8
            };
            val.push(b);
        }
        headers.push((name, val));
    }
    headers
}

#[test]
fn round_trip_requests_and_responses() {
    let lim = Limits::default();
    forall(500, 0x5EED_0001, |r| {
        let minor = r.below(2) as u8;
        let mut req = Request {
            method: gen_token(r, 1, 8),
            target: format!("/{}", gen_vchar_str(r, 0, 20)),
            minor,
            headers: gen_headers(r, 6),
            body: r.bytes(128),
        };
        if !req.body.is_empty() {
            req.headers.push((
                "Content-Length".into(),
                req.body.len().to_string().into_bytes(),
            ));
        }
        let wire = write_request(&req);
        let (parsed, used) = parse_request(&wire, &lim).unwrap();
        assert_eq!(used, wire.len());
        assert_eq!(parsed, req);

        let status = loop {
            let s = 200 + r.below(400) as u16;
            if s != 204 && s != 304 {
                break s;
            }
        };
        let mut resp = Response {
            minor,
            status,
            reason: if r.chance(1, 4) {
                String::new()
            } else {
                gen_token(r, 1, 10)
            },
            headers: gen_headers(r, 6),
            body: r.bytes(128),
        };
        // Response without Content-Length and non-empty body needs Content-Length to parse
        resp.headers.push((
            "Content-Length".into(),
            resp.body.len().to_string().into_bytes(),
        ));
        let rwire = write_response(&resp);
        let (rparsed, rused) = parse_response(&rwire, &lim, false).unwrap();
        assert_eq!(rused, rwire.len());
        assert_eq!(rparsed, resp);
    });
}

#[test]
fn fuzz_no_panic_and_bounded_bytes_used() {
    let lim = Limits::default();
    let seeds: &[&[u8]] = &[
        b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n",
        b"POST /up HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\r\n0\r\n\r\n",
    ];
    forall(2000, 0x5EED_0002, |r| {
        let buf = if r.chance(1, 2) {
            r.bytes(256)
        } else {
            let mut v = (*r.pick(seeds)).to_vec();
            let mutations = 1 + r.below(4);
            for _ in 0..mutations {
                if !v.is_empty() && r.chance(1, 2) {
                    let idx = r.below(v.len());
                    v[idx] = r.u64() as u8;
                } else if r.chance(1, 2) {
                    let idx = r.below(v.len() + 1);
                    v.insert(idx, r.u64() as u8);
                } else if !v.is_empty() {
                    let idx = r.below(v.len());
                    v.remove(idx);
                }
            }
            v
        };

        if let Ok((_, used)) = parse_request(&buf, &lim) {
            assert!(used <= buf.len());
        }
        if let Ok((_, used)) = parse_response(&buf, &lim, false) {
            assert!(used <= buf.len());
        }
        if let Ok((_, used)) = parse_response(&buf, &lim, true) {
            assert!(used <= buf.len());
        }
    });
}
