//! A page asks main for a change and hears how it went: `ask` is an action that keeps the page busy and shows its error,
//! `ask_quiet` hands the result to the caller.

use crate::ASK;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use wings_plugin::{messages, task::spawn};

type Done = Box<dyn FnOnce(Result<(), String>)>;

thread_local! {
    /// How many actions wait for main.
    static ACTIONS: Cell<u32> = const { Cell::new(0) };
    static ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn send(op: &str, mut args: Value, action: bool, finish: Done) {
    args["op"] = op.into();
    if action {
        ERROR.with(|slot| *slot.borrow_mut() = None);
        ACTIONS.with(|count| count.set(count.get() + 1));
    }
    let reply = messages::ask(ASK, args);
    spawn(async move {
        let result = reply.await.map(drop);
        if action {
            ACTIONS.with(|count| count.set(count.get() - 1));
            if let Err(error) = &result {
                ERROR.with(|slot| *slot.borrow_mut() = Some(error.clone()));
            }
        }
        finish(result);
    });
}

/// Asks for `op` with `args`, as an action: the page is busy until main answers, an error shows, and `after` runs when it worked.
pub fn ask(op: &str, args: Value, after: impl FnOnce() + 'static) {
    send(op, args, true, Box::new(move |result| if result.is_ok() { after() }));
}

/// `ask` for a change whose result the caller shows itself.
pub fn ask_quiet(op: &str, args: Value, done: impl FnOnce(Result<(), String>) + 'static) {
    send(op, args, false, Box::new(done));
}

pub fn busy() -> bool {
    ACTIONS.with(|count| count.get() > 0)
}

pub fn error() -> Option<String> {
    ERROR.with(|slot| slot.borrow().clone())
}

pub fn clear_error() {
    ERROR.with(|slot| *slot.borrow_mut() = None);
}
