//! The side panel's list of the pull requests and issues the conversation names (`refs.rs`), newest first, each with a
//! card of what it is while the pointer is on it. An issue is read from the Issues plugin, and listed only once it knows
//! the key; a pull request from `gh` or `az`, and listed either way. A pull request's row opens it in the PR tracker's tab,
//! or in the browser where no plugin opens it, and an issue's key opens it in Issues, as any issue key in the chat does.

use serde_json::{Value, json};
use wings_plugin::{
    host, task,
    ui::{self, Node, badge, list_button, muted, row, stack, text},
};

use crate::{
    chat::{self, Chat},
    refs::{self, Detail, Issue, Pr, Ref},
};

/// The most the panel lists.
const LISTED: usize = 10;
/// How many are looked up at once.
const AT_ONCE: usize = 4;

/// Reads what the conversation named since it was last read, and looks up what is new.
pub fn scan(c: &mut Chat) {
    let new = c.refs.scan(&c.session.items);
    if new.is_empty() {
        return;
    }
    for found in &new {
        c.refs.details.entry(found.key().to_owned()).or_insert(Detail::Loading);
    }
    let new: Vec<Ref> = new.into_iter().filter(|found| c.refs.details.get(found.key()) == Some(&Detail::Loading)).collect();
    task::spawn(async move {
        task::map_limit(new, AT_ONCE, |found| async move {
            let detail = look_up(&found).await;
            chat::with(|c| c.refs.details.insert(found.key().to_owned(), detail));
        })
        .await;
    });
}

/// How often the issues listed are read again while you look at the chat: an issue changes in Issues, or from another
/// session, and the Issues plugin tells only its own frames.
pub const ISSUES_EVERY_MS: u32 = 15_000;

/// Reads the issues listed again, keeping what each showed until its answer comes.
pub fn refresh_issues() {
    let keys: Vec<String> = chat::with(|c| c.refs.details.iter().filter(|(_, detail)| matches!(detail, Detail::Issue(_))).map(|(key, _)| key.clone()).collect());
    if keys.is_empty() {
        return;
    }
    task::spawn(async move {
        task::map_limit(keys, AT_ONCE, |key| async move {
            let detail = look_up(&Ref::Issue(key.clone())).await;
            // A lookup that fails now leaves the row as it was.
            if matches!(detail, Detail::Issue(_)) {
                chat::with(|c| c.refs.details.insert(key, detail));
            }
        })
        .await;
    });
}

async fn look_up(found: &Ref) -> Detail {
    let detail = match found {
        Ref::Issue(key) => {
            let answer = host::call("callTool", json!({ "name": "tasks__get", "arguments": { "key": key } })).await;
            answer.ok().filter(|answer| answer["isError"] != true).and_then(|answer| refs::issue_of(answer["text"].as_str()?)).map(Detail::Issue)
        }
        Ref::GitHub { url, .. } => printed(host::exec("gh", &["pr", "view", url, "--json", refs::GH_FIELDS]).await).and_then(|pr| refs::github_pr(&pr)).map(Detail::Pr),
        Ref::Azure { org, number, .. } => {
            let (number, organization) = (number.to_string(), format!("https://dev.azure.com/{org}"));
            let shown = host::exec("az", &["repos", "pr", "show", "--id", &number, "--organization", &organization, "--output", "json"]).await;
            printed(shown).and_then(|pr| refs::azure_pr(&pr)).map(Detail::Pr)
        }
    };
    detail.unwrap_or(Detail::Missing)
}

/// The JSON a command printed, when it succeeded.
fn printed(out: Result<Value, String>) -> Option<Value> {
    let out = out.ok().filter(|out| out["code"] == 0)?;
    serde_json::from_str(out["stdout"].as_str()?).ok()
}

/// The panel's section, with nothing named yet none.
pub fn section(c: &Chat) -> Option<Node> {
    let shown = c.refs.shown();
    if shown.is_empty() {
        return None;
    }
    let hidden = shown.len().saturating_sub(LISTED);
    let title = format!("Mentioned · {}", shown.len());
    let rows = shown.iter().take(LISTED).map(|(found, detail)| row_of(found, *detail));
    let earlier = (hidden > 0).then(|| ui::entry(ui::Entry { lead: None, title: muted(format!("{hidden} earlier")).prop("size", "small"), detail: None, trailing: None }));
    Some(ui::section(title, None, rows.chain(earlier)))
}

fn small(said: impl Into<String>) -> Node {
    text(said).prop("size", "small")
}

/// One row, in a `HoverCard` with what is known of it.
fn row_of(found: &Ref, detail: Option<&Detail>) -> Node {
    let (title, state, card) = match detail {
        Some(Detail::Issue(issue)) => (issue.title.clone(), Some(status(&issue.status)), Some(issue_card(found, issue))),
        Some(Detail::Pr(pr)) => (pr.title.clone(), Some((pr.state.to_owned(), pr_tone(pr.state))), Some(pr_card(found, pr))),
        Some(Detail::Loading) | None => ("…".to_owned(), None, None),
        Some(Detail::Missing) => ("Pull request".to_owned(), None, None),
    };
    // `grow`: a list button lays out its content as it is wide, so a row that doesn't grow ends right after its pill.
    let line = |label: Node| {
        row()
            .prop("grow", true)
            .prop("gap", "s")
            .prop("align", "center")
            .child(label)
            .child(stack().prop("grow", true).child(small(title.as_str()).prop("tone", "muted").prop("truncate", true)))
            .maybe(state.as_ref().map(|(said, tone)| badge(said.as_str(), *tone)))
    };
    let trigger = match found {
        // The key is a link Wings opens in Issues.
        Ref::Issue(key) => crate::view::inset(line(small(key.as_str()).prop("mono", true).prop("links", true))),
        Ref::GitHub { url, .. } | Ref::Azure { url, .. } => {
            let url = url.clone();
            list_button([line(small(found.label()).prop("mono", true))], move || open_pull_request(&url))
        }
    };
    let hover = Node::new("HoverCard").prop("width", 300).child(trigger);
    match card {
        Some(card) => hover.child(Node::new("Slot").prop("name", "card").child(card)),
        None => hover,
    }
    .key(found.key())
}

/// Opens a pull request in Wings' own tab, which the PR tracker's link for its address does; the browser where nothing
/// does (no PR tracker, or an older Wings).
fn open_pull_request(url: &str) {
    let opening = host::open_link(url);
    let url = url.to_owned();
    task::spawn(async move {
        if opening.await.is_err() {
            host::open_url(&url);
        }
    });
}

fn issue_card(found: &Ref, issue: &Issue) -> Node {
    let (said, tone) = status(&issue.status);
    let kind = match &issue.priority {
        Some(priority) => format!("{} · {} · {priority}", found.label(), issue.kind),
        None => format!("{} · {}", found.label(), issue.kind),
    };
    stack()
        .prop("gap", "xs")
        .child(row().prop("gap", "s").prop("align", "center").child(muted(kind).prop("size", "small").prop("grow", true)).child(badge(said, tone)))
        .child(text(issue.title.as_str()).prop("weight", "medium"))
        .maybe(issue.parent.as_ref().map(|parent| muted(format!("Part of {parent}")).prop("size", "small").prop("clamp", true)))
}

fn pr_card(found: &Ref, pr: &Pr) -> Node {
    let by = if pr.author.is_empty() { found.label() } else { format!("{} · {}", found.label(), pr.author) };
    let checks = pr.checks.map(|(passed, failed, running)| {
        let mut said = Vec::new();
        if failed > 0 {
            said.push(format!("{failed} failed"));
        }
        if running > 0 {
            said.push(format!("{running} running"));
        }
        said.push(format!("{passed} passed"));
        small(format!("Checks: {}", said.join(", "))).prop("tone", if failed > 0 { "danger" } else if running > 0 { "muted" } else { "success" })
    });
    let lines = pr.lines.map(|(added, removed)| {
        row().prop("gap", "s").child(small(format!("+{added}")).prop("mono", true).prop("tone", "success")).child(small(format!("−{removed}")).prop("mono", true).prop("tone", "danger"))
    });
    stack()
        .prop("gap", "xs")
        .child(row().prop("gap", "s").prop("align", "center").child(muted(by).prop("size", "small").prop("grow", true).prop("truncate", true)).child(badge(pr.state, pr_tone(pr.state))))
        .child(text(pr.title.as_str()).prop("weight", "medium"))
        .child(muted(pr.branch.as_str()).prop("size", "small").prop("mono", true).prop("truncate", true))
        .maybe(checks)
        .maybe(pr.review.as_ref().map(|review| small(review.as_str())))
        .maybe(lines)
}

/// An issue's status as Issues names it, and its tone.
fn status(status: &str) -> (String, Option<&'static str>) {
    let tone = match status {
        "done" => Some("success"),
        "in_review" => Some("info"),
        "in_progress" => Some("warn"),
        _ => None,
    };
    let mut said = status.replace('_', " ");
    if let Some(first) = said.get(..1) {
        said = first.to_uppercase() + &said[1..];
    }
    (said, tone)
}

fn pr_tone(state: &str) -> Option<&'static str> {
    match state {
        "Merged" => Some("success"),
        "Open" => Some("info"),
        "Closed" => Some("danger"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[test]
    fn a_listed_issue_shows_its_new_status_once_it_is_read_again_and_a_failed_read_keeps_the_old_one() {
        let status = Rc::new(RefCell::new("todo"));
        let now = status.clone();
        host::testing::answer(move |method, params| {
            if method != "callTool" || params["name"] != "tasks__get" {
                return Ok(Value::Null);
            }
            let said: &str = *now.borrow();
            Ok(match said {
                "gone" => json!({ "isError": true, "text": "No such issue" }),
                said => json!({ "text": format!("WIN-246 [{said}] (feature) A real diff\nPriority: No priority") }),
            })
        });
        let shown = || chat::with(|c| match c.refs.details.get("WIN-246") {
            Some(Detail::Issue(issue)) => issue.status.clone(),
            other => format!("{other:?}"),
        });
        chat::with(|c| c.refs.details.clear());
        let detail = task::block_on(async { look_up(&Ref::Issue("WIN-246".into())).await });
        chat::with(|c| c.refs.details.insert("WIN-246".into(), detail));
        assert_eq!(shown(), "todo");
        *status.borrow_mut() = "done";
        refresh_issues();
        task::block_on(async {});
        assert_eq!(shown(), "done");
        *status.borrow_mut() = "gone";
        refresh_issues();
        task::block_on(async {});
        assert_eq!(shown(), "done");
    }
}
