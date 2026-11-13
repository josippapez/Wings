//! What the pages ask of main: the Pull requests block's lists, and what only main can do for a PR tab (the diff viewer
//! that takes replies, the agent on the branch, marking the PR read). A tab loads its PR itself and tells main its status,
//! so a pane on the PR has its badge kept fresh by the same load.

use super::{badges, diff, problems, projects, retry, state::{self, Loaded}, status::prefetch};
use crate::{forges, ipc, remote::remote_for, types::{Link, PrArgs, Provider, Status}};
use serde_json::Value;
use wings_plugin::{host, task::spawn};

/// The link a tab's request is about: a pane's, else one a list or a tab found before, else asked of its forge. In the
/// folder of the project the tab came from, where the diff and the snippets are read.
async fn link_of_args(args: &PrArgs) -> Result<Link, String> {
    let found = state::link_for(&args.url);
    let mut link = match found {
        Some(link) => link,
        None => forges::link_of(args).await.map_err(|error| error.to_string())?,
    };
    if matches!(link.provider, Provider::Azure | Provider::Bitbucket) {
        link.cwd = Some(args.cwd.clone());
    }
    state::remember(link.clone());
    Ok(link)
}

fn args_of(payload: &Value) -> Result<PrArgs, String> {
    crate::args::read_args(&payload["args"]).ok_or_else(|| "That isn't a pull request.".to_owned())
}

async fn pr_op(payload: Value) -> Result<Value, String> {
    let args = args_of(&payload)?;
    let focus = payload.get("focus").filter(|focus| !focus.is_null()).cloned();
    if payload["op"] == "seen" {
        projects::mark_seen(&args.url);
        return Ok(Value::Null);
    }
    let link = link_of_args(&args).await?;
    match payload["op"].as_str().unwrap_or_default() {
        "diff" => diff::open_diff_for(&link, Some(&args.title), None, focus).await?,
        "commit" => {
            let commit = &payload["commit"];
            diff::open_commit(&link, &args.cwd, commit["sha"].as_str().unwrap_or_default(), commit["message"].as_str().unwrap_or_default()).await?;
        }
        // The agent working on this PR's branch: Send to agent in the viewer offers the one in this folder first.
        "agent" => {
            let Some((pane, _)) = state::agent_on(&args.url) else {
                return Err("No agent is working on this branch. Start Claude in a terminal on it, then try again.".into());
            };
            diff::open_diff_for(&link, Some(&args.title), Some(&pane.cwd), focus).await?;
        }
        other => return Err(format!("Unknown request {other}")),
    }
    Ok(Value::Null)
}

/// A block's or a tab's button: what the pane badge's Sign in, Add token or Install does for this project's host.
async fn pulls_action(payload: Value) -> Result<Value, String> {
    let path = payload["path"].as_str().unwrap_or_default().to_owned();
    let remote = remote_for(&path).await;
    // A host no CLI knew: look at the CLIs' hosts again.
    if !forges::is_known(&remote) {
        retry::retry().await;
    } else if matches!(problems::problem_of(Some(&remote)), Some(problems::Problem::Missing(_))) {
        retry::install(Some(remote)).await?;
    } else {
        retry::sign_in(Some(remote)).await?;
    }
    Ok(Value::Null)
}

/// The status a tab loaded: the pane's badge on the same PR shows it, and the diff and failed checks are fetched ahead.
fn tab_status(payload: &Value) {
    let (Some(url), Ok(status)) = (payload["url"].as_str(), serde_json::from_value::<Status>(payload["status"].clone())) else { return };
    state::with(|s| s.status.insert(url.to_owned(), Loaded { entry: Ok(status), at: host::now() }));
    badges::render();
    prefetch(url);
}

pub fn serve() {
    ipc::answer(ipc::PULLS, |payload| async move { projects::pulls_for(payload["path"].as_str().unwrap_or_default()).await });
    ipc::answer(ipc::PR, pr_op);
    ipc::answer(ipc::PULLS_ACTION, pulls_action);
    ipc::hear(ipc::PR_STATUS, tab_status);
    // Settings saved or removed a token: ask again with it.
    ipc::hear(ipc::TOKENS_CHANGED, |_| {
        spawn(async {
            retry::retry().await;
            projects::refresh_projects(true).await;
        });
    });
}
