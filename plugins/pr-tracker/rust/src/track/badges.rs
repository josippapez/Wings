//! The badge each pane shows for its PR, or for a host that needs signing in.

use super::{problems::{self, Problem}, state};
use crate::{checklist::iso_to_ms, forges, providers::{self, link_ref}, remote, types::{Link, Status}};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashSet};
use wings_plugin::{cli::{ProblemText, last_line}, format::ago, host, task::spawn};

thread_local! {
    /// PR urls whose status is loading.
    static IN_FLIGHT: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

pub fn set_in_flight(url: &str, on: bool) {
    IN_FLIGHT.with(|set| if on { set.borrow_mut().insert(url.to_owned()) } else { set.borrow_mut().remove(url) });
}

pub fn in_flight(url: &str) -> bool {
    IN_FLIGHT.with(|set| set.borrow().contains(url))
}

fn sync_text(ahead: i64, behind: i64) -> String {
    let mut parts = Vec::new();
    if ahead > 0 {
        parts.push(format!("{ahead} to push"));
    }
    if behind > 0 {
        parts.push(format!("{behind} to pull"));
    }
    parts.join(", ")
}

fn badge_for(link: &Link, loaded: Option<&state::Loaded>, local: Option<(i64, i64)>) -> Value {
    let actions = json!([
        { "id": "diff", "label": "View changes", "primary": true },
        { "id": "open", "label": format!("Open in {}", providers::info(link.provider).name) },
        { "id": "refresh", "label": "Refresh" },
    ]);
    let r = link_ref(link);
    let subtitle = format!("{} {r}", link.repo);
    let st: &Status = match loaded.map(|loaded| &loaded.entry) {
        None => return json!({ "label": r, "tone": "neutral", "icon": "pr-open", "title": "Loading pull request…", "subtitle": subtitle, "actions": actions }),
        Some(Err(error)) => {
            return json!({ "label": r, "tone": "neutral", "icon": "pr-open", "title": format!("Pull request {r}"), "subtitle": subtitle, "rows": [{ "label": "Status", "value": last_line(error), "tone": "danger" }], "actions": actions });
        }
        Some(Ok(st)) => st,
    };
    let (passed, failed, running) = (st.checks.passed, st.checks.failed, st.checks.running);
    let total = passed + failed + running;
    let (ahead, behind) = local.unwrap_or((0, 0));
    let pending = sync_text(ahead, behind);
    let details = state::with(|s| s.failures.get(&link.url).filter(|f| failed > 0 && f.key == state::fail_key(st)).and_then(|f| f.list.clone())).unwrap_or_default();
    let mut rows = vec![
        json!({ "label": "Status", "value": st.label, "tone": st.tone }),
        json!({ "label": "Review", "value": st.review }),
        json!({
            "label": "Checks",
            "value": if total > 0 { format!("{passed} passed, {failed} failed, {running} running") } else { "None".to_owned() },
            "tone": if failed > 0 { json!("danger") } else if running > 0 { json!("warning") } else if total > 0 { json!("success") } else { Value::Null },
        }),
    ];
    rows.extend(details.iter().take(3).map(|f| json!({ "label": "Failed", "value": format!("{}: {}", f.step.as_deref().or(f.check.as_deref()).unwrap_or_default(), f.summary), "tone": "danger" })));
    if let Some(merge) = &st.merge {
        rows.push(json!({ "label": "Merge", "value": merge }));
    }
    if let Some(changes) = &st.changes {
        rows.push(json!({ "label": "Changes", "value": changes }));
    }
    rows.push(json!({ "label": "Branch", "value": st.branch }));
    if !pending.is_empty() {
        rows.push(json!({ "label": "Local", "value": pending.replace("push", "push to the PR"), "tone": "info" }));
    }
    rows.push(json!({ "label": "Updated", "value": iso_to_ms(&st.updated).map_or_else(|| st.updated.clone(), |ms| ago(ms, host::now())) }));
    let counts: Vec<Value> = [("push", ahead), ("pull", behind)].into_iter().filter(|(_, value)| *value > 0).map(|(icon, value)| json!({ "icon": icon, "value": value })).collect();
    json!({ "label": format!("{r} {}", st.label), "counts": counts, "tone": st.tone, "icon": st.icon, "title": st.title, "subtitle": subtitle, "rows": rows, "actions": actions })
}

/// What the pane shows while its host has a problem: a button to install the CLI, sign in or add a token, or the error
/// when nothing can be done but try again.
fn problem_badge(problem: &Problem, remote: &wings_plugin::remote::Remote) -> Value {
    let Some(provider) = forges::provider_of(remote) else { return Value::Null };
    let info = providers::info(provider);
    let retry = json!({ "id": "retry", "label": "Try again" });
    match problem {
        Problem::Unavailable(message) => json!({
            "label": "PRs unavailable", "tone": "warning", "icon": "pr-open", "title": info.name,
            "rows": [{ "label": "Error", "value": message, "tone": "warning" }],
            "actions": [{ "id": "retry", "label": "Try again", "primary": true }],
        }),
        Problem::Token(message) => json!({
            "label": format!("Add {} token", info.name), "tone": info.tone, "icon": "pr-open", "title": info.name, "subtitle": message,
            "actions": [{ "id": "signin", "label": "Add token", "primary": true }, retry],
        }),
        Problem::Missing(_) | Problem::SignedOut(_) => {
            let Some(cli) = forges::cli_of(provider) else { return Value::Null };
            let key = forges::problem_key(remote);
            let words = forges::words(remote);
            let text = ProblemText { title: info.name, signed_out: &words.signed_out, missing: &words.missing };
            let Some(mut badge) = cli.problem_badge(&key, &text) else { return Value::Null };
            badge["tone"] = info.tone.into();
            badge["icon"] = "pr-open".into();
            // While `gh` waits for the code to be entered in the browser, the badge says which.
            if let Some(code) = problems::code_for(&key).filter(|_| badge["loading"] == true) {
                badge["label"] = format!("Code {code}").into();
                badge["subtitle"] = format!("Enter this code on the {} page in your browser. It's on your clipboard too.", info.name).into();
            }
            badge
        }
    }
}

/// Sets every pane's badge. One a pane already shows isn't sent again.
pub fn render() {
    let panes = state::with(|s| s.panes.clone());
    for pane in panes {
        let remote = remote::known(&pane.cwd);
        let problem = problems::problem_of(remote.as_ref());
        let (link, loaded, local) = state::with(|s| {
            let link = s.links.get(&pane.id).cloned();
            let loaded = link.as_ref().and_then(|link| s.status.get(&link.url).cloned());
            (link, loaded, s.sync.get(&pane.id).copied())
        });
        // A problem with the host replaces the PR's badge, except one that only means nothing could be listed.
        let badge = match (&problem, &link) {
            (Some(problem), link) if !matches!(problem, Problem::Unavailable(_)) || link.is_none() => problem_badge(problem, remote.as_ref().expect("a problem has a remote")),
            (_, Some(link)) => {
                let loading = in_flight(&link.url) && (loaded.is_none() || state::with(|s| s.manual.contains(&link.url)));
                let mut badge = badge_for(link, loaded.as_ref(), local);
                badge["loading"] = loading.into();
                badge
            }
            _ => Value::Null,
        };
        let id = pane.id;
        spawn(async move {
            drop(host::set_badge(&id, &badge).await);
        });
    }
}
