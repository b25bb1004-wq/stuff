//! RFC 1951 DEFLATE compressor (stored, fixed, and dynamic Huffman with LZ77).

use alloc::vec;
use alloc::vec::Vec;

use super::huffman::{
    build_code_lengths, canonical_codes, BL_ORDER, DIST_EXTRA, FIXED_DIST_LENS, FIXED_LIT_LENS,
    LEN_EXTRA, MAX_BITS,
};

/// LSB-first bitstream writer.
struct BitWriter {
    out: Vec<u8>,
    buf: u64,
    cnt: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            buf: 0,
            cnt: 0,
        }
    }

    #[inline]
    fn write_bits(&mut self, val: u32, n: u8) {
        self.buf |= u64::from(val) << self.cnt;
        self.cnt += n;
        while self.cnt >= 8 {
            self.out.push((self.buf & 0xFF) as u8);
            self.buf >>= 8;
            self.cnt -= 8;
        }
    }

    fn align_byte(&mut self) {
        if self.cnt > 0 {
            self.out.push((self.buf & 0xFF) as u8);
            self.buf = 0;
            self.cnt = 0;
        }
    }

    fn write_stored_block(&mut self, data: &[u8], is_final: bool) {
        self.write_bits(u32::from(is_final), 1);
        self.write_bits(0, 2);
        self.align_byte();
        let len = data.len() as u16;
        self.out.extend_from_slice(&len.to_le_bytes());
        self.out.extend_from_slice(&(!len).to_le_bytes());
        self.out.extend_from_slice(data);
    }
}

#[derive(Clone, Copy)]
enum Token {
    Lit(u8),
    Match { len: u16, dist: u16 },
}

#[derive(Clone, Copy)]
struct LevelParams {
    good_len: usize,
    max_lazy: usize,
    nice_len: usize,
    max_chain: usize,
    lazy: bool,
}

fn level_params(level: u8) -> LevelParams {
    match level {
        1 => LevelParams {
            good_len: 4,
            max_lazy: 4,
            nice_len: 8,
            max_chain: 4,
            lazy: false,
        },
        2 => LevelParams {
            good_len: 4,
            max_lazy: 6,
            nice_len: 16,
            max_chain: 8,
            lazy: false,
        },
        3 => LevelParams {
            good_len: 4,
            max_lazy: 12,
            nice_len: 32,
            max_chain: 16,
            lazy: false,
        },
        4 => LevelParams {
            good_len: 4,
            max_lazy: 4,
            nice_len: 16,
            max_chain: 16,
            lazy: true,
        },
        5 => LevelParams {
            good_len: 8,
            max_lazy: 16,
            nice_len: 32,
            max_chain: 32,
            lazy: true,
        },
        6 => LevelParams {
            good_len: 8,
            max_lazy: 16,
            nice_len: 128,
            max_chain: 128,
            lazy: true,
        },
        7 => LevelParams {
            good_len: 8,
            max_lazy: 32,
            nice_len: 128,
            max_chain: 256,
            lazy: true,
        },
        8 => LevelParams {
            good_len: 32,
            max_lazy: 128,
            nice_len: 258,
            max_chain: 1024,
            lazy: true,
        },
        _ => LevelParams {
            good_len: 32,
            max_lazy: 258,
            nice_len: 258,
            max_chain: 4096,
            lazy: true,
        },
    }
}

#[inline]
fn len_code(len: u16) -> usize {
    if len == 258 {
        return 28;
    }
    let l = len - 3;
    if l < 8 {
        return usize::from(l);
    }
    let msb = 15 - l.leading_zeros() as usize;
    4 * (msb - 1) + ((usize::from(l) >> (msb - 2)) & 3)
}

#[inline]
fn dist_code(dist: u16) -> usize {
    let d = dist - 1;
    if d < 4 {
        return usize::from(d);
    }
    let msb = 15 - d.leading_zeros() as usize;
    2 * msb + ((usize::from(d) >> (msb - 1)) & 1)
}

#[inline]
fn hash3(data: &[u8], pos: usize) -> usize {
    ((usize::from(data[pos]) << 10)
        ^ (usize::from(data[pos + 1]) << 5)
        ^ usize::from(data[pos + 2]))
        & 0x7FFF
}

struct Matcher {
    head: Vec<usize>,
    prev: Vec<usize>,
}

impl Matcher {
    fn new() -> Self {
        Self {
            head: vec![0; 32768],
            prev: vec![0; 32768],
        }
    }

    #[inline]
    fn insert(&mut self, data: &[u8], pos: usize) -> usize {
        if pos + 2 >= data.len() {
            return 0;
        }
        let h = hash3(data, pos);
        let old = self.head[h];
        self.prev[pos & 0x7FFF] = old;
        self.head[h] = pos + 1;
        old
    }

    fn find_match(
        &self,
        data: &[u8],
        pos: usize,
        mut cur_p1: usize,
        params: LevelParams,
        prev_best: usize,
    ) -> (u16, u16) {
        let max_len = (data.len() - pos).min(258);
        if max_len < 3 {
            return (0, 0);
        }
        let chain_limit = if prev_best >= params.good_len {
            params.max_chain >> 2
        } else {
            params.max_chain
        };
        let mut best_len = 2usize;
        let mut best_dist = 0usize;
        let mut steps = 0usize;
        let p_slice = &data[pos..pos + max_len];

        while cur_p1 != 0 && steps < chain_limit {
            let cur = cur_p1 - 1;
            let dist = pos - cur;
            if dist == 0 || dist > 32768 {
                break;
            }
            let c_slice = &data[cur..cur + max_len];
            if c_slice[best_len] == p_slice[best_len]
                && c_slice[0] == p_slice[0]
                && c_slice[1] == p_slice[1]
                && c_slice[2] == p_slice[2]
            {
                let mut k = 3usize;
                while k < max_len && c_slice[k] == p_slice[k] {
                    k += 1;
                }
                if k > best_len {
                    best_len = k;
                    best_dist = dist;
                    if best_len >= params.nice_len || best_len == max_len {
                        break;
                    }
                }
            }
            cur_p1 = self.prev[cur & 0x7FFF];
            steps += 1;
        }

        if best_len < 3 || (best_len == 3 && best_dist > 4096) {
            return (0, 0);
        }
        (best_len as u16, best_dist as u16)
    }
}

/// Raw DEFLATE. `level` 0 = stored blocks only; 1..=9 = LZ77 with dynamic/fixed/stored blocks.
pub fn deflate(data: &[u8], level: u8) -> Vec<u8> {
    let mut bw = BitWriter::new();
    if level == 0 {
        if data.is_empty() {
            bw.write_stored_block(&[], true);
        } else {
            let chunks = data.chunks(65535);
            let total = chunks.len();
            for (idx, chunk) in chunks.enumerate() {
                bw.write_stored_block(chunk, idx + 1 == total);
            }
        }
        return bw.out;
    }

    if data.is_empty() {
        bw.write_bits(1, 1);
        bw.write_bits(1, 2);
        bw.write_bits(0, 7);
        bw.align_byte();
        return bw.out;
    }

    let params = level_params(level.min(9));
    let mut matcher = Matcher::new();
    let mut tokens: Vec<Token> = Vec::with_capacity(16384);
    let mut block_start = 0usize;
    let mut pos = 0usize;
    let mut prev_match: Option<(u16, u16)> = None;

    while pos < data.len() {
        let old = matcher.insert(data, pos);
        let m = matcher.find_match(
            data,
            pos,
            old,
            params,
            prev_match.map_or(0, |p| usize::from(p.0)),
        );

        if let Some((p_len, p_dist)) = prev_match {
            if usize::from(m.0) > usize::from(p_len) {
                tokens.push(Token::Lit(data[pos - 1]));
                if usize::from(m.0) < params.max_lazy && pos + 3 < data.len() {
                    prev_match = Some(m);
                    pos += 1;
                } else {
                    tokens.push(Token::Match {
                        len: m.0,
                        dist: m.1,
                    });
                    let end = pos + usize::from(m.0);
                    for p in (pos + 1)..end {
                        matcher.insert(data, p);
                    }
                    pos = end;
                    prev_match = None;
                }
            } else {
                tokens.push(Token::Match {
                    len: p_len,
                    dist: p_dist,
                });
                let end = (pos - 1) + usize::from(p_len);
                for p in (pos + 1)..end {
                    matcher.insert(data, p);
                }
                pos = end;
                prev_match = None;
            }
        } else if m.0 >= 3 {
            if params.lazy && usize::from(m.0) < params.max_lazy && pos + 3 < data.len() {
                prev_match = Some(m);
                pos += 1;
            } else {
                tokens.push(Token::Match {
                    len: m.0,
                    dist: m.1,
                });
                let end = pos + usize::from(m.0);
                for p in (pos + 1)..end {
                    matcher.insert(data, p);
                }
                pos = end;
            }
        } else {
            tokens.push(Token::Lit(data[pos]));
            pos += 1;
        }

        if prev_match.is_none() && (pos - block_start >= 32768 || tokens.len() >= 16384) {
            let is_final = pos >= data.len();
            flush_block(&mut bw, &data[block_start..pos], &tokens, is_final);
            tokens.clear();
            block_start = pos;
        }
    }

    if let Some((p_len, p_dist)) = prev_match {
        tokens.push(Token::Match {
            len: p_len,
            dist: p_dist,
        });
    }
    if block_start < data.len() || !tokens.is_empty() {
        flush_block(&mut bw, &data[block_start..], &tokens, true);
    }

    bw.align_byte();
    bw.out
}

struct RleCode {
    sym: u8,
    extra: u8,
    extra_bits: u8,
}

fn rle_encode_lengths(lens: &[u8]) -> (Vec<RleCode>, [u32; 19]) {
    let mut out = Vec::with_capacity(lens.len());
    let mut freqs = [0u32; 19];
    let mut i = 0usize;
    while i < lens.len() {
        let val = lens[i];
        let mut run = 1usize;
        while i + run < lens.len() && lens[i + run] == val {
            run += 1;
        }
        i += run;

        if val == 0 {
            while run >= 11 {
                let cnt = run.min(138);
                out.push(RleCode {
                    sym: 18,
                    extra: (cnt - 11) as u8,
                    extra_bits: 7,
                });
                freqs[18] += 1;
                run -= cnt;
            }
            if run >= 3 {
                out.push(RleCode {
                    sym: 17,
                    extra: (run - 3) as u8,
                    extra_bits: 3,
                });
                freqs[17] += 1;
                run = 0;
            }
            while run > 0 {
                out.push(RleCode {
                    sym: 0,
                    extra: 0,
                    extra_bits: 0,
                });
                freqs[0] += 1;
                run -= 1;
            }
        } else {
            out.push(RleCode {
                sym: val,
                extra: 0,
                extra_bits: 0,
            });
            freqs[usize::from(val)] += 1;
            run -= 1;
            while run >= 3 {
                let cnt = run.min(6);
                out.push(RleCode {
                    sym: 16,
                    extra: (cnt - 3) as u8,
                    extra_bits: 2,
                });
                freqs[16] += 1;
                run -= cnt;
            }
            while run > 0 {
                out.push(RleCode {
                    sym: val,
                    extra: 0,
                    extra_bits: 0,
                });
                freqs[usize::from(val)] += 1;
                run -= 1;
            }
        }
    }
    (out, freqs)
}

fn flush_block(bw: &mut BitWriter, raw: &[u8], tokens: &[Token], is_final: bool) {
    let mut lit_freqs = [0u32; 286];
    let mut dist_freqs = [0u32; 30];
    let mut extra_bits_total = 0u64;

    for &tok in tokens {
        match tok {
            Token::Lit(b) => lit_freqs[usize::from(b)] += 1,
            Token::Match { len, dist } => {
                let lidx = len_code(len);
                let didx = dist_code(dist);
                lit_freqs[257 + lidx] += 1;
                dist_freqs[didx] += 1;
                extra_bits_total += u64::from(LEN_EXTRA[lidx]) + u64::from(DIST_EXTRA[didx]);
            }
        }
    }
    lit_freqs[256] += 1;

    let fixed_bits: u64 = 3
        + lit_freqs
            .iter()
            .enumerate()
            .map(|(i, &f)| u64::from(f) * u64::from(FIXED_LIT_LENS[i]))
            .sum::<u64>()
        + dist_freqs
            .iter()
            .enumerate()
            .map(|(i, &f)| u64::from(f) * u64::from(FIXED_DIST_LENS[i]))
            .sum::<u64>()
        + extra_bits_total;

    let stored_bits: u64 = if raw.len() <= 65535 {
        let pad = u64::from((8 - ((bw.cnt + 3) & 7)) & 7);
        3 + pad + 32 + (raw.len() as u64) * 8
    } else {
        u64::MAX
    };

    let lit_lens = build_code_lengths(&lit_freqs, MAX_BITS, true);
    let dist_lens = build_code_lengths(&dist_freqs, MAX_BITS, false);
    let hlit = lit_lens
        .iter()
        .rposition(|&l| l > 0)
        .map_or(257, |p| (p + 1).max(257));
    let hdist = dist_lens
        .iter()
        .rposition(|&l| l > 0)
        .map_or(1, |p| (p + 1).max(1));

    let mut combined = Vec::with_capacity(hlit + hdist);
    combined.extend_from_slice(&lit_lens[..hlit]);
    combined.extend_from_slice(&dist_lens[..hdist]);
    let (rle, bl_freqs) = rle_encode_lengths(&combined);
    let bl_lens = build_code_lengths(&bl_freqs, 7, true);
    let hclen = BL_ORDER
        .iter()
        .rposition(|&idx| bl_lens[idx] > 0)
        .map_or(4, |p| (p + 1).max(4));

    let dyn_header_bits: u64 = 3
        + 5
        + 5
        + 4
        + (hclen as u64) * 3
        + rle
            .iter()
            .map(|r| u64::from(bl_lens[usize::from(r.sym)]) + u64::from(r.extra_bits))
            .sum::<u64>();
    let dyn_data_bits: u64 = lit_freqs
        .iter()
        .enumerate()
        .map(|(i, &f)| u64::from(f) * u64::from(lit_lens[i]))
        .sum::<u64>()
        + dist_freqs
            .iter()
            .enumerate()
            .map(|(i, &f)| u64::from(f) * u64::from(dist_lens[i]))
            .sum::<u64>()
        + extra_bits_total;
    let dyn_bits = dyn_header_bits + dyn_data_bits;

    if stored_bits <= fixed_bits && stored_bits <= dyn_bits {
        bw.write_stored_block(raw, is_final);
        return;
    }

    bw.write_bits(u32::from(is_final), 1);
    if fixed_bits <= dyn_bits {
        bw.write_bits(1, 2);
        let lit_codes = canonical_codes(&FIXED_LIT_LENS);
        let dist_codes = canonical_codes(&FIXED_DIST_LENS);
        write_tokens(
            bw,
            tokens,
            &lit_codes,
            &FIXED_LIT_LENS,
            &dist_codes,
            &FIXED_DIST_LENS,
        );
    } else {
        bw.write_bits(2, 2);
        bw.write_bits((hlit - 257) as u32, 5);
        bw.write_bits((hdist - 1) as u32, 5);
        bw.write_bits((hclen - 4) as u32, 4);
        for &idx in &BL_ORDER[..hclen] {
            bw.write_bits(u32::from(bl_lens[idx]), 3);
        }
        let bl_codes = canonical_codes(&bl_lens);
        for r in &rle {
            let s = usize::from(r.sym);
            bw.write_bits(u32::from(bl_codes[s]), bl_lens[s]);
            if r.extra_bits > 0 {
                bw.write_bits(u32::from(r.extra), r.extra_bits);
            }
        }
        let lit_codes = canonical_codes(&lit_lens);
        let dist_codes = canonical_codes(&dist_lens);
        write_tokens(bw, tokens, &lit_codes, &lit_lens, &dist_codes, &dist_lens);
    }
}

fn write_tokens(
    bw: &mut BitWriter,
    tokens: &[Token],
    lit_codes: &[u16],
    lit_lens: &[u8],
    dist_codes: &[u16],
    dist_lens: &[u8],
) {
    use super::huffman::{DIST_BASE, LEN_BASE};
    for &tok in tokens {
        match tok {
            Token::Lit(b) => {
                let s = usize::from(b);
                bw.write_bits(u32::from(lit_codes[s]), lit_lens[s]);
            }
            Token::Match { len, dist } => {
                let lidx = len_code(len);
                let lsym = 257 + lidx;
                bw.write_bits(u32::from(lit_codes[lsym]), lit_lens[lsym]);
                let lextra = LEN_EXTRA[lidx];
                if lextra > 0 {
                    bw.write_bits(u32::from(len - LEN_BASE[lidx]), lextra);
                }

                let didx = dist_code(dist);
                bw.write_bits(u32::from(dist_codes[didx]), dist_lens[didx]);
                let dextra = DIST_EXTRA[didx];
                if dextra > 0 {
                    bw.write_bits(u32::from(dist - DIST_BASE[didx]), dextra);
                }
            }
        }
    }
    bw.write_bits(u32::from(lit_codes[256]), lit_lens[256]);
}
