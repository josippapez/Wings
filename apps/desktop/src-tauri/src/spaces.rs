//! Spaces are the projects in the sidebar, persisted as JSON in the app data folder.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    pub id: String,
    pub name: String,
    pub path: String,
    /// The project this folder is a git worktree of, when Wings made it for a new agent. Shown under that project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// When the project was added, in ms since the epoch. Absent for projects added before Wings kept it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_at: Option<u64>,
    /// Listed above the unpinned projects.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    /// Patterns (gitignore syntax, one per line) for the gitignored local files copied into a new worktree of this
    /// project. Unset means the default list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_include: Option<String>,
    /// A shell line run in a new worktree of this project before the first command typed there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_setup: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
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

    /// Adds a worktree Wings made, shown under `parent`.
    pub fn add_worktree(&mut self, path: &str, parent: &str) -> Space {
        if let Some(existing) = self.spaces.iter().find(|s| s.path == path) {
            return existing.clone();
        }
        let space = Space { parent: Some(parent.to_string()), ..new_space(path) };
        self.spaces.push(space.clone());
        self.save();
        space
    }

    pub fn remove(&mut self, id: &str) {
        self.spaces.retain(|s| s.id != id);
        self.save();
    }

    pub fn set_pinned(&mut self, id: &str, pinned: bool) {
        if let Some(space) = self.spaces.iter_mut().find(|s| s.id == id) {
            space.pinned = pinned;
            self.save();
        }
    }

    /// The project whose worktree settings apply to `id`: a worktree uses its project's.
    pub fn settings_owner(&self, id: &str) -> Option<&Space> {
        settings_owner_in(&self.spaces, id)
    }

    /// Blank means unset.
    pub fn set_worktree_settings(&mut self, id: &str, include: &str, setup: &str) {
        if update_worktree_settings(&mut self.spaces, id, Some(include), Some(setup)).is_ok() {
            self.save();
        }
    }

    /// Changes only the settings given, on the project `id` belongs to. Blank means unset.
    pub fn update_worktree_settings(&mut self, id: &str, include: Option<&str>, setup: Option<&str>) -> Result<(), String> {
        update_worktree_settings(&mut self.spaces, id, include, setup)?;
        self.save();
        Ok(())
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

/// The project whose worktree settings apply to `id`: a worktree uses its project's.
pub fn settings_owner_in<'a>(spaces: &'a [Space], id: &str) -> Option<&'a Space> {
    let space = spaces.iter().find(|s| s.id == id)?;
    space.parent.as_deref().and_then(|p| spaces.iter().find(|s| s.id == p)).or(Some(space))
}

/// Sets the given worktree settings (blank unsets one) on the project `id` belongs to; the other one stays.
pub fn update_worktree_settings(spaces: &mut [Space], id: &str, include: Option<&str>, setup: Option<&str>) -> Result<(), String> {
    let owner = settings_owner_in(spaces, id).ok_or("unknown space")?.id.clone();
    let space = spaces.iter_mut().find(|s| s.id == owner).ok_or("unknown space")?;
    let keep = |t: &str| (!t.trim().is_empty()).then(|| t.trim().to_string());
    if let Some(include) = include {
        space.worktree_include = keep(include);
    }
    if let Some(setup) = setup {
        space.worktree_setup = keep(setup);
    }
    Ok(())
}

/// The include list as stored text, or "" for the default: no lines, or exactly the default lines, stay unset so a
/// later change to the default reaches the project. Refuses a line a list file can't hold.
pub fn include_text(lines: &[String]) -> Result<String, String> {
    let lines: Vec<&str> = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if let Some(bad) = lines.iter().find(|l| l.chars().any(char::is_control)) {
        return Err(format!("A pattern is one line with no control characters: {bad:?}"));
    }
    Ok(if lines == crate::worktree::DEFAULT_INCLUDE { String::new() } else { lines.join("\n") })
}

/// A setup command as stored text: one shell line, or "" to clear it.
pub fn setup_text(setup: &str) -> Result<String, String> {
    let setup = setup.trim();
    // What the approval prompt shows has to be what runs: no invisible, look-alike or right-to-left characters.
    if !setup.chars().all(|c| c == ' ' || c.is_ascii_graphic()) {
        return Err("The setup command is one line of plain ASCII characters".into());
    }
    if setup.len() > SETUP_MAX {
        return Err(format!("The setup command is at most {SETUP_MAX} characters"));
    }
    Ok(setup.to_string())
}

/// So the approval prompt shows the whole command at once.
pub const SETUP_MAX: usize = 500;

/// What a project's worktrees get, as plugins and tools show it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSetup {
    pub project: String,
    pub path: String,
    /// The patterns the project keeps, or null for the default list.
    pub include: Option<Vec<String>>,
    pub defaults: Vec<String>,
    /// The repo has a `.worktreeinclude`, which is used instead of `include`.
    pub has_include_file: bool,
    pub setup: Option<String>,
}

pub fn worktree_setup(spaces: &[Space], id: &str) -> Option<WorktreeSetup> {
    let owner = settings_owner_in(spaces, id)?;
    let lines = |t: &str| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect::<Vec<_>>();
    Some(WorktreeSetup {
        project: owner.name.clone(),
        path: owner.path.clone(),
        include: owner.worktree_include.as_deref().map(lines),
        defaults: crate::worktree::DEFAULT_INCLUDE.iter().map(|s| s.to_string()).collect(),
        has_include_file: crate::worktree::has_include_file(Path::new(&owner.path)),
        setup: owner.worktree_setup.clone(),
    })
}

fn new_space(path: &str) -> Space {
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
    // The path is unique per space, so it doubles as a stable id.
    let added_at = SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis() as u64);
    Space { id: path.to_string(), name, path: path.to_string(), parent: None, added_at, pinned: false, worktree_include: None, worktree_setup: None }
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
    fn a_setup_line_is_plain_ascii_that_fits_in_the_prompt() {
        assert_eq!(setup_text("  pnpm install  ").unwrap(), "pnpm install");
        // Zero-width space, right-to-left override, a Cyrillic look-alike, a tab: shown differently from how they run.
        for bad in ["pnpm\u{200B} install", "echo \u{202E}lmth.x", "pnpm іnstall", "pnpm\tinstall"] {
            assert!(setup_text(bad).unwrap_err().contains("plain ASCII"), "{bad:?}");
        }
        assert!(setup_text(&"a".repeat(SETUP_MAX + 1)).unwrap_err().contains("at most"));
    }

    #[test]
    fn old_files_load_without_added_time_or_pin_and_new_fields_round_trip() {
        let old: Vec<Space> = serde_json::from_str(r#"[{"id":"/a","name":"a","path":"/a"}]"#).unwrap();
        assert_eq!((old[0].added_at, old[0].pinned), (None, false));
        let file = std::env::temp_dir().join(format!("wings-spaces-{}.json", std::process::id()));
        let mut store = SpaceStore { file: file.clone(), spaces: vec![] };
        let added = store.add("/x/proj");
        assert!(added.added_at.is_some_and(|t| t > 0));
        store.set_pinned("/x/proj", true);
        let loaded = SpaceStore::load(file.clone());
        assert_eq!((loaded.spaces[0].added_at, loaded.spaces[0].pinned), (added.added_at, true));
        let _ = fs::remove_file(&file);
    }

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
