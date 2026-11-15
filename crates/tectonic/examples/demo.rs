//! Demo: fixed-point maths, a bouncing ball, seeded dice, a fixed clock, an ECS hash,
//! a replayed scene, and a tone written as WAV. Integer-only, no clock, no network.

use tectonic::audio::{tone, wav};
use tectonic::ecs::World;
use tectonic::fx::{Fx, Vec2};
use tectonic::phys::{Body, Shape, World as Physics};
use tectonic::rng::Pcg32;
use tectonic::scene::{replay, Command, Recorder, Scene, Stage};
use tectonic::Clock;

#[derive(Clone, Hash)]
struct Pos(i32);

/// A scene that adds the key bits it is given to a running total.
struct Counter {
    total: u64,
}

impl Scene for Counter {
    fn update(&mut self, keys: u32, _out: &mut Vec<Command>) {
        self.total = self.total.wrapping_add(u64::from(keys));
    }

    fn state_hash(&self) -> u64 {
        self.total
    }
}

fn main() {
    // 1. Fixed-point numbers: Q47.16, so 1/3 is stored as a rounded integer.
    let third = Fx::from_ratio(1, 3);
    let times3 = third.mul(Fx::from_int(3)).0;
    println!("fx: 1/3 raw={} times 3 raw={times3}", third.0);
    let root = Fx::from_int(2).sqrt().0 as f64 / 65536.0;
    println!("fx: sqrt(2) = {root:.4}");
    let sin = Fx::PI.mul(Fx::HALF).sin().0;
    println!("fx: sin(pi/2) raw={sin}");
    let v = Vec2::new(Fx::from_int(3), Fx::from_int(4));
    println!("vec: len of (3, 4) = {}", v.len().to_int());

    // 2. Physics: a ball falls onto a static floor and bounces.
    let mut world = Physics::new(Vec2::new(Fx::ZERO, Fx::from_int(-10)));
    world.add(Body {
        pos: Vec2::new(Fx::ZERO, Fx::from_int(-5)),
        vel: Vec2::new(Fx::ZERO, Fx::ZERO),
        inv_mass: Fx::ZERO,
        restitution: Fx::ZERO,
        shape: Shape::Aabb {
            half: Vec2::new(Fx::from_int(10), Fx::ONE),
        },
    });
    let ball = world.add(Body {
        pos: Vec2::new(Fx::ZERO, Fx::from_int(5)),
        vel: Vec2::new(Fx::ZERO, Fx::ZERO),
        inv_mass: Fx::ONE,
        restitution: Fx::HALF,
        shape: Shape::Circle { r: Fx::HALF },
    });
    let dt = Fx::from_ratio(1, 60);
    for step in 1..=120 {
        world.step(dt);
        if step % 30 == 0 {
            let y = world.body(ball).map_or(0.0, |b| b.pos.y.0 as f64 / 65536.0);
            println!("phys: after {step:3} steps ball y = {y:.3}");
        }
    }
    println!("phys: kinetic energy raw = {}", world.kinetic_energy().0);

    // 3. Random numbers: the same seed always gives the same stream.
    let mut rng = Pcg32::new(42, 54);
    let dice: Vec<u32> = (0..5).map(|_| rng.below(6) + 1).collect();
    let first = Pcg32::new(42, 54).next_u32();
    println!("rng: first word {first}, dice {dice:?}");

    // 4. Clock: real microseconds become whole fixed steps.
    let mut clock = Clock::new(16_667, 4);
    let steps: Vec<u32> = [10_000, 10_000, 50_000]
        .into_iter()
        .map(|us| clock.advance(us))
        .collect();
    println!("clock: steps per call {steps:?}");

    // 5. ECS: entities and components, hashed in slot order.
    let mut ecs = World::new();
    for i in 0..3 {
        let e = ecs.spawn();
        ecs.insert(e, Pos(i * 10));
    }
    let mut sum = 0;
    ecs.each(|_e, p: &mut Pos| sum += p.0);
    let (count, hash) = (ecs.entities().len(), ecs.hash());
    println!("ecs: {count} entities, position sum {sum}, hash {hash:016x}");

    // 6. Scenes: the same recorded input replays to the same state hash.
    let mut rec = Recorder::new();
    rec.record(0, 1);
    rec.record(3, 2);
    rec.record(6, 0);
    let run = |frames| {
        replay(
            &mut Stage::new(Box::new(Counter { total: 0 })),
            &rec,
            frames,
        )
    };
    let (first_run, second_run) = (run(8), run(8));
    println!("scene: replay 8 frames hash {first_run:016x}");
    println!("scene: replay again equal = {}", first_run == second_run);

    // 7. Audio: a 440 Hz tone, stereo-duplicated and encoded as WAV.
    let mono = tone(8_000, 440, 10, 1_000);
    let stereo: Vec<i16> = mono.samples.iter().flat_map(|&s| [s, s]).collect();
    let bytes = wav(8_000, &stereo).len();
    println!(
        "audio: {} mono samples, WAV is {bytes} bytes",
        mono.samples.len()
    );
    println!("demo: done");
}
