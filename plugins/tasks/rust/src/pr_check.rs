//! Issues left in progress or in review after their pull request merged: every 10 minutes while Wings is focused, each live
//! issue's linked PR is looked up, and a merged one finishes the issue. An issue with a branch and no PR link gets the PR for
//! that branch looked up and linked. The PR tracker also says right away when a PR it knew merged or closed (`pr-state`).
//! Background reads: a missing or signed-out CLI only skips the issue.

use crate::{
    domain::{Actor, Issue, Op},
    journal::is_live,
    pr_state::{Command, FoundPr, PrState, PrStateEvent, branch_pr_command, branch_pr_of, found_ops, is_pr_of, parse_remote, pr_ops, pr_state_command, pr_state_event_of, pr_state_of},
    storage::read_store,
    writer::write,
};
use serde_json::Value;
use std::collections::HashMap;
use wings_plugin::{git, host, poll, task::spawn};

const EVERY_MS: u32 = 10 * 60 * 1000;

async fn state_of(url: &str) -> Option<PrState> {
    let (program, args) = pr_state_command(url)?;
    let result = host::exec(program, &args.iter().map(String::as_str).collect::<Vec<_>>()).await.ok()?;
    if result["code"] != 0 {
        return None;
    }
    pr_state_of(result["stdout"].as_str().unwrap_or_default())
}

/// The PR for a branch, looked up in the project's folder: the worktrees of a project share its remote.
async fn pr_of_branch(path: &str, branch: &str) -> Option<FoundPr> {
    let remote = parse_remote(&git::ok(path, &["remote", "get-url", "origin"]).await.unwrap_or_default());
    let (program, args): Command = branch_pr_command(&remote, branch)?;
    let result = host::exec_in(path, program, &args.iter().map(String::as_str).collect::<Vec<_>>()).await.ok()?;
    if result["code"] != 0 {
        return None;
    }
    branch_pr_of(&remote, result["stdout"].as_str().unwrap_or_default())
}

fn apply(id: &str, name: &str, ops: Vec<Op>) {
    // Found by the plugin itself, like a commit with the issue's key.
    for op in ops {
        drop(write(id, name, &op, &Actor::Git));
    }
}

pub async fn check_prs() {
    let mut states: HashMap<String, Option<PrState>> = HashMap::new();
    let projects = host::projects().await.unwrap_or_default();
    for project in projects.as_array().into_iter().flatten().filter(|p| p["parentId"].is_null()) {
        let (id, name, path) = (project["id"].as_str().unwrap_or_default(), project["name"].as_str().unwrap_or_default(), project["path"].as_str().unwrap_or_default());
        let Ok(store) = read_store(id, name) else {
            continue;
        };
        let mut branches: HashMap<String, Option<FoundPr>> = HashMap::new();
        let live = |i: &&Issue| is_live(i);
        for issue in store.issues.iter().filter(live).filter(|i| i.links.as_ref().is_some_and(|l| l.pr.is_none() && l.branch.is_some())) {
            let branch = issue.links.as_ref().and_then(|l| l.branch.clone()).unwrap_or_default();
            if !branches.contains_key(&branch) {
                let found = pr_of_branch(path, &branch).await;
                branches.insert(branch.clone(), found);
            }
            if let Some(found) = &branches[&branch] {
                apply(id, name, found_ops(issue, found, &store.issues));
            }
        }
        for issue in store.issues.iter().filter(live).filter(|i| i.links.as_ref().is_some_and(|l| l.pr.is_some())) {
            let url = issue.links.as_ref().and_then(|l| l.pr.clone()).unwrap_or_default();
            if !states.contains_key(&url) {
                let state = state_of(&url).await;
                states.insert(url.clone(), state);
            }
            if let Some(state) = states[&url] {
                apply(id, name, pr_ops(issue, state, &store.issues));
            }
        }
    }
}

pub fn watch_prs() {
    poll::while_focused(EVERY_MS, true, || spawn(check_prs()));
}

/// The PR tracker saw a PR merge or close: the live issues it belongs to, by link or branch, are finished or noted at once.
pub fn listen_pr_state() {
    host::on("pluginEvent", |_, event| on_event(event));
}

fn on_event(event: &Value) {
    if event["from"] == "pr-tracker" && event["name"] == "pr-state" {
        if let Some(event) = pr_state_event_of(&event["data"]) {
            spawn(apply_pr_state(event));
        }
    }
}

async fn apply_pr_state(event: PrStateEvent) {
    let projects = host::projects().await.unwrap_or_default();
    let projects: Vec<Value> = projects.as_array().cloned().unwrap_or_default();
    let source = projects.iter().find(|p| p["id"] == event.project_id.as_str());
    // A worktree's issues are kept in the project it was made from.
    let home = source.and_then(|s| s["parentId"].as_str().and_then(|parent| projects.iter().find(|p| p["id"] == parent))).or(source);
    let Some(home) = home else {
        return;
    };
    let (id, name) = (home["id"].as_str().unwrap_or_default(), home["name"].as_str().unwrap_or_default());
    let Ok(store) = read_store(id, name) else {
        return;
    };
    let found = FoundPr { url: event.url.clone(), state: event.state };
    for issue in store.issues.iter().filter(|i| is_pr_of(i, &event.url, &event.source_branch)) {
        apply(id, name, found_ops(issue, &found, &store.issues));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kv::memory, tool_defs::{context_of, run}};
    use serde_json::json;
    use wings_plugin::task::block_on;

    fn web() -> Value {
        json!({ "id": "proj1", "name": "web", "path": "/work/web" })
    }

    fn create(input: Value) {
        block_on(run("create".into(), input, context_of(web(), json!({ "id": "s", "name": "a" })))).unwrap();
    }

    fn stored(title: &str) -> Value {
        memory::keys().iter().filter_map(|k| memory::peek(k)).find(|v| v["title"] == title).unwrap()
    }

    fn event(from: &str, data: Value) {
        on_event(&json!({ "from": from, "name": "pr-state", "data": data }));
        block_on(async {});
    }

    #[test]
    fn a_pr_state_event_from_the_pr_tracker_finishes_the_live_issue_on_that_branch_links_the_pr_and_ignores_other_senders_and_other_branches() {
        memory::reset(&[]);
        host::testing::answer(|method, _| match method {
            "projects" => Ok(json!([{ "id": "proj1", "name": "web", "path": "/work/web" }, { "id": "wt1", "name": "web-x", "path": "/work/web-worktrees/x", "parentId": "proj1" }])),
            _ => Ok(Value::Null),
        });
        for (title, branch) in [("Merged elsewhere", "feature/merged"), ("Other branch", "feature/other")] {
            create(json!({ "title": title, "status": "in_progress" }));
            let key = if title == "Merged elsewhere" { "WEB-1" } else { "WEB-2" };
            block_on(run("update".into(), json!({ "key": key, "branch": branch }), context_of(web(), json!({ "id": "s", "name": "a" })))).unwrap();
        }
        let url = "https://github.com/acme/web/pull/9";
        let data = json!({ "url": url, "state": "merged", "sourceBranch": "feature/merged", "projectId": "wt1" });
        event("someone-else", data.clone());
        assert_eq!(stored("Merged elsewhere")["status"], "in_progress");
        event("pr-tracker", data);
        assert_eq!(stored("Merged elsewhere")["status"], "done");
        assert_eq!(stored("Merged elsewhere")["links"]["pr"], url);
        assert_eq!(stored("Other branch")["status"], "in_progress");
    }
}
