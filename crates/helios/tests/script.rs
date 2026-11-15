//! Integration tests for `helios::script` and `game.logos`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use helios::scene::{replay, Command, Recorder, Scene, Stage};
use helios::script::{render, Draw, ScriptScene, GAME_SRC};
use pigment::Rgba;

#[test]
fn all_externs_and_init_work() {
    let src = r#"
        fn g_keys() -> int = extern "game.keys"
        fn g_frame() -> int = extern "game.frame"
        fn g_get(i: int) -> int = extern "game.get"
        fn g_set(i: int, v: int) = extern "game.set"
        fn g_rand(n: int) -> int = extern "game.rand"
        fn g_rect(x: int, y: int, w: int, h: int, c: int) = extern "game.rect"
        fn g_log(s: str) = extern "game.log"
        fn g_quit() = extern "game.quit"

        fn init() {
            g_set(0, 42)
            g_set(-1, 99)
            g_set(256, 99)
            g_log("init;")
        }

        fn update() {
            let f = g_frame()
            let k = g_keys()
            let r = g_rand(10)
            let r_zero = g_rand(0)
            let r_neg = g_rand(-5)
            g_set(1, f)
            g_set(2, k)
            g_set(3, g_get(0) + r + r_zero + r_neg)
            g_set(4, g_get(-1) + g_get(256))
            g_rect(10, 20, 30, 40, 0x112233)
            g_rect(10000000000, -10000000000, 5, 5, -1)
            g_log("u;")
            if f >= 1 {
                g_quit()
            }
        }
    "#;

    let mut scene = ScriptScene::new(src, 12345, 100_000).unwrap();
    let mut cmds = Vec::new();
    scene.update(0b101, &mut cmds);
    assert!(cmds.is_empty());
    assert_eq!(scene.slot(0), 42);
    assert_eq!(scene.slot(1), 0);
    assert_eq!(scene.slot(2), 5);
    assert!((42..52).contains(&scene.slot(3)));
    assert_eq!(scene.slot(4), 0);
    assert_eq!(scene.slot(256), 0);
    assert_eq!(
        scene.draws(),
        &[
            Draw {
                x: 10,
                y: 20,
                w: 30,
                h: 40,
                color: 0x112233,
            },
            Draw {
                x: i32::MAX,
                y: i32::MIN,
                w: 5,
                h: 5,
                color: 0,
            },
        ]
    );

    cmds.clear();
    scene.update(0, &mut cmds);
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], Command::Quit));
    assert_eq!(scene.slot(1), 1);
    assert_eq!(scene.log(), "init;u;u;");
}

#[test]
fn invalid_scripts_fail_to_load() {
    assert!(ScriptScene::new("fn foo() {}", 1, 1000).is_err());
    assert!(ScriptScene::new("fn update(x: int) {}", 1, 1000).is_err());
    assert!(ScriptScene::new("fn update() -> int { 1 }", 1, 1000).is_err());
    assert!(ScriptScene::new("fn init(x: int) {}\nfn update() {}", 1, 1000).is_err());
    assert!(ScriptScene::new(
        "fn bad() = extern \"unknown.fn\"\nfn update() { bad() }",
        1,
        1000
    )
    .is_err());
    assert!(ScriptScene::new("fn update() { let x = }", 1, 1000).is_err());
}

#[test]
fn trap_and_out_of_fuel_pop_and_keep_draws() {
    let div_zero = r#"
        fn g_rect(x: int, y: int, w: int, h: int, c: int) = extern "game.rect"
        fn update() {
            g_rect(1, 2, 3, 4, 0xff0000)
            let x = 1 / 0
        }
    "#;
    let mut s1 = ScriptScene::new(div_zero, 1, 100_000).unwrap();
    let mut cmds = Vec::new();
    s1.update(0, &mut cmds);
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], Command::Pop));
    assert!(s1.log().contains("trap: DivideByZero\n"));
    assert_eq!(s1.draws().len(), 1);

    let spin = r#"
        fn g_rect(x: int, y: int, w: int, h: int, c: int) = extern "game.rect"
        fn update() {
            g_rect(5, 6, 7, 8, 0x00ff00)
            var i = 0
            while i < 10000 {
                i = i + 1
            }
        }
    "#;
    let mut s2 = ScriptScene::new(spin, 1, 200).unwrap();
    cmds.clear();
    s2.update(0, &mut cmds);
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], Command::Pop));
    assert!(s2.log().contains("trap: OutOfFuel\n"));
    assert_eq!(s2.draws().len(), 1);
}

#[test]
fn state_hash_matches_fnv1a_spec() {
    let src = r#"
        fn g_set(i: int, v: int) = extern "game.set"
        fn g_rect(x: int, y: int, w: int, h: int, c: int) = extern "game.rect"
        fn update() {
            g_set(3, 99)
            g_rect(1, 2, 3, 4, 0xabcdef)
        }
    "#;
    let mut scene = ScriptScene::new(src, 7, 10_000).unwrap();
    let mut cmds = Vec::new();
    scene.update(0, &mut cmds);

    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let prime: u64 = 0x0100_0000_01b3;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(prime);
        }
    };
    for i in 0..256 {
        let v: i64 = if i == 3 { 99 } else { 0 };
        feed(&v.to_le_bytes());
    }
    feed(&1i32.to_le_bytes());
    feed(&2i32.to_le_bytes());
    feed(&3i32.to_le_bytes());
    feed(&4i32.to_le_bytes());
    feed(&0x00ab_cdef_u32.to_le_bytes());
    assert_eq!(scene.state_hash(), h);
}

#[test]
fn replay_determinism() {
    forall(20, 42, |rng| {
        let seed = rng.u64();
        let mut rec = Recorder::new();
        for f in 0..40u64 {
            if rng.range(0, 3) == 0 {
                rec.record(f, rng.range(0, 32) as u32);
            }
        }
        let mut st1 = Stage::new(Box::new(ScriptScene::new(GAME_SRC, seed, 200_000).unwrap()));
        let mut st2 = Stage::new(Box::new(ScriptScene::new(GAME_SRC, seed, 200_000).unwrap()));
        let h1 = replay(&mut st1, &rec, 60);
        let h2 = replay(&mut st2, &rec, 60);
        assert_eq!(h1, h2);
        assert_eq!(st1.frame(), st2.frame());
    });
}

#[test]
fn game_no_input_reaches_game_over_and_quits() {
    let mut scene = ScriptScene::new(GAME_SRC, 99, 200_000).unwrap();
    let mut game_over_frame = None;
    let mut cmds = Vec::new();
    let mut quit_frame = None;

    for f in 0..20_000 {
        cmds.clear();
        scene.update(0, &mut cmds);
        assert!(
            scene.log().is_empty(),
            "unexpected log/trap: {}",
            scene.log()
        );
        if scene.slot(2) == 1 && game_over_frame.is_none() {
            game_over_frame = Some(f);
        }
        if cmds.iter().any(|c| matches!(c, Command::Quit)) {
            quit_frame = Some(f);
            break;
        }
    }

    let go = game_over_frame.expect("game should reach game over within 20_000 frames");
    let q = quit_frame.expect("game should quit 60 frames after game over");
    assert_eq!(q - go, 60);
    assert_eq!(scene.slot(1), 0);
}

#[test]
fn game_scripted_input_scores_higher_than_no_input() {
    let mut idle = ScriptScene::new(GAME_SRC, 7, 200_000).unwrap();
    let mut cmds = Vec::new();
    for _ in 0..2000 {
        cmds.clear();
        idle.update(0, &mut cmds);
        if cmds.iter().any(|c| matches!(c, Command::Quit)) {
            break;
        }
    }
    let idle_score = idle.slot(0);

    let mut active = ScriptScene::new(GAME_SRC, 7, 200_000).unwrap();
    for f in 0..2000 {
        cmds.clear();
        // Sweep left/right while firing action (bit 4).
        let dir = if (f / 45) % 2 == 0 { 1 } else { 2 };
        let keys = dir | (1 << 4);
        active.update(keys, &mut cmds);
        assert!(
            active.log().is_empty(),
            "trap in active run: {}",
            active.log()
        );
        if cmds.iter().any(|c| matches!(c, Command::Quit)) {
            break;
        }
    }
    let active_score = active.slot(0);
    assert!(
        active_score > idle_score,
        "expected active_score ({active_score}) > idle_score ({idle_score})"
    );
}

#[test]
fn render_overlapping_and_clipped_rects() {
    let draws = [
        Draw {
            x: -2,
            y: -1,
            w: 4,
            h: 3,
            color: 0xff0000,
        },
        Draw {
            x: 1,
            y: 1,
            w: 5,
            h: 5,
            color: 0x00ff00,
        },
        Draw {
            x: 10,
            y: 10,
            w: 4,
            h: 4,
            color: 0x0000ff,
        },
        Draw {
            x: 0,
            y: 0,
            w: 0,
            h: 4,
            color: 0xffffff,
        },
    ];
    let canvas = render(&draws, 4, 4);
    assert_eq!(canvas.width, 4);
    assert_eq!(canvas.height, 4);
    // (0, 0) covered by first red rect (clipped from -2..-1 to 0..2 x 0..2)
    assert_eq!(canvas.get(0, 0), Some(Rgba(255, 0, 0, 255)));
    // (1, 1) overlapped by second green rect
    assert_eq!(canvas.get(1, 1), Some(Rgba(0, 255, 0, 255)));
    // (3, 3) covered by second green rect (clipped to 4x4)
    assert_eq!(canvas.get(3, 3), Some(Rgba(0, 255, 0, 255)));
    // (3, 0) untouched -> black background
    assert_eq!(canvas.get(3, 0), Some(Rgba(0, 0, 0, 255)));
    // (0, 3) untouched -> black background
    assert_eq!(canvas.get(0, 3), Some(Rgba(0, 0, 0, 255)));
}
