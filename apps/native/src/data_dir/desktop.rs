//! The desktop app's `workspace.json`, read as the desktop writes it (`SavedWorkspace` in `apps/desktop/src/App.tsx`),
//! and turned into the native app's tabs and settings when the replacement first starts on the desktop's folder.
//!
//! Only the fields the native app has a place for are read; the rest of the file is the desktop's, and it is never
//! written here.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use wings_spaces::claude_accounts::{AccountPick, valid_name};

use crate::saved::{
    GlobalHotkey, NotifySound, SavedDir, SavedLayout, SavedPluginTab, SavedProject, SavedTab,
    Settings, valid_history_id,
};

/// `LayoutNode` in `apps/desktop/src/lib/layout.ts`.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Layout {
    Pane {
        id: String,
        #[serde(default)]
        name: Option<String>,
    },
    Split {
        dir: SavedDir,
        ratio: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

/// `PaneTab` in `App.tsx`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneTab {
    pub id: String,
    pub space_id: String,
    pub layout: Layout,
    #[serde(default)]
    pub focused_pane: Option<String>,
    #[serde(default)]
    pub zoomed_pane: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub plugin_id: String,
    pub view_id: String,
}

/// `SavedViewTab` in `apps/desktop/src/lib/plugin-pages.ts`: a plugin view, at its place among all of a project's tabs.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewTab {
    pub id: String,
    pub space_id: String,
    pub view: View,
    #[serde(default)]
    pub state: Option<serde_json::Value>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    pub at: usize,
}

/// `TerminalLook` in `apps/desktop/src/lib/terminal.ts`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalLook {
    pub font_family: Option<String>,
    pub line_height: Option<f32>,
    pub cursor_style: Option<String>,
    pub cursor_blink: Option<bool>,
    pub cursor_inactive_style: Option<String>,
    pub scrollback: Option<usize>,
    pub copy_on_select: Option<bool>,
    pub minimum_contrast_ratio: Option<f32>,
    pub images: Option<bool>,
    pub clipboard_write: Option<bool>,
}

/// `TerminalKeys` in `terminal.ts`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalKeys {
    pub option_as_meta: Option<String>,
    pub shift_return_sends_meta_return: Option<bool>,
}

/// `QuickTerminalKey` in `apps/desktop/src/lib/quick-terminal.ts`.
#[derive(Debug, Deserialize)]
pub struct QuickTerminalKey {
    pub enabled: bool,
}

/// `Widths` in `App.tsx`, in pixels.
#[derive(Debug, Deserialize)]
pub struct Widths {
    pub left: f32,
    pub right: f32,
}

/// `SavedWorkspace` in `App.tsx`, `version` 1. Every field the desktop marks optional, or that older files lack, has a
/// default, as `parseSaved` takes any object with `version: 1`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub version: u32,
    #[serde(default)]
    pub active_space_id: Option<String>,
    #[serde(default)]
    pub active_tab_by_space: HashMap<String, String>,
    #[serde(default)]
    pub sidebar_open: Option<bool>,
    #[serde(default)]
    pub widths: Option<Widths>,
    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub terminal_keys: Option<TerminalKeys>,
    #[serde(default)]
    pub global_hotkey: Option<GlobalHotkey>,
    #[serde(default)]
    pub quick_terminal_key: Option<QuickTerminalKey>,
    #[serde(default)]
    pub terminal_look: Option<TerminalLook>,
    #[serde(default)]
    pub tabs: Vec<PaneTab>,
    #[serde(default)]
    pub view_tabs: Vec<ViewTab>,
    #[serde(default)]
    pub new_tab_page: Option<String>,
    #[serde(default)]
    pub new_project_opens: Option<String>,
    #[serde(default)]
    pub project_sort: Option<String>,
    #[serde(default)]
    pub older_open: Option<bool>,
    #[serde(default)]
    pub last_active: BTreeMap<String, u64>,
    #[serde(default)]
    pub notify_sound: Option<NotifySound>,
    #[serde(default)]
    pub pinned_buttons: Option<Vec<String>>,
    /// The projects' order on screen, by id; the list file keeps them in the order they were added.
    #[serde(default)]
    pub space_order: Vec<String>,
    #[serde(default)]
    pub agents_run_commands: Option<bool>,
    #[serde(default)]
    pub agents_message: Option<bool>,
    /// The folder each pane's shell was in, by pane key.
    #[serde(default)]
    pub cwds: HashMap<String, String>,
    /// Each pane's saved output in `scrollback/`, by pane key.
    #[serde(default)]
    pub histories: HashMap<String, String>,
    /// The Claude Code account of each pane that runs under one other than its project's, by pane key, as
    /// `{ "name": "work" }` or `{ "name": null }`. Read loosely, as the desktop's `savedClaudeAccount` does: an entry of
    /// another shape is dropped, not the whole file.
    #[serde(default)]
    pub claude_accounts: HashMap<String, serde_json::Value>,
}

/// The desktop's workspace at `path`, when it is one the desktop itself would load.
pub fn read(path: &Path) -> Option<Workspace> {
    let workspace: Workspace = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (workspace.version == 1).then_some(workspace)
}

/// The saved outputs the desktop's workspace at `path` refers to.
pub fn history_ids(path: &Path) -> Vec<String> {
    read(path)
        .map(|workspace| workspace.histories.into_values().collect())
        .unwrap_or_default()
}

impl Workspace {
    /// The desktop's settings, in the native app's names. What the desktop has no setting for stays `None`.
    pub fn settings(&self) -> Settings {
        let look = self.terminal_look.as_ref();
        let keys = self.terminal_keys.as_ref();
        Settings {
            font_size: self.font_size,
            sidebar_open: self.sidebar_open,
            sidebar_width: self.widths.as_ref().map(|widths| widths.left),
            right_width: self.widths.as_ref().map(|widths| widths.right),
            // An empty family is the desktop's default font, as `None` is here.
            font_family: look
                .and_then(|look| look.font_family.clone())
                .filter(|family| !family.is_empty()),
            line_height: look.and_then(|look| look.line_height),
            cursor_style: look.and_then(|look| look.cursor_style.clone()),
            cursor_blink: look.and_then(|look| look.cursor_blink),
            cursor_inactive_style: look.and_then(|look| look.cursor_inactive_style.clone()),
            scrollback: look.and_then(|look| look.scrollback),
            copy_on_select: look.and_then(|look| look.copy_on_select),
            minimum_contrast_ratio: look.and_then(|look| look.minimum_contrast_ratio),
            images: look.and_then(|look| look.images),
            clipboard_write: look.and_then(|look| look.clipboard_write),
            option_as_meta: keys.and_then(|keys| keys.option_as_meta.clone()),
            shift_return_meta: keys.and_then(|keys| keys.shift_return_sends_meta_return),
            global_hotkey: self.global_hotkey.clone(),
            quick_terminal: self.quick_terminal_key.as_ref().map(|key| key.enabled),
            // The desktop saves `pluginId:viewId`, `terminal`, or null for a terminal, as the native app does.
            new_tab: Some(
                self.new_tab_page
                    .clone()
                    .unwrap_or_else(|| "terminal".into()),
            ),
            new_project_opens: self.new_project_opens.clone(),
            project_sort: self.project_sort.clone(),
            older_open: self.older_open,
            last_active: self.last_active.clone(),
            notify_sound: self.notify_sound.clone(),
            agents_run_commands: self.agents_run_commands,
            agents_message: self.agents_message,
            pinned_buttons: self.pinned_buttons.clone(),
            ..Settings::default()
        }
    }

    /// Each project's tabs, by its folder (the desktop's space id), in the native app's form. A project with no tabs
    /// is left out, so the native app opens its usual first terminal there.
    pub fn projects(&self) -> BTreeMap<String, SavedProject> {
        let mut projects: BTreeMap<String, SavedProject> = BTreeMap::new();
        // The tab the desktop showed in each project, as a terminal tab or a plugin page.
        let mut active: HashMap<&str, Active> = HashMap::new();
        for tab in &self.tabs {
            let project = projects.entry(tab.space_id.clone()).or_default();
            let mut keys = Vec::new();
            let layout = self.layout(&tab.layout, &mut keys);
            let at = |key: &Option<String>| {
                key.as_ref()
                    .and_then(|key| keys.iter().position(|k| k == key))
            };
            if self.active_tab_by_space.get(&tab.space_id) == Some(&tab.id) {
                active.insert(&tab.space_id, Active::Terminal(project.tabs.len()));
            }
            project.tabs.push(SavedTab {
                layout,
                focused: at(&tab.focused_pane).unwrap_or(0),
                zoomed: at(&tab.zoomed_pane),
            });
        }
        let mut views: Vec<&ViewTab> = self.view_tabs.iter().collect();
        views.sort_by_key(|tab| tab.at);
        for tab in views {
            let project = projects.entry(tab.space_id.clone()).or_default();
            if self.active_tab_by_space.get(&tab.space_id) == Some(&tab.id) {
                active.insert(&tab.space_id, Active::Plugin(project.plugin_tabs.len()));
            }
            project.plugin_tabs.push(SavedPluginTab {
                plugin: tab.view.plugin_id.clone(),
                view: tab.view.view_id.clone(),
                args: tab.args.clone(),
                state: tab.state.clone(),
            });
        }
        for (space, project) in &mut projects {
            match active.get(space.as_str()) {
                Some(Active::Terminal(at)) => project.active = Some(*at),
                Some(Active::Plugin(at)) => project.active_plugin = Some(*at),
                // The desktop showed a Project page, or nothing: the first terminal tab.
                None => project.active = (!project.tabs.is_empty()).then_some(0),
            }
        }
        projects
    }

    /// `layout` with each pane's folder and saved output, listing the pane keys in reading order into `keys`.
    fn layout(&self, layout: &Layout, keys: &mut Vec<String>) -> SavedLayout {
        match layout {
            Layout::Pane { id, name } => {
                keys.push(id.clone());
                SavedLayout::Pane {
                    cwd: self.cwds.get(id).map(PathBuf::from),
                    history: self
                        .histories
                        .get(id)
                        .filter(|history| valid_history_id(history))
                        .cloned(),
                    name: name.clone().filter(|name| !name.is_empty()),
                    claude_account: self.claude_accounts.get(id).and_then(|pick| {
                        match pick.get("name")? {
                            serde_json::Value::String(name) if valid_name(name) => {
                                Some(AccountPick {
                                    name: Some(name.clone()),
                                })
                            }
                            serde_json::Value::Null => Some(AccountPick { name: None }),
                            _ => None,
                        }
                    }),
                }
            }
            Layout::Split { dir, ratio, a, b } => SavedLayout::Split {
                dir: *dir,
                ratio: *ratio,
                a: Box::new(self.layout(a, keys)),
                b: Box::new(self.layout(b, keys)),
            },
        }
    }
}

enum Active {
    Terminal(usize),
    Plugin(usize),
}
