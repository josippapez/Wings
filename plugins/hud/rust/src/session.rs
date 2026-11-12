//! One Claude session, and what its transcript says about it.

use crate::format::text;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Tools a claude.ai connector offers until you sign in to it.
const SIGN_IN_TOOLS: [&str; 2] = ["authenticate", "complete_authentication"];

#[derive(Default)]
pub struct Session {
    pub id: String,
    /// The pane that runs it, which shows its strip.
    pub pane_id: String,
    pub project: String,
    pub name: Option<String>,
    /// Its pane is the selected one: the focused pane of the tab on screen.
    pub focused: bool,
    /// When this run's `claude` started, in ms. What Claude Code reported before that isn't this run's.
    pub started_at: f64,
    // What one `claude` process knows. A new process reconnects to every MCP server, so this starts over.
    pub tokens: Option<f64>,
    pub window: Option<f64>,
    /// Connected servers and their tools.
    tools: BTreeMap<String, BTreeSet<String>>,
    /// Servers that failed to connect, with the error.
    failed: BTreeMap<String, String>,
    pending: BTreeSet<String>,
    /// Servers whose tools Claude called.
    used: BTreeSet<String>,
    /// Whether each server's last call failed.
    last_failed: HashMap<String, bool>,
    /// Tool call id to server.
    calls: HashMap<String, String>,
    /// In the order Claude first used them.
    pub skills: Vec<String>,
}

pub fn session(id: &str) -> Session {
    Session { id: id.to_owned(), ..Session::default() }
}

/// A new `claude` process runs the session, so what the last one knew is dropped.
pub fn start_run(s: &mut Session, started_at: f64) {
    *s = Session { id: std::mem::take(&mut s.id), pane_id: std::mem::take(&mut s.pane_id), project: std::mem::take(&mut s.project), name: s.name.take(), focused: s.focused, started_at, ..Session::default() };
}

/// `mcp__plugin_dev-core_chrome-devtools__click` is server `plugin_dev-core_chrome-devtools`, tool `click`.
fn mcp_tool(name: &Value) -> Option<(&str, &str)> {
    let name = name.as_str()?.strip_prefix("mcp__")?;
    let at = name.match_indices("__").map(|(at, _)| at).find(|&at| at >= 1 && at + 2 < name.len())?;
    Some((&name[..at], &name[at + 2..]))
}

fn skill(s: &mut Session, name: &Value) {
    let name = text(name, 64);
    if !name.is_empty() && !s.skills.contains(&name) {
        s.skills.push(name);
    }
}

fn list(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

/// `this_run` is false for an entry from before this `claude` started, which a resumed session's transcript also holds.
pub fn ingest(s: &mut Session, entry: &Value, this_run: bool) {
    match entry["type"].as_str() {
        Some("attachment") => {
            let a = &entry["attachment"];
            match a["type"].as_str() {
                // `claude-opus-5-5[1m]` is the 1M-token window; otherwise Claude Code's default of 200k.
                Some("model") => {
                    let model = a["identity"]["modelId"].as_str().unwrap_or_default().to_lowercase();
                    s.window = Some(if model.ends_with("[1m]") { 1_000_000.0 } else { 200_000.0 });
                }
                Some("deferred_tools_delta") if this_run => {
                    for (server, tool) in list(&a["addedNames"]).filter_map(mcp_tool) {
                        s.tools.entry(server.to_owned()).or_default().insert(tool.to_owned());
                    }
                    for (server, tool) in list(&a["removedNames"]).filter_map(mcp_tool) {
                        if let Some(tools) = s.tools.get_mut(server) {
                            tools.remove(tool);
                            if tools.is_empty() {
                                s.tools.remove(server);
                            }
                        }
                    }
                    // These two list every server in that state now, not what changed.
                    if let Some(failed) = a["failedMcpServers"].as_array() {
                        s.failed = failed
                            .iter()
                            .map(|f| {
                                let code = &f["errorCode"];
                                let empty = code.is_null() || code == "" || code == false || code == 0;
                                (text(&f["name"], 64), text(if empty { &f["error"] } else { code }, 80))
                            })
                            .collect();
                    }
                    if let Some(pending) = a["pendingMcpServers"].as_array() {
                        s.pending = pending.iter().map(|name| text(name, 64)).collect();
                    }
                }
                Some("invoked_skills") => {
                    for used in list(&a["skills"]) {
                        skill(s, &used["name"]);
                    }
                }
                _ => {}
            }
        }
        Some("assistant") => {
            let usage = &entry["message"]["usage"];
            // What Claude Code counts as context: everything sent in, cached or not.
            if usage.is_object() {
                let count = |name: &str| usage[name].as_f64().unwrap_or(0.0);
                s.tokens = Some(count("input_tokens") + count("cache_creation_input_tokens") + count("cache_read_input_tokens"));
            }
            for block in list(&entry["message"]["content"]).filter(|block| block["type"] == "tool_use") {
                if block["name"] == "Skill" {
                    skill(s, &block["input"]["skill"]);
                }
                if let Some((server, _)) = mcp_tool(&block["name"]) {
                    s.used.insert(server.to_owned());
                    s.calls.insert(block["id"].as_str().unwrap_or_default().to_owned(), server.to_owned());
                }
            }
        }
        Some("user") => {
            for block in list(&entry["message"]["content"]).filter(|block| block["type"] == "tool_result") {
                let call = block["tool_use_id"].as_str().unwrap_or_default();
                if let Some(server) = s.calls.remove(call) {
                    s.last_failed.insert(server, block["is_error"] == true);
                }
            }
        }
        _ => {}
    }
}

/// Every server this run knows about, failed ones first: each `{ name, state, tools?, error?, used, lastFailed }`.
pub fn servers(s: &Session) -> Vec<Value> {
    let mut list: Vec<Value> = Vec::new();
    for (name, tools) in &s.tools {
        let sign_in = tools.iter().all(|tool| SIGN_IN_TOOLS.contains(&tool.as_str()));
        list.push(json!({ "name": name, "state": if sign_in { "sign-in" } else { "connected" }, "tools": tools.len() }));
    }
    for (name, error) in &s.failed {
        if !s.tools.contains_key(name) {
            list.push(json!({ "name": name, "state": "failed", "error": error }));
        }
    }
    for name in &s.pending {
        if !s.tools.contains_key(name) && !s.failed.contains_key(name) {
            list.push(json!({ "name": name, "state": "connecting" }));
        }
    }
    // Without tool search Claude Code records no tool lists, so a server Claude used is the only sign of it.
    for name in &s.used {
        if !list.iter().any(|server| server["name"] == name.as_str()) {
            list.push(json!({ "name": name, "state": "connected", "tools": null }));
        }
    }
    for server in &mut list {
        let name = server["name"].as_str().unwrap_or_default().to_owned();
        server["used"] = s.used.contains(&name).into();
        server["lastFailed"] = (s.last_failed.get(&name) == Some(&true)).into();
    }
    let order = |server: &Value| match server["state"].as_str() {
        Some("failed") => 0,
        Some("connecting") => 1,
        Some("connected") => 2,
        _ => 3,
    };
    let name = |server: &Value| server["name"].as_str().unwrap_or_default().to_lowercase();
    list.sort_by(|a, b| order(a).cmp(&order(b)).then((b["used"] == true).cmp(&(a["used"] == true))).then(name(a).cmp(&name(b))));
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(a: Value) -> Value {
        json!({ "type": "attachment", "attachment": a })
    }
    fn tools(mut extra: Value) -> Value {
        extra["type"] = "deferred_tools_delta".into();
        attachment(extra)
    }
    fn call(id: &str, name: &str) -> Value {
        json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": id, "name": name, "input": {} }] } })
    }
    fn result(id: &str, is_error: bool) -> Value {
        json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "is_error": is_error }] } })
    }
    fn list(s: &Session) -> Vec<String> {
        servers(s).iter().map(|x| format!("{}:{}", x["name"].as_str().unwrap(), x["state"].as_str().unwrap())).collect()
    }

    #[test]
    fn tool_lists_add_and_remove_a_servers_tools_and_the_failed_and_pending_lists_say_every_server_in_that_state() {
        let mut s = session("s");
        ingest(&mut s, &tools(json!({ "addedNames": ["mcp__github__list", "mcp__github__get", "Read"] })), true);
        assert_eq!(list(&s), ["github:connected"]);
        let next = json!({ "removedNames": ["mcp__github__list", "mcp__github__get"], "failedMcpServers": [{ "name": "db", "error": "refused" }], "pendingMcpServers": ["sentry"] });
        ingest(&mut s, &tools(next), true);
        assert_eq!(list(&s), ["db:failed", "sentry:connecting"]);
        ingest(&mut s, &tools(json!({ "failedMcpServers": [], "pendingMcpServers": [] })), true);
        assert!(list(&s).is_empty());
    }

    #[test]
    fn an_entry_from_before_this_run_adds_no_tool_lists_but_its_model_and_skills_still_count() {
        let mut s = session("s");
        ingest(&mut s, &tools(json!({ "addedNames": ["mcp__github__list"] })), false);
        ingest(&mut s, &attachment(json!({ "type": "model", "identity": { "modelId": "claude-opus-5-5[1m]" } })), false);
        ingest(&mut s, &attachment(json!({ "type": "invoked_skills", "skills": [{ "name": "wings-plugin" }] })), false);
        assert!(list(&s).is_empty());
        assert_eq!(s.window, Some(1_000_000.0));
        assert_eq!(s.skills, ["wings-plugin"]);
    }

    #[test]
    fn a_result_says_whether_its_servers_last_call_failed_and_answers_its_call_once() {
        let mut s = session("s");
        ingest(&mut s, &call("a", "mcp__github__list"), true);
        ingest(&mut s, &call("b", "Read"), true);
        ingest(&mut s, &result("a", true), true);
        assert_eq!(s.last_failed.get("github"), Some(&true));
        ingest(&mut s, &call("c", "mcp__github__get"), true);
        ingest(&mut s, &result("c", false), true);
        assert_eq!(s.last_failed.get("github"), Some(&false));
        assert!(s.calls.is_empty());
        assert_eq!(s.used.iter().collect::<Vec<_>>(), ["github"]);
    }

    #[test]
    fn the_context_is_the_newest_replys_input_cached_or_not_and_a_new_run_starts_over() {
        let mut s = session("s");
        ingest(&mut s, &json!({ "type": "assistant", "message": { "usage": { "input_tokens": 10, "cache_creation_input_tokens": 20, "cache_read_input_tokens": 300 } } }), true);
        assert_eq!(s.tokens, Some(330.0));
        start_run(&mut s, 5.0);
        assert_eq!(s.tokens, None);
        assert_eq!((s.started_at, s.id.as_str()), (5.0, "s"));
    }
}
