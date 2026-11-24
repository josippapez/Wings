//! Claude asks to share a port: the modal shows "Share localhost:3000 publicly?", and the tool call waits for the user's
//! answer. Claude Code asks before it calls an MCP tool unless the user allowed the tool for good, so this is the answer
//! that stays the user's whatever Claude Code was told. An ask lives only while its call waits, so one nobody answers is
//! gone with it.

use crate::main_frame::changed;
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
pub const VIEW: &str = "share";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Answer {
    Allow,
    Deny,
    Timeout,
    /// Wings couldn't show the modal, so nobody could answer.
    Unshown,
}

#[derive(Default)]
struct Slot {
    answer: Option<Answer>,
    waker: Option<Waker>,
}

struct Ask {
    id: String,
    port: u16,
    /// The agent session that asks, when it has a name.
    asker: Option<String>,
    slot: Rc<RefCell<Slot>>,
}

thread_local! {
    static PENDING: RefCell<Vec<Ask>> = const { RefCell::new(Vec::new()) };
    static NEXT: Cell<u32> = const { Cell::new(0) };
    /// Whether the modal was opened for an ask, so it closes with the last one. One opened from a badge stays for the user.
    static OPENED_FOR_ASK: Cell<bool> = const { Cell::new(false) };
}

/// What waits for the user, oldest first.
pub fn waiting() -> Vec<Value> {
    PENDING.with(|pending| pending.borrow().iter().map(|ask| json!({ "id": ask.id, "port": ask.port, "asker": ask.asker })).collect())
}

/// The modal was opened from a badge: it stays until the user closes it.
pub fn opened_by_user() {
    OPENED_FOR_ASK.with(|opened| opened.set(false));
}

fn settle(id: &str, answer: Answer) -> bool {
    let Some(ask) = PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        pending.iter().position(|ask| ask.id == id).map(|at| pending.remove(at))
    }) else {
        return false;
    };
    changed();
    if PENDING.with(|pending| pending.borrow().is_empty()) && OPENED_FOR_ASK.with(|opened| opened.replace(false)) {
        drop(host::close_view(VIEW));
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
    if settle(id, if allow { Answer::Allow } else { Answer::Deny }) { Ok(()) } else { Err("That request is gone: Claude stopped waiting.".into()) }
}

/// Shows the ask in the modal and gives the user's answer, or `Timeout`.
pub async fn ask(asker: Option<String>, port: u16) -> Answer {
    let id = NEXT.with(|next| {
        next.set(next.get() + 1);
        next.get().to_string()
    });
    let slot = Rc::new(RefCell::new(Slot::default()));
    PENDING.with(|pending| pending.borrow_mut().push(Ask { id: id.clone(), port, asker, slot: slot.clone() }));
    let late = id.clone();
    spawn(async move {
        host::sleep(WAIT_MS).await;
        settle(&late, Answer::Timeout);
    });
    changed();
    // An ask Wings can't show fails at once: waiting out the 100 s told Claude the user hadn't answered, when the user
    // never saw it.
    let shown = id.clone();
    spawn(async move {
        OPENED_FOR_ASK.with(|opened| opened.set(true));
        if host::open_view_with(VIEW, "modal", &Value::Null).await.is_err() {
            settle(&shown, Answer::Unshown);
        }
    });
    if !host::focused() {
        drop(host::notify(&format!("Share localhost:{port} publicly?"), "Allow or deny it in Wings."));
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

#[cfg(test)]
mod tests {
    use super::*;
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
