//! A tool call that waits for the user's answer in a modal: an agent asks to do something, the modal shows the asks that
//! wait with Allow and Deny, and the call gets the answer, or `Timeout` when nobody gave one. An ask lives only while its
//! call waits, so one nobody answers is gone with it.

use crate::{host, task};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    rc::Rc,
};

/// Under the 120 s Wings gives a plugin's tool call, so the call can still say nobody answered.
pub const WAIT_MS: u32 = 100_000;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Answer {
    Allow,
    Deny,
    Timeout,
    /// Wings couldn't show the modal, so nobody could answer.
    Unshown,
}

struct Entry<T> {
    id: String,
    what: T,
    answer: task::Sender<Answer>,
}

struct Inner<T> {
    view: &'static str,
    changed: fn(),
    pending: RefCell<Vec<Entry<T>>>,
    next: Cell<u32>,
    /// Whether the modal was opened for an ask, so it closes with the last one. One the user opened stays for them.
    opened_for_ask: Cell<bool>,
}

/// The asks that wait for the user, of what `T` says about each. Keep one in a `thread_local`.
pub struct Approvals<T>(Rc<Inner<T>>);

impl<T: 'static> Approvals<T> {
    /// `view` is the modal, a view the plugin's manifest has as a modal; `changed` runs each time the list changes, to tell the modal.
    pub fn new(view: &'static str, changed: fn()) -> Self {
        Self(Rc::new(Inner { view, changed, pending: RefCell::new(Vec::new()), next: Cell::new(0), opened_for_ask: Cell::new(false) }))
    }

    /// What waits, oldest first, as `show` makes of each one's id and what it asks.
    pub fn waiting<R>(&self, show: impl Fn(&str, &T) -> R) -> Vec<R> {
        self.0.pending.borrow().iter().map(|entry| show(&entry.id, &entry.what)).collect()
    }

    /// The modal was opened by the user, not for an ask: it stays until they close it.
    pub fn opened_by_user(&self) {
        self.0.opened_for_ask.set(false);
    }

    fn settle(&self, id: &str, answer: Answer) -> bool {
        let inner = &self.0;
        let Some(entry) = ({
            let mut pending = inner.pending.borrow_mut();
            pending.iter().position(|entry| entry.id == id).map(|at| pending.remove(at))
        }) else {
            return false;
        };
        (inner.changed)();
        if inner.pending.borrow().is_empty() && inner.opened_for_ask.replace(false) {
            drop(host::close_view(inner.view));
        }
        entry.answer.send(answer);
        true
    }

    /// The user's answer to the ask `id`, or `gone` as the error when it isn't waiting any more.
    pub fn answer(&self, id: &str, allow: bool, gone: &str) -> Result<(), String> {
        if self.settle(id, if allow { Answer::Allow } else { Answer::Deny }) { Ok(()) } else { Err(gone.to_owned()) }
    }

    /// Shows an ask that `make` builds from its id in the modal, and gives the user's answer, or `Timeout`. `notify` runs
    /// when Wings isn't in front, to tell the user something waits.
    pub fn ask(&self, make: impl FnOnce(&str) -> T, notify: impl FnOnce()) -> Pin<Box<dyn Future<Output = Answer>>> {
        let inner = &self.0;
        let id = {
            inner.next.set(inner.next.get() + 1);
            inner.next.get().to_string()
        };
        let (sender, receiver) = task::oneshot();
        inner.pending.borrow_mut().push(Entry { what: make(&id), id: id.clone(), answer: sender });
        let (late, shown) = (self.clone_handle(), self.clone_handle());
        let (late_id, shown_id) = (id.clone(), id);
        task::spawn(async move {
            host::sleep(WAIT_MS).await;
            late.settle(&late_id, Answer::Timeout);
        });
        (inner.changed)();
        // An ask Wings can't show fails at once: waiting out the 100 s for an answer nobody can give told the agent the
        // user hadn't answered, when the user never saw it.
        task::spawn(async move {
            shown.0.opened_for_ask.set(true);
            if host::open_view_with(shown.0.view, "modal", &Value::Null).await.is_err() {
                shown.settle(&shown_id, Answer::Unshown);
            }
        });
        if !host::focused() {
            notify();
        }
        Box::pin(async move { receiver.await.unwrap_or(Answer::Timeout) })
    }

    fn clone_handle(&self) -> Self {
        Self(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::testing;
    use std::cell::Cell;

    thread_local! {
        static TOLD: Cell<u32> = const { Cell::new(0) };
    }

    fn approvals() -> Approvals<u16> {
        Approvals::new("share", || TOLD.with(|told| told.set(told.get() + 1)))
    }

    fn shown(result: Result<Value, String>) {
        testing::answer(move |method, _| if method == "openView" { result.clone() } else { Ok(Value::Null) });
    }

    fn waiting(approvals: &Approvals<u16>) -> Vec<(String, u16)> {
        approvals.waiting(|id, port| (id.to_owned(), *port))
    }

    fn run(approvals: &Approvals<u16>, port: u16) -> Rc<RefCell<Option<Answer>>> {
        let slot = Rc::new(RefCell::new(None));
        let (ask, slot_in) = (approvals.ask(move |_| port, || {}), slot.clone());
        task::spawn(async move { *slot_in.borrow_mut() = Some(ask.await) });
        testing::settle();
        slot
    }

    #[test]
    fn an_ask_waits_in_the_list_until_the_user_answers_it_and_is_gone_after() {
        shown(Ok(Value::Null));
        let approvals = approvals();
        let slot = run(&approvals, 3000);
        let list = waiting(&approvals);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1, 3000);
        assert!(slot.borrow().is_none());
        approvals.answer(&list[0].0, true, "gone").unwrap();
        testing::settle();
        assert_eq!(*slot.borrow(), Some(Answer::Allow));
        assert!(waiting(&approvals).is_empty());
        assert_eq!(approvals.answer(&list[0].0, true, "gone"), Err("gone".to_owned()));
    }

    #[test]
    fn a_denied_ask_says_deny_and_a_late_one_says_timeout() {
        shown(Ok(Value::Null));
        let approvals = approvals();
        let slot = run(&approvals, 5173);
        approvals.answer(&waiting(&approvals)[0].0, false, "gone").unwrap();
        testing::settle();
        assert_eq!(*slot.borrow(), Some(Answer::Deny));
        let slot = run(&approvals, 5174);
        testing::fire_timers();
        testing::settle();
        assert_eq!(*slot.borrow(), Some(Answer::Timeout));
        assert!(waiting(&approvals).is_empty());
    }

    #[test]
    fn an_ask_wings_cannot_show_fails_at_once() {
        shown(Err("Opening views isn't available".into()));
        let approvals = approvals();
        assert_eq!(*run(&approvals, 3000).borrow(), Some(Answer::Unshown));
        assert!(waiting(&approvals).is_empty());
    }

    #[test]
    fn the_modal_closes_with_the_last_ask_unless_the_user_opened_it() {
        let closed = Rc::new(Cell::new(0));
        let count = closed.clone();
        testing::answer(move |method, _| {
            if method == "closeView" {
                count.set(count.get() + 1);
            }
            Ok(Value::Null)
        });
        let approvals = approvals();
        run(&approvals, 1);
        approvals.answer(&waiting(&approvals)[0].0, true, "gone").unwrap();
        assert_eq!(closed.get(), 1);
        run(&approvals, 2);
        approvals.opened_by_user();
        approvals.answer(&waiting(&approvals)[0].0, true, "gone").unwrap();
        assert_eq!(closed.get(), 1);
    }
}
