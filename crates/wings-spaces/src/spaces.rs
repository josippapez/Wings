//! Spaces are the projects in the sidebar, persisted as JSON in the app data folder.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::worktree;

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
    /// The Claude Code account (`claude_accounts`) this project's new terminals use. Unset means the default one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_account: Option<String>,
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
    /// `None` keeps the list in memory only.
    file: Option<PathBuf>,
    pub spaces: Vec<Space>,
}

impl SpaceStore {
    /// Loads the projects you added. Wings keeps its own list; it never fills it from other apps' data.
    pub fn load(file: PathBuf) -> Self {
        let saved = fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok());
        Self { file: Some(file), spaces: saved.unwrap_or_default() }
    }

    /// An empty list that is never written anywhere.
    pub fn in_memory() -> Self {
        Self { file: None, spaces: Vec::new() }
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

    /// Sets the account new terminals of the project use, or clears it with `None`. The caller checks the name.
    pub fn set_claude_account(&mut self, id: &str, name: Option<&str>) -> Result<(), String> {
        let space = self.spaces.iter_mut().find(|s| s.id == id).ok_or("No such project in Wings")?;
        space.claude_account = name.map(String::from);
        self.save();
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Space> {
        self.spaces.iter().find(|s| s.id == id)
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.spaces) {
            let _ = fs::write(file, json);
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
    Ok(if lines == worktree::DEFAULT_INCLUDE { String::new() } else { lines.join("\n") })
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
        defaults: worktree::DEFAULT_INCLUDE.iter().map(|s| s.to_string()).collect(),
        has_include_file: worktree::has_include_file(Path::new(&owner.path)),
        setup: owner.worktree_setup.clone(),
    })
}

pub fn new_space(path: &str) -> Space {
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
    // The path is unique per space, so it doubles as a stable id.
    let added_at = SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis() as u64);
    Space {
        id: path.to_string(),
        name,
        path: path.to_string(),
        parent: None,
        added_at,
        pinned: false,
        worktree_include: None,
        worktree_setup: None,
        claude_account: None,
    }
}

/// A worktree Wings made: its project, what was copied into it, and the setup line to run in it first.
#[derive(Debug, Clone)]
pub struct Made {
    pub space: Space,
    pub local: worktree::LocalCopy,
    pub setup: Option<String>,
}

/// Makes the worktree of `space_id` on a new branch from `base` (the repo's default branch without one), copies the
/// project's gitignored local setup into it, and adds it to the store as a project under `space_id`. Git runs without
/// the store locked.
pub fn make_worktree(store: &Mutex<SpaceStore>, space_id: &str, branch: &str, base: Option<&str>) -> Result<Made, String> {
    let (path, include, setup) = {
        let store = store.lock().unwrap();
        let path = store.get(space_id).ok_or("No such project in Wings")?.path.clone();
        let owner = store.settings_owner(space_id);
        (path, owner.and_then(|s| s.worktree_include.clone()), owner.and_then(|s| s.worktree_setup.clone()))
    };
    let dir = Path::new(&path);
    let base = match base {
        Some(base) => base.to_string(),
        None => worktree::info(dir)?.default_branch.ok_or("This repo has no default branch to start from: pass base")?,
    };
    let made = worktree::create(dir, branch, &base, include.as_deref())?;
    let space = store.lock().unwrap().add_worktree(&made.path.to_string_lossy(), space_id);
    Ok(Made { space, local: made.local, setup })
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
        let mut store = SpaceStore { file: Some(file.clone()), spaces: vec![] };
        let added = store.add("/x/proj");
        assert!(added.added_at.is_some_and(|t| t > 0));
        store.set_pinned("/x/proj", true);
        let loaded = SpaceStore::load(file.clone());
        assert_eq!((loaded.spaces[0].added_at, loaded.spaces[0].pinned), (added.added_at, true));
        let _ = fs::remove_file(&file);
    }

    /// A real repo and a real `git worktree add`: the worktree is a project under the one it was made of, in the file.
    #[test]
    fn a_made_worktree_is_a_project_under_its_project_and_carries_its_setup_line() {
        if wings_plugins::exec::find_program("git").is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("wings-spaces-make-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        // git reports real paths: /var is /private/var on macOS.
        fs::create_dir_all(base.join("app")).unwrap();
        let repo = base.canonicalize().unwrap().join("app");
        for args in [["init", "-b", "main"].as_slice(), &["commit", "--allow-empty", "-m", "first"]] {
            let ok = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .current_dir(&repo)
                .stdout(Stdio::null())
                .status()
                .unwrap()
                .success();
            assert!(ok, "{args:?}");
        }
        let file = base.join("spaces.json");
        let store = Mutex::new(SpaceStore::load(file.clone()));
        let id = store.lock().unwrap().add(repo.to_str().unwrap()).id;
        store.lock().unwrap().update_worktree_settings(&id, None, Some("pnpm install")).unwrap();

        let made = make_worktree(&store, &id, "wings/one", None).unwrap();
        assert_eq!(made.space.path, repo.parent().unwrap().join("app-worktrees/wings-one").to_str().unwrap());
        assert_eq!((made.space.parent.as_deref(), made.setup.as_deref()), (Some(id.as_str()), Some("pnpm install")));
        let saved = SpaceStore::load(file);
        assert_eq!(saved.spaces.iter().map(|s| s.parent.as_deref()).collect::<Vec<_>>(), [None, Some(id.as_str())]);
        assert_eq!(make_worktree(&store, &id, "wings/one", None).unwrap_err(), format!("{} already exists", made.space.path));
        assert_eq!(make_worktree(&store, "/nowhere", "wings/two", None).unwrap_err(), "No such project in Wings");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_projects_claude_account_survives_a_restart() {
        let file = std::env::temp_dir().join(format!("wings-spaces-account-{}.json", std::process::id()));
        let mut store = SpaceStore { file: Some(file.clone()), spaces: vec![] };
        store.add("/x/proj");
        store.set_claude_account("/x/proj", Some("work")).unwrap();
        assert_eq!(SpaceStore::load(file.clone()).spaces[0].claude_account.as_deref(), Some("work"));
        store.set_claude_account("/x/proj", None).unwrap();
        assert_eq!(SpaceStore::load(file.clone()).spaces[0].claude_account, None);
        assert!(store.set_claude_account("/nowhere", Some("work")).is_err());
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
