//! Path scoping and disk operations for the `athena serve` file API.
//!
//! Every path a request names is checked before the disk is touched. The check is lexical first
//! (no absolute paths, drive letters, NUL bytes, `..`, trash folder or Windows device names) and
//! then physical: the longest existing ancestor is canonicalised and must still sit under the
//! canonical served folder, so a symlink that points outside is refused.

use std::ffi::OsString;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::time::{SystemTime, UNIX_EPOCH};

/// Folder inside the served folder that receives deleted entries.
pub const TRASH_DIR: &str = ".athena-trash";
/// Largest file the API reads or writes, in bytes.
pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A refused or failed request: the HTTP status and a short message for the JSON error body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    /// HTTP status code to answer with.
    pub status: u16,
    /// Human-readable reason.
    pub message: String,
}

impl Denied {
    /// Build a refusal with `status` and `message`.
    pub fn new(status: u16, message: impl Into<String>) -> Denied {
        Denied {
            status,
            message: message.into(),
        }
    }
}

/// One child of a folder, as the explorer shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// File or folder name (no separators).
    pub name: String,
    /// True for a folder.
    pub is_dir: bool,
    /// Size in bytes (0 for a folder).
    pub size: u64,
}

fn io_denied(err: std::io::Error, rel: &str) -> Denied {
    match err.kind() {
        // Plain words for a missing name; the OS text is only for the other cases.
        ErrorKind::NotFound => Denied::new(404, format!("not found: {rel}")),
        ErrorKind::AlreadyExists => Denied::new(409, format!("{rel}: {err}")),
        _ => Denied::new(500, format!("{rel}: {err}")),
    }
}

fn outside() -> Denied {
    Denied::new(403, "path leaves the served folder")
}

/// Split a folder-relative path into its real components. `/` and `\` both separate on every
/// OS, so a Windows-style path is checked the same way on Linux and macOS.
pub fn split_rel(rel: &str) -> Result<Vec<&str>, Denied> {
    if rel.contains('\0') {
        return Err(Denied::new(403, "NUL byte in path"));
    }
    if rel.starts_with(['/', '\\']) {
        return Err(Denied::new(403, "absolute paths are not allowed"));
    }
    if rel.contains(':') {
        return Err(Denied::new(
            403,
            "drive letters and stream names are not allowed",
        ));
    }
    let mut parts = Vec::new();
    for part in rel.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(Denied::new(403, "'..' is not allowed")),
            TRASH_DIR => {
                return Err(Denied::new(
                    403,
                    "the trash folder is not reachable through the API",
                ))
            }
            other if is_device_name(other) => {
                return Err(Denied::new(403, "Windows device names are not allowed"))
            }
            other => parts.push(other),
        }
    }
    Ok(parts)
}

/// True for `CON`, `NUL`, `COM1` and the other reserved names, with or without an extension.
fn is_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    let numbered =
        |p: &str| stem.len() == 4 && stem.starts_with(p) && stem.as_bytes()[3].is_ascii_digit();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered("COM") || numbered("LPT")
}

/// Canonicalise the longest existing ancestor of `path` and require it to stay under `root`.
fn confine(root: &Path, path: PathBuf) -> Result<PathBuf, Denied> {
    let mut existing = path;
    let mut tail: Vec<OsString> = Vec::new();
    loop {
        match fs::canonicalize(&existing) {
            Ok(real) => {
                if !real.starts_with(root) {
                    return Err(outside());
                }
                let mut out = real;
                out.extend(tail.iter().rev());
                return Ok(out);
            }
            Err(_) => {
                // A link that exists but points nowhere is refused, not read as a missing name.
                if fs::symlink_metadata(&existing).is_ok() {
                    return Err(Denied::new(403, "symbolic link cannot be followed"));
                }
                let name = existing.file_name().ok_or_else(outside)?.to_owned();
                tail.push(name);
                if !existing.pop() {
                    return Err(outside());
                }
            }
        }
    }
}

/// Resolve `rel` to a real path under the canonical `root`, following links. Used for reads and
/// writes, where the content of the target is what matters.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, Denied> {
    let mut path = root.to_path_buf();
    for part in split_rel(rel)? {
        path.push(part);
    }
    confine(root, path)
}

/// Resolve `rel` to its entry: the parent folder is confined, the final name is kept as it is,
/// so rename and trash move a link itself and never its target. An empty path is refused.
pub fn resolve_entry(root: &Path, rel: &str) -> Result<PathBuf, Denied> {
    let parts = split_rel(rel)?;
    let Some((last, parents)) = parts.split_last() else {
        return Err(Denied::new(
            400,
            "the served folder itself cannot be changed here",
        ));
    };
    let mut parent = root.to_path_buf();
    for part in parents {
        parent.push(part);
    }
    let mut path = confine(root, parent)?;
    path.push(last);
    Ok(path)
}

/// List the children of folder `rel`, folders first. Entries that lead outside are hidden.
pub fn list(root: &Path, rel: &str) -> Result<Vec<Entry>, Denied> {
    let dir = resolve(root, rel)?;
    if !dir.is_dir() {
        return Err(Denied::new(400, format!("{rel} is not a folder")));
    }
    let read = fs::read_dir(&dir).map_err(|e| io_denied(e, rel))?;
    let mut out = Vec::new();
    for item in read {
        let item = item.map_err(|e| io_denied(e, rel))?;
        let Ok(name) = item.file_name().into_string() else {
            continue;
        };
        if name == TRASH_DIR || confine(root, item.path()).is_err() {
            continue;
        }
        let Ok(meta) = fs::metadata(item.path()) else {
            continue;
        };
        out.push(Entry {
            name,
            is_dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// Read a UTF-8 text file. Binary files are refused with 415 so an editor never rewrites them.
pub fn read_text(root: &Path, rel: &str) -> Result<String, Denied> {
    let path = resolve(root, rel)?;
    let meta = fs::metadata(&path).map_err(|e| io_denied(e, rel))?;
    if meta.is_dir() {
        return Err(Denied::new(400, format!("{rel} is a folder")));
    }
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(Denied::new(413, format!("{rel} is larger than 16 MiB")));
    }
    let bytes = fs::read(&path).map_err(|e| io_denied(e, rel))?;
    String::from_utf8(bytes).map_err(|_| Denied::new(415, format!("{rel} is not UTF-8 text")))
}

/// Write a file atomically: the bytes go to a temporary file beside it, then replace the target.
pub fn write_file(root: &Path, rel: &str, bytes: &[u8]) -> Result<(), Denied> {
    // A trailing separator names a folder, so it can never be a file.
    if rel.is_empty() || rel.ends_with(['/', '\\']) {
        return Err(Denied::new(400, "not a file path"));
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Denied::new(413, "body is larger than 16 MiB"));
    }
    let path = resolve(root, rel)?;
    if path.is_dir() {
        return Err(Denied::new(400, format!("{rel} is a folder")));
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(Denied::new(400, format!("{rel} has no file name")));
    };
    // Missing parent folders are created, like mkdir -p.
    fs::create_dir_all(parent).map_err(|e| io_denied(e, rel))?;
    let temp = parent.join(format!(
        ".{}.athena-tmp-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        COUNTER.fetch_add(1, SeqCst)
    ));
    if let Err(err) = write_then_rename(&temp, &path, bytes) {
        let _ = fs::remove_file(&temp);
        return Err(io_denied(err, rel));
    }
    Ok(())
}

fn write_then_rename(temp: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)
}

/// Create an empty file or a folder. Fails with 409 if the name is taken.
pub fn create(root: &Path, rel: &str, is_dir: bool) -> Result<(), Denied> {
    let path = resolve_entry(root, rel)?;
    if let Some(parent) = path.parent() {
        // Missing parent folders are created, like mkdir -p; the leaf still fails if taken.
        fs::create_dir_all(parent).map_err(|e| io_denied(e, rel))?;
    }
    let made = if is_dir {
        fs::create_dir(&path)
    } else {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map(drop)
    };
    made.map_err(|e| io_denied(e, rel))
}

/// Rename or move an entry. Fails with 404 if the source is missing and 409 if the target exists.
pub fn rename(root: &Path, from: &str, to: &str) -> Result<(), Denied> {
    let src = resolve_entry(root, from)?;
    let dst = resolve_entry(root, to)?;
    fs::symlink_metadata(&src).map_err(|e| io_denied(e, from))?;
    if fs::symlink_metadata(&dst).is_ok() {
        return Err(Denied::new(409, format!("{to} already exists")));
    }
    fs::rename(&src, &dst).map_err(|e| io_denied(e, from))
}

/// Move an entry into the trash folder instead of deleting it. Returns the new name in the trash.
pub fn trash(root: &Path, rel: &str) -> Result<String, Denied> {
    let src = resolve_entry(root, rel)?;
    fs::symlink_metadata(&src).map_err(|e| io_denied(e, rel))?;
    let trash_dir = root.join(TRASH_DIR);
    fs::create_dir_all(&trash_dir).map_err(|e| io_denied(e, rel))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let base = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let entry = format!("{stamp}-{}-{base}", COUNTER.fetch_add(1, SeqCst));
    fs::rename(&src, trash_dir.join(&entry)).map_err(|e| io_denied(e, rel))?;
    Ok(entry)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let n = COUNTER.fetch_add(1, SeqCst);
            let dir = std::env::temp_dir()
                .join(format!("athena_fsscope_{tag}_{}_{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(fs::canonicalize(&dir).unwrap())
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn status(r: Result<Vec<&str>, Denied>) -> u16 {
        r.unwrap_err().status
    }

    #[test]
    fn lexical_rejections_hold_for_unix_and_windows_forms() {
        // Each row is refused with 403 whatever OS runs the test.
        let refused = [
            "/etc/passwd",
            "\\etc\\passwd",
            "\\\\server\\share\\x",
            "C:\\Windows\\win.ini",
            "c:win.ini",
            "C:/Windows",
            "..",
            "a/../b",
            "a\\..\\b",
            "..\\..\\secret",
            "nul\0x",
            "a/.athena-trash/x",
            "CON",
            "src\\con.logos",
            "LPT3.txt",
        ];
        for rel in refused {
            assert_eq!(status(split_rel(rel)), 403, "{rel:?} must be refused");
        }
    }

    #[test]
    fn plain_relative_forms_are_accepted_on_every_os() {
        assert_eq!(split_rel("").unwrap(), Vec::<&str>::new());
        assert_eq!(split_rel("a/b").unwrap(), vec!["a", "b"]);
        assert_eq!(split_rel("a\\b").unwrap(), vec!["a", "b"]);
        assert_eq!(split_rel("./a//b/").unwrap(), vec!["a", "b"]);
        assert_eq!(split_rel("conduit.logos").unwrap(), vec!["conduit.logos"]);
    }

    #[test]
    fn resolve_keeps_paths_inside_the_folder() {
        let dir = TempDir::new("resolve");
        fs::write(dir.0.join("a.logos"), "x").unwrap();
        assert_eq!(resolve(&dir.0, "a.logos").unwrap(), dir.0.join("a.logos"));
        // A missing name under a real folder resolves (for create) and stays inside.
        assert_eq!(
            resolve(&dir.0, "new/file.txt").unwrap(),
            dir.0.join("new").join("file.txt")
        );
        assert_eq!(resolve(&dir.0, "../x").unwrap_err().status, 403);
        assert_eq!(resolve(&dir.0, "C:\\x").unwrap_err().status, 403);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_the_folder_is_refused() {
        let dir = TempDir::new("link");
        let outside_dir = TempDir::new("outside");
        fs::write(outside_dir.0.join("secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(outside_dir.0.join("secret.txt"), dir.0.join("leak.txt"))
            .unwrap();
        std::os::unix::fs::symlink(&outside_dir.0, dir.0.join("dirleak")).unwrap();
        std::os::unix::fs::symlink("/nowhere/at/all", dir.0.join("dangling")).unwrap();
        assert_eq!(resolve(&dir.0, "leak.txt").unwrap_err().status, 403);
        assert_eq!(
            resolve(&dir.0, "dirleak/secret.txt").unwrap_err().status,
            403
        );
        assert_eq!(resolve(&dir.0, "dangling").unwrap_err().status, 403);
        assert_eq!(read_text(&dir.0, "leak.txt").unwrap_err().status, 403);
        assert!(list(&dir.0, "")
            .unwrap()
            .iter()
            .all(|e| e.name != "leak.txt"));
    }

    #[cfg(windows)]
    #[test]
    fn symlink_out_of_the_folder_is_refused() {
        let dir = TempDir::new("link");
        let outside_dir = TempDir::new("outside");
        fs::write(outside_dir.0.join("secret.txt"), "s").unwrap();
        // Creating links needs a privilege some accounts lack; the Unix test covers the rule.
        if std::os::windows::fs::symlink_file(
            outside_dir.0.join("secret.txt"),
            dir.0.join("leak.txt"),
        )
        .is_ok()
        {
            assert_eq!(resolve(&dir.0, "leak.txt").unwrap_err().status, 403);
        }
    }

    #[test]
    fn round_trip_read_write_create_rename_trash() {
        let dir = TempDir::new("trip");
        write_file(&dir.0, "main.logos", b"fn main() {}\n").unwrap();
        assert_eq!(read_text(&dir.0, "main.logos").unwrap(), "fn main() {}\n");
        write_file(&dir.0, "main.logos", b"v2").unwrap();
        assert_eq!(read_text(&dir.0, "main.logos").unwrap(), "v2");
        assert!(list(&dir.0, "")
            .unwrap()
            .iter()
            .all(|e| !e.name.contains("athena-tmp")));

        create(&dir.0, "notes", true).unwrap();
        create(&dir.0, "notes/todo.md", false).unwrap();
        assert_eq!(
            create(&dir.0, "notes/todo.md", false).unwrap_err().status,
            409
        );
        let names: Vec<String> = list(&dir.0, "notes")
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["todo.md"]);

        rename(&dir.0, "notes/todo.md", "notes/done.md").unwrap();
        assert!(dir.0.join("notes").join("done.md").is_file());
        assert_eq!(
            rename(&dir.0, "missing.txt", "x.txt").unwrap_err().status,
            404
        );
        create(&dir.0, "other.txt", false).unwrap();
        assert_eq!(
            rename(&dir.0, "other.txt", "main.logos")
                .unwrap_err()
                .status,
            409
        );

        let entry = trash(&dir.0, "main.logos").unwrap();
        assert!(!dir.0.join("main.logos").exists());
        assert!(dir.0.join(TRASH_DIR).join(&entry).is_file());
        assert!(list(&dir.0, "")
            .unwrap()
            .iter()
            .all(|e| e.name != TRASH_DIR));
        assert_eq!(trash(&dir.0, "").unwrap_err().status, 400);
    }

    #[test]
    fn binary_and_oversize_bodies_are_refused() {
        let dir = TempDir::new("limits");
        fs::write(dir.0.join("bin.dat"), [0xff_u8, 0xfe, 0x00]).unwrap();
        assert_eq!(read_text(&dir.0, "bin.dat").unwrap_err().status, 415);
        let big = vec![b'a'; MAX_FILE_BYTES + 1];
        assert_eq!(write_file(&dir.0, "big.txt", &big).unwrap_err().status, 413);
        assert!(!dir.0.join("big.txt").exists());
    }

    #[test]
    fn write_and_create_make_missing_parents_and_refuse_folder_paths() {
        let dir = TempDir::new("parents");
        write_file(&dir.0, "deep/er/file.txt", b"hi").unwrap();
        assert_eq!(read_text(&dir.0, "deep/er/file.txt").unwrap(), "hi");
        create(&dir.0, "made/x/y", true).unwrap();
        assert!(dir.0.join("made/x/y").is_dir());
        assert_eq!(create(&dir.0, "made/x/y", true).unwrap_err().status, 409);
        assert_eq!(
            write_file(&dir.0, "dir/", b"x").unwrap_err(),
            Denied::new(400, "not a file path")
        );
        assert_eq!(
            write_file(&dir.0, "dir\\", b"x").unwrap_err().message,
            "not a file path"
        );
        assert!(!dir.0.join("dir").exists());
        let missing = read_text(&dir.0, "nope/none.txt").unwrap_err();
        assert_eq!(
            (missing.status, missing.message.as_str()),
            (404, "not found: nope/none.txt")
        );
    }
}
