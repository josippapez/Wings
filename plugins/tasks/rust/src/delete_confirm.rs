//! Asks before deleting an issue, in a dialog over the page. With child issues (at any depth) it offers to delete only this
//! one, which leaves them without a parent, or all of them. Keep it, Escape or a press beside it closes the dialog, and a
//! failed delete leaves it open with the reason.

use crate::{
    domain::{Issue, Op},
    feed,
    parts::{line, row, stack},
    relations::descendants_of,
};
use std::cell::RefCell;
use wings_plugin::{task::spawn, ui::{Node, dialog}};

/// The dialog's width in points.
const WIDTH: u32 = 460;

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

/// The dialog for the issue being asked about, when it is `issue`. The delete that keeps the most comes first.
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
    actions.insert(0, action("Keep it".into(), "ghost", Box::new(cancel)));
    let said = line(format!("{}This removes {} and its log. Keys are never used again.", if below > 0 { format!("It has {children}. ") } else { String::new() }, if below > 0 { "what you delete" } else { "the issue" }), None, None);
    let body = stack("m")
        .child(line(issue.title.clone(), None, None).prop("weight", "medium"))
        .child(said)
        .maybe(error.map(|error| Node::new("Notice").prop("tone", "danger").text(error)))
        .child(row("s").prop("justify", "end").prop("wrap", true).children(actions));
    Some(dialog(&format!("Delete {}?", issue.key), WIDTH, cancel).child(body))
}
