//! The prompt: `cwd [branch] $`, with the home folder shown as `~` and the branch read from
//! the nearest `.git/HEAD` (no git program is run).

use std::fs;
use std::path::{Path, PathBuf};
use std::string::String;

/// Build the prompt text for `cwd`.
pub(crate) fn prompt(cwd: &Path, home: Option<&Path>) -> String {
    let dir = display_dir(cwd, home);
    match branch_of(cwd) {
        Some(branch) => format!("{dir} [{branch}] $ "),
        None => format!("{dir} $ "),
    }
}

/// The directory as shown in the prompt: `~` for home, otherwise the full path.
pub(crate) fn display_dir(cwd: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home {
        if let Ok(rest) = cwd.strip_prefix(home) {
            let rest = rest.to_string_lossy();
            return if rest.is_empty() {
                String::from("~")
            } else {
                format!("~{}{}", std::path::MAIN_SEPARATOR, rest)
            };
        }
    }
    cwd.to_string_lossy().into_owned()
}

/// Branch name of the git repository containing `start`, if any.
pub(crate) fn branch_of(start: &Path) -> Option<String> {
    let mut dir: Option<&Path> = Some(start);
    while let Some(d) = dir {
        let dot_git = d.join(".git");
        if dot_git.is_dir() {
            return read_head(&dot_git.join("HEAD"));
        }
        if dot_git.is_file() {
            // A worktree or submodule: the file holds `gitdir: <path>`.
            let text = fs::read_to_string(&dot_git).ok()?;
            let target = text.trim().strip_prefix("gitdir:")?.trim();
            let gitdir = if Path::new(target).is_absolute() {
                PathBuf::from(target)
            } else {
                d.join(target)
            };
            return read_head(&gitdir.join("HEAD"));
        }
        dir = d.parent();
    }
    None
}

fn read_head(head: &Path) -> Option<String> {
    let text = fs::read_to_string(head).ok()?;
    Some(branch_from_head(&text))
}

/// `ref: refs/heads/main` gives `main`; a detached HEAD gives the first seven characters.
pub(crate) fn branch_from_head(text: &str) -> String {
    let text = text.trim();
    match text.strip_prefix("ref:") {
        Some(reference) => {
            let reference = reference.trim();
            reference
                .strip_prefix("refs/heads/")
                .unwrap_or(reference)
                .to_string()
        }
        None => text.chars().take(7).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{branch_from_head, branch_of, display_dir};
    use std::format;
    use std::path::{Path, PathBuf};

    #[test]
    fn head_text_gives_branch_or_short_sha() {
        assert_eq!(branch_from_head("ref: refs/heads/main\n"), "main");
        assert_eq!(branch_from_head("ref: refs/heads/feat/x\n"), "feat/x");
        assert_eq!(branch_from_head("0123456789abcdef\n"), "0123456");
    }

    #[test]
    fn home_is_shown_as_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(display_dir(Path::new("/home/me"), Some(home)), "~");
        let nested = PathBuf::from("/home/me").join("work");
        assert_eq!(
            display_dir(&nested, Some(home)),
            format!("~{}work", std::path::MAIN_SEPARATOR)
        );
        assert_eq!(display_dir(Path::new("/srv"), Some(home)), "/srv");
    }

    #[test]
    fn branch_is_found_from_a_subfolder() {
        let root = std::env::temp_dir().join(format!("krypton-git-{}", std::process::id()));
        let sub = root.join("a").join("b");
        let _ = std::fs::create_dir_all(root.join(".git"));
        let _ = std::fs::create_dir_all(&sub);
        let _ = std::fs::write(root.join(".git").join("HEAD"), "ref: refs/heads/topic\n");
        let found = branch_of(&sub);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(found.as_deref(), Some("topic"));
    }
}
