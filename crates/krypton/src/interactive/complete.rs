//! Tab completion: command names from PATH in command position, file and folder names
//! elsewhere. Folders get a trailing separator so the next Tab can descend into them.

use std::fs;
use std::path::{Path, PathBuf};
use std::string::String;
use std::vec::Vec;

/// What completion needs from the shell: working folder, home, and PATH settings.
pub(crate) struct CompleteEnv {
    pub(crate) cwd: PathBuf,
    pub(crate) home: Option<PathBuf>,
    pub(crate) path: Option<String>,
    pub(crate) pathext: Option<String>,
    pub(crate) windows: bool,
}

/// Candidates for the word that ends at the cursor.
pub(crate) struct Completion {
    /// Character index where the word being completed starts.
    pub(crate) word_start: usize,
    pub(crate) candidates: Vec<String>,
}

fn is_sep(c: char, windows: bool) -> bool {
    c == '/' || (windows && c == '\\')
}

/// Complete the word that ends at character index `cursor` of `buf`.
pub(crate) fn complete_at(buf: &[char], cursor: usize, env: &CompleteEnv) -> Completion {
    let cursor = cursor.min(buf.len());
    let mut start = cursor;
    while start > 0 && !buf[start - 1].is_whitespace() {
        start -= 1;
    }
    let word: String = buf[start..cursor].iter().collect();
    let before: String = buf[..start].iter().collect();
    let before = before.trim_end();
    let command_position = before.is_empty() || before.ends_with(['|', ';', '&', '(']);
    let has_sep = word.chars().any(|c| is_sep(c, env.windows));
    let candidates = if command_position && !has_sep {
        command_names(&word, env)
    } else {
        path_names(&word, env)
    };
    Completion {
        word_start: start,
        candidates,
    }
}

/// Longest common prefix of the candidates (character-wise).
pub(crate) fn common_prefix(candidates: &[String]) -> String {
    let Some((first, rest)) = candidates.split_first() else {
        return String::new();
    };
    let mut prefix: Vec<char> = first.chars().collect();
    for cand in rest {
        let shared = prefix
            .iter()
            .zip(cand.chars())
            .take_while(|(a, b)| *a == b)
            .count();
        prefix.truncate(shared);
    }
    prefix.into_iter().collect()
}

fn starts_with(name: &str, prefix: &str, windows: bool) -> bool {
    if windows {
        name.to_ascii_lowercase()
            .starts_with(&prefix.to_ascii_lowercase())
    } else {
        name.starts_with(prefix)
    }
}

fn command_names(prefix: &str, env: &CompleteEnv) -> Vec<String> {
    let sep = if env.windows { ';' } else { ':' };
    let exts: Vec<String> = env
        .pathext
        .as_deref()
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .filter(|e| !e.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let mut out = Vec::new();
    for dir in env.path.as_deref().unwrap_or("").split(sep) {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let name = if env.windows {
                let path = Path::new(&file_name);
                let ext = path
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()));
                match ext {
                    Some(ext) if exts.contains(&ext) => match path.file_stem() {
                        Some(stem) => stem.to_string_lossy().into_owned(),
                        None => continue,
                    },
                    _ => continue,
                }
            } else {
                if !is_executable(&entry.path()) {
                    continue;
                }
                file_name
            };
            if starts_with(&name, prefix, env.windows) {
                out.push(name);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

fn path_names(word: &str, env: &CompleteEnv) -> Vec<String> {
    let split = word.rfind(|c| is_sep(c, env.windows));
    let (dir_part, file_prefix) = match split {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word),
    };
    let base = match (dir_part.strip_prefix('~'), env.home.as_ref()) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        _ if dir_part.is_empty() => env.cwd.clone(),
        _ => env.cwd.join(dir_part),
    };
    let sep = if env.windows { '\\' } else { '/' };
    let Ok(entries) = fs::read_dir(&base) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && !file_prefix.starts_with('.') {
            continue;
        }
        if !starts_with(&name, file_prefix, env.windows) {
            continue;
        }
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        let mut cand = format!("{dir_part}{name}");
        if is_dir {
            cand.push(sep);
        }
        out.push(cand);
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::{common_prefix, complete_at, CompleteEnv};
    use std::format;
    use std::path::Path;
    use std::string::String;
    use std::vec::Vec;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn env_for(dir: &Path, path: Option<String>) -> CompleteEnv {
        CompleteEnv {
            cwd: dir.to_path_buf(),
            home: None,
            path,
            pathext: Some(".EXE;.CMD".into()),
            windows: cfg!(windows),
        }
    }

    #[test]
    fn completes_files_and_folders_in_a_temp_directory() {
        let dir = std::env::temp_dir().join(format!("krypton-comp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("alpha_dir")).expect("mkdir");
        std::fs::write(dir.join("alpha.txt"), "x").expect("write");
        std::fs::write(dir.join("beta.txt"), "x").expect("write");
        std::fs::write(dir.join("alpha_dir").join("zed.txt"), "x").expect("write");
        std::fs::write(dir.join(".hidden"), "x").expect("write");
        let env = env_for(&dir, Some(String::new()));
        let sep = if cfg!(windows) { '\\' } else { '/' };

        let line = chars("cat al");
        let c = complete_at(&line, line.len(), &env);
        assert_eq!(c.word_start, 4);
        assert_eq!(
            c.candidates,
            ["alpha.txt".to_string(), format!("alpha_dir{sep}")]
        );
        assert_eq!(common_prefix(&c.candidates), "alpha");

        let line = chars("cat alpha_dir/z");
        let c = complete_at(&line, line.len(), &env);
        // The folder part keeps the separator the user typed; only new folders get `sep`.
        assert_eq!(c.candidates, ["alpha_dir/zed.txt".to_string()]);

        let line = chars("cat .h");
        let c = complete_at(&line, line.len(), &env);
        assert_eq!(c.candidates, [".hidden".to_string()]);

        let line = chars("cat ");
        let c = complete_at(&line, line.len(), &env);
        assert!(c.candidates.contains(&"beta.txt".to_string()));
        assert!(!c.candidates.iter().any(|n| n == ".hidden"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn completes_commands_from_path_in_command_position() {
        let dir = std::env::temp_dir().join(format!("krypton-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let tool = if cfg!(windows) {
            "mytool.cmd"
        } else {
            "mytool"
        };
        std::fs::write(dir.join(tool), "x").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join(tool), std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }
        let path = dir.to_string_lossy().into_owned();
        let env = env_for(&dir, Some(path));
        let line = chars("myt");
        let c = complete_at(&line, line.len(), &env);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(c.word_start, 0);
        assert_eq!(c.candidates, ["mytool".to_string()]);
    }

    #[test]
    fn common_prefix_of_no_candidates_is_empty() {
        assert_eq!(common_prefix(&[]), "");
    }
}
