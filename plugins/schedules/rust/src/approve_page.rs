//! "Allow this?": an agent asks to add or change a timed schedule, which then starts agents by itself, or to start an agent
//! on a ticket. Main opens this modal for each ask and closes it once none is left.

use crate::{
    STATE, WANT,
    approval::{Approval, StartAsk, What},
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
    let mut items = vec![("Project".to_owned(), text(a.project_name.as_str()).prop("size", "small"))];
    match &a.what {
        What::Schedule { current, proposed: p } => {
            let c = current.as_ref();
            items.push(("Runs".to_owned(), changed(describe_when(&p.when), c.map(|c| describe_when(&c.when)))));
            items.push(("Agent".to_owned(), changed(p.agent.label().to_owned(), c.map(|c| c.agent.label().to_owned()))));
            if !p.name.is_empty() {
                items.push(("Name".to_owned(), changed(p.name.clone(), c.map(|c| c.name.clone()).filter(|name| !name.is_empty()))));
            }
            items.push(("Prompt".to_owned(), changed(p.prompt.clone(), c.map(|c| c.prompt.clone()))));
        }
        What::Start(s) => {
            items.push(("Ticket".to_owned(), text(ticket_line(s)).prop("size", "small")));
            if let Some(project) = &s.starts_in {
                items.push(("Starts in".to_owned(), text(project.as_str()).prop("size", "small")));
            }
            items.push(("Agent".to_owned(), text(s.agent.label()).prop("size", "small")));
            items.push(("Prompt".to_owned(), text(s.prompt.as_str()).prop("size", "small")));
        }
    }
    properties(items)
}

fn ticket_line(s: &StartAsk) -> String {
    format!("#{} {}", s.ticket_id, s.title)
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
        let (what, note) = match &first.what {
            What::Schedule { current, .. } => (
                format!("{} a schedule in {}", if current.is_some() { "change" } else { "add" }, first.project_name),
                "It starts the agent with this prompt by itself, without asking again, while Wings runs.",
            ),
            What::Start(s) => (
                format!("start an agent on {} in {}", ticket_line(s), first.project_name),
                "It makes a worktree and opens the agent with this prompt. To let agents start this entry's tickets without asking, turn that on in the entry on the Schedules page.",
            ),
        };
        let (deny, allow) = (first.id.clone(), first.id.clone());
        let working = busy();
        let body = stack()
            .prop("gap", "m")
            .child(text(format!("{who} asks to {what}.{}", if more > 0 { format!(" {more} more waiting.") } else { String::new() })))
            .child(card().child(details(first)))
            .child(muted(note).prop("size", "small"))
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
