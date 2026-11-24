//! Pages of the plugin that reach each other by name, like a chat tab and the tabs it opens for its subagents (Needs
//! Wings 0.2.41). A page takes a name with `name`, every page hears which names there are with `on_names`, a page tells
//! another a message with `tell` and hears what it is told with `on_told`, and `show` brings a named page's tab to the
//! front.
//!
//! `tell` with `write` hands on your click or key: a page that doesn't run the program, like a subagent's tab, has the
//! page that does write to it once, for the Enter you pressed. Wings lets a page write to a program only right after
//! your click or key in it, and this is the one way round that, one write for each.

use std::rc::Rc;

use serde_json::{Value, json};

use crate::host::{self, Pending};

/// This page answers to `name` (1 to 128 letters, digits, `-`, `_`, `.` and `:`) from now on, in place of a name it had;
/// a page that had it loses it. `None` drops its name. The name goes when the page closes.
pub fn name(name: Option<&str>) -> Pending<Result<Value, String>> {
    host::call("namePage", json!({ "name": name }))
}

/// The names the plugin's pages have, now and each time one is taken or goes.
pub fn on_names(listener: impl Fn(&[String]) + 'static) {
    let listener: Rc<dyn Fn(&[String])> = Rc::new(listener);
    let heard = listener.clone();
    host::on("pages", move |_, names| heard(&names_of(names)));
    let now = host::call("pages", json!({}));
    crate::task::spawn(async move {
        if let Ok(names) = now.await {
            listener(&names_of(&names));
        }
    });
}

fn names_of(names: &Value) -> Vec<String> {
    names.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

/// Tells the page named `name` `message` (JSON of up to 1,000,000 characters). With `write`, right after your click or key in this page,
/// which goes with it: that page may then write once to a program it runs. It fails when no page has that name.
pub fn tell(name: &str, message: &Value, write: bool) -> Pending<Result<Value, String>> {
    host::call("tellPage", json!({ "name": name, "message": message, "write": write }))
}

/// Hears what other pages tell this one: the message, the name of the page that told it (`None` for one without a
/// name), and whether it came with a write.
pub fn on_told(listener: impl Fn(&Value, Option<&str>, bool) + 'static) {
    host::on("pageMessage", move |_, told| listener(&told["message"], told["from"].as_str(), told["write"] == true));
}

/// Brings the tab of the page named `name` to the front, in its project.
pub fn show(name: &str) -> Pending<Result<Value, String>> {
    host::call("showPage", json!({ "name": name }))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::host::testing;

    #[test]
    fn a_message_hands_on_a_write_only_when_asked_and_a_tab_opens_beside_this_one() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        drop(tell("chat:s1", &json!({ "kind": "send" }), true));
        drop(tell("subagent:s1:a1", &json!({ "kind": "line" }), false));
        drop(host::open_tab_beside("chat", &json!({ "subagent": { "agent": "a1" } })));
        assert_eq!(
            *calls.borrow(),
            [
                ("tellPage".to_owned(), json!({ "name": "chat:s1", "message": { "kind": "send" }, "write": true })),
                ("tellPage".to_owned(), json!({ "name": "subagent:s1:a1", "message": { "kind": "line" }, "write": false })),
                ("openView".to_owned(), json!({ "id": "chat", "as": "tab", "args": { "subagent": { "agent": "a1" } }, "newTab": false, "after": "self" })),
            ]
        );
    }

    #[test]
    fn the_names_are_heard_now_and_each_time_they_change() {
        testing::answer(|method, _| Ok(if method == "pages" { json!(["chat:s1"]) } else { Value::Null }));
        let heard = Rc::new(RefCell::new(Vec::new()));
        let kept = heard.clone();
        on_names(move |names| kept.borrow_mut().push(names.to_vec()));
        crate::task::run_ready();
        host::deliver("pages", 0, r#"["chat:s1","subagent:s1:a1"]"#.into());
        assert_eq!(*heard.borrow(), [vec!["chat:s1".to_owned()], vec!["chat:s1".to_owned(), "subagent:s1:a1".to_owned()]]);
    }
}
