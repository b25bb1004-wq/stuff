//! Integration tests for standalone installable apps, launcher, icons, manifests, and service workers.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use site::pack::pack;

#[derive(Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    fn as_obj(&self) -> Option<&BTreeMap<String, Json>> {
        match self {
            Json::Obj(m) => Some(m),
            _ => None,
        }
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn expect(&mut self, b: u8) {
        self.skip_ws();
        assert_eq!(
            self.bytes.get(self.pos).copied(),
            Some(b),
            "expected '{}' at pos {}",
            char::from(b),
            self.pos
        );
        self.pos += 1;
    }

    fn parse_value(&mut self) -> Json {
        match self.peek().expect("unexpected EOF in JSON") {
            b'"' => Json::Str(self.parse_string()),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b't' => {
                assert_eq!(&self.bytes[self.pos..self.pos + 4], b"true");
                self.pos += 4;
                Json::Bool(true)
            }
            b'f' => {
                assert_eq!(&self.bytes[self.pos..self.pos + 5], b"false");
                self.pos += 5;
                Json::Bool(false)
            }
            b'n' => {
                assert_eq!(&self.bytes[self.pos..self.pos + 4], b"null");
                self.pos += 4;
                Json::Null
            }
            other => panic!(
                "unexpected JSON token '{}' at {}",
                char::from(other),
                self.pos
            ),
        }
    }

    fn parse_string(&mut self) -> String {
        self.expect(b'"');
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            self.pos += 1;
            if b == b'"' {
                return out;
            }
            if b == b'\\' {
                let esc = self.bytes[self.pos];
                self.pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    other => panic!("unsupported JSON escape \\{}", char::from(other)),
                }
            } else {
                out.push(char::from(b));
            }
        }
        panic!("unterminated JSON string");
    }

    fn parse_array(&mut self) -> Json {
        self.expect(b'[');
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Json::Arr(items);
        }
        loop {
            items.push(self.parse_value());
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Json::Arr(items);
                }
                other => panic!("expected ',' or ']' in array, got {other:?}"),
            }
        }
    }

    fn parse_object(&mut self) -> Json {
        self.expect(b'{');
        let mut map = BTreeMap::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Json::Obj(map);
        }
        loop {
            let key = self.parse_string();
            self.expect(b':');
            let val = self.parse_value();
            map.insert(key, val);
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Json::Obj(map);
                }
                other => panic!("expected ',' or '}}' in object, got {other:?}"),
            }
        }
    }
}

fn parse_json(text: &str) -> Json {
    let mut p = JsonParser::new(text);
    let val = p.parse_value();
    p.skip_ws();
    assert_eq!(p.pos, text.len(), "trailing bytes in JSON");
    val
}

fn check_png_ihdr(path: &Path, expected_w: u32, expected_h: u32) {
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() >= 24, "PNG too small: {}", path.display());
    assert_eq!(
        &bytes[..8],
        b"\x89PNG\r\n\x1a\n",
        "bad PNG signature in {}",
        path.display()
    );
    assert_eq!(
        &bytes[12..16],
        b"IHDR",
        "missing IHDR chunk in {}",
        path.display()
    );
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    assert_eq!(
        (w, h),
        (expected_w, expected_h),
        "unexpected PNG dimensions in {}",
        path.display()
    );
}

fn extract_sw_string(sw_text: &str, marker: &str) -> String {
    let start = sw_text.find(marker).expect("sw.js missing constant") + marker.len();
    let rest = &sw_text[start + 1..];
    rest[..rest.find('"').expect("unterminated string")].to_owned()
}

fn extract_sw_assets(sw_text: &str, marker: &str) -> Vec<String> {
    let start = sw_text.find(marker).expect("sw.js missing asset list") + marker.len();
    let rest = &sw_text[start..];
    let end = rest.find(';').expect("sw.js ASSETS missing ';'");
    let arr_json = parse_json(&rest[..end]);
    arr_json
        .as_arr()
        .expect("ASSETS must be a JSON array")
        .iter()
        .map(|v| v.as_str().expect("asset path must be string").to_owned())
        .collect()
}

fn extract_attr_values(html: &str, attr: &str) -> Vec<String> {
    let needle = format!("{attr}=\"");
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find(&needle) {
        let after = &rest[pos + needle.len()..];
        let end = after.find('"').expect("unclosed attribute quote");
        out.push(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    out
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn pack_produces_all_installable_apps_and_launcher() {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("site-apps-bundle-test");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).unwrap();
    }
    pack(&tmp).unwrap();

    for kit_file in ["_kit/kit.css", "_kit/kit.js", "_kit/demo-app.js"] {
        let p = tmp.join(kit_file);
        assert!(p.is_file(), "missing {}", p.display());
        assert!(std::fs::metadata(&p).unwrap().len() > 0);
    }

    let expected_apps: [(&str, &str); 23] = [
        ("titan", "Titan"),
        ("iridium", "Iridium"),
        ("elysium", "Elysium"),
        ("mercury", "Mercury"),
        ("helios", "Helios"),
        ("cobalt", "Cobalt"),
        ("logos", "Logos"),
        ("odyssey", "Odyssey"),
        ("aurum", "Aurum"),
        ("sphinx", "Sphinx"),
        ("silicon", "Silicon"),
        ("chisel", "Chisel"),
        ("hades", "Hades"),
        ("pandora", "Pandora"),
        ("argo", "Argo"),
        ("krypton", "Krypton"),
        ("iris", "Iris"),
        ("indium", "Indium"),
        ("calliope", "Calliope"),
        ("chronos", "Chronos"),
        ("xenon", "Xenon"),
        ("lithium", "Lithium"),
        ("kairos", "Kairos"),
    ];

    for demo in site::demos::all() {
        assert!(
            expected_apps.iter().any(|(id, _)| *id == demo.id),
            "expected_apps missing registered demo {}",
            demo.id
        );
    }

    let mut core_names: BTreeSet<String> = BTreeSet::new();
    for (id, title) in expected_apps {
        let app_dir = tmp.join(id);
        assert!(app_dir.is_dir(), "missing app dir {}", app_dir.display());

        let entries: BTreeSet<String> = std::fs::read_dir(&app_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let required: BTreeSet<String> = [
            "index.html",
            "app.js",
            "manifest.webmanifest",
            "sw.js",
            "icon-192.png",
            "icon-512.png",
            "icon-maskable-512.png",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(entries, required, "app {id} must have the exact 7 files");

        let manifest_text = std::fs::read_to_string(app_dir.join("manifest.webmanifest")).unwrap();
        let manifest = parse_json(&manifest_text);
        let obj = manifest.as_obj().unwrap();
        assert_eq!(
            obj.get("name").and_then(Json::as_str),
            Some(format!("Monolith {title}").as_str())
        );
        assert_eq!(obj.get("short_name").and_then(Json::as_str), Some(title));
        assert_eq!(obj.get("start_url").and_then(Json::as_str), Some("./"));
        assert_eq!(obj.get("scope").and_then(Json::as_str), Some("./"));
        assert_eq!(
            obj.get("display").and_then(Json::as_str),
            Some("standalone")
        );
        let icons = obj.get("icons").and_then(Json::as_arr).unwrap();
        assert_eq!(icons.len(), 3, "app {id} manifest must have 3 icons");

        check_png_ihdr(&app_dir.join("icon-192.png"), 192, 192);
        check_png_ihdr(&app_dir.join("icon-512.png"), 512, 512);
        check_png_ihdr(&app_dir.join("icon-maskable-512.png"), 512, 512);

        let sw_text = std::fs::read_to_string(app_dir.join("sw.js")).unwrap();
        assert!(sw_text.contains(&format!("monolith-{id}-")));
        assert!(
            !sw_text.contains("caches.match("),
            "{id}/sw.js must not search every cache on the origin"
        );
        assert!(sw_text.contains("key.startsWith(CACHE_PREFIX)"));
        assert!(sw_text.contains("key.startsWith(\"monolith-core-\")"));
        assert!(
            !sw_text.contains("athena"),
            "{id}/sw.js must not touch athena caches"
        );
        core_names.insert(extract_sw_string(&sw_text, "const CORE_CACHE = "));
        let core_assets = extract_sw_assets(&sw_text, "const CORE_ASSETS = ");
        assert!(core_assets.contains(&String::from("../site.wasm")));
        assert!(core_assets.contains(&String::from("../titan.elf")));
        let assets = extract_sw_assets(&sw_text, "const ASSETS = ");
        assert!(!assets.is_empty());
        for asset in core_assets.iter().chain(assets.iter()) {
            let resolved = app_dir.join(asset);
            assert!(
                resolved.is_file(),
                "{id}/sw.js references missing file `{asset}` ({})",
                resolved.display()
            );
        }
    }

    assert_eq!(
        core_names.len(),
        1,
        "every app must share one core cache: {core_names:?}"
    );

    let launcher_dir = tmp.join("apps");
    let launcher_html = std::fs::read_to_string(launcher_dir.join("index.html")).unwrap();
    let systems = extract_attr_values(&launcher_html, "data-system");
    assert_eq!(systems.len(), 25, "launcher must list all 25 systems");

    for href in extract_attr_values(&launcher_html, "href") {
        if href.starts_with('#') {
            continue;
        }
        if href.ends_with(".css") || href == "../" {
            let target = launcher_dir.join(&href);
            assert!(target.exists(), "broken launcher href {href}");
            continue;
        }
        let target = launcher_dir.join(&href);
        assert!(
            target.is_dir() && target.join("index.html").is_file(),
            "launcher links to missing app folder `{href}`"
        );
    }

    for src in extract_attr_values(&launcher_html, "src") {
        let target = launcher_dir.join(&src);
        assert!(target.is_file(), "launcher references missing icon `{src}`");
        check_png_ihdr(&target, 192, 192);
    }

    for soon in ["midas", "zeus"] {
        assert!(
            !launcher_html.contains(&format!("href=\"../{soon}/\"")),
            "coming-soon system `{soon}` must not be a link"
        );
    }
    assert!(launcher_html.contains("href=\"../athena/\""));
    assert!(launcher_html.contains("coming soon"));

    let site_html = std::fs::read_to_string(tmp.join("index.html")).unwrap();
    assert!(site_html.contains("href=\"apps/\""));
    let site_js = std::fs::read_to_string(tmp.join("site.js")).unwrap();
    assert!(site_js.contains("Open app"));

    let mut all_files = Vec::new();
    collect_files(&tmp, &mut all_files);
    for path in all_files {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !matches!(ext, "html" | "js" | "css" | "webmanifest") {
            continue;
        }
        let content = std::fs::read_to_string(&path).unwrap();
        for forbidden in ["http://", "https://", "alert(", "confirm(", "prompt("] {
            assert!(
                !content.contains(forbidden),
                "file {} contains forbidden `{forbidden}`",
                path.display()
            );
        }
    }
}

#[test]
fn all_second_and_error_examples_match_expected_outcomes() {
    let second_examples: [(&str, &str); 21] = [
        (
            "iridium",
            "CREATE TABLE rocks (id INT PRIMARY KEY, kind TEXT, mass INT);\n\
             INSERT INTO rocks VALUES (1, 'basalt', 40), (2, 'granite', 65), (3, 'shale', 25), (4, 'slate', 50);\n\
             DELETE FROM rocks WHERE mass < 30;\n\
             SELECT id, kind, mass FROM rocks WHERE mass >= 40 ORDER BY mass ASC;",
        ),
        (
            "elysium",
            "fn print(s: str) uses io = extern \"io.print\"\n\n\
             fn fib(n: int) -> int {\n\
               var a = 0\n\
               var b = 1\n\
               for i in 0..n {\n\
                 let next = a + b\n\
                 a = b\n\
                 b = next\n\
               }\n\
               a\n\
             }\n\n\
             fn main() uses io {\n\
               print(\"fib(10) = \" + str(fib(10)) + \"\\n\")\n\
             }",
        ),
        (
            "mercury",
            "GET /status?verbose=1 HTTP/1.0\nHost: monolith.local\nUser-Agent: Monolith/0.1\n\n",
        ),
        (
            "helios",
            "ticks 45\n\
             fn g_frame() -> int = extern \"game.frame\"\n\
             fn g_rect(x: int, y: int, w: int, h: int, c: int) = extern \"game.rect\"\n\n\
             fn update() {\n\
               let f = g_frame()\n\
               g_rect(0, 0, 320, 240, 0x111827)\n\
               g_rect(20, 200, 280, 10, 0x475569)\n\
               let bx = 24 + (f * 5) % 248\n\
               let by = 176 - ((f * 7) % 120)\n\
               g_rect(bx, by, 24, 24, 0xf59e0b)\n\
               g_rect(bx + 4, by + 4, 8, 8, 0xfef3c7)\n\
             }",
        ),
        (
            "cobalt",
            "fn print(s: str) uses io = extern \"io.print\"\n\n\
             fn clamp(v: int, lo: int, hi: int) -> int {\n\
               if v < lo { lo } else if v > hi { hi } else { v }\n\
             }\n\n\
             fn main() uses io {\n\
               print(\"clamped = \" + str(clamp(42, 0, 10)) + \"\\n\")\n\
             }",
        ),
        (
            "odyssey",
            "<style>\n\
               body { background: #0f172a; color: #e2e8f0; padding: 16px; }\n\
               .box { background: #1e293b; border: 3px solid #38bdf8; color: #f8fafc; padding: 12px; }\n\
             </style>\n\
             <div class=\"box\">\n\
               <h2>Strata Render</h2>\n\
               <p>Cascade, block boxes, and bitmap text rendered without a GPU.</p>\n\
             </div>",
        ),
        (
            "aurum",
            "bob alice 200\ncarol bob 150\nalice carol 50",
        ),
        (
            "sphinx",
            "Monolith zero-dependency cryptography: SHA-256, ChaCha20, Ed25519.",
        ),
        (
            "silicon",
            "    li a0, 0\n    li t0, 1\n    li t1, 11\nsum_loop:\n    add a0, a0, t0\n    addi t0, t0, 1\n    blt t0, t1, sum_loop\n    li t2, 0x00100000\n    li t3, 0x5555\n    sw t3, 0(t2)\nhalt:\n    j halt",
        ),
        (
            "chisel",
            "entry:\n    la t0, banner\n    li t1, 0\nspin:\n    lbu t2, 0(t0)\n    beqz t2, exit\n    addi t0, t0, 1\n    addi t1, t1, 1\n    j spin\nexit:\n    ebreak\nbanner:\n    .asciz \"chisel\"",
        ),
        (
            "hades",
            "mkdir /var\nmkdir /var/log\nwrite /var/log/boot.txt stage1\nwrite /var/log/boot.txt stage2 ready\ncat /var/log/boot.txt\nmkdir /var/tmp\nrmdir /var/tmp\nls /var",
        ),
        (
            "pandora",
            "rootfs /srv/containers/worker\nhostname worker-01\nmemory 1G\npids max\ncmd /usr/bin/worker --port 8080",
        ),
        (
            "argo",
            "[index]\nweb 1.0.0 : http ^1.0; codec ^1.0\nhttp 1.2.0 : core ^1.0\ncodec 1.1.0 : core ^1.1\ncore 1.0.0\ncore 1.2.0\n\n[manifest]\n[package]\nname = \"service\"\nversion = \"0.2.0\"\n\n[dependencies]\nweb = \"^1.0\"",
        ),
        (
            "krypton",
            "A=3; cat < input.txt | grep monolith > out.txt && echo ~/done $((A + 4))",
        ),
        (
            "iris",
            "open files / 260 190\nopen terminal 300 180\nmove 2 290 70\ntext ls /\nkey enter\nwait 200",
        ),
        (
            "indium",
            "row\n  column\n    text \"Sidebar\"\n    list 4\n  column\n    text \"Inspector\"\n    button \"Apply\"\n    button \"Reset\"\nfocus 4",
        ),
        (
            "calliope",
            "0x[0-9a-fA-F]+\nconst UART = 0x10000000\nconst MAGIC = 0xdeadBEEF\nlet bad = 1234",
        ),
        (
            "chronos",
            "write a.txt v1\ncommit first\nwrite a.txt v2\nwrite b.txt sidecar\ncommit second\nlog",
        ),
        (
            "xenon",
            "loss 45\ndelay 80\nsend packet one\nsend packet two",
        ),
        ("lithium", "50000"),
        (
            "kairos",
            "cpus 1\ntask heavy 8 16\ntask medium 2 8\ntask light 1 4",
        ),
    ];

    for (id, input) in second_examples {
        let demo = site::demos::find(id).unwrap();
        let res = (demo.run)(input);
        assert!(res.is_ok(), "second example for {id} failed: {res:?}");
    }

    let oversized = "x".repeat(65537);
    let error_examples: [(&str, &str); 21] = [
        ("iridium", "SELECT * FROM missing_table;"),
        ("elysium", "fn main() {\n  let bad: int = \"wrong\"\n}"),
        ("mercury", "BAD_REQUEST_LINE\nHost: monolith.local\n\n"),
        ("helios", "ticks 10\nfn main() -> int { 42 }"),
        ("cobalt", "fn main( {\n  let x = 1\n}"),
        ("odyssey", ""),
        ("aurum", "alice mallory 50"),
        ("sphinx", oversized.as_str()),
        ("silicon", "    bogus_op a0, a1, a2"),
        ("chisel", "    addi x99, zero, 1"),
        ("hades", "cat /missing/file.txt"),
        ("pandora", "rootfs relative/rootfs\nhostname bad\ncmd /bin/sh"),
        (
            "argo",
            "[index]\nalpha 1.0.0 : beta ^1.0\nbeta 1.0.0\nbeta 2.0.0\n\n[manifest]\n[package]\nname = \"broken\"\nversion = \"0.1.0\"\n\n[dependencies]\nalpha = \"^1.0\"\nbeta = \"^2.0\"",
        ),
        ("krypton", "echo \"unterminated"),
        ("iris", "open terminal\nwarp 99"),
        ("indium", "column\n  button \"Only\"\nfocus 5"),
        ("calliope", "[A-Z\nsample text"),
        ("chronos", "checkout nonexistent"),
        ("xenon", "loss 100\ndelay 10\nsend unreachable"),
        ("lithium", "0"),
        ("kairos", "cpus 99\ntask bad 1 4"),
    ];

    for (id, input) in error_examples {
        let demo = site::demos::find(id).unwrap();
        let res = (demo.run)(input);
        assert!(
            res.is_err(),
            "error example for {id} unexpectedly succeeded"
        );
    }
}
