//! The fixture plugin's main in Rust: it counts its starts in storage, badges every pane, answers the tool `greet` and
//! any action, never comes back from the tool `spin`, and draws a page, which a main can't, at the tool `draw`: the app
//! stops it at both. Built by the tests that run it
//! (`page::plugin::wasm_tests`).

use serde_json::json;
use wings_plugin::host::{self, storage};

#[unsafe(no_mangle)]
pub fn start() {
    let starts = storage::get("starts").ok().and_then(|n| n.as_u64()).unwrap_or(0);
    let _ = storage::set("starts", &json!(starts + 1));
    host::on("panes", |_, panes| {
        for pane in panes.as_array().into_iter().flatten() {
            drop(host::call("setBadge", json!({ "paneId": pane["paneId"], "badge": { "label": "rust" } })));
        }
    });
    host::on("tool", |token, call| {
        if call["name"] == "spin" {
            loop {}
        }
        if call["name"] == "draw" {
            wings_plugin::run(Vec::new);
            return;
        }
        let text = format!("Hello, {}", call["arguments"]["name"].as_str().unwrap_or("nobody"));
        drop(host::call("toolDone", json!({ "token": token, "text": text, "isError": false })));
    });
    host::on("action", |token, action| {
        let _ = storage::set("action", &action["actionId"]);
        drop(host::call("actionDone", json!({ "token": token, "error": null })));
    });
}
