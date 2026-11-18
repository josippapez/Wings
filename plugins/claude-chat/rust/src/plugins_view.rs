//! The plugins dialog over the chat, which `/plugins`, `/plugin` and `/mcp` open, as Claude Code's own /plugin screen
//! lists what is installed: the plugins your settings name, each with its marketplace and skills and its MCP servers
//! under it, then the other MCP servers with their status. Each has a checkbox that turns it on or off, one request a
//! click, and a search goes over them all. Its Discover view lists the plugins your marketplaces offer, each with an
//! Install button.

use wings_plugin::{
    format::plural,
    host, task,
    ui::{Node, badge, button, checkbox, chips, dialog as panel, empty, heading, loading, muted, notice, row, search_field, stack, text, title},
};

use crate::{
    chat::{self, Chat},
    plugins::{Available, Dialog, Discover, Plugin, Server, Show, Tab, install_outcome, skills},
};

const WIDTH: u32 = 640;
/// An install clones the plugin's repository, which the usual 30 seconds may not do.
const INSTALL_MS: u32 = 120_000;
/// The most nodes the dialog sends: the conversation under it may take 3,500 of the 5,000 a page may draw.
const BUDGET: usize = 1_000;

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
        small("Plugins turn on or off for this chat only, once reloaded, and your settings stay as they are. MCP servers turn on or off for this project, as /mcp does.")
    };
    let stale = (plugins.stale && !discovering).then(|| small("Refresh to see how the servers you turned on connected."));
    let error = plugins.error.as_deref().map(|why| notice("danger", why));
    let body = if discovering {
        Some(discover(open))
    } else {
        match (&plugins.list, &plugins.servers) {
            (Some(list), Some(servers)) => Some(lists(c, &open.query, open.show, list, servers)),
            _ if plugins.error.is_some() => None,
            _ => Some(loading()),
        }
    };
    let footer = small("To add marketplaces, use /plugin in Claude Code in a terminal.");
    Some(panel(title, WIDTH, close).child(tabs).child(bar).child(hint).maybe(stale).maybe(error).maybe(body).child(footer))
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
struct Budget {
    spent: usize,
    left: usize,
}

impl Budget {
    fn take(&mut self, row: Node) -> Option<Node> {
        let size = row.count();
        if self.left > 0 || self.spent + size > BUDGET {
            self.left += 1;
            return None;
        }
        self.spent += size;
        Some(row)
    }
}

fn lists(c: &Chat, query: &str, show: Show, list: &[Plugin], servers: &[Server]) -> Node {
    if list.is_empty() && servers.is_empty() {
        return empty("No plugins or MCP servers", "Claude Code has none set up here. Install plugins with /plugin in a terminal.");
    }
    let wanted = query.trim().to_lowercase();
    let found = |name: &str| name.to_lowercase().contains(&wanted);
    let mut budget = Budget::default();
    let sections: Vec<Node> = match show {
        Show::Plugins => [plugin_section(&mut budget, c, list, servers, &found), server_section(&mut budget, servers, &found)],
        Show::Mcp => [server_section(&mut budget, servers, &found), plugin_section(&mut budget, c, list, servers, &found)],
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

/// A section of the dialog: its heading over its groups, each a label over its rows. None when no group has a row.
fn section(title: &str, groups: Vec<(&str, Vec<Node>)>) -> Option<Node> {
    let groups: Vec<Node> = groups
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(label, rows)| stack().prop("gap", "xs").child(text(label).prop("size", "small").prop("weight", "medium").prop("tone", "muted")).children(rows))
        .collect();
    (!groups.is_empty()).then(|| stack().prop("gap", "m").child(heading(3, title)).children(groups))
}

/// The plugins by the settings that decide them, as Claude Code groups them, each with its servers under it.
fn plugin_section(budget: &mut Budget, c: &Chat, list: &[Plugin], servers: &[Server], found: &dyn Fn(&str) -> bool) -> Option<Node> {
    let mut groups = Vec::new();
    for scope in ["User", "Project", "Local", "Managed", "Session"] {
        let mut rows = Vec::new();
        for plugin in list.iter().filter(|plugin| plugin.scope == scope) {
            let itself = found(&plugin.name) || found(&plugin.marketplace);
            let own: Vec<&Server> = servers
                .iter()
                .filter(|server| server.of_plugin().is_some_and(|(of, _)| of == plugin.name) && (itself || found(server.short_name())))
                .collect();
            if !itself && own.is_empty() {
                continue;
            }
            rows.extend(budget.take(plugin_row(plugin, skills(&c.session.commands, &plugin.name))));
            for server in own {
                rows.extend(budget.take(server_row(server, true)));
            }
        }
        groups.push((scope, rows));
    }
    section("Plugins", groups)
}

/// The servers that aren't a plugin's, by where they are set up: yours, the project's, the local ones and claude.ai's.
fn server_section(budget: &mut Budget, servers: &[Server], found: &dyn Fn(&str) -> bool) -> Option<Node> {
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
        let rows: Vec<Node> = own.iter().filter(|server| server.scope == scope).filter_map(|server| budget.take(server_row(server, false))).collect();
        groups.push((label, rows));
    }
    section("MCP servers", groups)
}

/// A plugin: its checkbox and name, its marketplace and how many skills it brings, and whether this chat turned it on or
/// off apart from your settings.
fn plugin_row(plugin: &Plugin, skills: usize) -> Node {
    let id = plugin.id.clone();
    let toggle = checkbox(plugin.name.as_str(), plugin.on, move |on| set_plugin(&id, on)).prop("disabled", plugin.locked());
    let facts: Vec<String> = [plugin.marketplace.clone()].into_iter().chain((skills > 0).then(|| plural(skills, "skill"))).filter(|fact| !fact.is_empty()).collect();
    let note = if plugin.locked() {
        Some(muted("Set by your organization"))
    } else if plugin.on != plugin.saved {
        Some(text(if plugin.on { "On in this chat" } else { "Off in this chat" }).prop("tone", "info"))
    } else {
        None
    };
    row()
        .prop("gap", "s")
        .prop("align", "center")
        .key(plugin.id.as_str())
        .child(toggle)
        .child(muted(facts.join(" · ")).prop("size", "small").prop("truncate", true).prop("grow", true))
        .maybe(note.map(|note| note.prop("size", "small")))
}

/// An MCP server: its checkbox and name, its status, and its tools or why it failed. A plugin's is set in under it.
fn server_row(server: &Server, nested: bool) -> Node {
    let name = server.name.clone();
    let (said, tone) = status(&server.status);
    let mut state = text(said).prop("size", "small").prop("tone", tone);
    if server.status == "needs-auth" {
        state = state.prop("title", "Sign in with /mcp in Claude Code in a terminal");
    }
    let detail = match (&server.error, server.tools) {
        (Some(why), _) => Some(muted(why.as_str()).prop("size", "small").prop("truncate", true).prop("title", why.as_str())),
        (None, Some(tools)) if server.status == "connected" => Some(muted(plural(tools, "tool")).prop("size", "small")),
        _ => None,
    };
    row()
        .prop("gap", "s")
        .prop("align", "center")
        .key(server.name.as_str())
        .maybe(nested.then(|| stack().prop("width", 24)))
        .child(checkbox(server.short_name(), server.status != "disabled", move |on| set_server(&name, on)))
        .child(state)
        .child(stack().prop("grow", true).maybe(detail))
}

/// A server's status as the dialog says it, and its tone.
fn status(status: &str) -> (&str, &'static str) {
    match status {
        "connected" => ("Connected", "success"),
        "pending" => ("Connecting", "muted"),
        "needs-auth" => ("Needs sign-in", "warn"),
        "failed" => ("Failed", "danger"),
        "disabled" => ("Off", "muted"),
        other => (other, "muted"),
    }
}

fn close() {
    chat::with(|c| c.plugins = None);
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

/// Reads what the marketplaces offer: one command, its output about 2.5 MB for 4,000 plugins, under Wings' 8 MB limit.
fn list() {
    if !chat::with(|c| c.plugins.as_mut().is_some_and(|open| open.discover.begin())) {
        return;
    }
    task::spawn(async {
        let out = host::exec("claude", &["plugin", "list", "--available", "--json"]).await;
        chat::with(|c| {
            if let Some(open) = &mut c.plugins {
                open.discover.listed(out);
            }
        });
    });
}

/// Installs the plugin `id`, from a click on its Install button and nothing else, then reloads the plugins to load it.
fn install(id: &str) {
    let Some(args) = chat::with(|c| c.plugins.as_mut()?.discover.install(id)) else {
        return;
    };
    let id = id.to_owned();
    task::spawn(async move {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let outcome = install_outcome(&host::exec_timeout("claude", &args, INSTALL_MS).await);
        let installed = outcome.is_ok();
        chat::with(|c| {
            if let Some(open) = &mut c.plugins {
                open.discover.installed(&id, outcome);
            }
        });
        if installed {
            chat::write(|c, _| Some(c.session.plugins.after_install()));
        }
    });
}
