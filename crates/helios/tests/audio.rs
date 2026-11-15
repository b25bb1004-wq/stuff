//! Integration tests for `helios::audio`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use helios::audio::{tone, wav, Mixer, Sound, VoiceId};
use helios::fx::Fx;

#[test]
fn tone_shapes_and_edge_cases() {
    // 8 samples at 8000 Hz, 1000 Hz => half-period is 4 samples: 4x +amp, 4x -amp
    let s = tone(8000, 1000, 1, 1000);
    assert_eq!(s.rate, 8000);
    assert_eq!(
        s.samples,
        vec![1000, 1000, 1000, 1000, -1000, -1000, -1000, -1000]
    );

    // 2000 Hz at 8000 Hz for 1 ms => 2x +amp, 2x -amp, 2x +amp, 2x -amp
    let s2 = tone(8000, 2000, 1, 500);
    assert_eq!(s2.samples, vec![500, 500, -500, -500, 500, 500, -500, -500]);

    // freq_hz == 0 gives all +amp
    let dc = tone(4000, 0, 2, 321);
    assert_eq!(dc.samples, vec![321; 8]);

    // rate == 0 gives empty samples
    let zero_rate = tone(0, 440, 10, 1000);
    assert_eq!(zero_rate.rate, 0);
    assert!(zero_rate.samples.is_empty());

    // i16::MIN amplitude does not panic
    let min_amp = tone(4000, 1000, 1, i16::MIN);
    assert_eq!(
        min_amp.samples,
        vec![i16::MIN, i16::MIN, i16::MAX, i16::MAX]
    );
}

#[test]
fn one_voice_center_left_right() {
    let s = Sound {
        rate: 44100,
        samples: vec![1000, -2000],
    };

    // Center: gl = gr = 0.5 -> [500, 500, -1000, -1000]
    let mut m = Mixer::new(44100);
    m.play(&s, Fx::ONE, Fx::ZERO, false);
    let mut out = [0i16; 4];
    m.mix(&mut out);
    assert_eq!(out, [500, 500, -1000, -1000]);
    assert_eq!(m.playing(), 0);

    // Full left: pan = -ONE -> gl = 1.0, gr = 0.0
    let mut m = Mixer::new(44100);
    m.play(&s, Fx::ONE, -Fx::ONE, false);
    let mut out = [0i16; 4];
    m.mix(&mut out);
    assert_eq!(out, [1000, 0, -2000, 0]);

    // Full right: pan = ONE -> gl = 0.0, gr = 1.0
    let mut m = Mixer::new(44100);
    m.play(&s, Fx::ONE, Fx::ONE, false);
    let mut out = [0i16; 4];
    m.mix(&mut out);
    assert_eq!(out, [0, 1000, 0, -2000]);
}

#[test]
fn two_voices_summing_and_saturating() {
    let pos = Sound {
        rate: 44100,
        samples: vec![30000, -30000],
    };
    let mut m = Mixer::new(44100);
    // Pan left for both at full volume -> each contributes 30000 to L, 0 to R
    m.play(&pos, Fx::ONE, -Fx::ONE, false);
    m.play(&pos, Fx::ONE, -Fx::ONE, false);
    let mut out = [0i16; 4];
    m.mix(&mut out);
    assert_eq!(out, [i16::MAX, 0, i16::MIN, 0]);
}

#[test]
fn looped_and_one_shot_ends() {
    let s = Sound {
        rate: 44100,
        samples: vec![100, 200, 300],
    };
    let mut m = Mixer::new(44100);
    let v_loop = m.play(&s, Fx::ONE, -Fx::ONE, true);
    let v_once = m.play(&s, Fx::ONE, Fx::ONE, false);
    assert_eq!(v_loop, VoiceId(0));
    assert_eq!(v_once, VoiceId(1));
    assert_eq!(m.playing(), 2);

    let mut out = [0i16; 12]; // 6 frames
    m.mix(&mut out);
    assert_eq!(m.playing(), 1);
    assert_eq!(out, [100, 100, 200, 200, 300, 300, 100, 0, 200, 0, 300, 0]);
    assert!(!m.stop(v_once));
    assert!(m.stop(v_loop));
    assert_eq!(m.playing(), 0);
}

#[test]
fn resampling_22050_into_44100_and_48000() {
    let s = Sound {
        rate: 22050,
        samples: vec![1000, -1000],
    };

    // 22050 -> 44100: step = 0.5, so each sample repeats twice
    let mut m44 = Mixer::new(44100);
    m44.play(&s, Fx::ONE, -Fx::ONE, false);
    let mut out44 = [0i16; 10]; // 5 frames
    m44.mix(&mut out44);
    assert_eq!(out44, [1000, 0, 1000, 0, -1000, 0, -1000, 0, 0, 0]);
    assert_eq!(m44.playing(), 0);

    // 22050 -> 48000: step = (22050 << 32) / 48000 = 1_972_991_692
    let mut m48 = Mixer::new(48000);
    m48.play(&s, Fx::ONE, -Fx::ONE, false);
    let step = (22050u64 << 32) / 48000u64;
    let mut pos = 0u64;
    let mut expected = [0i16; 12];
    for frame in 0..6 {
        let idx = (pos >> 32) as usize;
        if idx < s.samples.len() {
            expected[frame * 2] = s.samples[idx];
            pos += step;
        }
    }
    let mut out48 = [0i16; 12];
    m48.mix(&mut out48);
    assert_eq!(out48, expected);
}

#[test]
fn stop_set_volume_and_empty_or_zero_rate() {
    let mut m = Mixer::new(44100);
    let empty = Sound {
        rate: 44100,
        samples: vec![],
    };
    let zero_rate = Sound {
        rate: 0,
        samples: vec![100, 200],
    };
    let id0 = m.play(&empty, Fx::ONE, Fx::ZERO, true);
    let id1 = m.play(&zero_rate, Fx::ONE, Fx::ZERO, true);
    assert_eq!(id0, VoiceId(0));
    assert_eq!(id1, VoiceId(1));
    assert_eq!(m.playing(), 0);
    assert!(!m.stop(id0));
    assert!(!m.set_volume(id1, Fx::HALF));

    let valid = Sound {
        rate: 44100,
        samples: vec![1000; 4],
    };
    let id2 = m.play(&valid, Fx::ONE, -Fx::ONE, false);
    assert_eq!(id2, VoiceId(2));
    assert_eq!(m.playing(), 1);
    assert!(m.set_volume(id2, Fx::HALF));

    // Odd-length buffer: last slot must be 0
    let mut out = [999i16; 3];
    m.mix(&mut out);
    assert_eq!(out, [500, 0, 0]);
    assert!(m.stop(id2));
    assert!(!m.stop(id2));

    // Zero-rate mixer writes zeros
    let mut mz = Mixer::new(0);
    mz.play(&valid, Fx::ONE, Fx::ZERO, false);
    let mut zout = [123i16; 4];
    mz.mix(&mut zout);
    assert_eq!(zout, [0, 0, 0, 0]);
}

#[test]
fn wav_header_byte_for_byte() {
    let stereo = [0x1234i16, -0x0001i16, 0x0000, 0x7FFF];
    let bytes = wav(44100, &stereo);
    assert_eq!(bytes.len(), 44 + 8);
    let expected_header: [u8; 44] = [
        b'R', b'I', b'F', b'F', // ChunkID
        44, 0, 0, 0, // ChunkSize = 36 + 8 = 44
        b'W', b'A', b'V', b'E', // Format
        b'f', b'm', b't', b' ', // Subchunk1ID
        16, 0, 0, 0, // Subchunk1Size = 16
        1, 0, // AudioFormat = 1 (PCM)
        2, 0, // NumChannels = 2
        0x44, 0xAC, 0x00, 0x00, // SampleRate = 44100 (0xAC44)
        0x10, 0xB1, 0x02, 0x00, // ByteRate = 44100 * 4 = 176400 (0x0002B110)
        4, 0, // BlockAlign = 4
        16, 0, // BitsPerSample = 16
        b'd', b'a', b't', b'a', // Subchunk2ID
        8, 0, 0, 0, // Subchunk2Size = 8
    ];
    assert_eq!(&bytes[..44], &expected_header);
    assert_eq!(
        &bytes[44..],
        &[0x34, 0x12, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0x7F]
    );
}

#[test]
fn mixer_never_panics_on_random_inputs() {
    forall(200, 32, |rng| {
        let mix_rate = if rng.range(0, 10) == 0 {
            0
        } else {
            rng.range(8000, 96000) as u32
        };
        let mut mixer = Mixer::new(mix_rate);
        let len = rng.range(0, 32) as usize;
        let samples: Vec<i16> = (0..len).map(|_| rng.range(0, 65536) as i16).collect();
        let s = Sound {
            rate: rng.range(0, 96000) as u32,
            samples,
        };
        let vol = Fx(rng.range(0, 300_000) as i64 - 100_000);
        let pan = Fx(rng.range(0, 400_000) as i64 - 200_000);
        let id = mixer.play(&s, vol, pan, rng.range(0, 2) == 1);
        let out_len = rng.range(0, 65) as usize;
        let mut out = vec![0i16; out_len];
        mixer.mix(&mut out);
        if out_len % 2 == 1 {
            assert_eq!(out[out_len - 1], 0);
        }
        let _ = mixer.set_volume(id, Fx(rng.range(0, 200_000) as i64 - 50_000));
        mixer.mix(&mut out);
        let _ = mixer.stop(id);
    });
}
