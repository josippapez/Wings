//! The plugins dialog over the chat, which `/plugins`, `/plugin` and `/mcp` open, as Claude Code's own /plugin screen
//! lists what is installed: the plugins your settings name, each with its marketplace and skills and its MCP servers
//! under it, then the other MCP servers with their status. Each has a checkbox that turns it on or off, one request a
//! click, and a search goes over them all. Its Discover view lists the plugins your marketplaces offer, each with an
//! Install button.

use std::collections::BTreeSet;

use wings_plugin::{
    format::plural,
    host, task,
    ui::{Node, badge, button, card, checkbox, chips, dialog as panel, disclosure, dot, empty, heading, loading, muted, notice, row, rows_card, search_field, stack, text, title},
};

use crate::{
    chat::{self, Chat},
    plugins::{Auth, Available, Dialog, Discover, Plugin, Server, Show, Tab, Tool, install_outcome, skills},
};

/// How long to wait before each new read of the servers after a reconnect (ms): about 21 seconds in all.
const WATCH_PAUSES: [u32; 5] = [700, 1_500, 3_000, 6_000, 10_000];
const WIDTH: u32 = 640;
/// An install clones the plugin's repository, which the usual 30 seconds may not do.
const INSTALL_MS: u32 = 120_000;
/// The most nodes the dialog sends: the conversation under it may take 3,500 of the 5,000 a page may draw.
const BUDGET: usize = 1_000;
/// A scope with more servers than this that need a sign-in folds them under one line.
const FOLD_SIGN_IN: usize = 8;
/// Width of the checkbox, which what sits under a server's name is set in by.
const CHECKBOX: u32 = 24;
/// How far a fold's trigger sets its chevron in.
const FOLD_PADDING: u32 = 6;
/// How many of a server's tools an unfolded list shows before its "Show N more".
const TOOLS_SHOWN: usize = 40;
/// Said once under a group whose servers need a sign-in this dialog can't start.
const SIGN_IN_ELSEWHERE: &str = "Sign in to these with /mcp in Claude Code in a terminal.";

pub fn dialog(c: &Chat) -> Option<Node> {
    let open = c.plugins.as_ref()?;
    let plugins = &c.session.plugins;
    let title = match open.show {
        Show::Plugins => "Plugins",
        Show::Mcp => "MCP servers",
    };
    let discovering = open.tab == Tab::Discover;
    let tabs = chips("View", &[("installed", "Installed"), ("discover", "Discover")], if discovering { "discover" } else { "installed" }, set_tab);
    let search = search_field(if discovering { "Search available plugins" } else { "Search plugins and servers" }, &open.query, set_query);
    let apply = plugins.unapplied.then(|| button("Reload plugins", "default", false, reload).prop("title", "Applies the plugins you turned on or off to this chat"));
    let refresh_title = if discovering { "Reads the list of plugins again" } else { "Reads the plugins and the servers' status again" };
    let bar = row()
        .prop("gap", "s")
        .prop("align", "center")
        .child(stack().prop("grow", true).child(search))
        .child(button("Refresh", "ghost", false, refresh).prop("title", refresh_title))
        .maybe(apply);
    let small = |said: &str| muted(said).prop("size", "small");
    let hint = if discovering {
        small("Install adds a plugin to your user settings and reloads the plugins, so this chat has it. Most installed first.")
    } else {
        small("A plugin turns on or off for this chat only, and your settings stay as they are. An MCP server turns on or off for this project.")
    };
    let stale = (plugins.stale && !plugins.watching && !discovering).then(|| small("Refresh to see how the servers you turned on connected."));
    let error = plugins.error.as_deref().map(|why| notice("danger", why));
    let auth = plugins.auth.as_ref().map(auth_row);
    let body = if discovering {
        Some(discover(open))
    } else {
        match (&plugins.list, &plugins.servers) {
            (Some(list), Some(servers)) => Some(lists(c, open, list, servers, title)),
            _ if plugins.error.is_some() => None,
            _ => Some(loading()),
        }
    };
    let footer = small("To add marketplaces, use /plugin in Claude Code in a terminal.");
    Some(panel(title, WIDTH, close).child(tabs).child(bar).child(hint).maybe(stale).maybe(error).maybe(auth).maybe(body).child(footer))
}

/// The plugins your marketplaces offer that match the search, the first `LISTED` of them, most installed first.
fn discover(open: &Dialog) -> Node {
    let found = &open.discover;
    let error = found.error.as_deref().map(|why| notice("danger", why));
    let done = found.done.as_deref().map(|id| notice("success", format!("Installed {id}.")));
    let body = match &found.list {
        None if found.error.is_some() => None,
        None => Some(loading()),
        Some(_) => {
            let (shown, left) = found.found(&open.query);
            if shown.is_empty() {
                Some(muted(format!("Nothing matches “{}”.", open.query.trim())))
            } else {
                let more = (left > 0).then(|| muted(format!("{left} more. Search to find them.")).prop("size", "small"));
                Some(stack().prop("gap", "m").children(shown.into_iter().map(|plugin| available_row(found, plugin))).maybe(more))
            }
        }
    };
    stack().prop("gap", "m").maybe(error).maybe(done).maybe(body)
}

/// A plugin a marketplace offers: its name and marketplace over its description, and its Install button, or why there
/// isn't one.
fn available_row(found: &Discover, plugin: &Available) -> Node {
    let id = plugin.id.clone();
    let action = if found.is_installed(&plugin.id) {
        badge("Installed", Some("success"))
    } else if plugin.command {
        muted("Install it with /plugin in a terminal").prop("size", "small").prop("title", "It is installed by running a command, which you accept in a terminal")
    } else {
        let busy = found.installing.as_deref() == Some(plugin.id.as_str());
        button("Install", "default", busy, move || install(&id)).prop("disabled", found.installing.is_some())
    };
    let name = row().prop("gap", "s").prop("align", "center").child(title(plugin.name.as_str())).child(muted(plugin.marketplace.as_str()).prop("size", "small"));
    let description = muted(plugin.description.as_str()).prop("size", "small").prop("truncate", true).prop("title", plugin.description.as_str());
    row()
        .prop("gap", "s")
        .prop("align", "center")
        .key(plugin.id.as_str())
        .child(stack().prop("grow", true).prop("gap", "xs").child(name).child(description))
        .child(action)
}

/// How many nodes the dialog's rows have taken, and how many rows were left out once they had taken all they may.
#[derive(Default)]
pub(crate) struct Budget {
    spent: usize,
    pub(crate) left: usize,
}

impl Budget {
    pub(crate) fn take(&mut self, row: Node) -> Option<Node> {
        let size = row.count();
        if self.left > 0 || self.spent + size > BUDGET {
            self.left += 1;
            return None;
        }
        self.spent += size;
        Some(row)
    }
}

fn lists(c: &Chat, open: &Dialog, list: &[Plugin], servers: &[Server], dialog: &str) -> Node {
    let (query, show, unfolded) = (open.query.as_str(), open.show, &open.unfolded);
    if list.is_empty() && servers.is_empty() {
        return empty("No plugins or MCP servers", "Claude Code has none set up here. Install plugins with /plugin in a terminal.");
    }
    let wanted = query.trim().to_lowercase();
    let found = |name: &str| name.to_lowercase().contains(&wanted);
    let mut budget = Budget::default();
    let sections: Vec<Node> = match show {
        Show::Plugins => [plugin_section(&mut budget, c, unfolded, list, servers, &found, dialog), server_section(&mut budget, unfolded, servers, &found, wanted.is_empty(), dialog)],
        Show::Mcp => [server_section(&mut budget, unfolded, servers, &found, wanted.is_empty(), dialog), plugin_section(&mut budget, c, unfolded, list, servers, &found, dialog)],
    }
    .into_iter()
    .flatten()
    .collect();
    if sections.is_empty() {
        return muted(format!("Nothing matches “{}”.", query.trim()));
    }
    let more = (budget.left > 0).then(|| muted(format!("{} more. Search to find them.", budget.left)).prop("size", "small"));
    stack().prop("gap", "l").children(sections).maybe(more)
}

/// A section of the dialog: its heading over its groups, each a label over a card of its rows, split by hairlines as
/// Settings groups its rows. None when no group has a row. A section named as the dialog is has no heading, which would
/// say it twice.
fn section(title: &str, dialog: &str, groups: Vec<(&str, Vec<Node>)>) -> Option<Node> {
    let groups: Vec<Node> = groups
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(label, rows)| stack().prop("gap", "s").child(text(label).prop("size", "small").prop("weight", "medium").prop("tone", "muted")).child(rows_card(rows)))
        .collect();
    (!groups.is_empty()).then(|| stack().prop("gap", "m").maybe((title != dialog).then(|| heading(3, title))).children(groups))
}

/// The plugins by the settings that decide them, as Claude Code groups them, each with its servers under it.
fn plugin_section(budget: &mut Budget, c: &Chat, unfolded: &BTreeSet<String>, list: &[Plugin], servers: &[Server], found: &dyn Fn(&str) -> bool, dialog: &str) -> Option<Node> {
    let mut groups = Vec::new();
    for scope in ["User", "Project", "Local", "Managed", "Session"] {
        let mut rows = Vec::new();
        for plugin in list.iter().filter(|plugin| plugin.scope == scope) {
            let itself = found(&plugin.name) || found(&plugin.marketplace);
            let mut own: Vec<&Server> = servers
                .iter()
                .filter(|server| server.of_plugin().is_some_and(|(of, _)| of == plugin.name) && (itself || found(server.short_name())))
                .collect();
            own.sort_by_key(|server| server.rank());
            if !itself && own.is_empty() {
                continue;
            }
            rows.extend(budget.take(plugin_row(plugin, skills(&c.session.commands, &plugin.name))));
            for server in &own {
                rows.extend(budget.take(server_row(server, true, unfolded)));
            }
            rows.extend(sign_in_note(&own).map(|note| indent(CHECKBOX * 2, note)));
        }
        groups.push((scope, rows));
    }
    section("Plugins", dialog, groups)
}

/// The servers that aren't a plugin's, by where they are set up: yours, the project's, the local ones and claude.ai's.
/// Each scope lists them connected first, and folds the ones that need a sign-in when there are many, unless a search
/// has already cut the list down (`fold` is false then).
fn server_section(budget: &mut Budget, unfolded: &BTreeSet<String>, servers: &[Server], found: &dyn Fn(&str) -> bool, fold: bool, dialog: &str) -> Option<Node> {
    let own: Vec<&Server> = servers.iter().filter(|server| server.of_plugin().is_none() && found(&server.name)).collect();
    let mut scopes: Vec<&str> = Vec::new();
    for server in &own {
        if !scopes.contains(&server.scope.as_str()) {
            scopes.push(&server.scope);
        }
    }
    scopes.sort_by_key(|scope| ["user", "project", "local", "claudeai"].iter().position(|known| known == scope).unwrap_or(4));
    let mut groups = Vec::new();
    for scope in scopes {
        let label = match scope {
            "user" => "User",
            "project" => "Project",
            "local" => "Local",
            "claudeai" => "claude.ai",
            other => other,
        };
        let mut ranked: Vec<&Server> = own.iter().copied().filter(|server| server.scope == scope).collect();
        ranked.sort_by_key(|server| server.rank());
        let mut rows = scope_rows(budget, unfolded, scope, &ranked, fold);
        rows.extend(sign_in_note(&ranked));
        groups.push((label, rows));
    }
    section("MCP servers", dialog, groups)
}

/// The rows of one scope's `ranked` servers. Past `FOLD_SIGN_IN` that need a sign-in they sit under one disclosure, in the
/// place the first of them would have, which lists them only once it is unfolded.
fn scope_rows(budget: &mut Budget, unfolded: &BTreeSet<String>, scope: &str, ranked: &[&Server], fold: bool) -> Vec<Node> {
    let waiting = ranked.iter().filter(|server| server.status == "needs-auth").count();
    if !fold || waiting <= FOLD_SIGN_IN {
        return ranked.iter().filter_map(|server| budget.take(server_row(server, false, unfolded))).collect();
    }
    let key = format!("signin:{scope}");
    let open = unfolded.contains(&key);
    // Only an unfolded body is built, so a long list of them costs nothing while it is folded.
    let body: Vec<Node> = if open { ranked.iter().filter(|server| server.status == "needs-auth").filter_map(|server| budget.take(server_row(server, false, unfolded))).collect() } else { Vec::new() };
    let fold_key = key.clone();
    let fold = disclosure(format!("{waiting} need sign-in"), stack().prop("gap", "xs").children(body)).prop("open", open).on("onOpenChange", move |_| toggle(&fold_key));
    // Its chevron sits where a server's tools fold has its own.
    let fold = indent(CHECKBOX - FOLD_PADDING, fold).key(key);
    let mut fold = Some(fold);
    let mut rows = Vec::new();
    for server in ranked {
        if server.status != "needs-auth" {
            rows.extend(budget.take(server_row(server, false, unfolded)));
        } else {
            // In the place the first of them would have, so the connected ones stay above it.
            rows.extend(fold.take());
        }
    }
    rows
}

/// The one line under a group that has servers needing a sign-in this dialog can't start, in place of the same line under each.
fn sign_in_note(servers: &[&Server]) -> Option<Node> {
    servers.iter().any(|server| server.status == "needs-auth" && !server.signs_in()).then(|| muted(SIGN_IN_ELSEWHERE).prop("size", "small"))
}

/// A plugin: its checkbox and name, its marketplace and how many skills it brings, and whether this chat turned it on or
/// off apart from your settings.
fn plugin_row(plugin: &Plugin, skills: usize) -> Node {
    let id = plugin.id.clone();
    let toggle = checkbox(plugin.name.as_str(), plugin.on, move |on| set_plugin(&id, on)).prop("disabled", plugin.locked());
    let facts: Vec<String> = [plugin.marketplace.clone()].into_iter().chain((skills > 0).then(|| plural(skills, "skill"))).filter(|fact| !fact.is_empty()).collect();
    let note = if plugin.locked() {
        Some(mark("Set by your organization", "muted"))
    } else if plugin.on != plugin.saved {
        Some(mark(if plugin.on { "On in this chat" } else { "Off in this chat" }, "info"))
    } else {
        None
    };
    row()
        .prop("gap", "s")
        .prop("align", "center")
        .key(plugin.id.as_str())
        .child(toggle)
        .child(muted(facts.join(" · ")).prop("size", "small").prop("truncate", true).prop("grow", true))
        .maybe(note)
}

/// A status at a row's end: a dot in its tone and the word, quiet beside the name.
fn mark(said: &str, tone: &str) -> Node {
    row().prop("gap", "s").prop("align", "center").child(dot().prop("tone", tone)).child(muted(said).prop("size", "small"))
}

/// An MCP server: its checkbox and name over its tools or why it failed, and at the row's end the button that fits its
/// status and the status as a badge, so every row's badge sits at the same right edge as a plugin's note. One that is off
/// has no badge: its checkbox says so. The tools are under a disclosure, unfolded when `unfolded` has it. A plugin's
/// server is set in under its plugin.
fn server_row(server: &Server, nested: bool, unfolded: &BTreeSet<String>) -> Node {
    let name = server.name.clone();
    let (said, tone) = status(&server.status);
    let key = format!("tools:{}", server.name);
    let folds = server.error.is_none() && server.status == "connected" && server.tools.is_some_and(|tools| tools > 0);
    let detail = match (&server.error, server.tools) {
        (Some(why), _) if server.status != "pending" => Some(muted(why.as_str()).prop("size", "small").prop("clamp", true).prop("title", why.as_str())),
        (None, Some(tools)) if server.status == "connected" && tools > 0 => Some(tools_fold(server, unfolded, key)),
        (None, Some(_)) if server.status == "connected" => Some(muted("No tools").prop("size", "small")),
        _ => None,
    };
    // The status and button keep to the name's line, so an unfolded tool list doesn't pull them down with it.
    let line = row()
        .prop("gap", "s")
        .prop("align", "center")
        .child(stack().prop("grow", true).child(checkbox(server.short_name(), server.status != "disabled", move |on| set_server(&name, on))))
        .maybe(server_action(server))
        .maybe((server.status != "disabled").then(|| mark(said, tone)));
    // What's under the name starts where the name does: a fold's chevron sits in its trigger's padding, so it is set that
    // much less far in.
    let inset = if nested { CHECKBOX } else { 0 } + CHECKBOX - if folds { FOLD_PADDING } else { 0 };
    stack()
        .prop("gap", "xs")
        .key(server.name.as_str())
        .child(if nested { indent(CHECKBOX, line) } else { line })
        .maybe(detail.map(|detail| indent(inset, detail)))
}

/// `node` set in by `inset` points.
fn indent(inset: u32, node: Node) -> Node {
    row().prop("gap", "none").child(stack().prop("width", inset)).child(stack().prop("grow", true).child(node))
}

/// A connected server's tool count, which unfolds to list them: each name over the start of its description. Past
/// `TOOLS_SHOWN` a "Show N more" lists the rest the dialog kept; what it didn't keep is only counted.
fn tools_fold(server: &Server, unfolded: &BTreeSet<String>, key: String) -> Node {
    let count = server.tools.unwrap_or(server.listed.len());
    let open = unfolded.contains(&key);
    // Only an unfolded body is built.
    let body = if open {
        let all_key = format!("alltools:{}", server.name);
        let shown = if unfolded.contains(&all_key) { server.listed.len() } else { server.listed.len().min(TOOLS_SHOWN) };
        let hidden = server.listed.len() - shown;
        let unkept = count.saturating_sub(server.listed.len());
        let more = if hidden > 0 {
            Some(row().key("more").child(button(format!("Show {hidden} more"), "ghost", false, move || toggle(&all_key)).prop("size", "sm")))
        } else {
            (unkept > 0).then(|| muted(format!("{unkept} more")).prop("size", "small"))
        };
        stack().prop("gap", "xs").children(server.listed[..shown].iter().map(tool_row)).maybe(more)
    } else {
        stack()
    };
    let fold_key = key.clone();
    disclosure(plural(count, "tool"), body).prop("open", open).on("onOpenChange", move |_| toggle(&fold_key)).key(key)
}

/// One tool on one line: its name in mono, then its description cut to the line, the whole in a tooltip.
fn tool_row(tool: &Tool) -> Node {
    let note = (!tool.description.is_empty()).then(|| muted(tool.description.as_str()).prop("size", "small").prop("truncate", true).prop("grow", true).prop("title", tool.description.as_str()));
    row().prop("gap", "s").prop("align", "baseline").key(tool.name.as_str()).child(text(tool.name.as_str()).prop("mono", true).prop("size", "small")).maybe(note)
}

/// What a server's row offers: Reconnect for one that failed, Sign in for one that wants it, Sign out for one that is.
fn server_action(server: &Server) -> Option<Node> {
    let name = server.name.clone();
    match (server.status.as_str(), server.signs_in()) {
        ("failed", _) => Some(button("Reconnect", "outline", false, move || reconnect(&name)).prop("size", "sm").prop("title", "Connect to it again")),
        ("needs-auth", true) => Some(button("Sign in", "outline", false, move || sign_in(&name)).prop("size", "sm").prop("title", "Claude Code starts the sign-in, and you finish it in your browser")),
        ("connected", true) => Some(button("Sign out", "outline", false, move || sign_out(&name)).prop("size", "sm").prop("title", "Forgets this server's sign-in")),
        _ => None,
    }
}

/// The page Claude Code gave to sign in at, with the site it is on, and a button that opens it.
fn auth_row(auth: &Auth) -> Node {
    let url = auth.url.clone();
    let site = auth.url.trim_start_matches("https://").split(['/', '?', '#']).next().unwrap_or_default();
    let said = stack()
        .prop("grow", true)
        .prop("gap", "xs")
        .child(text(format!("Sign in to {} at {site}", auth.server)).prop("weight", "medium"))
        .child(muted("Finish in your browser, then Refresh to see it connect.").prop("size", "small"));
    card().child(row().prop("gap", "s").prop("align", "center").child(said).child(button("Open sign-in page", "default", false, move || host::open_url(&url))))
}

/// A server's status as the dialog says it, and its tone.
fn status(status: &str) -> (&str, &'static str) {
    match status {
        "connected" => ("Connected", "success"),
        "pending" => ("Connecting", "info"),
        "needs-auth" => ("Needs sign-in", "warn"),
        "failed" => ("Failed", "danger"),
        "disabled" => ("Off", "muted"),
        other => (other, "muted"),
    }
}

fn close() {
    chat::with(|c| c.plugins = None);
}

/// Unfolds or folds what `key` names in the dialog.
fn toggle(key: &str) {
    chat::with(|c| {
        if let Some(open) = &mut c.plugins {
            open.toggle(key);
        }
    });
}

fn set_query(query: &str) {
    chat::with(|c| {
        if let Some(open) = &mut c.plugins {
            open.query = query.to_owned();
        }
    });
}

fn set_plugin(id: &str, on: bool) {
    let id = id.to_owned();
    chat::write(move |c, n| c.session.plugins.set_plugin(&id, on, n));
}

fn set_server(name: &str, on: bool) {
    let name = name.to_owned();
    chat::write(move |c, n| c.session.plugins.set_server(&name, on, n));
    watch_servers();
}

fn reconnect(name: &str) {
    let name = name.to_owned();
    chat::write(move |c, n| c.session.plugins.reconnect(&name, n));
    watch_servers();
}

/// Claude Code answers a reconnect or a turn on before the server has connected, and the answer holds no status. So the
/// servers are read again after a pause, a few times with longer ones, while one is still connecting and the dialog is
/// open: the host's timer, not a poll.
fn watch_servers() {
    let generation = chat::with(|c| c.session.plugins.watch());
    task::spawn(async move {
        for pause in WATCH_PAUSES {
            host::sleep(pause).await;
            if !chat::with(|c| c.plugins.is_some() && c.session.plugins.wants(generation)) {
                break;
            }
            chat::write(|c, _| c.session.plugins.recheck(generation));
        }
        chat::with(|c| c.session.plugins.unwatch(generation));
    });
}

fn sign_in(name: &str) {
    let name = name.to_owned();
    chat::write(move |c, n| c.session.plugins.authenticate(&name, n));
}

fn sign_out(name: &str) {
    let name = name.to_owned();
    chat::write(move |c, n| c.session.plugins.sign_out(&name, n));
}

fn reload() {
    chat::write(|c, _| Some(c.session.plugins.reload()));
}

fn refresh() {
    if chat::with(|c| c.plugins.as_ref().is_some_and(|open| open.tab == Tab::Discover)) {
        return list();
    }
    chat::write(|c, _| Some(c.session.plugins.ask()));
}

fn set_tab(tab: &str) {
    let tab = if tab == "discover" { Tab::Discover } else { Tab::Installed };
    let first = chat::with(|c| {
        let open = c.plugins.as_mut()?;
        // The search keeps its text, which the field holds itself.
        open.tab = tab;
        Some(tab == Tab::Discover && open.discover.list.is_none())
    });
    if first == Some(true) {
        list();
    }
}

/// The account the `claude` commands here run under, and the folder they run in: the chat's own, so that plugins land in
/// the account the chat's session is in.
fn account() -> (host::Account, String) {
    chat::with(|c| (chat::account(c, c.session.id.as_deref()), c.cwd.clone()))
}

/// Reads what the marketplaces offer: one command, its output about 2.5 MB for 4,000 plugins, under Wings' 8 MB limit.
fn list() {
    if !chat::with(|c| c.plugins.as_mut().is_some_and(|open| open.discover.begin())) {
        return;
    }
    let (account, cwd) = account();
    task::spawn(async move {
        let out = host::exec_as(&account, &cwd, "claude", &["plugin", "list", "--available", "--json"], None).await;
        chat::with(|c| {
            if let Some(open) = &mut c.plugins {
                open.discover.listed(out);
            }
        });
    });
}

/// Installs the plugin `id`, from a click on its Install button and nothing else, then reloads the plugins to load it and
/// reads the list again, as the plugins it lists under `optionalDependencies` install with it.
fn install(id: &str) {
    let Some(args) = chat::with(|c| c.plugins.as_mut()?.discover.install(id)) else {
        return;
    };
    let id = id.to_owned();
    let (account, cwd) = account();
    task::spawn(async move {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let outcome = install_outcome(&host::exec_as(&account, &cwd, "claude", &args, Some(INSTALL_MS)).await);
        let installed = outcome.is_ok();
        chat::with(|c| {
            if let Some(open) = &mut c.plugins {
                open.discover.installed(&id, outcome);
            }
        });
        if installed {
            chat::write(|c, _| Some(c.session.plugins.after_install()));
            list();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(name: &str, status: &str, scope: &str) -> Server {
        Server { name: name.into(), status: status.into(), scope: scope.into(), tools: None, listed: Vec::new(), error: None, transport: "http".into() }
    }

    fn tool(name: &str, description: &str) -> Tool {
        Tool { name: name.into(), description: description.into() }
    }

    /// A connected server with two kept tools, one of them without a description, and `extra` more it didn't keep.
    fn with_tools(extra: usize) -> Server {
        Server { tools: Some(2 + extra), listed: vec![tool("read_tree", "Lists a folder."), tool("find", "")], ..server("files", "connected", "user") }
    }

    #[test]
    fn a_connected_servers_tools_are_built_only_once_unfolded() {
        let server = with_tools(0);
        let closed = server_row(&server, false, &BTreeSet::new());
        let open = server_row(&server, false, &BTreeSet::from(["tools:files".to_owned()]));
        // A tool with a description is a stack, its mono name and its muted line, each with its text: 5 nodes. One
        // without is 3.
        assert_eq!(open.count(), closed.count() + 5 + 3);
        // Tools the dialog didn't keep are counted: "N more" is a text node and its text.
        let more = with_tools(7);
        let open_more = server_row(&more, false, &BTreeSet::from(["tools:files".to_owned()]));
        assert_eq!(open_more.count(), open.count() + 2);
    }

    #[test]
    fn a_row_has_its_status_as_a_badge_and_a_line_under_the_name() {
        let plain = server_row(&server("a", "disabled", "user"), false, &BTreeSet::new());
        let connected = server_row(&with_tools(0), false, &BTreeSet::new());
        let mut failed = server("b", "failed", "user");
        failed.error = Some("Connection closed".into());
        let failed = server_row(&failed, false, &BTreeSet::new());
        // The badge and its text come to 2 nodes in every row, and the line under the name adds its own row.
        assert!(connected.count() > plain.count() && failed.count() > plain.count());
        // Set in by a row holding a spacer and the line.
        assert_eq!(server_row(&server("a", "disabled", "user"), true, &BTreeSet::new()).count(), plain.count() + 3, "a plugin's server is set in");
    }

    fn many(waiting: usize) -> Vec<Server> {
        let mut all = vec![with_tools(0), server("down", "failed", "claudeai")];
        all.extend((0..waiting).map(|n| server(&format!("claude.ai {n}"), "needs-auth", "claudeai")));
        all.sort_by_key(Server::rank);
        all
    }

    #[test]
    fn more_than_eight_that_need_a_sign_in_fold_under_one_line_below_the_connected_ones() {
        let all = many(9);
        let ranked: Vec<&Server> = all.iter().collect();
        let mut budget = Budget::default();
        // The connected one, the fold, and the failed one.
        assert_eq!(scope_rows(&mut budget, &BTreeSet::new(), "claudeai", &ranked, true).len(), 3);
        let folded = budget.spent;
        // Searching has cut the list already, so nothing folds.
        assert_eq!(scope_rows(&mut Budget::default(), &BTreeSet::new(), "claudeai", &ranked, false).len(), 11);
        // Unfolded, the nine rows are built and take their share of the budget.
        let mut unfolded_budget = Budget::default();
        let rows = scope_rows(&mut unfolded_budget, &BTreeSet::from(["signin:claudeai".to_owned()]), "claudeai", &ranked, true);
        assert_eq!(rows.len(), 3);
        assert!(unfolded_budget.spent > folded + 9 * 5);
    }

    #[test]
    fn eight_or_fewer_that_need_a_sign_in_are_listed_each_in_its_own_row() {
        let all = many(8);
        let ranked: Vec<&Server> = all.iter().collect();
        assert_eq!(scope_rows(&mut Budget::default(), &BTreeSet::new(), "claudeai", &ranked, true).len(), 10);
    }

    #[test]
    fn a_section_named_as_the_dialog_has_no_heading_and_another_has() {
        let drawn = |dialog: &str| wings_plugin::ui::tree_json(section("MCP servers", dialog, vec![("User", vec![text("a")])]).expect("a section")).to_string();
        assert!(!drawn("MCP servers").contains("Heading"), "said twice");
        assert!(drawn("Plugins").contains("Heading"));
    }

    /// A server's status badge ends its row, after its button, at the right edge a plugin's note ends at; one that is
    /// off has none, as its checkbox says it.
    #[test]
    fn a_servers_badge_ends_its_row_and_one_that_is_off_has_none() {
        let line = |server: &Server| wings_plugin::ui::tree_json(server_row(server, false, &BTreeSet::new()))["children"][0].clone();
        let failed = line(&server("b", "failed", "user"));
        let children = failed["children"].as_array().unwrap();
        assert_eq!(children.len(), 3, "the name, Reconnect and the badge: {failed}");
        assert!(children[2].to_string().contains("Failed") && children[1].to_string().contains("Reconnect"), "{failed}");
        let off = line(&server("a", "disabled", "user"));
        assert_eq!(off["children"].as_array().unwrap().len(), 1, "{off}");
        assert!(!off.to_string().contains("\"Off\""), "{off}");
    }

    /// Past 40 tools an unfolded list has "Show N more", which lists the rest; tools the dialog didn't keep are counted.
    #[test]
    fn a_long_tool_list_shows_forty_then_a_button_for_the_rest() {
        let mut big = with_tools(0);
        big.listed = (0..45).map(|n| tool(&format!("tool_{n}"), "")).collect();
        big.tools = Some(45);
        let drawn = |server: &Server, unfolded: &[&str]| wings_plugin::ui::tree_json(server_row(server, false, &unfolded.iter().map(|key| (*key).to_owned()).collect())).to_string();
        let first = drawn(&big, &["tools:files"]);
        assert!(first.contains("tool_39") && !first.contains("tool_40") && first.contains("Show 5 more"), "{first}");
        let all = drawn(&big, &["tools:files", "alltools:files"]);
        assert!(all.contains("tool_44") && !all.contains("Show 5 more"), "{all}");
        big.tools = Some(200);
        let counted = drawn(&big, &["tools:files", "alltools:files"]);
        assert!(counted.contains("155 more") && !counted.contains("Show"), "{counted}");
    }

    /// Servers that need a sign-in this dialog can't start say how once under their group, not under each.
    #[test]
    fn the_sign_in_hint_is_said_once_for_a_group() {
        let waiting: Vec<Server> = (0..3).map(|n| Server { transport: "claudeai-proxy".into(), ..server(&format!("claude.ai {n}"), "needs-auth", "claudeai") }).collect();
        let ranked: Vec<&Server> = waiting.iter().collect();
        let rows = scope_rows(&mut Budget::default(), &BTreeSet::new(), "claudeai", &ranked, true);
        let drawn: String = rows.into_iter().map(|row| wings_plugin::ui::tree_json(row).to_string()).collect();
        assert!(!drawn.contains("/mcp"), "{drawn}");
        assert!(sign_in_note(&ranked).is_some());
        assert!(sign_in_note(&[&with_tools(0)]).is_none());
    }

    #[test]
    fn a_status_has_a_tone() {
        assert_eq!(status("connected"), ("Connected", "success"));
        assert_eq!(status("needs-auth"), ("Needs sign-in", "warn"));
        assert_eq!(status("failed"), ("Failed", "danger"));
        assert_eq!(status("disabled"), ("Off", "muted"));
        assert_eq!(status("pending"), ("Connecting", "info"));
    }

    #[test]
    fn a_plugin_that_another_installed_with_it_shows_as_installed_after_the_install() {
        use serde_json::{Value, json};
        use wings_plugin::host;
        const AVAILABLE: &str = include_str!("../tests/available.json");
        let mut listing: Value = serde_json::from_str(AVAILABLE).unwrap();
        let first = listing.to_string();
        // What the list says once api-guard is installed: pixel-perfect, which it lists under optionalDependencies, came with it.
        for id in ["api-guard@claude-plugins-official", "pixel-perfect@community"] {
            listing["installed"].as_array_mut().unwrap().push(json!({ "id": id, "scope": "user", "enabled": true }));
        }
        let after = listing.to_string();
        host::testing::answer(move |method, params| match (method, params["args"][1].as_str()) {
            ("exec", Some("install")) => Ok(json!({ "code": 0, "stdout": "{\"outcome\":\"ok\"}\n", "stderr": "" })),
            ("exec", Some("list")) => Ok(json!({ "code": 0, "stdout": after.clone(), "stderr": "" })),
            _ => Ok(Value::Null),
        });
        chat::with(|c| {
            let mut open = Dialog::new(Show::Plugins);
            open.discover.listed(Ok(json!({ "code": 0, "stdout": first, "stderr": "" })));
            c.plugins = Some(open);
        });
        install("api-guard@claude-plugins-official");
        wings_plugin::task::block_on(async {});
        chat::with(|c| {
            let found = &c.plugins.as_ref().unwrap().discover;
            assert!(found.is_installed("api-guard@claude-plugins-official"));
            assert!(found.is_installed("pixel-perfect@community"), "the plugin installed with it");
        });
    }
}
