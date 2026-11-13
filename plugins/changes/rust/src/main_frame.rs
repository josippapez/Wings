//! The main script: a badge on each pane with what changed in its repo, the tools Claude gets, and the diff viewer,
//! which a page asks it to open.

use crate::{
    COMMIT, DONE, GRAPH, GRAPH_REPO, GRAPH_WANT, OPEN, POLL_MS,
    changes::{Changes, Mode, changes, forget_repos},
    diff::{open_changes, open_commit},
    log::{LOG, parse_log},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, future::Future, pin::Pin, rc::Rc};
use wings_plugin::{
    diff::refresh_live,
    format::{basename, plural},
    git::{branch_label, ok, on_change, raw, repo_root},
    host, messages, poll,
    task::{join_all, spawn},
    tools,
};

thread_local! {
    /// The panes as Wings last listed them.
    static PANES: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    /// The repo the graph view was last opened on.
    static GRAPH_ROOT: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn panes() -> Vec<Value> {
    PANES.with(|panes| panes.borrow().clone())
}

fn lines(c: &Changes) -> String {
    format!("+{} −{}", c.added, c.removed)
}

fn badge_for(c: &Changes) -> Value {
    let mut by: BTreeMap<&str, usize> = BTreeMap::new();
    for file in &c.files {
        *by.entry(file.status.as_str()).or_default() += 1;
    }
    let kind = |status: &str| match status {
        "M" => "modified",
        "A" => "added",
        "D" => "deleted",
        "R" => "renamed",
        "?" => "new",
        other => other,
    }
    .to_owned();
    let summary = by.iter().map(|(status, n)| format!("{n} {}", kind(status))).collect::<Vec<_>>().join(", ");
    json!({
        "label": if c.added > 0 || c.removed > 0 { lines(c) } else { "changed".to_owned() },
        "tone": "neutral",
        "counts": [{ "icon": "changed", "value": c.files.len() }],
        "title": plural(c.files.len(), "changed file"),
        "subtitle": format!("{} · {}", c.name, c.branch),
        "rows": [
            { "label": "Files", "value": summary },
            { "label": "Lines", "value": if c.partial { format!("{}, not counting every new file", lines(c)) } else { lines(c) } },
        ],
        "actions": [
            { "id": "open", "label": "Open changes", "primary": true },
            { "id": "branch", "label": "Changes on this branch" },
            { "id": "sidebar", "label": "Show in sidebar" },
            { "id": "refresh", "label": "Refresh" },
        ],
    })
}

type Root = Pin<Box<dyn Future<Output = Option<String>>>>;

/// The repo each of `list`'s panes is in.
async fn roots_of(list: &[Value]) -> Vec<Option<String>> {
    let asked: Vec<Root> = list
        .iter()
        .map(|pane| -> Root {
            let cwd = pane["cwd"].as_str().unwrap_or_default().to_owned();
            Box::pin(async move { repo_root(&cwd).await })
        })
        .collect();
    join_all(asked).await
}

/// Looks up each repo once, then badges every pane in it.
async fn refresh_badges(list: Vec<Value>, fresh: bool) {
    let roots = roots_of(&list).await;
    let mut read: Vec<(String, Option<Rc<Changes>>)> = Vec::new();
    for root in roots.iter().flatten() {
        if read.iter().all(|(known, _)| known != root) {
            read.push((root.clone(), changes(root, Mode::Uncommitted, fresh).await.ok()));
        }
    }
    for (pane, root) in list.iter().zip(&roots) {
        let c = root.as_ref().and_then(|root| read.iter().find(|(known, _)| known == root)).and_then(|(_, c)| c.clone());
        let badge = c.filter(|c| !c.files.is_empty()).map_or(Value::Null, |c| badge_for(&c));
        let id = pane["paneId"].as_str().unwrap_or_default().to_owned();
        spawn(async move { drop(host::set_badge(&id, &badge).await) });
    }
}

/// The repo a view shows when nothing says which: the focused pane's, else the first pane's that is in one.
async fn shown_root() -> Option<String> {
    let list = panes();
    let roots = roots_of(&list).await;
    let focused = list.iter().position(|pane| pane["focused"] == true).and_then(|at| roots[at].clone());
    focused.or_else(|| roots.into_iter().flatten().next())
}

/// Readable refs for Claude, like `git log --decorate` shows them.
fn ref_name(name: &str) -> String {
    match name.split_once(':') {
        Some(("H", "HEAD")) => "HEAD".into(),
        Some(("H", branch)) => format!("HEAD -> {branch}"),
        Some(("t", tag)) => format!("tag: {tag}"),
        Some((_, rest)) => rest.to_owned(),
        None => name.to_owned(),
    }
}

/// The root of the repo that contains `dir`.
async fn root_of(dir: &str) -> Result<String, String> {
    repo_root(dir).await.ok_or_else(|| format!("{dir} is not inside a git repo."))
}

fn register_tools() {
    let folder = json!({ "type": "string", "description": "An absolute path to a folder inside the repo." });
    tools::register(
        "changed_files",
        json!({ "type": "object", "properties": { "cwd": folder, "against": { "type": "string", "enum": ["uncommitted", "branch"] } }, "required": ["cwd"], "additionalProperties": false }),
        |input, _| async move {
            let root = root_of(input["cwd"].as_str().unwrap_or_default()).await?;
            let c = changes(&root, Mode::named(input["against"].as_str().unwrap_or_default()), true).await?;
            let files: Vec<Value> = c
                .files
                .iter()
                .take(500)
                .map(|file| {
                    let mut out = json!({ "path": file.path, "status": file.status, "added": file.added, "removed": file.removed });
                    if let Some(old) = &file.old_path {
                        out["oldPath"] = old.clone().into();
                    }
                    out
                })
                .collect();
            Ok(json!({ "repo": c.root, "branch": c.branch, "against": c.against, "added": c.added, "removed": c.removed, "total": c.files.len(), "files": files }))
        },
    );
    tools::register(
        "recent_commits",
        json!({ "type": "object", "properties": { "cwd": folder, "limit": { "type": "integer", "minimum": 1, "maximum": 200 } }, "required": ["cwd"], "additionalProperties": false }),
        |input, _| async move {
            let root = root_of(input["cwd"].as_str().unwrap_or_default()).await?;
            let Some(head) = ok(&root, &["rev-parse", "--verify", "--quiet", "HEAD"]).await.filter(|head| !head.is_empty()) else {
                return Ok(json!({ "repo": root, "commits": [] }));
            };
            let max = format!("--max-count={}", input["limit"].as_u64().unwrap_or(20));
            let commits: Vec<Value> = parse_log(&raw(&root, &[&["log"], &LOG[..], &[&max]].concat()).await?)
                .iter()
                .map(|c| {
                    let mut out = json!({ "hash": c.hash, "subject": c.subject, "author": c.author, "date": crate::dates::date_time_of(c.at), "refs": c.refs.iter().map(|name| ref_name(name)).collect::<Vec<_>>() });
                    if c.parents.len() > 1 {
                        out["merge"] = true.into();
                    }
                    out
                })
                .collect();
            let branch = ok(&root, &["branch", "--show-current"]).await;
            Ok(json!({ "repo": root, "branch": branch_label(branch.as_deref(), Some(&head)), "commits": commits }))
        },
    );
}

/// Tells the page that asked under `id` how it went.
async fn answer(id: Value, done: impl Future<Output = Result<(), String>>) {
    let error = done.await.err();
    messages::tell(DONE, &json!({ "id": id, "error": error }));
}

async fn tell_graph_repo() {
    let root = match GRAPH_ROOT.with(|root| root.borrow().clone()) {
        Some(root) => Some(root),
        None => shown_root().await,
    };
    messages::tell(GRAPH_REPO, &json!({ "root": root, "name": root.as_deref().map(basename) }));
}

#[unsafe(no_mangle)]
pub fn start() {
    host::on_panes(|list| {
        let key = |pane: &Value| (pane["paneId"].clone(), pane["cwd"].clone());
        let moved = PANES.with(|panes| {
            let before: Vec<_> = panes.borrow().iter().map(key).collect();
            let moved = list.len() != before.len() || list.iter().any(|pane| !before.contains(&key(pane)));
            *panes.borrow_mut() = list.to_vec();
            moved
        });
        if moved {
            spawn(refresh_badges(list.to_vec(), false));
        }
    });
    // The badge counts edits to files in the working tree, which Wings doesn't watch, so it keeps a slow poll. Paused
    // while Wings is in the background, and run once when you come back.
    poll::while_focused(POLL_MS, false, || spawn(refresh_badges(panes(), false)));
    // A commit, checkout or pull shows at once in the panes of the repos that moved, and in an open diff viewer of
    // that repo.
    on_change(|_, roots| async move {
        let list = panes();
        let pane_roots = roots_of(&list).await;
        let moved: Vec<Value> = list.into_iter().zip(pane_roots).filter(|(_, root)| root.as_ref().is_some_and(|root| roots.contains(root))).map(|(pane, _)| pane).collect();
        refresh_badges(moved, true).await;
        refresh_live(Some(&roots));
    });
    host::on_action(|action| async move {
        let asked = action["actionId"].as_str().unwrap_or_default();
        if asked == "refresh" {
            forget_repos();
        }
        let pane = panes().into_iter().find(|pane| pane["paneId"] == action["paneId"]);
        let Some(root) = repo_root(pane.as_ref().and_then(|pane| pane["cwd"].as_str()).unwrap_or_default()).await else {
            return Ok(());
        };
        match asked {
            "open" => open_changes(root, Mode::Uncommitted, None).await,
            "branch" => open_changes(root, Mode::Branch, None).await,
            "refresh" => {
                refresh_badges(panes(), true).await;
                Ok(())
            }
            "sidebar" => {
                host::open_view("changes");
                Ok(())
            }
            _ => Ok(()),
        }
    });
    messages::on(OPEN, |asked| {
        let asked = asked.clone();
        let text = |name: &str| asked[name].as_str().map(str::to_owned);
        let opening = open_changes(text("root").unwrap_or_default(), Mode::named(&text("mode").unwrap_or_default()), text("path"));
        spawn(answer(asked["id"].clone(), opening));
    });
    messages::on(COMMIT, |asked| {
        let asked = asked.clone();
        spawn(async move { answer(asked["id"].clone(), open_commit(asked["root"].as_str().unwrap_or_default(), &asked)).await });
    });
    messages::on(GRAPH, |asked| {
        GRAPH_ROOT.with(|root| *root.borrow_mut() = asked["root"].as_str().map(str::to_owned));
        host::open_view("graph");
        let id = asked["id"].clone();
        // A graph view that is already open hears which repo it shows now.
        spawn(async move {
            tell_graph_repo().await;
            answer(id, async { Ok(()) }).await;
        });
    });
    messages::on(GRAPH_WANT, |_| spawn(tell_graph_repo()));
    register_tools();
}
