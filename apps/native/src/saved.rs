//! The native app's own workspace file: each project's terminal tabs and splits, restored on launch.
//! The desktop app's `workspace.json` is a different file, and the native app never writes it (`data_dir`).

use std::{
    collections::{BTreeMap, BTreeSet},
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
        /// The Claude Code account its shell runs under when it isn't its project's, as a plugin's `openPane` chose it:
        /// `{ "name": null }` for the default account. Absent in files from before it, which use the project's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        claude_account: Option<wings_spaces::claude_accounts::AccountPick>,
        /// The agent session last seen in the pane, which a restore continues when the pane can't attach to its
        /// program again, as the desktop's `sessions`, `sessionKinds` and `sessionArgs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<SavedSession>,
        /// Wings ended its idle agent to free memory, and resumes `session` when the pane is next shown, as the
        /// desktop's `asleep`.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        asleep: bool,
    },
    Split {
        dir: SavedDir,
        ratio: f32,
        a: Box<SavedLayout>,
        b: Box<SavedLayout>,
    },
}

/// Which agent CLI a saved session belongs to, as the desktop's `SessionKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Claude,
    Opencode,
}

/// An agent session a pane ran, to continue in a new shell there: `claude --resume <id>` with the flags it was
/// started with, or `opencode --session <id>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSession {
    pub kind: SessionKind,
    pub id: String,
    /// Claude Code's start flags, without the ones that choose a session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// The agent's name in the Agents list, which its row keeps until the session is resumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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

/// The sound for new notifications, as the desktop's `notifySound`. Cleaned when read, so an unknown sound or a
/// missing value takes the default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NotifySound {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// `chime`, `pop`, `ping` or `drop`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
    /// Also for an agent in the pane on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watching: Option<bool>,
}

/// "Put idle agents to sleep" and "Sleep after", as the desktop's `sleep`. Either part missing takes the default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SleepAgents {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes: Option<u32>,
}

/// The global shortcut that brings Wings forward from any app, saved as the desktop's `globalHotkey`: on or off, and
/// the chord as an accelerator like `Ctrl+Alt+KeyJ`. Either part missing takes the desktop's default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalHotkey {
    #[serde(default = "on")]
    pub enabled: bool,
    #[serde(default = "default_accelerator")]
    pub accelerator: String,
}

/// The desktop's `hotkey.default` in `src/lib/commands.json`.
pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+KeyJ";

fn on() -> bool {
    true
}

fn default_accelerator() -> String {
    DEFAULT_HOTKEY.into()
}

impl Default for GlobalHotkey {
    fn default() -> Self {
        Self {
            enabled: true,
            accelerator: DEFAULT_HOTKEY.into(),
        }
    }
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
    /// `off`, `left`, `right` or `both`, as the desktop's `optionAsMeta`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_as_meta: Option<String>,
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
    /// Images programs print are drawn, as the desktop's `images`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<bool>,
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
    /// Where the first terminal an agent opens goes: `below`, `right` or `float`, as the desktop's `agentTerminalsOpen`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_terminals_open: Option<String>,
    /// Idle agents out of sight are ended after a while and resumed when shown, as the desktop's `sleep`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep: Option<SleepAgents>,
    /// The sidebar's width in points, as last dragged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<f32>,
    /// The right-hand sidebar's width in points, as last dragged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_width: Option<f32>,
    /// `manual`, `added`, `worked` or `name`, as the desktop app names them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_sort: Option<String>,
    /// Whether the sidebar's "Older" group is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older_open: Option<bool>,
    /// The plugin sections turned off under each project, as `plugin:section`, by its folder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sections_off: BTreeMap<String, BTreeSet<String>>,
    /// When each project was last worked on, in ms since the epoch, by its folder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub last_active: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_sound: Option<NotifySound>,
    /// Official plugin ids already announced as new, so reopening the app does not announce them again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen_plugins: Vec<String>,
    /// The global shortcut, under the desktop's own name.
    #[serde(
        default,
        rename = "globalHotkey",
        skip_serializing_if = "Option::is_none"
    )]
    pub global_hotkey: Option<GlobalHotkey>,
    /// The diff viewer wraps long lines, as the desktop's `wings.diff.wrap`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_wrap: Option<bool>,
    /// The diff viewer lists files as a folder tree, as the desktop's `wings.diff.tree`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_tree: Option<bool>,
    /// The docked diff shows its file list, as the desktop's `wings.diff.dockFiles`. Absent is shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_dock_files: Option<bool>,
    /// The docked diff's width in points, as last dragged, as the desktop's `widths.diff`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_width: Option<f32>,
    /// The plugin buttons kept in the project bar, as `plugin:sidebar` in the order they were pinned, as the
    /// desktop's `pinnedButtons`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_buttons: Option<Vec<String>>,
    /// The projects were put in the desktop's order (its `spaceOrder`), which the desktop kept apart from the list, so it
    /// happens once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_order: Option<bool>,
    /// New terminals run in the terminal server, so they keep running when Wings quits or updates. Absent is on
    /// (`terminal_server::enabled`). The desktop has no such setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_server: Option<bool>,
    /// Wings asked once to turn on other apps' data for its terminals, which macOS never asks for itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_data_asked: Option<bool>,
    /// Wings asked once whether to install the official plugins, on the first launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub official_plugins_asked: Option<bool>,
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

/// Every project's entry and the settings, for moving them to another file (`data_dir::migrate`).
pub fn read_all(path: &Path) -> Option<(Settings, BTreeMap<String, SavedProject>)> {
    read(path).map(|file| (file.settings, file.projects))
}

/// Writes a whole file, then renames it into place.
pub fn write_all(
    path: &Path,
    settings: Settings,
    projects: BTreeMap<String, SavedProject>,
) -> io::Result<()> {
    let file = File {
        version: VERSION,
        settings,
        projects,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&file)?)?;
    std::fs::rename(tmp, path)
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

/// On a launch whose settings have no global shortcut yet, takes the desktop's choice from its `workspace.json`, so
/// moving to the native app keeps the shortcut you set, or kept off. Returns whether it copied one.
pub fn seed_global_hotkey(native: &Path, desktop: &Path) -> io::Result<bool> {
    let mut file = match std::fs::read(native) {
        Ok(_) => match read(native) {
            Some(file) => file,
            // A file this version can't read is left for `save` to replace, not overwritten here.
            None => return Ok(false),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => File {
            version: VERSION,
            ..File::default()
        },
        Err(error) => return Err(error),
    };
    if file.settings.global_hotkey.is_some() {
        return Ok(false);
    }
    let Some(hotkey) = std::fs::read(desktop)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|desktop| serde_json::from_value(desktop.get("globalHotkey")?.clone()).ok())
    else {
        return Ok(false);
    };
    file.settings.global_hotkey = Some(hotkey);
    if let Some(dir) = native.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = native.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&file)?)?;
    std::fs::rename(tmp, native)?;
    Ok(true)
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

/// Deletes saved output no pane in any project refers to any more, like the desktop's `histories_keep`. On the desktop's
/// folder that also keeps what the desktop's own `workspace.json` refers to, so going back to it finds its panes' output.
pub fn prune_histories(state: &Path) {
    let mut keep = history_ids(state);
    let desktop = state.with_file_name("workspace.json");
    if desktop != state {
        keep.extend(crate::data_dir::desktop::history_ids(&desktop));
    }
    let dir = state.with_file_name("scrollback");
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if !keep.iter().any(|id| entry.file_name() == id.as_str()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// This build's workspace file (`data_dir::state_file`).
pub fn default_path() -> Option<PathBuf> {
    crate::data_dir::state_file()
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
                            asleep: false,
                            claude_account: None,
                            session: None,
                        }),
                        b: Box::new(SavedLayout::Pane {
                            cwd: None,
                            history: None,
                            name: None,
                            asleep: false,
                            claude_account: None,
                            session: Some(SavedSession {
                                kind: SessionKind::Opencode,
                                id: "ses_01HZabcdefgh".into(),
                                args: Vec::new(),
                                name: None,
                            }),
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
                        asleep: false,
                        claude_account: Some(wings_spaces::claude_accounts::AccountPick {
                            name: Some("work".into()),
                        }),
                        session: Some(SavedSession {
                            kind: SessionKind::Claude,
                            id: "0aa900ec-ed63-4df5-97c1-eecebeb8ef8d".into(),
                            args: vec!["--model".into(), "opus".into()],
                            name: Some("wings-12".into()),
                        }),
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
    fn the_global_shortcut_is_saved_under_the_desktops_name() {
        let path = temp_file("hotkey");
        let hotkey = GlobalHotkey {
            enabled: false,
            accelerator: "Cmd+Shift+KeyK".into(),
        };
        let settings = Settings {
            global_hotkey: Some(hotkey.clone()),
            ..Settings::default()
        };
        save(&path, "/a", SavedProject::default(), settings).unwrap();
        let text: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            text["globalHotkey"],
            serde_json::json!({ "enabled": false, "accelerator": "Cmd+Shift+KeyK" })
        );
        assert_eq!(load_settings(&path).global_hotkey, Some(hotkey));
        // A part missing takes the default, as the desktop's `{ ...DEFAULT_GLOBAL_HOTKEY, ...saved }`.
        std::fs::write(
            &path,
            br#"{"version":1,"globalHotkey":{"enabled":false},"projects":{}}"#,
        )
        .unwrap();
        assert_eq!(
            load_settings(&path).global_hotkey,
            Some(GlobalHotkey {
                enabled: false,
                accelerator: DEFAULT_HOTKEY.into()
            })
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_first_launch_takes_the_desktops_shortcut_and_later_ones_keep_their_own() {
        let path = temp_file("hotkey-seed");
        let desktop = path.with_file_name("desktop.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &desktop,
            br#"{"version":3,"fontSize":13,"globalHotkey":{"enabled":true,"accelerator":"Ctrl+Shift+KeyW"}}"#,
        )
        .unwrap();
        // No native file yet: it starts with the desktop's choice.
        assert!(seed_global_hotkey(&path, &desktop).unwrap());
        assert_eq!(
            load_settings(&path).global_hotkey.unwrap().accelerator,
            "Ctrl+Shift+KeyW"
        );
        // A native file with its projects and no shortcut keeps them and gains the shortcut.
        save(&path, "/a", split(), Settings::default()).unwrap();
        assert!(seed_global_hotkey(&path, &desktop).unwrap());
        assert_eq!(load(&path, "/a"), Some(split()));
        // Once the native app has its own, the desktop's no longer counts.
        let mine = GlobalHotkey {
            enabled: false,
            accelerator: DEFAULT_HOTKEY.into(),
        };
        let settings = Settings {
            global_hotkey: Some(mine.clone()),
            ..Settings::default()
        };
        save(&path, "/a", split(), settings).unwrap();
        assert!(!seed_global_hotkey(&path, &desktop).unwrap());
        assert_eq!(load_settings(&path).global_hotkey, Some(mine));
        // No desktop file, or one without the setting: nothing changes.
        std::fs::remove_file(&path).unwrap();
        assert!(!seed_global_hotkey(&path, &path.with_file_name("none.json")).unwrap());
        std::fs::write(&desktop, br#"{"version":3}"#).unwrap();
        assert!(!seed_global_hotkey(&path, &desktop).unwrap());
        assert!(!path.exists());
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
                asleep: false,
                claude_account: None,
                session: None,
            }
        );
        save(&path, "/a", split(), settings(13.0)).unwrap();
        assert_eq!(load(&path, "/a"), Some(split()));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
