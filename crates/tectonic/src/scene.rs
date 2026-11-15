//! Deterministic scene stack, input recorder, binary codec, and replay runner.
use alloc::boxed::Box;
use alloc::vec::Vec;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn fnv1a_update(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(FNV_PRIME);
    }
}

/// Commands emitted by a [`Scene`] during [`Scene::update`] to mutate the [`Stage`] stack.
pub enum Command {
    /// Push a new scene onto the top of the stack.
    Push(Box<dyn Scene>),
    /// Pop the top scene from the stack.
    Pop,
    /// Replace the top scene (or push if the stack is empty).
    Replace(Box<dyn Scene>),
    /// Stop the stage and ignore any subsequent commands.
    Quit,
}

/// A deterministic game or application state that runs on a [`Stage`].
pub trait Scene {
    /// Advance the scene by one frame with `keys` and append stack mutations to `out`.
    fn update(&mut self, keys: u32, out: &mut Vec<Command>);
    /// Return a deterministic 64-bit hash of the scene's internal state.
    fn state_hash(&self) -> u64;
}

/// Deterministic stack of [`Scene`] instances with frame counter and state hashing.
pub struct Stage {
    stack: Vec<Box<dyn Scene>>,
    frame: u64,
    quit: bool,
}

impl Stage {
    /// Create a new [`Stage`] with `root` as the initial scene.
    pub fn new(root: Box<dyn Scene>) -> Stage {
        Stage {
            stack: alloc::vec![root],
            frame: 0,
            quit: false,
        }
    }

    /// Run one frame on the top scene and apply its commands; returns `false` once quit or empty.
    pub fn step(&mut self, keys: u32) -> bool {
        if self.quit {
            return false;
        }
        let Some(top) = self.stack.last_mut() else {
            return false;
        };
        let mut cmds = Vec::new();
        top.update(keys, &mut cmds);
        self.frame = self.frame.saturating_add(1);
        for cmd in cmds {
            match cmd {
                Command::Push(s) => self.stack.push(s),
                Command::Pop => {
                    self.stack.pop();
                }
                Command::Replace(s) => {
                    if let Some(top_slot) = self.stack.last_mut() {
                        *top_slot = s;
                    } else {
                        self.stack.push(s);
                    }
                }
                Command::Quit => {
                    self.quit = true;
                    break;
                }
            }
        }
        !self.quit && !self.stack.is_empty()
    }

    /// Number of scenes currently on the stack.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Number of completed steps that ran a scene.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// FNV-1a 64-bit hash over frame, stack depth, scene hashes bottom-to-top, and quit flag.
    pub fn hash(&self) -> u64 {
        let mut h = FNV_OFFSET;
        fnv1a_update(&mut h, &self.frame.to_le_bytes());
        fnv1a_update(&mut h, &(self.stack.len() as u64).to_le_bytes());
        for scene in &self.stack {
            fnv1a_update(&mut h, &scene.state_hash().to_le_bytes());
        }
        fnv1a_update(&mut h, &[u8::from(self.quit)]);
        h
    }
}

/// Recorded input state change at a specific frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Input {
    /// Frame number when the key state took effect.
    pub frame: u64,
    /// Bitmask of active keys.
    pub keys: u32,
}

/// Run-length-encoded input recorder storing only changes of key state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recorder {
    /// Recorded input transitions in non-decreasing frame order.
    pub inputs: Vec<Input>,
}

impl Recorder {
    /// Create an empty [`Recorder`].
    pub fn new() -> Recorder {
        Recorder { inputs: Vec::new() }
    }

    /// Record `keys` at `frame`, ignoring earlier frames, overwriting equal frames, and storing changes only.
    pub fn record(&mut self, frame: u64, keys: u32) {
        match self.inputs.last_mut() {
            None => {
                if keys != 0 {
                    self.inputs.push(Input { frame, keys });
                }
            }
            Some(last) => {
                if frame < last.frame {
                    return;
                }
                if frame == last.frame {
                    last.keys = keys;
                } else if keys != last.keys {
                    self.inputs.push(Input { frame, keys });
                }
            }
        }
    }

    /// Return the active keys at `frame` (the last input with `frame <= frame`, or `0`).
    pub fn keys_at(&self, frame: u64) -> u32 {
        let idx = self.inputs.partition_point(|inp| inp.frame <= frame);
        if idx == 0 {
            0
        } else {
            self.inputs.get(idx - 1).map_or(0, |inp| inp.keys)
        }
    }

    /// Encode the recorded inputs into the `TREC` binary format.
    pub fn encode(&self) -> Vec<u8> {
        let count = u32::try_from(self.inputs.len()).unwrap_or(u32::MAX);
        let mut out =
            Vec::with_capacity(8usize.saturating_add(self.inputs.len().saturating_mul(12)));
        out.extend_from_slice(b"TREC");
        out.extend_from_slice(&count.to_le_bytes());
        for inp in &self.inputs {
            out.extend_from_slice(&inp.frame.to_le_bytes());
            out.extend_from_slice(&inp.keys.to_le_bytes());
        }
        out
    }

    /// Decode a `TREC` byte slice into a [`Recorder`], returning `None` on malformed input.
    pub fn decode(bytes: &[u8]) -> Option<Recorder> {
        if bytes.get(..4)? != b"TREC" {
            return None;
        }
        let count_bytes: [u8; 4] = bytes.get(4..8)?.try_into().ok()?;
        let count = u32::from_le_bytes(count_bytes) as usize;
        let expected_len = 8usize.checked_add(count.checked_mul(12)?)?;
        if bytes.len() != expected_len {
            return None;
        }
        let mut inputs: Vec<Input> = Vec::with_capacity(count);
        let (chunks, _) = bytes.get(8..)?.as_chunks::<12>();
        for chunk in chunks {
            let frame_bytes: [u8; 8] = chunk.get(..8)?.try_into().ok()?;
            let keys_bytes: [u8; 4] = chunk.get(8..12)?.try_into().ok()?;
            let frame = u64::from_le_bytes(frame_bytes);
            let keys = u32::from_le_bytes(keys_bytes);
            if let Some(prev) = inputs.last() {
                if frame < prev.frame {
                    return None;
                }
            }
            inputs.push(Input { frame, keys });
        }
        Some(Recorder { inputs })
    }
}

/// Replay up to `frames` steps on `stage` using `rec` and return the resulting `stage.hash()`.
pub fn replay(stage: &mut Stage, rec: &Recorder, frames: u64) -> u64 {
    for _ in 0..frames {
        let keys = rec.keys_at(stage.frame());
        if !stage.step(keys) {
            break;
        }
    }
    stage.hash()
}
