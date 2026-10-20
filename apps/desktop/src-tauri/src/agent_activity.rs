//! What an OpenCode session is doing, for plugins with `permissions.agentActivity`.
//!
//! OpenCode has no transcript file, so a plugin can't follow it the way it follows Claude Code's. Instead the
//! newest page of the session's messages is fetched from the background service (see `opencode.rs`, which
//! keeps the password) and turned into a short list of prompts and tool calls, plus the model, context size
//! and token totals. A plugin never sees the service address or its password, only this.
//!
//! Only one page is asked for, newest first: one session here has a 78 MB page of tool output, and the
//! activity only needs what happened lately.

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::opencode::{self, Api};

/// Messages asked for: the latest few turns, which hold the calls a sidebar lists.
const MESSAGES: &str = "30";
/// Characters kept of one tool input field and of a tool result.
const INPUT_FIELD: usize = 2000;
const RESULT: usize = 1500;
/// Input fields kept of a tool call.
const INPUT_FIELDS: usize = 12;
const PROMPT: usize = 4000;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    Prompt { id: String, at: u64, text: String },
    Tool {
        id: String,
        at: u64,
        name: String,
        input: Value,
        /// `running`, `completed` or `error`.
        status: String,
        ms: Option<u64>,
        result: Option<String>,
        /// Files the call changed, absolute when the session's folder is known.
        files: Vec<String>,
    },
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub model: Option<String>,
    /// What the newest reply read: input plus cache, the same sum as Claude Code's context.
    pub context: Option<u64>,
    /// The session's totals, children included.
    pub tokens: Tokens,
    pub cost: f64,
    /// Oldest first.
    pub items: Vec<Item>,
}

fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

fn number(v: &Value, pointer: &str) -> u64 {
    v.pointer(pointer).and_then(Value::as_u64).unwrap_or(0)
}

fn tokens_of(v: &Value) -> Tokens {
    Tokens { input: number(v, "/input"), output: number(v, "/output"), reasoning: number(v, "/reasoning"), cache_read: number(v, "/cache/read"), cache_write: number(v, "/cache/write") }
}

/// A tool's input with every string cut short and at most `INPUT_FIELDS` fields.
fn input_of(state: &Value) -> Value {
    let Some(input) = state.get("input").and_then(Value::as_object) else { return json!({}) };
    let mut out = Map::new();
    for (key, value) in input.iter().take(INPUT_FIELDS) {
        let value = match value {
            Value::String(s) => Value::String(cut(s, INPUT_FIELD)),
            other => Value::String(cut(&other.to_string(), INPUT_FIELD)),
        };
        out.insert(key.clone(), value);
    }
    Value::Object(out)
}

fn absolute(directory: Option<&str>, file: &str) -> String {
    match directory {
        Some(dir) if !file.starts_with('/') => format!("{}/{}", dir.trim_end_matches('/'), file),
        _ => file.to_string(),
    }
}

/// The files a finished call changed: the ones the tool reports (`patch` lists each with its status).
fn files_of(state: &Value, directory: Option<&str>) -> Vec<String> {
    state.pointer("/metadata/files").and_then(Value::as_array).into_iter().flatten().filter_map(|f| f.get("file").and_then(Value::as_str)).map(|f| absolute(directory, f)).collect()
}

fn result_of(state: &Value) -> Option<String> {
    // An errored call has an `error` and may have no `content`.
    let text = state.get("content").and_then(Value::as_array).into_iter().flatten().filter(|c| c.get("type").and_then(Value::as_str) == Some("text")).filter_map(|c| c.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n");
    let text = if text.is_empty() { state.pointer("/error/message").and_then(Value::as_str).unwrap_or_default().to_string() } else { text };
    (!text.is_empty()).then(|| cut(&text, RESULT))
}

fn tool_item(part: &Value, directory: Option<&str>) -> Option<Item> {
    let state = part.get("state")?;
    let status = match state.get("status").and_then(Value::as_str)? {
        "completed" => "completed",
        "error" => "error",
        _ => "running",
    };
    let at = number(part, "/time/created");
    let ms = part.pointer("/time/completed").and_then(Value::as_u64).map(|done| done.saturating_sub(at));
    Some(Item::Tool {
        id: part.get("id")?.as_str()?.to_string(),
        at,
        name: part.get("name")?.as_str()?.to_string(),
        input: input_of(state),
        status: status.to_string(),
        ms,
        result: result_of(state),
        files: files_of(state, directory),
    })
}

/// The activity from `GET /api/session/:id` and the newest messages, which the service sends newest first.
pub fn normalize(info: &Value, messages_newest_first: &[Value]) -> Activity {
    let directory = info.pointer("/location/directory").and_then(Value::as_str);
    let mut items = Vec::new();
    let mut model = info.pointer("/model/id").and_then(Value::as_str).map(str::to_string);
    let mut context = None;
    for message in messages_newest_first.iter().rev() {
        match message.get("type").and_then(Value::as_str) {
            Some("user") => {
                let text = message.get("text").and_then(Value::as_str).unwrap_or_default().trim();
                if let (false, Some(id)) = (text.is_empty(), message.get("id").and_then(Value::as_str)) {
                    items.push(Item::Prompt { id: id.to_string(), at: number(message, "/time/created"), text: cut(text, PROMPT) });
                }
            }
            Some("assistant") => {
                if let Some(id) = message.pointer("/model/id").and_then(Value::as_str) {
                    model = Some(id.to_string());
                }
                if let Some(t) = message.get("tokens") {
                    let t = tokens_of(t);
                    // A reply that is still being written has no tokens yet.
                    if t.input + t.cache_read + t.cache_write > 0 {
                        context = Some(t.input + t.cache_read + t.cache_write);
                    }
                }
                items.extend(message.get("content").and_then(Value::as_array).into_iter().flatten().filter(|p| p.get("type").and_then(Value::as_str) == Some("tool")).filter_map(|p| tool_item(p, directory)));
            }
            _ => {}
        }
    }
    Activity { model, context, tokens: info.get("tokens").map(tokens_of).unwrap_or(Tokens { input: 0, output: 0, reasoning: 0, cache_read: 0, cache_write: 0 }), cost: info.get("cost").and_then(Value::as_f64).unwrap_or(0.0), items }
}

/// Fetches the session and its newest messages. Two requests, a few milliseconds each on this machine.
pub fn fetch(api: &impl Api, session_id: &str) -> Result<Activity, String> {
    if !opencode::plausible_id(session_id) {
        return Err("not an OpenCode session id".into());
    }
    let describe = |e: opencode::Error| e.to_string();
    let info = api.get(&format!("/api/session/{session_id}"), &[]).map_err(describe)?;
    let page = api.get(&format!("/api/session/{session_id}/message"), &[("limit", MESSAGES), ("order", "desc")]).map_err(describe)?;
    let messages = page.get("data").and_then(Value::as_array).ok_or("unexpected answer from the service")?;
    Ok(normalize(info.get("data").unwrap_or(&Value::Null), messages))
}

/// Fetches through a fresh client for the registered service.
pub fn fetch_live(session_id: &str) -> Result<Activity, String> {
    fetch(&opencode::connect().map_err(|e| e.to_string())?, session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> Value {
        json!({ "location": { "directory": "/work/app" }, "model": { "id": "gpt-x" }, "cost": 0.25, "tokens": { "input": 100, "output": 20, "reasoning": 3, "cache": { "read": 500, "write": 7 } } })
    }

    fn tool(id: &str, name: &str, state: Value) -> Value {
        json!({ "type": "tool", "id": id, "name": name, "state": state, "time": { "created": 1000, "ran": 1100, "completed": 1400 } })
    }

    fn assistant(tokens: Value, content: Vec<Value>) -> Value {
        json!({ "id": "msg_a", "type": "assistant", "model": { "id": "gpt-y" }, "tokens": tokens, "content": content })
    }

    #[test]
    fn newest_first_messages_become_oldest_first_items() {
        let messages = vec![
            json!({ "id": "msg_idle", "type": "idle" }),
            assistant(json!({ "input": 10, "output": 1, "cache": { "read": 90, "write": 0 } }), vec![json!({ "type": "text", "text": "done" })]),
            assistant(
                json!({ "input": 5, "output": 1, "cache": { "read": 40, "write": 5 } }),
                vec![tool("call_1", "patch", json!({ "status": "completed", "input": { "patchText": "x" }, "content": [{ "type": "text", "text": "Success" }], "metadata": { "files": [{ "file": "src/a.txt" }, { "file": "/abs/b.txt" }] } }))],
            ),
            json!({ "id": "msg_u", "type": "user", "text": "  fix it  ", "time": { "created": 900 } }),
        ];
        let a = normalize(&info(), &messages);
        assert_eq!(a.model.as_deref(), Some("gpt-y"));
        assert_eq!(a.context, Some(100), "from the newest reply");
        assert_eq!((a.tokens.input, a.tokens.cache_read, a.cost), (100, 500, 0.25));
        assert_eq!(a.items.len(), 2);
        assert_eq!(a.items[0], Item::Prompt { id: "msg_u".into(), at: 900, text: "fix it".into() });
        let Item::Tool { name, status, ms, result, files, .. } = &a.items[1] else { panic!("a tool call") };
        assert_eq!((name.as_str(), status.as_str(), *ms, result.as_deref()), ("patch", "completed", Some(400), Some("Success")));
        assert_eq!(files, &["/work/app/src/a.txt", "/abs/b.txt"], "relative paths resolve against the session folder");
    }

    #[test]
    fn a_call_in_flight_is_running_and_errors_carry_their_message() {
        let messages = vec![assistant(
            json!({}),
            vec![
                // Still running: no completed time yet.
                json!({ "type": "tool", "id": "c1", "name": "shell", "state": { "status": "running", "input": { "command": "sleep 9" }, "metadata": {} }, "time": { "created": 1000 } }),
                tool("c2", "read", json!({ "status": "error", "input": {}, "error": { "message": "no such file" } })),
            ],
        )];
        let a = normalize(&info(), &messages);
        assert_eq!(a.context, None, "a reply without tokens doesn't set the context");
        let [Item::Tool { status: s1, ms: m1, .. }, Item::Tool { status: s2, result, .. }] = &a.items[..] else { panic!("two calls") };
        assert_eq!((s1.as_str(), *m1, s2.as_str(), result.as_deref()), ("running", None, "error", Some("no such file")));
    }

    #[test]
    fn long_text_is_cut() {
        let long = "é".repeat(INPUT_FIELD + 10);
        let messages = vec![assistant(json!({}), vec![tool("c", "shell", json!({ "status": "completed", "input": { "command": long }, "content": [{ "type": "text", "text": "x".repeat(RESULT * 2) }] }))])];
        let Item::Tool { input, result, .. } = &normalize(&info(), &messages).items[0] else { panic!("a tool call") };
        assert_eq!(input["command"].as_str().unwrap().chars().count(), INPUT_FIELD + 1);
        assert_eq!(result.as_ref().unwrap().chars().count(), RESULT + 1);
    }

    struct Fake;
    impl Api for Fake {
        fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, opencode::Error> {
            if path.ends_with("/message") {
                assert_eq!(query, [("limit", MESSAGES), ("order", "desc")], "one newest page, never the history");
                Ok(json!({ "data": [{ "id": "m", "type": "user", "text": "hi", "time": { "created": 1 } }] }))
            } else {
                Ok(json!({ "data": info() }))
            }
        }
    }

    #[test]
    fn fetch_asks_for_the_newest_page_only_and_refuses_odd_ids() {
        assert_eq!(fetch(&Fake, "ses_abc").unwrap().items.len(), 1);
        assert!(fetch(&Fake, "../x").is_err());
    }

    /// Reads the sessions of the OpenCode service on this Mac and prints what the sidebar would get:
    /// `cargo test live_agent_activity -- --ignored --nocapture`. It only reads.
    #[test]
    #[ignore]
    fn live_agent_activity() {
        let client = opencode::connect().expect("an opencode service is running");
        let list = client.get("/api/session", &[("limit", "40")]).unwrap();
        let mut worst = std::time::Duration::ZERO;
        for session in list["data"].as_array().unwrap() {
            let id = session["id"].as_str().unwrap();
            let start = std::time::Instant::now();
            let a = fetch(&client, id).unwrap();
            let took = start.elapsed();
            worst = worst.max(took);
            let tools = a.items.iter().filter(|i| matches!(i, Item::Tool { .. })).count();
            let files: usize = a.items.iter().map(|i| if let Item::Tool { files, .. } = i { files.len() } else { 0 }).sum();
            println!("{id} {:?}: model {:?}, context {:?}, {} prompts, {tools} tool calls, {files} files, {} bytes", took, a.model, a.context, a.items.len() - tools, serde_json::to_string(&a).unwrap().len());
        }
        println!("slowest: {worst:?}");
    }
}
