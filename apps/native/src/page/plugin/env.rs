//! What a plugin host asks of the app around it. The permission checks and the argument rules are the host's, in
//! `calls`; this is only what the app has to do or know: show a badge, open a URL, say which folders are projects.
//!
//! Every method may be called from any thread, a frame's or a worker's, so an implementation hands work on rather than
//! touching the window.

use std::path::PathBuf;

use serde::Serialize;
use wings_plugins::manifest::Plugin;

use super::ui::{Badge, PaneStatus};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
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
    /// The folders of the app's projects, which `exec` may run in.
    fn projects(&self) -> Vec<PathBuf>;
    /// Claude Code's folder, where the transcripts are.
    fn claude_dir(&self) -> PathBuf;
    /// The local ports the programs in the panes listen on. Takes a while: called on a thread of its own.
    fn ports(&self) -> Result<Vec<PanePort>, String>;
    /// Opens `url` in the browser. The host has checked the plugin may.
    fn open_url(&self, url: &str) -> Result<(), String>;
    /// Shows a notification, with its title and body already cut to length and led by the plugin's name.
    fn notify(&self, title: &str, body: &str) -> Result<(), String>;
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
