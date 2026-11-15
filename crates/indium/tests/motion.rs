//! Unit and property tests for `indium::motion`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use indium::motion::{css_linear, sample, FIRM, HEAVY, LIVE, MAX_STOPS};

#[test]
fn presets_settle_by_their_duration() {
    for p in [FIRM, LIVE, HEAVY] {
        let pts = sample(p, p.duration_ms as usize + 1);
        assert_eq!(pts.first().copied(), Some(0));
        assert_eq!(pts.last().copied(), Some(1000), "{p:?} not at target");
    }
}

#[test]
fn firm_and_heavy_never_pass_target() {
    for p in [FIRM, HEAVY] {
        let pts = sample(p, 401);
        assert!(pts.iter().all(|&v| v <= 1000), "{p:?} overshoots");
    }
}

#[test]
fn live_peaks_between_four_and_eight_percent() {
    let peak = sample(LIVE, 801).into_iter().max().unwrap_or(0);
    assert!((1040..=1080).contains(&peak), "LIVE peak {peak}");
}

#[test]
fn sample_is_deterministic_and_sized() {
    assert_eq!(sample(LIVE, 50), sample(LIVE, 50));
    assert_eq!(sample(FIRM, 0).len(), 0);
    assert_eq!(sample(FIRM, 7).len(), 7);
}

#[test]
fn css_has_at_most_32_stops_and_fixed_format() {
    let pts = sample(LIVE, 200);
    let css = css_linear(&pts);
    assert!(css.starts_with("linear(0.000, "), "{css}");
    assert!(css.ends_with(", 1.000)"), "{css}");
    assert!(css.matches(',').count() < MAX_STOPS);
    assert_eq!(css_linear(&[]), "linear(0, 1)");
    assert_eq!(css_linear(&[0, -40, 1000]), "linear(0.000, -0.040, 1.000)");
}

#[test]
fn css_stop_count_is_capped_for_any_length() {
    forall(300, 0x9001, |rng| {
        let n = (rng.u64() % 300) as usize;
        let pts: Vec<i32> = (0..n).map(|i| i as i32 - 50).collect();
        let css = css_linear(&pts);
        let stops = if n == 0 { 2 } else { n.min(MAX_STOPS) };
        assert_eq!(css.matches(',').count(), stops - 1, "n={n}");
    });
}
