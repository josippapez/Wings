//! The side panel's list of the pull requests and issues the conversation names (`refs.rs`), newest first, each with a
//! card of what it is while the pointer is on it. An issue is read from the Issues plugin, and listed only once it knows
//! the key; a pull request from `gh` or `az`, and listed either way. A pull request's row opens it in the PR tracker's tab,
//! or in the browser where no plugin opens it, and an issue's key opens it in Issues, as any issue key in the chat does.

use serde_json::{Value, json};
use wings_plugin::{
    host, task,
    ui::{Node, badge, list_button, muted, row, stack, text},
};

use crate::{
    chat::{self, Chat},
    refs::{self, Detail, Issue, Pr, Ref},
};

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
            let answer = host::call_tool("tasks__get", &json!({ "key": key })).await;
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
    let title = format!("Mentioned · {}", shown.len());
    // Newest first, as many as `LISTED_NODES` holds: each row is a hover card, and a chat that named a hundred issues sent
    // a page over the 5,000 nodes Wings draws.
    let mut rows = Vec::new();
    let mut spent = 0;
    for (found, detail) in &shown {
        let row = row_of(found, *detail);
        spent += row.count();
        if spent > LISTED_NODES && !rows.is_empty() {
            break;
        }
        rows.push(row);
    }
    let hidden = shown.len() - rows.len();
    let earlier = (hidden > 0).then(|| muted(format!("{hidden} earlier, not listed")).prop("size", "small").key("earlier"));
    Some(crate::view::fold_list(c, "refs", title, rows.into_iter().chain(earlier)))
}

/// The most nodes the Mentioned list sends.
const LISTED_NODES: usize = 1_000;

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
            let (name, number) = found.parts();
            let label = found.label();
            // Only the repo is cut short in a narrow row: the number at the end stays, to tell two pull requests apart.
            let both = row()
                .prop("gap", "none")
                .prop("align", "center")
                .prop("grow", true)
                .child(small(name).prop("mono", true).prop("truncate", true).prop("title", label))
                .child(small(number).prop("mono", true));
            list_button(
                [line(both)],
                move || open_pull_request(&url),
            )
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
pub(crate) fn open_pull_request(url: &str) {
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

    /// Before, the section listed the newest ten and ended in an "N earlier" line.
    #[test]
    fn the_section_lists_every_pull_request_named_in_a_column_that_scrolls() {
        chat::with(|c| {
            c.refs.list = (1..=25)
                .map(|number| Ref::GitHub { url: format!("https://github.com/acme/shop/pull/{number}"), repo: "acme/shop".into(), number })
                .collect();
            c.refs.details.clear();
        });
        let tree = chat::with(|c| wings_plugin::ui::tree_json(section(c).expect("the section")));
        let text = tree.to_string();
        assert_eq!(text.matches("HoverCard").count(), 25);
        assert!(!text.contains("earlier"));
        assert!(text.contains(&format!("\"maxHeight\":{}", crate::view::LIST_HEIGHT)), "{text}");
    }

    /// A chat that names hundreds of issues lists the newest that fit and counts the rest, so the page stays under the
    /// 5,000 nodes Wings draws.
    #[test]
    fn a_chat_that_names_hundreds_of_issues_lists_the_newest_and_counts_the_rest() {
        chat::with(|c| {
            c.refs.list = (1..=200).map(|n| Ref::Issue(format!("WIN-{n}"))).collect();
            c.refs.details = (1..=200)
                .map(|n| {
                    let issue = Issue { title: format!("Issue number {n} with a title"), status: "in_progress".into(), kind: "bug, high".into(), priority: Some("high".into()), parent: Some("WIN-1 The epic".into()) };
                    (format!("WIN-{n}"), Detail::Issue(issue))
                })
                .collect();
        });
        let section = chat::with(|c| section(c).expect("the section"));
        let nodes = section.count();
        let text = wings_plugin::ui::tree_json(section).to_string();
        chat::with(|c| (c.refs.list.clear(), c.refs.details.clear()));
        assert!(nodes <= LISTED_NODES + 50, "the list sent {nodes} nodes");
        assert!(text.contains("WIN-200") && text.contains("earlier, not listed"), "{text}");
        assert!(!text.contains("\"WIN-1\""), "the oldest isn't listed: {text}");
    }

    #[test]
    fn a_long_pull_request_label_cuts_the_repo_and_keeps_its_number_before_the_state_badge() {
        let reference = Ref::Azure {
            url: "https://dev.azure.com/acme/_git/Sciensus.Digital.Core.BE/pullrequest/7755".into(),
            org: "acme".into(),
            repo: "Sciensus.Digital.Core.BE".into(),
            number: 7755,
        };
        let label = reference.label();
        let (name, number) = reference.parts();
        assert_eq!((name.as_str(), number.as_str()), ("Sciensus.Digital.Core.BE", " !7755"));
        chat::with(|c| {
            c.refs.list = vec![reference.clone()];
            c.refs.details.insert(
                reference.key().into(),
                Detail::Pr(Pr {
                    title: "Use the new service".into(),
                    state: "Merged",
                    author: "alice".into(),
                    branch: "feature/service".into(),
                    checks: None,
                    lines: None,
                    review: None,
                }),
            );
        });

        let tree = chat::with(|c| wings_plugin::ui::tree_json(section(c).expect("the section")));
        fn text_node<'a>(node: &'a Value, text: &str) -> Option<&'a Value> {
            if node["type"] == "Text" && node["children"].as_array().is_some_and(|children| children.iter().any(|child| child["text"] == text)) {
                return Some(node);
            }
            node["children"].as_array()?.iter().find_map(|child| text_node(child, text))
        }
        let name_node = text_node(&tree, &name).expect("the repo's name");
        assert_eq!(name_node["props"]["truncate"], true, "{tree}");
        assert_eq!(name_node["props"]["title"], label, "{tree}");
        let number_node = text_node(&tree, &number).expect("the PR's number, apart from the name");
        assert!(number_node["props"]["truncate"].is_null(), "the number is never cut: {tree}");
        assert!(tree.to_string().contains("\"type\":\"Badge\""), "{tree}");
        assert!(tree.to_string().contains("Merged"), "{tree}");
    }

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
