//! Spaces are the projects in the sidebar, persisted as JSON in the app data folder.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceView {
    #[serde(flatten)]
    pub space: Space,
    pub branch: Option<String>,
}

pub struct SpaceStore {
    file: PathBuf,
    pub spaces: Vec<Space>,
}

impl SpaceStore {
    /// Loads the projects you added. Wings keeps its own list; it never fills it from other apps' data.
    pub fn load(file: PathBuf) -> Self {
        let saved = fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok());
        Self { file, spaces: saved.unwrap_or_default() }
    }

    pub fn add(&mut self, path: &str) -> Space {
        if let Some(existing) = self.spaces.iter().find(|s| s.path == path) {
            return existing.clone();
        }
        let space = new_space(path);
        self.spaces.push(space.clone());
        self.save();
        space
    }

    pub fn remove(&mut self, id: &str) {
        self.spaces.retain(|s| s.id != id);
        self.save();
    }

    pub fn get(&self, id: &str) -> Option<&Space> {
        self.spaces.iter().find(|s| s.id == id)
    }

    fn save(&self) {
        if let Some(dir) = self.file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.spaces) {
            let _ = fs::write(&self.file, json);
        }
    }
}

fn new_space(path: &str) -> Space {
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
    // The path is unique per space, so it doubles as a stable id.
    Space { id: path.to_string(), name, path: path.to_string() }
}

pub fn view(space: &Space) -> SpaceView {
    SpaceView { space: space.clone(), branch: git_branch(Path::new(&space.path)) }
}

/// Current branch of the repo containing `dir`, read from `.git/HEAD` (worktrees use a `.git` file).
pub fn git_branch(dir: &Path) -> Option<String> {
    let dot_git = dir.ancestors().map(|d| d.join(".git")).find(|p| p.exists())?;
    let git_dir = if dot_git.is_file() {
        let text = fs::read_to_string(&dot_git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        dot_git.parent()?.join(target)
    } else {
        dot_git
    };
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => branch.to_string(),
        None => head.chars().take(7).collect(),
    })
}

/// What `git status` says about a project: commits to push and pull (as of the last fetch) and changed files.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changed: u32,
}

/// `None` when `dir` isn't in a repo. Optional locks are off, so this never refreshes the index or
/// gets in the way of your own git commands.
pub fn git_status(git: &Path, dir: &Path) -> Option<GitStatus> {
    let out = Command::new(git)
        .args(["status", "--porcelain=v2", "--branch"])
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| parse_status(&String::from_utf8_lossy(&out.stdout)))
}

fn parse_status(text: &str) -> GitStatus {
    let mut status = GitStatus::default();
    let mut oid = None;
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            status.branch = (head != "(detached)").then(|| head.to_string());
        } else if let Some(id) = line.strip_prefix("# branch.oid ") {
            oid = Some(id.chars().take(7).collect());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut counts = ab.split(' ').map(|n| n.trim_start_matches(['+', '-']).parse().unwrap_or(0));
            (status.ahead, status.behind) = (counts.next().unwrap_or(0), counts.next().unwrap_or(0));
        } else if !line.starts_with('#') {
            status.changed += 1;
        }
    }
    status.branch = status.branch.or(oid);
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2_status() {
        let text = "# branch.oid 2ae5a3d54c\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -3\n1 .M N... 100644 100644 100644 a b src/x.ts\n? tmp/\n";
        assert_eq!(parse_status(text), GitStatus { branch: Some("main".into()), ahead: 2, behind: 3, changed: 2 });
        let detached = parse_status("# branch.oid 2ae5a3d54c\n# branch.head (detached)\n");
        assert_eq!(detached, GitStatus { branch: Some("2ae5a3d".into()), ..Default::default() });
    }

    #[test]
    fn reads_branch_from_repo_and_worktree() {
        let root = std::env::temp_dir().join(format!("wings-git-{}", std::process::id()));
        let repo = root.join("repo");
        fs::create_dir_all(repo.join(".git").join("worktrees").join("wt")).unwrap();
        fs::create_dir_all(repo.join("sub")).unwrap();
        fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_eq!(git_branch(&repo.join("sub")).as_deref(), Some("main"));

        let wt = root.join("wt");
        fs::create_dir_all(&wt).unwrap();
        fs::write(repo.join(".git/worktrees/wt/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", repo.join(".git/worktrees/wt").display())).unwrap();
        assert_eq!(git_branch(&wt).as_deref(), Some("feature/x"));
        let _ = fs::remove_dir_all(&root);
    }
}
