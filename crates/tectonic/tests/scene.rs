//! Integration tests for `tectonic::scene`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use tectonic::scene::{replay, Command, Input, Recorder, Scene, Stage};

struct CounterScene {
    id: u64,
    acc: u64,
    script: Vec<(u32, Vec<Action>)>,
}

#[derive(Clone, Copy)]
enum Action {
    Push(u64),
    Pop,
    Replace(u64),
    Quit,
}

impl CounterScene {
    fn new(id: u64) -> Self {
        Self {
            id,
            acc: 0,
            script: Vec::new(),
        }
    }

    fn with_script(id: u64, script: Vec<(u32, Vec<Action>)>) -> Self {
        Self { id, acc: 0, script }
    }
}

impl Scene for CounterScene {
    fn update(&mut self, keys: u32, out: &mut Vec<Command>) {
        self.acc = self
            .acc
            .wrapping_mul(131)
            .wrapping_add(u64::from(keys) ^ self.id);
        for (trigger, actions) in &self.script {
            if *trigger == keys {
                for &act in actions {
                    match act {
                        Action::Push(nid) => {
                            out.push(Command::Push(Box::new(CounterScene::new(nid))))
                        }
                        Action::Pop => out.push(Command::Pop),
                        Action::Replace(nid) => {
                            out.push(Command::Replace(Box::new(CounterScene::new(nid))))
                        }
                        Action::Quit => out.push(Command::Quit),
                    }
                }
            }
        }
    }

    fn state_hash(&self) -> u64 {
        self.id.rotate_left(17) ^ self.acc
    }
}

#[test]
fn stage_push_pop_replace_quit_and_hash_changes() {
    let root = CounterScene::with_script(
        1,
        vec![
            (1, vec![Action::Push(2)]),
            (2, vec![Action::Replace(3)]),
            (3, vec![Action::Pop, Action::Replace(4)]),
            (4, vec![Action::Quit, Action::Push(99)]),
            (5, vec![Action::Pop]),
        ],
    );
    let mut stage = Stage::new(Box::new(root));
    assert_eq!(stage.depth(), 1);
    assert_eq!(stage.frame(), 0);
    let h0 = stage.hash();

    // Push scene 2
    assert!(stage.step(1));
    assert_eq!(stage.depth(), 2);
    assert_eq!(stage.frame(), 1);
    let h1 = stage.hash();
    assert_ne!(h0, h1);

    // Top is scene 2 (no script), so step 2 just updates scene 2
    assert!(stage.step(2));
    assert_eq!(stage.depth(), 2);
    assert_eq!(stage.frame(), 2);
    let h2 = stage.hash();
    assert_ne!(h1, h2);

    // Test Replace and Pop on a fresh stage
    let root2 = CounterScene::with_script(
        10,
        vec![
            (1, vec![Action::Replace(20)]),
            (2, vec![Action::Pop, Action::Replace(30)]),
            (3, vec![Action::Quit, Action::Push(40)]),
        ],
    );
    let mut s2 = Stage::new(Box::new(root2));
    assert!(s2.step(1));
    assert_eq!(s2.depth(), 1);
    assert_eq!(s2.frame(), 1);

    // Pop then Replace on empty pushes 30
    let root3 = CounterScene::with_script(10, vec![(2, vec![Action::Pop, Action::Replace(30)])]);
    let mut s3 = Stage::new(Box::new(root3));
    assert!(s3.step(2));
    assert_eq!(s3.depth(), 1);

    // Pop to empty returns false and does not advance frame on further calls
    let root4 = CounterScene::with_script(10, vec![(1, vec![Action::Pop])]);
    let mut s4 = Stage::new(Box::new(root4));
    assert!(!s4.step(1));
    assert_eq!(s4.depth(), 0);
    assert_eq!(s4.frame(), 1);
    assert!(!s4.step(0));
    assert_eq!(s4.frame(), 1);

    // Quit ignores subsequent Push and stops stage
    let root5 = CounterScene::with_script(10, vec![(3, vec![Action::Quit, Action::Push(40)])]);
    let mut s5 = Stage::new(Box::new(root5));
    let h_before_quit = s5.hash();
    assert!(!s5.step(3));
    assert_eq!(s5.depth(), 1);
    assert_eq!(s5.frame(), 1);
    assert_ne!(h_before_quit, s5.hash());
    assert!(!s5.step(0));
    assert_eq!(s5.frame(), 1);
}

#[test]
fn recorder_record_keys_at_and_codec_round_trip() {
    let mut rec = Recorder::new();
    assert_eq!(rec.keys_at(0), 0);
    assert_eq!(rec.keys_at(100), 0);

    // Initial 0 is not stored
    rec.record(0, 0);
    assert!(rec.inputs.is_empty());

    rec.record(5, 0b101);
    rec.record(8, 0b101); // duplicate ignored
    rec.record(3, 0b111); // earlier frame ignored
    rec.record(5, 0b010); // equal frame overwrites last
    rec.record(10, 0);
    assert_eq!(
        rec.inputs,
        vec![
            Input {
                frame: 5,
                keys: 0b010
            },
            Input { frame: 10, keys: 0 },
        ]
    );
    assert_eq!(rec.keys_at(0), 0);
    assert_eq!(rec.keys_at(4), 0);
    assert_eq!(rec.keys_at(5), 0b010);
    assert_eq!(rec.keys_at(9), 0b010);
    assert_eq!(rec.keys_at(10), 0);
    assert_eq!(rec.keys_at(50), 0);

    let bytes = rec.encode();
    assert_eq!(&bytes[..4], b"TREC");
    let decoded = Recorder::decode(&bytes).unwrap();
    assert_eq!(decoded, rec);
}

#[test]
fn decode_rejects_hostile_bytes() {
    assert!(Recorder::decode(&[]).is_none());
    assert!(Recorder::decode(b"TRE").is_none());
    assert!(Recorder::decode(b"BAD!0000").is_none());

    // Wrong length (count = 1, but 0 payload bytes)
    let mut bad_len = b"TREC".to_vec();
    bad_len.extend_from_slice(&1u32.to_le_bytes());
    assert!(Recorder::decode(&bad_len).is_none());

    // Huge count (u32::MAX)
    let mut huge = b"TREC".to_vec();
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(Recorder::decode(&huge).is_none());

    // Trailing byte
    let mut trailing = Recorder::new().encode();
    trailing.push(0);
    assert!(Recorder::decode(&trailing).is_none());

    // Decreasing frames
    let mut dec = b"TREC".to_vec();
    dec.extend_from_slice(&2u32.to_le_bytes());
    dec.extend_from_slice(&10u64.to_le_bytes());
    dec.extend_from_slice(&1u32.to_le_bytes());
    dec.extend_from_slice(&9u64.to_le_bytes());
    dec.extend_from_slice(&2u32.to_le_bytes());
    assert!(Recorder::decode(&dec).is_none());

    // Fuzz decode with arbitrary bytes
    forall(200, 32, |rng| {
        let len = rng.range(0, 64) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| rng.range(0, 256) as u8).collect();
        if let Some(r) = Recorder::decode(&bytes) {
            for w in r.inputs.windows(2) {
                assert!(w[0].frame <= w[1].frame);
            }
        }
    });
}

#[test]
fn live_play_and_replay_give_same_end_hash() {
    fn make_stage() -> Stage {
        let root = CounterScene::with_script(
            42,
            vec![(1, vec![Action::Push(7)]), (9, vec![Action::Quit])],
        );
        Stage::new(Box::new(root))
    }

    let mut live = make_stage();
    let mut rec = Recorder::new();
    let live_keys = [0u32, 0, 1, 1, 3, 7, 7, 0, 9, 1];
    for &k in &live_keys {
        rec.record(live.frame(), k);
        if !live.step(k) {
            break;
        }
    }
    let live_hash = live.hash();

    let encoded = rec.encode();
    let decoded = Recorder::decode(&encoded).unwrap();

    let mut replayed = make_stage();
    let replay_hash = replay(&mut replayed, &decoded, live_keys.len() as u64);
    assert_eq!(replay_hash, live_hash);
    assert_eq!(replayed.frame(), live.frame());
    assert_eq!(replayed.depth(), live.depth());
}
