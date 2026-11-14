//! IEEE CRC-32 and Adler-32 checksums.

/// Precomputed IEEE 802.3 CRC-32 lookup table (reflected polynomial 0xEDB88320).
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

/// IEEE CRC-32 checksum, as used in PNG and zlib.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        let idx = ((crc ^ u32::from(b)) & 0xFF) as usize;
        crc = CRC_TABLE[idx] ^ (crc >> 8);
    }
    !crc
}

/// Adler-32 checksum, as used in the zlib wrapper (RFC 1950).
pub fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65521;
    const NMAX: usize = 5552;
    let mut a = 1u32;
    let mut b = 0u32;
    for chunk in data.chunks(NMAX) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD_ADLER;
        b %= MOD_ADLER;
    }
    (b << 16) | a
}
