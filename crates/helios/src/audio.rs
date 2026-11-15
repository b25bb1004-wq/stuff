//! Fixed-point stereo audio mixer, tone generator, and WAV encoder.
use alloc::vec::Vec;

use crate::fx::Fx;

/// Mono PCM sound buffer with sample rate in Hz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sound {
    /// Sample rate in Hz.
    pub rate: u32,
    /// Mono 16-bit signed PCM samples.
    pub samples: Vec<i16>,
}

/// Unique identifier for a playing voice, never reused and starting from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VoiceId(pub u64);

/// Generate a mono square-wave [`Sound`] at `rate` Hz, `freq_hz`, duration `ms`, and amplitude `amp`.
pub fn tone(rate: u32, freq_hz: u32, ms: u32, amp: i16) -> Sound {
    if rate == 0 {
        return Sound {
            rate,
            samples: Vec::new(),
        };
    }
    let n = (u64::from(rate) * u64::from(ms)) / 1000;
    let mut samples = Vec::with_capacity(n as usize);
    let neg_amp = amp.saturating_neg();
    let rate_u128 = u128::from(rate);
    let freq_u128 = u128::from(freq_hz);
    for i in 0..n {
        let half_period = (u128::from(i) * freq_u128 * 2) / rate_u128;
        samples.push(if half_period % 2 == 0 { amp } else { neg_amp });
    }
    Sound { rate, samples }
}

/// Encode interleaved stereo 16-bit PCM samples as a 44-byte-header RIFF WAV byte vector.
pub fn wav(rate: u32, stereo: &[i16]) -> Vec<u8> {
    let data_bytes = u32::try_from(stereo.len().saturating_mul(2)).unwrap_or(u32::MAX);
    let riff_size = 36u32.saturating_add(data_bytes);
    let byte_rate = rate.saturating_mul(4);
    let mut out = Vec::with_capacity(44usize.saturating_add(stereo.len().saturating_mul(2)));
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for &sample in stereo {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[derive(Debug, Clone)]
struct Voice {
    id: VoiceId,
    samples: Vec<i16>,
    pos: u64,
    step: u64,
    pan: i64,
    gl: i64,
    gr: i64,
    looped: bool,
    stopped: bool,
}

/// Fixed-point stereo audio mixer with resampling, panning, and looping voices.
#[derive(Debug, Clone)]
pub struct Mixer {
    rate: u32,
    next_id: u64,
    voices: Vec<Voice>,
}

impl Mixer {
    /// Create a new [`Mixer`] outputting at `rate` Hz.
    pub fn new(rate: u32) -> Mixer {
        Mixer {
            rate,
            next_id: 0,
            voices: Vec::new(),
        }
    }

    /// Start playing `s` with clamped `volume` (`0..=ONE`) and `pan` (`-ONE..=ONE`).
    pub fn play(&mut self, s: &Sound, volume: Fx, pan: Fx, looped: bool) -> VoiceId {
        let id = VoiceId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        if s.samples.is_empty() || s.rate == 0 {
            return id;
        }
        let pan_clamped = pan.0.clamp(-Fx::ONE.0, Fx::ONE.0);
        let (gl, gr) = compute_gains(volume, pan_clamped);
        let step = if self.rate == 0 {
            0
        } else {
            (u64::from(s.rate) << 32) / u64::from(self.rate)
        };
        self.voices.push(Voice {
            id,
            samples: s.samples.clone(),
            pos: 0,
            step,
            pan: pan_clamped,
            gl,
            gr,
            looped,
            stopped: false,
        });
        id
    }

    /// Stop voice `v`; returns `false` if it was not playing.
    pub fn stop(&mut self, v: VoiceId) -> bool {
        if let Some(idx) = self.voices.iter().position(|voice| voice.id == v) {
            self.voices.remove(idx);
            true
        } else {
            false
        }
    }

    /// Update the volume of playing voice `v`; returns `false` if it was not playing.
    pub fn set_volume(&mut self, v: VoiceId, volume: Fx) -> bool {
        if let Some(voice) = self.voices.iter_mut().find(|voice| voice.id == v) {
            let (gl, gr) = compute_gains(volume, voice.pan);
            voice.gl = gl;
            voice.gr = gr;
            true
        } else {
            false
        }
    }

    /// Number of currently playing voices.
    pub fn playing(&self) -> usize {
        self.voices.len()
    }

    /// Mix playing voices into interleaved `[L, R, ...]` samples; odd-length trailing slot is zeroed.
    pub fn mix(&mut self, out: &mut [i16]) {
        if self.rate == 0 {
            out.fill(0);
            return;
        }
        let (pairs, remainder) = out.as_chunks_mut::<2>();
        for [l, r] in pairs {
            let mut acc_l: i64 = 0;
            let mut acc_r: i64 = 0;
            for voice in &mut self.voices {
                let idx = (voice.pos >> 32) as usize;
                if let Some(&sample) = voice.samples.get(idx) {
                    let s = i64::from(sample);
                    acc_l += (s * voice.gl) >> 16;
                    acc_r += (s * voice.gr) >> 16;
                }
                let len_u128 = voice.samples.len() as u128;
                let mut next_pos = u128::from(voice.pos) + u128::from(voice.step);
                if (next_pos >> 32) >= len_u128 {
                    if voice.looped {
                        let len_shifted = len_u128 << 32;
                        next_pos %= len_shifted;
                    } else {
                        voice.stopped = true;
                        continue;
                    }
                }
                voice.pos = next_pos as u64;
            }
            self.voices.retain(|voice| !voice.stopped);
            *l = acc_l.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16;
            *r = acc_r.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16;
        }
        if let [last] = remainder {
            *last = 0;
        }
    }
}

fn compute_gains(volume: Fx, pan_clamped: i64) -> (i64, i64) {
    let vol = volume.0.clamp(0, Fx::ONE.0);
    let gl = (vol * (65_536 - pan_clamped)) >> 17;
    let gr = (vol * (65_536 + pan_clamped)) >> 17;
    (gl, gr)
}
