//! Named events between the frames of one plugin, as `wings-plugin-sdk/messages` sends them over `wings.broadcast`, so
//! a frame in Rust and one in TypeScript hear each other. Only its `tell` and `on` are here: asks, queries, and a
//! message too long for one broadcast, which comes in parts, are not.

use crate::host;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

thread_local! {
    static EVENTS: RefCell<HashMap<String, Vec<Rc<dyn Fn(&Value)>>>> = RefCell::new(HashMap::new());
}

/// Tells the plugin's other frames `name` happened, with `payload` when it isn't null.
pub fn tell(name: &str, payload: &Value) {
    let mut message = json!({ "wings:sdk": 1, "kind": "tell", "name": name });
    if !payload.is_null() {
        message["payload"] = payload.clone();
    }
    host::broadcast(&message);
}

/// Hears each `name` another frame tells, with its payload, or null when it has none.
pub fn on(name: &str, listener: impl Fn(&Value) + 'static) {
    let first = EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        let first = events.is_empty();
        events.entry(name.to_owned()).or_default().push(Rc::new(listener));
        first
    });
    if first {
        host::on_broadcast(|message| {
            if message["wings:sdk"] != 1 || message["kind"] != "tell" {
                return;
            }
            let name = message["name"].as_str().unwrap_or_default();
            for listener in EVENTS.with(|events| events.borrow().get(name).cloned()).unwrap_or_default() {
                listener(&message["payload"]);
            }
        });
    }
}
