//! Host bundler that compiles `site` (wasm32), `titan` (riscv64), and packs `athena` and all apps.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::icons::render_icon_png;
use neon::webkit;

struct AppSpec {
    id: &'static str,
    name: &'static str,
    initial: char,
}

const APPS: [AppSpec; 23] = [
    AppSpec {
        id: "titan",
        name: "Titan",
        initial: 'T',
    },
    AppSpec {
        id: "iridium",
        name: "Iridium",
        initial: 'I',
    },
    AppSpec {
        id: "elysium",
        name: "Elysium",
        initial: 'E',
    },
    AppSpec {
        id: "mercury",
        name: "Mercury",
        initial: 'M',
    },
    AppSpec {
        id: "helios",
        name: "Helios",
        initial: 'H',
    },
    AppSpec {
        id: "cobalt",
        name: "Cobalt",
        initial: 'C',
    },
    AppSpec {
        id: "logos",
        name: "Logos",
        initial: 'L',
    },
    AppSpec {
        id: "odyssey",
        name: "Odyssey",
        initial: 'O',
    },
    AppSpec {
        id: "aurum",
        name: "Aurum",
        initial: 'A',
    },
    AppSpec {
        id: "sphinx",
        name: "Sphinx",
        initial: 'S',
    },
    AppSpec {
        id: "silicon",
        name: "Silicon",
        initial: 'S',
    },
    AppSpec {
        id: "chisel",
        name: "Chisel",
        initial: 'C',
    },
    AppSpec {
        id: "hades",
        name: "Hades",
        initial: 'H',
    },
    AppSpec {
        id: "pandora",
        name: "Pandora",
        initial: 'P',
    },
    AppSpec {
        id: "argo",
        name: "Argo",
        initial: 'A',
    },
    AppSpec {
        id: "krypton",
        name: "Krypton",
        initial: 'K',
    },
    AppSpec {
        id: "iris",
        name: "Iris",
        initial: 'I',
    },
    AppSpec {
        id: "indium",
        name: "Indium",
        initial: 'I',
    },
    AppSpec {
        id: "calliope",
        name: "Calliope",
        initial: 'C',
    },
    AppSpec {
        id: "chronos",
        name: "Chronos",
        initial: 'C',
    },
    AppSpec {
        id: "xenon",
        name: "Xenon",
        initial: 'X',
    },
    AppSpec {
        id: "lithium",
        name: "Lithium",
        initial: 'L',
    },
    AppSpec {
        id: "kairos",
        name: "Kairos",
        initial: 'K',
    },
];

const LAUNCHER_EXTRA_ICONS: [(&str, char); 3] = [("athena", 'A'), ("midas", 'M'), ("zeus", 'Z')];

/// Build `site.wasm`, `titan.elf`, copy web assets, and pack `athena`, `_kit`, `/apps/`, and all standalone apps.
pub fn pack(out: &Path) -> Result<(), String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| String::from("cannot locate workspace root"))?;

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));

    let wasm_target = root.join("target").join("site-wasm");
    run_cargo(
        &cargo,
        root,
        &wasm_target,
        &[
            "build",
            "-p",
            "site",
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
        .join("site.wasm");
    let titan_src = titan_target
        .join("riscv64gc-unknown-none-elf")
        .join("release")
        .join("kernel");
    let web_dir = manifest_dir.join("web");

    let copies: [(&Path, PathBuf); 6] = [
        (&wasm_src, out.join("site.wasm")),
        (&titan_src, out.join("titan.elf")),
        (&web_dir.join("index.html"), out.join("index.html")),
        (&web_dir.join("site.js"), out.join("site.js")),
        (&web_dir.join("style.css"), out.join("style.css")),
        (&web_dir.join("systems.js"), out.join("systems.js")),
    ];

    for (src, dst) in copies {
        copy_checked(src, &dst)?;
    }

    athena::pack::pack(&out.join("athena"))?;
    pack_kit_and_launcher(&manifest_dir.join("apps"), out)?;
    pack_apps(&manifest_dir.join("apps"), out)?;
    Ok(())
}

fn pack_kit_and_launcher(apps_src: &Path, out: &Path) -> Result<(), String> {
    let kit_out = out.join("_kit");
    std::fs::create_dir_all(&kit_out)
        .map_err(|e| format!("failed to create {}: {e}", kit_out.display()))?;
    for name in ["kit.css", "kit.js", "demo-app.js"] {
        copy_checked(&apps_src.join("_kit").join(name), &kit_out.join(name))?;
    }
    for (name, text) in webkit::FILES {
        write_checked(&kit_out.join(name), text.as_bytes())?;
    }
    // The kit compares this with the wasm's `site_abi()` (see `kit.js`).
    let abi = format!("{{\"abi\": {}}}\n", crate::SITE_ABI);
    write_checked(&kit_out.join("abi.json"), abi.as_bytes())?;

    let launcher_out = out.join("apps");
    let icons_out = launcher_out.join("icons");
    std::fs::create_dir_all(&icons_out)
        .map_err(|e| format!("failed to create {}: {e}", icons_out.display()))?;
    copy_checked(
        &apps_src.join("index.html"),
        &launcher_out.join("index.html"),
    )?;

    for (id, initial) in LAUNCHER_EXTRA_ICONS {
        let png = render_icon_png(initial, 192, false);
        write_checked(&icons_out.join(format!("{id}.png")), &png)?;
    }
    Ok(())
}

fn pack_apps(apps_src: &Path, out: &Path) -> Result<(), String> {
    for app in &APPS {
        let app_src = apps_src.join(app.id);
        let app_out = out.join(app.id);
        std::fs::create_dir_all(&app_out)
            .map_err(|e| format!("failed to create {}: {e}", app_out.display()))?;

        copy_checked(&app_src.join("index.html"), &app_out.join("index.html"))?;
        copy_checked(&app_src.join("app.js"), &app_out.join("app.js"))?;
        for name in OPTIONAL_APP_ASSETS {
            if app_src.join(name).is_file() {
                copy_checked(&app_src.join(name), &app_out.join(name))?;
            }
        }

        write_checked(
            &app_out.join("icon-192.png"),
            &render_icon_png(app.initial, 192, false),
        )?;
        write_checked(
            &app_out.join("icon-512.png"),
            &render_icon_png(app.initial, 512, false),
        )?;
        write_checked(
            &app_out.join("icon-maskable-512.png"),
            &render_icon_png(app.initial, 512, true),
        )?;

        let manifest = build_manifest(app.name);
        write_checked(&app_out.join("manifest.webmanifest"), manifest.as_bytes())?;

        let sw = build_service_worker(app, &app_out)?;
        write_checked(&app_out.join("sw.js"), sw.as_bytes())?;
    }
    Ok(())
}

fn build_manifest(name: &str) -> String {
    format!(
        "{{\n\
  \"name\": \"Monolith {name}\",\n\
  \"short_name\": \"{name}\",\n\
  \"start_url\": \"./\",\n\
  \"scope\": \"./\",\n\
  \"display\": \"standalone\",\n\
  \"display_override\": [\"window-controls-overlay\", \"standalone\"],\n\
  \"background_color\": \"#0e1013\",\n\
  \"theme_color\": \"#0e1013\",\n\
  \"icons\": [\n\
    {{\n\
      \"src\": \"icon-192.png\",\n\
      \"sizes\": \"192x192\",\n\
      \"type\": \"image/png\",\n\
      \"purpose\": \"any\"\n\
    }},\n\
    {{\n\
      \"src\": \"icon-512.png\",\n\
      \"sizes\": \"512x512\",\n\
      \"type\": \"image/png\",\n\
      \"purpose\": \"any\"\n\
    }},\n\
    {{\n\
      \"src\": \"icon-maskable-512.png\",\n\
      \"sizes\": \"512x512\",\n\
      \"type\": \"image/png\",\n\
      \"purpose\": \"maskable\"\n\
    }}\n\
  ]\n\
}}\n"
    )
}

/// Files every app shares; one core cache serves all apps so each stores them once.
const CORE_ASSETS: [&str; 5] = [
    "../site.wasm",
    "../titan.elf",
    "../_kit/kit.css",
    "../_kit/kit.js",
    "../_kit/demo-app.js",
];

/// `CORE_ASSETS` plus every neon web-kit file under `../_kit/`.
fn core_assets() -> Vec<String> {
    let mut list: Vec<String> = CORE_ASSETS.iter().map(|s| (*s).to_owned()).collect();
    list.extend(
        webkit::FILES
            .iter()
            .map(|(name, _)| format!("../_kit/{name}")),
    );
    list.push("../_kit/abi.json".to_owned());
    list
}

/// Files that belong to one app alone.
const APP_ASSETS: [&str; 6] = [
    "index.html",
    "app.js",
    "manifest.webmanifest",
    "icon-192.png",
    "icon-512.png",
    "icon-maskable-512.png",
];

/// Extra per-app files, cached only where the app has them.
const OPTIONAL_APP_ASSETS: [&str; 2] = ["app.css", "sig.js"];

fn hash_files<S: AsRef<str>>(app_out: &Path, rels: &[S]) -> Result<String, String> {
    let mut hash_buf = Vec::new();
    for rel in rels {
        let rel = rel.as_ref();
        let resolved = app_out.join(rel);
        let bytes = std::fs::read(&resolved).map_err(|e| {
            format!(
                "failed to read asset {} for sw hash: {e}",
                resolved.display()
            )
        })?;
        let digest = sphinx::sha256::digest(&bytes);
        hash_buf.extend_from_slice(rel.as_bytes());
        hash_buf.push(0);
        hash_buf.extend_from_slice(&digest);
    }
    let final_hash = sphinx::sha256::digest(&hash_buf);
    let mut hex_tag = String::with_capacity(16);
    for b in &final_hash[..8] {
        let _ = write!(hex_tag, "{b:02x}");
    }
    Ok(hex_tag)
}

fn js_string_array<S: AsRef<str>>(items: &[S]) -> String {
    let mut out = String::from("[\n");
    for (idx, item) in items.iter().enumerate() {
        let comma = if idx + 1 < items.len() { "," } else { "" };
        let _ = writeln!(out, "  \"{}\"{comma}", item.as_ref());
    }
    out.push(']');
    out
}

/// Lookups use only the two caches this worker opens, never the origin-wide `caches.match`.
const SW_TEMPLATE: &str = r#"const CORE_CACHE = "__CORE_CACHE__";
const CACHE_PREFIX = "__CACHE_PREFIX__";
const CACHE_NAME = "__CACHE_NAME__";
const CORE_ASSETS = __CORE_ASSETS__;
const ASSETS = __ASSETS__;

function lookup(req) {
  return Promise.all([caches.open(CORE_CACHE), caches.open(CACHE_NAME)]).then(([core, own]) =>
    core.match(req, { ignoreSearch: true }).then((hit) => hit || own.match(req, { ignoreSearch: true }))
  );
}

self.addEventListener("install", (event) => {
  event.waitUntil(
    Promise.all([
      caches.open(CORE_CACHE).then((cache) => cache.addAll(CORE_ASSETS)),
      caches.open(CACHE_NAME).then((cache) => cache.addAll(ASSETS)),
    ])
      .then(() => self.skipWaiting())
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            // Only this app's old caches. Old core caches stay: another app's old worker may still read them.
            .filter((key) => key.startsWith(CACHE_PREFIX) && key !== CACHE_NAME)
            .map((key) => caches.delete(key))
        )
      )
      .then(() => self.clients.claim())
  );
});

self.addEventListener("fetch", (event) => {
  const req = event.request;
  if (req.method !== "GET") {
    return;
  }
  const url = new URL(req.url);
  if (url.origin !== self.location.origin) {
    return;
  }
  event.respondWith(
    lookup(req).then((cached) => {
      if (cached) {
        return cached;
      }
      if (req.mode === "navigate" || url.pathname.endsWith("/")) {
        return caches
          .open(CACHE_NAME)
          .then((own) => own.match("index.html"))
          .then((indexResp) => indexResp || fetch(req));
      }
      return fetch(req);
    })
  );
});
"#;

fn build_service_worker(app: &AppSpec, app_out: &Path) -> Result<String, String> {
    let core = core_assets();
    let core_cache = format!("monolith-core-{}", hash_files(app_out, core.as_slice())?);
    let cache_prefix = format!("monolith-{}-", app.id);
    let mut own: Vec<&str> = APP_ASSETS.to_vec();
    own.extend(
        OPTIONAL_APP_ASSETS
            .into_iter()
            .filter(|name| app_out.join(name).is_file()),
    );
    let cache_name = format!("{cache_prefix}{}", hash_files(app_out, &own)?);
    Ok(SW_TEMPLATE
        .replace("__CORE_CACHE__", &core_cache)
        .replace("__CACHE_PREFIX__", &cache_prefix)
        .replace("__CACHE_NAME__", &cache_name)
        .replace("__CORE_ASSETS__", &js_string_array(core.as_slice()))
        .replace("__ASSETS__", &js_string_array(&own)))
}

fn copy_checked(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::copy(src, dst)
        .map_err(|e| format!("failed to copy {} to {}: {e}", src.display(), dst.display()))?;
    Ok(())
}

fn write_checked(dst: &Path, data: &[u8]) -> Result<(), String> {
    std::fs::write(dst, data).map_err(|e| format!("failed to write {}: {e}", dst.display()))
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
