//! Version, requirement, and parser fuzz tests for cairn.
#![allow(clippy::unwrap_used)]

use assay::forall;
use cairn::version::{Req, Version};

fn v(major: u64, minor: u64, patch: u64) -> Version {
    Version {
        major,
        minor,
        patch,
    }
}

#[test]
fn version_parse_valid_and_invalid() {
    let valid = [
        ("0.0.0", v(0, 0, 0)),
        ("1.2.3", v(1, 2, 3)),
        ("10.20.30", v(10, 20, 30)),
        ("18446744073709551615.0.1", v(u64::MAX, 0, 1)),
    ];
    for (s, expected) in valid {
        let parsed = Version::parse(s).unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(parsed.to_string(), s);
    }

    let invalid = [
        "",
        "1",
        "1.2",
        "1.2.3.4",
        "v1.2.3",
        "1.2.3-rc",
        "1.2.3+build",
        " 1.2.3",
        "1.2.3 ",
        "01.2.3",
        "1.02.3",
        "1.2.03",
        "1.2.",
        ".1.2",
        "+1.2.3",
        "18446744073709551616.0.0",
    ];
    for s in invalid {
        assert!(Version::parse(s).is_err(), "expected error for {s:?}");
    }
}

#[test]
fn req_expansions_table() {
    let cases: &[(&str, &[&str], &[&str])] = &[
        (
            "^1.2.3",
            &["1.2.3", "1.2.4", "1.9.9"],
            &["1.2.2", "2.0.0", "0.9.9"],
        ),
        ("^0.2.3", &["0.2.3", "0.2.9"], &["0.2.2", "0.3.0", "1.0.0"]),
        ("^0.0.3", &["0.0.3"], &["0.0.2", "0.0.4", "0.1.0"]),
        ("^1.2", &["1.2.0", "1.5.0", "1.9.9"], &["1.1.9", "2.0.0"]),
        ("^1", &["1.0.0", "1.9.9"], &["0.9.9", "2.0.0"]),
        ("^0.0", &["0.0.0", "0.0.9"], &["0.1.0", "1.0.0"]),
        ("^0", &["0.0.0", "0.9.9"], &["1.0.0"]),
        ("1.2.3", &["1.2.3", "1.5.0"], &["1.2.2", "2.0.0"]),
        ("1.2", &["1.2.0", "1.9.0"], &["1.1.9", "2.0.0"]),
        ("1", &["1.0.0", "1.9.0"], &["0.9.9", "2.0.0"]),
        ("~1.2.3", &["1.2.3", "1.2.9"], &["1.2.2", "1.3.0"]),
        ("~1.2", &["1.2.0", "1.2.9"], &["1.1.9", "1.3.0"]),
        ("~1", &["1.0.0", "1.9.9"], &["0.9.9", "2.0.0"]),
        ("=1.2.3", &["1.2.3"], &["1.2.2", "1.2.4"]),
        ("=1.2", &["1.2.0", "1.2.9"], &["1.1.9", "1.3.0"]),
        ("=1", &["1.0.0", "1.9.9"], &["0.9.9", "2.0.0"]),
        (">1.2.3", &["1.2.4", "2.0.0"], &["1.2.3", "1.2.2"]),
        (">1.2", &["1.3.0", "2.0.0"], &["1.2.9", "1.2.0"]),
        (">1", &["2.0.0", "3.0.0"], &["1.9.9", "1.0.0"]),
        (">=1.2", &["1.2.0", "1.2.5", "2.0.0"], &["1.1.9"]),
        (">=1", &["1.0.0", "2.0.0"], &["0.9.9"]),
        ("<1.2.3", &["1.2.2", "0.0.0"], &["1.2.3", "1.2.4"]),
        ("<1.2", &["1.1.9", "0.0.0"], &["1.2.0", "1.2.1"]),
        ("<1", &["0.9.9", "0.0.0"], &["1.0.0", "1.0.1"]),
        ("<=1.2.3", &["1.2.3", "1.0.0"], &["1.2.4"]),
        ("<=1.2", &["1.2.0", "1.2.9", "0.9.0"], &["1.3.0"]),
        ("<=1", &["1.0.0", "1.9.9", "0.0.0"], &["2.0.0"]),
        ("*", &["0.0.0", "1.2.3", "99.99.99"], &[]),
        ("1.*", &["1.0.0", "1.9.9"], &["0.9.9", "2.0.0"]),
        ("1.2.*", &["1.2.0", "1.2.99"], &["1.1.9", "1.3.0"]),
        (">=1.2, <1.5", &["1.2.0", "1.4.9"], &["1.1.9", "1.5.0"]),
    ];

    for &(req_str, yes, no) in cases {
        let req = Req::parse(req_str).unwrap();
        let reparsed = Req::parse(&req.to_string()).unwrap();
        assert_eq!(req, reparsed, "roundtrip failed for {req_str}");
        for &ver in yes {
            assert!(
                req.matches(&Version::parse(ver).unwrap()),
                "{req_str} should match {ver}"
            );
        }
        for &ver in no {
            assert!(
                !req.matches(&Version::parse(ver).unwrap()),
                "{req_str} should NOT match {ver}"
            );
        }
    }
}

#[test]
fn req_display_and_errors() {
    assert_eq!(Req::parse("1.2.3").unwrap().to_string(), "1.2.3");
    assert_eq!(Req::parse("^ 1.2.3").unwrap().to_string(), "^1.2.3");
    assert_eq!(
        Req::parse(" >= 1.2 , < 2.0 ").unwrap().to_string(),
        ">=1.2, <2.0"
    );
    assert_eq!(Req::parse("1.*").unwrap().to_string(), "1.*");
    assert_eq!(Req::parse("*").unwrap().to_string(), "*");

    let bad = [
        "",
        "   ",
        "1.0.0,",
        ",1.0.0",
        "1.0.0, , 2.0.0",
        "~>1.0.0",
        "==1.0.0",
        "!=1.0.0",
        "1.*.3",
        "^*",
        ">=1.*",
        "*.*",
        "1.*.*",
        "1.2.3 1.2.4",
        "01.2.3",
        "1.2.3-rc",
        "^",
        ">=",
    ];
    for s in bad {
        assert!(Req::parse(s).is_err(), "expected error for {s:?}");
    }
}

#[test]
fn req_u64_max_no_panic_or_overflow() {
    let max_v = v(u64::MAX, u64::MAX, u64::MAX);
    let r1 = Req::parse("^18446744073709551615").unwrap();
    assert!(r1.matches(&max_v));
    assert!(!r1.matches(&v(0, 0, 0)));

    let r2 = Req::parse(">1.18446744073709551615").unwrap();
    assert!(!r2.matches(&v(1, u64::MAX, u64::MAX)));
    assert!(r2.matches(&v(2, 0, 0)));

    let r3 = Req::parse("<=18446744073709551615").unwrap();
    assert!(r3.matches(&max_v));

    let r4 = Req::parse(">18446744073709551615").unwrap();
    assert!(!r4.matches(&max_v));

    let r5 = Req::parse("<=1.18446744073709551615").unwrap();
    assert!(r5.matches(&v(1, u64::MAX, u64::MAX)));
    assert!(!r5.matches(&v(2, 0, 0)));
}

#[test]
fn fuzz_parsers_no_panic() {
    forall(500, 0xCA14_0001, |rng| {
        let bytes = rng.bytes(200);
        let s = String::from_utf8_lossy(&bytes);
        let _ = Version::parse(&s);
        let _ = Req::parse(&s);
        let _ = cairn::manifest::Manifest::parse(&s);
        let _ = cairn::lock::read(&s);
    });
}
