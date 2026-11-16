//! Motion lint over the packed site bundle (T103, docs/PLAN-NEXT.md 4.8 item 1).
//! Every css, js and html file goes through `neon::webkit::lint`; pages and the site's own scripts also go
//! through `lint_page` (no `requestAnimationFrame` outside the kit). Pages must link a stylesheet that
//! imports the shared tokens, and the size budgets of 4.6 hold for the kit files.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use neon::webkit;
use site::pack::pack;

/// Every file under `dir`, sorted, so failures print in a stable order.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn ext_of(path: &Path) -> &str {
    path.extension().and_then(|e| e.to_str()).unwrap_or("")
}

/// Stylesheet hrefs of one page, relative to the page's folder.
fn linked_css(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("rel=\"stylesheet\"") {
        let tag_start = rest[..at].rfind('<').unwrap_or(0);
        let tag_end = rest[at..].find('>').map_or(rest.len(), |e| at + e);
        let tag = &rest[tag_start..tag_end];
        if let Some(h) = tag.find("href=\"") {
            let body = &tag[h + 6..];
            if let Some(end) = body.find('"') {
                out.push(body[..end].to_owned());
            }
        }
        rest = &rest[tag_end..];
    }
    out
}

#[test]
fn packed_bundle_passes_motion_lint() {
    let bundle = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("site-motion-lint-bundle");
    if bundle.exists() {
        std::fs::remove_dir_all(&bundle).unwrap();
    }
    pack(&bundle).unwrap();

    let mut files = Vec::new();
    collect(&bundle, &mut files);
    let mut problems = Vec::new();
    let mut pages = 0;
    let mut texts = 0;
    for path in &files {
        let kind = ext_of(path);
        if !matches!(kind, "css" | "js" | "html") {
            continue;
        }
        texts += 1;
        let text = std::fs::read_to_string(path).unwrap();
        let shown = path
            .strip_prefix(&bundle)
            .unwrap_or(path)
            .display()
            .to_string();
        for p in webkit::lint(&text) {
            problems.push(format!("{shown}: {p}"));
        }
        let site_script = matches!(
            path.file_name().and_then(|n| n.to_str()),
            Some("site.js" | "systems.js" | "board.js" | "feed.js")
        ) && path.parent() == Some(bundle.as_path());
        if kind == "html" || site_script {
            for p in webkit::lint_page(&text) {
                problems.push(format!("{shown}: {p}"));
            }
        }
        if kind == "html" {
            pages += 1;
            let dir = path.parent().unwrap_or(&bundle);
            let links_tokens = linked_css(&text).iter().any(|href| {
                href.ends_with("tokens.css")
                    || std::fs::read_to_string(dir.join(href))
                        .map(|css| css.contains("tokens.css"))
                        .unwrap_or(false)
            });
            if !links_tokens {
                problems.push(format!("{shown}: page does not link tokens.css"));
            }
        }
    }
    assert!(
        pages > 0 && texts > pages,
        "the bundle has no pages to lint"
    );
    assert!(
        problems.is_empty(),
        "motion lint problems:\n{}",
        problems.join("\n")
    );
    kit_tokens_and_budgets_hold(&bundle.join("_kit"));
}

/// The reduced-motion switch and the 4.6 size budgets, checked on the packed kit files.
fn kit_tokens_and_budgets_hold(kit: &Path) {
    let tokens = std::fs::read_to_string(kit.join("tokens.css")).unwrap();
    assert!(
        tokens.contains("prefers-reduced-motion: reduce"),
        "tokens.css must hold the reduced-motion switch"
    );
    let mut files = Vec::new();
    collect(kit, &mut files);
    for path in files {
        let len = std::fs::metadata(&path).unwrap().len();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if ext_of(&path) == "js" {
            assert!(len <= 64 * 1024, "{name} is over 64 KB");
        }
        if name == "motion.js" {
            assert!(len <= 10 * 1024, "motion.js is over its 10 KB budget");
        }
        if name == "tokens.css" {
            assert!(len <= 4 * 1024, "tokens.css is over its 4 KB budget");
        }
    }
}
