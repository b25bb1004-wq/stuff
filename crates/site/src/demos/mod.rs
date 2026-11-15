//! Demo registry and pure bounded runners for the Monolith site.

pub mod chisel;
pub mod hades;
pub mod iridium;
pub mod lithium;
pub mod silicon;
pub mod sphinx;

/// Maximum allowed input size in bytes (64 KiB) for any demo run.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;

/// Output produced by running a demo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// UTF-8 text transcript or formatted result.
    Text(String),
    /// RGBA8 framebuffer image (`rgba.len() == w * h * 4`).
    Pixels {
        /// Width in pixels.
        w: u32,
        /// Height in pixels.
        h: u32,
        /// Row-major RGBA8 pixel bytes.
        rgba: Vec<u8>,
    },
}

/// A registered live demo entry.
#[derive(Debug, Clone, Copy)]
pub struct Demo {
    /// Unique lowercase identifier matching `systems.js`.
    pub id: &'static str,
    /// Human-readable title shown in listings.
    pub title: &'static str,
    /// Default script or input prefilled in the demo editor.
    pub default_input: &'static str,
    /// Pure, deterministic, bounded runner.
    pub run: fn(&str) -> Result<Output, String>,
}

static DEMOS: [Demo; 6] = [
    iridium::DEMO,
    sphinx::DEMO,
    silicon::DEMO,
    hades::DEMO,
    chisel::DEMO,
    lithium::DEMO,
];

/// Return all registered live demos in presentation order.
pub fn all() -> &'static [Demo] {
    &DEMOS
}

/// Look up a registered demo by its identifier.
pub fn find(id: &str) -> Option<&'static Demo> {
    DEMOS.iter().find(|d| d.id == id)
}

pub(crate) fn check_input(input: &str) -> Result<(), String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "input exceeds {} byte limit (got {} bytes)",
            MAX_INPUT_BYTES,
            input.len()
        ));
    }
    Ok(())
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for &b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn registry_ids_are_unique_and_lookup_works() {
        let demos = all();
        assert_eq!(demos.len(), 6);
        for d in demos {
            assert_eq!(find(d.id).unwrap().id, d.id);
        }
        assert!(find("unknown").is_none());
    }

    #[test]
    fn input_cap_and_hex_helper() {
        assert!(check_input(&"x".repeat(MAX_INPUT_BYTES)).is_ok());
        assert!(check_input(&"x".repeat(MAX_INPUT_BYTES + 1)).is_err());
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
    }
}
