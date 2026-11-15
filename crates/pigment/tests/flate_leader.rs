//! Leader check for T59: Python zlib and Pillow as the oracle. zlib streams (all levels, all
//! strategies, small windows) inflated by us; our deflate/zlib output inflated by zlib; size vs
//! zlib level 6; 240 hand-encoded PNGs (every colour type, bit depth, filter, Adam7, tRNS, split
//! IDAT) and Pillow-saved PNGs decoded pixel for pixel; our PNGs opened by Pillow; mutation fuzz.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use pigment::{
    decode_png, deflate, encode_png, inflate, zlib_compress, zlib_decompress, Canvas, FlateError,
    Rgba,
};
use std::path::{Path, PathBuf};
use std::process::Command;

const PY: &str = r#"
import sys, os, zlib, struct, random
mode, D = sys.argv[1], sys.argv[2]
def P(n): return os.path.join(D, n)
def chunk(t, b): return struct.pack(">I", len(b)) + t + b + struct.pack(">I", zlib.crc32(t + b) & 0xffffffff)
ADAM = [(0,0,8,8),(4,0,8,8),(0,4,4,8),(2,0,4,4),(0,2,2,4),(1,0,2,2),(0,1,1,2)]
def paeth(a, b, c):
    p = a + b - c; pa = abs(p - a); pb = abs(p - b); pc = abs(p - c)
    return a if pa <= pb and pa <= pc else (b if pb <= pc else c)
def filt(f, line, prev, bpp):
    out = bytearray()
    for x in range(len(line)):
        a = line[x - bpp] if x >= bpp else 0; b = prev[x]; c = prev[x - bpp] if x >= bpp else 0
        out.append((line[x] - [0, a, b, (a + b) // 2, paeth(a, b, c)][f]) & 255)
    return bytes([f]) + bytes(out)
def pack(samples, d):
    if d == 8: return bytes(samples)
    if d == 16: return b"".join(struct.pack(">H", s) for s in samples)
    out = bytearray(); acc = 0; n = 0
    for s in samples:
        acc = (acc << d) | s; n += d
        if n == 8: out.append(acc); acc = 0; n = 0
    if n: out.append(acc << (8 - n))
    return bytes(out)
CH = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}
DEPTHS = {0: [1, 2, 4, 8, 16], 2: [8, 16], 3: [1, 2, 4, 8], 4: [8, 16], 6: [8, 16]}
def make_pngs(rnd):
    for n in range(240):
        ct = rnd.choice([0, 2, 3, 4, 6]); d = rnd.choice(DEPTHS[ct]); mx = (1 << d) - 1
        w = rnd.randrange(1, 34); h = rnd.randrange(1, 34); il = rnd.randrange(2)
        img = [[[rnd.randrange(mx + 1) for _ in range(CH[ct])] for _ in range(w)] for _ in range(h)]
        chunks = [chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, d, ct, 0, 0, il))]
        pal = None; trns = None
        if ct == 3:
            npal = rnd.randrange(1, mx + 2)
            pal = [(rnd.randrange(256), rnd.randrange(256), rnd.randrange(256)) for _ in range(npal)]
            for row in img:
                for px in row: px[0] %= npal
            chunks.append(chunk(b"PLTE", bytes(sum(pal, ()))))
            if rnd.randrange(2):
                trns = [rnd.randrange(256) for _ in range(rnd.randrange(1, npal + 1))]
                chunks.append(chunk(b"tRNS", bytes(trns)))
        elif ct in (0, 2) and rnd.randrange(2):
            trns = list(img[rnd.randrange(h)][rnd.randrange(w)])
            chunks.append(chunk(b"tRNS", b"".join(struct.pack(">H", s) for s in trns)))
        if rnd.randrange(3) == 0: chunks.append(chunk(b"tEXt", b"Comment\x00leader"))
        bpp = max(1, CH[ct] * d // 8); raw = bytearray()
        for (xs, ys, dx, dy) in (ADAM if il else [(0, 0, 1, 1)]):
            xsel = list(range(xs, w, dx)); ysel = list(range(ys, h, dy))
            if not xsel or not ysel: continue
            prev = None
            for y in ysel:
                line = pack([s for x in xsel for s in img[y][x]], d)
                if prev is None: prev = bytes(len(line))
                raw += filt(rnd.randrange(5), line, prev, bpp); prev = line
        z = zlib.compress(bytes(raw), rnd.randrange(10)); k = rnd.randrange(1, 4); step = len(z) // k + 1
        for j in range(0, len(z), step): chunks.append(chunk(b"IDAT", z[j:j + step]))
        def s8(v): return v >> 8 if d == 16 else (v * 255 // mx if d < 8 else v)
        out = bytearray()
        for row in img:
            for px in row:
                if ct == 0: g = s8(px[0]); out += bytes([g, g, g, 0 if trns and px[0] == trns[0] else 255])
                elif ct == 2: out += bytes([s8(px[0]), s8(px[1]), s8(px[2]), 0 if trns and px == trns else 255])
                elif ct == 3:
                    r, g, b = pal[px[0]]; out += bytes([r, g, b, trns[px[0]] if trns and px[0] < len(trns) else 255])
                elif ct == 4: g = s8(px[0]); out += bytes([g, g, g, s8(px[1])])
                else: out += bytes([s8(c) for c in px])
        open(P(f"png_{n}.png"), "wb").write(b"\x89PNG\r\n\x1a\n" + b"".join(chunks) + chunk(b"IEND", b""))
        open(P(f"png_{n}.rgba"), "wb").write(struct.pack(">II", w, h) + bytes(out))
    from PIL import Image
    for n in range(40):
        w = rnd.randrange(1, 60); h = rnd.randrange(1, 60); m = rnd.choice(["RGBA", "RGB", "L", "LA", "P"])
        im = Image.frombytes("RGBA", (w, h), bytes(rnd.randrange(256) for _ in range(w * h * 4))).convert(m)
        im.save(P(f"pil_{n}.png"), optimize=bool(n % 2))
        open(P(f"pil_{n}.rgba"), "wb").write(struct.pack(">II", w, h) + Image.open(P(f"pil_{n}.png")).convert("RGBA").tobytes())
if mode == "make":
    rnd = random.Random(59); i = 0
    S = [zlib.Z_DEFAULT_STRATEGY, zlib.Z_FILTERED, zlib.Z_HUFFMAN_ONLY, zlib.Z_RLE, zlib.Z_FIXED]
    while os.path.exists(P(f"in_{i}.bin")):
        data = open(P(f"in_{i}.bin"), "rb").read()
        for k in range(4):
            c = zlib.compressobj(rnd.randrange(10), zlib.DEFLATED, rnd.choice([-15, -12, -9]), rnd.randrange(1, 10), rnd.choice(S))
            open(P(f"z_{i}_{k}.bin"), "wb").write(c.compress(data) + c.flush())
        open(P(f"zl_{i}.bin"), "wb").write(zlib.compress(data, 6))
        c = zlib.compressobj(6, zlib.DEFLATED, -15)
        open(P(f"size6_{i}.txt"), "w").write(str(len(c.compress(data) + c.flush())))
        i += 1
    open(P("bomb.bin"), "wb").write(zlib.compress(b"\0" * (10 << 20), 9))
    make_pngs(rnd)
else:
    from PIL import Image
    bad = 0; n = 0
    for f in sorted(os.listdir(D)):
        ok = None
        if f.startswith("ours_") or f.startswith("ourz_"):
            data = open(P("in_" + f.split("_")[1].split(".")[0] + ".bin"), "rb").read()
            try: ok = zlib.decompress(open(P(f), "rb").read(), -15 if f.startswith("ours_") else 15) == data
            except Exception: ok = False
        elif f.startswith("ourpng_") and f.endswith(".png"):
            try:
                im = Image.open(P(f)); im.load()
                ok = im.mode == "RGBA" and im.tobytes() == open(P(f[:-4] + ".rgba"), "rb").read()
            except Exception: ok = False
        if ok is None: continue
        n += 1
        if not ok: bad += 1; print("BAD", f)
    print("CHECKED", n, "BAD", bad)
"#;

fn py(script: &Path, mode: &str, dir: &Path) -> String {
    let o = Command::new("python")
        .arg(script)
        .arg(mode)
        .arg(dir)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "python {mode} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn input(r: &mut Rng, i: usize) -> Vec<u8> {
    let words: &[&str] = &[
        "the ",
        "monolith ",
        "of ",
        "rock ",
        "and ",
        "deflate ",
        "window ",
        "huffman ",
        "\n",
        "x",
    ];
    match i % 3 {
        0 => (0..r.below(4000)).map(|_| r.below(256) as u8).collect(),
        1 => {
            let mut s = String::new();
            for _ in 0..r.below(6000) {
                s.push_str(r.pick(words));
            }
            s.into_bytes()
        }
        _ => {
            let block: Vec<u8> = (0..1000).map(|_| r.below(256) as u8).collect();
            let mut v = block.clone();
            v.extend((0..33000 + r.below(4000)).map(|_| r.below(256) as u8));
            v.extend(&block);
            v.extend(std::iter::repeat_n(r.below(256) as u8, r.below(70000)));
            v.extend(b"abcabcabd".repeat(r.below(3000)));
            v
        }
    }
}

fn bytes(c: &Canvas) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend(c.width.to_be_bytes());
    v.extend(c.height.to_be_bytes());
    for p in &c.pixels {
        v.extend([p.0, p.1, p.2, p.3]);
    }
    v
}

#[test]
fn leader_flate_and_png_against_python() {
    let d: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR")).join("t59");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let script = d.join("oracle.py");
    std::fs::write(&script, PY).unwrap();
    let mut r = Rng::new(0x59_2026_1115);
    let inputs: Vec<Vec<u8>> = (0..120).map(|i| input(&mut r, i)).collect();
    for (i, data) in inputs.iter().enumerate() {
        std::fs::write(d.join(format!("in_{i}.bin")), data).unwrap();
    }
    py(&script, "make", &d);
    let rd = |n: String| std::fs::read(d.join(n)).unwrap();
    let mut size_bad = Vec::new();
    for (i, data) in inputs.iter().enumerate() {
        for k in 0..4 {
            let z = rd(format!("z_{i}_{k}.bin"));
            assert_eq!(
                inflate(&z, data.len()).as_deref(),
                Ok(&data[..]),
                "inflate z_{i}_{k}"
            );
            if !data.is_empty() {
                assert_eq!(inflate(&z, data.len() - 1), Err(FlateError::TooLarge));
            }
        }
        let zl = rd(format!("zl_{i}.bin"));
        assert_eq!(
            zlib_decompress(&zl, data.len()).as_deref(),
            Ok(&data[..]),
            "zl_{i}"
        );
        let mut broken = zl.clone();
        *broken.last_mut().unwrap() ^= 1;
        assert_eq!(
            zlib_decompress(&broken, data.len()),
            Err(FlateError::Checksum)
        );
        for level in 0..=9u8 {
            let ours = deflate(data, level);
            assert_eq!(
                inflate(&ours, data.len()).as_deref(),
                Ok(&data[..]),
                "self round trip {i} level {level}"
            );
            std::fs::write(d.join(format!("ours_{i}_{level}.bin")), &ours).unwrap();
            if level == 6 && i % 3 == 1 {
                let zs: usize = String::from_utf8(rd(format!("size6_{i}.txt")))
                    .unwrap()
                    .parse()
                    .unwrap();
                if ours.len() * 10 > zs * 11 + 160 {
                    size_bad.push(format!("input {i}: ours {} zlib {zs}", ours.len()));
                }
            }
        }
        std::fs::write(d.join(format!("ourz_{i}.bin")), zlib_compress(data, 6)).unwrap();
    }
    assert!(
        size_bad.is_empty(),
        "level 6 over 10% larger than zlib: {size_bad:?}"
    );
    assert_eq!(
        zlib_decompress(&rd("bomb.bin".into()), 1 << 20),
        Err(FlateError::TooLarge)
    );
    let mut png_bad = Vec::new();
    let names: Vec<String> = (0..240)
        .map(|n| format!("png_{n}"))
        .chain((0..40).map(|n| format!("pil_{n}")))
        .collect();
    for name in &names {
        let png = rd(format!("{name}.png"));
        let want = rd(format!("{name}.rgba"));
        match std::panic::catch_unwind(|| decode_png(&png, 1 << 20)).map_err(|_| "PANIC") {
            Ok(Ok(c)) if bytes(&c) == want => {}
            other => png_bad.push(format!(
                "{name}: IHDR {:?} -> {:?}",
                &png[16..29],
                other.map(|r| r.map(|c| (c.width, c.height)))
            )),
        }
    }
    assert!(
        png_bad.is_empty(),
        "{} of {} PNGs wrong:\n{}",
        png_bad.len(),
        names.len(),
        png_bad[..png_bad.len().min(10)].join("\n")
    );
    for n in 0..40 {
        let (w, h) = (1 + r.below(70) as u32, 1 + r.below(50) as u32);
        let mut c = Canvas::new(w, h, Rgba(0, 0, 0, 0));
        let smooth = n % 2 == 0;
        for (j, p) in c.pixels.iter_mut().enumerate() {
            *p = if smooth {
                let (x, y) = (j as u32 % w, j as u32 / w);
                Rgba((x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255)
            } else {
                Rgba(
                    r.below(256) as u8,
                    r.below(256) as u8,
                    r.below(256) as u8,
                    r.below(256) as u8,
                )
            };
        }
        let png = encode_png(&c);
        assert_eq!(decode_png(&png, 1 << 20).as_ref(), Ok(&c));
        std::fs::write(d.join(format!("ourpng_{n}.png")), &png).unwrap();
        std::fs::write(d.join(format!("ourpng_{n}.rgba")), &bytes(&c)[8..]).unwrap();
    }
    let out = py(&script, "verify", &d);
    assert!(
        out.contains(&format!("CHECKED {} BAD 0", 120 * 10 + 120 + 40)),
        "{out}"
    );
    // Mutation fuzz: must return, never panic.
    let mut fuzz_panics = Vec::new();
    for name in names.iter().take(80) {
        let png = rd(format!("{name}.png"));
        for _ in 0..40 {
            let mut m = png.clone();
            match r.below(3) {
                0 => {
                    let i = r.below(m.len());
                    m[i] ^= 1 << r.below(8);
                }
                1 => m.truncate(r.below(m.len())),
                _ => {
                    let i = 8 + r.below(m.len() - 8);
                    m[i] = r.below(256) as u8;
                }
            }
            if std::panic::catch_unwind(|| decode_png(&m, 1 << 16)).is_err() {
                fuzz_panics.push(format!("{name} mutated: {:?}", &m[..m.len().min(40)]));
            }
        }
    }
    for i in 0..60 {
        let z = rd(format!("z_{i}_0.bin"));
        for _ in 0..40 {
            let mut m = z.clone();
            if !m.is_empty() {
                let j = r.below(m.len());
                m[j] ^= 1 << r.below(8);
            }
            if std::panic::catch_unwind(|| inflate(&m, 1 << 20)).is_err() {
                fuzz_panics.push(format!("inflate z_{i}_0 mutated"));
            }
        }
    }
    assert!(
        fuzz_panics.is_empty(),
        "{} panics, first: {:?}",
        fuzz_panics.len(),
        fuzz_panics.first()
    );
}
