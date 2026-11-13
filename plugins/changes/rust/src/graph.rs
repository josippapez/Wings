//! The commit graph: every branch of the repo the main script names, newest first. The app scrolls it and asks for
//! the rows it shows, so only those cross over.

use crate::{
    COMMIT, DONE, GRAPH_REPO, GRAPH_WANT,
    dates::date_of,
    layout::{Placed, layout},
    log::{LOG, parse_log},
};
use serde_json::{Value, json};
use std::cell::RefCell;
use wings_plugin::{
    format::grouped,
    git::raw,
    messages,
    task::spawn,
    ui::{Graph, Node, empty, graph, muted, notice, page, row, spinner, toolbar_with},
};

/// The newest commits the graph reads.
const MAX_COMMITS: usize = 10_000;

struct Page {
    root: Option<String>,
    name: Option<String>,
    rows: Vec<Placed>,
    lanes: usize,
    loading: bool,
    error: Option<String>,
    selected: i64,
    /// The rows the app last asked for.
    range: (usize, usize),
    /// Goes up with each load: an older one that finds a later number drops out.
    loads: u64,
}

thread_local! {
    static PAGE: RefCell<Page> = const { RefCell::new(Page { root: None, name: None, rows: Vec::new(), lanes: 1, loading: true, error: None, selected: -1, range: (0, 64), loads: 0 }) };
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

/// Shows the repo main named, reading its history again.
fn load(repo: &Value) {
    let (root, name) = (repo["root"].as_str().map(str::to_owned), repo["name"].as_str().map(str::to_owned));
    let mine = with(|page| {
        page.loads += 1;
        (page.loading, page.error) = (true, None);
        if page.root != root {
            (page.rows, page.lanes, page.selected, page.range) = (Vec::new(), 1, -1, (0, 64));
        }
        (page.root, page.name) = (root.clone(), name);
        page.loads
    });
    spawn(async move {
        let read = match &root {
            Some(root) => raw(root, &[&["log", "--all", "--topo-order"], &LOG[..], &[&format!("--max-count={MAX_COMMITS}")]].concat()).await.map(|log| layout(parse_log(&log))),
            None => Ok((Vec::new(), 1)),
        };
        with(|page| {
            if page.loads != mine {
                return;
            }
            page.loading = false;
            match read {
                Ok((rows, lanes)) => (page.rows, page.lanes, page.selected) = (rows, lanes, -1),
                Err(error) => page.error = Some(error),
            }
        });
    });
}

fn view() -> Vec<Node> {
    PAGE.with(|state| {
        let p = &*state.borrow();
        let total = p.rows.len();
        let refresh = Node::new("Button").prop("variant", "outline").on("onClick", |_| messages::tell(GRAPH_WANT, &Value::Null)).text("Refresh");
        let refresh = if p.loading || p.root.is_none() { refresh.prop("disabled", true) } else { refresh };
        let actions = row().prop("gap", "xs").maybe(p.loading.then(|| spinner().prop("label", "Loading"))).child(refresh);
        let said = if p.loading {
            "Loading…".to_owned()
        } else {
            format!("{} commits on every branch{}", grouped(total as u64), if total >= MAX_COMMITS { ", the newest 10,000" } else { "" })
        };
        let offset = p.range.0.min(total);
        let body = if total > 0 {
            let rows = p.rows[offset..p.range.1.min(offset + 256).min(total).max(offset)]
                .iter()
                .map(|placed| {
                    let c = &placed.commit;
                    json!({ "hash": c.hash, "subject": c.subject, "author": c.author, "date": date_of(c.at), "refs": c.refs, "lane": placed.lane, "color": placed.color, "merge": c.parents.len() > 1, "lines": placed.lines })
                })
                .collect();
            let shown = graph(Graph { total, lanes: p.lanes, selected: p.selected, offset, rows }, |start, end| with(|page| page.range = (start, end)), |index| {
                let asked = with(|page| {
                    let c = &page.rows.get(index)?.commit;
                    let asked = json!({ "id": index, "root": page.root.clone()?, "hash": c.hash, "subject": c.subject, "author": c.author, "at": c.at });
                    page.selected = index as i64;
                    Some(asked)
                });
                if let Some(asked) = asked {
                    messages::tell(COMMIT, &asked);
                }
            });
            Some(shown.key("graph"))
        } else if p.loading {
            None
        } else {
            Some(empty(if p.root.is_some() { "No commits yet." } else { "No git repo in your panes." }, "").key("none"))
        };
        let content = p.error.as_ref().map(|error| notice("warn", error).key("error")).into_iter().chain(body);
        vec![page(toolbar_with(p.name.as_deref().unwrap_or("Commit graph"), muted(said), Some(actions)), content.collect::<Vec<_>>()).prop("flush", true)]
    })
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
    messages::on(GRAPH_REPO, load);
    messages::on(DONE, |done| {
        if let Some(error) = done["error"].as_str() {
            with(|page| page.error = Some(error.to_owned()));
        }
    });
    messages::tell(GRAPH_WANT, &Value::Null);
}
