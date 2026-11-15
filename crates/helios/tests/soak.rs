//! Soak tests: random and mutated inputs against every script, scene-codec and audio entry point.
//! Budget per target: `STRATA_SOAK_MS` (default 300). Seed: `STRATA_SOAK_SEED` (decimal or 0x hex).
//! Run with `--nocapture` to see the per-target seed and case counts.
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use assay::Rng;
use helios::audio::{tone, wav, Mixer, Sound, VoiceId};
use helios::fx::Fx;
use helios::scene::{replay, Recorder, Scene, Stage};
use helios::script::{render, ScriptScene, GAME_SRC};

const DEFAULT_SEED: u64 = 0x05EE_D7A1;
const DEFAULT_MS: u64 = 300;
const CASE_LIMIT: Duration = Duration::from_secs(1);
const MAX_BYTES: usize = 64 * 1024;
const FUEL: u64 = 20_000;

const SCRIPT_SEED: &str = "fn g_get(i: int) -> int = extern \"game.get\"\n\
fn g_set(i: int, v: int) = extern \"game.set\"\n\
fn g_rect(x: int, y: int, w: int, h: int, c: int) = extern \"game.rect\"\n\
fn update() {\n\
    g_set(0, g_get(0) + 1)\n\
    g_rect(1, 2, 3, 4, 5)\n\
}\n";

const TOKENS: &[&str] = &[
    "fn ",
    "let ",
    "var ",
    "if ",
    "else ",
    "for ",
    "in ",
    "while ",
    "return ",
    "extern ",
    "int",
    "str",
    "bool",
    "unit",
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    ",",
    ":",
    ";",
    "=",
    "==",
    "!=",
    "<",
    ">",
    "+",
    "-",
    "*",
    "/",
    "%",
    "&",
    "|",
    "^",
    "<<",
    ">>",
    "!",
    "&&",
    "||",
    "\"game.get\"",
    "\"",
    "\n",
    " ",
    "0",
    "1",
    "-1",
    "0x7fffffffffffffff",
    "9223372036854775808",
    "true",
    "false",
    "update",
    "init",
    "g_set",
    "é",
    "→",
    "//",
    "/*",
    "\t",
    "\r\n",
    "..",
    "g_get(0)",
    "\u{0}",
    "\u{10ffff}",
];

/// Seed parsed from `STRATA_SOAK_SEED`, or the default.
fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(s) => parse_u64(&s).expect("STRATA_SOAK_SEED must be decimal or 0x-prefixed hex"),
        Err(_) => DEFAULT_SEED,
    }
}

/// Budget parsed from `STRATA_SOAK_MS`, or the default.
fn soak_budget() -> Duration {
    let ms = match std::env::var("STRATA_SOAK_MS") {
        Ok(s) => s
            .trim()
            .parse::<u64>()
            .expect("STRATA_SOAK_MS must be a whole number"),
        Err(_) => DEFAULT_MS,
    };
    Duration::from_millis(ms)
}

fn parse_u64(s: &str) -> Option<u64> {
    let t: String = s.trim().chars().filter(|c| *c != '_').collect();
    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => t.parse().ok(),
    }
}

/// Prints the replay seed of a case that panics, so it can be re-run in isolation.
struct ReplayGuard {
    target: &'static str,
    case_seed: u64,
}

impl Drop for ReplayGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "soak: target {} failed; replay case seed {:#018x}",
                self.target, self.case_seed
            );
        }
    }
}

/// Run `case` on fresh streams derived from the soak seed until the budget ends (at least once).
fn run_target(target: &'static str, case: fn(&mut Rng)) {
    let seed = soak_seed();
    let budget = soak_budget();
    let name_hash = target.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    let mut master = Rng::new(seed ^ name_hash);
    let deadline = Instant::now() + budget;
    let mut cases = 0u64;
    loop {
        let case_seed = master.u64();
        let guard = ReplayGuard { target, case_seed };
        let started = Instant::now();
        case(&mut Rng::new(case_seed));
        let took = started.elapsed();
        assert!(
            took <= CASE_LIMIT,
            "{target}: case seed {case_seed:#018x} took {took:?}"
        );
        drop(guard);
        cases += 1;
        if Instant::now() >= deadline {
            break;
        }
    }
    eprintln!(
        "soak: target={target} seed={seed:#x} budget_ms={} cases={cases}",
        budget.as_millis()
    );
}

/// Random bytes, or in half the cases a token soup, so the lexer and parser see realistic text.
fn random_text(rng: &mut Rng) -> String {
    if rng.chance(1, 2) {
        let mut s = String::new();
        for _ in 0..rng.below(300) {
            s.push_str(rng.pick(TOKENS));
        }
        s
    } else {
        let raw = rng.bytes(4096);
        let mut s = String::from_utf8_lossy(&raw).into_owned();
        for _ in 0..rng.below(64) {
            if let Some(c) = char::from_u32(rng.below(0x11_0000) as u32) {
                s.push(c);
            }
        }
        s
    }
}

/// Apply 1 to 4 random edits (bit flip, truncation, splice, repeated chunk) to a valid input.
fn mutate(rng: &mut Rng, valid: &[u8]) -> Vec<u8> {
    let mut out = valid.to_vec();
    for _ in 0..=rng.below(4) {
        if out.is_empty() {
            out.push(rng.u64() as u8);
        }
        match rng.below(4) {
            0 => {
                let i = rng.below(out.len());
                out[i] ^= 1u8 << rng.below(8);
            }
            1 => {
                let keep = rng.below(out.len() + 1);
                out.truncate(keep);
            }
            2 => {
                let (a, b) = chunk_bounds(rng, out.len());
                let chunk = out[a..b].to_vec();
                let at = rng.below(out.len() + 1);
                out.splice(at..at, chunk);
            }
            _ => {
                let (a, b) = chunk_bounds(rng, out.len());
                let chunk = out[a..b].to_vec();
                let at = rng.below(out.len() + 1);
                for _ in 0..=rng.below(8) {
                    out.splice(at..at, chunk.iter().copied());
                }
            }
        }
        out.truncate(MAX_BYTES);
    }
    out
}

fn chunk_bounds(rng: &mut Rng, len: usize) -> (usize, usize) {
    let a = rng.below(len);
    let b = a + rng.below(len - a + 1);
    (a, b)
}

fn random_keys(rng: &mut Rng) -> u32 {
    rng.u64() as u32
}

/// Source text for the script targets: half random text, half mutations of the built-in game
/// or the small seed script.
fn script_source(rng: &mut Rng) -> String {
    if rng.chance(1, 3) {
        return random_text(rng);
    }
    let base = if rng.chance(1, 2) {
        GAME_SRC
    } else {
        SCRIPT_SEED
    };
    if rng.chance(1, 2) {
        String::from_utf8_lossy(&mutate(rng, base.as_bytes())).into_owned()
    } else {
        mutate_lines(rng, base)
    }
}

/// Line-level edits (delete, duplicate, swap, nudge a literal or operator): most results still parse.
fn mutate_lines(rng: &mut Rng, base: &str) -> String {
    let mut lines: Vec<String> = base.lines().map(str::to_owned).collect();
    for _ in 0..=rng.below(3) {
        if lines.is_empty() {
            break;
        }
        let i = rng.below(lines.len());
        match rng.below(6) {
            0 => {
                lines.remove(i);
            }
            1 => {
                let copy = lines[i].clone();
                lines.insert(i, copy);
            }
            2 => {
                let j = rng.below(lines.len());
                lines.swap(i, j);
            }
            3 => lines[i] = lines[i].replacen('0', &rng.below(40).to_string(), 1),
            4 => lines[i] = lines[i].replacen('+', "-", 1),
            _ => lines[i] = lines[i].replacen(' ', "", 1),
        }
    }
    lines.join("\n")
}

fn script_load_case(rng: &mut Rng) {
    let src = script_source(rng);
    let Ok(mut scene) = ScriptScene::new(&src, rng.u64(), FUEL) else {
        return;
    };
    let mut out = Vec::new();
    for _ in 0..=rng.below(8) {
        scene.update(random_keys(rng), &mut out);
        out.clear();
        let _ = render(scene.draws(), 64, 48);
    }
}

/// Game steps with random inputs; the same inputs twice must give the same state hash.
fn game_run_case(rng: &mut Rng) {
    let seed = rng.u64();
    let frames = 200 + rng.below(200) as u64;
    let mut rec = Recorder::new();
    for f in 0..frames {
        if rng.chance(1, 8) {
            rec.record(f, random_keys(rng));
        }
    }
    let mut hashes = Vec::with_capacity(2);
    for _ in 0..2 {
        let scene = ScriptScene::new(GAME_SRC, seed, 100_000).unwrap();
        let mut stage = Stage::new(Box::new(scene));
        hashes.push(replay(&mut stage, &rec, frames));
    }
    assert_eq!(hashes[0], hashes[1], "game replay is not deterministic");

    let mut game = ScriptScene::new(GAME_SRC, seed, 100_000).unwrap();
    let mut out = Vec::new();
    for f in 0..frames {
        game.update(rec.keys_at(f), &mut out);
        out.clear();
        if f % 16 == 0 {
            let _ = render(game.draws(), 320, 240);
        }
    }
}

fn scene_codec_case(rng: &mut Rng) {
    let mut rec = Recorder::new();
    let mut frame = 0u64;
    for _ in 0..rng.below(300) {
        frame = frame.saturating_add(rng.below(5) as u64);
        rec.record(frame, random_keys(rng));
    }
    let encoded = rec.encode();
    assert_eq!(
        Recorder::decode(&encoded),
        Some(rec.clone()),
        "codec round trip"
    );

    let bytes = match rng.below(3) {
        0 => {
            let mut b = b"TREC".to_vec();
            let count = rng.below(64) as u32;
            b.extend_from_slice(&count.to_le_bytes());
            b.extend_from_slice(&rng.bytes(12 * count as usize + 4));
            b
        }
        1 => {
            let mut b = encoded.clone();
            b.resize(b.len() + rng.below(16), 0);
            mutate(rng, &b)
        }
        _ => rng.bytes(MAX_BYTES),
    };
    if let Some(decoded) = Recorder::decode(&bytes) {
        assert_eq!(
            decoded.encode(),
            bytes,
            "decode accepted bytes that do not re-encode"
        );
    }
}

fn random_fx(rng: &mut Rng) -> Fx {
    match rng.below(6) {
        0 => Fx(i64::MIN),
        1 => Fx(i64::MAX),
        2 => Fx::ZERO,
        3 => Fx::ONE,
        _ => Fx(rng.u64() as i64),
    }
}

fn random_sound(rng: &mut Rng) -> Sound {
    let rate = 1 + rng.below(48_000) as u32;
    let base = if rng.chance(1, 2) {
        tone(
            rate,
            rng.u64() as u32,
            rng.below(200) as u32,
            rng.u64() as i16,
        )
    } else {
        let n = rng.below(4096);
        Sound {
            rate: rng.u64() as u32,
            samples: (0..n).map(|_| rng.u64() as i16).collect(),
        }
    };
    if rng.chance(1, 2) {
        return base;
    }
    // Mutate the samples through their byte form; an odd trailing byte is dropped.
    let bytes: Vec<u8> = base.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mutated = mutate(rng, &bytes);
    let samples = mutated
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| i16::from_le_bytes(*p))
        .collect();
    Sound {
        rate: base.rate,
        samples,
    }
}

fn audio_case(rng: &mut Rng) {
    let mixer_rate = [0u32, 1, 8_000, 22_050, 48_000][rng.below(5)];
    let mut mixer = Mixer::new(mixer_rate);
    let sounds: Vec<Sound> = (0..1 + rng.below(4)).map(|_| random_sound(rng)).collect();
    let mut voices: Vec<VoiceId> = Vec::new();
    let mut plays = 0usize;
    for _ in 0..rng.below(40) {
        match rng.below(5) {
            0 | 1 => {
                let s = rng.pick(&sounds);
                voices.push(mixer.play(s, random_fx(rng), random_fx(rng), rng.chance(1, 2)));
                plays += 1;
            }
            2 if !voices.is_empty() => {
                let v = voices[rng.below(voices.len())];
                let _ = mixer.set_volume(v, random_fx(rng));
            }
            3 if !voices.is_empty() => {
                let v = voices[rng.below(voices.len())];
                let _ = mixer.stop(v);
            }
            _ => {
                let n = rng.below(4097);
                let mut buf = vec![0x7fff_i16; n];
                mixer.mix(&mut buf);
                if mixer_rate == 0 {
                    assert!(buf.iter().all(|&s| s == 0), "rate 0 must be silent");
                }
                if n % 2 == 1 {
                    assert_eq!(buf.last(), Some(&0), "odd trailing slot must be zero");
                }
            }
        }
    }
    assert!(
        mixer.playing() <= plays,
        "more voices playing than were started"
    );

    // Tone and WAV: the sample count and the RIFF data chunk must match the input exactly.
    let rate = rng.below(48_001) as u32;
    let ms = rng.below(1_001) as u32;
    let amp = rng.u64() as i16;
    let t = tone(rate, rng.u64() as u32, ms, amp);
    let expected = if rate == 0 {
        0
    } else {
        (u64::from(rate) * u64::from(ms) / 1000) as usize
    };
    assert_eq!(t.samples.len(), expected, "tone length");
    let limit = i32::from(amp).abs();
    assert!(
        t.samples.iter().all(|&s| i32::from(s).abs() <= limit),
        "tone exceeds amplitude"
    );

    let stereo: Vec<i16> = (0..rng.below(8192)).map(|_| rng.u64() as i16).collect();
    let out = wav(rate, &stereo);
    let data_bytes = stereo.len() * 2;
    assert_eq!(out.len(), 44 + data_bytes, "wav length");
    assert_eq!(&out[0..4], b"RIFF");
    assert_eq!(&out[8..12], b"WAVE");
    assert_eq!(
        &out[40..44],
        (data_bytes as u32).to_le_bytes().as_slice(),
        "data size"
    );
    let decoded: Vec<i16> = out[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| i16::from_le_bytes(*p))
        .collect();
    assert_eq!(decoded, stereo, "wav data round trip");
}

#[test]
fn soak_script_load() {
    run_target("script_load", script_load_case);
}

#[test]
fn soak_game_run() {
    run_target("game_run", game_run_case);
}

#[test]
fn soak_scene_codec() {
    run_target("scene_codec", scene_codec_case);
}

#[test]
fn soak_audio() {
    run_target("audio", audio_case);
}

#[test]
fn parse_seed_forms() {
    assert_eq!(parse_u64("0x5EED_7A1"), Some(0x05EE_D7A1));
    assert_eq!(parse_u64("42"), Some(42));
    assert_eq!(parse_u64("nope"), None);
}
