//! The commit graph window: every branch of the repo the main script names, newest first. The state and the graph
//! node are `graph_log`'s, which the sidebar's Graph section shows too.

use crate::{
    COMMIT, DONE, GRAPH_REPO, GRAPH_WANT,
    graph_log::{MAX_COMMITS, begin, body, finish, read, with},
};
use serde_json::Value;
use wings_plugin::{
    format::grouped,
    messages,
    task::spawn,
    ui::{muted, page, row, spinner, toolbar_with, Node},
};

/// Shows the repo main named, reading its history again.
fn load(repo: &Value) {
    let (root, name) = (repo["root"].as_str().map(str::to_owned), repo["name"].as_str().map(str::to_owned));
    let mine = begin(root.clone(), name);
    spawn(async move { finish(mine, read(root.as_deref()).await) });
}

fn view() -> Vec<Node> {
    with(|p| {
        let total = p.rows.len();
        let refresh = Node::new("Button").prop("variant", "outline").on("onClick", |_| messages::tell(GRAPH_WANT, &Value::Null)).text("Refresh");
        let refresh = if p.loading || p.root.is_none() { refresh.prop("disabled", true) } else { refresh };
        let actions = row().prop("gap", "xs").maybe(p.loading.then(|| spinner().prop("label", "Loading"))).child(refresh);
        let said = if p.loading {
            "Loading…".to_owned()
        } else {
            format!("{} commits on every branch{}", grouped(total as u64), if total >= MAX_COMMITS { ", the newest 10,000" } else { "" })
        };
        vec![page(toolbar_with(p.name.as_deref().unwrap_or("Commit graph"), muted(said), Some(actions)), body(p, |asked| messages::tell(COMMIT, &asked))).prop("flush", true)]
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
