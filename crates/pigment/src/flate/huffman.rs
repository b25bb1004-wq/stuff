//! Canonical Huffman tables, RFC 1951 code tables, and Package-Merge tree builder.

use alloc::vec;
use alloc::vec::Vec;

use super::FlateError;

/// Maximum Huffman code length in DEFLATE.
pub(crate) const MAX_BITS: usize = 15;

/// Alphabet order for dynamic block code-length codes.
pub(crate) const BL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Base lengths for symbols 257..=285.
pub(crate) const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

/// Extra bits for length symbols 257..=285.
pub(crate) const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// Base distances for symbols 0..=29.
pub(crate) const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

/// Extra bits for distance symbols 0..=29.
pub(crate) const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Fixed Huffman literal/length bit lengths (RFC 1951 section 3.2.6).
pub(crate) const FIXED_LIT_LENS: [u8; 288] = {
    let mut a = [0u8; 288];
    let mut i = 0;
    while i <= 143 {
        a[i] = 8;
        i += 1;
    }
    while i <= 255 {
        a[i] = 9;
        i += 1;
    }
    while i <= 279 {
        a[i] = 7;
        i += 1;
    }
    while i <= 287 {
        a[i] = 8;
        i += 1;
    }
    a
};

/// Fixed Huffman distance bit lengths (RFC 1951 section 3.2.6).
pub(crate) const FIXED_DIST_LENS: [u8; 32] = [5u8; 32];

/// Reverse the lowest `len` bits of `code`.
#[inline]
pub(crate) fn reverse_bits(code: u16, len: u8) -> u16 {
    code.reverse_bits() >> (16 - len)
}

/// Canonical Huffman decoder with a 9-bit fast lookup table.
pub(crate) struct HuffDecoder {
    pub(crate) fast: [u32; 512],
    pub(crate) count: [u16; MAX_BITS + 1],
    pub(crate) first_code: [u16; MAX_BITS + 1],
    pub(crate) first_sym: [u16; MAX_BITS + 1],
    pub(crate) syms: [u16; 288],
    pub(crate) max_len: u8,
}

impl HuffDecoder {
    /// Build a canonical Huffman decoder from `lengths`.
    pub(crate) fn build(lengths: &[u8], allow_sparse_dist: bool) -> Result<Self, FlateError> {
        if lengths.len() > 288 {
            return Err(FlateError::Invalid);
        }
        let mut count = [0u16; MAX_BITS + 1];
        let mut max_len = 0u8;
        let mut total_nonzero = 0u16;
        for &len in lengths {
            if usize::from(len) > MAX_BITS {
                return Err(FlateError::Invalid);
            }
            if len > 0 {
                count[usize::from(len)] += 1;
                total_nonzero += 1;
                if len > max_len {
                    max_len = len;
                }
            }
        }

        if max_len == 0 {
            if allow_sparse_dist {
                return Ok(Self {
                    fast: [0; 512],
                    count,
                    first_code: [0; MAX_BITS + 1],
                    first_sym: [0; MAX_BITS + 1],
                    syms: [0; 288],
                    max_len: 0,
                });
            }
            return Err(FlateError::Invalid);
        }

        let mut left = 1i32;
        for &c in count.iter().skip(1) {
            left = (left << 1) - i32::from(c);
            if left < 0 {
                return Err(FlateError::Invalid);
            }
        }
        if left > 0 && !(allow_sparse_dist && total_nonzero == 1 && max_len == 1) {
            return Err(FlateError::Invalid);
        }

        let mut next_code = [0u16; MAX_BITS + 1];
        let mut first_code = [0u16; MAX_BITS + 1];
        let mut first_sym = [0u16; MAX_BITS + 1];
        let mut code = 0u16;
        let mut sym_off = 0u16;
        for bits in 1..=MAX_BITS {
            code = (code + count[bits - 1]) << 1;
            first_code[bits] = code;
            next_code[bits] = code;
            first_sym[bits] = sym_off;
            sym_off += count[bits];
        }

        let mut offs = first_sym;
        let mut syms = [0u16; 288];
        let mut fast = [0u32; 512];
        for (sym, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let b = usize::from(len);
            let dst = usize::from(offs[b]);
            if dst >= syms.len() {
                return Err(FlateError::Invalid);
            }
            syms[dst] = sym as u16;
            offs[b] += 1;

            let c = next_code[b];
            next_code[b] = c + 1;
            if b <= 9 {
                let rev = reverse_bits(c, len) as usize;
                let step = 1usize << b;
                let entry = (u32::from(len) << 16) | (sym as u32);
                let mut idx = rev;
                while idx < 512 {
                    fast[idx] = entry;
                    idx += step;
                }
            }
        }

        Ok(Self {
            fast,
            count,
            first_code,
            first_sym,
            syms,
            max_len,
        })
    }
}

/// Compute LSB-first (bit-reversed) canonical Huffman codewords for `lengths`.
pub(crate) fn canonical_codes(lengths: &[u8]) -> Vec<u16> {
    let mut bl_count = [0u16; MAX_BITS + 1];
    for &len in lengths {
        if len > 0 && usize::from(len) <= MAX_BITS {
            bl_count[usize::from(len)] += 1;
        }
    }
    let mut next_code = [0u16; MAX_BITS + 1];
    let mut code = 0u16;
    for bits in 1..=MAX_BITS {
        code = (code + bl_count[bits - 1]) << 1;
        next_code[bits] = code;
    }
    let mut out = vec![0u16; lengths.len()];
    for (i, &len) in lengths.iter().enumerate() {
        if len > 0 && usize::from(len) <= MAX_BITS {
            let b = usize::from(len);
            let c = next_code[b];
            next_code[b] = c + 1;
            out[i] = reverse_bits(c, len);
        }
    }
    out
}

#[derive(Clone, Copy)]
enum PkgNode {
    Leaf(usize),
    Pair(usize, usize),
}

#[derive(Clone, Copy)]
struct PkgItem {
    weight: u64,
    node: usize,
}

/// Build length-limited canonical code lengths using Package-Merge.
pub(crate) fn build_code_lengths(freqs: &[u32], max_bits: usize, ensure_two: bool) -> Vec<u8> {
    let mut lengths = vec![0u8; freqs.len()];
    let mut active: Vec<(u64, usize)> = freqs
        .iter()
        .enumerate()
        .filter(|(_, &f)| f > 0)
        .map(|(sym, &f)| (u64::from(f), sym))
        .collect();
    active.sort_unstable();

    if active.is_empty() {
        if ensure_two && lengths.len() >= 2 {
            lengths[0] = 1;
            lengths[1] = 1;
        }
        return lengths;
    }
    if active.len() == 1 {
        let sym = active[0].1;
        lengths[sym] = 1;
        if ensure_two && lengths.len() >= 2 {
            let other = if sym == 0 { 1 } else { 0 };
            lengths[other] = 1;
        }
        return lengths;
    }

    let n = active.len();
    let mut nodes: Vec<PkgNode> = Vec::with_capacity(n * max_bits);
    let mut leaves: Vec<PkgItem> = Vec::with_capacity(n);
    for &(weight, sym) in &active {
        let node = nodes.len();
        nodes.push(PkgNode::Leaf(sym));
        leaves.push(PkgItem { weight, node });
    }

    let mut packages: Vec<PkgItem> = Vec::new();
    let mut merged: Vec<PkgItem> = Vec::with_capacity(2 * n);

    for level in 0..max_bits {
        merged.clear();
        let (mut li, mut pi) = (0usize, 0usize);
        while li < leaves.len() && pi < packages.len() {
            if leaves[li].weight <= packages[pi].weight {
                merged.push(leaves[li]);
                li += 1;
            } else {
                merged.push(packages[pi]);
                pi += 1;
            }
        }
        merged.extend_from_slice(&leaves[li..]);
        merged.extend_from_slice(&packages[pi..]);

        if level + 1 == max_bits {
            break;
        }

        packages.clear();
        let mut i = 0;
        while i + 1 < merged.len() {
            let a = merged[i];
            let b = merged[i + 1];
            let node = nodes.len();
            nodes.push(PkgNode::Pair(a.node, b.node));
            packages.push(PkgItem {
                weight: a.weight.saturating_add(b.weight),
                node,
            });
            i += 2;
        }
    }

    let take = (2 * n - 2).min(merged.len());
    let mut refs = vec![0u16; nodes.len()];
    for item in &merged[..take] {
        refs[item.node] += 1;
    }
    for i in (0..nodes.len()).rev() {
        let r = refs[i];
        if r == 0 {
            continue;
        }
        match nodes[i] {
            PkgNode::Pair(a, b) => {
                refs[a] += r;
                refs[b] += r;
            }
            PkgNode::Leaf(sym) => {
                lengths[sym] = r as u8;
            }
        }
    }
    lengths
}
