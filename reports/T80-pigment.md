# T80 security review: pigment (read-only; nothing executed)
Read: png/decode.rs (parse_ihdr 53, expected_raw_len 103, decode_png 128 chunk loop, pixel alloc 284-291, decode_pass 328), flate/mod.rs zlib_decompress 31-57, flate/inflate.rs ensure_cap 118 and inflate_with_len 143, canvas.rs Canvas::new 46-60, polygon.rs fill (coverage buffer). lib.rs forbid(unsafe_code).
HIGH: none. MEDIUM: none.
LOW L1 canvas.rs:46-60: Canvas::new returns a 0x0 canvas on allocation failure with no error. Input: Canvas::new(40000, 40000, c) (about 6.4 GB) gives width 0 and height 0 silently. Fix: return Result or Option and let callers fail.
No finding (sampled): decode_png checks width, height and pixel count against max_pixels in parse_ihdr before any allocation; expected_raw_len uses checked math; inflate caps output at the expected length (bomb guard) and checks Adler-32; pixel buffer uses try_reserve_exact; polygon coverage buffer is bounded by canvas width.
Note (caller concern, not a pigment bug): decode_png has no default for max_pixels, so a caller passing u64::MAX allows about 4 bytes per pixel up to the allocator limit.
Not reviewed: flate/deflate.rs and huffman.rs (encoder side, no untrusted input).
