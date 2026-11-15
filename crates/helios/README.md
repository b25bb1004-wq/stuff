# helios

helios is the game engine core of Monolith. It keeps the simulation in integers: fixed-point maths, an entity system, rigid-body physics, a fixed time step, seeded random numbers, a scene stack with input replay, and a small audio mixer. The same inputs give the same bits on every platform, so a recorded run can be replayed and checked by its state hash. The crate is `no_std` with `alloc`.

## Features

- Q47.16 fixed-point scalar `Fx`: arithmetic with rounding, `sqrt`, `floor`, `round`, `from_ratio`, and `sin`, `cos`, `atan2`.
- `Vec2` with dot and cross products, length, normalisation and scaling.
- Entities with generations, typed component storage, spawn and despawn, and iteration in slot order.
- FNV-1a hash of the whole entity world, used for state checks.
- PCG32 random numbers: the same seed and stream always give the same values.
- Fixed time step `Clock` that turns elapsed microseconds into whole steps, with a cap per call.
- Physics: circles and axis-aligned boxes, impulse resolution with substeps, positional correction, contact list and kinetic energy. No rotation.
- Software drawing of physics worlds onto a pigment canvas through an orthographic camera.
- Audio: square-wave `tone`, a `Mixer` with resampling, panning, looping voices and volume, and a 16-bit stereo WAV encoder.
- Scene stack with push, pop, replace and quit, a run-length input recorder with binary encode and decode, and `replay`.
- Scripted scenes: Logos source run on the Elysium VM with fuel per frame, plus a rectangle renderer. The built-in game source is `GAME_SRC`.

## Public API

| Item | What it does |
| --- | --- |
| `fx::Fx`, `fx::Vec2` | Fixed-point scalar and 2-D vector |
| `ecs::World`, `ecs::Entity` | Entities and typed components, with `hash()` |
| `phys::World`, `phys::Body`, `phys::Shape` | Physics world, bodies and shapes |
| `phys::collide(&Body, &Body)` | Overlap test returning normal and depth |
| `rng::Pcg32` | Seeded random number generator |
| `Clock` | Fixed time step from elapsed microseconds |
| `scene::Scene`, `scene::Stage` | Scene trait and the stack that runs it |
| `scene::Recorder`, `scene::replay` | Input recording, codec and replay |
| `audio::tone`, `audio::wav`, `audio::Mixer` | Tone generator, WAV encoder and mixer |
| `draw::Camera`, `draw::draw_world` | Camera and physics drawing |
| `script::ScriptScene`, `script::render` | Scripted scene and rectangle renderer |

## Demo

The demo prints fixed-point results, runs a ball bouncing on a floor for 120 steps, draws five seeded dice, steps a clock, hashes an entity world, replays a recorded input twice, and encodes a tone as WAV. It uses fixed input, reads no clock, and needs no network.

```
cargo run -p helios --example demo
```

Real output:

```text
fx: 1/3 raw=21845 times 3 raw=65535
fx: sqrt(2) = 1.4142
fx: sin(pi/2) raw=65536
vec: len of (3, 4) = 5
phys: after  30 steps ball y = 3.709
phys: after  60 steps ball y = -0.081
phys: after  90 steps ball y = -3.510
phys: after 120 steps ball y = -3.510
phys: kinetic energy raw = 0
rng: first word 2707161783, dice [4, 4, 3, 2, 2]
clock: steps per call [0, 1, 3]
ecs: 3 entities, position sum 30, hash 5f94a4fbbf709853
scene: replay 8 frames hash 441d71e4c0e4a71f
scene: replay again equal = true
audio: 80 mono samples, WAV is 364 bytes
demo: done
```

## Tests

```
cargo test -p helios
```

The crate's gate is `bash tools/check.sh helios`, which runs format, clippy with warnings denied, the tests, and the zero-dependency check. The tests include property tests over fixed inputs.
