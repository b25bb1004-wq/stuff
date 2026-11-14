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
    let mut c = !0u32;
    for &b in data {
        let idx = ((c ^ u32::from(b)) & 0xFF) as usize;
        c = TABLE[idx] ^ (c >> 8);
    }
    !c
}

#[cfg(test)]
mod tests {
    use super::crc32c;

    #[test]
    fn check_value_and_empty() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(&[]), 0);
    }
}
