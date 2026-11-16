//! Integration test for `athena::pack::pack`, `athena.wasm` exports, PWA assets, and web rules.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use std::path::Path;

use athena::pack::pack;
use neon::webkit::lint;

/// Every file under `dir` as a `/`-separated path relative to it.
fn files_under(dir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                todo.push(path);
            } else {
                let rel = path.strip_prefix(dir).unwrap();
                out.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> u8 {
        let b = self.bytes[self.pos];
        self.pos += 1;
        b
    }

    fn read_bytes(&mut self, n: usize) -> &'a [u8] {
        let slice = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        slice
    }

    fn read_leb_u32(&mut self) -> u32 {
        let mut result = 0u32;
        let mut shift = 0u32;
        loop {
            let byte = self.read_u8();
            result |= u32::from(byte & 0x7f) << shift;
            if (byte & 0x80) == 0 {
                return result;
            }
            shift += 7;
            assert!(shift < 35, "LEB128 overflow");
        }
    }

    fn read_name(&mut self) -> String {
        let len = self.read_leb_u32() as usize;
        let raw = self.read_bytes(len);
        std::str::from_utf8(raw).unwrap().to_owned()
    }

    fn skip_limits(&mut self) {
        let flags = self.read_u8();
        let _min = self.read_leb_u32();
        if (flags & 1) != 0 {
            let _max = self.read_leb_u32();
        }
    }
}

#[derive(Debug, PartialEq)]
enum JsonVal {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(BTreeMap<String, JsonVal>),
}

impl JsonVal {
    fn as_str(&self) -> Option<&str> {
        match self {
            JsonVal::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn as_arr(&self) -> Option<&[JsonVal]> {
        match self {
            JsonVal::Arr(a) => Some(a.as_slice()),
            _ => None,
        }
    }

    fn as_obj(&self) -> Option<&BTreeMap<String, JsonVal>> {
        match self {
            JsonVal::Obj(o) => Some(o),
            _ => None,
        }
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn parse(input: &'a str) -> JsonVal {
        let mut p = Self {
            bytes: input.as_bytes(),
            pos: 0,
        };
        let val = p.parse_val();
        p.skip_ws();
        assert_eq!(p.pos, p.bytes.len(), "trailing bytes in JSON");
        val
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> u8 {
        self.bytes[self.pos]
    }

    fn next(&mut self) -> u8 {
        let b = self.bytes[self.pos];
        self.pos += 1;
        b
    }

    fn expect(&mut self, expected: &[u8]) {
        assert!(
            self.bytes[self.pos..].starts_with(expected),
            "expected {:?} at pos {}",
            std::str::from_utf8(expected),
            self.pos
        );
        self.pos += expected.len();
    }

    fn parse_val(&mut self) -> JsonVal {
        self.skip_ws();
        match self.peek() {
            b'"' => JsonVal::Str(self.parse_str()),
            b'{' => self.parse_obj(),
            b'[' => self.parse_arr(),
            b't' => {
                self.expect(b"true");
                JsonVal::Bool(true)
            }
            b'f' => {
                self.expect(b"false");
                JsonVal::Bool(false)
            }
            b'n' => {
                self.expect(b"null");
                JsonVal::Null
            }
            b'-' | b'0'..=b'9' => self.parse_num(),
            other => panic!("unexpected JSON byte {other} at pos {}", self.pos),
        }
    }

    fn parse_str(&mut self) -> String {
        assert_eq!(self.next(), b'"');
        let mut out = String::new();
        loop {
            let b = self.next();
            match b {
                b'"' => return out,
                b'\\' => {
                    let esc = self.next();
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        _ => panic!("unsupported JSON escape \\{}", esc as char),
                    }
                }
                _ => out.push(b as char),
            }
        }
    }

    fn parse_num(&mut self) -> JsonVal {
        let start = self.pos;
        if self.peek() == b'-' {
            self.pos += 1;
        }
        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_digit()
                || matches!(self.bytes[self.pos], b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            self.pos += 1;
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
        JsonVal::Num(s.to_owned())
    }

    fn parse_arr(&mut self) -> JsonVal {
        assert_eq!(self.next(), b'[');
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == b']' {
            self.pos += 1;
            return JsonVal::Arr(items);
        }
        loop {
            items.push(self.parse_val());
            self.skip_ws();
            match self.next() {
                b',' => {}
                b']' => return JsonVal::Arr(items),
                other => panic!("expected , or ] in JSON array, got {other}"),
            }
        }
    }

    fn parse_obj(&mut self) -> JsonVal {
        assert_eq!(self.next(), b'{');
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == b'}' {
            self.pos += 1;
            return JsonVal::Obj(map);
        }
        loop {
            self.skip_ws();
            let key = self.parse_str();
            self.skip_ws();
            assert_eq!(self.next(), b':');
            let val = self.parse_val();
            map.insert(key, val);
            self.skip_ws();
            match self.next() {
                b',' => {}
                b'}' => return JsonVal::Obj(map),
                other => panic!("expected , or }} in JSON object, got {other}"),
            }
        }
    }
}

fn verify_png(bytes: &[u8], expected_size: u32, name: &str) {
    assert!(bytes.len() >= 24, "{name} too short for PNG IHDR");
    assert_eq!(
        &bytes[0..8],
        &[137, 80, 78, 71, 13, 10, 26, 10],
        "{name} missing PNG signature"
    );
    assert_eq!(
        &bytes[8..12],
        &13u32.to_be_bytes(),
        "{name} bad IHDR length"
    );
    assert_eq!(&bytes[12..16], b"IHDR", "{name} first chunk must be IHDR");
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    assert_eq!(width, expected_size, "{name} wrong IHDR width");
    assert_eq!(height, expected_size, "{name} wrong IHDR height");
}

#[test]
fn pack_produces_valid_wasm_and_conforming_web_bundle() {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("athena-pack-test");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).unwrap();
    }
    pack(&tmp).unwrap();

    let expected_files: BTreeSet<String> = [
        "index.html",
        "athena.js",
        "workbench.js",
        "style.css",
        "athena.wasm",
        "titan.elf",
        "manifest.webmanifest",
        "sw.js",
        "icon-192.png",
        "icon-512.png",
        "icon-maskable-512.png",
        "debug.js",
        "fs-api.js",
        "text-spans.js",
        "athena-motion.js",
        "selftest.html",
    ]
    .into_iter()
    .map(String::from)
    .chain(
        neon::webkit::FILES
            .iter()
            .map(|(name, _)| format!("_kit/{name}")),
    )
    .collect();

    let actual_files = files_under(&tmp);
    for name in &actual_files {
        let meta = std::fs::metadata(tmp.join(name)).unwrap();
        assert!(meta.len() > 0, "empty bundled file {name}");
    }
    assert_eq!(actual_files, expected_files, "bundle file set mismatch");

    // Every shipped text file passes the bundle lint: no external URLs, no layout transitions.
    for name in expected_files
        .iter()
        .filter(|n| !n.ends_with(".wasm") && !n.ends_with(".png") && !n.ends_with(".elf"))
    {
        let text = std::fs::read_to_string(tmp.join(name)).unwrap();
        let problems = lint(&text);
        assert!(problems.is_empty(), "{name}: {problems:?}");
    }

    let html = std::fs::read_to_string(tmp.join("index.html")).unwrap();
    for forbidden in ["http://", "https://", "//", "ws://", "wss://"] {
        assert!(
            !html.contains(forbidden),
            "index.html must reference only local files, found {forbidden}"
        );
    }
    assert!(html.contains("manifest.webmanifest"));
    assert!(html.contains("style.css"));
    assert!(html.contains("athena.js"));
    assert!(html.contains("type=\"module\""));

    for js_name in ["athena.js", "workbench.js", "sw.js"] {
        let js = std::fs::read_to_string(tmp.join(js_name)).unwrap();
        for forbidden in ["alert(", "confirm(", "prompt(", "http://", "https://"] {
            assert!(
                !js.contains(forbidden),
                "{js_name} must never contain {forbidden}"
            );
        }
    }
    let athena_js = std::fs::read_to_string(tmp.join("athena.js")).unwrap();
    assert!(athena_js.contains("F12"), "athena.js must handle F12");
    assert!(
        athena_js.contains("role=\"tooltip\"")
            || athena_js.contains("setAttribute(\"role\", \"tooltip\")"),
        "athena.js must set tooltip role"
    );
    assert!(athena_js.contains("serviceWorker"));

    let css = std::fs::read_to_string(tmp.join("style.css")).unwrap();
    // Since T101 the shared tokens live in the kit's tokens.css, which index.html links first.
    let tokens = std::fs::read_to_string(tmp.join("_kit").join("tokens.css")).unwrap();
    for token in [
        "--bg",
        "--panel",
        "--line",
        "--text",
        "--muted",
        "--accent",
        "--ok",
        "--bad",
        "--font-ui",
        "--font-code",
        "--t-fast",
        "--t-med",
        "--t-slow",
    ] {
        assert!(
            css.contains(token) || tokens.contains(token),
            "style.css and _kit/tokens.css both lack required token {token}"
        );
    }
    assert!(css.contains("window-controls-overlay"));
    assert!(css.contains("titlebar-area-"));

    let manifest_raw = std::fs::read_to_string(tmp.join("manifest.webmanifest")).unwrap();
    let manifest_val = JsonParser::parse(&manifest_raw);
    let manifest = manifest_val.as_obj().expect("manifest must be JSON object");
    assert_eq!(
        manifest.get("name").and_then(JsonVal::as_str),
        Some("Athena")
    );
    assert_eq!(
        manifest.get("short_name").and_then(JsonVal::as_str),
        Some("Athena")
    );
    assert_eq!(
        manifest.get("display").and_then(JsonVal::as_str),
        Some("standalone")
    );
    let display_override: Vec<&str> = manifest
        .get("display_override")
        .and_then(JsonVal::as_arr)
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        display_override,
        vec!["window-controls-overlay", "standalone"]
    );
    assert_eq!(
        manifest.get("start_url").and_then(JsonVal::as_str),
        Some("./")
    );
    assert_eq!(manifest.get("scope").and_then(JsonVal::as_str), Some("./"));
    assert_eq!(
        manifest.get("theme_color").and_then(JsonVal::as_str),
        Some("#0e1013")
    );
    assert_eq!(
        manifest.get("background_color").and_then(JsonVal::as_str),
        Some("#0e1013")
    );
    let icons = manifest
        .get("icons")
        .and_then(JsonVal::as_arr)
        .expect("manifest icons must be array");
    assert_eq!(icons.len(), 3);
    for icon in icons {
        let obj = icon.as_obj().unwrap();
        let src = obj.get("src").and_then(JsonVal::as_str).unwrap();
        assert!(!src.starts_with('/'), "icon src must be relative: {src}");
        assert!(!src.contains("://"), "icon src must be relative: {src}");
        assert_eq!(obj.get("type").and_then(JsonVal::as_str), Some("image/png"));
    }

    verify_png(
        &std::fs::read(tmp.join("icon-192.png")).unwrap(),
        192,
        "icon-192.png",
    );
    verify_png(
        &std::fs::read(tmp.join("icon-512.png")).unwrap(),
        512,
        "icon-512.png",
    );
    verify_png(
        &std::fs::read(tmp.join("icon-maskable-512.png")).unwrap(),
        512,
        "icon-maskable-512.png",
    );

    let sw_js = std::fs::read_to_string(tmp.join("sw.js")).unwrap();
    assert!(
        !sw_js.contains("caches.match("),
        "sw.js must answer from its own opened cache"
    );
    let assets_prefix = "var FILES = ";
    let assets_start = sw_js.find(assets_prefix).unwrap() + assets_prefix.len();
    let assets_end = sw_js[assets_start..].find(";\n").unwrap() + assets_start;
    let sw_assets_val = JsonParser::parse(&sw_js[assets_start..assets_end]);
    let sw_assets: BTreeSet<String> = sw_assets_val
        .as_arr()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    let mut want: BTreeSet<String> = expected_files
        .iter()
        .filter(|n| n.as_str() != "sw.js")
        .map(|n| {
            if n.starts_with("_kit/") {
                format!("/{n}")
            } else {
                n.clone()
            }
        })
        .collect();
    want.insert(String::from("./"));
    assert_eq!(
        sw_assets, want,
        "sw.js FILES must list the page, every bundle file and the kit, and nothing else"
    );
    assert!(
        sw_js.contains("var PREFIX = \"athena\";"),
        "sw.js must use the athena- cache prefix"
    );

    let wasm = std::fs::read(tmp.join("athena.wasm")).unwrap();
    assert!(wasm.len() >= 8, "wasm too short");
    assert_eq!(&wasm[0..4], b"\0asm", "bad wasm magic");
    assert_eq!(&wasm[4..8], &[1, 0, 0, 0], "bad wasm version");

    let mut cur = Cursor::new(&wasm[8..]);
    let mut exports = BTreeSet::new();
    let mut imports = Vec::new();

    while cur.remaining() > 0 {
        let sec_id = cur.read_u8();
        let sec_len = cur.read_leb_u32() as usize;
        let payload = cur.read_bytes(sec_len);
        match sec_id {
            2 => {
                let mut ic = Cursor::new(payload);
                let count = ic.read_leb_u32();
                for _ in 0..count {
                    let module = ic.read_name();
                    let field = ic.read_name();
                    let kind = ic.read_u8();
                    match kind {
                        0 => {
                            let _ty = ic.read_leb_u32();
                        }
                        1 => {
                            let _elem = ic.read_u8();
                            ic.skip_limits();
                        }
                        2 => ic.skip_limits(),
                        3 => {
                            let _val = ic.read_u8();
                            let _mut = ic.read_u8();
                        }
                        other => panic!("unknown import kind {other}"),
                    }
                    imports.push((module, field));
                }
                assert_eq!(ic.remaining(), 0);
            }
            7 => {
                let mut ec = Cursor::new(payload);
                let count = ec.read_leb_u32();
                for _ in 0..count {
                    let name = ec.read_name();
                    let _kind = ec.read_u8();
                    let _idx = ec.read_leb_u32();
                    exports.insert(name);
                }
                assert_eq!(ec.remaining(), 0);
            }
            _ => {}
        }
    }

    for (module, field) in &imports {
        assert_eq!(
            module, "env",
            "unexpected import outside module `env`: {module}.{field}"
        );
    }

    for required in [
        "memory",
        "neon_alloc",
        "neon_free",
        "neon_last_ptr",
        "neon_last_len",
        "titan_boot",
        "titan_send",
        "titan_pump",
        "titan_output",
        "logos_run",
        "athena_last_ptr",
        "athena_last_len",
        "athena_check",
        "athena_highlight",
        "athena_test",
        "athena_definition",
        "athena_hover",
        "athena_format",
        "athena_debug_start",
        "athena_debug_step",
        "athena_debug_frame",
        "athena_debug_stop",
        "athena_alloc",
        "athena_free",
    ] {
        assert!(
            exports.contains(required),
            "missing required wasm export `{required}`; exports = {exports:?}"
        );
    }
}
