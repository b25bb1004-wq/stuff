//! Scene tests: lithium and the spy run under zeus, stepped in slices and inspected.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use silicon::machine::RAM_BASE;
use zeus::scene::{Exit, Scene, SPY_SRC};
use zeus::{Guest, Image};

const MIB: u64 = 1024 * 1024;
const STEP_LIMIT: u64 = 80_000_000;

fn zeus_elf() -> &'static [u8] {
    static ELF: OnceLock<Vec<u8>> = OnceLock::new();
    ELF.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let target_dir = root.join("target").join("zeus-bin");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let status = Command::new(cargo)
            .current_dir(root)
            .env("CARGO_TARGET_DIR", &target_dir)
            .args([
                "build",
                "-p",
                "zeus",
                "--bin",
                "zeus",
                "--release",
                "--target",
                "riscv64gc-unknown-none-elf",
            ])
            .status()
            .unwrap();
        assert!(status.success(), "cargo build for zeus binary failed");
        let path = target_dir
            .join("riscv64gc-unknown-none-elf")
            .join("release")
            .join("zeus");
        std::fs::read(path).unwrap()
    })
}

fn lithium_guest() -> Guest {
    Guest {
        name: "lithium".into(),
        image: Image::Raw {
            bytes: lithium::image().unwrap(),
            gpa: RAM_BASE,
        },
        size: 4 * MIB,
        a1: 0,
    }
}

fn spy_guest() -> Guest {
    Guest {
        name: "spy".into(),
        image: Image::Raw {
            bytes: chisel::assemble(SPY_SRC, RAM_BASE).unwrap().bytes,
            gpa: RAM_BASE,
        },
        size: 2 * MIB,
        a1: 0,
    }
}

#[test]
fn spy_beside_lithium_leaves_lithium_unchanged_and_faults_are_recorded() {
    let mut solo = Scene::new(zeus_elf(), &[lithium_guest()]).unwrap();
    assert_eq!(solo.step(STEP_LIMIT), Exit::Pass);
    let solo_lines = solo.console("lithium");
    assert_eq!(
        solo_lines.first().map(String::as_str),
        Some("lithium: boot")
    );

    let mut both = Scene::new(zeus_elf(), &[spy_guest(), lithium_guest()]).unwrap();
    assert!(both.last_fault().is_none());
    assert_eq!(both.step(STEP_LIMIT), Exit::Pass);
    assert_eq!(both.console("spy"), vec!["spy: faults 7".to_owned()]);
    // The spy shifts lithium's task interleaving, so compare the transcript as a multiset.
    let (mut a, mut b) = (solo_lines.clone(), both.console("lithium"));
    a.sort();
    b.sort();
    assert_eq!(a, b);

    // The spy's probes of unmapped GPAs are guest page faults (load 21 / store 23) seen by the hart.
    let fault = both.last_fault().unwrap();
    assert!(
        fault.cause == 21 || fault.cause == 23,
        "cause {}",
        fault.cause
    );
    assert!(fault.count > 0);
}

#[test]
fn step_limit_pauses_and_resumes() {
    let mut scene = Scene::new(zeus_elf(), &[lithium_guest()]).unwrap();
    assert_eq!(scene.step(1), Exit::OutOfSteps);
    assert!(scene.console("lithium").is_empty());
    assert_eq!(scene.step(STEP_LIMIT), Exit::Pass);
    assert_eq!(
        scene.console("lithium").last().map(String::as_str),
        Some("lithium: halt")
    );
}

#[test]
fn rejects_non_elf_input() {
    assert!(Scene::new(&[0u8; 8], &[lithium_guest()]).is_err());
}
