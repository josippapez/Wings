//! A tab asks main to do something and waits for its answer, over the plugin's messages.

use crate::{
    gate::Gate,
    state::{ASK, DONE},
};
use serde_json::{Value, json};
use std::{cell::{Cell, RefCell}, collections::HashMap, rc::Rc};
use wings_plugin::{host, messages};

struct Waiting {
    done: Gate,
    answer: RefCell<Option<Result<Value, String>>>,
}

thread_local! {
    static WAITING: RefCell<HashMap<String, Rc<Waiting>>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(0) };
    /// What tells this tab's asks from another tab's, which main answers in the same message.
    static TAB: Cell<u64> = const { Cell::new(0) };
}

/// Hears main's answers. Call it once, from `start`.
pub fn listen() {
    TAB.with(|tab| tab.set(host::now() as u64));
    messages::on(DONE, |told| {
        let Some(waiting) = told["id"].as_str().and_then(|id| WAITING.with(|waiting| waiting.borrow_mut().remove(id))) else { return };
        *waiting.answer.borrow_mut() = Some(match told["error"].as_str() {
            Some(error) => Err(error.to_owned()),
            None => Ok(told["result"].clone()),
        });
        waiting.done.open();
    });
}

/// Asks main to do `op` with `args`, and gives what it answers, or why it couldn't.
pub async fn ask(op: &str, args: Value) -> Result<Value, String> {
    let id = format!("{}-{}", TAB.with(Cell::get), NEXT.with(|next| next.replace(next.get() + 1)));
    let waiting = Rc::new(Waiting { done: Gate::default(), answer: RefCell::new(None) });
    WAITING.with(|all| all.borrow_mut().insert(id.clone(), waiting.clone()));
    let mut asked = if args.is_null() { json!({}) } else { args };
    asked["id"] = id.into();
    asked["op"] = op.into();
    messages::tell(ASK, &asked);
    waiting.done.wait().await;
    let answer = waiting.answer.borrow_mut().take();
    answer.unwrap_or_else(|| Err("Wings gave no answer.".into()))
}
