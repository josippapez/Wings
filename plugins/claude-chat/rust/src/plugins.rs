//! What the plugins dialog (`/plugins`, `/plugin` and `/mcp`) shows, from Claude Code's answers: the plugins your
//! settings name, each on or off and in which settings, and the MCP servers with their status. And the request each
//! toggle in it sends, one a click.
//!
//! Discover lists the plugins your marketplaces offer, from `claude plugin list --available --json`, and installs one
//! with `claude plugin install`, only from a click on that plugin's Install button.
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
const SERVER_RECONNECT: &str = "server-reconnect-";
const SERVER_AUTH: &str = "server-auth-";
const SERVER_SIGN_OUT: &str = "server-sign-out-";

/// Which list the dialog leads with: `/plugins` and `/plugin` open it on the plugins, `/mcp` on the servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    Plugins,
    Mcp,
}

/// The dialog's two views: what is set up, and what your marketplaces offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Installed,
    Discover,
}

/// The dialog, while it is open, and what its search holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Dialog {
    pub show: Show,
    pub query: String,
    pub tab: Tab,
    pub discover: Discover,
}

impl Dialog {
    pub fn new(show: Show) -> Self {
        Dialog { show, query: String::new(), tab: Tab::Installed, discover: Discover::default() }
    }
}

/// How many matches Discover lists: the marketplaces offer thousands.
pub const LISTED: usize = 50;

/// A plugin a marketplace offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Available {
    /// `name@marketplace`, which `claude plugin install` takes.
    pub id: String,
    pub name: String,
    pub marketplace: String,
    pub description: String,
    pub installs: u64,
    /// It is installed by running a command the marketplace names, which only a person may accept, in a terminal.
    pub command: bool,
    /// Name, marketplace and description in lower case, which a search looks in.
    haystack: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Discover {
    /// From `claude plugin list --available`, None until it has answered.
    pub list: Option<Vec<Available>>,
    /// The ids of the plugins installed, in any scope.
    pub installed: Vec<String>,
    pub loading: bool,
    /// The plugin being installed: one at a time.
    pub installing: Option<String>,
    pub error: Option<String>,
    /// The id of the plugin that was just installed.
    pub done: Option<String>,
}

impl Discover {
    /// Starts reading the list. False when that is already going on.
    pub fn begin(&mut self) -> bool {
        if self.loading {
            return false;
        }
        (self.loading, self.error) = (true, None);
        true
    }

    /// Takes in what `claude plugin list --available --json` printed.
    pub fn listed(&mut self, out: Result<Value, String>) {
        self.loading = false;
        let out = out.and_then(|out| match out["code"] == 0 {
            true => listing_of(out["stdout"].as_str().unwrap_or_default()).ok_or_else(|| "Claude Code's list of plugins wasn't understood.".to_owned()),
            false => Err(failure(&out)),
        });
        match out {
            Ok((list, installed)) => (self.list, self.installed) = (Some(list), installed),
            Err(why) => self.error = Some(why),
        }
    }

    /// The matches for `query`, at most `LISTED` of them, and how many more there are. Each word has to be in the
    /// plugin's name, marketplace or description.
    pub fn found(&self, query: &str) -> (Vec<&Available>, usize) {
        let wanted = query.to_lowercase();
        let mut matches = self.list.iter().flatten().filter(|plugin| wanted.split_whitespace().all(|word| plugin.haystack.contains(word)));
        let shown: Vec<&Available> = matches.by_ref().take(LISTED).collect();
        (shown, matches.count())
    }

    pub fn is_installed(&self, id: &str) -> bool {
        self.installed.iter().any(|installed| installed == id)
    }

    /// The arguments of the one `claude` command that installs the plugin `id`, which a click on its Install button
    /// runs. None when it isn't on the list, is installed already, needs its command accepted, or another install is
    /// running.
    pub fn install(&mut self, id: &str) -> Option<Vec<String>> {
        let plugin = self.list.iter().flatten().find(|plugin| plugin.id == id)?;
        if plugin.command || self.installing.is_some() || self.is_installed(id) {
            return None;
        }
        (self.installing, self.error, self.done) = (Some(id.to_owned()), None, None);
        Some(["plugin", "install", id, "--json"].map(str::to_owned).to_vec())
    }

    /// Takes in how the install of `id` went.
    pub fn installed(&mut self, id: &str, outcome: Result<(), String>) {
        self.installing = None;
        match outcome {
            Ok(()) => {
                self.installed.push(id.to_owned());
                self.done = Some(id.to_owned());
            }
            Err(why) => self.error = Some(why),
        }
    }
}

/// How an install ended, from what `claude plugin install --json` printed. A plugin that is installed by a command is
/// refused, as nothing here accepts a command.
pub fn install_outcome(out: &Result<Value, String>) -> Result<(), String> {
    let out = out.as_ref().map_err(String::clone)?;
    if out["code"] == 0 {
        return Ok(());
    }
    let result: Value = out["stdout"].as_str().unwrap_or_default().lines().rev().find_map(|line| serde_json::from_str(line).ok()).unwrap_or_default();
    if result["failureCode"] == "command_source_refused" {
        return Err("It is installed by running a command on your computer. Install it with /plugin in a terminal, where you can read the command first.".to_owned());
    }
    Err(result["message"].as_str().map_or_else(|| failure(out), str::to_owned))
}

/// Why a `claude` command failed: what it said, or its exit code.
fn failure(out: &Value) -> String {
    let said = out["stderr"].as_str().unwrap_or_default().trim();
    if said.is_empty() { format!("claude exited with code {}.", out["code"]) } else { said.to_owned() }
}

/// The available plugins, most installed first, and the ids of the installed ones, from `list --available --json`.
fn listing_of(stdout: &str) -> Option<(Vec<Available>, Vec<String>)> {
    let listing: Value = serde_json::from_str(stdout).ok()?;
    let installed = listing["installed"].as_array().into_iter().flatten().filter_map(|plugin| plugin["id"].as_str().map(str::to_owned)).collect();
    let mut available: Vec<Available> = listing["available"]
        .as_array()?
        .iter()
        .filter_map(|plugin| {
            // An id is passed to `claude` as an argument, so one that reads as a flag is left out.
            let id = plugin["pluginId"].as_str().filter(|id| !id.starts_with('-'))?;
            let (name, marketplace) = (plugin["name"].as_str()?, plugin["marketplaceName"].as_str().unwrap_or_default());
            let description = plugin["description"].as_str().unwrap_or_default();
            Some(Available {
                id: id.to_owned(),
                name: name.to_owned(),
                marketplace: marketplace.to_owned(),
                description: description.to_owned(),
                installs: plugin["installCount"].as_u64().unwrap_or(0),
                command: plugin["source"]["source"] == "command",
                haystack: format!("{name}\n{marketplace}\n{description}").to_lowercase(),
            })
        })
        .collect();
    available.sort_by(|a, b| b.installs.cmp(&a.installs));
    Some((available, installed))
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
    /// How it is reached: `stdio`, `http`, `sse`, or `claudeai-proxy` for claude.ai's own.
    pub transport: String,
}

impl Server {
    /// It is signed in to in a browser, which Claude Code starts: an http or sse server, not claude.ai's own.
    pub fn signs_in(&self) -> bool {
        matches!(self.transport.as_str(), "http" | "sse")
    }

    /// The plugin it comes with, and its own name, for a plugin's.
    pub fn of_plugin(&self) -> Option<(&str, &str)> {
        self.name.strip_prefix("plugin:")?.split_once(':')
    }

    pub fn short_name(&self) -> &str {
        self.of_plugin().map_or(self.name.as_str(), |(_, name)| name)
    }
}

/// What a toggle changed, to put back if Claude Code refuses it. A sign-in changes nothing until it is answered.
#[derive(Debug)]
enum Undo {
    Plugin { id: String, on: bool, set: Option<bool>, unapplied: bool },
    Server { name: String, status: String },
    Auth { name: String },
}

/// The page Claude Code gave to sign in to a server, which a click opens.
#[derive(Debug, Clone, PartialEq)]
pub struct Auth {
    pub server: String,
    pub url: String,
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
    /// The sign-in page of the server you asked to sign in to, until you read the servers again.
    pub auth: Option<Auth>,
}

impl Plugins {
    /// The requests that fill the dialog: your settings, for the plugins, and the servers' status. Both only read.
    pub fn ask(&mut self) -> String {
        (self.error, self.auth) = (None, None);
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

    /// The one request that reconnects the server `name`, which failed, the `n`th write. None for a server that didn't fail.
    pub fn reconnect(&mut self, name: &str, n: u32) -> Option<String> {
        let server = self.servers.as_mut()?.iter_mut().find(|server| server.name == name && server.status == "failed")?;
        let request_id = format!("{SERVER_RECONNECT}{n}");
        self.pending.push((request_id.clone(), Undo::Server { name: name.to_owned(), status: server.status.clone() }));
        server.status = "pending".to_owned();
        Some(request(&request_id, json!({ "subtype": "mcp_reconnect", "serverName": name })))
    }

    /// The one request that has Claude Code start the sign-in to the server `name`, which wants one, the `n`th write. It
    /// answers with the page to sign in at, which a second click opens. None for any other server.
    pub fn authenticate(&mut self, name: &str, n: u32) -> Option<String> {
        self.servers.iter().flatten().find(|server| server.name == name && server.status == "needs-auth" && server.signs_in())?;
        let request_id = format!("{SERVER_AUTH}{n}");
        self.pending.push((request_id.clone(), Undo::Auth { name: name.to_owned() }));
        (self.error, self.auth) = (None, None);
        Some(request(&request_id, json!({ "subtype": "mcp_authenticate", "serverName": name })))
    }

    /// The one request that signs the connected server `name` out, the `n`th write. None for any other server.
    pub fn sign_out(&mut self, name: &str, n: u32) -> Option<String> {
        let server = self.servers.as_mut()?.iter_mut().find(|server| server.name == name && server.status == "connected" && server.signs_in())?;
        let request_id = format!("{SERVER_SIGN_OUT}{n}");
        self.pending.push((request_id.clone(), Undo::Server { name: name.to_owned(), status: server.status.clone() }));
        server.status = "needs-auth".to_owned();
        Some(request(&request_id, json!({ "subtype": "mcp_clear_auth", "serverName": name })))
    }

    /// Takes in Claude Code's answer to the sign-in of `name`: the page to open, or none when it needs none.
    fn signing_in(&mut self, name: &str, answer: &Value) {
        match answer["authUrl"].as_str() {
            Some(url) if url.starts_with("https://") => self.auth = Some(Auth { server: name.to_owned(), url: url.to_owned() }),
            Some(_) => self.error = Some("Claude Code's sign-in address isn't https, so it isn't opened here. Sign in with /mcp in Claude Code in a terminal.".to_owned()),
            // It is signed in already, and only a new read says so.
            None => self.stale = true,
        }
    }

    /// The request that reloads the plugins, which applies what was turned on or off. Not while Claude works: what it
    /// runs would change under it.
    pub fn reload(&mut self) -> String {
        self.error = None;
        request(RELOAD, json!({ "subtype": "reload_plugins", "refuse_if_busy": true }))
    }

    /// What follows an install: the plugins reload to load it, and your settings are read again for the list.
    pub fn after_install(&mut self) -> String {
        self.unapplied = true;
        [self.reload(), request(SETTINGS, json!({ "subtype": "get_settings" }))].concat()
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
                (None, Undo::Auth { name }) => self.signing_in(&name, answer),
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
            Undo::Auth { .. } => {}
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
                transport: server["transport"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `claude plugin list --available --json` printed (claude 2.1.327), cut down and made up.
    const AVAILABLE: &str = include_str!("../tests/available.json");

    fn discover() -> Discover {
        let mut discover = Discover::default();
        discover.listed(Ok(json!({ "code": 0, "stdout": AVAILABLE, "stderr": "" })));
        discover
    }

    fn ids(found: (Vec<&Available>, usize)) -> Vec<&str> {
        found.0.into_iter().map(|plugin| plugin.id.as_str()).collect()
    }

    #[test]
    fn the_list_has_each_plugin_most_installed_first_and_the_installed_ids() {
        let discover = discover();
        let list = discover.list.as_ref().expect("the list");
        let shown: Vec<(&str, &str, bool)> = list.iter().map(|plugin| (plugin.id.as_str(), plugin.marketplace.as_str(), plugin.command)).collect();
        // The id that reads as a flag is left out, so it can never be passed to claude.
        assert_eq!(
            shown,
            [
                ("code-search@claude-plugins-official", "claude-plugins-official", false),
                ("api-guard@claude-plugins-official", "claude-plugins-official", false),
                ("toolbox@team-tools", "team-tools", false),
                ("ios-build@claude-plugins-official", "claude-plugins-official", true),
                ("pixel-perfect@community", "community", false),
                ("release-notes@team-tools", "team-tools", false),
            ]
        );
        assert_eq!(list[1].description, "Audit OpenAPI specs and find vulnerabilities before they ship.");
        assert_eq!(discover.installed, ["toolbox@team-tools", "code-search@claude-plugins-official"]);
        assert!(discover.is_installed("toolbox@team-tools") && !discover.is_installed("api-guard@claude-plugins-official"));
    }

    #[test]
    fn a_failed_or_unreadable_list_is_an_error_not_an_empty_list() {
        let mut discover = Discover::default();
        discover.listed(Ok(json!({ "code": 1, "stdout": "", "stderr": "not signed in\n" })));
        assert_eq!((discover.list.as_ref(), discover.error.as_deref()), (None, Some("not signed in")));
        discover.listed(Ok(json!({ "code": 0, "stdout": "{\"installed\":[]}", "stderr": "" })));
        assert!(discover.list.is_none() && discover.error.is_some());
        discover.listed(Err("Output is larger than 8 MB".into()));
        assert_eq!(discover.error.as_deref(), Some("Output is larger than 8 MB"));
        assert!(!discover.loading);
    }

    #[test]
    fn the_search_looks_in_name_description_and_marketplace_and_takes_each_word() {
        let discover = discover();
        assert_eq!(ids(discover.found("")).len(), 6);
        assert_eq!(ids(discover.found("API-GUARD")), ["api-guard@claude-plugins-official"]);
        assert_eq!(ids(discover.found("by meaning")), ["code-search@claude-plugins-official"]);
        assert_eq!(ids(discover.found("team-tools")), ["toolbox@team-tools", "release-notes@team-tools"]);
        assert_eq!(ids(discover.found("team lint")), ["toolbox@team-tools"]);
        assert!(ids(discover.found("zzz")).is_empty());
    }

    #[test]
    fn the_search_lists_the_first_fifty_and_counts_the_rest() {
        let mut discover = Discover::default();
        let many: Vec<Value> = (0..4_000).map(|n| json!({ "pluginId": format!("p{n}@m"), "name": format!("p{n}"), "marketplaceName": "m", "source": "./p" })).collect();
        discover.listed(Ok(json!({ "code": 0, "stdout": json!({ "installed": [], "available": many }).to_string(), "stderr": "" })));
        let (shown, left) = discover.found("");
        assert_eq!((shown.len(), left), (LISTED, 4_000 - LISTED));
        // p39, p390 to p399, and p3900 to p3999 are 111 names with "p39" in them.
        let (shown, left) = discover.found("p39");
        assert_eq!((shown.len(), left), (LISTED, 111 - LISTED));
    }

    #[test]
    fn install_builds_one_command_for_that_plugin_and_no_second_while_it_runs() {
        let mut discover = discover();
        let args = discover.install("api-guard@claude-plugins-official").expect("a command");
        assert_eq!(args, ["plugin", "install", "api-guard@claude-plugins-official", "--json"]);
        assert_eq!(discover.installing.as_deref(), Some("api-guard@claude-plugins-official"));
        assert_eq!(discover.install("pixel-perfect@community"), None, "one at a time");
        discover.installed("api-guard@claude-plugins-official", Ok(()));
        assert!(discover.is_installed("api-guard@claude-plugins-official") && discover.installing.is_none());
        assert_eq!(discover.done.as_deref(), Some("api-guard@claude-plugins-official"));
        assert_eq!(discover.install("api-guard@claude-plugins-official"), None, "installed already");
        assert!(discover.install("pixel-perfect@community").is_some());
    }

    #[test]
    fn install_refuses_what_is_not_listed_or_needs_its_command_accepted() {
        let mut discover = discover();
        assert_eq!(discover.install("ios-build@claude-plugins-official"), None);
        assert_eq!(discover.install("toolbox@team-tools"), None, "installed already");
        for id in ["nothing@nowhere", "--yes@community", "-y", "", "api-guard@claude-plugins-official --scope=project"] {
            assert_eq!(discover.install(id), None, "{id}");
        }
        assert!(discover.installing.is_none());
    }

    #[test]
    fn a_failed_install_shows_why_and_a_command_source_is_left_to_the_terminal() {
        let out = |code: i64, stdout: &str, stderr: &str| Ok(json!({ "code": code, "stdout": stdout, "stderr": stderr }));
        assert_eq!(install_outcome(&out(0, "{\"outcome\":\"ok\"}\n", "")), Ok(()));
        let missing = "{\"command\":\"install\",\"outcome\":\"failed\",\"message\":\"Plugin \\\"x\\\" not found in marketplace \\\"m\\\"\",\"failureCode\":\"not_found\"}\n";
        assert_eq!(install_outcome(&out(1, missing, "")), Err("Plugin \"x\" not found in marketplace \"m\"".into()));
        let command = "\"xcode\" is installed by running a command\n{\"outcome\":\"failed\",\"failureCode\":\"command_source_refused\",\"message\":\"not run\"}\n";
        assert!(install_outcome(&out(1, command, "")).unwrap_err().contains("/plugin in a terminal"));
        assert_eq!(install_outcome(&out(1, "", "boom\n")), Err("boom".into()));
        assert_eq!(install_outcome(&Err("claude is not installed or not on PATH".into())), Err("claude is not installed or not on PATH".into()));
        let mut discover = discover();
        discover.install("api-guard@claude-plugins-official");
        discover.installed("api-guard@claude-plugins-official", Err("boom".into()));
        assert_eq!((discover.error.as_deref(), discover.installing.as_ref(), discover.done.as_ref()), (Some("boom"), None, None));
        assert!(!discover.is_installed("api-guard@claude-plugins-official"));
    }

    /// Servers as `mcp_status` lists them (claude 2.1.327), cut down.
    fn with_servers() -> Plugins {
        let mut plugins = Plugins::default();
        let servers = json!([
            { "name": "broken", "status": "failed", "scope": "user", "transport": "stdio", "error": "spawn failed" },
            { "name": "issues", "status": "needs-auth", "scope": "user", "transport": "http" },
            { "name": "tickets", "status": "connected", "scope": "project", "transport": "http" },
            { "name": "notes", "status": "connected", "scope": "user", "transport": "stdio" },
            { "name": "claude.ai Drive", "status": "needs-auth", "scope": "claudeai", "transport": "claudeai-proxy" },
        ]);
        plugins.answered(SERVERS, &json!({ "subtype": "success", "request_id": SERVERS, "response": { "mcpServers": servers } }));
        plugins
    }

    fn status(plugins: &Plugins, name: &str) -> String {
        plugins.servers.iter().flatten().find(|server| server.name == name).map(|server| server.status.clone()).unwrap_or_default()
    }

    fn sent(line: &str) -> Value {
        assert_eq!(line.matches('\n').count(), 1, "one request for the one click");
        serde_json::from_str(line.trim()).unwrap()
    }

    #[test]
    fn reconnect_is_one_request_for_a_server_that_failed() {
        let mut plugins = with_servers();
        let line = sent(&plugins.reconnect("broken", 7).expect("a request"));
        assert_eq!((line["request_id"].as_str(), &line["request"]), (Some("server-reconnect-7"), &json!({ "subtype": "mcp_reconnect", "serverName": "broken" })));
        assert_eq!(status(&plugins, "broken"), "pending");
        for name in ["issues", "tickets", "notes", "nothere"] {
            assert_eq!(plugins.reconnect(name, 8), None, "{name}");
        }
        // Claude Code reconnects, and only a new read says how it went.
        plugins.answered("server-reconnect-7", &json!({ "subtype": "success", "request_id": "server-reconnect-7", "response": {} }));
        assert!(plugins.stale);
    }

    #[test]
    fn a_refused_reconnect_puts_the_status_back_and_says_why() {
        let mut plugins = with_servers();
        plugins.reconnect("broken", 3);
        plugins.answered("server-reconnect-3", &json!({ "subtype": "error", "request_id": "server-reconnect-3", "error": "Server not found: broken" }));
        assert_eq!((status(&plugins, "broken").as_str(), plugins.error.as_deref()), ("failed", Some("Server not found: broken")));
    }

    #[test]
    fn sign_in_is_one_request_for_an_http_server_that_wants_it_and_its_page_comes_back_to_open() {
        let mut plugins = with_servers();
        let line = sent(&plugins.authenticate("issues", 4).expect("a request"));
        assert_eq!((line["request_id"].as_str(), &line["request"]), (Some("server-auth-4"), &json!({ "subtype": "mcp_authenticate", "serverName": "issues" })));
        // Not a server that is connected, one of claude.ai's own, a stdio one or one that isn't there.
        for name in ["tickets", "claude.ai Drive", "broken", "nothere"] {
            assert_eq!(plugins.authenticate(name, 5), None, "{name}");
        }
        let answer = json!({ "authUrl": "https://auth.example.com/authorize?state=x", "requiresUserAction": true, "callbackExpected": true, "callbackPort": 63305 });
        plugins.answered("server-auth-4", &json!({ "subtype": "success", "request_id": "server-auth-4", "response": answer }));
        assert_eq!(plugins.auth, Some(Auth { server: "issues".into(), url: "https://auth.example.com/authorize?state=x".into() }));
        // Reading the servers again is what comes next, so the page goes.
        plugins.ask();
        assert_eq!(plugins.auth, None);
    }

    #[test]
    fn a_sign_in_page_that_is_not_https_is_not_offered_and_one_with_no_page_asks_for_a_new_read() {
        let mut plugins = with_servers();
        plugins.authenticate("issues", 1);
        let answer = json!({ "authUrl": "http://127.0.0.1:18765/authorize", "requiresUserAction": true });
        plugins.answered("server-auth-1", &json!({ "subtype": "success", "request_id": "server-auth-1", "response": answer }));
        assert!(plugins.auth.is_none() && plugins.error.as_deref().is_some_and(|why| why.contains("isn't https")));
        plugins.error = None;
        plugins.authenticate("issues", 2);
        plugins.answered("server-auth-2", &json!({ "subtype": "success", "request_id": "server-auth-2", "response": {} }));
        assert!(plugins.auth.is_none() && plugins.stale && plugins.error.is_none());
        plugins.authenticate("issues", 3);
        plugins.answered("server-auth-3", &json!({ "subtype": "error", "request_id": "server-auth-3", "error": "Server type \"stdio\" does not support OAuth authentication" }));
        assert!(plugins.error.as_deref().is_some_and(|why| why.contains("does not support OAuth")));
    }

    #[test]
    fn sign_out_is_one_request_for_a_connected_http_server_and_shows_it_needing_sign_in() {
        let mut plugins = with_servers();
        let line = sent(&plugins.sign_out("tickets", 6).expect("a request"));
        assert_eq!((line["request_id"].as_str(), &line["request"]), (Some("server-sign-out-6"), &json!({ "subtype": "mcp_clear_auth", "serverName": "tickets" })));
        assert_eq!(status(&plugins, "tickets"), "needs-auth");
        for name in ["notes", "issues", "broken", "claude.ai Drive"] {
            assert_eq!(plugins.sign_out(name, 9), None, "{name}");
        }
        plugins.answered("server-sign-out-6", &json!({ "subtype": "error", "request_id": "server-sign-out-6", "error": "Cannot clear auth" }));
        assert_eq!(status(&plugins, "tickets"), "connected");
    }

    #[test]
    fn after_an_install_one_reload_and_one_read_of_the_settings_go_out() {
        let mut plugins = Plugins::default();
        let sent: Vec<Value> = plugins.after_install().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        let subtypes: Vec<&str> = sent.iter().filter_map(|line| line["request"]["subtype"].as_str()).collect();
        assert_eq!(subtypes, ["reload_plugins", "get_settings"]);
        assert!(plugins.unapplied);
    }
}
