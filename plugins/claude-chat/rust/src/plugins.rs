//! What the plugins dialog (`/plugins`, `/plugin` and `/mcp`) shows, from Claude Code's answers: the plugins your
//! settings name, each on or off and in which settings, and the MCP servers with their status. And the request each
//! toggle in it sends, one a click.
//!
//! A plugin is turned on or off for this chat only, in the session's flag settings, then applied by reloading the
//! plugins: Claude Code refuses `enabledPlugins` in `update_settings`, so the chat never writes your settings files. A
//! server is turned on or off as `/mcp` does it, for this project, by Claude Code itself.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::session::{Command, request};

/// The ids of the dialog's requests, and the start of a toggle's, which a number ends.
pub const SETTINGS: &str = "plugins-settings";
pub const SERVERS: &str = "plugins-servers";
pub const RELOAD: &str = "plugins-reload";
const PLUGIN_TOGGLE: &str = "plugin-toggle-";
const SERVER_TOGGLE: &str = "server-toggle-";

/// Which list the dialog leads with: `/plugins` and `/plugin` open it on the plugins, `/mcp` on the servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    Plugins,
    Mcp,
}

/// The dialog, while it is open, and what its search holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Dialog {
    pub show: Show,
    pub query: String,
}

/// The dialog a draft opens, when the draft is exactly its command.
pub fn command(text: &str) -> Option<Show> {
    match text.trim() {
        "/plugins" | "/plugin" => Some(Show::Plugins),
        "/mcp" => Some(Show::Mcp),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plugin {
    /// As your settings name it: `name@marketplace`.
    pub id: String,
    pub name: String,
    pub marketplace: String,
    /// On in this session, this chat's toggles included.
    pub on: bool,
    /// On in your settings files.
    pub saved: bool,
    /// The settings that decide it: "User", "Project" or "Local", "Managed" for your organization's, which the chat
    /// can't change, or "Session" when only this session's flags name it.
    pub scope: &'static str,
}

impl Plugin {
    pub fn locked(&self) -> bool {
        self.scope == "Managed"
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Server {
    /// As `mcp_status` names it and `mcp_toggle` takes it: a plugin's is `plugin:<plugin>:<server>`.
    pub name: String,
    /// `connected`, `pending`, `needs-auth`, `failed` or `disabled`.
    pub status: String,
    /// Where it is set up: `user`, `project`, `local`, `claudeai`, or `dynamic` for a plugin's.
    pub scope: String,
    /// How many tools it has, once connected.
    pub tools: Option<usize>,
    /// Why it failed.
    pub error: Option<String>,
}

impl Server {
    /// The plugin it comes with, and its own name, for a plugin's.
    pub fn of_plugin(&self) -> Option<(&str, &str)> {
        self.name.strip_prefix("plugin:")?.split_once(':')
    }

    pub fn short_name(&self) -> &str {
        self.of_plugin().map_or(self.name.as_str(), |(_, name)| name)
    }
}

/// What a toggle changed, to put back if Claude Code refuses it.
#[derive(Debug)]
enum Undo {
    Plugin { id: String, on: bool, set: Option<bool>, unapplied: bool },
    Server { name: String, status: String },
}

#[derive(Debug, Default)]
pub struct Plugins {
    /// From `get_settings`, None until it has answered.
    pub list: Option<Vec<Plugin>>,
    /// From `mcp_status`, or a reload's answer.
    pub servers: Option<Vec<Server>>,
    pub error: Option<String>,
    /// A plugin was turned on or off since the plugins were last reloaded, which applies it.
    pub unapplied: bool,
    /// A server was turned on since the servers were last read, so its status isn't known.
    pub stale: bool,
    /// The plugins this session's flag settings turn on or off. A toggle sends them all: a second `enabledPlugins` there
    /// replaces the first.
    set: BTreeMap<String, bool>,
    /// The toggles not answered yet, by request.
    pending: Vec<(String, Undo)>,
}

impl Plugins {
    /// The requests that fill the dialog: your settings, for the plugins, and the servers' status. Both only read.
    pub fn ask(&mut self) -> String {
        self.error = None;
        [request(SETTINGS, json!({ "subtype": "get_settings" })), request(SERVERS, json!({ "subtype": "mcp_status" }))].concat()
    }

    /// The one request that turns the plugin `id` on or off in this chat, the `n`th write. None when it is so already,
    /// or your organization's settings decide it.
    pub fn set_plugin(&mut self, id: &str, on: bool, n: u32) -> Option<String> {
        let plugin = self.list.as_mut()?.iter_mut().find(|plugin| plugin.id == id && !plugin.locked() && plugin.on != on)?;
        let request_id = format!("{PLUGIN_TOGGLE}{n}");
        let undo = Undo::Plugin { id: id.to_owned(), on: plugin.on, set: self.set.get(id).copied(), unapplied: self.unapplied };
        self.pending.push((request_id.clone(), undo));
        plugin.on = on;
        self.set.insert(id.to_owned(), on);
        self.unapplied = true;
        Some(request(&request_id, json!({ "subtype": "apply_flag_settings", "settings": { "enabledPlugins": self.set } })))
    }

    /// The one request that turns the server `name` on or off, for this project, the `n`th write. None when it is so
    /// already.
    pub fn set_server(&mut self, name: &str, on: bool, n: u32) -> Option<String> {
        let server = self.servers.as_mut()?.iter_mut().find(|server| server.name == name && (server.status != "disabled") != on)?;
        let request_id = format!("{SERVER_TOGGLE}{n}");
        self.pending.push((request_id.clone(), Undo::Server { name: name.to_owned(), status: server.status.clone() }));
        server.status = if on { "pending" } else { "disabled" }.to_owned();
        Some(request(&request_id, json!({ "subtype": "mcp_toggle", "serverName": name, "enabled": on })))
    }

    /// The request that reloads the plugins, which applies what was turned on or off. Not while Claude works: what it
    /// runs would change under it.
    pub fn reload(&mut self) -> String {
        self.error = None;
        request(RELOAD, json!({ "subtype": "reload_plugins", "refuse_if_busy": true }))
    }

    /// Takes in Claude Code's answer `response` to the request `id`. False when it isn't the dialog's.
    pub fn answered(&mut self, id: &str, response: &Value) -> bool {
        let answer = &response["response"];
        let refused = (response["subtype"] == "error").then(|| response["error"].as_str().unwrap_or("Claude Code didn't answer.").to_owned());
        if let Some(at) = self.pending.iter().position(|(request, _)| request == id) {
            let (_, undo) = self.pending.remove(at);
            match (refused, undo) {
                (Some(why), undo) => {
                    self.undo(undo);
                    self.error = Some(why);
                }
                // Turned on, it connects anew, and only a new read says how that went.
                (None, Undo::Server { name, .. }) => {
                    let connecting = self.servers.iter().flatten().any(|server| server.name == name && server.status == "pending");
                    self.stale |= connecting;
                }
                (None, Undo::Plugin { .. }) => {}
            }
            return true;
        }
        if !matches!(id, SETTINGS | SERVERS | RELOAD) {
            return false;
        }
        if refused.is_some() {
            self.error = refused;
            return true;
        }
        match id {
            SETTINGS => {
                let (list, set) = plugins_of(answer);
                (self.list, self.set) = (Some(list), set);
            }
            SERVERS => {
                self.servers = Some(servers_of(&answer["mcpServers"]));
                self.stale = false;
            }
            _ => {
                self.servers = Some(servers_of(&answer["mcpServers"]));
                (self.unapplied, self.stale) = (false, false);
            }
        }
        true
    }

    fn undo(&mut self, undo: Undo) {
        match undo {
            Undo::Plugin { id, on, set, unapplied } => {
                if let Some(plugin) = self.list.iter_mut().flatten().find(|plugin| plugin.id == id) {
                    plugin.on = on;
                }
                match set {
                    Some(set) => self.set.insert(id, set),
                    None => self.set.remove(&id),
                };
                self.unapplied = unapplied;
            }
            Undo::Server { name, status } => {
                if let Some(server) = self.servers.iter_mut().flatten().find(|server| server.name == name) {
                    server.status = status;
                }
            }
        }
    }
}

/// How many of `commands` the plugin `name` brings: its skills and commands, which Claude Code names `<plugin>:<name>`.
pub fn skills(commands: &[Command], name: &str) -> usize {
    commands.iter().filter(|command| command.name.strip_prefix(name).is_some_and(|rest| rest.starts_with(':'))).count()
}

/// The plugins `get_settings` names, by name, and the ones its flag settings turn on or off.
fn plugins_of(answer: &Value) -> (Vec<Plugin>, BTreeMap<String, bool>) {
    let sources = answer["sources"].as_array().map(Vec::as_slice).unwrap_or_default();
    // A value is a plugin's switch, or a list of the versions it may take, which is on too.
    let on = |value: &Value| *value != Value::Bool(false);
    let named = |source: &Value, id: &str| source["settings"]["enabledPlugins"].get(id).map(on);
    let mut list: Vec<Plugin> = answer["effective"]["enabledPlugins"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, value)| {
            let (name, marketplace) = id.rsplit_once('@').unwrap_or((id.as_str(), ""));
            // The sources come from the lowest priority up, so the last file that names it decides it.
            let file = sources.iter().rev().filter(|source| source["source"] != "flagSettings").find_map(|source| Some((source["source"].as_str()?, named(source, id)?)));
            let scope = match file.map(|(source, _)| source) {
                Some("userSettings") => "User",
                Some("projectSettings") => "Project",
                Some("localSettings") => "Local",
                Some("policySettings") => "Managed",
                _ => "Session",
            };
            Plugin { id: id.clone(), name: name.to_owned(), marketplace: marketplace.to_owned(), on: on(value), saved: file.is_some_and(|(_, on)| on), scope }
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    let set = sources
        .iter()
        .filter(|source| source["source"] == "flagSettings")
        .flat_map(|source| source["settings"]["enabledPlugins"].as_object().into_iter().flatten())
        .map(|(id, value)| (id.clone(), on(value)))
        .collect();
    (list, set)
}

/// The servers `mcp_status` (or a reload) lists.
fn servers_of(list: &Value) -> Vec<Server> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|server| {
            Some(Server {
                name: server["name"].as_str()?.to_owned(),
                status: server["status"].as_str().unwrap_or("pending").to_owned(),
                scope: server["scope"].as_str().unwrap_or_default().to_owned(),
                tools: server["tools"].as_array().map(Vec::len),
                error: server["error"].as_str().map(str::to_owned),
            })
        })
        .collect()
}
