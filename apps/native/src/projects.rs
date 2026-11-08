//! The projects in the sidebar: the native app's own list (`spaces.json` beside its workspace file), changed when a
//! plugin makes a worktree or sets a project's worktree setup. The desktop app's list is copied in once, on the first
//! run, and never read or written again: the desktop loads its file at startup and saves from that copy, so anything
//! written there would be lost.

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use wings_spaces::{
    claude_accounts,
    spaces::{self, Space, SpaceStore, WorktreeSetup},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectEntry {
    pub name: String,
    pub path: PathBuf,
    /// A git worktree Wings made for an agent, listed under its project.
    pub child: bool,
    pub pinned: bool,
    pub branch: Option<String>,
    /// When the project was added, in ms since the epoch.
    pub added: Option<u64>,
}

/// The native app's projects, shared by the sidebar and the plugins' host calls.
pub struct Projects {
    store: Mutex<SpaceStore>,
    /// The folder the app was opened in. It is listed even when the list doesn't hold it, and joins the list the first
    /// time a plugin changes something about it.
    launch: PathBuf,
    /// Set when the folder the app was opened in is removed from the sidebar, so it is no longer listed unasked.
    launch_removed: AtomicBool,
    /// A new worktree's setup line, until the first pane a plugin opens there types it.
    setup: Mutex<HashMap<PathBuf, String>>,
}

impl Projects {
    /// The list in `file`, or one kept in memory without a file.
    pub fn open(file: Option<PathBuf>, launch: PathBuf) -> Self {
        Self {
            store: Mutex::new(file.map_or_else(SpaceStore::in_memory, SpaceStore::load)),
            launch,
            launch_removed: AtomicBool::new(false),
            setup: Mutex::default(),
        }
    }

    /// Lists the folder, or finds it listed already.
    pub fn add(&self, folder: &str) -> Space {
        self.store.lock().unwrap().add(folder)
    }

    /// Takes the project in folder `project` off the list; its folders and worktrees stay on disk. The folder the app was
    /// opened in stays off the list too, while the app runs.
    pub fn remove(&self, project: &str) {
        let mut store = self.store.lock().unwrap();
        let id = store
            .spaces
            .iter()
            .find(|space| space.path == project)
            .map(|space| space.id.clone());
        if let Some(id) = id {
            store.remove(&id);
        }
        if Path::new(project) == self.launch {
            self.launch_removed.store(true, Ordering::Relaxed);
        }
    }

    /// Pins the project in folder `project` above the others, or unpins it. The folder the app was opened in joins the
    /// list when it is pinned.
    pub fn set_pinned(&self, project: &str, pinned: bool) {
        let mut store = self.store.lock().unwrap();
        let id = self.id_of(&mut store, project, true);
        store.set_pinned(&id, pinned);
    }

    /// Moves the project in folder `project` to the place of the one in folder `slot`, as the desktop's `onMoveSpace`,
    /// and saves the list; its worktrees stay under it. The folder the app was opened in joins the list where it is
    /// shown, first, when it is one of the two.
    pub fn move_project(&self, project: &str, slot: &str) {
        let mut store = self.store.lock().unwrap();
        if self.unlisted_launch(&store)
            && [project, slot]
                .iter()
                .any(|folder| Path::new(folder) == self.launch)
        {
            store
                .spaces
                .insert(0, spaces::new_space(&self.launch.to_string_lossy()));
        }
        let id = |store: &SpaceStore, folder: &str| {
            store
                .spaces
                .iter()
                .find(|space| space.path == folder)
                .map(|space| space.id.clone())
        };
        if let (Some(project), Some(slot)) = (id(&store, project), id(&store, slot)) {
            store.reorder(&project, &slot);
        }
    }

    /// Pinned projects first, then the rest, each in the list's order, with its worktrees right under it; the folder
    /// the app was opened in leads when the list doesn't hold it.
    pub fn entries(&self) -> Vec<ProjectEntry> {
        let store = self.store.lock().unwrap();
        let mut list = list(&store.spaces);
        if self.unlisted_launch(&store) {
            list.insert(
                0,
                entry(&spaces::new_space(&self.launch.to_string_lossy()), false),
            );
        }
        list
    }

    /// The folders of the projects, which `exec` may run in.
    pub fn paths(&self) -> Vec<PathBuf> {
        let store = self.store.lock().unwrap();
        let unlisted = self.unlisted_launch(&store).then(|| self.launch.clone());
        unlisted
            .into_iter()
            .chain(store.spaces.iter().map(|space| PathBuf::from(&space.path)))
            .collect()
    }

    /// The projects as the list holds them, the folder the app was opened in first when the list doesn't hold it: what
    /// Claude Code's tools and the accounts to connect go by.
    pub fn spaces(&self) -> Vec<Space> {
        let store = self.store.lock().unwrap();
        let unlisted = self
            .unlisted_launch(&store)
            .then(|| spaces::new_space(&self.launch.to_string_lossy()));
        unlisted
            .into_iter()
            .chain(store.spaces.iter().cloned())
            .collect()
    }

    /// Whether the folder the app was opened in is shown without being on the list.
    fn unlisted_launch(&self, store: &SpaceStore) -> bool {
        !self.listed(store) && !self.launch_removed.load(Ordering::Relaxed)
    }

    fn listed(&self, store: &SpaceStore) -> bool {
        store
            .spaces
            .iter()
            .any(|space| Path::new(&space.path) == self.launch)
    }

    /// The id of the listed project in folder `project`. A folder the list doesn't hold stays as it is, for the
    /// store's own "no such project"; the folder the app was opened in joins the list when `adopt`.
    fn id_of(&self, store: &mut SpaceStore, project: &str, adopt: bool) -> String {
        match store.spaces.iter().find(|space| space.path == project) {
            Some(space) => space.id.clone(),
            None if adopt && Path::new(project) == self.launch => store.add(project).id,
            None => project.to_owned(),
        }
    }

    /// Makes a worktree of the project in folder `project` and lists it under it. A project's setup line is kept for
    /// the first pane opened in the worktree.
    pub fn create_worktree(
        &self,
        project: &str,
        branch: &str,
        base: Option<&str>,
    ) -> Result<Space, String> {
        let id = self.id_of(&mut self.store.lock().unwrap(), project, true);
        let made = spaces::make_worktree(&self.store, &id, branch, base)?;
        if let Some(line) = made.setup {
            self.setup
                .lock()
                .unwrap()
                .insert(PathBuf::from(&made.space.path), line);
        }
        Ok(made.space)
    }

    pub fn worktree_setup(&self, project: &str) -> Result<WorktreeSetup, String> {
        let mut store = self.store.lock().unwrap();
        let id = self.id_of(&mut store, project, false);
        spaces::worktree_setup(&store.spaces, &id)
            .or_else(|| {
                (Path::new(project) == self.launch)
                    .then(|| spaces::worktree_setup(&[spaces::new_space(project)], project))
                    .flatten()
            })
            .ok_or_else(|| "unknown space".to_string())
    }

    /// Include pattern lines (none resets to the default list) and a setup line (blank clears it), each only when given.
    pub fn set_worktree_setup(
        &self,
        project: &str,
        include: Option<Vec<String>>,
        setup: Option<String>,
    ) -> Result<(), String> {
        let include = include
            .map(|lines| spaces::include_text(&lines))
            .transpose()?;
        let setup = setup.map(|line| spaces::setup_text(&line)).transpose()?;
        self.update_worktree_settings(project, include.as_deref(), setup.as_deref())
    }

    /// Saves the include text and the setup line already checked (`spaces::include_text`, `spaces::setup_text`).
    pub fn update_worktree_settings(
        &self,
        project: &str,
        include: Option<&str>,
        setup: Option<&str>,
    ) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        let id = self.id_of(&mut store, project, true);
        store.update_worktree_settings(&id, include, setup)
    }

    /// The setup line waiting for the first pane in the worktree `project`, which now has it.
    pub fn take_setup(&self, project: &Path) -> Option<String> {
        self.setup.lock().unwrap().remove(project)
    }

    /// The folder for `CLAUDE_CONFIG_DIR` in a new terminal of the project in folder `project`: its account's, or `None`
    /// for the default account.
    pub fn claude_config_dir(&self, project: &Path) -> Option<PathBuf> {
        let store = self.store.lock().unwrap();
        let space = store
            .spaces
            .iter()
            .find(|space| Path::new(&space.path) == project)?;
        claude_accounts::pane_dir(&store.spaces, &space.id)
    }

    /// The Claude Code accounts and the one each project uses, by the project's folder, which is its id to plugins.
    /// `auth` says whether an account is signed in, `None` being the default one; it is asked of each side by side.
    pub fn claude_accounts(
        &self,
        auth: impl Fn(Option<&str>) -> claude_accounts::AuthStatus + Sync,
    ) -> claude_accounts::Accounts {
        let spaces: Vec<Space> = self
            .store
            .lock()
            .unwrap()
            .spaces
            .iter()
            .map(|space| Space {
                id: space.path.clone(),
                ..space.clone()
            })
            .collect();
        claude_accounts::list_with(&spaces, auth)
    }

    /// Makes new terminals in the project in folder `project` use the account `name`, or the default account with
    /// `None`. `make_dir` makes the folder of a named account, once the project is known to be one. The name is the
    /// caller's to check.
    pub fn set_claude_account(
        &self,
        project: &str,
        name: Option<&str>,
        make_dir: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        let id = self.id_of(&mut store, project, true);
        store.get(&id).ok_or("No such project in Wings")?;
        if let Some(name) = name {
            make_dir(name)?;
        }
        store.set_claude_account(&id, name)
    }
}

fn entry(space: &Space, child: bool) -> ProjectEntry {
    ProjectEntry {
        name: space.name.clone(),
        path: PathBuf::from(&space.path),
        child,
        pinned: space.pinned,
        branch: branch(Path::new(&space.path)),
        added: space.added_at,
    }
}

fn list(spaces: &[Space]) -> Vec<ProjectEntry> {
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

/// What to type into the first pane of a worktree so its project's setup line runs before what was asked for, as the
/// desktop's `withSetup` does. `sh -c` keeps the line whole whatever the user's shell is, and `&&` starts the command only
/// when setup succeeded. A command typed without its Enter is left alone.
pub fn with_setup(setup: Option<&str>, input: Option<String>) -> Option<String> {
    let Some(line) = setup.map(str::trim).filter(|line| !line.is_empty()) else {
        return input;
    };
    let run = format!("sh -c {}", shell_quote(line));
    match input {
        None => Some(run + "\r"),
        Some(input) if input.ends_with('\r') => Some(format!("{run} && {input}")),
        input => input,
    }
}

/// The desktop's `shellQuote`: a plain word stays as it is, anything else is single-quoted. A leading `=` is zsh's command
/// path expansion.
pub fn shell_quote(arg: &str) -> String {
    let plain = |(index, c): (usize, char)| {
        c.is_ascii_alphanumeric() || "_@%+:,./-".contains(c) || (index > 0 && c == '=')
    };
    if !arg.is_empty() && arg.chars().enumerate().all(plain) {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
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

/// The desktop app's data folder (`dev.wings.app`).
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

/// The desktop app's list, which the native one starts as a copy of.
pub fn desktop_path() -> Option<PathBuf> {
    Some(desktop_dir()?.join("spaces.json"))
}

/// The native app's own list, beside its workspace file.
pub fn default_path() -> Option<PathBuf> {
    Some(crate::saved::default_path()?.with_file_name("spaces.json"))
}

/// On the first run, when the native list doesn't exist yet, copies the desktop's in. After that the native list is
/// the only one used, even if it is empty. Returns whether it copied.
pub fn seed(native: &Path, desktop: &Path) -> io::Result<bool> {
    if native.exists() || !desktop.is_file() {
        return Ok(false);
    }
    if let Some(dir) = native.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Copied beside it and renamed, so a crash mid-copy can't leave half a list that counts as the native one.
    let tmp = native.with_extension("json.tmp");
    std::fs::copy(desktop, &tmp)?;
    std::fs::rename(tmp, native)?;
    Ok(true)
}

/// An empty folder of its own in the system's temp folder, by its real path (/var is /private/var on macOS).
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> PathBuf {
    static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "wings-native-{name}-{}-{count}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// A real git repo `app` in `dir` on `main` with one commit, for tests that make worktrees.
#[cfg(test)]
pub(crate) fn test_repo(dir: &Path) -> PathBuf {
    let repo = dir.join("app");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        ["init", "-b", "main"].as_slice(),
        &["commit", "--allow-empty", "-m", "first"],
    ] {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&repo)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "{args:?}");
    }
    repo
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> PathBuf {
        test_dir(name)
    }

    #[test]
    fn pinned_projects_lead_and_worktrees_sit_under_their_project() {
        let dir = temp("projects");
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
        let spaces = json!([
            { "id": "a", "name": "alpha", "path": "/nowhere/alpha" },
            { "id": "r", "name": "repo", "path": repo, "pinned": true },
            { "id": "t", "name": "tree", "path": tree, "parent": "r" },
            { "id": "o", "name": "orphan", "path": "/nowhere/orphan", "parent": "gone" },
        ]);
        std::fs::write(&file, serde_json::to_vec(&spaces).unwrap()).unwrap();

        let projects = Projects::open(Some(file), "/nowhere/alpha".into());
        let rows: Vec<_> = projects
            .entries()
            .iter()
            .map(|p| (p.name.clone(), p.child, p.branch.clone()))
            .collect();
        assert_eq!(
            rows,
            [
                ("repo".to_owned(), false, Some("main".to_owned())),
                ("tree".to_owned(), true, Some("fix-tabs".to_owned())),
                ("alpha".to_owned(), false, None),
                ("orphan".to_owned(), false, None),
            ]
        );
        // A list that is missing lists only the folder the app opened.
        let alone = Projects::open(Some(dir.join("missing.json")), "/nowhere/launch".into());
        assert_eq!(
            alone
                .entries()
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["launch"]
        );
        assert_eq!(alone.paths(), [PathBuf::from("/nowhere/launch")]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_first_run_copies_the_desktops_list_and_later_runs_never_touch_it_again() {
        let dir = temp("seed");
        let (desktop, native) = (
            dir.join("desktop/spaces.json"),
            dir.join("native/spaces.json"),
        );
        // No desktop list yet: nothing to copy, and no native file made.
        assert!(!seed(&native, &desktop).unwrap());
        assert!(!native.exists());

        std::fs::create_dir_all(desktop.parent().unwrap()).unwrap();
        std::fs::write(&desktop, r#"[{"id":"/a","name":"a","path":"/a"}]"#).unwrap();
        assert!(seed(&native, &desktop).unwrap());
        assert_eq!(
            std::fs::read(&native).unwrap(),
            std::fs::read(&desktop).unwrap()
        );
        assert!(!native.with_extension("json.tmp").exists());

        // The native list changes and the desktop's does too: neither is read into the other.
        std::fs::write(&native, "[]").unwrap();
        std::fs::write(&desktop, r#"[{"id":"/b","name":"b","path":"/b"}]"#).unwrap();
        assert!(!seed(&native, &desktop).unwrap());
        assert_eq!(std::fs::read_to_string(&native).unwrap(), "[]");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_worktree_joins_its_project_in_the_native_list_and_never_the_desktops() {
        if wings_plugins::exec::find_program("git").is_none() {
            return;
        }
        let dir = temp("worktree");
        let repo = test_repo(&dir);
        let (desktop, native) = (dir.join("desktop.json"), dir.join("native.json"));
        let list =
            json!([{ "id": repo, "name": "app", "path": repo, "worktreeSetup": "pnpm install" }]);
        std::fs::write(&desktop, serde_json::to_vec(&list).unwrap()).unwrap();
        seed(&native, &desktop).unwrap();

        let projects = Projects::open(Some(native.clone()), repo.clone());
        let made = projects
            .create_worktree(repo.to_str().unwrap(), "wings/one", None)
            .unwrap();
        assert_eq!(
            made.path,
            dir.join("app-worktrees/wings-one").to_str().unwrap()
        );
        let rows: Vec<_> = projects
            .entries()
            .iter()
            .map(|p| (p.name.clone(), p.child, p.branch.clone()))
            .collect();
        assert_eq!(
            rows,
            [
                ("app".to_owned(), false, Some("main".to_owned())),
                ("wings-one".to_owned(), true, Some("wings/one".to_owned()))
            ]
        );
        assert_eq!(projects.paths(), [repo.clone(), PathBuf::from(&made.path)]);
        // Saved in the native file, not the desktop's.
        let saved: Vec<Space> = serde_json::from_slice(&std::fs::read(&native).unwrap()).unwrap();
        assert_eq!(saved.len(), 2);
        let desktop_list: Vec<Space> =
            serde_json::from_slice(&std::fs::read(&desktop).unwrap()).unwrap();
        assert_eq!(desktop_list.len(), 1);
        // The project's setup line waits for the worktree's first pane, once.
        assert_eq!(
            projects.take_setup(Path::new(&made.path)).as_deref(),
            Some("pnpm install")
        );
        assert_eq!(projects.take_setup(Path::new(&made.path)), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_folder_the_app_opened_joins_the_list_when_something_is_done_to_it() {
        let dir = temp("launch");
        let file = dir.join("spaces.json");
        let launch = dir.join("opened");
        let projects = Projects::open(Some(file.clone()), launch.clone());
        let project = launch.to_str().unwrap();
        // Reading its setup shows the defaults and lists nothing.
        let setup = projects.worktree_setup(project).unwrap();
        assert_eq!((setup.include, setup.setup), (None, None));
        assert!(!file.exists());
        projects
            .set_worktree_setup(project, None, Some("make".into()))
            .unwrap();
        assert_eq!(
            projects.worktree_setup(project).unwrap().setup.as_deref(),
            Some("make")
        );
        let saved: Vec<Space> = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            saved.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(),
            [project]
        );
        // A folder that was never listed is not a project.
        assert_eq!(
            projects.worktree_setup("/elsewhere").unwrap_err(),
            "unknown space"
        );
        assert_eq!(
            projects
                .set_worktree_setup("/elsewhere", None, Some("make".into()))
                .unwrap_err(),
            "unknown space"
        );
        assert_eq!(
            projects
                .create_worktree("/elsewhere", "x", None)
                .unwrap_err(),
            "No such project in Wings"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_projects_account_is_kept_by_its_folder_and_only_for_a_project_that_is_one() {
        let dir = temp("account");
        let file = dir.join("spaces.json");
        let (alpha, beta) = (dir.join("alpha"), dir.join("beta"));
        // The ids are not the folders here, as in a list edited by hand: plugins and panes name a project by its folder.
        let list = json!([
            { "id": "a", "name": "alpha", "path": alpha },
            { "id": "b", "name": "beta", "path": beta, "claudeAccount": "../etc" },
        ]);
        std::fs::write(&file, serde_json::to_vec(&list).unwrap()).unwrap();
        let projects = Projects::open(Some(file.clone()), alpha.clone());
        let folder = |path: &Path| path.to_str().unwrap().to_owned();
        let work = claude_accounts::dir(Some("work"));

        projects
            .set_claude_account(&folder(&alpha), Some("work"), |_| Ok(()))
            .unwrap();
        assert_eq!(projects.claude_config_dir(&alpha), Some(work));
        assert_eq!(
            projects.claude_accounts(|_| Default::default()).projects,
            [(folder(&alpha), "work".to_owned())].into()
        );
        // A name that isn't one, from a list edited by hand, is no account. Neither is a folder the list lacks.
        assert_eq!(projects.claude_config_dir(&beta), None);
        assert_eq!(projects.claude_config_dir(&dir), None);

        // Its folder is made once the project is known, and a folder that can't be made leaves the account as it was.
        let made = std::cell::RefCell::new(Vec::new());
        let make = |name: &str| {
            made.borrow_mut().push(name.to_owned());
            Ok(())
        };
        assert_eq!(
            projects
                .set_claude_account("/elsewhere", Some("home"), make)
                .unwrap_err(),
            "No such project in Wings"
        );
        assert!(made.borrow().is_empty());
        assert_eq!(
            projects
                .set_claude_account(&folder(&beta), Some("home"), |_| Err("no room".into()))
                .unwrap_err(),
            "no room"
        );
        projects
            .set_claude_account(&folder(&beta), Some("home"), make)
            .unwrap();
        assert_eq!(*made.borrow(), ["home"]);
        assert_eq!(
            projects.claude_config_dir(&beta),
            Some(claude_accounts::dir(Some("home")))
        );

        // Saved, and gone with `None`, which makes no folder.
        let saved: Vec<Space> = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved[0].claude_account.as_deref(), Some("work"));
        projects
            .set_claude_account(&folder(&alpha), None, |_| {
                panic!("no folder for the default")
            })
            .unwrap();
        assert_eq!(projects.claude_config_dir(&alpha), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_worktrees_first_pane_runs_the_setup_line_before_what_was_asked_for() {
        assert_eq!(
            with_setup(None, Some("claude\r".into())).as_deref(),
            Some("claude\r")
        );
        assert_eq!(with_setup(Some("  "), None), None);
        assert_eq!(
            with_setup(Some("pnpm install"), None).as_deref(),
            Some("sh -c 'pnpm install'\r")
        );
        assert_eq!(
            with_setup(Some("pnpm install"), Some("claude\r".into())).as_deref(),
            Some("sh -c 'pnpm install' && claude\r")
        );
        // The line stays whole whatever it holds.
        assert_eq!(
            with_setup(Some("echo 'a' || true"), None).as_deref(),
            Some("sh -c 'echo '\\''a'\\'' || true'\r")
        );
        assert_eq!(
            with_setup(Some("make"), None).as_deref(),
            Some("sh -c make\r")
        );
        // A command without its Enter is left alone.
        assert_eq!(
            with_setup(Some("make"), Some("claude".into())).as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn a_moved_project_takes_the_place_of_another_with_its_worktrees_under_it() {
        let dir = temp("move");
        let file = dir.join("spaces.json");
        let list = json!([
            { "id": "a", "name": "alpha", "path": "/nowhere/alpha" },
            { "id": "b", "name": "beta", "path": "/nowhere/beta" },
            { "id": "t", "name": "beta-fix", "path": "/nowhere/beta-fix", "parent": "b" },
            { "id": "c", "name": "gamma", "path": "/nowhere/gamma" },
        ]);
        std::fs::write(&file, serde_json::to_vec(&list).unwrap()).unwrap();
        let projects = Projects::open(Some(file.clone()), "/nowhere/launch".into());
        let names = || {
            projects
                .entries()
                .iter()
                .map(|entry| entry.name.clone())
                .collect::<Vec<_>>()
        };
        projects.move_project("/nowhere/gamma", "/nowhere/alpha");
        assert_eq!(names(), ["launch", "gamma", "alpha", "beta", "beta-fix"]);
        // The folder the app was opened in, shown first without being listed, joins the list there.
        projects.move_project("/nowhere/beta", "/nowhere/launch");
        assert_eq!(names(), ["beta", "beta-fix", "launch", "gamma", "alpha"]);
        let saved: Vec<Space> = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            saved
                .iter()
                .map(|space| space.name.as_str())
                .collect::<Vec<_>>(),
            ["beta", "launch", "gamma", "alpha", "beta-fix"]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn adding_and_removing_projects_change_the_native_list_and_never_the_desktops() {
        let dir = temp("add-remove");
        let (desktop, native) = (dir.join("desktop.json"), dir.join("native.json"));
        std::fs::write(&desktop, r#"[{"id":"/a","name":"a","path":"/a"}]"#).unwrap();
        seed(&native, &desktop).unwrap();
        let desktop_before = std::fs::read(&desktop).unwrap();

        let projects = Projects::open(Some(native.clone()), "/launch".into());
        let names = |projects: &Projects| {
            projects
                .entries()
                .iter()
                .map(|entry| entry.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(projects.add("/b").name, "b");
        // A folder listed twice is listed once.
        projects.add("/b");
        assert_eq!(names(&projects), ["launch", "a", "b"]);
        projects.remove("/a");
        assert_eq!(names(&projects), ["launch", "b"]);
        // The folder the app was opened in is listed without being in the file, and removing it hides it.
        projects.remove("/launch");
        assert_eq!(names(&projects), ["b"]);
        assert_eq!(projects.paths(), [PathBuf::from("/b")]);

        let saved: Vec<Space> = serde_json::from_slice(&std::fs::read(&native).unwrap()).unwrap();
        assert_eq!(
            saved
                .iter()
                .map(|space| space.path.as_str())
                .collect::<Vec<_>>(),
            ["/b"]
        );
        assert_eq!(std::fs::read(&desktop).unwrap(), desktop_before);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
