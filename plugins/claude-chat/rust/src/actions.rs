//! Two things headless Claude Code can do but the chat had no button for: moving a command that runs to the background
//! (`background_tasks`, which `Ctrl+B` does in a terminal), and passing your message on to a background agent. Claude Code
//! refuses `send_task_message` here, but the main Claude has a SendMessage tool that reaches a running agent by its id, so
//! the message goes to Claude as a normal message of yours that asks it to use that tool.
//!
//! It also holds what the session keeps for these and for MCP servers' questions (`elicitation.rs`). What the buttons do is
//! in `actions_view.rs`.

use std::collections::HashSet;

use serde_json::{Value, json};

use crate::{
    elicitation::Elicitation,
    session::{Item, Session, Task, request},
};

/// The id of the request that moves a command to the background, followed by the call's id.
pub const BACKGROUND: &str = "background-";

/// Claude Code's `task_type` for a subagent that runs in the background.
const AGENT: &str = "local_agent";

#[derive(Debug, Default)]
pub struct Ops {
    /// The commands you asked to move to the background, by the id of their call, until Claude Code has answered.
    pub moving: HashSet<String>,
    /// What MCP servers ask you, oldest first, until you answer or Claude Code withdraws it.
    pub elicitations: Vec<Elicitation>,
}

impl Ops {
    /// Takes in Claude Code's answer `response` to a request of these. False for any other.
    pub fn answered(&mut self, id: &str, response: &Value, items: &mut Vec<Item>) -> bool {
        let Some(call) = id.strip_prefix(BACKGROUND) else {
            return false;
        };
        self.moving.remove(call);
        let failed = match (response["subtype"].as_str(), response["response"]["backgrounded"] == true) {
            (Some("error"), _) => Some(format!("Couldn't move it to the background: {}", response["error"].as_str().unwrap_or("Claude Code didn't say why."))),
            (_, false) => Some("That command isn't running, so there was nothing to move to the background.".to_owned()),
            _ => None,
        };
        items.extend(failed.map(Item::Error));
        true
    }
}

impl Session {
    /// The request that moves the command of the tool call `id` to the background, from your click: Claude Code then goes
    /// on without waiting for it, and lists it with the background tasks. None for a call that isn't a command that runs
    /// now, and while the last click on it waits for its answer.
    pub fn background(&mut self, id: &str) -> Option<String> {
        let running = self.items.iter().any(|item| matches!(item, Item::Tool(tool) if tool.id == id && tool.name == "Bash" && tool.output.is_none()));
        if !running || !self.ops.moving.insert(id.to_owned()) {
            return None;
        }
        Some(request(&format!("{BACKGROUND}{id}"), json!({ "subtype": "background_tasks", "tool_use_id": id })))
    }
}

/// A background agent that still runs, which a message can go to.
pub fn is_running_agent(task: &Task) -> bool {
    task.kind == AGENT && task.ended.is_none()
}

/// The message that has Claude pass `said` on to the running background agent `task`, by the id SendMessage takes. None for
/// an empty message and for a task that isn't a running agent.
pub fn agent_message(session: &Session, task: &str, said: &str) -> Option<String> {
    let task = session.tasks.iter().find(|known| known.id == task && is_running_agent(known))?;
    let said = said.trim();
    if said.is_empty() {
        return None;
    }
    let name = if task.description.is_empty() { "a background agent" } else { task.description.as_str() };
    Some(format!("Use the SendMessage tool to pass this message on, word for word, to the background agent \"{name}\" (to: \"{}\"): {said}", task.id))
}

/// How a message from a subagent's tab starts, which the chat knows it by.
const RELAYED: &str = "Use the SendMessage tool to pass on this message from the user, word for word, to the subagent ";

/// The message that has Claude pass `said` on to the subagent `name` (`to`, its id) from its tab (`subagents.rs`).
pub fn relayed_message(name: &str, to: &str, said: &str) -> String {
    format!("{RELAYED}\"{name}\" (to: \"{to}\"): {said}")
}

/// What the chat shows for a message of `relayed_message`'s, in place of the words that ask Claude to pass it on.
pub fn relayed_note(text: &str) -> Option<String> {
    let rest = text.strip_prefix(RELAYED)?.strip_prefix('"')?;
    let (name, rest) = rest.split_once("\" (to: \"")?;
    let (_, said) = rest.split_once("\"): ")?;
    Some(format!("You messaged {name}: {said}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Tool;

    fn session_running(name: &str, output: Option<(String, bool)>) -> Session {
        let mut session = Session::default();
        session.items.push(Item::Tool(Tool { id: "t1".into(), name: name.into(), summary: String::new(), output, detail: crate::session::Detail::Plain, images: Vec::new() }));
        session
    }

    fn agent(id: &str, description: &str, ended: Option<&str>) -> Task {
        Task { id: id.into(), kind: "local_agent".into(), description: description.into(), call: String::new(), started: 0, ended: ended.map(str::to_owned), ended_at: 0, stopping: false, output: None, error: None }
    }

    #[test]
    fn a_running_command_is_moved_with_background_tasks_for_its_call() {
        let mut session = session_running("Bash", None);
        let sent: Value = serde_json::from_str(session.background("t1").expect("a request").trim()).unwrap();
        assert_eq!(sent["request_id"], "background-t1");
        assert_eq!(sent["request"], json!({ "subtype": "background_tasks", "tool_use_id": "t1" }));
        // The click waits for its answer: a second one sends nothing.
        assert_eq!(session.background("t1"), None);
    }

    #[test]
    fn only_a_command_that_still_runs_moves() {
        assert_eq!(session_running("Bash", Some(("done".into(), false))).background("t1"), None);
        assert_eq!(session_running("Read", None).background("t1"), None);
        assert_eq!(session_running("Bash", None).background("other"), None);
    }

    #[test]
    fn a_refused_or_late_move_says_so_and_a_done_one_is_quiet() {
        let mut ops = Ops::default();
        let mut items = Vec::new();
        let done = json!({ "subtype": "success", "request_id": "background-t1", "response": { "backgrounded": true } });
        assert!(ops.answered("background-t1", &done, &mut items) && items.is_empty());
        let late = json!({ "subtype": "success", "request_id": "background-t1", "response": { "backgrounded": false } });
        assert!(ops.answered("background-t1", &late, &mut items));
        let refused = json!({ "subtype": "error", "request_id": "background-t1", "error": "No such task" });
        assert!(ops.answered("background-t1", &refused, &mut items));
        assert_eq!(
            items,
            [
                Item::Error("That command isn't running, so there was nothing to move to the background.".into()),
                Item::Error("Couldn't move it to the background: No such task".into()),
            ]
        );
        assert!(!ops.answered("task-stop-t1", &done, &mut items));
    }

    #[test]
    fn the_answer_lets_the_command_be_moved_again_only_by_a_new_call() {
        let mut session = session_running("Bash", None);
        session.background("t1");
        let done = json!({ "subtype": "success", "request_id": "background-t1", "response": { "backgrounded": true } });
        session.ops.answered("background-t1", &done, &mut session.items);
        assert!(session.ops.moving.is_empty());
    }

    #[test]
    fn a_message_for_a_running_agent_asks_claude_to_send_it_by_the_agents_id() {
        let mut session = Session::default();
        session.tasks.push(agent("a4443e71dac390806", "Sleep 40 seconds then report done", None));
        let said = agent_message(&session, "a4443e71dac390806", "  also print hello  ").expect("a message");
        assert_eq!(
            said,
            "Use the SendMessage tool to pass this message on, word for word, to the background agent \"Sleep 40 seconds then report done\" (to: \"a4443e71dac390806\"): also print hello"
        );
    }

    #[test]
    fn nothing_is_sent_to_an_ended_agent_a_command_or_nobody() {
        let mut session = Session::default();
        session.tasks.push(agent("a1", "Done one", Some("completed")));
        session.tasks.push(Task { kind: "local_bash".into(), ..agent("b1", "A server", None) });
        session.tasks.push(agent("a2", "Running one", None));
        assert_eq!(agent_message(&session, "a1", "hi"), None);
        assert_eq!(agent_message(&session, "b1", "hi"), None);
        assert_eq!(agent_message(&session, "nobody", "hi"), None);
        assert_eq!(agent_message(&session, "a2", "   "), None);
        assert!(agent_message(&session, "a2", "hi").is_some());
    }
}
