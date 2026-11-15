//! IEEE 802.3 CRC-32 (reflected polynomial 0xEDB88320).

const POLY: u32 = 0xEDB8_8320;

const fn make_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            if (c & 1) != 0 {
                c = (c >> 1) ^ POLY;
            } else {
                c >>= 1;
            }
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

const TABLE: [u32; 256] = make_table();

/// Incremental IEEE CRC-32 hasher.
#[derive(Debug, Clone, Copy)]
pub struct Crc32 {
    state: u32,
}

impl Crc32 {
    /// Start a new CRC-32 calculation.
    pub const fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    /// Feed bytes into the CRC-32 state.
    pub fn update(&mut self, data: &[u8]) {
        let mut crc = self.state;
        for &b in data {
            let idx = ((crc ^ u32::from(b)) & 0xFF) as usize;
            crc = (crc >> 8) ^ TABLE[idx];
        }
        self.state = crc;
    }

    /// Finish and return the CRC-32 value.
    pub const fn finalize(self) -> u32 {
        self.state ^ 0xFFFF_FFFF
    }
}

/// Compute the IEEE CRC-32 of a single byte slice.
pub fn crc32(data: &[u8]) -> u32 {
    let mut h = Crc32::new();
    h.update(data);
    h.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ieee_check_vector() {
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn incremental_matches_oneshot() {
        let mut h = Crc32::new();
        h.update(b"1234");
        h.update(b"56789");
        assert_eq!(h.finalize(), 0xCBF4_3926);
    }
}
