//! What a plugin host asks of the app around it. The permission checks and the argument rules are the host's, in
//! `calls`; this is only what the app has to do or know: show a badge, open a URL, say which folders are projects.
//!
//! Every method may be called from any thread, a frame's or a worker's, so an implementation hands work on rather than
//! touching the window.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};

use serde::Serialize;
use wings_plugins::{manifest::Plugin, permissions::may_hear_event};
use wings_spaces::{
    claude_accounts::Accounts,
    spaces::{Space, WorktreeSetup},
};

use super::{
    PluginHost,
    ui::{Badge, PaneStatus},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

/// A project of the list, as plugins know it: its folder is its id.
impl From<Space> for Project {
    fn from(space: Space) -> Self {
        Self {
            id: space.path.clone(),
            name: space.name,
            path: space.path,
            parent_id: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenPane {
    pub cwd: PathBuf,
    pub project: PathBuf,
    pub input: Option<String>,
    pub placement: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenView {
    pub plugin: String,
    pub id: String,
    pub place: String,
    pub args: Option<serde_json::Value>,
    pub new_tab: bool,
}

/// One plugin's call of another's tool, as the app is asked to run it. The caller's permission is checked.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub target: String,
    pub tool: String,
    pub input: serde_json::Value,
    pub project: Option<Project>,
}

/// A running plugin, as another plugin's `callTool` finds it.
struct Target {
    plugin: Plugin,
    builtin: bool,
    /// The tools its manifest declares.
    tools: Vec<String>,
    host: Weak<PluginHost>,
}

/// The plugins an app runs, for the calls between them. It holds no plugin alive.
#[derive(Default)]
pub struct Running(Mutex<HashMap<String, Target>>);

impl Running {
    /// Makes `host` the plugin `plugin` is run by, `builtin` if it is one of Wings' own.
    pub fn add(&self, plugin: &Plugin, builtin: bool, host: &Arc<PluginHost>) {
        self.0.lock().unwrap().insert(
            plugin.manifest.id.clone(),
            Target {
                plugin: plugin.clone(),
                builtin,
                tools: plugin
                    .manifest
                    .contributes
                    .mcp_tools
                    .iter()
                    .map(|tool| tool.name.clone())
                    .collect(),
                host: Arc::downgrade(host),
            },
        );
    }

    /// Whether the plugin is one of Wings' own, or `None` when it doesn't run here.
    pub fn builtin(&self, plugin: &str) -> Option<bool> {
        self.0
            .lock()
            .unwrap()
            .get(plugin)
            .map(|target| target.builtin)
    }

    /// Sends the event `name` with `data` of the plugin `from` to the main of every other running plugin that declared
    /// `<from> <name>` in its `events`. Only Wings' own plugins send and hear events.
    pub fn emit(&self, from: &str, name: &str, data: &serde_json::Value) {
        let running = self.0.lock().unwrap();
        if running.get(from).is_none_or(|sender| !sender.builtin) {
            return;
        }
        let text = serde_json::json!({ "from": from, "name": name, "data": data }).to_string();
        for (id, target) in running.iter() {
            if id != from
                && target.builtin
                && may_hear_event(&target.plugin, from, name)
                && let Some(host) = target.host.upgrade()
            {
                host.hear_event(&text);
            }
        }
    }

    /// Hands the call to the plugin's main, or answers `reply` with why it can't.
    pub fn call_tool(&self, call: ToolCall, reply: crate::page::runtime::Reply) {
        let target = {
            let running = self.0.lock().unwrap();
            running
                .get(&call.target)
                .map(|target| (target.tools.contains(&call.tool), target.host.upgrade()))
        };
        match target {
            Some((true, Some(host))) => {
                host.run_tool(&call.tool, call.input, call.project.as_ref(), reply)
            }
            Some((false, Some(_))) => {
                reply.err(format!("{} has no tool {}", call.target, call.tool))
            }
            _ => reply.err(format!("{} is turned off or not installed", call.target)),
        }
    }
}

/// One program listening for TCP connections that `localhost` reaches, as `wings.ports` lists them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanePort {
    pub pane_id: String,
    pub pid: u32,
    /// The program, like `node` or `Python`.
    pub name: String,
    pub port: u16,
    /// When the program started, in ms since the epoch.
    pub started_at: u64,
}

/// A button on a plugin's notification, as the desktop's `cleanButtons` keeps it: `Open` shows one of the plugin's views
/// that open as a tab, `Url` opens a web page, which the app checks against the plugin's `openUrl` grant on the click.
#[derive(Clone, Debug, PartialEq)]
pub enum NotifyButton {
    Open {
        label: String,
        view: String,
        args: Option<serde_json::Value>,
    },
    Url {
        label: String,
        url: String,
    },
}

impl NotifyButton {
    pub fn label(&self) -> &str {
        match self {
            NotifyButton::Open { label, .. } | NotifyButton::Url { label, .. } => label,
        }
    }
}

/// A plugin's notification, checked and cut to length by the host.
#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub plugin: String,
    /// The plugin's name, which leads the system notification's title and names it in the inbox.
    pub name: String,
    /// The title without the plugin's name.
    pub title: String,
    pub body: String,
    /// The folder of the project its tabs open in; else they open in the one on screen.
    pub project: Option<String>,
    pub buttons: Vec<NotifyButton>,
}

impl Notification {
    /// The system notification's title: the plugin's name leads it, so a plugin can't pass for Wings or Claude.
    pub fn system_title(&self) -> String {
        format!("{}: {}", self.name, self.title)
    }
}

/// The app a host runs in.
pub trait Env: Send + Sync + 'static {
    fn open_diff(&self, _view: crate::diff::View) -> Result<(), String> {
        Err("Native diffs aren't available in this host".into())
    }
    fn update_diff(
        &self,
        _plugin: &str,
        _id: &str,
        _update: crate::diff::Update,
    ) -> Result<(), String> {
        Err("Native diffs aren't available in this host".into())
    }
    fn close_diff(&self, _plugin: &str, _id: &str) {}
    fn current_project(&self) -> Option<PathBuf> {
        None
    }
    fn open_pane(&self, _plan: OpenPane, reply: crate::page::runtime::Reply) {
        reply.err("Opening panes isn't available".into());
    }
    fn open_view(&self, _plan: OpenView, reply: crate::page::runtime::Reply) {
        reply.err("Opening views isn't available".into());
    }
    fn focus_pane(&self, _pane: &str, reply: crate::page::runtime::Reply) {
        reply.err("Focusing panes isn't available".into());
    }
    /// Closes the plugin's page `view` wherever the app shows it.
    fn close_view(&self, _plugin: &str, _view: &str) {}
    /// Whether the plugin is one of Wings' own, or `None` when it doesn't run here.
    fn builtin(&self, _plugin: &str) -> Option<bool> {
        None
    }
    /// Runs the tool of another running plugin, and answers `reply` with `{ text, isError }`, now or later.
    fn call_tool(&self, call: ToolCall, reply: crate::page::runtime::Reply) {
        reply.err(format!("{} is turned off or not installed", call.target));
    }
    /// Makes a git worktree of the project in folder `project`, on a new `branch` from `base` or the repo's default
    /// branch, and lists it under that project. The host has checked the plugin may.
    fn create_worktree(
        &self,
        _project: &str,
        _branch: &str,
        _base: Option<&str>,
    ) -> Result<Project, String> {
        Err("Worktrees aren't available in this host".into())
    }
    /// What the new worktrees of the project in folder `project` get.
    fn worktree_setup(&self, _project: &str) -> Result<WorktreeSetup, String> {
        Err("Worktrees aren't available in this host".into())
    }
    /// Changes the worktree setup of the project in folder `project`, only what is given: include pattern lines (none
    /// resets to the default list) and the setup line (blank clears it).
    fn set_worktree_setup(
        &self,
        _project: &str,
        _include: Option<Vec<String>>,
        _setup: Option<String>,
    ) -> Result<(), String> {
        Err("Worktrees aren't available in this host".into())
    }
    /// Tells the other plugins that listen for the event `name` of plugin `from` (`wings.emit`). The host has checked
    /// `name` and `data`.
    fn emit(&self, _from: &str, _name: &str, _data: &serde_json::Value) {}
    /// The secret `name` the plugin `plugin` kept under its install scope `scope`, if it kept one. The host has checked
    /// the plugin may.
    fn secret_get(
        &self,
        _plugin: &str,
        _scope: &str,
        _name: &str,
    ) -> Result<Option<String>, String> {
        Err("Secrets aren't available in this host".into())
    }
    fn secret_set(
        &self,
        _plugin: &str,
        _scope: &str,
        _name: &str,
        _value: &str,
    ) -> Result<(), String> {
        Err("Secrets aren't available in this host".into())
    }
    fn secret_delete(&self, _plugin: &str, _scope: &str, _name: &str) -> Result<(), String> {
        Err("Secrets aren't available in this host".into())
    }
    /// Pastes `text` into the prompt of the agent in pane `pane` and answers `{ "submitted": bool }`, now or later.
    /// The host has checked the plugin may, and that the text is a message.
    fn send_to_agent(&self, _pane: &str, _text: &str, reply: crate::page::runtime::Reply) {
        reply.err("Sending messages to agents isn't available in this host".into());
    }
    /// Shows the settings page of `plugin`, which the host has checked it has.
    fn open_settings(&self, _plugin: &str) -> Result<(), String> {
        Err("Plugin settings aren't available in this host".into())
    }
    /// The folders of the app's projects, which `exec` may run in.
    fn projects(&self) -> Vec<PathBuf>;
    /// The folders of the Claude Code accounts, the default one's first: where the transcripts are.
    fn claude_dirs(&self) -> Vec<PathBuf>;
    /// The Claude Code accounts, and the one each project's terminals use.
    fn claude_accounts(&self) -> Result<Accounts, String> {
        Err("Claude Code accounts aren't available in this host".into())
    }
    /// Makes new terminals in the project in folder `project` use the account `name`, or the default account with
    /// `None`. The host has checked the plugin may, and that `name` is an account name.
    fn set_claude_account(&self, _project: &str, _name: Option<&str>) -> Result<(), String> {
        Err("Claude Code accounts aren't available in this host".into())
    }
    /// What the OpenCode session `session_id` is doing, as `wings.agentActivity` answers it, asked of OpenCode's
    /// background service. The host has checked the plugin may. Takes a while: called on a thread of its own.
    fn agent_activity(&self, _session_id: &str) -> Result<serde_json::Value, String> {
        Err("Agent activity isn't available in this host".into())
    }
    /// The local ports the programs in the panes listen on. Takes a while: called on a thread of its own.
    fn ports(&self) -> Result<Vec<PanePort>, String>;
    /// Opens `url` in the browser. The host has checked the plugin may.
    fn open_url(&self, url: &str) -> Result<(), String>;
    /// Shows a notification and keeps it in the inbox.
    fn notify(&self, notification: Notification) -> Result<(), String>;
    /// The plugin set, or with `None` took away, its section `section` under the project in folder `project`. The host
    /// has checked the plugin declares the section and the project is listed.
    fn set_project_section(
        &self,
        _plugin: &str,
        _project: &str,
        _section: &str,
        _rows: Option<Vec<super::SectionRow>>,
    ) -> Result<(), String> {
        Err("Project sections aren't available in this host".into())
    }
    /// The plugin said which of its section's sorts its rows are in, for every project. The host has checked the plugin
    /// declares the sort.
    fn set_section_sort(&self, _plugin: &str, _section: &str, _sort: &str) {}
    /// The plugin set, or with `None` took away, the badge of a pane.
    fn set_badge(&self, plugin: &str, pane: &str, badge: Option<Badge>);
    /// The plugin set, or with `None` took away, the status under a pane's terminal.
    fn set_pane_status(&self, plugin: &str, pane: &str, status: Option<PaneStatus>);
}

/// A plugin that is on and approved, and the app it runs in: what every call of its frames is checked against.
#[derive(Clone)]
pub struct Access {
    pub plugin: Plugin,
    /// One of Wings' own plugins, which may make the calls the desktop lets those make anywhere.
    pub builtin: bool,
    pub env: std::sync::Arc<dyn Env>,
}

/// A pane as `wings.onPanes` lists it: the desktop's `WingsPane`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneEntry {
    pub pane_id: String,
    /// Where the pane's program is now, which follows `cd`.
    pub cwd: String,
    /// What runs in the pane, like `zsh` or `claude`.
    pub command: String,
    pub project: String,
    /// The folder of that project, which is its id.
    pub project_id: String,
    /// Set while an agent runs in the pane and has a session id.
    pub session: Option<SessionEntry>,
    /// The focused pane of the tab on screen: at most one pane has it.
    pub focused: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub session_id: String,
    /// `claude`, `opencode`, ...
    pub kind: String,
    pub name: Option<String>,
    /// `working`, `blocked`, `done` or `idle`.
    pub state: String,
    /// When the agent process started, in ms since 1970.
    pub started_at: u64,
}
