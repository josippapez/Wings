//! The main script: a hidden frame that runs while the plugin is on. It sets each pane's badge, tells the sidebar which
//! panes there are, and answers Claude's `pane_folders` tool.

use crate::{PANES, WANT, folder_name};
use serde_json::{Value, json};
use std::cell::RefCell;
use wings_plugin::{host, messages, task, tools};

thread_local! {
    /// The panes as of the last change, `[{ paneId, folder, cwd }]`.
    static PANES_NOW: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

fn tell_sidebar() {
    PANES_NOW.with(|panes| messages::tell(PANES, &Value::Array(panes.borrow().clone())));
}

fn panes_changed(list: &[Value]) {
    let panes: Vec<Value> = list
        .iter()
        .map(|pane| {
            let cwd = pane["cwd"].as_str().unwrap_or_default();
            json!({ "paneId": pane["paneId"], "folder": folder_name(cwd), "cwd": cwd })
        })
        .collect();
    for pane in &panes {
        let id = pane["paneId"].as_str().unwrap_or_default().to_owned();
        // Wings skips a badge a pane already shows, so every change can set every pane's.
        let badge = json!({
            "label": pane["folder"],
            "tone": "neutral",
            "rows": [{ "label": "Folder", "value": pane["cwd"] }],
        });
        task::spawn(async move { drop(host::set_badge(&id, &badge).await) });
    }
    PANES_NOW.with(|now| *now.borrow_mut() = panes);
    tell_sidebar();
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    host::on_panes(panes_changed);
    // A sidebar that opens after the last change asks for the panes.
    messages::on(WANT, |_| tell_sidebar());
    tools::register("pane_folders", json!({ "type": "object", "properties": {}, "additionalProperties": false }), |_, _| async {
        Ok(PANES_NOW.with(|panes| Value::Array(panes.borrow().clone())))
    });
}
