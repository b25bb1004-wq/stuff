//! User-space virtual address validation and Titan syscall error codes.

use core::fmt;

/// Inclusive lower bound of user-mode virtual address space (64 KiB).
pub const USER_MIN: u64 = 0x0000_0000_0001_0000;

/// Exclusive upper bound of user-mode virtual address space (256 GiB - 4 KiB).
pub const USER_MAX: u64 = 0x0000_003F_FFFF_F000;

/// Syscall error codes defined in `docs/TITAN.md` (`-1`..`-7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(isize)]
pub enum Errno {
    /// `-1`: Bad argument or invalid user pointer.
    BadArg = -1,
    /// `-2`: Target path, program, or entry not found.
    NotFound = -2,
    /// `-3`: Out of memory or process capacity.
    NoMem = -3,
    /// `-4`: Invalid file descriptor.
    BadFd = -4,
    /// `-5`: Target already exists.
    Exists = -5,
    /// `-6`: Path component is not a directory.
    NotDir = -6,
    /// `-7`: Resource is busy.
    Busy = -7,
}

impl Errno {
    /// Return the signed syscall return value (`-1`..`-7`).
    pub const fn as_isize(self) -> isize {
        self as isize
    }
}

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadArg => write!(f, "bad argument (-1)"),
            Self::NotFound => write!(f, "not found (-2)"),
            Self::NoMem => write!(f, "no memory (-3)"),
            Self::BadFd => write!(f, "bad fd (-4)"),
            Self::Exists => write!(f, "exists (-5)"),
            Self::NotDir => write!(f, "not a directory (-6)"),
            Self::Busy => write!(f, "busy (-7)"),
        }
    }
}

/// Validate that `[ptr, ptr + len)` lies completely within user space without overflow.
///
/// A zero-length range (`len == 0`) always succeeds.
pub fn check_user_range(ptr: u64, len: u64) -> Result<(), Errno> {
    if len == 0 {
        return Ok(());
    }
    let end = ptr.checked_add(len).ok_or(Errno::BadArg)?;
    if ptr < USER_MIN || end > USER_MAX {
        return Err(Errno::BadArg);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use assay::forall;

    #[test]
    fn errno_codes_match_spec() {
        assert_eq!(Errno::BadArg.as_isize(), -1);
        assert_eq!(Errno::NotFound.as_isize(), -2);
        assert_eq!(Errno::NoMem.as_isize(), -3);
        assert_eq!(Errno::BadFd.as_isize(), -4);
        assert_eq!(Errno::Exists.as_isize(), -5);
        assert_eq!(Errno::NotDir.as_isize(), -6);
        assert_eq!(Errno::Busy.as_isize(), -7);
    }

    #[test]
    fn user_range_boundaries() {
        // Zero length is always Ok regardless of pointer.
        assert_eq!(check_user_range(0, 0), Ok(()));
        assert_eq!(check_user_range(u64::MAX, 0), Ok(()));
        assert_eq!(check_user_range(0x8000_0000, 0), Ok(()));
        assert_eq!(check_user_range(USER_MAX, 0), Ok(()));

        // Lower boundary.
        assert_eq!(check_user_range(0, 1), Err(Errno::BadArg));
        assert_eq!(check_user_range(USER_MIN - 1, 1), Err(Errno::BadArg));
        assert_eq!(check_user_range(USER_MIN - 1, 2), Err(Errno::BadArg));
        assert_eq!(check_user_range(USER_MIN, 1), Ok(()));

        // Upper boundary.
        assert_eq!(check_user_range(USER_MAX - 1, 1), Ok(()));
        assert_eq!(check_user_range(USER_MAX - 1, 2), Err(Errno::BadArg));
        assert_eq!(check_user_range(USER_MAX, 1), Err(Errno::BadArg));
        assert_eq!(check_user_range(0x3F_FFFF_FF00, 4096), Err(Errno::BadArg));

        // Overflow.
        assert_eq!(check_user_range(u64::MAX, 1), Err(Errno::BadArg));
        assert_eq!(check_user_range(u64::MAX - 15, 32), Err(Errno::BadArg));
        assert_eq!(check_user_range(USER_MIN, u64::MAX), Err(Errno::BadArg));
    }

    #[test]
    fn user_range_forall_matches_u128_oracle() {
        forall(500, 0x0ACE_5501, |rng| {
            let ptr = match rng.below(5) {
                0 => 0,
                1 => USER_MIN.saturating_sub(rng.range(0, 16)),
                2 => USER_MAX.saturating_sub(rng.range(0, 16)),
                3 => u64::MAX.saturating_sub(rng.range(0, 64)),
                _ => rng.u64() & 0x0000_007F_FFFF_FFFF,
            };
            let len = match rng.below(4) {
                0 => 0,
                1 => rng.range(1, 32),
                2 => rng.range(1, 0x10_0000),
                _ => rng.u64(),
            };
            let oracle_ok = len == 0
                || ((ptr as u128) >= (USER_MIN as u128)
                    && (ptr as u128) + (len as u128) <= (USER_MAX as u128));
            let res = check_user_range(ptr, len);
            assert_eq!(res.is_ok(), oracle_ok);
            if let Err(e) = res {
                assert_eq!(e, Errno::BadArg);
            }
        });
    }
}
