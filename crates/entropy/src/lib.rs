//! OS randomness for key generation: the one place in Monolith that talks to the
//! operating system's CSPRNG. It never falls back to a weak source; a target with
//! no OS generator (wasm32-unknown-unknown) gets an error instead.

/// Why the OS generator could not fill the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// This target has no OS random source.
    Unsupported,
    /// The OS source exists but failed (the message says how).
    Os(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unsupported => f.write_str("no OS random source on this target"),
            Error::Os(m) => write!(f, "OS random source failed: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Fill `buf` from the OS CSPRNG.
pub fn fill(buf: &mut [u8]) -> Result<(), Error> {
    if buf.is_empty() {
        return Ok(());
    }
    os::fill(buf)
}

#[cfg(unix)]
mod os {
    use super::Error;
    use std::io::Read;

    pub fn fill(buf: &mut [u8]) -> Result<(), Error> {
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(buf))
            .map_err(|e| Error::Os(e.to_string()))
    }
}

#[cfg(windows)]
mod os {
    use super::Error;

    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x2;

    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(alg: *mut u8, buf: *mut u8, len: u32, flags: u32) -> i32;
    }

    pub fn fill(buf: &mut [u8]) -> Result<(), Error> {
        for chunk in buf.chunks_mut(u32::MAX as usize) {
            // SAFETY: `chunk` is a live, writable slice of exactly `chunk.len()` bytes
            // (at most u32::MAX), a null algorithm handle is valid with
            // BCRYPT_USE_SYSTEM_PREFERRED_RNG, and the call writes only inside `chunk`.
            #[allow(unsafe_code)]
            let status = unsafe {
                BCryptGenRandom(
                    std::ptr::null_mut(),
                    chunk.as_mut_ptr(),
                    chunk.len() as u32,
                    BCRYPT_USE_SYSTEM_PREFERRED_RNG,
                )
            };
            if status != 0 {
                return Err(Error::Os(format!("BCryptGenRandom status {status:#x}")));
            }
        }
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
mod os {
    use super::Error;

    pub fn fill(_buf: &mut [u8]) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_and_differs() {
        let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
        assert_eq!(fill(&mut a), Ok(()));
        assert_eq!(fill(&mut b), Ok(()));
        assert_ne!(a, [0u8; 64]);
        assert_ne!(a, b);
    }

    #[test]
    fn empty_is_ok() {
        assert_eq!(fill(&mut []), Ok(()));
    }
}
