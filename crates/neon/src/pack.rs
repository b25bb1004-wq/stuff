//! Host bundler that compiles `neon` (wasm32) and `titan` (riscv64) and writes the wasm, the ELF and the kit.

use std::path::Path;

use crate::webkit::{run_cargo, FILES};

/// Build `neon.wasm` and `titan.elf`, then write them and the kit (`_kit/*`) into `out`.
pub fn pack(out: &Path) -> Result<(), String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| String::from("cannot locate workspace root"))?;

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));

    let wasm_target = root.join("target").join("neon-wasm");
    run_cargo(
        &cargo,
        root,
        &wasm_target,
        &[
            "build",
            "-p",
            "neon",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
        ],
    )?;

    let titan_target = root.join("target").join("titan-kernel");
    run_cargo(
        &cargo,
        root,
        &titan_target,
        &[
            "build",
            "-p",
            "titan",
            "--bin",
            "kernel",
            "--release",
            "--target",
            "riscv64gc-unknown-none-elf",
        ],
    )?;

    let kit_out = out.join("_kit");
    std::fs::create_dir_all(&kit_out)
        .map_err(|e| format!("failed to create output dir {}: {e}", kit_out.display()))?;

    let wasm_src = wasm_target
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("neon.wasm");
    let titan_src = titan_target
        .join("riscv64gc-unknown-none-elf")
        .join("release")
        .join("kernel");

    let copies = [
        (wasm_src.as_path(), out.join("neon.wasm")),
        (titan_src.as_path(), out.join("titan.elf")),
    ];
    for (src, dst) in copies {
        std::fs::copy(src, &dst)
            .map_err(|e| format!("failed to copy {} to {}: {e}", src.display(), dst.display()))?;
    }

    for (name, text) in FILES {
        let dst = kit_out.join(name);
        std::fs::write(&dst, text)
            .map_err(|e| format!("failed to write {}: {e}", dst.display()))?;
    }

    Ok(())
}
