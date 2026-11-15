//! Security tests for mercury HTTP/1.1 parsing and serialization.

#![allow(clippy::unwrap_used)]

use assay::forall;
use mercury::{
    parse_request, parse_response, write_request, write_response, Error, Limits, Request, Response,
};

// --- Defects reproduced against unfixed src/ ---

#[test]
fn smuggling_non_chunked_transfer_encoding_with_content_length() {
    let lim = Limits::default();
    let obf_te_and_cl =
        b"POST / HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: gzip\r\nContent-Length: 5\r\n\r\nhello";
    assert_eq!(parse_request(obf_te_and_cl, &lim), Err(Error::BadHeader));

    let chunked_then_gzip_and_cl = b"POST / HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked, gzip\r\nContent-Length: 5\r\n\r\nhello";
    assert_eq!(
        parse_request(chunked_then_gzip_and_cl, &lim),
        Err(Error::BadHeader)
    );
}

#[test]
fn smuggling_non_chunked_transfer_encoding_response() {
    let lim = Limits::default();
    let resp = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\nContent-Length: 5\r\n\r\nhello";
    assert_eq!(parse_response(resp, &lim, false), Err(Error::BadHeader));

    let resp_dup = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked, chunked\r\n\r\n0\r\n\r\n";
    assert_eq!(parse_response(resp_dup, &lim, false), Err(Error::BadHeader));
}

#[test]
fn smuggling_non_chunked_transfer_encoding_request() {
    let lim = Limits::default();
    let smuggled = b"POST / HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: gzip\r\n\r\nGET /admin HTTP/1.1\r\nHost: a\r\n\r\n";
    assert_eq!(parse_request(smuggled, &lim), Err(Error::BadHeader));

    let chunked_not_last =
        b"POST / HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked, gzip\r\n\r\n0\r\n\r\n";
    assert_eq!(parse_request(chunked_not_last, &lim), Err(Error::BadHeader));

    let double_chunked =
        b"POST / HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked, chunked\r\n\r\n0\r\n\r\n";
    assert_eq!(parse_request(double_chunked, &lim), Err(Error::BadHeader));

    let empty_te = b"POST / HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: \r\n\r\n";
    assert_eq!(parse_request(empty_te, &lim), Err(Error::BadHeader));
}

#[test]
fn header_injection_crlf_in_written_header_value() {
    let lim = Limits::default();

    let resp_hdr = Response {
        minor: 1,
        status: 200,
        reason: "OK".to_string(),
        headers: vec![
            ("X-Test".to_string(), b"ok\r\nSet-Cookie: evil=1".to_vec()),
            ("Content-Length".to_string(), b"0".to_vec()),
        ],
        body: Vec::new(),
    };
    let wire_hdr = write_response(&resp_hdr);
    let (parsed_hdr, used_hdr) = parse_response(&wire_hdr, &lim, false).unwrap();
    assert_eq!(used_hdr, wire_hdr.len());
    assert!(parsed_hdr.header("set-cookie").is_none());

    let resp = Response {
        minor: 1,
        status: 200,
        reason: "OK".to_string(),
        headers: vec![
            (
                "X-Test".to_string(),
                b"ok\r\nSet-Cookie: evil=1\r\n\r\n<script>1</script>".to_vec(),
            ),
            ("Content-Length".to_string(), b"0".to_vec()),
        ],
        body: Vec::new(),
    };
    let wire = write_response(&resp);
    let (parsed, used) = parse_response(&wire, &lim, false).unwrap();
    assert_eq!(used, wire.len());
    assert!(parsed.header("set-cookie").is_none());
    assert!(parsed.body.is_empty());

    let req = Request {
        method: "GET".to_string(),
        target: "/".to_string(),
        minor: 1,
        headers: vec![("X-Test".to_string(), b"ok\nX-Admin: true".to_vec())],
        body: Vec::new(),
    };
    let req_wire = write_request(&req);
    let (parsed_req, req_used) = parse_request(&req_wire, &lim).unwrap();
    assert_eq!(req_used, req_wire.len());
    assert!(parsed_req.header("x-admin").is_none());
}

#[test]
fn header_injection_crlf_in_written_header_name() {
    let lim = Limits::default();
    let resp = Response {
        minor: 1,
        status: 200,
        reason: "OK".to_string(),
        headers: vec![
            ("X-Ok\r\nSet-Cookie".to_string(), b"evil=1".to_vec()),
            ("Bad: Name".to_string(), b"val".to_vec()),
            ("Content-Length".to_string(), b"0".to_vec()),
        ],
        body: Vec::new(),
    };
    let wire = write_response(&resp);
    let (parsed, used) = parse_response(&wire, &lim, false).unwrap();
    assert_eq!(used, wire.len());
    assert!(parsed.header("set-cookie").is_none());
    assert!(parsed.header("bad").is_none());
}

#[test]
fn header_injection_crlf_in_written_target_method_reason() {
    let lim = Limits::default();

    let req = Request {
        method: "GET /x HTTP/1.1\r\nX-Injected-M: 1\r\n\r\nPOST".to_string(),
        target: "/ HTTP/1.1\r\nX-Injected-T: 1\r\n\r\nGET /admin".to_string(),
        minor: 1,
        headers: vec![("Host".to_string(), b"example.com".to_vec())],
        body: Vec::new(),
    };
    let req_wire = write_request(&req);
    let (parsed_req, req_used) = parse_request(&req_wire, &lim).unwrap();
    assert_eq!(req_used, req_wire.len());
    assert!(parsed_req.header("x-injected-m").is_none());
    assert!(parsed_req.header("x-injected-t").is_none());

    let resp = Response {
        minor: 1,
        status: 200,
        reason: "OK\r\nSet-Cookie: evil=1\r\n\r\npwn".to_string(),
        headers: vec![("Content-Length".to_string(), b"0".to_vec())],
        body: Vec::new(),
    };
    let resp_wire = write_response(&resp);
    let (parsed_resp, resp_used) = parse_response(&resp_wire, &lim, false).unwrap();
    assert_eq!(resp_used, resp_wire.len());
    assert!(parsed_resp.header("set-cookie").is_none());
    assert!(parsed_resp.body.is_empty());
}

// --- Additional security & boundary attack tests ---

#[test]
fn content_length_negative_overflow_and_duplicates() {
    let lim = Limits::default();
    let cases: &[&[u8]] = &[
        b"POST / HTTP/1.1\r\nHost: a\r\nContent-Length: -1\r\n\r\n",
        b"POST / HTTP/1.1\r\nHost: a\r\nContent-Length: +5\r\n\r\nhello",
        b"POST / HTTP/1.1\r\nHost: a\r\nContent-Length: 5a\r\n\r\nhello",
        b"POST / HTTP/1.1\r\nHost: a\r\nContent-Length: \r\n\r\n",
        b"POST / HTTP/1.1\r\nHost: a\r\nContent-Length: 999999999999999999999999999\r\n\r\n",
        b"POST / HTTP/1.1\r\nHost: a\r\nContent-Length: 5\r\nContent-Length: 10\r\n\r\n0123456789",
        b"POST / HTTP/1.1\r\nHost: a\r\nContent-Length: 5, 10\r\n\r\n0123456789",
    ];
    for raw in cases {
        assert_eq!(parse_request(raw, &lim), Err(Error::BadContentLength));
        assert_eq!(
            parse_response(&[b"HTTP/1.1 200 OK\r\n", &raw[18..]].concat(), &lim, false),
            Err(Error::BadContentLength)
        );
    }
}

#[test]
fn chunk_size_overflow_and_malformed() {
    let lim = Limits::default();
    let bad_chunks: &[&[u8]] = &[
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nfffffffffffffffff\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n10000000000000000\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n-1\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n+1\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0x1\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n 1\r\na\r\n0\r\n\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n;ext\r\n0\r\n\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n1\r\naX0\r\n\r\n",
    ];
    for raw in bad_chunks {
        assert_eq!(parse_request(raw, &lim), Err(Error::BadChunk));
    }

    // Valid u64 size that exceeds max_body must fail before allocating or waiting for bytes.
    let huge = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n7fffffff\r\n";
    assert_eq!(parse_request(huge, &lim), Err(Error::BodyTooLarge));
}

#[test]
fn line_endings_bare_cr_rejected_and_bare_lf_safe() {
    let lim = Limits::default();
    let bare_cr_cases: &[&[u8]] = &[
        b"GET / HTTP/1.1\rHost: a\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: a\rX-Evil: b\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost\r: a\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: a\r\r\n\r\n",
        b"GET / HTTP/1.1\r\n\rHost: a\r\n\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n1\r;ext\r\na\r\n0\r\n\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r0\r\n\r\n",
    ];
    for raw in bare_cr_cases {
        assert!(parse_request(raw, &lim).is_err());
    }

    let bare_cr_resp = b"HTTP/1.1 200 OK\rX-Evil: 1\r\nContent-Length: 0\r\n\r\n";
    assert_eq!(
        parse_response(bare_cr_resp, &lim, false),
        Err(Error::BadStatusLine)
    );

    let bare_lf_req = b"POST / HTTP/1.1\nHost: a\nTransfer-Encoding: chunked\n\n2\nhi\n0\n\n";
    let (req, used) = parse_request(bare_lf_req, &lim).unwrap();
    assert_eq!(used, bare_lf_req.len());
    assert_eq!(req.body, b"hi");
}

#[test]
fn limits_enforcement_head_headers_body_and_trailers() {
    let lim = Limits {
        max_head: 40,
        max_headers: 2,
        max_body: 4,
    };

    // Head too large
    let big_head = b"GET /very-long-target-exceeding-limit HTTP/1.1\r\nHost: a\r\n\r\n";
    assert_eq!(parse_request(big_head, &lim), Err(Error::HeadTooLarge));

    // Too many headers
    let many_hdrs = b"GET / HTTP/1.1\r\nA: 1\r\nB: 2\r\nC: 3\r\n\r\n";
    assert_eq!(parse_request(many_hdrs, &lim), Err(Error::TooManyHeaders));

    // Body too large via Content-Length (without full body present)
    let cl_big = b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\n";
    assert_eq!(parse_request(cl_big, &lim), Err(Error::BodyTooLarge));

    // Body too large via cumulative chunked pieces
    let chunk_lim = Limits {
        max_head: 64,
        max_headers: 2,
        max_body: 4,
    };
    let chunk_big =
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
    assert_eq!(
        parse_request(chunk_big, &chunk_lim),
        Err(Error::BodyTooLarge)
    );

    // Trailer count limit
    let many_trailers =
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nA: 1\r\nB: 2\r\nC: 3\r\n\r\n";
    assert_eq!(
        parse_request(many_trailers, &chunk_lim),
        Err(Error::TooManyHeaders)
    );

    // Trailer head size limit
    let big_trailer = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-Very-Long-Trailer: 12345678901234567890123456789012345678901234567890\r\n\r\n";
    assert_eq!(
        parse_request(big_trailer, &chunk_lim),
        Err(Error::HeadTooLarge)
    );

    // Truncated input returns Incomplete, never panics
    let req = b"POST / HTTP/1.1\r\nContent-Length: 4\r\n\r\nabcd";
    for cut in 0..req.len() {
        assert_eq!(parse_request(&req[..cut], &lim), Err(Error::Incomplete));
    }
}

#[test]
fn obsolete_line_folding_and_whitespace_before_colon() {
    let lim = Limits::default();
    let fold = b"GET / HTTP/1.1\r\nHost: a\r\n folded: 1\r\n\r\n";
    assert_eq!(parse_request(fold, &lim), Err(Error::BadHeader));

    let space_before_colon = b"GET / HTTP/1.1\r\nHost : a\r\n\r\n";
    assert_eq!(
        parse_request(space_before_colon, &lim),
        Err(Error::BadHeader)
    );

    let nul_in_val = b"GET / HTTP/1.1\r\nHost: a\x00b\r\n\r\n";
    assert_eq!(parse_request(nul_in_val, &lim), Err(Error::BadHeader));
}

#[test]
fn fuzz_hostile_inputs_never_panic() {
    let lim = Limits {
        max_head: 256,
        max_headers: 8,
        max_body: 512,
    };
    forall(500, 0x5EC0_7171, |rng| {
        let data = rng.bytes(300);
        let _ = parse_request(&data, &lim);
        let _ = parse_response(&data, &lim, rng.chance(1, 2));

        let s = String::from_utf8_lossy(&data).into_owned();
        let req = Request {
            method: s.clone(),
            target: s.clone(),
            minor: 1,
            headers: vec![(s.clone(), data.clone())],
            body: data.clone(),
        };
        let rw = write_request(&req);
        if let Ok((pr, _)) = parse_request(&rw, &Limits::default()) {
            assert!(!pr.method.contains(['\r', '\n']));
            assert!(!pr.target.contains(['\r', '\n']));
        }

        let resp = Response {
            minor: 1,
            status: 200,
            reason: s.clone(),
            headers: vec![(s, data.clone())],
            body: data,
        };
        let rsw = write_response(&resp);
        if let Ok((pr, _)) = parse_response(&rsw, &Limits::default(), false) {
            assert!(!pr.reason.contains(['\r', '\n']));
        }
    });
}
