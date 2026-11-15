//! Web kit shared by every neon-based page: the kit files, the wasm build helper, a content hash,
//! the service worker template and the bundle lint.

use std::path::Path;
use std::process::Command;

/// The kit files as `(name under _kit/, contents)`.
pub const FILES: &[(&str, &str)] = &[
    ("tokens.css", include_str!("../web/kit/tokens.css")),
    ("wasm.js", include_str!("../web/kit/wasm.js")),
    ("term.js", include_str!("../web/kit/term.js")),
];

const SW_TEMPLATE: &str = r#"// Service worker written by neon::webkit::sw_js. It opens only its own cache.
var PREFIX = __PREFIX__;
var CACHE = PREFIX + "-" + __HASH__;
var FILES = __FILES__;
self.addEventListener("install", function (ev) {
  ev.waitUntil(caches.open(CACHE).then(function (cache) {
    return cache.addAll(FILES);
  }).then(function () {
    return self.skipWaiting();
  }));
});
self.addEventListener("activate", function (ev) {
  ev.waitUntil(caches.keys().then(function (keys) {
    return Promise.all(keys.filter(function (k) {
      return k.indexOf(PREFIX + "-") === 0 && k !== CACHE;
    }).map(function (k) {
      return caches.delete(k);
    }));
  }).then(function () {
    return self.clients.claim();
  }));
});
self.addEventListener("fetch", function (ev) {
  if (ev.request.method !== "GET") {
    return;
  }
  if (new URL(ev.request.url).origin !== self.location.origin) {
    return;
  }
  ev.respondWith(caches.open(CACHE).then(function (cache) {
    return cache.match(ev.request).then(function (hit) {
      return hit || fetch(ev.request);
    });
  }));
});
"#;

/// Run cargo for `args` in the repo root with its own target dir; the error carries cargo's stderr.
pub fn run_cargo(cargo: &str, root: &Path, target_dir: &Path, args: &[&str]) -> Result<(), String> {
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

/// FNV-1a 64-bit hash of all parts as 16 hex digits. Names caches; not a security hash.
pub fn content_hash(parts: &[&[u8]]) -> String {
    const PRIME: u64 = 0x0100_0000_01b3;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for &b in *part {
            h ^= u64::from(b);
            h = h.wrapping_mul(PRIME);
        }
        // Separator byte, so that ["ab", "c"] and ["a", "bc"] hash differently.
        h ^= 0xff;
        h = h.wrapping_mul(PRIME);
    }
    format!("{h:016x}")
}

/// Service worker source. It uses only the cache `<prefix>-<hash>`, precaches `files` (paths
/// relative to the worker) and answers cache first, then the network.
pub fn sw_js(prefix: &str, hash: &str, files: &[&str]) -> String {
    let list = files
        .iter()
        .map(|f| js_str(f))
        .collect::<Vec<_>>()
        .join(", ");
    SW_TEMPLATE
        .replace("__PREFIX__", &js_str(prefix))
        .replace("__HASH__", &js_str(hash))
        .replace("__FILES__", &format!("[{list}]"))
}

/// A JavaScript string literal, escaped so that no input can end the literal or the script.
fn js_str(s: &str) -> String {
    const BACKSLASH: char = '\u{5c}';
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => {
                out.push(BACKSLASH);
                out.push('"');
            }
            '\\' => {
                out.push(BACKSLASH);
                out.push(BACKSLASH);
            }
            '<' => out.push_str(&format!("{BACKSLASH}u003c")),
            c if c.is_control() => out.push_str(&format!("{BACKSLASH}u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Problems found in one bundle file (empty when it passes): external URLs and layout-animating
/// transitions. Pages may load only files served next to them.
pub fn lint(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for scheme in ["http://", "https://"] {
        if text.contains(scheme) {
            problems.push(format!(
                "external URL `{scheme}`: load only files served next to the page"
            ));
        }
    }
    let lower = text.to_ascii_lowercase();
    for (at, _) in lower.match_indices("transition") {
        let rest = lower.get(at + "transition".len()..).unwrap_or("");
        let rest = rest.strip_prefix("-property").unwrap_or(rest).trim_start();
        let value = match rest.strip_prefix(':') {
            Some(value) => value,
            None => continue,
        };
        let decl = value.split([';', '}', '\n']).next().unwrap_or("");
        for item in decl.split(',') {
            let prop = item.split_whitespace().next().unwrap_or("");
            if prop == "width" || prop == "height" {
                problems.push(format!(
                    "layout-animating transition on `{prop}`: animate only opacity and transform"
                ));
            }
        }
    }
    problems
}
