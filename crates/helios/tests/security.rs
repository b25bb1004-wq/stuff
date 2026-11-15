//! Security and robustness tests for helios trust-boundary code.
#![allow(clippy::unwrap_used)]

use std::time::Instant;

use helios::audio::{tone, wav, Mixer, Sound};
use helios::draw::{draw_world, to_screen, Camera};
use helios::ecs::World as EcsWorld;
use helios::fx::{Fx, Vec2};
use helios::phys::{collide, Body, Shape, World as PhysWorld};
use helios::rng::Pcg32;
use helios::scene::{Command, Recorder, Scene, Stage};
use helios::script::ScriptScene;
use helios::Clock;
use pigment::{Canvas, Rgba};

#[test]
fn audio_tone_huge_duration_is_capped() {
    let start = Instant::now();
    let s1 = tone(u32::MAX, 440, u32::MAX, 1000);
    assert!(s1.samples.len() * 2 <= 64 * 1024 * 1024);

    let s2 = tone(48_000, 440, u32::MAX, i16::MIN);
    assert!(s2.samples.len() * 2 <= 64 * 1024 * 1024);
    assert!(start.elapsed().as_secs_f64() < 1.0);
}

#[test]
fn script_log_is_bounded() {
    let src = r#"
fn g_log(s: str) = extern "game.log"

fn update() {
    let chunk = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    for i in 0..200 {
        g_log(chunk)
    }
}
"#;
    let mut scene = ScriptScene::new(src, 1, 100_000).unwrap();
    let mut cmds = Vec::new();
    for _ in 0..100 {
        cmds.clear();
        scene.update(0, &mut cmds);
    }
    assert!(
        scene.log().len() <= 256 * 1024,
        "script log grew unboundedly to {} bytes",
        scene.log().len()
    );

    // Repeated traps across frames must also respect the log cap.
    let trap_src = r#"
fn update() {
    let x = 1 / 0
}
"#;
    let mut trap_scene = ScriptScene::new(trap_src, 1, 10_000).unwrap();
    for _ in 0..5_000 {
        cmds.clear();
        trap_scene.update(0, &mut cmds);
    }
    assert!(trap_scene.log().len() <= 256 * 1024);
}

#[test]
fn fx_round_saturates_and_draw_giant_circle_does_not_vanish() {
    assert_eq!(Fx(i64::MAX).round(), Fx(i64::MAX));
    assert_eq!(Fx(i64::MAX - 1).round(), Fx(i64::MAX));
    assert_eq!(Fx(i64::MIN).round(), Fx(i64::MIN));

    let cam = Camera {
        center: Vec2::new(Fx::ZERO, Fx::ZERO),
        scale: Fx::from_int(2),
    };
    let (sx, _) = to_screen(&cam, 16, 16, Vec2::new(Fx(i64::MAX), Fx::ZERO));
    assert_eq!(sx, i32::MAX);

    let mut world = PhysWorld::new(Vec2::new(Fx::ZERO, Fx::ZERO));
    let id = world.add(Body {
        pos: Vec2::new(Fx::ZERO, Fx::ZERO),
        vel: Vec2::new(Fx::ZERO, Fx::ZERO),
        inv_mass: Fx::ZERO,
        restitution: Fx::ZERO,
        shape: Shape::Circle { r: Fx(i64::MAX) },
    });
    let mut canvas = Canvas::new(16, 16, Rgba(0, 0, 0, 255));
    let fg = Rgba(255, 0, 0, 255);
    draw_world(&mut canvas, &cam, &world, &|bid| {
        assert_eq!(bid, id);
        fg
    });
    assert_eq!(canvas.get(8, 8), Some(fg));
}

#[test]
fn audio_mixer_and_wav_hostile_inputs() {
    assert!(tone(0, 440, 1000, 100).samples.is_empty());
    assert!(tone(44_100, 0, 0, 100).samples.is_empty());

    let mut mixer = Mixer::new(0);
    let empty = Sound {
        rate: 44_100,
        samples: Vec::new(),
    };
    let v_empty = mixer.play(&empty, Fx::ONE, Fx::ZERO, false);
    assert_eq!(mixer.playing(), 0);
    assert!(!mixer.stop(v_empty));

    let mut out = [99i16; 5];
    mixer.mix(&mut out);
    assert_eq!(out, [0, 0, 0, 0, 0]);

    let mut fast_mixer = Mixer::new(1);
    let high_rate = Sound {
        rate: u32::MAX,
        samples: vec![i16::MIN, i16::MAX],
    };
    let vid = fast_mixer.play(&high_rate, Fx(i64::MAX), Fx(i64::MIN), true);
    assert_eq!(fast_mixer.playing(), 1);
    assert!(fast_mixer.set_volume(vid, Fx(i64::MIN)));
    let mut odd_buf = [123i16; 7];
    fast_mixer.mix(&mut odd_buf);
    assert_eq!(odd_buf[6], 0);

    let w = wav(u32::MAX, &[i16::MIN, i16::MAX, 0]);
    assert_eq!(w.len(), 44 + 6);
    assert_eq!(&w[0..4], b"RIFF");
}

#[test]
fn scene_recorder_decode_rejects_malformed_and_oversized() {
    assert!(Recorder::decode(b"").is_none());
    assert!(Recorder::decode(b"TRE").is_none());
    assert!(Recorder::decode(b"NOPE\x00\x00\x00\x00").is_none());

    // Claim u32::MAX entries with only an 8-byte buffer (must not allocate or panic).
    let mut huge_count = Vec::from(*b"TREC");
    huge_count.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(Recorder::decode(&huge_count).is_none());

    // Claim 1 entry but provide truncated payload.
    let mut truncated = Vec::from(*b"TREC");
    truncated.extend_from_slice(&1u32.to_le_bytes());
    truncated.extend_from_slice(&[0u8; 11]);
    assert!(Recorder::decode(&truncated).is_none());

    // Claim 0 entries with trailing garbage.
    let mut trailing = Vec::from(*b"TREC");
    trailing.extend_from_slice(&0u32.to_le_bytes());
    trailing.push(0);
    assert!(Recorder::decode(&trailing).is_none());

    // Unsorted frames (frame 10 followed by frame 9).
    let mut unsorted = Vec::from(*b"TREC");
    unsorted.extend_from_slice(&2u32.to_le_bytes());
    unsorted.extend_from_slice(&10u64.to_le_bytes());
    unsorted.extend_from_slice(&1u32.to_le_bytes());
    unsorted.extend_from_slice(&9u64.to_le_bytes());
    unsorted.extend_from_slice(&2u32.to_le_bytes());
    assert!(Recorder::decode(&unsorted).is_none());
}

#[test]
fn script_runaway_infinite_loop_halts_within_fuel() {
    let start = Instant::now();
    let src_update = r#"
fn update() {
    while true {
    }
}
"#;
    let scene = ScriptScene::new(src_update, 0, 5_000).unwrap();
    let mut stage = Stage::new(Box::new(scene));
    assert!(!stage.step(0));
    assert_eq!(stage.depth(), 0);

    let src_init = r#"
fn init() {
    while true {
    }
}
fn update() {}
"#;
    let mut scene_init = ScriptScene::new(src_init, 0, 5_000).unwrap();
    let mut cmds = Vec::new();
    scene_init.update(0, &mut cmds);
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], Command::Pop));
    assert!(scene_init.log().contains("trap:"));
    assert!(start.elapsed().as_secs_f64() < 1.0);
}

#[test]
fn script_hostile_extern_types_and_oob_slots() {
    // Mismatched extern signature triggers Trap::TypeError cleanly without panic.
    let bad_extern = r#"
fn bad_log(x: int) = extern "game.log"
fn update() {
    bad_log(42)
}
"#;
    let mut s1 = ScriptScene::new(bad_extern, 0, 10_000).unwrap();
    let mut cmds = Vec::new();
    s1.update(0, &mut cmds);
    assert!(matches!(cmds.as_slice(), [Command::Pop]));
    assert!(s1.log().contains("TypeError"));

    // Out-of-bounds slots and negative/zero rand bounds do not panic or hang.
    let oob_src = r#"
fn g_get(i: int) -> int = extern "game.get"
fn g_set(i: int, v: int) = extern "game.set"
fn g_rand(n: int) -> int = extern "game.rand"
fn update() {
    g_set(-1, 99)
    g_set(256, 99)
    g_set(999999999999, 99)
    g_set(0, g_get(-1) + g_get(256) + g_rand(0) + g_rand(-100))
}
"#;
    let mut s2 = ScriptScene::new(oob_src, 7, 10_000).unwrap();
    cmds.clear();
    s2.update(0, &mut cmds);
    assert!(cmds.is_empty());
    assert_eq!(s2.slot(0), 0);
    assert_eq!(s2.slot(usize::MAX), 0);

    // Invalid entrypoints rejected at load time.
    assert!(ScriptScene::new("fn update() = extern \"game.quit\"", 0, 100).is_err());
    assert!(ScriptScene::new("fn update(x: int) {}", 0, 100).is_err());
    assert!(ScriptScene::new("fn init() -> int { 1 }\nfn update() {}", 0, 100).is_err());
}

#[test]
fn phys_world_extreme_bounds_and_degenerate_shapes() {
    let mut world = PhysWorld::new(Vec2::new(Fx(i64::MIN), Fx(i64::MAX)));
    let a = world.add(Body {
        pos: Vec2::new(Fx(i64::MIN), Fx(i64::MAX)),
        vel: Vec2::new(Fx(i64::MAX), Fx(i64::MIN)),
        inv_mass: Fx(i64::MAX),
        restitution: Fx(i64::MAX),
        shape: Shape::Circle { r: Fx(i64::MIN) },
    });
    let b = world.add(Body {
        pos: Vec2::new(Fx(i64::MIN), Fx(i64::MAX)),
        vel: Vec2::new(Fx(i64::MIN), Fx(i64::MAX)),
        inv_mass: Fx(i64::MIN),
        restitution: Fx(i64::MIN),
        shape: Shape::Aabb {
            half: Vec2::new(Fx(i64::MIN), Fx(i64::MAX)),
        },
    });
    let c = world.add(Body {
        pos: Vec2::new(Fx::ZERO, Fx::ZERO),
        vel: Vec2::new(Fx(i64::MAX), Fx(i64::MAX)),
        inv_mass: Fx::ONE,
        restitution: Fx::ONE,
        shape: Shape::Aabb {
            half: Vec2::new(Fx(i64::MAX), Fx(i64::MAX)),
        },
    });

    world.step(Fx(i64::MIN));
    world.step(Fx::ZERO);
    world.step(Fx(i64::MAX));
    let _ = world.kinetic_energy();
    let _ = world.contacts();
    let _ = collide(world.body(a).unwrap(), world.body(c).unwrap());
    let _ = collide(world.body(b).unwrap(), world.body(c).unwrap());
    assert!(!world.remove(usize::MAX));
    assert!(world.body(usize::MAX).is_none());
}

#[test]
fn ecs_stale_and_forged_entities() {
    let mut ecs = EcsWorld::new();
    let e = ecs.spawn();
    assert!(ecs.insert(e, 42u32));
    assert!(ecs.despawn(e));
    assert!(!ecs.alive(e));
    assert!(!ecs.despawn(e));
    assert!(!ecs.insert(e, 99u32));
    assert!(ecs.get::<u32>(e).is_none());
    assert!(ecs.get_mut::<u32>(e).is_none());
    assert!(ecs.remove::<u32>(e).is_none());

    let forged = helios::ecs::Entity {
        index: u32::MAX,
        generation: u32::MAX,
    };
    assert!(!ecs.alive(forged));
    assert!(!ecs.despawn(forged));
    assert!(!ecs.insert(forged, 1u32));
    assert!(ecs.get::<u32>(forged).is_none());

    let e2 = ecs.spawn();
    assert!(ecs.insert(e2, 7u32));
    let mut visited = 0usize;
    ecs.each2::<u32, u32>(|_, _, _| {
        visited += 1;
    });
    assert_eq!(visited, 0);
}

#[test]
fn clock_and_rng_extreme_values() {
    let mut c0 = Clock::new(0, 0);
    assert_eq!(c0.advance(u64::MAX), 0);
    assert_eq!(c0.alpha(), Fx::ZERO);

    let mut c_max = Clock::new(u64::MAX, u32::MAX);
    assert_eq!(c_max.advance(u64::MAX), 1);
    assert_eq!(c_max.alpha(), Fx::ZERO);
    let _ = c_max.advance(u64::MAX - 1);
    assert!(c_max.alpha() >= Fx::ZERO && c_max.alpha() < Fx::ONE);

    let mut pcg = Pcg32::new(u64::MAX, u64::MAX);
    assert_eq!(pcg.below(0), 0);
    assert!(pcg.below(u32::MAX) < u32::MAX);
}

#[test]
fn fx_extreme_arithmetic_and_trig() {
    for &x in &[
        Fx(i64::MIN),
        Fx(i64::MIN + 1),
        Fx(-1),
        Fx::ZERO,
        Fx(1),
        Fx(i64::MAX),
    ] {
        let _ = x + Fx(i64::MAX);
        let _ = x - Fx(i64::MAX);
        let _ = -x;
        let _ = x.abs();
        let _ = x.floor();
        let _ = x.round();
        let _ = x.sqrt();
        let _ = x.sin();
        let _ = x.cos();
        let _ = x * Fx(i64::MIN);
        let _ = x / Fx::ZERO;
        let _ = x / Fx(-1);
        let _ = Fx::atan2(x, Fx(i64::MIN));
        let v = Vec2::new(x, Fx(i64::MIN));
        let _ = v.len();
        let _ = v.normalized();
    }
    assert_eq!(Fx::from_ratio(i64::MIN, -1), Fx(i64::MAX));
    assert_eq!(Fx::from_ratio(0, 0), Fx::ZERO);
}
