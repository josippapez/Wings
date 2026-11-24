//! Claude asks to share a port: the modal shows "Share localhost:3000 publicly?", and the tool call waits for the user's
//! answer. Claude Code asks before it calls an MCP tool unless the user allowed the tool for good, so this is the answer
//! that stays the user's whatever Claude Code was told. An ask lives only while its call waits, so one nobody answers is
//! gone with it.

use crate::main_frame::changed;
use serde_json::{Value, json};
pub use wings_plugin::approvals::Answer;
use wings_plugin::{approvals::Approvals, host};

pub const VIEW: &str = "share";

/// What an ask says: the port, and the agent session that asks when it has a name.
struct Share {
    port: u16,
    asker: Option<String>,
}

thread_local! {
    static ASKS: Approvals<Share> = Approvals::new(VIEW, changed);
}

/// What waits for the user, oldest first.
pub fn waiting() -> Vec<Value> {
    ASKS.with(|asks| asks.waiting(|id, ask| json!({ "id": id, "port": ask.port, "asker": ask.asker })))
}

/// The modal was opened from a badge: it stays until the user closes it.
pub fn opened_by_user() {
    ASKS.with(Approvals::opened_by_user);
}

/// The user's answer to the ask `id`.
pub fn answer(id: &str, allow: bool) -> Result<(), String> {
    ASKS.with(|asks| asks.answer(id, allow, "That request is gone: Claude stopped waiting."))
}

/// Shows the ask in the modal and gives the user's answer, or `Timeout`.
pub async fn ask(asker: Option<String>, port: u16) -> Answer {
    let waiting = ASKS.with(|asks| asks.ask(move |_| Share { port, asker }, move || drop(host::notify(&format!("Share localhost:{port} publicly?"), "Allow or deny it in Wings."))));
    waiting.await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::poll_fn, task::Poll};
    use wings_plugin::task::{block_on, join};

    fn shown(result: Result<Value, String>) {
        host::testing::answer(move |method, _| match method {
            "openView" => result.clone(),
            _ => Ok(Value::Null),
        });
    }

    /// The user: waits for an ask to show, then answers it.
    async fn user_answers(allow: bool) -> Value {
        poll_fn(|cx| {
            if waiting().is_empty() {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Poll::Ready(())
        })
        .await;
        let shown = waiting()[0].clone();
        answer(shown["id"].as_str().unwrap(), allow).unwrap();
        shown
    }

    #[test]
    fn an_ask_waits_in_the_list_until_the_user_answers_it() {
        shown(Ok(Value::Null));
        let (answered, shown) = block_on(join(ask(Some("fix the login".into()), 3000), user_answers(true)));
        assert_eq!(answered, Answer::Allow);
        assert_eq!((shown["port"].clone(), shown["asker"].clone()), (json!(3000), json!("fix the login")));
        assert!(waiting().is_empty());
        // The ask is gone: answering it again says so.
        assert!(answer(shown["id"].as_str().unwrap(), true).is_err());
    }

    #[test]
    fn a_denied_ask_says_deny() {
        shown(Ok(Value::Null));
        assert_eq!(block_on(join(ask(None, 5173), user_answers(false))).0, Answer::Deny);
    }

    #[test]
    fn an_ask_wings_cannot_show_fails_at_once() {
        shown(Err("Opening views isn't available".into()));
        assert_eq!(block_on(ask(None, 3000)), Answer::Unshown);
        assert!(waiting().is_empty());
    }
}
