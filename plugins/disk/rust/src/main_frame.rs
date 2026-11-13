//! The main script: it runs every scan and keeps the results for the tabs and for Claude's tools, keeps the Collector, and
//! does what the tab asks on the disk (the Trash, Finder, Quick Look, local snapshots).

use crate::{
    actions, answers, collector,
    scans::{forget_scan, publish, start_scan, stop_scan},
    state::{ASK, Collected, DONE, WANT},
};
use serde_json::{Value, json};
use wings_plugin::{messages, task::spawn, tools};

fn text(asked: &Value, name: &str) -> String {
    asked[name].as_str().unwrap_or_default().to_owned()
}

/// Does what a tab asked for, and gives what it answers with.
async fn answer(asked: Value) -> Result<Value, String> {
    match asked["op"].as_str().unwrap_or_default() {
        // A scan answers once it shows as scanning: its progress and result come through the state.
        "scan" => {
            start_scan(&text(&asked, "root")).started.wait().await;
            Ok(Value::Null)
        }
        "stop" => Ok(stop_scan(&text(&asked, "root")).into()),
        "forget" => Ok(forget_scan(&text(&asked, "root")).into()),
        "collect" => {
            let items = asked["items"].as_array().map_or_else(Vec::new, |list| list.iter().filter_map(Collected::from_value).collect());
            let (_, refused) = collector::collect(items).await?;
            if refused.is_empty() { Ok(Value::Null) } else { Err(refused.iter().map(|r| r.reason.as_str()).collect::<Vec<_>>().join(" ")) }
        }
        "uncollect" => {
            let paths: Vec<String> = asked["paths"].as_array().map_or_else(Vec::new, |list| list.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect());
            collector::uncollect(&paths).map(|()| Value::Null)
        }
        "trash" => {
            let reviewed: Vec<String> = asked["paths"].as_array().into_iter().flatten().filter_map(|path| path.as_str().map(str::to_owned)).collect();
            actions::trash_collected(&reviewed).await.map(|result| result.to_value())
        }
        "reveal" => actions::reveal(&text(&asked, "path")).await.map(|()| Value::Null),
        "preview" => actions::preview(&text(&asked, "path")).map(|()| Value::Null),
        "deleteSnapshot" => actions::delete_snapshot(&text(&asked, "date")).await.map(|()| Value::Null),
        "purge" => actions::purge(&text(&asked, "mount")).await.map(|()| Value::Null),
        other => Err(format!("The Disk usage main doesn't do {other}.")),
    }
}

fn register_tools() {
    let path = json!({ "type": "string", "minLength": 1 });
    tools::register("list_disks", json!({ "type": "object", "properties": {}, "additionalProperties": false }), |_, _| answers::list_disks());
    tools::register(
        "folder_usage",
        json!({ "type": "object", "properties": {
            "path": path,
            "depth": { "type": "integer", "minimum": 1, "maximum": 4 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 50 },
            "fresh": { "type": "boolean" },
        }, "required": ["path"], "additionalProperties": false }),
        |input, _| answers::folder_usage(input),
    );
    tools::register(
        "largest_files",
        json!({ "type": "object", "properties": { "path": path, "limit": { "type": "integer", "minimum": 1, "maximum": 100 } }, "required": ["path"], "additionalProperties": false }),
        |input, _| answers::largest_files(input),
    );
    tools::register(
        "collect",
        json!({ "type": "object", "properties": { "paths": { "type": "array", "items": { "type": "string", "minLength": 1 }, "maxItems": 200 } }, "required": ["paths"], "additionalProperties": false }),
        |input, _| answers::collect(input),
    );
}

#[unsafe(no_mangle)]
pub fn start() {
    register_tools();
    messages::on(WANT, |_| publish());
    messages::on(ASK, |asked| {
        let asked = asked.clone();
        spawn(async move {
            let id = asked["id"].clone();
            let told = match answer(asked).await {
                Ok(result) => json!({ "id": id, "result": result }),
                Err(error) => json!({ "id": id, "error": error }),
            };
            messages::tell(DONE, &told);
        });
    });
    publish();
}
