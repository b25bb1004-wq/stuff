//! Unit and integration tests for `bench` harness and suites.
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use assay::forall;
use bench::{aurum, iridium, measure, mercury, table, xenon, Opts, Sample};

#[test]
fn measure_runs_at_least_once_and_sums_ops() {
    let mut calls = 0u64;
    let sample = measure("test", "once", "ops/s", Duration::ZERO, || {
        calls += 1;
        Ok(7)
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(sample.ops, 7);
    assert_eq!(sample.suite, "test");
    assert_eq!(sample.name, "once");
    assert_eq!(sample.unit, "ops/s");
}

#[test]
fn measure_respects_budget_and_sums_batches() {
    let start = Instant::now();
    let mut calls = 0u64;
    let sample = measure("test", "sum", "ops/s", Duration::from_millis(30), || {
        calls += 1;
        std::thread::sleep(Duration::from_millis(5));
        Ok(10)
    })
    .unwrap();
    let wall = start.elapsed();
    assert!(calls >= 1);
    assert_eq!(sample.ops, calls * 10);
    assert!(sample.secs >= 0.025);
    assert!(wall < Duration::from_secs(1), "took too long: {wall:?}");
}

#[test]
fn measure_propagates_error_and_catches_mismatch() {
    let mut count = 0u32;
    let err = measure("test", "fail", "ops/s", Duration::from_millis(50), || {
        count += 1;
        if count == 2 {
            return Err("mismatch".to_string());
        }
        Ok(5)
    })
    .unwrap_err();
    assert_eq!(err, "mismatch");

    let err_immediate = measure("test", "immediate", "ops/s", Duration::ZERO, || {
        Err("mismatch".to_string())
    })
    .unwrap_err();
    assert_eq!(err_immediate, "mismatch");
}

#[test]
fn rate_zero_secs_and_positive() {
    let s = Sample {
        suite: "x",
        name: "zero".to_string(),
        unit: "ops/s",
        ops: 500,
        secs: 0.0,
    };
    assert_eq!(s.rate(), 0.0);

    forall(50, 42, |r| {
        let ops = r.range(1, 100_000);
        let secs = r.range(1, 100) as f64 / 10.0;
        let sample = Sample {
            suite: "x",
            name: "p".to_string(),
            unit: "ops/s",
            ops,
            secs,
        };
        assert!((sample.rate() - (ops as f64 / secs)).abs() < 1e-9);
    });
}

#[test]
fn table_formatting_exact() {
    let samples = vec![
        Sample {
            suite: "mercury",
            name: "parse_request".to_string(),
            unit: "req/s",
            ops: 12_340,
            secs: 1.0,
        },
        Sample {
            suite: "aurum",
            name: "merkle_root".to_string(),
            unit: "roots/s",
            ops: 4_561_000,
            secs: 1.0,
        },
        Sample {
            suite: "xenon",
            name: "bulk_transfer".to_string(),
            unit: "MB/s",
            ops: 42,
            secs: 1.0,
        },
        Sample {
            suite: "iridium",
            name: "idle".to_string(),
            unit: "ops/s",
            ops: 0,
            secs: 0.0,
        },
    ];
    let expected = "\
| suite | benchmark | rate | unit |
|---|---|---|---|
| mercury | parse_request | 12.3k | req/s |
| aurum | merkle_root | 4.56M | roots/s |
| xenon | bulk_transfer | 42.0 | MB/s |
| iridium | idle | 0 | ops/s |
";
    assert_eq!(table(&samples), expected);
}

#[test]
fn table_property_never_panics_and_keeps_shape() {
    forall(64, 99, |r| {
        let count = r.range(0, 6) as usize;
        let mut samples = Vec::with_capacity(count);
        for i in 0..count {
            let secs = match r.range(0, 5) {
                0 => 0.0,
                1 => f64::NAN,
                2 => f64::INFINITY,
                _ => (r.range(1, 10_000) as f64) / 100.0,
            };
            samples.push(Sample {
                suite: "s",
                name: format!("b{i}"),
                unit: "u",
                ops: r.u64(),
                secs,
            });
        }
        let out = table(&samples);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), count + 2);
        for line in lines {
            assert!(line.starts_with('|') && line.ends_with('|'));
            assert_eq!(line.matches('|').count(), 5);
        }
    });
}

#[test]
fn all_suites_quick_smoke() {
    let opts = Opts {
        budget: Duration::from_millis(5),
        scale: 1,
    };

    let m = mercury::run(&opts).unwrap();
    assert_eq!(m.len(), 3);

    let i = iridium::run(&opts).unwrap();
    assert_eq!(i.len(), 5);

    let x = xenon::run(&opts).unwrap();
    assert_eq!(x.len(), 2);

    let a = aurum::run(&opts).unwrap();
    assert_eq!(a.len(), 4);

    for s in m.into_iter().chain(i).chain(x).chain(a) {
        assert!(s.ops > 0, "{}/{} had 0 ops", s.suite, s.name);
        let rate = s.rate();
        assert!(
            rate.is_finite() && rate > 0.0,
            "{}/{} had non-positive/non-finite rate: {rate}",
            s.suite,
            s.name
        );
    }
}
