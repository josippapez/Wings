//! Asks before deleting an issue. With child issues (at any depth) it offers to delete only this one, which leaves them without
//! a parent, or all of them. Keep it closes the card, and a failed delete leaves it open with the reason.

use crate::{
    domain::{Issue, Op},
    feed,
    parts::line,
    relations::descendants_of,
};
use std::cell::RefCell;
use wings_plugin::{task::spawn, ui::{Node, confirm}};

struct Deleting {
    key: String,
    busy: bool,
    error: Option<String>,
}

thread_local! {
    static DELETING: RefCell<Option<Deleting>> = const { RefCell::new(None) };
}

fn with<T>(change: impl FnOnce(&mut Option<Deleting>) -> T) -> T {
    DELETING.with(|d| change(&mut d.borrow_mut()))
}

/// Starts asking to delete the issue with this key.
pub fn ask_to_delete(key: &str) {
    with(|d| *d = Some(Deleting { key: key.to_owned(), busy: false, error: None }));
}

/// The key of the issue being asked about.
pub fn asking() -> Option<String> {
    with(|d| d.as_ref().map(|d| d.key.clone()))
}

fn cancel() {
    with(|d| *d = None);
}

fn delete(key: String, children: bool) {
    with(|d| {
        if let Some(d) = d {
            d.busy = true;
            d.error = None;
        }
    });
    spawn(async move {
        let done = feed::apply(Op::Remove { key, children }).await;
        with(|d| match done {
            Ok(()) => *d = None,
            Err(error) => *d = d.take().map(|d| Deleting { busy: false, error: Some(error), ..d }),
        });
    });
}

/// The card for the issue being asked about, when it is `issue`. Focus starts on the delete that keeps the most, so Enter
/// never takes more than asked.
pub fn node(issue: &Issue, issues: &[Issue]) -> Option<Node> {
    let (busy, error) = with(|d| d.as_ref().filter(|d| d.key == issue.key).map(|d| (d.busy, d.error.clone())))?;
    let below = descendants_of(issues, &issue.key).len();
    let children = if below == 1 { "1 child issue".to_owned() } else { format!("{below} child issues") };
    let action = |label: String, variant: &str, on_click: Box<dyn Fn()>| Node::new("Button").prop("variant", variant).prop("disabled", busy).on("onClick", move |_| on_click()).text(label);
    let key = issue.key.clone();
    let mut actions = if below > 0 {
        let (only, all) = (key.clone(), key);
        vec![action(format!("Delete only {}", issue.key), "outline", Box::new(move || delete(only.clone(), false))), action(format!("Delete with {children}"), "destructive", Box::new(move || delete(all.clone(), true)))]
    } else {
        vec![action(format!("Delete {}", issue.key), "destructive", Box::new(move || delete(key.clone(), false)))]
    };
    actions.push(action("Keep it".into(), "ghost", Box::new(cancel)));
    let body = match error {
        Some(error) => Node::new("Notice").prop("tone", "danger").text(error),
        None => line(format!("{}This removes {} and its log. Keys are never used again.", if below > 0 { format!("It has {children}. ") } else { String::new() }, if below > 0 { "what you delete" } else { "the issue" }), None, Some("small")),
    };
    Some(confirm(format!("Delete {} {}?", issue.key, issue.title), [body], actions))
}
