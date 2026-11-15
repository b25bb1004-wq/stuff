//! Host bundler that compiles `athena` (wasm32) and `titan` (riscv64) and packages the web app.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::icons::render_icon_png;

const STATIC_WEB_FILES: [&str; 5] = [
    "index.html",
    "athena.js",
    "workbench.js",
    "style.css",
    "manifest.webmanifest",
];

const BUNDLE_FILES: [&str; 11] = [
    "athena.js",
    "athena.wasm",
    "icon-192.png",
    "icon-512.png",
    "icon-maskable-512.png",
    "index.html",
    "manifest.webmanifest",
    "style.css",
    "sw.js",
    "titan.elf",
    "workbench.js",
];

/// Build `athena.wasm`, `titan.elf`, PWA icons, service worker, and copy all web assets into `out`.
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

    let mut copies: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(7);
    copies.push((wasm_src, out.join("athena.wasm")));
    copies.push((titan_src, out.join("titan.elf")));
    for name in STATIC_WEB_FILES {
        copies.push((web_dir.join(name), out.join(name)));
    }

    for (src, dst) in copies {
        std::fs::copy(&src, &dst)
            .map_err(|e| format!("failed to copy {} to {}: {e}", src.display(), dst.display()))?;
    }

    let icons: [(&str, u32, bool); 3] = [
        ("icon-192.png", 192, false),
        ("icon-512.png", 512, false),
        ("icon-maskable-512.png", 512, true),
    ];
    for (name, size, maskable) in icons {
        let bytes = render_icon_png(size, maskable);
        let dst = out.join(name);
        std::fs::write(&dst, bytes)
            .map_err(|e| format!("failed to write icon {}: {e}", dst.display()))?;
    }

    let sw_template_path = web_dir.join("sw.js");
    let sw_template = std::fs::read_to_string(&sw_template_path)
        .map_err(|e| format!("failed to read {}: {e}", sw_template_path.display()))?;

    let hash = compute_bundle_hash(out, &sw_template)?;
    let cache_name = format!("athena-{hash}");
    let assets_json = format_json_string_array(&BUNDLE_FILES);

    let sw_rendered = sw_template
        .replace("__ATHENA_CACHE_NAME__", &cache_name)
        .replace("\"__ATHENA_ASSETS__\"", &assets_json)
        .replace("__ATHENA_ASSETS__", &assets_json);

    let sw_dst = out.join("sw.js");
    std::fs::write(&sw_dst, sw_rendered)
        .map_err(|e| format!("failed to write {}: {e}", sw_dst.display()))?;

    Ok(())
}

fn format_json_string_array(items: &[&str]) -> String {
    let mut out = String::from("[");
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(item);
        out.push('"');
    }
    out.push(']');
    out
}

fn compute_bundle_hash(out: &Path, sw_template: &str) -> Result<String, String> {
    let mut h1: u64 = 0xcbf2_9ce4_8422_2325;
    let mut h2: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h1 ^= u64::from(b);
            h1 = h1.wrapping_mul(0x0100_0000_01b3);
            h2 = h2
                .wrapping_add(u64::from(b))
                .wrapping_mul(0xbf58_476d_1ce4_e5b9);
        }
    };

    for name in BUNDLE_FILES {
        if name == "sw.js" {
            feed(b"sw.js\0");
            feed(sw_template.as_bytes());
            continue;
        }
        let path = out.join(name);
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("failed to read {} for hashing: {e}", path.display()))?;
        feed(name.as_bytes());
        feed(&[0]);
        feed(&bytes);
    }

    Ok(format!("{h1:016x}{h2:016x}"))
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
