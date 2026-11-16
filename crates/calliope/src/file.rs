//! Reading and writing text files without changing their bytes: the line endings, the final
//! newline and any stray carriage returns are kept as found, and invalid UTF-8 is refused.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Line-ending style of a file as it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    /// Every line ends in `\n` (or the file has no line ends).
    Lf,
    /// Every `\n` was preceded by `\r`; the buffer holds `\n` and saving writes `\r\n`.
    Crlf,
    /// Both styles appear; the text is kept byte for byte.
    Mixed,
}

/// Why a file cannot be opened as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The bytes are not UTF-8; `at` is the offset of the first bad byte.
    InvalidUtf8 {
        /// Offset of the first invalid byte.
        at: usize,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::InvalidUtf8 { at } => write!(
                f,
                "not valid UTF-8 (first bad byte at offset {at}); refusing to open it so nothing is changed"
            ),
        }
    }
}

/// Split file bytes into editor text and the line-ending style to restore on save.
pub fn decode(bytes: &[u8]) -> Result<(String, Eol), DecodeError> {
    let s = std::str::from_utf8(bytes).map_err(|e| DecodeError::InvalidUtf8 {
        at: e.valid_up_to(),
    })?;
    let lf = s.matches('\n').count();
    let crlf = s.matches("\r\n").count();
    if lf == 0 || crlf == 0 {
        Ok((s.to_string(), Eol::Lf))
    } else if crlf == lf {
        Ok((s.replace("\r\n", "\n"), Eol::Crlf))
    } else {
        Ok((s.to_string(), Eol::Mixed))
    }
}

/// Turn editor text back into file text for the given line-ending style.
pub fn encode(text: &str, eol: Eol) -> String {
    match eol {
        Eol::Crlf => text.replace('\n', "\r\n"),
        Eol::Lf | Eol::Mixed => text.to_string(),
    }
}

/// Write `bytes` to `path` through a temporary file in the same directory and a rename, so a
/// failed save never leaves a half-written file.
pub fn save_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map_or_else(|| "file".to_string(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.calliope-{}.tmp", std::process::id()));
    let result = write_then_rename(&tmp, path, bytes);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn write_then_rename(tmp: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = fs::File::create(tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(tmp, path)
}
