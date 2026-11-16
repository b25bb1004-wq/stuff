//! Cross-process exclusive lock on a database, held on a sibling `<db>-lock` file.

use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::Error;

/// How long a second opener waits for the lock before failing.
const LOCK_WAIT: Duration = Duration::from_secs(5);
/// Pause between lock attempts while waiting.
const LOCK_RETRY: Duration = Duration::from_millis(50);

/// Exclusive lock on a database; released when dropped.
///
/// A process that has a database open holds this lock for its whole lifetime. Because no
/// other process can hold the same database open at the same time, a live writer's WAL is
/// never mistaken for a crashed log: recovery only runs when the lock is free, and nothing
/// can truncate or checkpoint the log while it is held.
#[derive(Debug)]
pub struct DbLock {
    _file: File,
}

/// Take the exclusive lock for the database at `db_path`, waiting up to 5 s.
///
/// Fails with `Error::Locked` if another process still holds it. The lock file is created
/// if missing and is left on disk; the database and WAL files are not touched.
pub fn lock_database(db_path: &Path) -> Result<DbLock, Error> {
    let mut os: OsString = db_path.as_os_str().to_os_string();
    os.push("-lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(PathBuf::from(os))
        .map_err(|_| Error::Io)?;

    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(DbLock { _file: file }),
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    return Err(Error::Locked);
                }
                thread::sleep(LOCK_RETRY);
            }
            Err(TryLockError::Error(_)) => return Err(Error::Io),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn second_opener_waits_then_fails_until_release() {
        let dir = std::env::temp_dir().join(format!("iridium_lock_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("lock_test.idb");

        let held = lock_database(&db).unwrap();
        let start = Instant::now();
        assert_eq!(lock_database(&db).err(), Some(Error::Locked));
        assert!(start.elapsed() >= LOCK_WAIT);

        drop(held);
        assert!(lock_database(&db).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
