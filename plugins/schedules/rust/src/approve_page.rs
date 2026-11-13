//! "Allow this schedule?": an agent asks to add or change a timed schedule, which then starts agents by itself. Main opens
//! this modal for each ask and closes it once none is left.

use crate::{
    STATE, WANT,
    approval::Approval,
    page_ask::{ask, busy, error, listen},
    schedule::describe_when,
    view::{button, properties},
};
use serde_json::{Value, json};
use std::cell::RefCell;
use wings_plugin::{
    host, messages,
    task::spawn,
    ui::{Node, card, empty, loading, muted, notice, row, slot, stack, text},
};

/// Allow waits this long after an ask shows, so a click meant for something else can't land on it.
const ARM_MS: u32 = 1000;

#[derive(Default)]
struct Page {
    /// What agents are waiting for, oldest first, once main has said.
    approvals: Option<Vec<Approval>>,
    /// The ask that is first, which Allow waits on.
    shown: Option<String>,
    armed: bool,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page::default());
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

/// The value, and under it what it was, when a change makes it different.
fn changed(now: String, was: Option<String>) -> Node {
    match was.filter(|was| *was != now) {
        Some(was) => stack().prop("gap", "xs").child(text(now).prop("size", "small")).child(muted(format!("Was: {was}")).prop("size", "small")),
        None => text(now).prop("size", "small"),
    }
}

fn details(a: &Approval) -> Node {
    let (p, c) = (&a.proposed, a.current.as_ref());
    let mut items = vec![
        ("Project".to_owned(), text(a.project_name.as_str()).prop("size", "small")),
        ("Runs".to_owned(), changed(describe_when(&p.when), c.map(|c| describe_when(&c.when)))),
        ("Agent".to_owned(), changed(p.agent.label().to_owned(), c.map(|c| c.agent.label().to_owned()))),
    ];
    if !p.name.is_empty() {
        items.push(("Name".to_owned(), changed(p.name.clone(), c.map(|c| c.name.clone()).filter(|name| !name.is_empty()))));
    }
    items.push(("Prompt".to_owned(), changed(p.prompt.clone(), c.map(|c| c.prompt.clone()))));
    properties(items)
}

fn view() -> Vec<Node> {
    PAGE.with(|state| {
        let p = &*state.borrow();
        let Some(approvals) = &p.approvals else {
            return vec![loading()];
        };
        let Some(first) = approvals.first() else {
            return vec![empty("Nothing is waiting", "The agent stopped waiting, or the request was answered.")];
        };
        let more = approvals.len() - 1;
        let who = first.asker.as_ref().map_or("An agent".to_owned(), |asker| format!("\"{asker}\""));
        let what = if first.current.is_some() { "change a schedule" } else { "add a schedule" };
        let (deny, allow) = (first.id.clone(), first.id.clone());
        let working = busy();
        let body = stack()
            .prop("gap", "m")
            .child(text(format!("{who} asks to {what} in {}.{}", first.project_name, if more > 0 { format!(" {more} more waiting.") } else { String::new() })))
            .child(card().child(details(first)))
            .child(muted("It starts the agent with this prompt by itself, without asking again, while Wings runs.").prop("size", "small"))
            .maybe(error().map(|error| notice("danger", error)))
            .child(
                row()
                    .prop("gap", "s")
                    .prop("justify", "end")
                    .child(button("Deny", "ghost", None, None, working, move || ask("answerApproval", json!({ "approval": deny, "allow": false }), || {})))
                    .child(button("Allow", "default", None, None, working || !p.armed, move || ask("answerApproval", json!({ "approval": allow, "allow": true }), || {}))),
            );
        vec![Node::new("Page").child(slot("body", body))]
    })
}

/// Takes what main told: the asks that wait. Allow for the first one waits a second after it shows.
fn hear(state: &Value) {
    let approvals: Vec<Approval> = state["approvals"].as_array().map(|list| list.iter().filter_map(Approval::from_json).collect()).unwrap_or_default();
    let first = approvals.first().map(|a| a.id.clone());
    let moved = with(|p| {
        let moved = p.shown != first;
        (p.approvals, p.shown) = (Some(approvals), first.clone());
        if moved {
            p.armed = false;
        }
        moved
    });
    if let (true, Some(first)) = (moved, first) {
        spawn(async move {
            host::sleep(ARM_MS).await;
            with(|p| p.armed = p.shown.as_ref() == Some(&first));
        });
    }
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
    listen("a");
    messages::on(STATE, hear);
    messages::tell(WANT, &Value::Null);
}
