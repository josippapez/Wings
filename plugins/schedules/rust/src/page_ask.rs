//! A page asks main for a change and hears how it went: `ask` is an action that keeps the page busy and shows its error,
//! `ask_quiet` hands the result to the caller.

use crate::{ASK, DONE};
use serde_json::Value;
use std::{cell::{Cell, RefCell}, collections::HashMap};
use wings_plugin::{host, messages};

type Done = Box<dyn FnOnce(Result<(), String>)>;

thread_local! {
    static PENDING: RefCell<HashMap<String, (bool, Done)>> = RefCell::new(HashMap::new());
    static ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    static NEXT: Cell<u32> = const { Cell::new(0) };
    static PREFIX: RefCell<&'static str> = const { RefCell::new("") };
}

/// Hears how main did each change this frame asked for. `prefix` tells this frame's asks from another's.
pub fn listen(prefix: &'static str) {
    PREFIX.with(|slot| *slot.borrow_mut() = prefix);
    messages::on(DONE, |done| {
        let Some(id) = done["id"].as_str() else {
            return;
        };
        let Some((action, finish)) = PENDING.with(|pending| pending.borrow_mut().remove(id)) else {
            return;
        };
        let result = match done["error"].as_str() {
            Some(error) => Err(error.to_owned()),
            None => Ok(()),
        };
        if let (true, Err(error)) = (action, &result) {
            ERROR.with(|slot| *slot.borrow_mut() = Some(error.clone()));
        }
        finish(result);
    });
}

fn send(op: &str, mut args: Value, action: bool, finish: Done) {
    let id = format!("{}{}-{}", PREFIX.with(|prefix| *prefix.borrow()), host::now() as i64, NEXT.with(|next| {
        next.set(next.get() + 1);
        next.get()
    }));
    args["id"] = id.clone().into();
    args["op"] = op.into();
    if action {
        ERROR.with(|slot| *slot.borrow_mut() = None);
    }
    PENDING.with(|pending| pending.borrow_mut().insert(id, (action, finish)));
    messages::tell(ASK, &args);
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
    PENDING.with(|pending| pending.borrow().values().any(|(action, _)| *action))
}

pub fn error() -> Option<String> {
    ERROR.with(|slot| slot.borrow().clone())
}

pub fn clear_error() {
    ERROR.with(|slot| *slot.borrow_mut() = None);
}
