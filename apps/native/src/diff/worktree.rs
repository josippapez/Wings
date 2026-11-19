//! The unchanged lines of a diff's files, read from the repository folder the diff is of, so its folds can open. Only
//! a diff whose new side is that folder's working tree asks for them (`worktree`), like the Changes plugin's, whose
//! uncommitted and branch diffs are both against the working tree. A path comes from a plugin's patch, so it is read
//! only from inside the folder, links followed, and within a size limit.

use std::path::{Component, Path, PathBuf};

use super::{
    patch::{Diff, DiffFile},
    rows::file_path,
};

/// The most read of one file, and of all the files of one patch.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
/// The most files read for one patch. The rest keep their folds closed.
pub const MAX_FILES: usize = 300;

/// The repository folder `root`, with its links followed. It must be a git repository's top folder.
pub fn repo(root: &Path) -> Result<PathBuf, String> {
    if !root.is_absolute() {
        return Err("The diff's folder isn't an absolute path".into());
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("Can't read the diff's folder: {error}"))?;
    if !root.join(".git").exists() {
        return Err("The diff's folder isn't a git repository".into());
    }
    Ok(root)
}

/// The file at `path` in the repository folder `repo` (from `repo`), refused unless it is inside it once its links
/// are followed. A path is plain names: no `..`, no root, nothing in `.git`.
pub fn inside(repo: &Path, path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(path);
    // `.GIT` is the same folder on a Mac's disk, which ignores case.
    let git = |name: &std::ffi::OsStr| name.eq_ignore_ascii_case(".git");
    let plain = !path.is_empty()
        && relative.components().all(
            |part| matches!(part, Component::Normal(name) if !git(name)),
        );
    if !plain {
        return Err(format!("{path} isn't a path inside the repository"));
    }
    let file = repo
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("Can't read {path}: {error}"))?;
    let Ok(within) = file.strip_prefix(repo) else {
        return Err(format!("{path} leads outside the repository"));
    };
    // A link inside the repository can still lead into its `.git`.
    if within.components().any(|part| matches!(part, Component::Normal(name) if git(name))) {
        return Err(format!("{path} isn't a path inside the repository"));
    }
    if !file.is_file() {
        return Err(format!("{path} isn't a file"));
    }
    Ok(file)
}

/// `inside`, from a folder that isn't known to be a repository yet.
pub fn confined(root: &Path, path: &str) -> Result<PathBuf, String> {
    inside(&repo(root)?, path)
}

/// The new side's lines of `file` in `repo`, at most `max` bytes, if they still match the patch: each line its hunks
/// have on the new side is where the patch says. Else the file changed since the patch was made, and its folds wait for
/// the next patch.
pub fn read_lines(repo: &Path, file: &DiffFile, max: u64) -> Result<Vec<String>, String> {
    let path = file
        .new_path
        .as_deref()
        .ok_or("The file has no new side")?;
    let at = inside(repo, path)?;
    let size = std::fs::metadata(&at)
        .map_err(|error| error.to_string())?
        .len();
    if size > max {
        return Err(format!("{path} is too large to read"));
    }
    let bytes = std::fs::read(&at).map_err(|error| error.to_string())?;
    // It may have grown since it was measured.
    if bytes.len() as u64 > max {
        return Err(format!("{path} is too large to read"));
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if text.is_empty() || text.ends_with('\n') {
        lines.pop();
    }
    // A hunk that says it starts past the end, as one with no new line can, is as stale as a line that doesn't match.
    let inside = file
        .hunks
        .iter()
        .all(|hunk| hunk.new_range.start <= lines.len());
    let matches = inside
        && file.hunks.iter().flat_map(|hunk| &hunk.lines).all(|line| {
            line.new_line.is_none_or(|number| {
                number
                    .checked_sub(1)
                    .and_then(|at| lines.get(at))
                    .is_some_and(|text| *text == line.text)
            })
        });
    if !matches {
        return Err(format!("{path} changed since the diff was made"));
    }
    Ok(lines)
}

/// The lines of `diff`'s files `wanted` (by their place in the patch), each with its path: an error for a file past
/// `MAX_FILES` or `MAX_TOTAL_BYTES`, outside the repository, or changed since. Slow on many files: run it off the
/// window's thread.
pub fn read_all(
    root: &Path,
    diff: &Diff,
    wanted: &[usize],
) -> Vec<(String, Result<Vec<String>, String>)> {
    let repo = repo(root);
    let mut total = 0;
    wanted
        .iter()
        .enumerate()
        .filter_map(|(n, &at)| diff.files.get(at).map(|file| (n, file)))
        .map(|(n, file)| {
            let path = file_path(file).to_owned();
            let lines = match &repo {
                Err(error) => Err(error.clone()),
                Ok(_) if n >= MAX_FILES || total >= MAX_TOTAL_BYTES => {
                    Err("Too many files to read".into())
                }
                Ok(repo) => read_lines(repo, file, MAX_FILE_BYTES.min(MAX_TOTAL_BYTES - total)),
            };
            if let Ok(lines) = &lines {
                total += lines.iter().map(|line| line.len() as u64 + 1).sum::<u64>();
            }
            (path, lines)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::patch::parse_patch;

    const PATCH: &str = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -5,3 +5,4 @@\n five\n+new six\n six\n seven\n@@ -20,3 +21,2 @@\n twenty\n-twenty-one\n twenty-two\n";

    /// A new, empty folder of its own under the temp folder, as a repository when `git`.
    fn folder(name: &str, git: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wings-worktree-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if git {
            std::fs::create_dir(dir.join(".git")).unwrap();
        }
        dir
    }

    /// The 30 lines of a.rs's new side, as `PATCH` has them.
    fn a_rs() -> String {
        (1..=30)
            .map(|n| {
                let line = match n {
                    5 => "five".to_owned(),
                    6 => "new six".to_owned(),
                    7 => "six".to_owned(),
                    8 => "seven".to_owned(),
                    21 => "twenty".to_owned(),
                    22 => "twenty-two".to_owned(),
                    n => format!("line {n}"),
                };
                line + "\n"
            })
            .collect()
    }

    #[test]
    fn a_file_is_read_when_it_still_matches_the_patch() {
        let dir = folder("read", true);
        std::fs::write(dir.join("a.rs"), a_rs()).unwrap();
        let diff = parse_patch(PATCH).unwrap();
        let read = read_all(&dir, &diff, &[0]);
        let lines = read[0].1.as_ref().unwrap();
        assert_eq!((read[0].0.as_str(), lines.len()), ("a.rs", 30));
        assert_eq!((lines[8].as_str(), lines[29].as_str()), ("line 9", "line 30"));
        // Edited since: a line inserted above the second hunk moves it.
        std::fs::write(dir.join("a.rs"), a_rs().replace("line 12\n", "line 12\nmore\n")).unwrap();
        let repo = repo(&dir).unwrap();
        assert!(read_lines(&repo, &diff.files[0], MAX_FILE_BYTES).unwrap_err().contains("changed since"));
        // Bounded.
        std::fs::write(dir.join("a.rs"), a_rs()).unwrap();
        assert!(read_lines(&repo, &diff.files[0], 10).unwrap_err().contains("too large"));
        // A hunk header saying it sits far past the file's 30 lines, with no new line to check, isn't of this file.
        let far = parse_patch("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +4000000,0 @@\n-x\n").unwrap();
        assert!(read_lines(&repo, &far.files[0], MAX_FILE_BYTES).unwrap_err().contains("changed since"));
    }

    #[test]
    fn only_files_inside_the_repository_are_read() {
        let dir = folder("confined", true);
        std::fs::create_dir(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "a").unwrap();
        let outside = folder("outside", false);
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        assert!(confined(&dir, "src/a.rs").is_ok());
        std::fs::write(dir.join(".git/config"), "[core]").unwrap();
        for path in ["", "../outside/secret.txt", "src/../../x", "/etc/hosts", ".git/config", ".GIT/config", ".Git/config", "src"] {
            assert!(confined(&dir, path).is_err(), "{path} was read");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.join("secret.txt"), dir.join("link.txt")).unwrap();
            assert!(confined(&dir, "link.txt").unwrap_err().contains("outside"));
            std::os::unix::fs::symlink(dir.join(".git/config"), dir.join("config.txt")).unwrap();
            assert!(confined(&dir, "config.txt").is_err(), "a link into .git was read");
        }
        // A folder that isn't a repository, or not an absolute path, reads nothing.
        assert!(confined(&outside, "secret.txt").is_err());
        assert!(confined(Path::new("relative"), "a.rs").is_err());
    }
}
