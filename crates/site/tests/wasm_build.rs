//! Integration test for `site::pack::pack`, the wasm binary exports, and web assets.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use site::pack::pack;

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
fn pack_produces_valid_wasm_and_local_web_bundle() {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("site-pack-test");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).unwrap();
    }
    pack(&tmp).unwrap();

    for name in [
        "index.html",
        "site.js",
        "style.css",
        "systems.js",
        "site.wasm",
        "titan.elf",
        "athena/index.html",
    ] {
        let path = tmp.join(name);
        assert!(path.is_file(), "missing bundled file {}", path.display());
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() > 0, "empty bundled file {}", path.display());
    }

    for local_file in ["index.html", "site.js", "style.css", "systems.js"] {
        let text = std::fs::read_to_string(tmp.join(local_file)).unwrap();
        for forbidden in ["http://", "https://", "//", "ws://", "wss://"] {
            assert!(
                !text.contains(forbidden),
                "{local_file} must reference only local files, found {forbidden}"
            );
        }
    }

    let html = std::fs::read_to_string(tmp.join("index.html")).unwrap();
    assert!(html.contains("style.css"));
    assert!(html.contains("systems.js"));
    assert!(html.contains("site.js"));

    let systems_js = std::fs::read_to_string(tmp.join("systems.js")).unwrap();
    let expected_names = [
        "Titan",
        "Iridium",
        "Elysium",
        "Mercury",
        "Helios",
        "Cobalt",
        "Logos",
        "Odyssey",
        "Aurum",
        "Sphinx + Medusa",
        "Silicon",
        "Hades",
        "Pandora",
        "Argo",
        "Krypton",
        "Iris",
        "Indium",
        "Calliope",
        "Athena",
        "Chronos",
        "Xenon",
        "Lithium",
        "Kairos",
        "Midas",
        "Zeus",
    ];
    let nums: Vec<u32> = systems_js
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("num:")
                .map(|rest| rest.trim().trim_end_matches(',').parse::<u32>().unwrap())
        })
        .collect();
    assert_eq!(nums, (1..=25).collect::<Vec<u32>>());

    let names: Vec<String> = systems_js
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed.strip_prefix("name:").map(|rest| {
                rest.trim()
                    .trim_end_matches(',')
                    .trim_matches('"')
                    .to_owned()
            })
        })
        .collect();
    assert_eq!(
        names,
        expected_names
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>()
    );

    for demo_id in [
        "iridium", "elysium", "mercury", "helios", "cobalt", "odyssey", "aurum", "sphinx",
        "silicon", "hades", "pandora", "chisel", "argo", "krypton", "iris", "indium", "calliope",
        "chronos", "xenon", "lithium", "kairos",
    ] {
        let needle_main = format!("demo: \"{demo_id}\"");
        let needle_extra = format!("id: \"{demo_id}\"");
        assert!(
            systems_js.contains(&needle_main) || systems_js.contains(&needle_extra),
            "systems.js missing demo wiring for `{demo_id}`"
        );
        assert!(
            site::demos::find(demo_id).is_some(),
            "registry missing demo `{demo_id}`"
        );
    }

    let wasm = std::fs::read(tmp.join("site.wasm")).unwrap();
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
        "site_demo",
        "site_last_ptr",
        "site_last_len",
        "site_pixels_w",
        "site_pixels_h",
        "site_list",
        "neon_alloc",
        "neon_free",
        "neon_last_ptr",
        "neon_last_len",
        "titan_boot",
        "titan_send",
        "titan_pump",
        "titan_output",
        "logos_run",
    ] {
        assert!(
            exports.contains(required),
            "missing required wasm export `{required}`; exports = {exports:?}"
        );
    }
}
