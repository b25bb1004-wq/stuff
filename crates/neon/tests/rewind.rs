//! Rewind tests for `neon::session::Titan`: seek then replay, input-log truncation, and the cap.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use assay::{forall, Rng};
use neon::session::{Pump, Titan};

/// Checkpoint spacing used here. A checkpoint rescans all of RAM, so this is kept coarse.
const INTERVAL: u64 = 1_000_000;
const CAP: usize = 48 * 1024 * 1024;
/// Host-side sampling spacing for marks, in instructions.
const STRIDE: u64 = 200_000;
/// Instructions per `pump` call while driving. Marks and sends stay exact at each return.
const CHUNK: u64 = 4_096;
const STEP_LIMIT: u64 = 20_000_000;

fn kernel_elf() -> &'static [u8] {
    static ELF: OnceLock<Vec<u8>> = OnceLock::new();
    ELF.get_or_init(|| {
        let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let target_dir = root.join("target").join("titan-kernel");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());

        let status = Command::new(cargo)
            .current_dir(&root)
            .env("CARGO_TARGET_DIR", &target_dir)
            .args([
                "build",
                "-p",
                "titan",
                "--bin",
                "kernel",
                "--release",
                "--target",
                "riscv64gc-unknown-none-elf",
            ])
            .status()
            .unwrap();
        assert!(status.success(), "cargo build for titan kernel failed");

        std::fs::read(
            target_dir
                .join("riscv64gc-unknown-none-elf")
                .join("release")
                .join("kernel"),
        )
        .unwrap()
    })
}

const COMMANDS: [&str; 9] = [
    "echo hi there",
    "ls /",
    "cat /hello.txt",
    "mkdir /tmp",
    "echo saved > /tmp/out.txt",
    "cat < /tmp/out.txt",
    "cat /nope || echo failed $?",
    "nosuch",
    "X=7; echo x=$X",
];

fn random_script(rng: &mut Rng) -> Vec<String> {
    let n = 2 + rng.below(4);
    (0..n)
        .map(|_| {
            if rng.chance(1, 3) {
                format!("echo w{}", rng.below(1000))
            } else {
                (*rng.pick(&COMMANDS)).to_owned()
            }
        })
        .collect()
}

/// A point in a run: the instret, and how much transcript existed there.
#[derive(Clone, Copy)]
struct Mark {
    instret: u64,
    offset: usize,
}

/// What one host-driven run produced.
struct Run {
    transcript: Vec<u8>,
    instret: u64,
    marks: Vec<Mark>,
    /// Instret of each input the driver sent.
    sends: Vec<u64>,
}

/// Drive the shell like a host: single-step, and send the next scripted line whenever the guest
/// waits for input. A seeked session replays the logged inputs, so it stops at the same point.
fn drive(titan: &mut Titan, script: &[String]) -> Run {
    let mut run = Run {
        transcript: Vec::new(),
        instret: 0,
        marks: Vec::new(),
        sends: Vec::new(),
    };
    let mut next_mark = titan.instret();
    for _ in 0..STEP_LIMIT {
        let p = titan.pump(CHUNK);
        run.transcript.extend(titan.take_output());
        if titan.instret() >= next_mark {
            run.marks.push(Mark {
                instret: titan.instret(),
                offset: run.transcript.len(),
            });
            next_mark = titan.instret() + STRIDE;
        }
        match p {
            Pump::WaitingForInput => {
                let sent = titan.input_log_len();
                if sent >= script.len() {
                    run.instret = titan.instret();
                    return run;
                }
                run.sends.push(titan.instret());
                titan.send(&script[sent]);
            }
            Pump::Busy => {}
            Pump::Exited(code) => panic!("unexpected exit {code}"),
        }
    }
    panic!("guest did not settle within {STEP_LIMIT} steps");
}

fn boot() -> Titan {
    let mut titan = Titan::boot(kernel_elf()).unwrap();
    titan.set_rewind_limits(INTERVAL, CAP);
    titan
}

#[test]
fn seek_then_replay_matches_the_straight_run() {
    forall(3, 0x5EE_D0C, |rng| {
        let script = random_script(rng);
        let mut titan = boot();
        let straight = drive(&mut titan, &script);

        let mark = *rng.pick(&straight.marks);
        titan.seek(mark.instret).unwrap();
        assert_eq!(titan.instret(), mark.instret);

        let replay = drive(&mut titan, &script);
        assert_eq!(
            replay.transcript.as_slice(),
            &straight.transcript[mark.offset..],
            "transcript after seek to instret {}",
            mark.instret
        );
        assert_eq!(replay.instret, straight.instret);
    });
}

#[test]
fn typing_after_a_seek_cuts_the_old_future() {
    forall(2, 0xC07_F00D, |rng| {
        let script = random_script(rng);
        let mut titan = boot();
        let straight = drive(&mut titan, &script);

        // Seek to a point before the last input, so part of the log is still future.
        let last_send = *straight.sends.last().unwrap();
        let early: Vec<Mark> = straight
            .marks
            .iter()
            .copied()
            .filter(|m| m.instret < last_send)
            .collect();
        let mark = *rng.pick(&early);
        titan.seek(mark.instret).unwrap();
        assert_eq!(titan.input_log_len(), script.len(), "seek keeps the future");

        let kept = straight
            .sends
            .iter()
            .filter(|&&s| s <= mark.instret)
            .count();
        titan.send("echo new");
        assert_eq!(
            titan.input_log_len(),
            kept + 1,
            "typing drops the old future"
        );

        // The new future runs to a prompt, and the session still works.
        let mut prompted = false;
        for _ in 0..STEP_LIMIT {
            match titan.pump(CHUNK) {
                Pump::WaitingForInput => {
                    prompted = true;
                    break;
                }
                Pump::Busy => {}
                Pump::Exited(code) => panic!("unexpected exit {code}"),
            }
        }
        assert!(prompted, "no prompt after typing post-seek");
        // Idle at the prompt with nothing logged: an instret beyond the log cannot be reached.
        assert!(titan.seek(u64::MAX).is_err());
    });
}

#[test]
fn checkpoint_bytes_stay_under_the_cap() {
    let mut titan = Titan::boot(kernel_elf()).unwrap();
    let base = titan.checkpoint_bytes();
    let cap = base + 64 * 1024;
    let interval = INTERVAL / 2;
    titan.set_rewind_limits(interval, cap);

    let script: Vec<String> = [
        "echo hi there",
        "ls /",
        "cat /hello.txt",
        "echo saved > /tmp/out.txt",
        "cat /tmp/out.txt",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let run = drive(&mut titan, &script);

    assert!(
        titan.checkpoint_bytes() <= cap,
        "checkpoints exceed the cap"
    );
    // Every interval boundary crossed takes a checkpoint; the cap must have dropped some.
    let taken = usize::try_from(run.instret / interval).unwrap() + 1;
    assert!(
        titan.checkpoint_count() < taken,
        "nothing was thinned: {} kept of {taken}",
        titan.checkpoint_count()
    );

    // Thinned-away points still seek, because the boot base is never dropped.
    for target in [run.instret / 3, run.instret / 2, 0] {
        titan.seek(target).unwrap();
        assert_eq!(titan.instret(), target);
    }
}
