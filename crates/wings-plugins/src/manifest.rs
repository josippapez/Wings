//! What a plugin's manifest says: the access it asks for and what it adds to Wings.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Permissions {
    /// Commands the plugin may run: a program and the subcommand its arguments must start with, e.g.
    /// `gh pr view`. A bare program name allows any arguments. A trailing `[...]` lists the only flags it may pass,
    /// and `[]` allows none.
    pub exec: Vec<String>,
    /// Claude Code transcript entry types the plugin may read, e.g. `pr-link`, or `attachment:<kind>` for one
    /// kind of attachment, e.g. `attachment:model`.
    pub transcript: Vec<String>,
    /// URL prefixes the plugin may open in the browser: https, or http to this computer (`localhost`,
    /// `127.0.0.1` or `[::1]`).
    pub open_url: Vec<String>,
    /// https URLs the plugin may call with `wings.fetch`. A bare entry is a prefix, like an API's base URL, for
    /// any method. `GET <prefix>` allows only GET and HEAD. `POST <url>` allows one POST path, where `*` stands
    /// for one path segment, with a JSON object (or no body, if nothing is pinned) and no query. Either may end with ` {field=a|b}` to
    /// pin body fields (POST only).
    pub fetch: Vec<String>,
    /// The secrets the plugin may keep with `wings.secrets.set`, each with the one URL prefix `wings.fetch` sends it to
    /// as the bearer, like `{"azure-devops": "https://dev.azure.com/"}`. A plugin that may call two hosts can't send
    /// one's token to the other.
    pub secrets: BTreeMap<String, String>,
    /// API paths the plugin may POST to through a signed-in CLI, as `<gh|glab|az> <pattern>`. `*` stands for
    /// one path segment, e.g. `gh repos/*/*/pulls/*/comments/*/replies`. `exec` refuses every write flag. An
    /// entry may end with ` {field=a|b}`, which pins a body field to one of those values, like
    /// `gh repos/*/*/pulls/*/reviews {event=APPROVE}`.
    pub post: Vec<String>,
    /// Lets the plugin open terminals in your projects and move focus to a pane. Each entry is a command it
    /// may start in a new pane, matched like `exec`; `[]` allows plain shells only. Absent, it can do neither.
    pub panes: Option<Vec<String>>,
    /// Lets the plugin show desktop notifications.
    pub notify: bool,
    /// Lets the plugin read what Claude Code last told `wings statusline`: usage limits and each session's
    /// context window and prompt cache.
    pub statusline: bool,
    /// Lets the plugin see which local TCP ports the programs in each pane listen on, with `wings.ports`.
    pub ports: bool,
    /// Lets the plugin read what an OpenCode session is doing, with `wings.agentActivity`, and hear when it
    /// changes, with `wings.onAgentActivity`.
    pub agent_activity: bool,
    /// Lets the plugin read the names and sizes of the files and folders on your disks, with `wings.diskUsage`, look
    /// for them by name, with `wings.diskFind`, and list the disks, with `wings.disks`.
    pub disk_usage: bool,
    /// Lets the plugin type a message into the prompt of an agent running in a pane, with `wings.sendToAgent`. Enter
    /// is only pressed while the user lets agents message each other.
    pub send_to_agents: bool,
    /// Lets the plugin make a git worktree of one of your projects with `wings.createWorktree`: a new branch in a
    /// folder beside the repo, added to Wings as a project.
    pub worktrees: bool,
    /// Lets the plugin list your Claude Code accounts with `wings.claudeAccounts` and choose the one each project's
    /// new terminals use with `wings.setClaudeAccount`.
    pub claude_accounts: bool,
    /// Other plugins' tools the plugin may call with `wings.callTool`: a plugin id for all of its tools, or
    /// `<plugin> <tool>` for one, like `tasks list`.
    pub call_tools: Vec<String>,
    /// Events from other plugins the plugin hears with `wings.onEvent`: `<plugin> <event>`, like `pr-tracker pr-state`.
    pub events: Vec<String>,
}

/// What a plugin adds to Wings, shown in the manager. The host refuses UI calls a plugin didn't declare.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Contributes {
    /// UI it draws: `badges` on pane headers, `status` under a pane's terminal, the `diff` viewer.
    pub ui: Vec<String>,
    /// Tools it offers Claude through the Wings MCP server.
    pub mcp_tools: Vec<McpTool>,
    /// A line for Claude on when to reach for those tools. Claude defers tool descriptions behind tool search
    /// and sees only tool names and the server's instructions at session start, so this is what makes it
    /// use them unprompted. Up to `MAX_MCP_INSTRUCTIONS` characters.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mcp_instructions: String,
    /// Hooks it answers with `wings.onHook`: `SessionStart` to add context to a session, `PreToolUse` to refuse a
    /// Bash command (Wings 0.1.54), `Journal` to get what an agent's change journal reports at the end of a turn,
    /// sent by `wings journal` (Wings 0.1.61).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<String>,
    /// Web pages it shows in a popover from a title bar button.
    pub panels: Vec<Panel>,
    /// Its own pages shown in the right sidebar from a title bar button.
    pub sidebars: Vec<Sidebar>,
    /// Its own pages that can open in the right sidebar, in a modal over Wings, in a window of their own or as a tab.
    pub views: Vec<View>,
    /// Lists it can show under each project in the sidebar, filled with `wings.setProjectSection`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub project_sections: Vec<ProjectSection>,
    /// Pages that can be placed on a project's Project page, each in a frame of its own.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// Its own page in Wings Settings, under Plugin settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<SettingsPage>,
}

/// A plugin page the user can place on a Project page. The page is an HTML file like a view's, so a view may share it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`, or empty to use the plugin's own `icon`.
    #[serde(default)]
    pub icon: String,
    pub page: String,
    /// The least height in CSS pixels, within `BLOCK_MIN_HEIGHT`. Absent means 160, which the UI fills in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_height: Option<u32>,
}

pub const MAX_MCP_INSTRUCTIONS: usize = 500;
pub const BLOCK_MIN_HEIGHT: (u32, u32) = (80, 800);
pub const MAX_BLOCKS: usize = 8;

/// A plugin's page in Wings Settings: an HTML file in the plugin, sandboxed like its sidebars.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsPage {
    pub page: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidebar {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`, or empty to use the plugin's own `icon`.
    #[serde(default)]
    pub icon: String,
    /// An HTML file in the plugin. It runs sandboxed like the plugin itself, with the same `wings` object.
    pub page: String,
    /// A smaller page shown under the title bar button while you point at it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub popover: Option<Popover>,
}

/// A sidebar's hover page: another HTML file in the plugin, sized in CSS pixels within `POPOVER_WIDTH` and `POPOVER_HEIGHT`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Popover {
    pub page: String,
    pub width: u32,
    pub height: u32,
}

pub const POPOVER_WIDTH: (u32, u32) = (240, 480);
pub const POPOVER_HEIGHT: (u32, u32) = (160, 640);

/// One of the plugin's own pages, like a sidebar that may also open as a modal, a window or a tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`, or empty to use the plugin's own `icon`.
    #[serde(default)]
    pub icon: String,
    /// An HTML file in the plugin. It runs sandboxed like the plugin itself, with the same `wings` object.
    pub page: String,
    /// Where it may open, from `VIEW_PLACES`. Its title bar button opens it in the first.
    #[serde(default = "sidebar_only")]
    pub opens: Vec<String>,
    /// The modal's size, and a window's size when it first opens, in CSS pixels.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// Lists the view in the + menu and the command palette. Needs `tab` in `opens`.
    #[serde(default, rename = "newTab", skip_serializing_if = "std::ops::Not::not")]
    pub new_tab: bool,
    /// Opening it as a tab always opens a new tab, instead of showing the project's existing one. Needs `tab` in `opens`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multiple: bool,
    /// The page works with no project on screen, so the + menu opens it as a tab then too. Needs `tab` in `opens`.
    #[serde(default, rename = "noProject", skip_serializing_if = "std::ops::Not::not")]
    pub no_project: bool,
    /// `false` leaves it out of the title bar's plugin buttons: a page that shows one item, opened from a row
    /// with its args, is empty from a button.
    #[serde(default = "yes", skip_serializing_if = "Clone::clone")]
    pub button: bool,
}

fn yes() -> bool {
    true
}

/// A section a plugin may add under each project in the sidebar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSection {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`, or empty to use the plugin's own `icon`.
    #[serde(default)]
    pub icon: String,
    /// A view that opens as a tab and lists everything, not just the rows: the section's All button opens it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub all: String,
    /// The orders the section's header menu offers. The plugin sorts the rows itself: it hears the pick with `onSectionSort`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sorts: Vec<SectionSort>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionSort {
    pub id: String,
    pub label: String,
}

pub const VIEW_PLACES: [&str; 4] = ["sidebar", "modal", "window", "tab"];

fn sidebar_only() -> Vec<String> {
    vec!["sidebar".into()]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`, or empty to use the plugin's own `icon`.
    #[serde(default)]
    pub icon: String,
    /// An https page. It runs as a normal website, with no access to Wings.
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// The icon names the UI can draw, shared with `src/lib/plugin-icons.ts`.
pub(crate) static PANEL_ICONS: std::sync::LazyLock<Vec<String>> =
    std::sync::LazyLock::new(|| serde_json::from_str(include_str!("../../../apps/desktop/src/lib/plugin-icons.json")).expect("plugin-icons.json"));

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema for the tool's arguments; an object schema. Defaults to taking no arguments.
    #[serde(default = "empty_schema")]
    pub input_schema: Value,
}

fn empty_schema() -> Value {
    serde_json::json!({ "type": "object" })
}

pub(crate) const UI_KINDS: [&str; 4] = ["badges", "diff", "rail", "status"];
/// The hooks a plugin can answer. `PreToolUse` only ever sees Bash commands. `UserPromptSubmit` runs on each message to
/// the agent. `Journal` isn't a Claude Code event: it is what `wings journal` sends when an agent's change journal
/// reports a finished turn.
pub const HOOK_EVENTS: [&str; 4] = ["SessionStart", "PreToolUse", "UserPromptSubmit", "Journal"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub api: u32,
    /// The oldest Wings that can run it, for a plugin that uses something newer than its `api`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_wings_version: Option<String>,
    pub main: String,
    /// An SVG file in the plugin folder, like `icon.svg`: its tile in the Plugins list, and the icon of any
    /// sidebar, view or panel that doesn't name a built-in one. Wings draws it as a one-colour mask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub contributes: Contributes,
}

#[derive(Clone)]
pub struct Plugin {
    pub manifest: Manifest,
    pub dir: PathBuf,
    /// The checked contents of `manifest.icon`.
    pub icon_svg: Option<String>,
}
