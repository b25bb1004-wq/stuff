//! Host bundler that compiles `site` (wasm32), `titan` (riscv64), and packs `athena` and all apps.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::icons::render_icon_png;

struct AppSpec {
    id: &'static str,
    name: &'static str,
    initial: char,
    uses_demo_app: bool,
    uses_titan_elf: bool,
}

const APPS: [AppSpec; 23] = [
    AppSpec {
        id: "titan",
        name: "Titan",
        initial: 'T',
        uses_demo_app: false,
        uses_titan_elf: true,
    },
    AppSpec {
        id: "iridium",
        name: "Iridium",
        initial: 'I',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "elysium",
        name: "Elysium",
        initial: 'E',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "mercury",
        name: "Mercury",
        initial: 'M',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "helios",
        name: "Helios",
        initial: 'H',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "cobalt",
        name: "Cobalt",
        initial: 'C',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "logos",
        name: "Logos",
        initial: 'L',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "odyssey",
        name: "Odyssey",
        initial: 'O',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "aurum",
        name: "Aurum",
        initial: 'A',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "sphinx",
        name: "Sphinx",
        initial: 'S',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "silicon",
        name: "Silicon",
        initial: 'S',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "chisel",
        name: "Chisel",
        initial: 'C',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "hades",
        name: "Hades",
        initial: 'H',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "pandora",
        name: "Pandora",
        initial: 'P',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "argo",
        name: "Argo",
        initial: 'A',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "krypton",
        name: "Krypton",
        initial: 'K',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "iris",
        name: "Iris",
        initial: 'I',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "indium",
        name: "Indium",
        initial: 'I',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "calliope",
        name: "Calliope",
        initial: 'C',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "chronos",
        name: "Chronos",
        initial: 'C',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "xenon",
        name: "Xenon",
        initial: 'X',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "lithium",
        name: "Lithium",
        initial: 'L',
        uses_demo_app: true,
        uses_titan_elf: false,
    },
    AppSpec {
        id: "kairos",
        name: "Kairos",
        initial: 'K',
        uses_demo_app: true,
        uses_titan_elf: false,
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

fn build_service_worker(app: &AppSpec, app_out: &Path) -> Result<String, String> {
    let mut assets: Vec<&'static str> = vec![
        "index.html",
        "app.js",
        "manifest.webmanifest",
        "icon-192.png",
        "icon-512.png",
        "icon-maskable-512.png",
        "../_kit/kit.css",
        "../_kit/kit.js",
    ];
    if app.uses_demo_app {
        assets.push("../_kit/demo-app.js");
    }
    assets.push("../site.wasm");
    if app.uses_titan_elf {
        assets.push("../titan.elf");
    }

    let mut hash_buf = Vec::new();
    for rel in &assets {
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

    let cache_prefix = format!("monolith-{}-", app.id);
    let cache_name = format!("{cache_prefix}{hex_tag}");
    let mut assets_js = String::from("[\n");
    for (idx, rel) in assets.iter().enumerate() {
        let comma = if idx + 1 < assets.len() { "," } else { "" };
        let _ = writeln!(assets_js, "  \"{rel}\"{comma}");
    }
    assets_js.push(']');

    Ok(format!(
        "const CACHE_PREFIX = \"{cache_prefix}\";\n\
const CACHE_NAME = \"{cache_name}\";\n\
const ASSETS = {assets_js};\n\
\n\
self.addEventListener(\"install\", (event) => {{\n\
  event.waitUntil(\n\
    caches\n\
      .open(CACHE_NAME)\n\
      .then((cache) => cache.addAll(ASSETS))\n\
      .then(() => self.skipWaiting())\n\
  );\n\
}});\n\
\n\
self.addEventListener(\"activate\", (event) => {{\n\
  event.waitUntil(\n\
    caches\n\
      .keys()\n\
      .then((keys) =>\n\
        Promise.all(\n\
          keys\n\
            .filter((key) => key.startsWith(CACHE_PREFIX) && key !== CACHE_NAME)\n\
            .map((key) => caches.delete(key))\n\
        )\n\
      )\n\
      .then(() => self.clients.claim())\n\
  );\n\
}});\n\
\n\
self.addEventListener(\"fetch\", (event) => {{\n\
  const req = event.request;\n\
  if (req.method !== \"GET\") {{\n\
    return;\n\
  }}\n\
  const url = new URL(req.url);\n\
  if (url.origin !== self.location.origin) {{\n\
    return;\n\
  }}\n\
  event.respondWith(\n\
    caches.match(req, {{ ignoreSearch: true }}).then((cached) => {{\n\
      if (cached) {{\n\
        return cached;\n\
      }}\n\
      if (req.mode === \"navigate\" || url.pathname.endsWith(\"/\")) {{\n\
        return caches.match(\"index.html\").then((indexResp) => {{\n\
          if (indexResp) {{\n\
            return indexResp;\n\
          }}\n\
          return fetch(req);\n\
        }});\n\
      }}\n\
      return fetch(req);\n\
    }})\n\
  );\n\
}});\n"
    ))
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
