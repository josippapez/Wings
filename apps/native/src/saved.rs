//! The native app's own workspace file: each project's terminal tabs and splits, restored on launch.
//! The desktop app's `workspace.json` is a different file, and the native app never writes it.

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SavedDir {
    Row,
    Column,
}

/// A tab's split tree. A restored pane gets a new shell in its last folder, below its saved output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SavedLayout {
    Pane {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
        /// Names the pane's saved output in `scrollback/`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        history: Option<String>,
        /// A name you gave the pane, shown instead of its program's title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    Split {
        dir: SavedDir,
        ratio: f32,
        a: Box<SavedLayout>,
        b: Box<SavedLayout>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub layout: SavedLayout,
    /// The focused pane's position in reading order.
    pub focused: usize,
    /// The zoomed pane's position, if one fills the tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoomed: Option<usize>,
}

/// A plugin page that was open: which plugin and which of its views, as the manifest names them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPluginTab {
    pub plugin: String,
    pub view: String,
    /// What `wings.openView` opened it with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedProject {
    pub tabs: Vec<SavedTab>,
    /// The active terminal tab's position, or `None` when a plugin page or Issues was open.
    pub active: Option<usize>,
    /// The plugin pages, in strip order after the terminal tabs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugin_tabs: Vec<SavedPluginTab>,
    /// The active plugin page's position, when one was open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_plugin: Option<usize>,
    /// The Issues tab was closed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub issues_closed: bool,
}

/// What the whole app keeps, the same for every project, as in the desktop app.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_open: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shift_return_meta: Option<bool>,
    /// `block`, `bar` or `underline`, as the desktop app names them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_style: Option<String>,
    /// The cursor blinks in the focused pane, as the desktop's `cursorBlink`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_blink: Option<bool>,
    /// `outline`, `block`, `bar`, `underline` or `none`, as the desktop app names them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_inactive_style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_contrast_ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrollback: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_on_select: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clipboard_write: Option<bool>,
    /// Ctrl-` shows and hides the quick terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_terminal: Option<bool>,
    /// The id of the editor "Open in" uses first, as the desktop's `wings.defaultEditor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    /// What the new tab button opens: `terminal`, `project`, or a plugin page as `plugin:view`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_tab: Option<String>,
    /// The first tab of a project you add: `terminal` or `project`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_project_opens: Option<String>,
    /// Claude's `open_pane` may press Enter after the command it types, as the desktop's `agentsRunCommands`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents_run_commands: Option<bool>,
    /// A message an agent or plugin sends to another agent is submitted with Enter, as the desktop's `agentsMessage`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents_message: Option<bool>,
    /// The sidebar's width in points, as last dragged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<f32>,
    /// `manual`, `added`, `worked` or `name`, as the desktop app names them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_sort: Option<String>,
    /// Whether the sidebar's "Older" group is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older_open: Option<bool>,
    /// When each project was last worked on, in ms since the epoch, by its folder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub last_active: BTreeMap<String, u64>,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    version: u32,
    #[serde(flatten)]
    settings: Settings,
    projects: BTreeMap<String, SavedProject>,
}

fn read(path: &Path) -> Option<File> {
    let file: File = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (file.version == VERSION).then_some(file)
}

/// A missing, unreadable or older file restores nothing, so the app opens one terminal.
pub fn load(path: &Path, project: &str) -> Option<SavedProject> {
    read(path)?.projects.remove(project)
}

pub fn load_settings(path: &Path) -> Settings {
    read(path).map(|file| file.settings).unwrap_or_default()
}

/// Replaces one project's entry and keeps the others. Write then rename, like the desktop app, so a
/// crash mid-write can't leave half a file.
pub fn save(path: &Path, project: &str, saved: SavedProject, settings: Settings) -> io::Result<()> {
    let mut file = read(path).unwrap_or(File {
        version: VERSION,
        settings: Settings::default(),
        projects: BTreeMap::new(),
    });
    file.settings = settings;
    file.projects.insert(project.to_owned(), saved);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&file)?)?;
    std::fs::rename(tmp, path)
}

/// History ids become file names, so a hand-edited file can't point anywhere else.
pub fn valid_history_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn scrollback(state: &Path, id: &str) -> Option<PathBuf> {
    valid_history_id(id).then(|| state.with_file_name("scrollback").join(id))
}

/// A pane's output from its last launch, or nothing.
pub fn load_history(state: &Path, id: &str) -> Vec<u8> {
    scrollback(state, id)
        .and_then(|file| std::fs::read(file).ok())
        .unwrap_or_default()
}

pub fn save_history(state: &Path, id: &str, output: &[u8]) -> io::Result<()> {
    let Some(file) = scrollback(state, id) else {
        return Ok(());
    };
    std::fs::create_dir_all(file.parent().unwrap_or(state))?;
    std::fs::write(file, output)
}

/// The history id of every saved pane in every project.
pub fn history_ids(state: &Path) -> Vec<String> {
    fn ids(layout: &SavedLayout, keep: &mut Vec<String>) {
        match layout {
            SavedLayout::Pane { history, .. } => keep.extend(history.clone()),
            SavedLayout::Split { a, b, .. } => {
                ids(a, keep);
                ids(b, keep);
            }
        }
    }
    let mut keep = Vec::new();
    for project in read(state)
        .map(|file| file.projects)
        .unwrap_or_default()
        .values()
    {
        for tab in &project.tabs {
            ids(&tab.layout, &mut keep);
        }
    }
    keep
}

/// Deletes saved output no pane in any project refers to any more, like the desktop's `histories_keep`.
pub fn prune_histories(state: &Path) {
    let keep = history_ids(state);
    let dir = state.with_file_name("scrollback");
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if !keep.iter().any(|id| entry.file_name() == id.as_str()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Beside the platform's other app data: `~/Library/Application Support` on macOS, `%APPDATA%` on
/// Windows, and `$XDG_DATA_HOME` or `~/.local/share` elsewhere.
pub fn default_path() -> Option<PathBuf> {
    let env = |key| {
        std::env::var_os(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let dir = if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/Wings Native")
    } else if cfg!(windows) {
        env("APPDATA")?.join("Wings Native")
    } else {
        env("XDG_DATA_HOME")
            .or_else(|| Some(env("HOME")?.join(".local/share")))?
            .join("wings-native")
    };
    Some(dir.join("workspace.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split() -> SavedProject {
        SavedProject {
            tabs: vec![
                SavedTab {
                    layout: SavedLayout::Split {
                        dir: SavedDir::Row,
                        ratio: 0.3,
                        a: Box::new(SavedLayout::Pane {
                            cwd: None,
                            history: None,
                            name: None,
                        }),
                        b: Box::new(SavedLayout::Pane {
                            cwd: None,
                            history: None,
                            name: None,
                        }),
                    },
                    focused: 1,
                    zoomed: Some(1),
                },
                SavedTab {
                    layout: SavedLayout::Pane {
                        cwd: Some("/tmp".into()),
                        history: Some("1a-2".into()),
                        name: Some("server".into()),
                    },
                    focused: 0,
                    zoomed: None,
                },
            ],
            active: Some(1),
            plugin_tabs: vec![
                SavedPluginTab {
                    plugin: "tasks".into(),
                    view: "board".into(),
                    args: None,
                    state: Some(serde_json::json!({ "view": "list" })),
                },
                SavedPluginTab {
                    plugin: "notes".into(),
                    view: "all".into(),
                    args: Some(serde_json::json!({ "filter": "mine" })),
                    state: None,
                },
            ],
            active_plugin: None,
            issues_closed: false,
        }
    }

    fn settings(font_size: f32) -> Settings {
        Settings {
            font_size: Some(font_size),
            sidebar_open: Some(false),
            ..Settings::default()
        }
    }

    fn temp_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wings-native-saved-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("workspace.json")
    }

    #[test]
    fn a_project_round_trips_and_other_projects_stay() {
        let path = temp_file("round-trip");
        assert_eq!(load(&path, "/a"), None);
        save(&path, "/a", split(), settings(15.0)).unwrap();
        save(&path, "/b", SavedProject::default(), settings(16.0)).unwrap();
        assert_eq!(load(&path, "/a"), Some(split()));
        assert_eq!(
            load_settings(&path),
            Settings {
                font_size: Some(16.0),
                sidebar_open: Some(false),
                ..Settings::default()
            }
        );
        assert_eq!(load(&path, "/b"), Some(SavedProject::default()));
        assert!(!path.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn saved_output_is_kept_only_for_panes_the_file_names() {
        let path = temp_file("histories");
        save(&path, "/a", split(), settings(13.0)).unwrap();
        save_history(&path, "1a-2", b"kept").unwrap();
        save_history(&path, "dead-1", b"gone").unwrap();
        save_history(&path, "../escape", b"never").unwrap();
        prune_histories(&path);
        assert_eq!(load_history(&path, "1a-2"), b"kept");
        assert_eq!(load_history(&path, "dead-1"), b"");
        assert!(!path.parent().unwrap().join("escape").exists());
        assert!(!valid_history_id("../escape") && !valid_history_id(""));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_damaged_or_older_file_restores_nothing() {
        let path = temp_file("damaged");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(load(&path, "/a"), None);
        std::fs::write(
            &path,
            br#"{"version":0,"projects":{"/a":{"tabs":[],"active":null}}}"#,
        )
        .unwrap();
        assert_eq!(load(&path, "/a"), None);
        // Files saved before plugin pages were tabs still load.
        let older = br#"{"version":1,"projects":{"/a":{"tabs":[],"active":null}}}"#;
        std::fs::write(&path, older).unwrap();
        assert_eq!(load(&path, "/a"), Some(SavedProject::default()));
        // Files saved before panes kept their folder still load.
        let older = br#"{"version":1,"projects":{"/a":{"tabs":[{"layout":{"kind":"pane"},"focused":0}],"active":0}}}"#;
        std::fs::write(&path, older).unwrap();
        assert_eq!(
            load(&path, "/a").unwrap().tabs[0].layout,
            SavedLayout::Pane {
                cwd: None,
                history: None,
                name: None,
            }
        );
        save(&path, "/a", split(), settings(13.0)).unwrap();
        assert_eq!(load(&path, "/a"), Some(split()));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
