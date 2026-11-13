use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use wings_spaces::claude_accounts;

pub const USAGE: &str = "Usage: wings-native [--project PATH] [--plugins-dev-root PATH]\n\n--project PATH              Project's saved absolute path (default: current directory)\n--plugins-dev-root PATH     Also run the plugins in PATH, like the repo's plugins/, with no approval (debug builds)\n--help                     Show this help";

/// Where the desktop app keeps its plugins, which the native app reads and never writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginPaths {
    /// The installed plugins, one folder each (`plugins/`).
    pub dir: PathBuf,
    /// Which plugins are turned on and approved, and each one's storage scope (`plugins.json`).
    pub state: PathBuf,
    /// What plugins save with `wings.storage` (`plugin-storage/`).
    pub storage: PathBuf,
    /// A folder of plugins to run as well, always on and approved, as the desktop's debug builds run the repo's
    /// `plugins/`. Only a debug build takes it (`--plugins-dev-root`).
    pub dev: Option<PathBuf>,
}

impl PluginPaths {
    /// The desktop's layout inside its data folder `data`.
    pub fn under(data: &Path) -> Self {
        Self {
            dir: data.join("plugins"),
            state: data.join("plugins.json"),
            storage: data.join("plugin-storage"),
            dev: None,
        }
    }

    /// The desktop app's own data folder.
    pub fn desktop() -> Option<Self> {
        crate::projects::desktop_dir().map(|data| Self::under(&data))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub project: PathBuf,
    /// Where tabs and splits are saved and restored from. `None` keeps nothing, as in tests.
    pub state: Option<PathBuf>,
    /// The native app's own project list, shown in the sidebar and changed by plugins. `None` keeps it in memory, with
    /// only `project` in it.
    pub projects: Option<PathBuf>,
    /// Claude Code's folder, where agent detection reads each session's state. `None` turns
    /// detection off, as in tests.
    pub agents: Option<PathBuf>,
    /// The desktop app's plugins, started and offered as tabs. `None` runs none, as in tests.
    pub plugins: Option<PluginPaths>,
    /// The desktop app's data folder, whose `projects/` holds each project's page: its facts, sections and blocks.
    /// `None` shows no project pages, as in tests.
    pub project_info: Option<PathBuf>,
    /// Where the history sheet and the command palette look for past sessions. `None` finds none, as in tests.
    pub sessions: Option<SessionSource>,
    /// The desktop app's `ai/` folder: the model Settings chose for session names and agent summaries, and the names
    /// made so far. `None` makes none, as in tests.
    pub ai: Option<PathBuf>,
    /// The editors "Open in" offers: `None` looks for the installed ones; tests and screenshots list their own.
    pub editors: Option<Vec<wings_editors::EditorView>>,
    /// Where the app serves the socket the `wings` command talks to (`wings_connect::cli::socket`), and connects Claude
    /// Code to it. `None` serves nothing and leaves Claude Code alone, as in tests and screenshots, which must never take
    /// the socket of the Wings you are using; only `main` sets it.
    pub socket: Option<PathBuf>,
    /// The terminal servers panes run in, so they outlive the app (`WINGS_TERMINAL_SERVER=1`). `None` runs every pane
    /// in the app, as tests do unless they start a server of their own.
    #[cfg(unix)]
    pub terminal_server: Option<TerminalServers>,
    /// Where drives mount, `/Volumes`: a pane whose folder is on one that isn't mounted yet waits for it. Tests use a
    /// folder of their own.
    pub volumes: PathBuf,
}

/// The app's terminal servers, shared by every view of the config.
#[cfg(unix)]
#[derive(Clone)]
pub struct TerminalServers(pub std::sync::Arc<crate::terminal_server::Servers>);

#[cfg(unix)]
impl std::fmt::Debug for TerminalServers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("TerminalServers")
            .field(&self.0.current())
            .finish()
    }
}

#[cfg(unix)]
impl PartialEq for TerminalServers {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

#[cfg(unix)]
impl Eq for TerminalServers {}

/// Where past sessions are read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionSource {
    /// Claude Code's folder and the folder of every other account beside it, and OpenCode's service.
    Accounts(PathBuf),
    /// Only these Claude Code folders, the default account's first: made-up sessions in tests and screenshots.
    Folders(Vec<PathBuf>),
}

impl Config {
    pub fn parse(
        args: impl IntoIterator<Item = OsString>,
        cwd: &Path,
    ) -> Result<Option<Self>, String> {
        let mut project = None;
        let mut dev_root = None;
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            let slot = match flag.to_str() {
                Some("--help") => return Ok(None),
                Some("--project") => &mut project,
                Some("--plugins-dev-root") if cfg!(debug_assertions) => &mut dev_root,
                Some("--plugins-dev-root") => {
                    return Err("--plugins-dev-root is only in debug builds".into());
                }
                _ => return Err(format!("Unknown argument: {}", flag.to_string_lossy())),
            };
            if slot.is_some() {
                return Err(format!(
                    "{} was supplied more than once",
                    flag.to_string_lossy()
                ));
            }
            *slot = Some(
                args.next()
                    .ok_or_else(|| format!("{} needs a value", flag.to_string_lossy()))?,
            );
        }
        let project = match project {
            Some(path) => absolute(path, cwd)?,
            // Opened from Finder or the Dock, the app starts in `/`, which is no project: open the one
            // the desktop app showed last, else the home folder.
            None if cwd == Path::new("/") => crate::projects::desktop_dir()
                .and_then(|dir| last_project(&dir.join("workspace.json")))
                .or_else(std::env::home_dir)
                .unwrap_or_else(|| cwd.to_owned()),
            None => cwd.to_owned(),
        };
        if project.to_str().is_none() {
            return Err("The project path must be valid UTF-8".into());
        }
        let dev = dev_root.map(|path| absolute(path, cwd)).transpose()?;
        Ok(Some(Self {
            project,
            state: crate::saved::default_path(),
            projects: crate::projects::default_path(),
            agents: Some(claude_accounts::claude_dir()),
            plugins: PluginPaths::desktop().map(|paths| PluginPaths { dev, ..paths }),
            project_info: crate::projects::desktop_dir(),
            sessions: Some(SessionSource::Accounts(claude_accounts::claude_dir())),
            ai: crate::projects::desktop_dir().map(|dir| dir.join("ai")),
            editors: None,
            socket: None,
            #[cfg(unix)]
            terminal_server: None,
            volumes: PathBuf::from("/Volumes"),
        }))
    }
}

fn absolute(value: OsString, cwd: &Path) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err("A path cannot be empty".into());
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        // Saved project ids must not change when a selected path contains symlinks.
        Ok(path)
    } else {
        std::path::absolute(cwd.join(path)).map_err(|error| error.to_string())
    }
}

/// The project the desktop app showed last, from its workspace file, if that folder still exists.
fn last_project(workspace: &Path) -> Option<PathBuf> {
    let file: serde_json::Value = serde_json::from_slice(&std::fs::read(workspace).ok()?).ok()?;
    let path = PathBuf::from(file.get("activeSpaceId")?.as_str()?);
    path.is_dir().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn parse(args: &[&str]) -> Result<Option<Config>, String> {
        Config::parse(
            args.iter().map(OsString::from),
            Path::new("/selected/project"),
        )
    }

    #[test]
    fn unconfigured_launch_uses_current_project() {
        let config = parse(&[]).unwrap().unwrap();
        assert_eq!(config.project, Path::new("/selected/project"));
    }

    #[test]
    fn the_removed_issues_flags_are_unknown_arguments() {
        for args in [
            &["--issues-storage-root", "/storage"][..],
            &["--issues-scope", "install-1"][..],
        ] {
            assert!(
                parse(args)
                    .unwrap_err()
                    .starts_with("Unknown argument: --issues-")
            );
        }
    }

    #[test]
    fn the_plugins_dev_root_is_for_debug_builds_and_is_made_absolute() {
        if !cfg!(debug_assertions) {
            assert_eq!(
                parse(&["--plugins-dev-root", "plugins"]).unwrap_err(),
                "--plugins-dev-root is only in debug builds"
            );
            return;
        }
        // Without a desktop data folder there are no plugins to add it to.
        let dev = |args: &[&str]| parse(args).unwrap().unwrap().plugins.map(|paths| paths.dev);
        if let Some(relative) = dev(&["--plugins-dev-root", "plugins"]) {
            assert_eq!(relative, Some("/selected/project/plugins".into()));
            assert_eq!(
                dev(&["--plugins-dev-root", "/repo/plugins"]),
                Some(Some("/repo/plugins".into()))
            );
            assert_eq!(dev(&[]), Some(None));
        }
        assert!(parse(&["--plugins-dev-root"]).is_err());
        assert!(parse(&["--plugins-dev-root", "/a", "--plugins-dev-root", "/b"]).is_err());
    }

    #[test]
    fn a_finder_launch_opens_the_desktops_last_project_if_it_still_exists() {
        let dir = std::env::temp_dir().join(format!("wings-native-last-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("project")).unwrap();
        let file = dir.join("workspace.json");
        let point_at = |path: &Path| {
            fs::write(&file, json!({ "activeSpaceId": path }).to_string()).unwrap();
            last_project(&file)
        };
        assert_eq!(point_at(&dir.join("project")), Some(dir.join("project")));
        assert_eq!(point_at(&dir.join("gone")), None);
        fs::write(&file, b"{ not json").unwrap();
        assert_eq!(last_project(&file), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn project_identity_is_not_canonicalized() {
        let path = "/selected/symlink/project";
        assert_eq!(
            parse(&["--project", path]).unwrap().unwrap().project,
            Path::new(path)
        );
    }

    #[test]
    fn cli_rejects_unknown_repeated_and_missing_values() {
        for args in [
            &["--unknown"][..],
            &["--project"][..],
            &["--project", "/a", "--project", "/b"][..],
        ] {
            assert!(parse(args).is_err());
        }
        assert_eq!(parse(&["--help"]).unwrap(), None);
    }
}
