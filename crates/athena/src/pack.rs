//! Host bundler that compiles `athena` (wasm32) and `titan` (riscv64) and packages the web app.

use std::path::{Path, PathBuf};

use neon::webkit::{content_hash, run_cargo, sw_js, FILES as KIT_FILES};

use crate::icons::render_icon_png;

const STATIC_WEB_FILES: [&str; 8] = [
    "athena-motion.js",
    "debug.js",
    "index.html",
    "athena.js",
    "selftest.html",
    "workbench.js",
    "style.css",
    "manifest.webmanifest",
];

/// Files the bundle ships besides the kit and the generated `sw.js`.
const BUNDLE_FILES: [&str; 13] = [
    "athena-motion.js",
    "debug.js",
    "athena.js",
    "athena.wasm",
    "icon-192.png",
    "icon-512.png",
    "icon-maskable-512.png",
    "index.html",
    "manifest.webmanifest",
    "selftest.html",
    "style.css",
    "titan.elf",
    "workbench.js",
];

/// Build `athena.wasm`, `titan.elf`, PWA icons, the kit under `_kit/`, and the service worker.
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

    let mut copies: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(8);
    copies.push((wasm_src, out.join("athena.wasm")));
    copies.push((titan_src, out.join("titan.elf")));
    for name in STATIC_WEB_FILES {
        copies.push((web_dir.join(name), out.join(name)));
    }

    for (src, dst) in copies {
        std::fs::copy(&src, &dst)
            .map_err(|e| format!("failed to copy {} to {}: {e}", src.display(), dst.display()))?;
    }

    let kit_out = out.join("_kit");
    std::fs::create_dir_all(&kit_out)
        .map_err(|e| format!("failed to create {}: {e}", kit_out.display()))?;
    for (name, text) in KIT_FILES {
        let dst = kit_out.join(name);
        std::fs::write(&dst, text)
            .map_err(|e| format!("failed to write kit file {}: {e}", dst.display()))?;
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

    // The cache name is a hash of every shipped file, so a changed build gets a new cache.
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for name in BUNDLE_FILES {
        let path = out.join(name);
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("failed to read {} for hashing: {e}", path.display()))?;
        parts.push(name.as_bytes().to_vec());
        parts.push(bytes);
    }
    for (name, text) in KIT_FILES {
        parts.push(name.as_bytes().to_vec());
        parts.push(text.as_bytes().to_vec());
    }
    let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
    let hash = content_hash(&refs);

    // Precache: the page itself (as "./"), every shipped file, and the kit at its absolute path.
    let mut precache: Vec<String> = vec![String::from("./")];
    precache.extend(BUNDLE_FILES.iter().map(|f| String::from(*f)));
    precache.extend(KIT_FILES.iter().map(|(name, _)| format!("/_kit/{name}")));
    let precache_refs: Vec<&str> = precache.iter().map(String::as_str).collect();

    let sw_dst = out.join("sw.js");
    std::fs::write(&sw_dst, sw_js("athena", &hash, &precache_refs))
        .map_err(|e| format!("failed to write {}: {e}", sw_dst.display()))?;

    Ok(())
}
