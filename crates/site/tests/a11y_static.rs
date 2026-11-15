//! Static accessibility and performance checks over the site and app HTML pages (T76).
//! Only the HTML source is checked; controls that JavaScript creates are covered by review.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The site page, the launcher, and one page per standalone app (skipping `_kit`).
fn html_pages() -> Vec<PathBuf> {
    let apps = crate_dir().join("apps");
    let mut pages = vec![crate_dir().join("web/index.html"), apps.join("index.html")];
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&apps)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir() && p.file_name().and_then(|n| n.to_str()) != Some("_kit"))
        .collect();
    dirs.sort();
    pages.extend(dirs.into_iter().map(|d| d.join("index.html")));
    pages
}

/// Start tags as (lowercased name, raw attribute text), in document order.
fn start_tags(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        let after = &rest[lt + 1..];
        let Some(gt) = after.find('>') else { break };
        let inner = &after[..gt];
        rest = &after[gt + 1..];
        if inner.starts_with('/') || inner.starts_with('!') {
            continue;
        }
        let name_end = inner.find(char::is_whitespace).unwrap_or(inner.len());
        out.push((
            inner[..name_end].to_ascii_lowercase(),
            inner[name_end..].to_owned(),
        ));
    }
    out
}

/// Value of a quoted or bare attribute; `Some("")` for a boolean attribute.
fn attr_value(attrs: &str, name: &str) -> Option<String> {
    let mut search = 0;
    while let Some(pos) = attrs[search..].find(name) {
        let start = search + pos;
        let end = start + name.len();
        let before_ok = start == 0 || attrs.as_bytes()[start - 1].is_ascii_whitespace();
        let after = &attrs[end..];
        let delimited =
            after.is_empty() || after.starts_with(|c: char| c.is_whitespace() || c == '=');
        if before_ok && delimited {
            if let Some(v) = after.trim_start().strip_prefix('=') {
                let v = v.trim_start();
                let quote = v.chars().next().unwrap_or(' ');
                if quote == '"' || quote == '\'' {
                    let body = &v[1..];
                    return Some(body[..body.find(quote).unwrap_or(body.len())].to_owned());
                }
                return Some(v.split_whitespace().next().unwrap_or("").to_owned());
            }
            return Some(String::new());
        }
        search = end;
    }
    None
}

/// Attributes and inner HTML of every element with this name (none of them nest).
fn elements_inner(html: &str, name: &str) -> Vec<(String, String)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find(&open) {
        let after = &rest[pos..];
        let gt = after.find('>').unwrap();
        let attrs = after[open.len()..gt].to_owned();
        let body = &after[gt + 1..];
        let end = body.find(&close).unwrap_or(body.len());
        out.push((attrs, body[..end].to_owned()));
        rest = &body[end..];
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// A form control is labelled by aria-label, aria-labelledby, or a `<label for>` naming its id.
fn has_label(html: &str, attrs: &str) -> bool {
    if attr_value(attrs, "aria-label").is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    if attr_value(attrs, "aria-labelledby").is_some() {
        return true;
    }
    match attr_value(attrs, "id") {
        Some(id) if !id.is_empty() => html.contains(&format!("for=\"{id}\"")),
        _ => false,
    }
}

fn check_page(path: &Path, kit_js: &str) -> Vec<String> {
    let html = std::fs::read_to_string(path).unwrap();
    let file = path.display().to_string();
    let mut errs = Vec::new();
    let tags = start_tags(&html);
    let count = |n: &str| tags.iter().filter(|(name, _)| name == n).count();

    match tags.iter().find(|(name, _)| name == "html") {
        Some((_, attrs)) if attr_value(attrs, "lang").is_some_and(|v| !v.is_empty()) => {}
        _ => errs.push(format!("{file}: <html> needs a lang attribute")),
    }

    // App pages get their titlebar (the h1 and the header landmark) from _kit/kit.js, so their source has neither.
    let app_page = html.contains("id=\"app-root\"");
    let kit_h1 = kit_js.contains("createEl(\"h1\", \"kit-titlebar-name\"");
    let kit_header = kit_js.contains("createEl(\"header\", \"kit-titlebar\"");
    let h1 = count("h1");
    if !(h1 == 1 || (h1 == 0 && app_page && kit_h1)) {
        errs.push(format!("{file}: expected one h1, found {h1}"));
    }

    if count("main") == 0 {
        errs.push(format!("{file}: missing <main> landmark"));
    }
    if count("header") == 0 && !(app_page && kit_header) {
        errs.push(format!("{file}: missing <header> landmark"));
    }
    let is_site = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        == Some("web");
    if is_site && count("nav") == 0 {
        errs.push(format!("{file}: site page needs a <nav> landmark"));
    }

    for (attrs, inner) in elements_inner(&html, "button") {
        let has_text = !strip_tags(&inner).trim().is_empty();
        let has_name = attr_value(&attrs, "aria-label").is_some_and(|v| !v.trim().is_empty());
        if !has_text && !has_name {
            errs.push(format!(
                "{file}: button without text or aria-label: {attrs}"
            ));
        }
    }

    for (name, attrs) in &tags {
        match name.as_str() {
            "select" | "textarea" => {
                if !has_label(&html, attrs) {
                    errs.push(format!("{file}: <{name}> without a label: {attrs}"));
                }
            }
            "input" => {
                let typ = attr_value(attrs, "type").unwrap_or_else(|| "text".to_owned());
                let exempt = matches!(typ.as_str(), "hidden" | "button" | "submit" | "reset");
                if !exempt && !has_label(&html, attrs) {
                    errs.push(format!("{file}: <input> without a label: {attrs}"));
                }
            }
            "img" => {
                if attr_value(attrs, "alt").is_none() {
                    errs.push(format!("{file}: <img> without alt"));
                }
                if attr_value(attrs, "width").is_none() || attr_value(attrs, "height").is_none() {
                    errs.push(format!("{file}: <img> without width and height"));
                }
            }
            "script" => {
                if let Some(src) = attr_value(attrs, "src") {
                    let deferred = attr_value(attrs, "defer").is_some()
                        || attr_value(attrs, "type").as_deref() == Some("module");
                    if !deferred {
                        errs.push(format!("{file}: script {src} must be defer or module"));
                    }
                }
            }
            _ => {}
        }
    }

    // Skip link: a fragment link whose target exists on the same page.
    let skip = tags.iter().find(|(name, attrs)| {
        name == "a"
            && attr_value(attrs, "class")
                .is_some_and(|c| c.split_whitespace().any(|t| t == "skip-link"))
    });
    match skip.and_then(|(_, attrs)| attr_value(attrs, "href")) {
        Some(href) if href.starts_with('#') && html.contains(&format!("id=\"{}\"", &href[1..])) => {
        }
        _ => errs.push(format!(
            "{file}: missing skip link to an element on the page"
        )),
    }

    errs
}

#[test]
fn every_html_page_has_lang_landmarks_skip_link_and_labels() {
    let kit_js = std::fs::read_to_string(crate_dir().join("apps/_kit/kit.js")).unwrap();
    let mut errs = Vec::new();
    for page in html_pages() {
        errs.extend(check_page(&page, &kit_js));
    }
    assert!(
        errs.is_empty(),
        "accessibility problems:\n{}",
        errs.join("\n")
    );
}

#[test]
fn scripts_defer_and_stylesheets_honour_reduced_motion() {
    for css in ["web/style.css", "apps/_kit/kit.css"] {
        let text = std::fs::read_to_string(crate_dir().join(css)).unwrap();
        assert!(
            text.contains("prefers-reduced-motion: reduce"),
            "{css} needs a reduced-motion rule"
        );
        assert!(
            text.contains("--line:"),
            "{css} must define the --line token"
        );
    }
    let mut blocking = Vec::new();
    for page in html_pages() {
        let html = std::fs::read_to_string(&page).unwrap();
        for (name, attrs) in start_tags(&html) {
            if name != "script" || attr_value(&attrs, "src").is_none() {
                continue;
            }
            let ok = attr_value(&attrs, "defer").is_some()
                || attr_value(&attrs, "type").as_deref() == Some("module");
            if !ok {
                blocking.push(format!("{}: {attrs}", page.display()));
            }
        }
    }
    assert!(
        blocking.is_empty(),
        "blocking scripts:\n{}",
        blocking.join("\n")
    );
}
