//! Host process calls on std. Fds passed to `spawn` are ignored on the host.
use super::map;
use crate::{Error, Result};
use alloc::string::String;
use std::collections::BTreeMap;
use std::process::{Child, Command};
use std::sync::Mutex;

static CHILDREN: Mutex<BTreeMap<u64, Child>> = Mutex::new(BTreeMap::new());

/// Exits the process with `code`.
pub fn exit(code: i32) -> ! {
    std::process::exit(code)
}

/// Starts `path` with `args` (argv[0] is `path`); returns its pid.
pub fn spawn(
    path: &str,
    args: &[&str],
    stdin_fd: Option<i32>,
    stdout_fd: Option<i32>,
) -> Result<u64> {
    let _ = (stdin_fd, stdout_fd);
    let child = Command::new(path).args(args).spawn().map_err(map)?;
    let pid = u64::from(child.id());
    CHILDREN
        .lock()
        .map_err(|_| Error::Io(String::from("child table poisoned")))?
        .insert(pid, child);
    Ok(pid)
}

/// Waits for a child started by `spawn` and returns its exit code.
pub fn wait(pid: u64) -> Result<i32> {
    let mut child = CHILDREN
        .lock()
        .map_err(|_| Error::Io(String::from("child table poisoned")))?
        .remove(&pid)
        .ok_or(Error::NotFound)?;
    let status = child.wait().map_err(map)?;
    status
        .code()
        .ok_or_else(|| Error::Io(String::from("child ended without an exit code")))
}

/// Prints `envelope` as the last stdout line.
pub fn result(envelope: &str) -> Result<()> {
    use std::io::Write;
    writeln!(std::io::stdout(), "{envelope}").map_err(map)
}

/// Always `Ok(None)` on the host: results are printed, not stored.
pub fn child_result(_pid: u64) -> Result<Option<String>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn spawn_and_wait_a_host_program() {
        // The test binary itself, with `--list`, exits 0 without running tests.
        let exe = std::env::current_exe().unwrap();
        let exe = exe.to_string_lossy().into_owned();
        let pid = spawn(&exe, &["--list"], None, None).unwrap();
        assert_eq!(wait(pid), Ok(0));
        assert_eq!(wait(pid), Err(Error::NotFound));
        assert_eq!(child_result(pid), Ok(None));
    }

    #[test]
    fn spawn_missing_program_is_not_found() {
        assert_eq!(
            spawn("/no/such/atlas-program", &[], None, None),
            Err(Error::NotFound)
        );
    }

    #[test]
    fn result_prints_envelope() {
        assert_eq!(result("{\"ok\":true}"), Ok(()));
    }
}
