//! The fixture plugin's main in Rust, which uses each part of the Rust SDK a main has: it counts its starts and its
//! polls in storage, badges every pane, answers any action, answers `ping` from another frame with `pong`, and has
//! tools: `greet`, `whoami`, `label` (the sidebar's label), `open` (shows the sidebar), `coded`, whose schema has a
//! pattern, `spin`, which never comes back, and `draw`, which draws a
//! page, as a main can't. The app stops it at those two. Built by the tests that run it (`page::plugin::wasm_tests`).

use serde_json::{Value, json};
use wings_plugin::{
    host::{self, storage},
    messages, poll, tools,
};

fn count(key: &str) {
    let so_far = storage::get(key).ok().and_then(|n| n.as_u64()).unwrap_or(0);
    let _ = storage::set(key, &json!(so_far + 1));
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}

#[unsafe(no_mangle)]
pub fn start() {
    count("starts");
    host::on("panes", |_, panes| {
        for pane in panes.as_array().into_iter().flatten() {
            drop(host::call("setBadge", json!({ "paneId": pane["paneId"], "badge": { "label": "rust" } })));
        }
    });
    host::on("action", |token, action| {
        let _ = storage::set("action", &action["actionId"]);
        drop(host::call("actionDone", json!({ "token": token, "error": null })));
    });
    messages::on("ping", |payload| messages::tell("pong", payload));
    poll::while_focused(50, false, || count("ticks"));

    tools::register("greet", object(json!({ "name": { "type": "string", "maxLength": 10 } }), &["name"]), |input, _| async move {
        Ok(json!(format!("Hello, {}", input["name"].as_str().unwrap_or_default())))
    });
    tools::register("whoami", object(json!({}), &[]), |_, context| async move { Ok(context) });
    tools::register("label", object(json!({ "text": { "type": "string" } }), &[]), |input, _| async move {
        host::set_sidebar_label("side", input["text"].as_str(), &[("Task", "Writing")]);
        Ok(Value::Null)
    });
    tools::register("open", object(json!({}), &[]), |_, _| async {
        host::open_view("side");
        Ok(Value::Null)
    });
    tools::register("coded", object(json!({ "code": { "type": "string", "pattern": "^[A-Z]+$" } }), &[]), |_, _| async { Ok(json!("ran")) });
    tools::register("spin", object(json!({}), &[]), |_, _| async { loop {} });
    tools::register("draw", object(json!({}), &[]), |_, _| async {
        wings_plugin::run(Vec::new);
        Ok(Value::Null)
    });
}
