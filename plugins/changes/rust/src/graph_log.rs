//! The commit graph's state and the node that shows it, shared by the graph window and the sidebar's Graph section.
//! Each frame is its own module, so each has its own copy of the state. The app scrolls the graph and asks for the
//! rows it shows, so only those cross over.

use crate::{
    dates::date_of,
    layout::{Placed, layout},
    log::{LOG, parse_log},
};
use serde_json::{Value, json};
use std::cell::RefCell;
use wings_plugin::{
    git::raw,
    ui::{Graph, Node, empty, graph, notice},
};

/// The newest commits the graph reads.
pub const MAX_COMMITS: usize = 10_000;

pub struct Log {
    pub root: Option<String>,
    pub name: Option<String>,
    pub rows: Vec<Placed>,
    pub lanes: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub selected: i64,
    /// The rows the app last asked for.
    pub range: (usize, usize),
    /// Goes up with each load: an older one that finds a later number drops out.
    pub loads: u64,
}

thread_local! {
    static LOG_STATE: RefCell<Log> = const { RefCell::new(Log { root: None, name: None, rows: Vec::new(), lanes: 1, loading: true, error: None, selected: -1, range: (0, 64), loads: 0 }) };
}

pub fn with<T>(change: impl FnOnce(&mut Log) -> T) -> T {
    LOG_STATE.with(|log| change(&mut log.borrow_mut()))
}

/// Starts a read of `root`'s history, and returns the number to `finish` it with.
pub fn begin(root: Option<String>, name: Option<String>) -> u64 {
    with(|log| {
        log.loads += 1;
        (log.loading, log.error) = (true, None);
        if log.root != root {
            (log.rows, log.lanes, log.selected, log.range) = (Vec::new(), 1, -1, (0, 64));
        }
        (log.root, log.name) = (root, name);
        log.loads
    })
}

/// Reads the history of every branch of the repo at `root`, laid out in lanes.
pub async fn read(root: Option<&str>) -> Result<(Vec<Placed>, usize), String> {
    match root {
        Some(root) => raw(root, &[&["log", "--all", "--topo-order"], &LOG[..], &[&format!("--max-count={MAX_COMMITS}")]].concat()).await.map(|log| layout(parse_log(&log))),
        None => Ok((Vec::new(), 1)),
    }
}

/// Shows what `read` found, unless a later `begin` has taken over.
pub fn finish(mine: u64, read: Result<(Vec<Placed>, usize), String>) {
    with(|log| {
        if log.loads != mine {
            return;
        }
        log.loading = false;
        match read {
            Ok((rows, lanes)) => (log.rows, log.lanes, log.selected) = (rows, lanes, -1),
            Err(error) => log.error = Some(error),
        }
    });
}

/// The rows the app asked for, as the graph node takes them.
fn rows_json(log: &Log) -> (usize, Vec<Value>) {
    let total = log.rows.len();
    let offset = log.range.0.min(total);
    let rows = log.rows[offset..log.range.1.min(offset + 256).min(total).max(offset)]
        .iter()
        .map(|placed| {
            let c = &placed.commit;
            json!({ "hash": c.hash, "subject": c.subject, "author": c.author, "date": date_of(c.at), "refs": c.refs, "lane": placed.lane, "color": placed.color, "merge": c.parents.len() > 1, "lines": placed.lines })
        })
        .collect();
    (offset, rows)
}

/// The error and the graph, or what stands in for it. `open` hears what a click on a commit asks for:
/// `{ id, root, hash, subject, author, at }`, with `id` the row's index.
pub fn body(log: &Log, open: fn(Value)) -> Vec<Node> {
    let total = log.rows.len();
    let shown = if total > 0 {
        let (offset, rows) = rows_json(log);
        let graph = graph(Graph { total, lanes: log.lanes, selected: log.selected, offset, rows }, |start, end| with(|log| log.range = (start, end)), move |index| {
            let asked = with(|log| {
                let c = &log.rows.get(index)?.commit;
                let asked = json!({ "id": index, "root": log.root.clone()?, "hash": c.hash, "subject": c.subject, "author": c.author, "at": c.at });
                log.selected = index as i64;
                Some(asked)
            });
            if let Some(asked) = asked {
                open(asked);
            }
        });
        Some(graph.key("graph"))
    } else if log.loading {
        None
    } else {
        Some(empty(if log.root.is_some() { "No commits yet." } else { "No git repo in your panes." }, "").key("none"))
    };
    log.error.as_ref().map(|error| notice("warn", error).key("error")).into_iter().chain(shown).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::tests::log_of;

    fn loaded(range: (usize, usize)) -> Log {
        let (rows, lanes) = layout(parse_log(&log_of(&[("m", "a b", "", "merge"), ("a", "r", "HEAD -> refs/heads/main", "a"), ("b", "r", "", "b"), ("r", "", "", "root")])));
        Log { root: Some("/repo".into()), name: None, rows, lanes, loading: false, error: None, selected: -1, range, loads: 1 }
    }

    #[test]
    fn the_rows_a_page_shows_come_from_the_log_in_the_window_the_app_asked_for() {
        let (offset, rows) = rows_json(&loaded((1, 3)));
        assert_eq!(offset, 1);
        assert_eq!(rows.iter().map(|row| row["subject"].as_str().unwrap()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!((rows[0]["refs"][0].as_str(), rows[0]["lane"].as_u64(), rows[0]["merge"].as_bool()), (Some("H:main"), Some(0), Some(false)));
        assert_eq!(rows_json(&loaded((0, 64))).1.len(), 4);
        // A window past the end shows nothing, not a panic.
        assert!(rows_json(&loaded((10, 20))).1.is_empty());
    }

    #[test]
    fn a_log_with_commits_shows_the_graph_and_one_without_says_so() {
        let open: fn(Value) = |_| {};
        assert_eq!(body(&loaded((0, 64)), open).len(), 1);
        let mut empty = loaded((0, 64));
        empty.rows.clear();
        assert_eq!(body(&empty, open).len(), 1);
        empty.loading = true;
        assert!(body(&empty, open).is_empty());
        empty.error = Some("git failed".into());
        assert_eq!(body(&empty, open).len(), 1);
    }
}
