//! A subagent in Wings' right sidebar, or popped out into a tab right after the chat that started it: `subagent_tab.rs`
//! runs either page, and `subagent_relay.rs` is the chat's side of it. This is what both share and needs no page.
//!
//! The sidebar is one page for the whole plugin, so it learns which subagent to show from the chat's message (`show`), not
//! from args, which `openView` allows only for a tab. A page named for its subagent is what the chat feeds, so the
//! sidebar takes the name `subagent-side:<session>:<agent>` once it shows one, and a tab `subagent:<session>:<agent>`.
//!
//! Claude Code 2.1.327 keeps a subagent's transcript beside its session's, in `<session>/subagents/agent-<id>.jsonl`, and
//! knows the subagent by that id: `task_started`'s `task_id`, the `agent_id` on each of its lines and `agentId` in the
//! result of the call that started it. SendMessage's `to` takes the same id. Its lines in the chat's output also carry
//! that call's id as `parent_tool_use_id`, including the ones it writes after SendMessage woke it.
//!
//! The tab reads the transcript as it opens, through Wings, which finds the file inside the session's folder by the two
//! ids. After that the chat passes on each line the subagent prints, which it hears anyway: only the chat's Claude Code
//! runs the subagent, so while the chat is closed there is nothing new to read. The tab's message goes the other way:
//! headless Claude Code refuses `send_task_message`, so the chat's Claude passes it on with SendMessage, and the chat
//! writes it with the write your Enter in the tab handed on (`wings_plugin::pages::tell`).

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use crate::session::{Item, Session, Tool};

/// Claude Code's `task_type` for a subagent.
const AGENT: &str = "local_agent";
/// The most of a tool's output passed on to the tab, as much as the tab keeps of it.
const OUTPUT_PASSED: usize = 4000;
/// A line passed on is one message between pages, which holds 64,000 characters with what goes around it.
const LINE_MOST: usize = 60_000;

/// Claude Code's id for a subagent: letters and digits, as Wings takes it.
pub fn is_agent_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The subagents a session started, by the id of the call that started each.
#[derive(Debug, Default)]
pub struct Agents(HashMap<String, String>);

impl Agents {
    /// Learns what `line`, one Claude Code printed or a transcript entry, says of a subagent's id.
    pub fn learn(&mut self, line: &Value) {
        if line["type"] == "system" && line["subtype"] == "task_started" && line["task_type"] == AGENT {
            // A SendMessage that wakes the agent starts it again under its own call: the first call is the one kept.
            if let Some(agent) = line["task_id"].as_str().filter(|agent| self.call_of(agent).is_none()) {
                self.add(line["tool_use_id"].as_str(), agent);
            }
            return;
        }
        if let Some(agent) = line["agent_id"].as_str() {
            self.add(line["parent_tool_use_id"].as_str(), agent);
        }
        // The result of the call that started it: `tool_use_result` as Claude Code prints it, `toolUseResult` in the
        // transcript.
        let result = if line["tool_use_result"].is_object() { &line["tool_use_result"] } else { &line["toolUseResult"] };
        if let Some(agent) = result["agentId"].as_str() {
            let call = line["message"]["content"].as_array().into_iter().flatten().find_map(|block| block["tool_use_id"].as_str());
            self.add(call, agent);
        }
    }

    fn add(&mut self, call: Option<&str>, agent: &str) {
        if let Some(call) = call.filter(|_| is_agent_id(agent)) {
            self.0.insert(call.to_owned(), agent.to_owned());
        }
    }

    /// The subagent the call `call` started.
    pub fn of_call(&self, call: &str) -> Option<&str> {
        self.0.get(call).map(String::as_str)
    }

    /// The call that started the subagent `agent`.
    pub fn call_of(&self, agent: &str) -> Option<&str> {
        self.0.iter().find(|(_, known)| *known == agent).map(|(call, _)| call.as_str())
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// The name a chat's page takes, so the tabs of its session's subagents reach it.
pub fn chat_page(session: &str) -> String {
    format!("chat:{session}")
}

/// The name a subagent's tab takes.
pub fn tab_page(session: &str, agent: &str) -> String {
    format!("subagent:{session}:{agent}")
}

/// The name the sidebar takes while it shows no subagent, and the start of the names it takes for one.
pub const SIDE_IDLE: &str = "subagent-side";

/// The name the sidebar takes while it shows `agent` of `session`.
pub fn side_page(session: &str, agent: &str) -> String {
    format!("subagent-side:{session}:{agent}")
}

/// The sidebar's name among the names the plugin's pages have, whatever it shows.
pub fn side_name(names: &[String]) -> Option<&str> {
    names.iter().map(String::as_str).find(|name| *name == SIDE_IDLE || name.starts_with("subagent-side:"))
}

/// The pages that show `agent` of `session`, a tab or the sidebar, from the names the plugin's pages have.
pub fn pages_of(names: &[String], session: &str, agent: &str) -> Vec<String> {
    let wanted = [tab_page(session, agent), side_page(session, agent)];
    names.iter().filter(|name| wanted.contains(name)).cloned().collect()
}

/// Each page that shows a subagent of `session` as (its name, the subagent).
pub fn watching(names: &[String], session: &str) -> Vec<(String, String)> {
    let prefixes = [format!("subagent:{session}:"), format!("subagent-side:{session}:")];
    names
        .iter()
        .filter_map(|name| prefixes.iter().find_map(|prefix| name.strip_prefix(prefix.as_str())).map(|agent| (name.clone(), agent.to_owned())))
        .collect()
}

/// What the chat tells the sidebar to show: `agent` of `session`, which `title` says what it is for.
pub fn show_message(session: &str, agent: &str, title: &str) -> Value {
    json!({ "kind": "show", "session": session, "agent": agent, "title": title })
}

/// What the sidebar tells the chat to open as a tab: its subagent, popped out.
pub fn pop_message(agent: &str, title: &str) -> Value {
    json!({ "kind": "pop", "agent": agent, "name": title })
}

/// The call `call` in the session's conversation or in a subagent's.
fn tool<'a>(session: &'a Session, call: &str) -> Option<&'a Tool> {
    let find = |session: &'a Session| {
        session.items.iter().find_map(|item| match item {
            Item::Tool(tool) if tool.id == call => Some(tool),
            _ => None,
        })
    };
    find(session).or_else(|| session.subagents.values().find_map(find))
}

/// What a click on `id` opens: the subagent a call of the session started, or a background agent by its own id, as
/// (agent, what it is for). None until Claude Code has said the agent's id.
pub fn target(session: &Session, id: &str) -> Option<(String, String)> {
    if let Some(agent) = session.agents.of_call(id) {
        let title = tool(session, id).map(|tool| tool.summary.clone()).unwrap_or_default();
        return Some((agent.to_owned(), title));
    }
    let task = session.tasks.iter().find(|task| task.id == id && task.kind == AGENT && is_agent_id(&task.id))?;
    Some((task.id.clone(), task.description.clone()))
}

/// What the tab of `agent` opens with. The same for each click on it, so Wings shows the tab already open.
pub fn tab_args(session: &str, agent: &str, title: &str) -> Value {
    json!({ "subagent": { "session": session, "agent": agent, "title": title } })
}

/// What the chat tells the tab of `agent` of it: whether it runs and since when, or how long it took, its model, the
/// chat's name, and whether the chat can pass a message on now.
pub fn state_of(session: &Session, agent: &str, chat: Option<&str>, ready: bool) -> Value {
    let call = session.agents.call_of(agent);
    let sub = call.and_then(|call| session.subagents.get(call));
    // A background agent, or one SendMessage woke, is a task while it runs; one in the foreground is its call.
    let task = session.tasks.iter().find(|task| task.id == agent);
    let running = match task {
        Some(task) => task.ended.is_none(),
        None => call.and_then(|call| tool(session, call)).is_some_and(|tool| tool.output.is_none()),
    };
    let since = sub.map(|sub| sub.started).filter(|started| *started > 0).or(task.map(|task| task.started));
    let took = sub.filter(|sub| !running && sub.started > 0).map(|sub| sub.last.saturating_sub(sub.started));
    let model = sub.and_then(|sub| sub.reported_model.clone());
    json!({ "kind": "state", "chat": chat, "ready": ready, "running": running, "since": since, "took": took, "model": model })
}

/// The line `line` of a subagent as its tab takes it: as one of its own, with no call around it, and with what it gave
/// back cut to what the tab keeps. None for a line that isn't a subagent's, and a `reload` for one too long to pass on,
/// so the tab reads its transcript again.
pub fn passed_line(line: &Value) -> Option<(String, Value)> {
    let agent = line["agent_id"].as_str().filter(|agent| is_agent_id(agent))?;
    line["parent_tool_use_id"].as_str()?;
    let mut line = line.clone();
    line["parent_tool_use_id"] = Value::Null;
    if let Some(line) = line.as_object_mut() {
        line.remove("tool_use_result");
    }
    if let Some(content) = line.pointer_mut("/message/content").and_then(Value::as_array_mut) {
        // Pictures stay out: the tab sees them when it reads the transcript, which Wings keeps them for.
        content.retain(|block| block["type"] != "image");
        for block in content.iter_mut().filter(|block| block["type"] == "tool_result") {
            block["content"] = Value::String(cut(&result_text(&block["content"]), OUTPUT_PASSED));
        }
    }
    let message = json!({ "kind": "line", "line": line });
    let message = if message.to_string().len() > LINE_MOST { json!({ "kind": "reload" }) } else { message };
    Some((agent.to_owned(), message))
}

/// The text of a tool's result, which is a string or a list of parts.
fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts.iter().filter_map(|part| part["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn cut(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

/// The message from the tab that has the chat pass `text` on to `agent`.
pub fn send_message(agent: &str, title: &str, text: &str) -> Value {
    json!({ "kind": "send", "agent": agent, "name": title, "text": text })
}

/// The most of a message passed on to a subagent.
const SAID_MOST: usize = 16_000;

/// The chat's side of a `send` message from a tab: the line that has its Claude pass the text on to the agent with
/// SendMessage, sent at once or queued while Claude works, and the note the chat shows for it. Only with `write`, the
/// click or key in the tab that Wings handed on with it; None for anything else.
pub fn relay_write(session: &mut Session, message: &Value, write: bool, stamp: u64, n: u32) -> Option<String> {
    if !write || message["kind"] != "send" {
        return None;
    }
    let agent = message["agent"].as_str().filter(|agent| is_agent_id(agent))?;
    let said = message["text"].as_str().map(str::trim).filter(|said| !said.is_empty() && said.len() <= SAID_MOST)?;
    let name = message["name"].as_str().map(str::trim).filter(|name| !name.is_empty()).unwrap_or("a subagent");
    let text = crate::actions::relayed_message(name, agent, said);
    Some(if session.working { session.queue(&text, Vec::new(), stamp, n) } else { session.send(&text, &[], stamp, n) })
}

/// A transcript entry of what SendMessage told the subagent, which Claude Code writes as a note in the subagent's name,
/// as the message it passed on.
fn passed_on(entry: &Value) -> Option<Value> {
    if entry["type"] != "user" || entry["origin"]["kind"] != "coordinator" {
        return None;
    }
    let text = entry["message"]["content"].as_str()?;
    let said = text.split_once('\n').map_or(text, |(_, rest)| rest);
    let said = said.rsplit_once("\n\n").map_or(said, |(said, _)| said).trim();
    let mut shown = Map::new();
    shown.insert("type".into(), "user".into());
    shown.insert("uuid".into(), entry["uuid"].clone());
    shown.insert("message".into(), json!({ "role": "user", "content": said }));
    Some(Value::Object(shown))
}

/// What the tab has heard of the subagent's conversation: its transcript as the tab opens, then the lines the chat
/// passes on, each once and in order.
#[derive(Debug, Default)]
pub struct Heard {
    /// The transcript has been read. Lines passed on before that wait, as they are newer than what it holds.
    pub loaded: bool,
    waiting: Vec<Value>,
    uuids: HashSet<String>,
}

impl Heard {
    /// The transcript's entries, oldest first, then the lines that waited for them.
    pub fn load(&mut self, entries: &[Value], session: &mut Session) {
        for entry in entries {
            self.hear(entry, session);
        }
        self.loaded = true;
        for line in std::mem::take(&mut self.waiting) {
            self.hear(&line, session);
        }
    }

    /// A line the chat passed on.
    pub fn line(&mut self, line: Value, session: &mut Session) {
        if self.loaded {
            self.hear(&line, session);
        } else {
            self.waiting.push(line);
        }
    }

    fn hear(&mut self, entry: &Value, session: &mut Session) {
        if let Some(uuid) = entry["uuid"].as_str()
            && !self.uuids.insert(uuid.to_owned())
        {
            return;
        }
        match passed_on(entry) {
            Some(said) => session.hear(&said),
            None => session.hear(entry),
        }
    }
}

/// Why the tab's message box can't send now: none when it can, and `Some(None)` while the tab hasn't heard yet whether the
/// chat is open (`chat_open`) and whether its Claude Code runs (`ready`, which the chat says once it sees the tab).
pub fn closed_because(chat_open: Option<bool>, ready: Option<bool>) -> Option<Option<&'static str>> {
    match (chat_open, ready) {
        (None, _) | (Some(true), None) => Some(None),
        (Some(false), _) => Some(Some("The chat this subagent belongs to is closed. Open it again to message the subagent.")),
        (Some(true), Some(false)) => Some(Some("Claude Code isn't running in the chat this subagent belongs to, so nothing can pass your message on.")),
        (Some(true), Some(true)) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines of a chat that ran one subagent, as `claude -p --output-format stream-json` 2.1.327 printed them, cut down
    /// and with their ids made up.
    fn ran_one() -> Vec<Value> {
        [
            json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": "toolu_1", "name": "Agent", "input": { "description": "count lines", "subagent_type": "general-purpose", "prompt": "Count the lines of a.txt" } }] } }),
            json!({ "type": "system", "subtype": "task_started", "task_id": "a1b2c3d4e5f60718a", "tool_use_id": "toolu_1", "description": "count lines", "task_type": "local_agent", "is_backgrounded": false }),
            json!({ "type": "user", "parent_tool_use_id": "toolu_1", "agent_id": "a1b2c3d4e5f60718a", "uuid": "u1", "message": { "role": "user", "content": [{ "type": "text", "text": "Count the lines of a.txt" }] } }),
            json!({ "type": "assistant", "parent_tool_use_id": "toolu_1", "agent_id": "a1b2c3d4e5f60718a", "uuid": "u2", "message": { "model": "claude-haiku-5-5", "role": "assistant", "content": [{ "type": "tool_use", "id": "toolu_2", "name": "Read", "input": { "file_path": "/repo/a.txt" } }] } }),
            json!({ "type": "user", "parent_tool_use_id": "toolu_1", "agent_id": "a1b2c3d4e5f60718a", "uuid": "u3", "message": { "role": "user", "content": [{ "tool_use_id": "toolu_2", "type": "tool_result", "content": "1\thello\n2\tworld\n" }] } }),
        ]
        .into()
    }

    #[test]
    fn a_subagents_id_is_learned_from_its_start_its_lines_and_its_calls_result() {
        let mut agents = Agents::default();
        for line in ran_one() {
            agents.learn(&line);
        }
        assert_eq!(agents.of_call("toolu_1"), Some("a1b2c3d4e5f60718a"));
        assert_eq!(agents.call_of("a1b2c3d4e5f60718a"), Some("toolu_1"));
        // SendMessage starts it again under its own call; the call that started it stays its call.
        agents.learn(&json!({ "type": "system", "subtype": "task_started", "task_id": "a1b2c3d4e5f60718a", "tool_use_id": "toolu_9", "task_type": "local_agent" }));
        assert_eq!(agents.call_of("a1b2c3d4e5f60718a"), Some("toolu_1"));
        // A resumed session knows it only from the transcript's result of the call.
        let mut resumed = Agents::default();
        resumed.learn(&json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "toolu_5", "content": "done" }] }, "toolUseResult": { "status": "completed", "agentId": "a9f8e7d6c5b4a3921" } }));
        assert_eq!(resumed.of_call("toolu_5"), Some("a9f8e7d6c5b4a3921"));
        // An id that isn't Claude Code's shape isn't kept: it would become a file name and a page name.
        resumed.learn(&json!({ "type": "system", "subtype": "task_started", "task_id": "../x", "tool_use_id": "toolu_6", "task_type": "local_agent" }));
        assert_eq!(resumed.of_call("toolu_6"), None);
    }

    #[test]
    fn a_click_on_a_subagent_or_a_background_agent_opens_the_same_tab_each_time() {
        let mut session = Session::default();
        for line in ran_one() {
            session.hear(&line);
        }
        let (agent, title) = target(&session, "toolu_1").expect("the call's subagent");
        assert_eq!((agent.as_str(), title.as_str()), ("a1b2c3d4e5f60718a", "count lines"));
        let args = tab_args("s1", &agent, &title);
        assert_eq!(args, json!({ "subagent": { "session": "s1", "agent": "a1b2c3d4e5f60718a", "title": "count lines" } }));
        assert_eq!(tab_args("s1", &agent, &title), args, "equal args, so Wings shows the open tab");
        // A background agent's row names it by its own id.
        session.hear(&json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": "a9f8e7d6c5b4a3921", "task_type": "local_agent", "description": "bg count" }] }));
        assert_eq!(target(&session, "a9f8e7d6c5b4a3921"), Some(("a9f8e7d6c5b4a3921".into(), "bg count".into())));
        assert_eq!(target(&session, "toolu_unknown"), None);
    }

    #[test]
    fn only_a_subagents_lines_are_passed_on_as_its_own_with_long_output_cut() {
        let lines = ran_one();
        assert_eq!(passed_line(&lines[0]), None);
        assert_eq!(passed_line(&lines[1]), None);
        let (agent, message) = passed_line(&lines[3]).expect("the subagent's line");
        assert_eq!((agent.as_str(), message["kind"].as_str()), ("a1b2c3d4e5f60718a", Some("line")));
        assert_eq!(message["line"]["parent_tool_use_id"], Value::Null);
        let mut long = lines[4].clone();
        long["message"]["content"][0]["content"] = json!([{ "type": "text", "text": "x".repeat(10_000) }, { "type": "image", "source": { "type": "base64", "data": "AAAA" } }]);
        long["tool_use_result"] = json!({ "file": { "content": "x".repeat(10_000) } });
        let (_, message) = passed_line(&long).unwrap();
        let output = message["line"]["message"]["content"][0]["content"].as_str().unwrap();
        assert_eq!(output.chars().count(), OUTPUT_PASSED + 1);
        assert_eq!(message["line"]["tool_use_result"], Value::Null);
        // Too long even so: the tab reads its transcript again.
        let mut huge = lines[3].clone();
        huge["message"]["content"] = json!([{ "type": "text", "text": "y".repeat(70_000) }]);
        assert_eq!(passed_line(&huge).unwrap().1, json!({ "kind": "reload" }));
    }

    /// A subagent's transcript as Claude Code 2.1.327 wrote it, cut down and with its ids made up: the prompt, the
    /// attachments it read first, a tool call and its result, the answer, then a message SendMessage passed on and the
    /// answer to that.
    fn transcript() -> Vec<Value> {
        [
            json!({ "parentUuid": null, "isSidechain": true, "agentId": "a5d4c3b2a1f0e9d8c", "type": "user", "uuid": "t1", "message": { "role": "user", "content": "Reply with the single word ready." } }),
            json!({ "isSidechain": true, "agentId": "a5d4c3b2a1f0e9d8c", "type": "attachment", "uuid": "t2", "attachment": { "type": "date", "date": "2026-11-18" } }),
            json!({ "isSidechain": true, "agentId": "a5d4c3b2a1f0e9d8c", "type": "assistant", "uuid": "t3", "message": { "model": "claude-haiku-5-5", "role": "assistant", "content": [{ "type": "tool_use", "id": "toolu_r", "name": "Read", "input": { "file_path": "/repo/a.txt" } }] } }),
            json!({ "isSidechain": true, "agentId": "a5d4c3b2a1f0e9d8c", "type": "user", "uuid": "t4", "message": { "role": "user", "content": [{ "tool_use_id": "toolu_r", "type": "tool_result", "content": "1\tready\n" }] } }),
            json!({ "isSidechain": true, "agentId": "a5d4c3b2a1f0e9d8c", "type": "assistant", "uuid": "t5", "message": { "role": "assistant", "content": [{ "type": "text", "text": "ready" }] } }),
            json!({ "isSidechain": true, "agentId": "a5d4c3b2a1f0e9d8c", "type": "user", "uuid": "t6", "isMeta": true, "origin": { "kind": "coordinator" }, "message": { "role": "user", "content": "The coordinator sent a message while you were working:\nNow reply with the single word hello.\n\nAddress this before completing your current task." } }),
            json!({ "isSidechain": true, "agentId": "a5d4c3b2a1f0e9d8c", "type": "assistant", "uuid": "t7", "message": { "role": "assistant", "content": [{ "type": "text", "text": "hello" }] } }),
        ]
        .into()
    }

    #[test]
    fn the_tab_shows_a_subagents_transcript_with_the_messages_passed_on_to_it() {
        let (mut session, mut heard) = (Session::default(), Heard::default());
        // A line the chat passed on before the transcript was read waits for it, and one it already holds shows once.
        heard.line(json!({ "type": "assistant", "uuid": "t7", "message": { "role": "assistant", "content": [{ "type": "text", "text": "hello" }] } }), &mut session);
        heard.line(json!({ "type": "assistant", "uuid": "t8", "message": { "role": "assistant", "content": [{ "type": "text", "text": "after" }] } }), &mut session);
        assert!(session.items.is_empty());
        heard.load(&transcript(), &mut session);
        let shown: Vec<String> = session
            .items
            .iter()
            .map(|item| match item {
                Item::User(said) => format!("user: {said}"),
                Item::Text(said) => format!("text: {said}"),
                Item::Tool(tool) => format!("tool: {} {}", tool.name, tool.output.as_ref().map_or("", |(output, _)| output.as_str())),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            shown,
            [
                "user: Reply with the single word ready.",
                "tool: Read 1\tready\n",
                "text: ready",
                "user: Now reply with the single word hello.",
                "text: hello",
                "text: after",
            ]
        );
        assert_eq!(session.reported_model.as_deref(), Some("claude-haiku-5-5"));
    }

    #[test]
    fn the_chat_tells_the_tab_whether_its_subagent_runs_and_how_long_it_took() {
        let mut session = Session::default();
        let lines = ran_one();
        for (at, line) in lines.iter().enumerate() {
            session.now = 1_000 + at as u64 * 1_000;
            session.hear(line);
        }
        let running = state_of(&session, "a1b2c3d4e5f60718a", Some("Fix the build"), true);
        assert_eq!(running, json!({ "kind": "state", "chat": "Fix the build", "ready": true, "running": true, "since": 3_000, "took": null, "model": "claude-haiku-5-5" }));
        session.now = 9_000;
        session.hear(&json!({ "type": "user", "parent_tool_use_id": null, "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "2" }] } }));
        let done = state_of(&session, "a1b2c3d4e5f60718a", None, false);
        assert_eq!((done["running"].clone(), done["took"].clone()), (json!(false), json!(2_000)));
        // Woken by SendMessage, it is a background task that runs again.
        session.hear(&json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": "a1b2c3d4e5f60718a", "task_type": "local_agent", "description": "count lines" }] }));
        assert_eq!(state_of(&session, "a1b2c3d4e5f60718a", None, true)["running"], true);
    }

    #[test]
    fn a_message_from_the_tab_has_the_chats_claude_pass_it_on_and_the_chat_notes_it() {
        let mut session = Session::default();
        let message = send_message("a1b2c3d4e5f60718a", "count lines", "  also count the words  ");
        // Without the click handed on with it, nothing is written.
        assert_eq!(relay_write(&mut session, &message, false, 1_800_000_000_000, 1), None);
        assert!(session.items.is_empty());
        let line = relay_write(&mut session, &message, true, 1_800_000_000_000, 1).expect("one line for Claude Code");
        assert_eq!(line.lines().filter(|line| line.contains("SendMessage")).count(), 1);
        let sent: Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
        assert_eq!(
            sent["message"]["content"],
            "Use the SendMessage tool to pass on this message from the user, word for word, to the subagent \"count lines\" (to: \"a1b2c3d4e5f60718a\"): also count the words"
        );
        assert_eq!(session.items, [Item::Notice("You messaged count lines: also count the words".into())]);
        // Read back from the transcript, it is the same note.
        let mut resumed = Session::default();
        resumed.hear(&json!({ "type": "user", "uuid": "x", "message": { "role": "user", "content": sent["message"]["content"] } }));
        assert_eq!(resumed.items, [Item::Notice("You messaged count lines: also count the words".into())]);
        // While Claude works it waits in the queue, as a message of yours does.
        session.working = true;
        relay_write(&mut session, &message, true, 1_800_000_000_000, 2).unwrap();
        assert_eq!(session.queued.len(), 1);
        // Nothing for an empty message, an id that isn't one, or another kind of message.
        assert_eq!(relay_write(&mut session, &send_message("a1b2c3d4e5f60718a", "x", "   "), true, 0, 3), None);
        assert_eq!(relay_write(&mut session, &send_message("../x", "x", "hi"), true, 0, 4), None);
        assert_eq!(relay_write(&mut session, &json!({ "kind": "line" }), true, 0, 5), None);
    }

    #[test]
    fn the_message_box_says_why_it_cant_send_while_the_chat_is_closed_or_not_running() {
        let why = |chat_open, ready| closed_because(chat_open, ready).flatten().unwrap_or_default();
        assert!(why(Some(false), None).contains("is closed"));
        assert!(why(Some(false), Some(true)).contains("is closed"), "a chat that closed since it said it was ready");
        assert!(why(Some(true), Some(false)).contains("isn't running"));
        assert_eq!(closed_because(Some(true), Some(true)), None);
        // Before the tab has heard, it can't send, and has nothing to say yet.
        assert_eq!(closed_because(None, None), Some(None));
        assert_eq!(closed_because(Some(true), None), Some(None));
    }

    #[test]
    fn the_pages_showing_a_sessions_subagents_are_known_by_their_names() {
        let names = [chat_page("s1"), tab_page("s1", "a1"), tab_page("s2", "a2"), side_page("s1", "a3"), side_page("s2", "a4")].map(String::from);
        let watching = watching(&names, "s1");
        assert_eq!(watching, [("subagent:s1:a1".to_owned(), "a1".to_owned()), ("subagent-side:s1:a3".to_owned(), "a3".to_owned())]);
        // The same subagent in a tab and in the sidebar is fed in both.
        let both = [tab_page("s1", "a1"), side_page("s1", "a1"), side_page("s1", "a2")].map(String::from);
        assert_eq!(pages_of(&both, "s1", "a1"), ["subagent:s1:a1", "subagent-side:s1:a1"]);
        assert_eq!(side_name(&both), Some("subagent-side:s1:a1"));
        assert_eq!(side_name(&[SIDE_IDLE.to_owned()]), Some(SIDE_IDLE));
        assert_eq!(side_name(&[chat_page("s1"), tab_page("s1", "a1")]), None);
    }

    #[test]
    fn the_sidebar_is_told_which_subagent_to_show_and_asks_for_it_as_a_tab() {
        assert_eq!(show_message("s1", "a1", "count"), json!({ "kind": "show", "session": "s1", "agent": "a1", "title": "count" }));
        assert_eq!(pop_message("a1", "count"), json!({ "kind": "pop", "agent": "a1", "name": "count" }));
    }
}
