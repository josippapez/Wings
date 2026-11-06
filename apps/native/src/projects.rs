//! The projects in the sidebar: the desktop app's own list (`spaces.json`), read and never written,
//! like the Issues storage.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The fields of a desktop `Space` the sidebar shows.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Space {
    id: String,
    name: String,
    path: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    pinned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectEntry {
    pub name: String,
    pub path: PathBuf,
    /// A git worktree Wings made for an agent, listed under its project.
    pub child: bool,
    pub pinned: bool,
    pub branch: Option<String>,
}

/// Pinned projects first, then the rest, each in the desktop's order, with its worktrees right
/// under it. A missing or unreadable file lists nothing.
pub fn load(file: &Path) -> Vec<ProjectEntry> {
    let Some(spaces) = std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Space>>(&bytes).ok())
    else {
        return Vec::new();
    };
    let entry = |space: &Space, child: bool| ProjectEntry {
        name: space.name.clone(),
        path: PathBuf::from(&space.path),
        child,
        pinned: space.pinned,
        branch: branch(Path::new(&space.path)),
    };
    let tops = spaces.iter().filter(|space| {
        space
            .parent
            .as_ref()
            .is_none_or(|parent| !spaces.iter().any(|other| &other.id == parent))
    });
    let (pinned, rest): (Vec<_>, Vec<_>) = tops.partition(|space| space.pinned);
    let mut list = Vec::new();
    for top in pinned.into_iter().chain(rest) {
        list.push(entry(top, false));
        for child in spaces
            .iter()
            .filter(|space| space.parent.as_deref() == Some(top.id.as_str()))
        {
            list.push(entry(child, true));
        }
    }
    list
}

/// The branch checked out in `folder`, from git's HEAD file. A worktree's `.git` is a file naming
/// its real git folder.
pub fn branch(folder: &Path) -> Option<String> {
    let dot_git = folder.join(".git");
    let git_dir = if dot_git.is_file() {
        let link = std::fs::read_to_string(&dot_git).ok()?;
        let target = link.strip_prefix("gitdir:")?.trim();
        folder.join(target)
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
}

/// The desktop app's data folder (`dev.wings.app`), where its `spaces.json` lives.
pub fn desktop_dir() -> Option<PathBuf> {
    let env = |key| {
        std::env::var_os(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let dir = if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else if cfg!(windows) {
        env("APPDATA")?
    } else {
        env("XDG_DATA_HOME").or_else(|| Some(env("HOME")?.join(".local/share")))?
    };
    Some(dir.join("dev.wings.app"))
}

pub fn default_path() -> Option<PathBuf> {
    Some(desktop_dir()?.join("spaces.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_projects_lead_and_worktrees_sit_under_their_project() {
        let dir =
            std::env::temp_dir().join(format!("wings-native-projects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let tree = dir.join("tree");
        std::fs::create_dir_all(repo.join(".git/worktrees/tree")).unwrap();
        std::fs::write(
            repo.join(".git/worktrees/tree/HEAD"),
            "ref: refs/heads/fix-tabs\n",
        )
        .unwrap();
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(
            tree.join(".git"),
            format!("gitdir: {}\n", repo.join(".git/worktrees/tree").display()),
        )
        .unwrap();
        let file = dir.join("spaces.json");
        let spaces = serde_json::json!([
            { "id": "a", "name": "alpha", "path": "/nowhere/alpha" },
            { "id": "r", "name": "repo", "path": repo, "pinned": true },
            { "id": "t", "name": "tree", "path": tree, "parent": "r" },
            { "id": "o", "name": "orphan", "path": "/nowhere/orphan", "parent": "gone" },
        ]);
        std::fs::write(&file, serde_json::to_vec(&spaces).unwrap()).unwrap();

        let list = load(&file);
        let rows: Vec<_> = list
            .iter()
            .map(|p| (p.name.as_str(), p.child, p.branch.as_deref()))
            .collect();
        assert_eq!(
            rows,
            [
                ("repo", false, Some("main")),
                ("tree", true, Some("fix-tabs")),
                ("alpha", false, None),
                ("orphan", false, None),
            ]
        );
        assert_eq!(load(&dir.join("missing.json")), Vec::new());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
