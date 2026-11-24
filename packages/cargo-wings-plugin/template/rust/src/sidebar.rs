//! The sidebar page: the panes main tells it about. Wings builds the page again after each message it hears.

use crate::{PANES, WANT};
use serde_json::Value;
use std::cell::RefCell;
use wings_plugin::{
    messages,
    ui::{Node, empty, list, loading, muted, stack, text},
};

thread_local! {
    /// What main last told, or none before it has.
    static PANES_NOW: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
}

fn view() -> Vec<Node> {
    PANES_NOW.with(|panes| match &*panes.borrow() {
        None => vec![loading()],
        Some(panes) if panes.is_empty() => vec![empty("No terminals", "Open a terminal pane and its folder shows here.")],
        Some(panes) => vec![list().children(panes.iter().map(|pane| {
            let id = pane["paneId"].as_str().unwrap_or_default();
            Node::new("ListItem").key(id).child(
                stack()
                    .prop("gap", "none")
                    .prop("grow", true)
                    .child(text(pane["folder"].as_str().unwrap_or_default()).prop("truncate", true))
                    .child(muted(pane["cwd"].as_str().unwrap_or_default()).prop("truncate", true)),
            )
        }))],
    })
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
    messages::on(PANES, |panes| PANES_NOW.with(|now| *now.borrow_mut() = Some(panes.as_array().cloned().unwrap_or_default())));
    messages::tell(WANT, &Value::Null);
}
