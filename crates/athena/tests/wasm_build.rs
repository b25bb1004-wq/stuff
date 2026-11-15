//! Integration test for `athena::pack::pack`, `athena.wasm` exports, and web asset rules.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use athena::pack::pack;

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

#[test]
fn pack_produces_valid_wasm_and_conforming_web_bundle() {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("athena-pack-test");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).unwrap();
    }
    pack(&tmp).unwrap();

    for name in [
        "index.html",
        "athena.js",
        "style.css",
        "athena.wasm",
        "titan.elf",
    ] {
        let path = tmp.join(name);
        assert!(path.is_file(), "missing bundled file {}", path.display());
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() > 0, "empty bundled file {}", path.display());
    }

    let html = std::fs::read_to_string(tmp.join("index.html")).unwrap();
    for forbidden in ["http://", "https://", "//", "ws://", "wss://"] {
        assert!(
            !html.contains(forbidden),
            "index.html must reference only local files, found {forbidden}"
        );
    }
    assert!(html.contains("style.css"));
    assert!(html.contains("athena.js"));

    let js = std::fs::read_to_string(tmp.join("athena.js")).unwrap();
    for forbidden in ["alert(", "confirm(", "prompt("] {
        assert!(
            !js.contains(forbidden),
            "athena.js must never call {forbidden}"
        );
    }
    assert!(js.contains("F12"), "athena.js must handle F12");
    assert!(
        js.contains("role=\"tooltip\"") || js.contains("setAttribute(\"role\", \"tooltip\")"),
        "athena.js must set tooltip role"
    );

    let css = std::fs::read_to_string(tmp.join("style.css")).unwrap();
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
            css.contains(token),
            "style.css missing required token {token}"
        );
    }

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
    ] {
        assert!(
            exports.contains(required),
            "missing required wasm export `{required}`; exports = {exports:?}"
        );
    }
}
