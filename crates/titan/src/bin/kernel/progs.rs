//! Embedded built-in RV64 user program ELFs assembled by `build.rs`.

static INIT_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/init.elf"));
static PING_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ping.elf"));
static PONG_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pong.elf"));
static HOG_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hog.elf"));
static FAULT_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fault.elf"));
static EVIL_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/evil.elf"));

/// Look up an embedded ELF image by program name.
pub fn find_builtin(name: &str) -> Option<&'static [u8]> {
    match name {
        "init" => Some(INIT_ELF),
        "ping" => Some(PING_ELF),
        "pong" => Some(PONG_ELF),
        "hog" => Some(HOG_ELF),
        "fault" => Some(FAULT_ELF),
        "evil" => Some(EVIL_ELF),
        _ => None,
    }
}
