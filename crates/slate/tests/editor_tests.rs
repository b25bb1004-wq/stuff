//! Multi-cursor and undo-tree tests for `slate::Editor`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use slate::{Cursor, Editor};

type TypingCase<'a> = (&'a str, &'a [Cursor], &'a str, &'a str, &'a [Cursor]);

fn c(anchor: usize, head: usize) -> Cursor {
    Cursor { anchor, head }
}

#[test]
fn multicursor_typing_table() {
    let cases: &[TypingCase<'_>] = &[
        ("hello", &[c(0, 0)], "X", "Xhello", &[c(1, 1)]),
        ("hello", &[c(5, 5)], "!", "hello!", &[c(6, 6)]),
        (
            "foo bar baz",
            &[c(0, 3), c(4, 7), c(8, 11)],
            "x",
            "x x x",
            &[c(1, 1), c(3, 3), c(5, 5)],
        ),
        (
            "abcdef",
            &[c(4, 4), c(1, 1)],
            "12",
            "a12bcd12ef",
            &[c(3, 3), c(8, 8)],
        ),
        // Overlapping and touching selections merge before editing.
        ("abcdef", &[c(1, 4), c(3, 5)], "Z", "aZf", &[c(2, 2)]),
        ("abcdef", &[c(1, 3), c(3, 5)], "XY", "aXYf", &[c(3, 3)]),
        // Reversed selection (anchor > head).
        ("abcdef", &[c(4, 1)], "Q", "aQef", &[c(2, 2)]),
        // Multi-byte UTF-8 selections and insertions.
        (
            "aéb🦀c",
            &[c(1, 3), c(4, 8)],
            "α",
            "aαbαc",
            &[c(3, 3), c(6, 6)],
        ),
        // Empty insertion deletes selections as one step.
        (
            "one two three",
            &[c(0, 4), c(8, 13)],
            "",
            "two ",
            &[c(0, 0), c(4, 4)],
        ),
        // Duplicate empty cursors merge into one.
        ("abc", &[c(1, 1), c(1, 1)], "!", "a!bc", &[c(2, 2)]),
    ];

    for (idx, &(init, cursors, text, expected, expected_cursors)) in cases.iter().enumerate() {
        let mut ed = Editor::new(init);
        ed.set_cursors(cursors.to_vec());
        ed.type_text(text);
        assert_eq!(ed.buffer.text(), expected, "typing case {idx}");
        assert_eq!(ed.cursors(), expected_cursors, "typing cursors {idx}");
        assert!(ed.undo(), "undo case {idx}");
        assert_eq!(ed.buffer.text(), init, "undo text case {idx}");
        assert!(ed.redo(), "redo case {idx}");
        assert_eq!(ed.buffer.text(), expected, "redo text case {idx}");
        assert_eq!(ed.cursors(), expected_cursors, "redo cursors {idx}");
    }
}

#[test]
fn multicursor_backspace_table() {
    let cases: &[(&str, &[Cursor], &str, &[Cursor])] = &[
        ("hello", &[c(0, 0)], "hello", &[c(0, 0)]),
        ("hello", &[c(5, 5)], "hell", &[c(4, 4)]),
        ("hello", &[c(1, 1), c(4, 4)], "elo", &[c(0, 0), c(2, 2)]),
        // Adjacent empty cursors after backspace merge at position 0.
        ("ab", &[c(1, 1), c(2, 2)], "", &[c(0, 0)]),
        // Selection plus empty cursor.
        ("abcdef", &[c(1, 3), c(5, 5)], "adf", &[c(1, 1), c(2, 2)]),
        // UTF-8 multi-byte backspace deletes whole char.
        ("a🦀b", &[c(5, 5)], "ab", &[c(1, 1)]),
        ("éα中", &[c(2, 2), c(7, 7)], "α", &[c(0, 0), c(2, 2)]),
        // Overlapping selections merge before backspace.
        ("abcdef", &[c(1, 3), c(2, 5)], "af", &[c(1, 1)]),
        // Reversed selection deleted by backspace.
        ("abcdef", &[c(5, 2)], "abf", &[c(2, 2)]),
        // Cursor at 0 plus cursor at 3.
        ("abcd", &[c(0, 0), c(3, 3)], "abd", &[c(0, 0), c(2, 2)]),
    ];

    for (idx, &(init, cursors, expected, expected_cursors)) in cases.iter().enumerate() {
        let mut ed = Editor::new(init);
        ed.set_cursors(cursors.to_vec());
        ed.backspace();
        assert_eq!(ed.buffer.text(), expected, "backspace case {idx}");
        assert_eq!(ed.cursors(), expected_cursors, "backspace cursors {idx}");
        if expected != init {
            assert!(ed.undo(), "backspace undo {idx}");
            assert_eq!(ed.buffer.text(), init, "backspace undo text {idx}");
            assert!(ed.redo(), "backspace redo {idx}");
            assert_eq!(ed.buffer.text(), expected, "backspace redo text {idx}");
        } else {
            assert!(!ed.undo(), "no-op backspace should not push undo {idx}");
        }
    }
}

#[test]
fn undo_tree_branching_and_navigation() {
    let mut ed = Editor::new("base");
    assert!(!ed.undo());
    assert!(!ed.redo());
    assert_eq!(ed.branches(), 0);

    ed.set_cursors(vec![c(4, 4)]);
    ed.type_text(" one");
    assert_eq!(ed.buffer.text(), "base one");

    assert!(ed.undo());
    assert_eq!(ed.buffer.text(), "base");
    assert_eq!(ed.branches(), 1);

    ed.type_text(" two");
    assert_eq!(ed.buffer.text(), "base two");
    ed.type_text("!");
    assert_eq!(ed.buffer.text(), "base two!");

    assert!(ed.undo());
    assert!(ed.undo());
    assert_eq!(ed.buffer.text(), "base");
    assert_eq!(ed.branches(), 2);
    assert!(!ed.redo_branch(2));

    assert!(ed.redo_branch(0));
    assert_eq!(ed.buffer.text(), "base one");
    assert!(!ed.redo());

    assert!(ed.undo());
    assert!(ed.redo());
    assert_eq!(ed.buffer.text(), "base two");
    assert!(ed.redo());
    assert_eq!(ed.buffer.text(), "base two!");
}

#[test]
fn random_edit_undo_redo_sequences_forall() {
    const TOKENS: &[&str] = &["a", "xy", "é", "🦀", "\n", ""];

    forall(300, 0xE017_0002, |r| {
        let initial = "alpha\nbeta é🦀";
        let mut ed = Editor::new(initial);
        let mut depth = 0usize;

        for _ in 0..40 {
            let action = r.below(6);
            match action {
                0..=2 => {
                    // Set 1..=4 random cursors and type_text.
                    let len = ed.buffer.len();
                    let s = ed.buffer.text();
                    let mut bounds: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
                    bounds.push(len);
                    let count = 1 + r.below(4);
                    let mut cs = Vec::with_capacity(count);
                    for _ in 0..count {
                        cs.push(c(*r.pick(&bounds), *r.pick(&bounds)));
                    }
                    ed.set_cursors(cs);
                    let tok = *r.pick(TOKENS);
                    let will_push = !ed.cursors().is_empty()
                        && (!tok.is_empty() || ed.cursors().iter().any(|x| x.anchor != x.head));
                    ed.type_text(tok);
                    if will_push {
                        depth += 1;
                    }
                }
                3 => {
                    let s = ed.buffer.text();
                    let mut bounds: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
                    bounds.push(s.len());
                    let cs = vec![c(*r.pick(&bounds), *r.pick(&bounds))];
                    ed.set_cursors(cs);
                    let will_push = ed
                        .cursors()
                        .iter()
                        .any(|x| x.anchor != x.head || x.anchor > 0);
                    ed.backspace();
                    if will_push {
                        depth += 1;
                    }
                }
                4 => {
                    if ed.undo() {
                        depth = depth.checked_sub(1).unwrap();
                    } else {
                        assert_eq!(depth, 0);
                        assert_eq!(ed.buffer.text(), initial);
                    }
                }
                _ => {
                    let b = ed.branches();
                    if b > 0 {
                        let pick = r.below(b);
                        assert!(ed.redo_branch(pick));
                        depth += 1;
                    } else {
                        assert!(!ed.redo());
                    }
                }
            }
            // Invariant: cursors are always sorted and non-overlapping.
            let cs = ed.cursors();
            for w in cs.windows(2) {
                assert!(w[0].anchor.max(w[0].head) < w[1].anchor.min(w[1].head));
            }
        }

        while ed.undo() {
            depth -= 1;
        }
        assert_eq!(depth, 0);
        assert_eq!(ed.buffer.text(), initial);
    });
}
