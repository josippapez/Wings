//! What a new tab shows before it has an address: the project's running dev servers, then the pages last visited. A click on
//! either opens it in the tab.

use crate::history::Entry;
use serde_json::Value;
use wings_plugin::ui::{Node, dot, list_button, list_row, muted, section_label, stack, text};

/// The most dev servers and recent pages shown.
pub const SERVERS: usize = 6;
pub const RECENT: usize = 6;

/// A program of the project's terminals that listens on a local port.
#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub name: String,
    pub port: u64,
}

impl Server {
    pub fn url(&self) -> String {
        format!("http://localhost:{}/", self.port)
    }
}

/// The ports the project's own terminals listen on, lowest first, one row for a port. `ports` is what `wings.ports` answered,
/// `panes` what `wings.onPanes` last said and `project` what `wings.project` answered: a pane is the project's when its
/// `projectId` is the project's `id` or its folder is inside the project's folder.
pub fn servers(ports: &Value, panes: &[Value], project: &Value) -> Vec<Server> {
    let (id, path) = (project["id"].as_str().unwrap_or_default(), project["path"].as_str().unwrap_or_default().trim_end_matches('/'));
    let ours = |pane: &str| {
        panes.iter().any(|p| {
            p["paneId"] == pane
                && ((!id.is_empty() && p["projectId"] == id) || (!path.is_empty() && p["cwd"].as_str().is_some_and(|cwd| cwd == path || cwd.starts_with(&format!("{path}/")))))
        })
    };
    let mut found: Vec<Server> = ports
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["paneId"].as_str().is_some_and(ours))
        .filter_map(|p| Some(Server { name: p["name"].as_str().filter(|name| !name.is_empty()).unwrap_or("Server").to_owned(), port: p["port"].as_u64()? }))
        .collect();
    found.sort_by_key(|server| server.port);
    found.dedup_by_key(|server| server.port);
    found.truncate(SERVERS);
    found
}

/// The newest `limit` pages of `history`, leaving out the ones a dev server above already opens.
pub fn recent<'a>(history: &'a [Entry], servers: &[Server], limit: usize) -> Vec<&'a Entry> {
    history.iter().filter(|entry| !servers.iter().any(|server| entry.url.trim_end_matches('/') == server.url().trim_end_matches('/'))).take(limit).collect()
}

/// An address without its scheme, `www.` and a lone trailing slash.
fn short(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.strip_prefix("www.").unwrap_or(rest).trim_end_matches('/')
}

fn entry_row(key: String, lead: Node, name: String, detail: String, open: impl Fn() + 'static) -> Node {
    list_row(key, list_button([lead, text(name).prop("truncate", true), muted(detail).prop("size", "small").prop("truncate", true).prop("grow", true)], open))
}

/// The page: the running servers when there are any, then the recent pages. `open` is given an address to go to.
pub fn view(servers: &[Server], recent: &[&Entry], open: fn(&str)) -> Node {
    let running = (!servers.is_empty()).then(|| {
        let rows = servers.iter().map(|server| {
            let url = server.url();
            entry_row(url.clone(), dot().prop("tone", "success"), server.name.clone(), format!("localhost:{}", server.port), move || open(&url))
        });
        stack().prop("gap", "s").child(section_label("Running in this project")).child(Node::new("List").children(rows))
    });
    let visited = (!recent.is_empty()).then(|| {
        let rows = recent.iter().map(|entry| {
            let url = entry.url.clone();
            let name = if entry.title.is_empty() { short(&entry.url).to_owned() } else { entry.title.clone() };
            entry_row(entry.url.clone(), dot().prop("tone", "muted"), name, short(&entry.url).to_owned(), move || open(&url))
        });
        stack().prop("gap", "s").child(section_label("Recent")).child(Node::new("List").children(rows))
    });
    let body = stack().prop("gap", "l").prop("grow", true).maybe(running).maybe(visited);
    Node::new("Scroll").child(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project() -> Value {
        json!({ "id": "/work/acme", "path": "/work/acme", "name": "acme" })
    }

    fn panes() -> Vec<Value> {
        vec![
            json!({ "paneId": "1", "cwd": "/work/acme/apps/web", "projectId": "/work/acme" }),
            json!({ "paneId": "2", "cwd": "/work/acme-other", "projectId": "/work/other" }),
            json!({ "paneId": "3", "cwd": "/work/acme/worktree", "projectId": null }),
        ]
    }

    #[test]
    fn only_the_projects_own_terminals_count_and_the_lowest_port_comes_first() {
        let ports = json!([
            { "paneId": "1", "name": "node", "port": 5173 },
            { "paneId": "2", "name": "python", "port": 8000 },
            { "paneId": "3", "name": "storybook", "port": 3000 },
            { "paneId": "1", "name": "node", "port": 5173 },
            { "paneId": "9", "name": "ghost", "port": 1 },
        ]);
        let found = servers(&ports, &panes(), &project());
        assert_eq!(found, [Server { name: "storybook".into(), port: 3000 }, Server { name: "node".into(), port: 5173 }]);
        assert_eq!(found[0].url(), "http://localhost:3000/");
    }

    #[test]
    fn a_folder_beside_the_project_with_the_same_start_is_not_inside_it() {
        let ports = json!([{ "paneId": "x", "name": "node", "port": 1 }]);
        let beside = [json!({ "paneId": "x", "cwd": "/work/acme-other", "projectId": null })];
        assert!(servers(&ports, &beside, &project()).is_empty());
        assert!(servers(&json!(null), &panes(), &project()).is_empty());
    }

    fn entry(url: &str, title: &str) -> Entry {
        Entry { url: url.into(), title: title.into(), at: 0.0 }
    }

    #[test]
    fn recent_pages_are_the_newest_few_without_the_dev_servers_already_listed() {
        let history = [entry("http://localhost:5173/", "App"), entry("https://a.com/", "A"), entry("https://b.com/", "B"), entry("http://localhost:9999/", "Old app"), entry("https://c.com/", "C")];
        let running = [Server { name: "node".into(), port: 5173 }];
        let shown: Vec<&str> = recent(&history, &running, 3).iter().map(|e| e.url.as_str()).collect();
        assert_eq!(shown, ["https://a.com/", "https://b.com/", "http://localhost:9999/"]);
        assert_eq!(recent(&history, &[], 10).len(), 5);
    }

    #[test]
    fn an_address_is_shown_without_its_scheme_and_www() {
        assert_eq!(short("https://www.rust-lang.org/"), "rust-lang.org");
        assert_eq!(short("http://localhost:3000/a/b"), "localhost:3000/a/b");
    }

    #[test]
    fn the_page_lists_servers_then_recent_pages_and_is_empty_of_sections_with_nothing_to_show() {
        let history = [entry("https://a.com/", "A")];
        let running = [Server { name: "node".into(), port: 5173 }];
        let tree = wings_plugin::ui::tree_json(view(&running, &recent(&history, &running, RECENT), |_| {})).to_string();
        let (servers_at, recent_at) = (tree.find("Running in this project"), tree.find("Recent"));
        assert!(servers_at.is_some() && recent_at.is_some() && servers_at < recent_at, "{tree}");
        let none = wings_plugin::ui::tree_json(view(&[], &[], |_| {})).to_string();
        assert!(!none.contains("Running") && !none.contains("Recent"), "{none}");
    }
}
