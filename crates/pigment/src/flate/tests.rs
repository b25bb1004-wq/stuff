#![allow(clippy::unwrap_used)]

use super::*;
use alloc::vec;
use alloc::vec::Vec;
use assay::{forall, Rng};

#[test]
fn known_checksum_vectors() {
    assert_eq!(crc32(b""), 0x0000_0000);
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(adler32(b""), 0x0000_0001);
    assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
}

#[test]
fn handmade_blocks_and_trailing_bytes() {
    // Empty fixed block [0x03, 0x00]
    assert_eq!(inflate(&[0x03, 0x00], 64).unwrap(), b"");
    // Trailing bytes after final block are ignored
    assert_eq!(
        inflate(&[0x03, 0x00, 0xDE, 0xAD, 0xBE, 0xEF], 64).unwrap(),
        b""
    );

    // Stored block for "hello"
    let stored = [0x01, 0x05, 0x00, 0xFA, 0xFF, b'h', b'e', b'l', b'l', b'o'];
    assert_eq!(inflate(&stored, 64).unwrap(), b"hello");

    // Empty stored block
    assert_eq!(inflate(&[0x01, 0x00, 0x00, 0xFF, 0xFF], 64).unwrap(), b"");

    // Hand-built fixed Huffman block for "AB"
    assert_eq!(inflate(&[0x73, 0x74, 0x02, 0x00], 64).unwrap(), b"AB");

    // Hand-built fixed Huffman block for "ABABAB" (literals 'A', 'B' + match len 4, dist 2)
    assert_eq!(
        inflate(&[0x73, 0x74, 0x02, 0x41, 0x00], 64).unwrap(),
        b"ABABAB"
    );
}

#[test]
fn malformed_streams_return_expected_errors() {
    // 1. Empty deflate input -> Truncated
    assert_eq!(inflate(b"", 1024), Err(FlateError::Truncated));

    // 2. Reserved BTYPE = 3 -> Invalid
    assert_eq!(inflate(&[0x07], 1024), Err(FlateError::Invalid));

    // 3. Stored block LEN/NLEN mismatch -> Invalid
    assert_eq!(
        inflate(&[0x01, 0x05, 0x00, 0x00, 0x00, 1, 2, 3, 4, 5], 1024),
        Err(FlateError::Invalid)
    );

    // 4. Stored block truncated payload -> Truncated
    assert_eq!(
        inflate(&[0x01, 0x05, 0x00, 0xFA, 0xFF, 1, 2], 1024),
        Err(FlateError::Truncated)
    );

    // 5. Dynamic block with HLIT > 286 (HLIT raw = 31 => 288) -> Invalid
    assert_eq!(
        inflate(&[0xFD, 0x00, 0x00, 0x00], 1024),
        Err(FlateError::Invalid)
    );

    // 6. Dynamic block code-length tree over-subscribed -> Invalid
    // BFINAL=1, BTYPE=2 (0b101), HLIT=0 (257), HDIST=0 (1), HCLEN=0 (4 code lengths)
    // BL_ORDER[0..4] = [16, 17, 18, 0]; give all 4 length = 1 => over-subscribed (4 * 2^-1 = 2 > 1).
    // Bits: 101 (3) + 00000 (5) + 00000 (5) + 0000 (4) + 001 001 001 001 (12) = 29 bits
    let over_sub = [0x05, 0x00, 0x48, 0x92, 0x00];
    assert_eq!(inflate(&over_sub, 1024), Err(FlateError::Invalid));

    // 7. Dynamic block code-length tree incomplete -> Invalid
    // Give BL_ORDER[0]=16 len=1, others 0 => left > 0
    let incomplete_bl = [0x05, 0x00, 0x02, 0x00, 0x00];
    assert_eq!(inflate(&incomplete_bl, 1024), Err(FlateError::Invalid));

    // 8. Fixed block with distance before start of output -> Invalid
    // BFINAL=1, BTYPE=1 (0b011), sym 257 (len 3: 7b 0000001 -> rev 1000000 = 0x40), dist 1 (5b 00000), EOB
    // Bits: 011 + 1000000 (10b: 0b00_0000_0011 = 0x03? Wait: 0b011 | (0x40 << 3) = 0x203 -> [0x03, 0x02])
    let bad_dist = [0x03, 0x02, 0x00, 0x00];
    assert_eq!(inflate(&bad_dist, 1024), Err(FlateError::Invalid));

    // 9. Fixed block with invalid symbol 286 -> Invalid
    // sym 286 is 8-bit code 11000110 (rev = 01100011 = 0x63)
    // 0b011 | (0x63 << 3) = 0x31B -> [0x1B, 0x03, 0x00]
    assert_eq!(inflate(&[0x1B, 0x03, 0x00], 1024), Err(FlateError::Invalid));

    // 10. Bomb guard max_out exceeded -> TooLarge
    let stored = [0x01, 0x05, 0x00, 0xFA, 0xFF, b'h', b'e', b'l', b'l', b'o'];
    assert_eq!(inflate(&stored, 4), Err(FlateError::TooLarge));
    assert_eq!(inflate(&stored, 5).unwrap(), b"hello");

    // 11. Truncated fixed block without EOB -> Truncated
    assert_eq!(inflate(&[0x73, 0x74], 1024), Err(FlateError::Truncated));

    // 12. zlib errors: bad header checksum, FDICT, bad CM, truncated trailer, checksum mismatch
    assert_eq!(
        zlib_decompress(&[0x78, 0x00, 0x03, 0x00, 0, 0, 0, 1], 64),
        Err(FlateError::Invalid)
    );
    // FDICT set (0x78, 0xBB: (0x78*256 + 0xBB) % 31 == 0)
    assert_eq!(
        zlib_decompress(&[0x78, 0xBB, 0x03, 0x00, 0, 0, 0, 1], 64),
        Err(FlateError::Invalid)
    );
    // Bad CM=9 (0x79, 0x99: (0x7999) % 31 == 0? 0x7999=31129, 31129 % 31 = 5; 0x7994 = 31124 % 31 = 0)
    assert_eq!(
        zlib_decompress(&[0x79, 0x94, 0x03, 0x00, 0, 0, 0, 1], 64),
        Err(FlateError::Invalid)
    );
    let valid_z = zlib_compress(b"hello", 6);
    assert_eq!(
        zlib_decompress(&valid_z[..valid_z.len() - 1], 64),
        Err(FlateError::Truncated)
    );
    let mut bad_cksum = valid_z.clone();
    let last = bad_cksum.len() - 1;
    bad_cksum[last] ^= 0x01;
    assert_eq!(zlib_decompress(&bad_cksum, 64), Err(FlateError::Checksum));
}

#[test]
fn single_code_distance_tree_allowed_by_rfc() {
    // Build a valid dynamic block with 1 distance code of length 1 (dist sym 0 = dist 1)
    // and simple lit/len tree: only lit 'A' (65) and EOB (256) and len 3 (257)?
    // Actually our own compressor emits single-code distance tree for RLE runs like b"AAAAA..."!
    let data = vec![b'X'; 500];
    let comp = deflate(&data, 6);
    assert_eq!(inflate(&comp, 500).unwrap(), data);
    // Verify it actually compressed tightly (< 30 bytes)
    assert!(comp.len() < 30, "comp len = {}", comp.len());
}

fn gen_test_payload(r: &mut Rng, max_len: usize) -> Vec<u8> {
    let mode = r.below(4);
    let len = r.below(max_len + 1);
    match mode {
        0 => {
            // Pure random
            let mut v = vec![0u8; len];
            r.fill(&mut v);
            v
        }
        1 => {
            // Repeated runs and patterns
            let mut v = Vec::with_capacity(len);
            while v.len() < len {
                let b = (r.u64() & 0xFF) as u8;
                let run = r.range(1, 64) as usize;
                for _ in 0..run.min(len - v.len()) {
                    v.push(b);
                }
            }
            v
        }
        2 => {
            // Repeating dictionary slices (exercises LZ77 + lazy matching)
            let dict =
                b"The quick brown fox jumps over the lazy dog. 0123456789 abcdefghijklmnop! ";
            let mut v = Vec::with_capacity(len);
            while v.len() < len {
                if r.chance(3, 4) {
                    let start = r.below(dict.len());
                    let take = r.range(1, 32) as usize;
                    let end = (start + take).min(dict.len());
                    let slice = &dict[start..end];
                    let rem = len - v.len();
                    v.extend_from_slice(&slice[..slice.len().min(rem)]);
                } else {
                    v.push((r.u64() & 0xFF) as u8);
                }
            }
            v
        }
        _ => {
            // Small alphabet
            let mut v = Vec::with_capacity(len);
            let alpha = *b"abc\n";
            for _ in 0..len {
                v.push(*r.pick(&alpha));
            }
            v
        }
    }
}

#[test]
fn forall_round_trip_all_levels() {
    forall(500, 0x59_F1A7_0001, |r| {
        let data = gen_test_payload(r, 4096);
        let level = r.below(10) as u8;
        let comp = deflate(&data, level);
        let dec = inflate(&comp, data.len()).unwrap();
        assert_eq!(dec, data, "deflate/inflate mismatch at level {level}");

        let zcomp = zlib_compress(&data, level);
        let zdec = zlib_decompress(&zcomp, data.len()).unwrap();
        assert_eq!(zdec, data, "zlib roundtrip mismatch at level {level}");
    });
}

#[test]
fn multi_block_and_large_window_round_trip() {
    // > 65535 bytes so stored level 0 splits blocks and level 6 exercises 32KiB window + multi-block
    let mut r = Rng::new(42);
    let data = gen_test_payload(&mut r, 90_000);
    for level in [0u8, 1, 4, 6, 9] {
        let comp = deflate(&data, level);
        assert_eq!(inflate(&comp, data.len()).unwrap(), data);
    }
    // Determinism check
    assert_eq!(deflate(&data, 6), deflate(&data, 6));
    assert_eq!(deflate(&data, 12), deflate(&data, 9));
}

#[test]
fn forall_fuzz_inflate_never_panics() {
    let sample = zlib_compress(b"Hello Strata world! Hello Strata world! 1234567890", 6);
    forall(1000, 0x59_F022_0002, |r| {
        if r.chance(1, 2) {
            let garbage = r.bytes(256);
            let _ = inflate(&garbage, 4096);
            let _ = zlib_decompress(&garbage, 4096);
        } else {
            let mut mutated = sample.clone();
            if r.chance(1, 2) && !mutated.is_empty() {
                let trunc = r.below(mutated.len());
                mutated.truncate(trunc);
            } else if !mutated.is_empty() {
                let flips = r.range(1, 5);
                for _ in 0..flips {
                    let idx = r.below(mutated.len());
                    mutated[idx] ^= 1u8 << (r.below(8) as u8);
                }
            }
            let _ = zlib_decompress(&mutated, 4096);
            if mutated.len() >= 2 {
                let _ = inflate(&mutated[2..], 4096);
            }
        }
    });
}
