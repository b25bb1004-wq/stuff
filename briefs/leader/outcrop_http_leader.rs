//! Leader check for T36 (public API, fresh seed): smuggling cases, limits, split at every byte,
//! pipelining, keep-alive, and a mutation fuzz whose accepted messages must round trip.
#![allow(clippy::unwrap_used)]
use assay::forall;
use mercury::{
    parse_request, parse_response, write_request, write_response, Error, Limits, Request, Response,
};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn req(s: &str) -> Result<(Request, usize), Error> {
    parse_request(s.as_bytes(), &Limits::default())
}

#[test]
fn leader_smuggling_and_framing() {
    // Chunked wins over Content-Length, and the whole chunked body is consumed.
    let s = "POST / HTTP/1.1\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\nGET";
    let (r, used) = req(s).unwrap();
    assert_eq!(r.body, b"abc");
    assert_eq!(used, s.len() - 3);
    let s =
        "POST / HTTP/1.1\r\nTransfer-Encoding: Chunked\r\n\r\n2;ext=1\r\nhi\r\n0\r\nX-T: 1\r\n\r\n";
    let (r, used) = req(s).unwrap();
    assert_eq!((r.body.as_slice(), used), (&b"hi"[..], s.len()));

    for bad in [
        "POST / HTTP/1.1\r\nContent-Length: 5\r\nContent-Length: 6\r\n\r\nhello",
        "POST / HTTP/1.1\r\nContent-Length: +5\r\n\r\nhello",
        "POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n",
        "POST / HTTP/1.1\r\nContent-Length: 0x5\r\n\r\nhello",
        "POST / HTTP/1.1\r\nContent-Length: 5 5\r\n\r\nhello",
    ] {
        assert_eq!(req(bad).err(), Some(Error::BadContentLength), "{bad:?}");
    }
    let same = "POST / HTTP/1.1\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\nhello";
    assert_eq!(req(same).unwrap().0.body, b"hello");
    let huge = "POST / HTTP/1.1\r\nContent-Length: 99999999999999999999999\r\n\r\n";
    assert!(matches!(
        req(huge),
        Err(Error::BadContentLength | Error::BodyTooLarge)
    ));
    let over = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffffffff\r\n";
    assert!(matches!(
        req(over),
        Err(Error::BadChunk | Error::BodyTooLarge)
    ));
    let notchunk = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n";
    assert_eq!(req(notchunk).err(), Some(Error::BadChunk));

    for (bad, want) in [
        ("GET / HTTP/1.1\r\nX: a\r\n b\r\n\r\n", Error::BadHeader),
        ("GET / HTTP/1.1\r\nHost : x\r\n\r\n", Error::BadHeader),
        ("GET / HTTP/1.1\r\n: x\r\n\r\n", Error::BadHeader),
        ("GET / HTTP/2.0\r\n\r\n", Error::BadVersion),
        ("GET / HTTP/1.2\r\n\r\n", Error::BadVersion),
    ] {
        assert_eq!(req(bad).err(), Some(want), "{bad:?}");
    }
    assert!(req("GET /\r\n\r\n").is_err());
    assert!(req("G@T / HTTP/1.1\r\n\r\n").is_err());
    assert!(req("GET  / HTTP/1.1\r\n\r\n").is_err());
}

#[test]
fn leader_limits_fire_before_the_bytes_arrive() {
    let lim = Limits {
        max_head: 200,
        max_headers: 5,
        max_body: 100,
    };
    let mut long = String::from("GET / HTTP/1.1\r\nX: ");
    long.push_str(&"a".repeat(300));
    assert_eq!(
        parse_request(long.as_bytes(), &lim).err(),
        Some(Error::HeadTooLarge),
        "a head past max_head without its blank line"
    );
    let mut many = String::from("GET / HTTP/1.1\r\n");
    for i in 0..6 {
        many.push_str(&format!("H{i}: v\r\n"));
    }
    many.push_str("\r\n");
    assert_eq!(
        parse_request(many.as_bytes(), &lim).err(),
        Some(Error::TooManyHeaders)
    );
    let cl = "POST / HTTP/1.1\r\nContent-Length: 101\r\n\r\n";
    assert_eq!(
        parse_request(cl.as_bytes(), &lim).err(),
        Some(Error::BodyTooLarge),
        "Content-Length over max_body is refused before the body arrives"
    );
    let ch = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n40\r\n";
    let two = format!("{ch}{}\r\n40\r\n", "x".repeat(64));
    assert_eq!(
        parse_request(two.as_bytes(), &lim).err(),
        Some(Error::BodyTooLarge),
        "chunks summing over max_body"
    );
}

#[test]
fn leader_keep_alive_trim_pipelining_and_responses() {
    let r = req("GET / HTTP/1.0\r\nConnection: Keep-Alive\r\n\r\n")
        .unwrap()
        .0;
    assert!(r.keep_alive());
    let r = req("GET / HTTP/1.0\r\n\r\n").unwrap().0;
    assert!(!r.keep_alive());
    let r = req("GET / HTTP/1.1\r\nConnection: Close\r\n\r\n")
        .unwrap()
        .0;
    assert!(!r.keep_alive());
    let r = req("GET /a?b=1 HTTP/1.1\nHoSt:  \t ex.com \t\nx-y: 1\n\n")
        .unwrap()
        .0;
    assert!(r.keep_alive());
    assert_eq!((r.target.as_str(), r.minor), ("/a?b=1", 1));
    assert_eq!(r.header("host"), Some(&b"ex.com"[..]));
    assert_eq!(r.header("X-Y"), Some(&b"1"[..]));

    let two = "POST /1 HTTP/1.1\r\nContent-Length: 2\r\n\r\nhiGET /2 HTTP/1.1\r\n\r\n";
    let (a, n) = req(two).unwrap();
    let (b, m) = req(&two[n..]).unwrap();
    assert_eq!(
        (a.target.as_str(), a.body.as_slice(), b.target.as_str()),
        ("/1", &b"hi"[..], "/2")
    );
    assert_eq!(n + m, two.len());

    let lim = Limits::default();
    let head = "HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n";
    let (r, used) = parse_response(head.as_bytes(), &lim, true).unwrap();
    assert_eq!((r.status, r.body.len(), used), (200, 0, head.len()));
    assert_eq!(
        parse_response(b"HTTP/1.1 200 OK\r\n\r\nabc", &lim, false).err(),
        Some(Error::Incomplete),
        "no length and not head-only: read to end, which the parser cannot know"
    );
    let ok = "HTTP/1.1 404 Not Found Here\r\nContent-Length: 3\r\n\r\nabc";
    let r = parse_response(ok.as_bytes(), &lim, false).unwrap().0;
    assert_eq!(
        (r.status, r.reason.as_str(), r.body.as_slice()),
        (404, "Not Found Here", &b"abc"[..])
    );
    assert!(parse_response(b"HTTP/1.1 2000 OK\r\n\r\n", &lim, true).is_err());
    assert!(parse_response(b"HTTP/1.1 20 OK\r\n\r\n", &lim, true).is_err());
}

const VALID: &[&str] = &[
    "GET / HTTP/1.1\r\nHost: a\r\n\r\n",
    "POST /x HTTP/1.1\r\nContent-Length: 11\r\n\r\nhello world",
    "PUT /y HTTP/1.0\nContent-Length: 1\n\nz",
    "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5;a=b\r\nhello\r\n1\r\n!\r\n0\r\nT: x\r\n\r\n",
    "BREW /pot HTTP/1.1\r\nAccept-Additions: milk\r\n\r\n",
];

#[test]
fn leader_split_at_every_byte() {
    for s in VALID {
        let b = s.as_bytes();
        let full = parse_request(b, &Limits::default()).unwrap();
        assert_eq!(full.1, b.len(), "{s:?}");
        for cut in 0..b.len() {
            assert_eq!(
                parse_request(&b[..cut], &Limits::default()).err(),
                Some(Error::Incomplete),
                "{s:?} cut at {cut}"
            );
        }
        // Parsing after a byte-by-byte stream gives the same request.
        let mut buf = Vec::new();
        let mut got = None;
        for &x in b {
            buf.push(x);
            if let Ok(r) = parse_request(&buf, &Limits::default()) {
                got = Some(r);
            }
        }
        assert_eq!(got.unwrap(), full);
    }
}

#[test]
fn leader_mutation_fuzz_never_panics_and_round_trips() {
    let lim = Limits {
        max_head: 4096,
        max_headers: 32,
        max_body: 4096,
    };
    let mut accepted = 0u32;
    let cases = env("STRATA_CASES", 4000) as u32;
    forall(cases, env("STRATA_SEED", 0x36_2026_1114), |r| {
        let mut b = r.pick(VALID).as_bytes().to_vec();
        for _ in 0..r.range(1, 6) {
            let n = b.len();
            let i = r.below(n.max(1));
            match r.below(5) {
                0 if n > 0 => b[i % n] = r.u64() as u8,
                1 => b.insert(
                    i.min(b.len()),
                    *r.pick(b": \r\n0123456789abcdefABCDEF;-+ \t"),
                ),
                2 if !b.is_empty() => {
                    b.remove(i % b.len());
                }
                3 => {
                    let tail = b[i.min(b.len())..].to_vec();
                    b.extend_from_slice(&tail);
                }
                _ => b.truncate(i),
            }
        }
        if let Ok((req, used)) = parse_request(&b, &lim) {
            assert!(used <= b.len());
            accepted += 1;
            // The writer may add Content-Length; a chunked header has no defined re-encoding.
            if req.header("transfer-encoding").is_none() {
                let w = write_request(&req);
                let (again, n) = parse_request(&w, &lim).unwrap();
                assert_eq!(n, w.len());
                assert_eq!(
                    (&again.method, &again.target, again.minor, &again.body),
                    (&req.method, &req.target, req.minor, &req.body),
                    "written request does not parse back"
                );
            }
        }
        let _ = parse_response(&b, &lim, r.chance(1, 2));
    });
    eprintln!("{accepted} of {cases} mutated messages were accepted and round tripped");
    assert!(accepted > 100, "fuzz accepted too little to mean anything");

    let resp = Response {
        minor: 1,
        status: 201,
        reason: "Created".into(),
        headers: vec![("X-A".into(), b"1".to_vec())],
        body: b"done".to_vec(),
    };
    let w = write_response(&resp);
    let (back, n) = parse_response(&w, &lim, false).unwrap();
    assert_eq!(n, w.len());
    assert_eq!((back.status, back.body.as_slice()), (201, &b"done"[..]));
    assert_eq!(back.header("content-length"), Some(&b"4"[..]));
}
