use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::plugin::{Action, Broker, Project, Screen, StorageSnapshot};
use wings_plugin_storage::Storage;
use wings_spaces::claude_accounts;

pub const USAGE: &str = "Usage: wings-native [--project PATH] [--issues-storage-root PATH --issues-scope SCOPE] [--plugins-dev-root PATH]\n\n--project PATH              Project's saved absolute path (default: current directory)\n--issues-storage-root PATH  Existing Wings plugin storage root\n--issues-scope SCOPE        Explicit trusted Issues installation scope\n--plugins-dev-root PATH     Also run the plugins in PATH, like the repo's plugins/, with no approval (debug builds)\n--help                     Show this help";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuesSource {
    pub root: PathBuf,
    pub scope: String,
}

impl IssuesSource {
    fn snapshot(&self) -> Result<StorageSnapshot, String> {
        Storage::new(self.root.clone())
            .snapshot("tasks", &self.scope)?
            .entries("")
    }
}

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
    pub issues: Option<IssuesSource>,
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
}

impl Config {
    pub fn parse(
        args: impl IntoIterator<Item = OsString>,
        cwd: &Path,
    ) -> Result<Option<Self>, String> {
        let mut project = None;
        let mut root = None;
        let mut scope = None;
        let mut dev_root = None;
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            let slot = match flag.to_str() {
                Some("--help") => return Ok(None),
                Some("--project") => &mut project,
                Some("--issues-storage-root") => &mut root,
                Some("--issues-scope") => &mut scope,
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
        let issues = match (root, scope) {
            (None, None) => None,
            (Some(root), Some(scope)) => Some(IssuesSource {
                root: absolute(root, cwd)?,
                scope: scope
                    .into_string()
                    .map_err(|_| "The Issues scope must be valid UTF-8")?,
            }),
            _ => return Err("Use --issues-storage-root and --issues-scope together".into()),
        };
        let dev = dev_root.map(|path| absolute(path, cwd)).transpose()?;
        Ok(Some(Self {
            project,
            issues,
            state: crate::saved::default_path(),
            projects: crate::projects::default_path(),
            agents: Some(claude_accounts::claude_dir()),
            plugins: PluginPaths::desktop().map(|paths| PluginPaths { dev, ..paths }),
        }))
    }

    /// Blocking disk reads and worker IPC. Call only on the background executor.
    pub fn load_issues(&self, action: Option<Action>) -> Result<Screen, String> {
        let source = self
            .issues
            .as_ref()
            .ok_or("Issues storage is not configured")?;
        let storage = source.snapshot()?;
        let project = Project {
            id: self
                .project
                .to_str()
                .ok_or("The project path must be valid UTF-8")?
                .to_owned(),
            name: self
                .project
                .file_name()
                .unwrap_or(self.project.as_os_str())
                .to_string_lossy()
                .into_owned(),
        };
        Broker::new()
            .map_err(|error| error.to_string())?
            .invoke(project, storage, action)
            .map_err(|error| error.to_string())
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
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };

    fn parse(args: &[&str]) -> Result<Option<Config>, String> {
        Config::parse(
            args.iter().map(OsString::from),
            Path::new("/selected/project"),
        )
    }

    #[test]
    fn unconfigured_launch_uses_current_project_and_no_storage() {
        let config = parse(&[]).unwrap().unwrap();
        assert_eq!(config.project, Path::new("/selected/project"));
        assert_eq!(config.issues, None);
    }

    #[test]
    fn root_and_scope_are_required_together() {
        assert!(parse(&["--issues-storage-root", "/storage"]).is_err());
        assert!(parse(&["--issues-scope", "install-1"]).is_err());
        let config = parse(&[
            "--issues-storage-root",
            "/storage",
            "--issues-scope",
            "install-1",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(
            config.issues.unwrap(),
            IssuesSource {
                root: "/storage".into(),
                scope: "install-1".into()
            }
        );
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

    #[test]
    fn host_reads_only_the_selected_namespace_without_filtering_or_migration() {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wings-native-host-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let file = root.join("tasks.chosen.json");
        let other = root.join("tasks.other.json");
        let unrelated = root.join("unrelated.chosen.json");
        let contents = json!({"issues/project-meta": "malformed metadata must reach the plugin", "settings": true}).to_string();
        fs::write(&file, &contents).unwrap();
        fs::write(&other, "not JSON").unwrap();
        fs::write(&unrelated, "not JSON").unwrap();
        let source = IssuesSource {
            root: root.clone(),
            scope: "chosen".into(),
        };
        let entries = source.snapshot().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].1,
            json!("malformed metadata must reach the plugin")
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), contents);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
        fs::write(&file, "{\"settings\":false}").unwrap();
        assert_eq!(
            source.snapshot().unwrap(),
            vec![("settings".into(), json!(false))]
        );
        for file in [file, other, unrelated] {
            fs::remove_file(file).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
}
