//! CRC-32C (Castagnoli, reflected polynomial 0x82F63B78), table driven.

const POLY: u32 = 0x82F6_3B78;

const TABLE: [u32; 256] = make_table();

const fn make_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

/// CRC-32C of `data`.
pub(crate) fn crc32c(data: &[u8]) -> u32 {
    crc_end(crc_update(crc_begin(), data))
}

/// Start a running CRC-32C over several pieces.
pub(crate) fn crc_begin() -> u32 {
    !0u32
}

/// Feed `data` into a running CRC-32C.
pub(crate) fn crc_update(mut c: u32, data: &[u8]) -> u32 {
    for &b in data {
        let idx = ((c ^ u32::from(b)) & 0xFF) as usize;
        c = TABLE[idx] ^ (c >> 8);
    }
    c
}

/// Finish a running CRC-32C.
pub(crate) fn crc_end(c: u32) -> u32 {
    !c
}

#[cfg(test)]
mod tests {
    use super::{crc32c, crc_begin, crc_end, crc_update};

    #[test]
    fn check_value_and_empty() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(&[]), 0);
    }

    #[test]
    fn running_crc_matches_one_shot() {
        let whole = crc32c(b"hello, journal");
        let c = crc_update(crc_update(crc_begin(), b"hello, "), b"journal");
        assert_eq!(crc_end(c), whole);
    }
}
