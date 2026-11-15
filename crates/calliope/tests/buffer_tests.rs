//! Property and edge-case tests for `calliope::Buffer`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use calliope::Buffer;

fn char_boundaries(s: &str) -> Vec<usize> {
    let mut out: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    out.push(s.len());
    out
}

fn random_utf8(r: &mut assay::Rng, max_chars: usize) -> String {
    const ALPHABET: &[char] = &[
        'a', 'b', 'c', '\n', ' ', 'é', '中', '🦀', 'α', '\t', '0', 'Z',
    ];
    let n = r.below(max_chars + 1);
    let mut s = String::new();
    for _ in 0..n {
        s.push(*r.pick(ALPHABET));
    }
    s
}

fn assert_agrees(buf: &Buffer, shadow: &str, r: &mut assay::Rng) {
    assert_eq!(buf.len(), shadow.len());
    assert_eq!(buf.is_empty(), shadow.is_empty());
    assert_eq!(buf.text(), shadow);

    let expected_lines = shadow.bytes().filter(|&b| b == b'\n').count() + 1;
    assert_eq!(buf.line_count(), expected_lines);

    for line in 0..=expected_lines {
        let expected_start = if line == 0 {
            Some(0)
        } else {
            shadow
                .match_indices('\n')
                .nth(line - 1)
                .map(|(idx, _)| idx + 1)
        };
        assert_eq!(buf.line_start(line), expected_start);
    }

    let bounds = char_boundaries(shadow);
    for _ in 0..5 {
        let a = *r.pick(&bounds);
        let b = *r.pick(&bounds);
        let (lo, hi) = (a.min(b), a.max(b));
        assert_eq!(buf.slice(lo, hi), &shadow[lo..hi]);
        let expected_line = shadow[..lo].bytes().filter(|&b| b == b'\n').count();
        assert_eq!(buf.line_of(lo), expected_line);
    }
    assert_eq!(
        buf.line_of(shadow.len()),
        shadow.bytes().filter(|&b| b == b'\n').count()
    );
}

#[test]
fn buffer_matches_shadow_model_forall_1000() {
    forall(1000, 0x51A7_E001, |r| {
        let init = random_utf8(r, 40);
        let mut buf = Buffer::new(&init);
        let mut shadow = init;
        assert_agrees(&buf, &shadow, r);

        let steps = 1 + r.below(25);
        for _ in 0..steps {
            let bounds = char_boundaries(&shadow);
            if r.chance(3, 5) || shadow.is_empty() {
                let at = *r.pick(&bounds);
                let ins = random_utf8(r, 12);
                buf.insert(at, &ins);
                shadow.insert_str(at, &ins);
            } else {
                let a = *r.pick(&bounds);
                let b = *r.pick(&bounds);
                let (lo, hi) = (a.min(b), a.max(b));
                buf.delete(lo, hi);
                shadow.drain(lo..hi);
            }
            assert_agrees(&buf, &shadow, r);
        }
    });
}

#[test]
fn typing_1000_chars_at_end_and_middle_merges_pieces() {
    let mut b = Buffer::new("initial text");
    for i in 0..1000 {
        let len = b.len();
        let ch = if i % 10 == 0 { "\n" } else { "a" };
        b.insert(len, ch);
    }
    assert!(b.pieces() <= 3, "pieces = {}", b.pieces());
    assert_eq!(b.len(), 12 + 1000);
    assert_eq!(b.line_count(), 101);

    let mut mid = Buffer::new("hello world");
    for i in 0..1000 {
        mid.insert(5 + i, "x");
    }
    assert!(mid.pieces() <= 3, "mid pieces = {}", mid.pieces());
    assert_eq!(mid.slice(5, 1005), "x".repeat(1000));
}

#[test]
#[should_panic]
fn slice_out_of_range_panics() {
    let b = Buffer::new("abc");
    let _ = b.slice(1, 4);
}

#[test]
#[should_panic]
fn insert_non_boundary_panics() {
    let mut b = Buffer::new("é");
    b.insert(1, "x");
}

#[test]
#[should_panic]
fn delete_non_boundary_panics() {
    let mut b = Buffer::new("🦀");
    b.delete(1, 3);
}
