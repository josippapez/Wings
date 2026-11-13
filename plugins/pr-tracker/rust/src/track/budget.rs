//! The process budget: how many `gh` processes a poll starts, which the TypeScript kept down by caching per repo and
//! branch, not per pane, and by asking GitHub for every repo's pull requests in one GraphQL call.

use super::{links::{Which, refresh_links}, projects::refresh_projects, state::{self, Pane}};
use crate::main_frame::refresh_panes;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use wings_plugin::{host, task::block_on};

const PR: &str = "https://github.com/acme/web/pull/482";

fn graphql_answer() -> Value {
    let check = json!({ "__typename": "CheckRun", "name": "Unit tests", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://github.com/acme/web/actions/runs/9/job/77", "startedAt": "2026-11-13T09:00:00Z", "completedAt": "2026-11-13T09:02:00Z", "checkSuite": null });
    let pr = json!({
        "number": 482, "title": "Add retry with backoff", "state": "OPEN", "isDraft": false, "reviewDecision": null, "mergeable": "MERGEABLE",
        "additions": 40, "deletions": 3, "changedFiles": 4, "headRefName": "feature/retry", "baseRefName": "main", "url": PR, "updatedAt": "2026-11-13T09:05:00Z",
        "author": { "login": "me" }, "comments": { "totalCount": 1 }, "reviews": { "totalCount": 0 },
        "statusCheckRollup": { "contexts": { "nodes": [check] } },
    });
    json!({ "data": { "r0": { "pullRequests": { "nodes": [pr] } }, "asked": { "nodes": [] } } })
}

/// Answers the calls a pane, a project and a PR make, and counts each program and its first words.
fn fake_host() -> Rc<RefCell<BTreeMap<String, u32>>> {
    let calls: Rc<RefCell<BTreeMap<String, u32>>> = Rc::default();
    let seen = calls.clone();
    host::testing::answer(move |method, params| {
        let args: Vec<&str> = params["args"].as_array().map(|args| args.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let program = params["program"].as_str().unwrap_or_default();
        let name = match method {
            "exec" => format!("{program} {}", args.iter().take(2).copied().collect::<Vec<_>>().join(" ")),
            "post" => format!("{program} {}", params["url"].as_str().unwrap_or_default()),
            "projects" => return Ok(json!([{ "id": "web", "path": "/work/web", "name": "web" }])),
            _ => return Ok(Value::Null),
        };
        *seen.borrow_mut().entry(name.clone()).or_default() += 1;
        let ok = |stdout: String| Ok(json!({ "code": 0, "stdout": stdout, "stderr": "" }));
        match name.as_str() {
            "git remote get-url" => ok("git@github.com:acme/web.git\n".into()),
            "git branch --show-current" => ok("feature/retry\n".into()),
            "git rev-parse --show-toplevel" => ok("/work/web\n".into()),
            "git rev-list --left-right" => ok("0\t1\n".into()),
            "gh pr list" => ok(json!([{ "url": PR, "number": 482 }]).to_string()),
            "gh api user" => ok("me\n".into()),
            "gh graphql" => ok(graphql_answer().to_string()),
            "gh pr diff" => ok("diff --git a/a b/a\n".into()),
            "gh pr view" => ok(json!({ "comments": [], "reviews": [] }).to_string()),
            "gh api repos/acme/web/pulls/482/comments" => ok("[[]]".into()),
            "gh api repos/acme/web/actions/jobs/77" => ok(json!({ "steps": [{ "name": "Unit tests", "conclusion": "failure" }] }).to_string()),
            "gh api repos/acme/web/actions/jobs/77/logs" => ok("2026-11-13T09:01:00.0000000Z ##[error]3 tests failed\n".into()),
            other => Ok(json!({ "code": 1, "stdout": "", "stderr": format!("no made-up answer for {other}") })),
        }
    });
    calls
}

fn pane(id: &str, cwd: &str) -> Pane {
    Pane { id: id.into(), cwd: cwd.into(), session: None }
}

fn count(calls: &Rc<RefCell<BTreeMap<String, u32>>>, prefix: &str) -> u32 {
    calls.borrow().iter().filter(|(name, _)| name.starts_with(prefix)).map(|(_, n)| n).sum()
}

#[test]
fn three_panes_on_one_branch_cost_one_lookup_one_list_and_one_diff_and_a_poll_costs_one_list() {
    let calls = fake_host();
    host::testing::set_now(1_000.0);
    state::with(|s| s.panes = vec![pane("1", "/work/web"), pane("2", "/work/web"), pane("3", "/work/web-feature")]);
    // A pane change: every pane's branch is read once, and the PR the three share is looked up once.
    block_on(async {
        refresh_links(true, Which::All).await;
        refresh_projects(false).await;
    });
    let first = calls.borrow().clone();
    assert_eq!(count(&calls, "gh pr list"), 1, "{first:?}");
    assert_eq!(count(&calls, "gh graphql"), 1, "{first:?}");
    // The failed check's log and the PR's diff are fetched ahead once, whatever the number of panes.
    assert_eq!(count(&calls, "gh pr diff"), 1, "{first:?}");
    assert_eq!(count(&calls, "gh api repos/acme/web/actions/jobs/77"), 2, "{first:?}");
    assert_eq!(count(&calls, "git branch"), 3, "{first:?}");
    let before = count(&calls, "gh");
    // The next poll, a minute later: the lists go first, then each pane's PR, with no git.
    host::testing::set_now(61_000.0);
    block_on(async {
        refresh_projects(true).await;
        refresh_panes().await;
    });
    let git_after = count(&calls, "git branch");
    assert_eq!(git_after, 3, "a poll reads no branch");
    // One `gh pr list` for the branch (the lookup is a minute old) and one GraphQL call for the list and the three statuses.
    assert_eq!(count(&calls, "gh pr list"), 2, "{:?}", calls.borrow());
    assert_eq!(count(&calls, "gh graphql"), 2, "{:?}", calls.borrow());
    // The PR hasn't changed, so its diff and failed checks are not fetched again.
    assert_eq!(count(&calls, "gh pr diff"), 1, "{:?}", calls.borrow());
    eprintln!("first look: {first:?}\nthe poll after: {} gh calls more", count(&calls, "gh") - before);
}
