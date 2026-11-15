//! Host bundler that compiles `athena` (wasm32) and `titan` (riscv64) and packages the web app.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Build `athena.wasm`, `titan.elf`, and copy all five web assets into `out`.
pub fn pack(out: &Path) -> Result<(), String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| String::from("cannot locate workspace root"))?;

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));

    let wasm_target = root.join("target").join("athena-wasm");
    run_cargo(
        &cargo,
        root,
        &wasm_target,
        &[
            "build",
            "-p",
            "athena",
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

    std::fs::create_dir_all(out)
        .map_err(|e| format!("failed to create output dir {}: {e}", out.display()))?;

    let wasm_src = wasm_target
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("athena.wasm");
    let titan_src = titan_target
        .join("riscv64gc-unknown-none-elf")
        .join("release")
        .join("kernel");
    let web_dir = manifest_dir.join("web");

    let copies: [(&Path, PathBuf); 5] = [
        (&wasm_src, out.join("athena.wasm")),
        (&titan_src, out.join("titan.elf")),
        (&web_dir.join("index.html"), out.join("index.html")),
        (&web_dir.join("athena.js"), out.join("athena.js")),
        (&web_dir.join("style.css"), out.join("style.css")),
    ];

    for (src, dst) in copies {
        std::fs::copy(src, &dst)
            .map_err(|e| format!("failed to copy {} to {}: {e}", src.display(), dst.display()))?;
    }

    Ok(())
}

fn run_cargo(cargo: &str, root: &Path, target_dir: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new(cargo)
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target_dir)
        .args(args)
        .output()
        .map_err(|e| format!("failed to spawn cargo ({}): {e}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "cargo {} failed ({}):\n{stderr}",
            args.join(" "),
            output.status
        ));
    }
    Ok(())
}
