//! Leader check for T70a: a model mixer compared sample by sample, scene and replay properties.
#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
use assay::Rng;
use tectonic::audio::{tone, wav, Mixer, Sound, VoiceId};
use tectonic::fx::Fx;
use tectonic::scene::{replay, Command, Input, Recorder, Scene, Stage};

struct MVoice {
    id: u64,
    s: Sound,
    gl: i64,
    gr: i64,
    pan: i64,
    pos: u64,
    step: u64,
    looped: bool,
    live: bool,
}

struct Model {
    rate: u32,
    next: u64,
    v: Vec<MVoice>,
}

fn gains(vol: i64, pan: i64) -> (i64, i64) {
    ((vol * (65536 - pan)) >> 17, (vol * (65536 + pan)) >> 17)
}

impl Model {
    fn play(&mut self, s: &Sound, vol: i64, pan: i64, looped: bool) -> u64 {
        let (vol, pan) = (vol.clamp(0, 65536), pan.clamp(-65536, 65536));
        let (gl, gr) = gains(vol, pan);
        let id = self.next;
        self.next += 1;
        let live = !s.samples.is_empty() && s.rate != 0;
        let step = if self.rate == 0 {
            0
        } else {
            (u64::from(s.rate) << 32) / u64::from(self.rate)
        };
        self.v.push(MVoice {
            id,
            s: s.clone(),
            gl,
            gr,
            pan,
            pos: 0,
            step,
            looped,
            live,
        });
        id
    }
    fn mix(&mut self, out: &mut [i16]) {
        out.fill(0);
        if self.rate == 0 {
            return;
        }
        for f in 0..out.len() / 2 {
            let (mut l, mut r) = (0i64, 0i64);
            for v in self.v.iter_mut().filter(|v| v.live) {
                let s = i64::from(v.s.samples[(v.pos >> 32) as usize]);
                l += (s * v.gl) >> 16;
                r += (s * v.gr) >> 16;
                v.pos += v.step;
                let len = (v.s.samples.len() as u64) << 32;
                if v.pos >= len {
                    if v.looped {
                        while v.pos >= len {
                            v.pos -= len;
                        }
                    } else {
                        v.live = false;
                    }
                }
            }
            out[2 * f] = l.clamp(-32768, 32767) as i16;
            out[2 * f + 1] = r.clamp(-32768, 32767) as i16;
        }
    }
}

fn rand_sound(r: &mut Rng) -> Sound {
    let rate = *r.pick(&[8000, 11025, 22050, 44100, 48000, 96000]);
    if r.chance(1, 2) {
        return tone(
            rate,
            50 + r.below(2000) as u32,
            r.below(40) as u32,
            (r.below(40000) as i32 - 20000) as i16,
        );
    }
    let n = r.below(300);
    Sound {
        rate,
        samples: (0..n).map(|_| (r.u64() as u16) as i16).collect(),
    }
}

#[test]
fn mixer_matches_model() {
    let mut r = Rng::new(0x70A_0001);
    for case in 0..60 {
        let rate = *r.pick(&[0, 8000, 22050, 44100, 48000]);
        let (mut m, mut model) = (
            Mixer::new(rate),
            Model {
                rate,
                next: 0,
                v: Vec::new(),
            },
        );
        for round in 0..12 {
            for _ in 0..r.below(4) {
                let s = rand_sound(&mut r);
                let vol = r.below(100_000) as i64 - 10_000;
                let pan = r.below(160_000) as i64 - 80_000;
                let lp = r.chance(1, 3);
                let id = m.play(&s, Fx(vol), Fx(pan), lp);
                assert_eq!(
                    id,
                    VoiceId(model.play(&s, vol, pan, lp)),
                    "case {case}: ids"
                );
            }
            if r.chance(1, 3) && model.next > 0 {
                let id = r.below(model.next as usize + 1) as u64;
                let mv = model.v.iter_mut().find(|v| v.id == id && v.live);
                let want = mv.is_some();
                if let Some(v) = mv {
                    v.live = false;
                }
                assert_eq!(m.stop(VoiceId(id)), want, "case {case}: stop {id}");
            }
            if r.chance(1, 3) && model.next > 0 {
                let id = r.below(model.next as usize) as u64;
                let vol = r.below(90_000) as i64 - 5000;
                let mv = model.v.iter_mut().find(|v| v.id == id && v.live);
                let want = mv.is_some();
                if let Some(v) = mv {
                    (v.gl, v.gr) = gains(vol.clamp(0, 65536), v.pan);
                }
                assert_eq!(
                    m.set_volume(VoiceId(id), Fx(vol)),
                    want,
                    "case {case}: set_volume {id}"
                );
            }
            let n = r.below(700);
            let (mut a, mut b) = (vec![7i16; n], vec![0i16; n]);
            m.mix(&mut a);
            model.mix(&mut b);
            if n % 2 == 1 {
                b[n - 1] = 0;
            }
            assert_eq!(a, b, "case {case} round {round}: samples differ");
            assert_eq!(
                m.playing(),
                model.v.iter().filter(|v| v.live).count(),
                "case {case}: playing"
            );
        }
    }
}

#[test]
fn tone_and_wav() {
    let t = tone(8000, 1000, 2, 1000);
    assert_eq!(t.samples.len(), 16);
    let want: Vec<i16> = (0..16u64)
        .map(|i| {
            if (i * 2000 / 8000) % 2 == 0 {
                1000
            } else {
                -1000
            }
        })
        .collect();
    assert_eq!(t.samples, want);
    assert_eq!(tone(0, 440, 100, 5).samples.len(), 0);
    assert!(tone(1000, 0, 10, 9).samples.iter().all(|&s| s == 9));
    let w = wav(44100, &[1, -2, 3, -4]);
    assert_eq!(w.len(), 52);
    assert_eq!(&w[0..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 44);
    assert_eq!(&w[8..16], b"WAVEfmt ");
    assert_eq!(u32::from_le_bytes(w[16..20].try_into().unwrap()), 16);
    assert_eq!(&w[20..24], &[1, 0, 2, 0]);
    assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 44100);
    assert_eq!(u32::from_le_bytes(w[28..32].try_into().unwrap()), 44100 * 4);
    assert_eq!(&w[32..36], &[4, 0, 16, 0]);
    assert_eq!(&w[36..40], b"data");
    assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 8);
    assert_eq!(&w[44..], &[1, 0, 0xfe, 0xff, 3, 0, 0xfc, 0xff]);
}

/// A counter scene: keys bit 0 adds, bit 1 pushes a child, bit 2 pops, bit 3 replaces, bit 4 quits.
struct Counter {
    n: u64,
    depth: u64,
}

impl Scene for Counter {
    fn update(&mut self, keys: u32, out: &mut Vec<Command>) {
        self.n = self.n.wrapping_mul(31).wrapping_add(u64::from(keys) + 1);
        if keys & 2 != 0 && self.depth < 6 {
            out.push(Command::Push(Box::new(Counter {
                n: self.n,
                depth: self.depth + 1,
            })));
        }
        if keys & 4 != 0 {
            out.push(Command::Pop);
        }
        if keys & 8 != 0 {
            out.push(Command::Replace(Box::new(Counter {
                n: self.n ^ 0xff,
                depth: self.depth,
            })));
        }
        if keys & 16 != 0 {
            out.push(Command::Quit);
            out.push(Command::Push(Box::new(Counter { n: 0, depth: 0 })));
        }
    }
    fn state_hash(&self) -> u64 {
        self.n ^ (self.depth << 56)
    }
}

fn fnv(bytes: &[u8], mut h: u64) -> u64 {
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
    }
    h
}

#[test]
fn stage_hash_and_replay() {
    let mut s = Stage::new(Box::new(Counter { n: 5, depth: 0 }));
    let mut h = fnv(&0u64.to_le_bytes(), 0xcbf2_9ce4_8422_2325);
    h = fnv(&1u64.to_le_bytes(), h);
    h = fnv(&(5u64).to_le_bytes(), h);
    assert_eq!(s.hash(), fnv(&[0], h), "hash of a fresh stage");
    assert!(s.step(2));
    assert_eq!((s.depth(), s.frame()), (2, 1));
    assert!(s.step(4));
    assert_eq!(s.depth(), 1);
    assert!(!s.step(16), "quit stops, later commands ignored");
    assert_eq!(s.depth(), 1);
    assert!(!s.step(0));
    assert_eq!(s.frame(), 3);

    let mut r = Rng::new(0x70A_0002);
    for case in 0..50 {
        let mut live = Stage::new(Box::new(Counter { n: case, depth: 0 }));
        let mut rec = Recorder::new();
        let frames = 50 + r.below(200) as u64;
        let mut keys = 0u32;
        for _ in 0..frames {
            if r.chance(1, 6) {
                keys = (r.below(16) as u32) | if r.chance(1, 40) { 16 } else { 0 };
            }
            rec.record(live.frame(), keys);
            if !live.step(keys) {
                break;
            }
        }
        let back = Recorder::decode(&rec.encode()).unwrap();
        assert_eq!(back, rec);
        assert!(
            rec.inputs
                .windows(2)
                .all(|w| w[0].keys != w[1].keys && w[0].frame < w[1].frame),
            "only changes stored"
        );
        let mut again = Stage::new(Box::new(Counter { n: case, depth: 0 }));
        assert_eq!(
            replay(&mut again, &back, frames),
            live.hash(),
            "case {case}: replay differs"
        );
    }
}

#[test]
fn recorder_rules_and_hostile_decode() {
    let mut rec = Recorder::new();
    rec.record(0, 0);
    assert!(rec.inputs.is_empty(), "keys 0 at start is no change");
    rec.record(3, 5);
    rec.record(3, 6);
    rec.record(2, 9);
    rec.record(7, 6);
    assert_eq!(rec.inputs, vec![Input { frame: 3, keys: 6 }]);
    assert_eq!(
        (rec.keys_at(2), rec.keys_at(3), rec.keys_at(100)),
        (0, 6, 6)
    );
    let mut r = Rng::new(0x70A_0003);
    for _ in 0..20_000 {
        let _ = Recorder::decode(&r.bytes(40));
    }
    let mut good = Recorder {
        inputs: vec![Input { frame: 1, keys: 2 }, Input { frame: 5, keys: 3 }],
    }
    .encode();
    assert_eq!(good.len(), 8 + 24);
    assert!(Recorder::decode(&good[..good.len() - 1]).is_none());
    good[12..20].copy_from_slice(&9u64.to_le_bytes());
    assert!(Recorder::decode(&good).is_none(), "decreasing frames");
}
