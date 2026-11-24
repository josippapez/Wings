//! An agent asks to add or change a timed schedule, or to start an agent on a ticket: the "Allow this?" modal shows it, and the tool call waits for
//! the user's answer. Asks live only while their call waits, so one nobody answers is gone with it.

use crate::{
    approval::{Approval, StartAsk, What},
    main_frame::changed,
    schedule::Draft,
};
use serde_json::Value;
pub use wings_plugin::approvals::Answer;
use wings_plugin::{approvals::Approvals, host};

const VIEW: &str = "approve";

thread_local! {
    static ASKS: Approvals<Approval> = Approvals::new(VIEW, changed);
}

/// What agents are waiting for the user to allow, oldest first.
pub fn waiting() -> Vec<Value> {
    ASKS.with(|asks| asks.waiting(|_, approval| approval.to_json()))
}

/// The user's answer to the ask `id`.
pub fn answer(id: &str, allow: bool) -> Result<(), String> {
    ASKS.with(|asks| asks.answer(id, allow, "That request is gone: the agent stopped waiting."))
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
    let (name, notice, project) = (project_name.to_owned(), title.to_owned(), project_id.to_owned());
    let waiting = ASKS.with(|asks| {
        asks.ask(
            |id| Approval { id: id.to_owned(), asker, project_name: project_name.to_owned(), what },
            move || drop(host::notify_with(&notice, &format!("In {name}. Allow or deny it in Wings."), None, Some(&project))),
        )
    });
    waiting.await
}
