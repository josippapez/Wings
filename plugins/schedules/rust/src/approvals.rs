//! An agent asks to add or change a timed schedule, or to start an agent on a ticket: the "Allow this?" modal shows it, and the tool call waits for
//! the user's answer. Asks live only while their call waits, so one nobody answers is gone with it.

use crate::{
    approval::{Approval, StartAsk, What},
    main_frame::changed,
    schedule::Draft,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    future::poll_fn,
    rc::Rc,
    task::{Poll, Waker},
};
use wings_plugin::{host, task::spawn};

/// Under the 120 s Wings gives a plugin's tool call, so the call can still say nobody answered.
const WAIT_MS: u32 = 100_000;
const VIEW: &str = "approve";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Answer {
    Allow,
    Deny,
    Timeout,
}

#[derive(Default)]
struct Slot {
    answer: Option<Answer>,
    waker: Option<Waker>,
}

struct Ask {
    approval: Approval,
    slot: Rc<RefCell<Slot>>,
}

thread_local! {
    static PENDING: RefCell<Vec<Ask>> = const { RefCell::new(Vec::new()) };
    static NEXT: Cell<u32> = const { Cell::new(0) };
}

/// What agents are waiting for the user to allow, oldest first.
pub fn waiting() -> Vec<Value> {
    PENDING.with(|pending| pending.borrow().iter().map(|ask| ask.approval.to_json()).collect())
}

fn settle(id: &str, answer: Answer) -> bool {
    let Some(ask) = PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        pending.iter().position(|ask| ask.approval.id == id).map(|at| pending.remove(at))
    }) else {
        return false;
    };
    changed();
    if PENDING.with(|pending| pending.borrow().is_empty()) {
        drop(host::call("closeView", json!({ "id": VIEW })));
    }
    let mut slot = ask.slot.borrow_mut();
    slot.answer = Some(answer);
    if let Some(waker) = slot.waker.take() {
        waker.wake();
    }
    true
}

/// The user's answer to the ask `id`.
pub fn answer(id: &str, allow: bool) -> Result<(), String> {
    if settle(id, if allow { Answer::Allow } else { Answer::Deny }) { Ok(()) } else { Err("That request is gone: the agent stopped waiting.".into()) }
}

/// Shows the schedule ask in the modal, and gives the user's answer, or `Timeout`.
pub async fn ask_approval(asker: Option<String>, project_name: &str, current: Option<Draft>, proposed: Draft, project_id: &str) -> Answer {
    let title = if current.is_some() { "An agent asks to change a schedule" } else { "An agent asks to add a schedule" };
    wait_for(asker, project_name, What::Schedule { current, proposed }, project_id, title).await
}

/// Shows the start ask in the modal, and gives the user's answer, or `Timeout`.
pub async fn ask_start(asker: Option<String>, project_name: &str, start: StartAsk, project_id: &str) -> Answer {
    wait_for(asker, project_name, What::Start(start), project_id, "An agent asks to start a ticket").await
}

async fn wait_for(asker: Option<String>, project_name: &str, what: What, project_id: &str, title: &str) -> Answer {
    let id = NEXT.with(|next| {
        next.set(next.get() + 1);
        next.get().to_string()
    });
    let slot = Rc::new(RefCell::new(Slot::default()));
    PENDING.with(|pending| pending.borrow_mut().push(Ask { approval: Approval { id: id.clone(), asker, project_name: project_name.to_owned(), what }, slot: slot.clone() }));
    let late = id.clone();
    spawn(async move {
        host::sleep(WAIT_MS).await;
        settle(&late, Answer::Timeout);
    });
    changed();
    drop(host::open_view_with(VIEW, "modal", &Value::Null));
    if !host::focused() {
        drop(host::call("notify", json!({ "title": title, "body": format!("In {project_name}. Allow or deny it in Wings."), "projectId": project_id })));
    }
    poll_fn(|cx| {
        let mut slot = slot.borrow_mut();
        match slot.answer.take() {
            Some(answer) => Poll::Ready(answer),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    })
    .await
}
