//! Corpus tests for outcrop HTTP/1.1 parser.
#![allow(clippy::unwrap_used)]

use outcrop::{parse_request, parse_response, Error, Limits, Request, Response};

type ReqCase<'a> = (&'a [u8], Result<(Request, usize), Error>);
type RespCase<'a> = (&'a [u8], bool, Result<(Response, usize), Error>);

fn h(pairs: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
    pairs
        .iter()
        .map(|&(k, v)| (k.to_string(), v.to_vec()))
        .collect()
}

#[test]
fn request_corpus() {
    let lim = Limits::default();
    let cases: Vec<ReqCase<'_>> = vec![
        // 1. Basic GET CRLF
        (
            b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n",
            Ok((
                Request {
                    method: "GET".into(),
                    target: "/".into(),
                    minor: 1,
                    headers: h(&[("Host", b"example.com")]),
                    body: vec![],
                },
                37,
            )),
        ),
        // 2. Basic GET bare LF
        (
            b"GET /a/b?c=1 HTTP/1.0\nHost: x\n\n",
            Ok((
                Request {
                    method: "GET".into(),
                    target: "/a/b?c=1".into(),
                    minor: 0,
                    headers: h(&[("Host", b"x")]),
                    body: vec![],
                },
                31,
            )),
        ),
        // 3. No headers
        (
            b"HEAD /health HTTP/1.1\r\n\r\n",
            Ok((
                Request {
                    method: "HEAD".into(),
                    target: "/health".into(),
                    minor: 1,
                    headers: vec![],
                    body: vec![],
                },
                25,
            )),
        ),
        // 4. Custom token method
        (
            b"MKCOL!#$ /dav HTTP/1.1\r\n\r\n",
            Ok((
                Request {
                    method: "MKCOL!#$".into(),
                    target: "/dav".into(),
                    minor: 1,
                    headers: vec![],
                    body: vec![],
                },
                26,
            )),
        ),
        // 5. Header value trimming spaces and tabs
        (
            b"GET / HTTP/1.1\r\nX-Test: \t hello world \t \r\n\r\n",
            Ok((
                Request {
                    method: "GET".into(),
                    target: "/".into(),
                    minor: 1,
                    headers: h(&[("X-Test", b"hello world")]),
                    body: vec![],
                },
                44,
            )),
        ),
        // 6. Empty header value
        (
            b"GET / HTTP/1.1\r\nX-Empty:\r\n\r\n",
            Ok((
                Request {
                    method: "GET".into(),
                    target: "/".into(),
                    minor: 1,
                    headers: h(&[("X-Empty", b"")]),
                    body: vec![],
                },
                28,
            )),
        ),
        // 7. POST with Content-Length
        (
            b"POST /submit HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello",
            Ok((
                Request {
                    method: "POST".into(),
                    target: "/submit".into(),
                    minor: 1,
                    headers: h(&[("Content-Length", b"5")]),
                    body: b"hello".to_vec(),
                },
                49,
            )),
        ),
        // 8. Content-Length: 0
        (
            b"POST /empty HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            Ok((
                Request {
                    method: "POST".into(),
                    target: "/empty".into(),
                    minor: 1,
                    headers: h(&[("Content-Length", b"0")]),
                    body: vec![],
                },
                43,
            )),
        ),
        // 9. Duplicate identical Content-Length
        (
            b"POST /dup HTTP/1.1\r\nContent-Length: 3\r\nContent-Length: 3\r\n\r\nabc",
            Ok((
                Request {
                    method: "POST".into(),
                    target: "/dup".into(),
                    minor: 1,
                    headers: h(&[("Content-Length", b"3"), ("Content-Length", b"3")]),
                    body: b"abc".to_vec(),
                },
                63,
            )),
        ),
        // 10. Comma-separated identical Content-Length
        (
            b"POST /c HTTP/1.1\r\nContent-Length: 2, 2\r\n\r\nhi",
            Ok((
                Request {
                    method: "POST".into(),
                    target: "/c".into(),
                    minor: 1,
                    headers: h(&[("Content-Length", b"2, 2")]),
                    body: b"hi".to_vec(),
                },
                44,
            )),
        ),
        // 11. Chunked basic
        (
            b"POST /up HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
            Ok((
                Request {
                    method: "POST".into(),
                    target: "/up".into(),
                    minor: 1,
                    headers: h(&[("Transfer-Encoding", b"chunked")]),
                    body: b"hello".to_vec(),
                },
                64,
            )),
        ),
        // 12. Chunked with extensions, uppercase hex, bare LF, and trailers
        (
            b"PUT /u HTTP/1.1\nTransfer-Encoding: CHUNKED\n\nA;ext=1;foo=\"bar\"\n0123456789\n0 ;last=yes\nX-Trailer: val\n\n",
            Ok((
                Request {
                    method: "PUT".into(),
                    target: "/u".into(),
                    minor: 1,
                    headers: h(&[("Transfer-Encoding", b"CHUNKED")]),
                    body: b"0123456789".to_vec(),
                },
                101,
            )),
        ),
        // 13. CL + TE smuggling: TE chunked wins over Content-Length
        (
            b"POST /smug HTTP/1.1\r\nContent-Length: 100\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n\r\n",
            Ok((
                Request {
                    method: "POST".into(),
                    target: "/smug".into(),
                    minor: 1,
                    headers: h(&[("Content-Length", b"100"), ("Transfer-Encoding", b"chunked")]),
                    body: b"ok".to_vec(),
                },
                84,
            )),
        ),
        // 14. TE: gzip, chunked
        (
            b"POST /gz HTTP/1.1\r\nTransfer-Encoding: gzip, chunked\r\n\r\n0\r\n\r\n",
            Ok((
                Request {
                    method: "POST".into(),
                    target: "/gz".into(),
                    minor: 1,
                    headers: h(&[("Transfer-Encoding", b"gzip, chunked")]),
                    body: vec![],
                },
                60,
            )),
        ),
        // 15. Pipelined extra bytes left untouched
        (
            b"GET /1 HTTP/1.1\r\n\r\nGET /2 HTTP/1.1\r\n\r\n",
            Ok((
                Request {
                    method: "GET".into(),
                    target: "/1".into(),
                    minor: 1,
                    headers: vec![],
                    body: vec![],
                },
                19,
            )),
        ),
        // 16. Incomplete: empty
        (b"", Err(Error::Incomplete)),
        // 17. Incomplete: partial request line
        (b"GET / HTTP/1.1", Err(Error::Incomplete)),
        // 18. Incomplete: missing blank line
        (b"GET / HTTP/1.1\r\nHost: a\r\n", Err(Error::Incomplete)),
        // 19. Incomplete: trailing CR of blank line
        (b"GET / HTTP/1.1\r\n\r", Err(Error::Incomplete)),
        // 20. Incomplete: partial fixed body
        (b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nabc", Err(Error::Incomplete)),
        // 21. Incomplete: chunked missing terminator
        (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n", Err(Error::Incomplete)),
        // 22. Incomplete: chunked missing trailer blank line
        (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n", Err(Error::Incomplete)),
        // 23. BadMethod: empty method
        (b" / HTTP/1.1\r\n\r\n", Err(Error::BadMethod)),
        // 24. BadMethod: invalid char in method
        (b"G@T / HTTP/1.1\r\n\r\n", Err(Error::BadMethod)),
        // 25. BadRequestLine: empty line
        (b"\r\nGET / HTTP/1.1\r\n\r\n", Err(Error::BadRequestLine)),
        // 26. BadRequestLine: no spaces
        (b"GET\r\n\r\n", Err(Error::BadRequestLine)),
        // 27. BadRequestLine: only one space
        (b"GET /\r\n\r\n", Err(Error::BadRequestLine)),
        // 28. BadRequestLine: empty target
        (b"GET  HTTP/1.1\r\n\r\n", Err(Error::BadRequestLine)),
        // 29. BadRequestLine: control char in target
        (b"GET /\x07 HTTP/1.1\r\n\r\n", Err(Error::BadRequestLine)),
        // 30. BadVersion: HTTP/2.0
        (b"GET / HTTP/2.0\r\n\r\n", Err(Error::BadVersion)),
        // 31. BadVersion: HTTP/1.2
        (b"GET / HTTP/1.2\r\n\r\n", Err(Error::BadVersion)),
        // 32. BadVersion: HTTP/0.9
        (b"GET / HTTP/0.9\r\n\r\n", Err(Error::BadVersion)),
        // 33. BadHeader: missing colon
        (b"GET / HTTP/1.1\r\nNoColon\r\n\r\n", Err(Error::BadHeader)),
        // 34. BadHeader: space before colon
        (b"GET / HTTP/1.1\r\nHost : a\r\n\r\n", Err(Error::BadHeader)),
        // 35. BadHeader: empty name
        (b"GET / HTTP/1.1\r\n: val\r\n\r\n", Err(Error::BadHeader)),
        // 36. BadHeader: obs-fold space
        (b"GET / HTTP/1.1\r\nHost: a\r\n folded\r\n\r\n", Err(Error::BadHeader)),
        // 37. BadHeader: obs-fold tab
        (b"GET / HTTP/1.1\r\nHost: a\r\n\tfolded\r\n\r\n", Err(Error::BadHeader)),
        // 38. BadHeader: bare CR in value
        (b"GET / HTTP/1.1\r\nHost: a\rb\r\n\r\n", Err(Error::BadHeader)),
        // 39. BadContentLength: conflicting values
        (b"POST / HTTP/1.1\r\nContent-Length: 4\r\nContent-Length: 5\r\n\r\n", Err(Error::BadContentLength)),
        // 40. BadContentLength: non-digit
        (b"POST / HTTP/1.1\r\nContent-Length: 4a\r\n\r\n", Err(Error::BadContentLength)),
        // 41. BadContentLength: negative
        (b"POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n", Err(Error::BadContentLength)),
        // 42. BadChunk: non-hex
        (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nxyz\r\n", Err(Error::BadChunk)),
        // 43. BadChunk: size overflow
        (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffffffff\r\n", Err(Error::BadChunk)),
        // 44. BadChunk: missing CRLF after chunk data
        (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n1\r\naX0\r\n\r\n", Err(Error::BadChunk)),
    ];

    assert!(cases.len() >= 40);
    for (i, (input, expected)) in cases.iter().enumerate() {
        let got = parse_request(input, &lim);
        assert_eq!(&got, expected, "request case {i} failed");
    }

    // Verify pipelined pair parses back-to-back
    let pipe = b"GET /1 HTTP/1.1\r\n\r\nPOST /2 HTTP/1.1\r\nContent-Length: 2\r\n\r\nhi";
    let (r1, used1) = parse_request(pipe, &lim).unwrap();
    let (r2, used2) = parse_request(&pipe[used1..], &lim).unwrap();
    assert_eq!(r1.target, "/1");
    assert_eq!(r2.target, "/2");
    assert_eq!(r2.body, b"hi");
    assert_eq!(used1 + used2, pipe.len());
}

#[test]
fn response_corpus() {
    let lim = Limits::default();
    let cases: Vec<RespCase<'_>> = vec![
        // 1. Basic 200 OK with Content-Length
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi",
            false,
            Ok((
                Response {
                    minor: 1,
                    status: 200,
                    reason: "OK".into(),
                    headers: h(&[("Content-Length", b"2")]),
                    body: b"hi".to_vec(),
                },
                40,
            )),
        ),
        // 2. HTTP/1.0 with bare LF
        (
            b"HTTP/1.0 404 Not Found\nContent-Length: 0\n\n",
            false,
            Ok((
                Response {
                    minor: 0,
                    status: 404,
                    reason: "Not Found".into(),
                    headers: h(&[("Content-Length", b"0")]),
                    body: vec![],
                },
                42,
            )),
        ),
        // 3. Empty reason phrase without space
        (
            b"HTTP/1.1 204\r\n\r\n",
            false,
            Ok((
                Response {
                    minor: 1,
                    status: 204,
                    reason: String::new(),
                    headers: vec![],
                    body: vec![],
                },
                16,
            )),
        ),
        // 4. Empty reason phrase with space
        (
            b"HTTP/1.1 204 \r\n\r\n",
            false,
            Ok((
                Response {
                    minor: 1,
                    status: 204,
                    reason: String::new(),
                    headers: vec![],
                    body: vec![],
                },
                17,
            )),
        ),
        // 5. 304 Not Modified ignores Content-Length even if head_only=false
        (
            b"HTTP/1.1 304 Not Modified\r\nContent-Length: 10\r\n\r\n",
            false,
            Ok((
                Response {
                    minor: 1,
                    status: 304,
                    reason: "Not Modified".into(),
                    headers: h(&[("Content-Length", b"10")]),
                    body: vec![],
                },
                49,
            )),
        ),
        // 6. 100 Continue has no body
        (
            b"HTTP/1.1 100 Continue\r\n\r\n",
            false,
            Ok((
                Response {
                    minor: 1,
                    status: 100,
                    reason: "Continue".into(),
                    headers: vec![],
                    body: vec![],
                },
                25,
            )),
        ),
        // 7. HEAD reply (head_only=true) ignores Content-Length
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 999\r\n\r\n",
            true,
            Ok((
                Response {
                    minor: 1,
                    status: 200,
                    reason: "OK".into(),
                    headers: h(&[("Content-Length", b"999")]),
                    body: vec![],
                },
                40,
            )),
        ),
        // 8. Chunked response with extensions & trailers
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;a=b\r\nhey\r\n0\r\nX-Tr: 1\r\n\r\n",
            false,
            Ok((
                Response {
                    minor: 1,
                    status: 200,
                    reason: "OK".into(),
                    headers: h(&[("Transfer-Encoding", b"chunked")]),
                    body: b"hey".to_vec(),
                },
                73,
            )),
        ),
        // 9. CL + TE on response: TE wins
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\r\n0\r\n\r\n",
            false,
            Ok((
                Response {
                    minor: 1,
                    status: 200,
                    reason: "OK".into(),
                    headers: h(&[("Content-Length", b"50"), ("Transfer-Encoding", b"chunked")]),
                    body: b"x".to_vec(),
                },
                78,
            )),
        ),
        // 10. Pipelined response pair
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\naHTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nb",
            false,
            Ok((
                Response {
                    minor: 1,
                    status: 200,
                    reason: "OK".into(),
                    headers: h(&[("Content-Length", b"1")]),
                    body: b"a".to_vec(),
                },
                39,
            )),
        ),
        // 11. 200 OK without CL or TE and head_only=false returns Incomplete
        (b"HTTP/1.1 200 OK\r\n\r\nhello", false, Err(Error::Incomplete)),
        // 12. Empty input
        (b"", false, Err(Error::Incomplete)),
        // 13. Partial status line
        (b"HTTP/1.1 200", false, Err(Error::Incomplete)),
        // 14. BadVersion
        (b"HTTP/2.0 200 OK\r\n\r\n", false, Err(Error::BadVersion)),
        // 15. BadStatusLine: non-digit status
        (b"HTTP/1.1 20x OK\r\n\r\n", false, Err(Error::BadStatusLine)),
        // 16. BadStatusLine: 4-digit status
        (b"HTTP/1.1 2000 OK\r\n\r\n", false, Err(Error::BadStatusLine)),
        // 17. BadStatusLine: missing HTTP/ prefix
        (b"FOO/1.1 200 OK\r\n\r\n", false, Err(Error::BadStatusLine)),
        // 18. BadStatusLine: empty first line
        (b"\r\nHTTP/1.1 200 OK\r\n\r\n", false, Err(Error::BadStatusLine)),
        // 19. BadHeader: obs-fold
        (b"HTTP/1.1 200 OK\r\nA: b\r\n c\r\n\r\n", false, Err(Error::BadHeader)),
        // 20. BadContentLength: mismatch
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
            false,
            Err(Error::BadContentLength),
        ),
        // 21. BadChunk: bad hex
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n",
            false,
            Err(Error::BadChunk),
        ),
    ];

    assert!(cases.len() >= 20);
    for (i, (input, head_only, expected)) in cases.iter().enumerate() {
        let got = parse_response(input, &lim, *head_only);
        assert_eq!(&got, expected, "response case {i} failed");
    }
}
