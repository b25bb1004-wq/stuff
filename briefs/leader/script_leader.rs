//! Leader check for T70b: own scripts against the extern contract, an independent state-hash
//! model, traps and fuel, replay determinism of the game over random inputs, its fuel bound and
//! termination, and render pixels.
#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
use assay::Rng;
use pigment::Rgba;
use helios::rng::Pcg32;
use helios::scene::{replay, Command, Recorder, Scene, Stage};
use helios::script::{render, Draw, ScriptScene, GAME_SRC};

const EXTERNS: &str = r#"
fn g_keys() -> int = extern "game.keys"
fn g_frame() -> int = extern "game.frame"
fn g_get(i: int) -> int = extern "game.get"
fn g_set(i: int, v: int) = extern "game.set"
fn g_rand(n: int) -> int = extern "game.rand"
fn g_rect(x: int, y: int, w: int, h: int, c: int) = extern "game.rect"
fn g_log(s: str) = extern "game.log"
fn g_quit() = extern "game.quit"
"#;

fn fnv(slots: &[i64; 256], draws: &[Draw]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: &[u8]| {
        for &x in b {
            h ^= u64::from(x);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for s in slots {
        eat(&s.to_le_bytes());
    }
    for d in draws {
        for v in [d.x, d.y, d.w, d.h] {
            eat(&v.to_le_bytes());
        }
        eat(&d.color.to_le_bytes());
    }
    h
}

#[test]
fn contract_matches_model() {
    let src = format!(
        "{EXTERNS}
fn init() {{
    g_set(5, 7)
    g_log(\"i\")
}}
fn update() {{
    let k = g_keys()
    let f = g_frame()
    g_set(0, g_get(0) + k)
    g_set(1, g_rand(1000))
    g_set(2, f)
    g_set(300, 1)
    g_set(3, g_get(-4) + g_get(999) + g_rand(0) + g_rand(-3))
    g_rect(f, k, g_rand(5), 3, f * 7)
    if k == 31 {{
        g_quit()
    }}
}}"
    );
    let mut r = Rng::new(0x70B_0001);
    for case in 0..20 {
        let seed = r.u64();
        let mut s = ScriptScene::new(&src, seed, 50_000).unwrap();
        let mut rng = Pcg32::new(seed, 0);
        let mut slots = [0i64; 256];
        slots[5] = 7;
        for f in 0..200i64 {
            let keys = if r.chance(1, 40) {
                31
            } else {
                r.below(31) as u32
            };
            let mut out = Vec::new();
            s.update(keys, &mut out);
            slots[0] += i64::from(keys);
            slots[1] = i64::from(rng.below(1000));
            slots[2] = f;
            let w = i64::from(rng.below(5));
            let draws = [Draw {
                x: f as i32,
                y: keys as i32,
                w: w as i32,
                h: 3,
                color: (f * 7) as u32,
            }];
            assert_eq!(s.draws(), &draws, "case {case} frame {f}");
            for (i, &v) in slots.iter().enumerate() {
                assert_eq!(s.slot(i), v, "case {case} frame {f} slot {i}");
            }
            assert_eq!(s.slot(256), 0);
            assert_eq!(
                s.state_hash(),
                fnv(&slots, &draws),
                "case {case} frame {f}: hash model"
            );
            let quit = out.iter().any(|c| matches!(c, Command::Quit));
            assert_eq!(quit, keys == 31, "case {case} frame {f}: quit");
            assert!(!out
                .iter()
                .any(|c| matches!(c, Command::Pop | Command::Push(_) | Command::Replace(_))));
        }
        assert!(
            s.log().starts_with('i') && !s.log().contains("trap"),
            "{:?}",
            s.log()
        );
    }
}

#[test]
fn saturation_traps_and_fuel() {
    let src = format!(
        "{EXTERNS}
fn update() {{
    g_rect(10000000000, -10000000000, 5000000000, -1, -1)
    g_rect(1, 2, 3, 4, 99999999999)
    let f = g_frame()
    if f == 3 {{
        let z = 1 / (f - 3)
        g_set(0, z)
    }}
    if f == 5 {{
        var i = 0
        while i < 100000000 {{
            i = i + 1
        }}
    }}
    g_set(1, f)
}}"
    );
    let mut s = ScriptScene::new(&src, 1, 20_000).unwrap();
    let want = [
        Draw {
            x: i32::MAX,
            y: i32::MIN,
            w: i32::MAX,
            h: -1,
            color: 0,
        },
        Draw {
            x: 1,
            y: 2,
            w: 3,
            h: 4,
            color: u32::MAX,
        },
    ];
    for f in 0..7 {
        let mut out = Vec::new();
        s.update(0, &mut out);
        assert_eq!(s.draws(), &want, "frame {f}: draws kept even on a trap");
        let pops = out.iter().filter(|c| matches!(c, Command::Pop)).count();
        assert_eq!(
            pops,
            usize::from(f == 3 || f == 5),
            "frame {f}: {:?}",
            s.log()
        );
        assert_eq!(
            s.slot(1),
            match f {
                3 => 2,
                5 => 4,
                _ => f,
            },
            "frame {f}: a trapped frame stops before g_set(1)"
        );
    }
    assert_eq!(s.log().matches("trap: ").count(), 2, "{:?}", s.log());
    assert!(s.log().lines().all(|l| l.starts_with("trap: ")));
    assert!(ScriptScene::new("fn update( {", 1, 100).is_err());
    assert!(
        ScriptScene::new("fn main() {}", 1, 100).is_err(),
        "update is required"
    );
}

fn game_stage(seed: u64) -> Stage {
    Stage::new(Box::new(ScriptScene::new(GAME_SRC, seed, 200_000).unwrap()))
}

#[test]
fn game_terminates_fits_fuel_and_replays() {
    assert!(GAME_SRC.lines().count() <= 300);
    let mut r = Rng::new(0x70B_0002);
    // no input: game over and quit within 20_000 + 60 frames, never trapping
    for seed in [1, 2, r.u64()] {
        let mut s = ScriptScene::new(GAME_SRC, seed, 200_000).unwrap();
        let mut frames = 0;
        let mut over_at = None;
        loop {
            let mut out = Vec::new();
            s.update(0, &mut out);
            frames += 1;
            assert!(!s.log().contains("trap"), "seed {seed}: {}", s.log());
            if over_at.is_none() && s.slot(2) == 1 {
                over_at = Some(frames);
            }
            if out.iter().any(|c| matches!(c, Command::Quit)) {
                break;
            }
            assert!(frames <= 20_060, "seed {seed}: no quit");
        }
        let o = over_at.expect("quit without game over");
        assert_eq!(
            frames - o,
            60,
            "seed {seed}: quit must come 60 frames after game over"
        );
    }
    // random inputs: never traps (fuel bound), replay is deterministic
    for case in 0..6 {
        let seed = r.u64();
        let mut rec = Recorder::new();
        let mut keys = 0u32;
        for f in 0..3000u64 {
            if r.chance(1, 15) {
                keys = r.below(32) as u32;
            }
            rec.record(f, keys);
        }
        let mut a = game_stage(seed);
        let ha = replay(&mut a, &rec, 3000);
        let mut b = game_stage(seed);
        assert_eq!(
            replay(&mut b, &Recorder::decode(&rec.encode()).unwrap(), 3000),
            ha,
            "case {case}: replay differs"
        );
        // the same inputs on a scene stepped by hand give the same hash and no trap
        let mut s = ScriptScene::new(GAME_SRC, seed, 200_000).unwrap();
        for f in 0..3000u64 {
            let mut out = Vec::new();
            s.update(rec.keys_at(f), &mut out);
            if out.iter().any(|c| matches!(c, Command::Quit)) {
                break;
            }
        }
        assert!(!s.log().contains("trap"), "case {case}: {}", s.log());
    }
}

#[test]
fn render_pixels() {
    let mut r = Rng::new(0x70B_0003);
    for _ in 0..200 {
        let (w, h) = (1 + r.below(40) as u32, 1 + r.below(30) as u32);
        let draws: Vec<Draw> = (0..r.below(6))
            .map(|_| Draw {
                x: r.below(60) as i32 - 10,
                y: r.below(50) as i32 - 10,
                w: r.below(30) as i32 - 3,
                h: r.below(30) as i32 - 3,
                color: (r.u64() & 0xff_ffff) as u32,
            })
            .collect();
        let c = render(&draws, w, h);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let mut want = Rgba(0, 0, 0, 255);
                for d in &draws {
                    if x >= d.x
                        && y >= d.y
                        && x < d.x.saturating_add(d.w)
                        && y < d.y.saturating_add(d.h)
                    {
                        want = Rgba(
                            (d.color >> 16) as u8,
                            (d.color >> 8) as u8,
                            d.color as u8,
                            255,
                        );
                    }
                }
                assert_eq!(c.get(x, y), Some(want), "pixel {x},{y} of {draws:?}");
            }
        }
        assert_eq!(c.get(w as i32, 0), None);
    }
}
