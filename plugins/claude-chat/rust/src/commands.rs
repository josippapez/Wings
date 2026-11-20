//! The slash commands the chat answers itself with a control request to Claude Code instead of sending them as a message:
//! `/status` (`get_status`), `/export` (`export_conversation`), `/btw <question>` (`side_question`) and `/plan <task>`
//! (`set_permission_mode` to plan, then the task). Each goes from the Enter that typed it, and what comes back is shown in
//! a dialog (`status_view.rs`), never added to the conversation.

use serde_json::{Value, json};

use crate::session::{Command, Session, request};

pub const STATUS: &str = "status-dialog";
pub const EXPORT: &str = "export-dialog";
/// A side question's id is this and the number of the write that asked it, so a late answer to an earlier one is told apart.
pub const SIDE: &str = "side-question-";

/// What a command's dialog holds: nothing until Claude Code answers, then the answer or why it couldn't.
pub type Answer<T> = Option<Result<T, String>>;

#[derive(Debug, PartialEq)]
pub enum Typed<'a> {
    Status,
    Export,
    Btw(&'a str),
    Plan(&'a str),
}

/// The command a draft is: `/status` and `/export` alone, `/btw` and `/plan` with something after them. A bare `/plan`
/// goes to Claude Code, which switches to plan mode by itself.
pub fn parse(text: &str) -> Option<Typed<'_>> {
    let text = text.trim();
    match text {
        "/status" => return Some(Typed::Status),
        "/export" => return Some(Typed::Export),
        _ => {}
    }
    let (name, rest) = text.split_once(char::is_whitespace)?;
    let rest = rest.trim();
    match (name, rest.is_empty()) {
        ("/btw", false) => Some(Typed::Btw(rest)),
        ("/plan", false) => Some(Typed::Plan(rest)),
        _ => None,
    }
}

/// A draft that is a whole command, ready to send: the command list over it would take the Enter.
pub fn is_whole(text: &str) -> bool {
    matches!(parse(text), Some(Typed::Status | Typed::Export))
}

/// One section of the status dialog: a title and its rows, each a label and a value. A row may have no label.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

/// The conversation as text, and the file name Claude Code would give it.
#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    pub text: String,
    pub filename: String,
}

/// The answer to a side question.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub text: String,
}

#[derive(Debug, PartialEq)]
pub struct Side {
    pub id: String,
    pub question: String,
    pub answer: Answer<Reply>,
}

/// The dialogs of these commands, each open while it is `Some`.
#[derive(Debug, Default)]
pub struct Commands {
    pub status: Option<Answer<Vec<Section>>>,
    pub export: Option<Answer<Export>>,
    pub side: Option<Side>,
    /// The Copy button last used, by its key, which says Copied.
    pub copied: Option<String>,
}

impl Commands {
    /// Takes in Claude Code's answer `response` to the request `id`. False when it isn't one of these commands'.
    pub fn answered(&mut self, id: &str, response: &Value) -> bool {
        let answer = match response["subtype"] == "success" {
            true => Ok(&response["response"]),
            false => Err(response["error"].as_str().unwrap_or("Claude Code didn't answer.").to_owned()),
        };
        match id {
            STATUS => self.status = self.status.is_some().then(|| Some(answer.map(sections))),
            EXPORT => self.export = self.export.is_some().then(|| Some(answer.map(export))),
            _ if id.starts_with(SIDE) => {
                if let Some(side) = self.side.as_mut().filter(|side| side.id == id) {
                    side.answer = Some(answer.map(|answer| Reply { text: answer["response"].as_str().unwrap_or_default().to_owned() }));
                }
            }
            _ => return false,
        }
        true
    }
}

fn sections(answer: &Value) -> Vec<Section> {
    let text = |value: &Value, key: &str| value[key].as_str().unwrap_or_default().to_owned();
    answer["sections"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|section| Section {
            title: text(section, "title"),
            rows: section["rows"].as_array().into_iter().flatten().map(|row| (text(row, "label"), text(row, "value"))).collect(),
        })
        .collect()
}

fn export(answer: &Value) -> Export {
    Export { text: answer["text"].as_str().unwrap_or_default().to_owned(), filename: answer["default_filename"].as_str().unwrap_or_default().to_owned() }
}

impl Session {
    /// The line that makes the command `text` is, from the Enter that sent it, and its dialog opened. None for a draft
    /// that is none of them. `/plan` sets the mode and sends the task in the same write, queued while Claude works as
    /// any message is. `stamp` and `n` are the time and the write's number, as `send` takes them.
    pub fn typed_command(&mut self, text: &str, stamp: u64, n: u32) -> Option<String> {
        Some(match parse(text)? {
            Typed::Status => {
                self.cmds.status = Some(None);
                request(STATUS, json!({ "subtype": "get_status" }))
            }
            Typed::Export => {
                self.cmds.export = Some(None);
                request(EXPORT, json!({ "subtype": "export_conversation" }))
            }
            Typed::Btw(question) => {
                let id = format!("{SIDE}{n}");
                let line = request(&id, json!({ "subtype": "side_question", "question": question }));
                self.cmds.side = Some(Side { id, question: question.to_owned(), answer: None });
                line
            }
            Typed::Plan(task) => {
                let mode = self.set_mode("plan", n);
                match self.working {
                    true => mode + &self.queue(task, Vec::new(), stamp, n),
                    false => mode + &self.send(task, &[], stamp, n),
                }
            }
        })
    }
}

/// The commands the chat answers itself that Claude Code doesn't list, for the slash menu to offer: each a name, what it does
/// and what it takes. One Claude Code lists itself stays as it lists it.
/// The first eight commands whose name holds `typed`, Claude Code's and the chat's own. A name that starts with it goes
/// first, so `/p` offers `/plugins` before the many skills with a `p` inside.
pub fn menu(listed: &[Command], typed: &str) -> Vec<Command> {
    let typed = typed.to_lowercase();
    let mut matches: Vec<Command> = listed.iter().cloned().chain(own_commands(listed)).filter(|command| command.name.to_lowercase().contains(&typed)).collect();
    matches.sort_by_key(|command| !command.name.to_lowercase().starts_with(&typed));
    matches.truncate(8);
    matches
}

pub fn own_commands(listed: &[Command]) -> impl Iterator<Item = Command> + '_ {
    [
        ("status", "Show the version, account, model and settings sources", ""),
        ("export", "Export the conversation as text", ""),
        ("btw", "Ask a side question without adding it to the conversation", "<question>"),
        ("plan", "Switch to plan mode, and plan a task", "<task>"),
        ("remote-control", "Continue this chat from claude.ai or the Claude app", "<name>"),
    ]
    .into_iter()
    .filter(move |(name, ..)| listed.iter().all(|command| command.name != *name))
    .map(|(name, description, hint)| Command { name: name.to_owned(), description: description.to_owned(), hint: hint.to_owned() })
    // The views the chat opens itself, from the table in `builtin.rs`.
    .chain(crate::builtin::menu(listed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(line: &str) -> Vec<Value> {
        line.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }

    fn success(id: &str, response: Value) -> Value {
        json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } })
    }

    #[test]
    fn the_menu_puts_the_names_that_start_with_what_was_typed_first() {
        let skill = |name: &str| Command { name: name.into(), description: String::new(), hint: String::new() };
        let listed: Vec<Command> = (0..10).map(|n| skill(&format!("dev-core:skill-{n}-plan"))).collect();
        let names: Vec<String> = menu(&listed, "P").into_iter().map(|command| command.name).collect();
        assert_eq!(names.len(), 8);
        assert_eq!(names[..4], ["plan", "plugins", "permissions", "dev-core:skill-0-plan"], "{names:?}");
    }

    #[test]
    fn the_commands_are_told_by_their_name_and_what_follows_it() {
        assert_eq!(parse(" /status "), Some(Typed::Status));
        assert_eq!(parse("/export"), Some(Typed::Export));
        assert_eq!(parse("/btw  what is a monad? "), Some(Typed::Btw("what is a monad?")));
        assert_eq!(parse("/plan add a dark mode"), Some(Typed::Plan("add a dark mode")));
        // Alone, /plan goes to Claude Code, and /status or /export with words after them are not these.
        assert!([parse("/plan"), parse("/btw"), parse("/status now"), parse("/export file.txt"), parse("plan x")].iter().all(Option::is_none));
        assert!(is_whole("/status") && is_whole("/export") && !is_whole("/btw hi") && !is_whole("/stat"));
    }

    #[test]
    fn status_asks_for_get_status_and_lists_the_sections_it_answers() {
        let mut session = Session::default();
        let asked = sent(&session.typed_command("/status", 1, 1).unwrap());
        assert_eq!((asked.len(), asked[0]["request"]["subtype"].as_str()), (1, Some("get_status")));
        assert_eq!(session.cmds.status, Some(None));
        session.hear(&success(
            STATUS,
            json!({ "sections": [
                { "title": "Session", "rows": [{ "label": "Version", "value": "2.1.327" }, { "label": "cwd", "value": "/work/shop" }] },
                { "title": "Environment", "rows": [{ "label": "Model", "value": "claude-haiku-5-5" }, { "value": "Managed by your organization." }] },
            ] }),
        ));
        let Some(Some(Ok(sections))) = &session.cmds.status else { panic!("no sections: {:?}", session.cmds.status) };
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].rows[0], ("Version".to_owned(), "2.1.327".to_owned()));
        assert_eq!(sections[1].rows[1], (String::new(), "Managed by your organization.".to_owned()));
        // A refused read says why, and an answer after the dialog was closed opens nothing.
        session.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": STATUS, "error": "no" } }));
        assert_eq!(session.cmds.status, Some(Some(Err("no".to_owned()))));
        session.cmds.status = None;
        session.hear(&success(STATUS, json!({ "sections": [] })));
        assert_eq!(session.cmds.status, None);
    }

    #[test]
    fn export_asks_for_the_conversation_and_keeps_its_text_and_file_name() {
        let mut session = Session::default();
        let asked = sent(&session.typed_command("/export", 1, 1).unwrap());
        assert_eq!(asked[0]["request"], json!({ "subtype": "export_conversation" }));
        session.hear(&success(EXPORT, json!({ "text": "User: hi\nClaude: hello\n", "default_filename": "conversation-2026-01-01-000000.txt" })));
        assert_eq!(session.cmds.export, Some(Some(Ok(Export { text: "User: hi\nClaude: hello\n".into(), filename: "conversation-2026-01-01-000000.txt".into() }))));
        // Nothing of it goes into the conversation.
        assert!(session.items.is_empty());
    }

    #[test]
    fn a_side_question_is_answered_apart_from_the_conversation_and_a_late_answer_to_an_old_one_is_dropped() {
        let mut session = Session::default();
        session.working = true;
        let asked = sent(&session.typed_command("/btw what does 2+2 make?", 1, 4).unwrap());
        assert_eq!((asked.len(), asked[0]["request_id"].as_str()), (1, Some("side-question-4")));
        assert_eq!(asked[0]["request"], json!({ "subtype": "side_question", "question": "what does 2+2 make?" }));
        session.hear(&success("side-question-3", json!({ "response": "old", "synthetic": false })));
        assert_eq!(session.cmds.side.as_ref().unwrap().answer, None);
        session.hear(&success("side-question-4", json!({ "response": "Four", "synthetic": false })));
        assert_eq!(session.cmds.side.as_ref().unwrap().answer, Some(Ok(Reply { text: "Four".into() })));
        assert!(session.items.is_empty() && session.working && session.queued.is_empty());
    }

    #[test]
    fn plan_sets_the_mode_then_sends_the_task_in_one_write_and_queues_it_while_claude_works() {
        let mut session = Session::default();
        let lines = sent(&session.typed_command("/plan add a dark mode", 7, 2).unwrap());
        assert_eq!(lines[0]["request"], json!({ "subtype": "set_permission_mode", "mode": "plan" }));
        assert_eq!((lines[1]["type"].as_str(), lines[1]["message"]["content"].as_str()), (Some("user"), Some("add a dark mode")));
        assert_eq!(session.mode.as_deref(), Some("plan"));
        assert_eq!(session.items, [crate::session::Item::User("add a dark mode".into())]);
        assert!(session.working);
        let mut busy = Session::default();
        busy.working = true;
        let lines = sent(&busy.typed_command("/plan and a light one", 8, 3).unwrap());
        assert_eq!(lines[1]["priority"], "next");
        assert_eq!(busy.queued.len(), 1);
    }

    #[test]
    fn the_slash_menu_offers_these_commands_unless_claude_code_lists_them() {
        let listed = |names: &[&str]| names.iter().map(|name| Command { name: (*name).into(), description: String::new(), hint: String::new() }).collect::<Vec<_>>();
        let own: Vec<String> = own_commands(&listed(&["clear"])).map(|command| command.name).collect();
        assert_eq!(own, ["status", "export", "btw", "plan", "remote-control", "plugins", "mcp", "usage", "cost", "context", "hooks", "memory", "permissions"]);
        let own: Vec<String> = own_commands(&listed(&["plan", "status", "usage"])).map(|command| command.name).collect();
        assert_eq!(own, ["export", "btw", "remote-control", "plugins", "mcp", "cost", "context", "hooks", "memory", "permissions"]);
    }
}
