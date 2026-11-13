//! The main script: PR tracker shows the pull request for each terminal pane's current branch as a pane badge, with live
//! status, checks, the diff and review comments. Five forges, each in `forges/`:
//! - GitHub: in a Claude pane, the PR Claude Code recorded in the transcript (`pr-link`); else the PR for the branch (`gh`).
//! - GitLab: the merge request for the branch, its pipeline, approvals and discussions, all through `glab api`.
//! - Azure DevOps: the PR for the branch (`az repos`), diff from local git, comments from threads.
//! - Bitbucket Cloud and Codeberg: the REST API with a token saved in Settings.
//!
//! When checks fail, the badge says which step failed and why, and Claude can read the logs with `failing_checks`.

use crate::{
    forges,
    track::{
        diff::{open_diff_for, reply_in_viewer},
        failing::failing_checks,
        links::{self, Which, refresh_links, session_linked},
        news::start_news,
        projects::{refresh_projects, set_notify, set_sidebar},
        retry,
        state::{self, Pane, Loaded},
        status::refresh_status,
        tab,
    },
    prefs,
    remote,
};
use serde_json::{Value, json};
use std::cell::Cell;
use wings_plugin::{git, host, poll, task::spawn, tools};

const LINK_POLL_MS: u32 = 15_000;
const STATUS_POLL_MS: u32 = 60_000;
/// A PR whose status is older than this is loaded again by the pane poll, so a pane and a tab on it fetch once a minute.
const PR_STALE_MS: f64 = 55_000.0;

/// Each pane's PR, once a minute. A PR a tab shows tells main its status, so it is fetched once.
pub(crate) async fn refresh_panes() {
    // The lookup uses the branch read last and the lookup cache, so a PR opened on the forge shows with no git call.
    refresh_links(false, Which::All).await;
    let mut urls: Vec<String> = state::with(|s| s.links.values().map(|link| link.url.clone()).collect());
    urls.sort();
    urls.dedup();
    for url in urls {
        let age = state::status_of(&url).map_or(f64::INFINITY, |Loaded { at, .. }| host::now() - at);
        if age > PR_STALE_MS {
            spawn(async move { refresh_status(&url, false).await });
        }
    }
}

thread_local! {
    /// Which run of the background timer is the live one: a run that finds another number here has been stopped.
    static WATCH_RUN: Cell<u64> = const { Cell::new(0) };
}

/// Notifications about new pull requests, review requests and mentions: with any of them on, the lists are also checked
/// while Wings is in the background, as often as Settings says.
fn watch() {
    let notify = prefs::notify_pulls().get();
    set_notify(notify);
    let run = WATCH_RUN.get() + 1;
    WATCH_RUN.set(run);
    if notify == "off" && !prefs::mentions_on().get() {
        return;
    }
    let ms = prefs::watch_minutes().get() * 60_000;
    spawn(async move {
        loop {
            host::sleep(ms).await;
            if WATCH_RUN.get() != run {
                break;
            }
            if !host::focused() {
                refresh_projects(true).await;
            }
        }
    });
}

async fn on_action(action: Value) -> Result<(), String> {
    let pane_id = action["paneId"].as_str().unwrap_or_default().to_owned();
    let remote = state::pane(&pane_id).and_then(|pane| remote::known(&pane.cwd));
    match action["actionId"].as_str().unwrap_or_default() {
        "signin" => return retry::sign_in(remote).await,
        "retry" => return Ok(retry::retry().await),
        // Wings asks you, installs it in a tab and starts this plugin again once it's there.
        "install" => return retry::install(remote).await,
        _ => {}
    }
    let Some(link) = state::with(|s| s.links.get(&pane_id).cloned()) else { return Ok(()) };
    match action["actionId"].as_str().unwrap_or_default() {
        "open" => host::open_url(&link.url),
        "refresh" => refresh_status(&link.url, true).await,
        "diff" => return open_diff_for(&link, None, None, None).await,
        _ => {}
    }
    Ok(())
}

fn on_panes(list: &[Value]) {
    let panes: Vec<Pane> = list.iter().filter_map(Pane::from_json).collect();
    let open: Vec<String> = panes.iter().map(|pane| pane.id.clone()).collect();
    state::with(|s| {
        s.panes = panes;
        // A closed pane's PR stops being polled.
        s.links.retain(|id, _| open.contains(id));
        s.sync.retain(|id, _| open.contains(id));
    });
    links::forget_panes(&open);
    spawn(async { refresh_links(true, Which::All).await });
    // An agent starting or stopping on a PR's branch changes its row.
    spawn(async { refresh_projects(false).await });
}

#[unsafe(no_mangle)]
pub fn start() {
    retry::connect_clis();
    host::on_panes(on_panes);
    host::on_projects(|_| spawn(async { refresh_projects(false).await }));
    // Wings says when a project's repo changes, so a pane's branch and its commits to push and pull are read then and
    // when you come back. Only a pane in a repo no project has keeps the poll. The polls pause while Wings is in the
    // background, and run once when you come back.
    git::on_change(|ids, _| async move { refresh_links(true, Which::Projects(ids)).await });
    host::on_focus(|focused| {
        if focused {
            spawn(async { refresh_links(true, Which::InProject).await });
        }
    });
    poll::while_focused(LINK_POLL_MS, false, || spawn(async { refresh_links(true, Which::NoProject).await }));
    // The PR Claude Code records for a session arrives as it is written.
    host::on_transcript(|event| {
        let Some(link) = forges::github::pr_link_of(&event["entry"]) else { return };
        let (Some(session), Some(pane)) = (event["sessionId"].as_str(), event["paneId"].as_str().map(str::to_owned)) else { return };
        session_linked(session, link);
        spawn(async move { refresh_links(false, Which::Pane(pane)).await });
    });
    // The project lists are cached for a little under this, so a poll fetches fresh and pane changes in between don't. The
    // lists go first: a forge that lists a repo's open PRs in one call (GitHub) answers the panes' statuses from them.
    poll::while_focused(STATUS_POLL_MS, false, || {
        spawn(async {
            refresh_projects(true).await;
            refresh_panes().await;
        })
    });

    watch();
    prefs::watch_minutes().on_change(|_| watch());
    prefs::notify_pulls().on_change(|_| watch());
    prefs::mentions_on().on_change(|_| watch());
    set_sidebar(prefs::sidebar_pulls().get());
    prefs::sidebar_pulls().on_change(|sidebar| set_sidebar(sidebar));

    // An agent on a PR's branch hears about its failed checks, new comments and conflicts, when the setting is on.
    start_news();

    tools::register(
        "failing_checks",
        json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        |_input, caller| async move { failing_checks(caller["paneId"].as_str()).await.map(Value::from) },
    );

    // A reply typed in a diff viewer is posted to the PR it shows.
    host::on("reply", |token, reply| {
        let (diff_id, reply_to, body) = (reply["diffId"].as_str().unwrap_or_default().to_owned(), reply["replyTo"].as_i64().unwrap_or_default(), reply["body"].as_str().unwrap_or_default().to_owned());
        spawn(async move {
            let done = reply_in_viewer(&diff_id, reply_to, &body).await;
            drop(host::call("actionDone", json!({ "token": token, "error": done.err() })));
        });
    });
    host::on_action(on_action);
    tab::serve();
}
