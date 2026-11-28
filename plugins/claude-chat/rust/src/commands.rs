//! The slash commands the chat answers itself with a control request to Claude Code instead of sending them as a message:
//! `/status` (`get_status`), `/export` (`export_conversation`), `/btw <question>` (`side_question`) and `/plan [task]`
//! (`set_permission_mode` to plan, then the task if there is one). Each goes from the Enter that typed it, and what comes
//! back is shown in a dialog (`status_view.rs`), never added to the conversation. And the slash menu, which lists every
//! command `builtin::all` knows.

use serde_json::{Value, json};

use crate::{
    builtin::{Found, Local},
    session::{Command, Session, request},
};

pub const STATUS: &str = "status-dialog";
pub const EXPORT: &str = "export-dialog";
/// A side question's id is this and the number of the write that asked it, so a late answer to an earlier one is told apart.
pub const SIDE: &str = "side-question-";

/// What a command's dialog holds: nothing until Claude Code answers, then the answer or why it couldn't.
pub type Answer<T> = Option<Result<T, String>>;

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

/// The most earlier exchanges a side question sends back with a follow-up, as VS Code keeps.
pub const SIDE_HISTORY: usize = 20;

#[derive(Debug, PartialEq)]
pub struct Side {
    pub id: String,
    pub question: String,
    pub answer: Answer<Reply>,
    /// The exchanges before this question in the open panel, oldest first, as (question, answer).
    pub history: Vec<(String, String)>,
    /// What is typed in the panel's follow-up field.
    pub follow: String,
}

/// The dialogs of these commands, each open while it is `Some`.
#[derive(Debug, Default)]
pub struct Commands {
    pub status: Option<Answer<Vec<Section>>>,
    pub export: Option<Answer<Export>>,
    pub side: Option<Side>,
    /// The reply the copy dialog holds, as how many replies back from the latest (`screens.rs`).
    pub copy: Option<usize>,
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
    /// The line that asks what the open side panel's follow-up field holds, with the panel's exchanges so far as `history`,
    /// which the question goes on from. None until the last question is answered, and for an empty field.
    pub fn side_follow_up(&mut self, n: u32) -> Option<String> {
        let side = self.cmds.side.as_mut()?;
        let Some(Ok(reply)) = &side.answer else {
            return None;
        };
        let asked = side.follow.trim().to_owned();
        if asked.is_empty() {
            return None;
        }
        let said = reply.text.clone();
        side.history.push((std::mem::replace(&mut side.question, asked.clone()), said));
        let extra = side.history.len().saturating_sub(SIDE_HISTORY);
        side.history.drain(..extra);
        side.follow.clear();
        side.answer = None;
        side.id = format!("{SIDE}{n}");
        let history: Vec<Value> = side.history.iter().map(|(question, response)| json!({ "question": question, "response": response })).collect();
        Some(request(&side.id, json!({ "subtype": "side_question", "question": asked, "history": history })))
    }

    /// The line that makes the command `text` is, from the Enter that sent it, and its dialog opened. None for a draft
    /// that is none of them. `/plan` sets the mode and sends the task in the same write, queued while Claude works as
    /// any message is. `stamp` and `n` are the time and the write's number, as `send` takes them.
    pub fn typed_command(&mut self, text: &str, stamp: u64, n: u32) -> Option<String> {
        let Some(Found::Own(local, rest)) = crate::builtin::recognize(text, &self.commands) else {
            return None;
        };
        Some(match local {
            Local::Status => {
                self.cmds.status = Some(None);
                request(STATUS, json!({ "subtype": "get_status" }))
            }
            Local::Export => {
                self.cmds.export = Some(None);
                request(EXPORT, json!({ "subtype": "export_conversation" }))
            }
            Local::Btw => {
                let id = format!("{SIDE}{n}");
                let line = request(&id, json!({ "subtype": "side_question", "question": rest }));
                self.cmds.side = Some(Side { id, question: rest.to_owned(), answer: None, history: Vec::new(), follow: String::new() });
                line
            }
            // Claude Code 2.1.332 answers a bare /plan in -p with "/plan isn't available in this environment."
            Local::Plan if rest.is_empty() => self.set_mode("plan", n),
            Local::Plan => {
                let mode = self.set_mode("plan", n);
                match self.working {
                    true => mode + &self.queue(rest, Vec::new(), stamp, n),
                    false => mode + &self.send(rest, &[], stamp, n),
                }
            }
            _ => return None,
        })
    }
}

/// The first eight commands that match `typed`, Claude Code's and the chat's own and the ones that run only in a terminal:
/// a name that starts with it goes first, so `/p` offers `/plugins` before the many skills with a `p` inside, then an
/// alias that does (`/reset` finds `/clear`), then a name with it inside. A terminal one is offered for its name or an
/// alias only, after the ones that run here.
pub fn menu(listed: &[Command], typed: &str) -> Vec<Command> {
    let typed = typed.to_lowercase();
    let mut matches: Vec<((u8, bool), Command)> = crate::builtin::all(listed)
        .into_iter()
        .filter_map(|command| {
            let name = command.name.to_lowercase();
            let rank = if name.starts_with(&typed) {
                0
            } else if command.aliases.iter().any(|alias| alias.to_lowercase().starts_with(&typed)) {
                1
            } else if name.contains(&typed) && !command.terminal {
                2
            } else {
                return None;
            };
            Some(((rank, command.terminal), command))
        })
        .collect();
    matches.sort_by_key(|(key, _)| *key);
    matches.truncate(8);
    matches.into_iter().map(|(_, command)| command).collect()
}

/// The `/word` the draft ends in when it comes after a space or a line start, as Claude Code completes a command partway
/// through a message: where the word starts and what follows its `/`. None for the message's own first word (the
/// leading `/` has its menu), and for a word that isn't last or has no `/` in front.
pub fn mid_active(draft: &str) -> Option<(usize, &str)> {
    let start = draft.char_indices().rev().find(|(_, ch)| ch.is_whitespace()).map_or(0, |(at, ch)| at + ch.len_utf8());
    if start == 0 {
        return None;
    }
    draft[start..].strip_prefix('/').map(|query| (start, query))
}

/// The commands a mid-message `/query` completes: names that start with it, or whose part after the last `:` does, so
/// `/deploy` finds `myplugin:deploy-app`. A query with a `/` in it is a path and matches none. An empty query lists every
/// command, which Tab asks for (`all`); without it nothing matches.
pub fn mid_menu(listed: &[Command], query: &str, all: bool) -> Vec<Command> {
    if query.contains('/') || (query.is_empty() && !all) {
        return Vec::new();
    }
    let query = query.to_lowercase();
    let mut matches: Vec<(bool, Command)> = runnable(listed)
        .filter_map(|command| {
            let name = command.name.to_lowercase();
            let full = name.starts_with(&query);
            (full || name.rsplit(':').next().is_some_and(|bare| bare.starts_with(&query))).then_some((!full, command))
        })
        .collect();
    matches.sort_by_key(|(bare, _)| *bare);
    matches.truncate(if query.is_empty() { 40 } else { 8 });
    matches.into_iter().map(|(_, command)| command).collect()
}

/// The commands that run in the chat, Claude Code's and its own: what a `/word` partway through a message completes, as
/// only a command that starts the message runs, and a terminal one runs nowhere here.
pub fn runnable(listed: &[Command]) -> impl Iterator<Item = Command> {
    crate::builtin::all(listed).into_iter().filter(|command| !command.terminal)
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
        let skill = |name: &str| Command { name: name.into(), ..Command::default() };
        let listed: Vec<Command> = (0..10).map(|n| skill(&format!("dev-core:skill-{n}-plan"))).collect();
        let names: Vec<String> = menu(&listed, "P").into_iter().map(|command| command.name).collect();
        assert_eq!(names.len(), 8);
        // Then the ones that run only in a terminal, by name, before a name with the letter inside.
        assert_eq!(names[..7], ["plan", "plugins", "permissions", "passes", "powerup", "privacy-settings", "dev-core:skill-0-plan"], "{names:?}");
    }

    /// Claude Code lists `/clear` with the aliases `/reset` and `/new`: typing one offers `/clear`.
    #[test]
    fn the_menu_finds_a_command_by_its_alias_and_offers_terminal_ones_with_a_mark() {
        let clear = Command { name: "clear".into(), aliases: vec!["reset".into(), "new".into()], builtin: true, ..Command::default() };
        let skill = Command { name: "news-digest".into(), ..Command::default() };
        let names = |typed: &str| menu(&[clear.clone(), skill.clone()], typed).into_iter().map(|command| command.name).collect::<Vec<_>>();
        assert_eq!(names("new"), ["news-digest", "clear"]);
        assert_eq!(names("reset"), ["clear"]);
        let theme = menu(&[], "them");
        assert_eq!((theme[0].name.as_str(), theme[0].terminal), ("theme", true));
        // Partway through a message only what runs here completes.
        assert!(mid_menu(&[], "them", false).is_empty());
    }

    #[test]
    fn a_slash_word_after_a_space_completes_by_prefix_or_bare_name_and_a_path_matches_nothing() {
        let skill = |name: &str| Command { name: name.into(), ..Command::default() };
        let listed = [skill("compact"), skill("myplugin:deploy-app"), skill("commit"), skill("review")];
        assert_eq!(mid_active("run the tests, then /com"), Some((20, "com")));
        assert_eq!(mid_active("see\n/x"), Some((4, "x")));
        assert_eq!((mid_active("/com"), mid_active("run /com "), mid_active("run com")), (None, None, None));
        let names = |query: &str, all: bool| mid_menu(&listed, query, all).into_iter().map(|command| command.name).collect::<Vec<_>>();
        assert_eq!(names("com", false), ["compact", "commit"]);
        assert_eq!(names("deploy", false), ["myplugin:deploy-app"]);
        assert!(names("tmp/notes.md", false).is_empty() && names("", false).is_empty());
        assert!(names("", true).len() >= 4);
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
    fn a_follow_up_sends_the_panels_exchanges_as_history_up_to_twenty() {
        let mut session = Session::default();
        session.typed_command("/btw first?", 1, 1).unwrap();
        assert_eq!(session.side_follow_up(2), None, "nothing to follow while the first is unanswered");
        session.hear(&success("side-question-1", json!({ "response": "one", "synthetic": false })));
        session.cmds.side.as_mut().unwrap().follow = "  and then?  ".into();
        let asked = sent(&session.side_follow_up(2).unwrap());
        assert_eq!(asked[0]["request_id"], "side-question-2");
        assert_eq!(asked[0]["request"], json!({ "subtype": "side_question", "question": "and then?", "history": [{ "question": "first?", "response": "one" }] }));
        let side = session.cmds.side.as_ref().unwrap();
        assert_eq!((side.question.as_str(), side.answer.as_ref(), side.follow.as_str()), ("and then?", None, ""));
        // A late answer to the first is dropped, and the field is empty until typed in.
        session.hear(&success("side-question-1", json!({ "response": "late" })));
        assert_eq!(session.cmds.side.as_ref().unwrap().answer, None);
        for n in 3..30 {
            session.hear(&success(&format!("side-question-{}", n - 1), json!({ "response": format!("a{n}") })));
            session.cmds.side.as_mut().unwrap().follow = format!("q{n}");
            session.side_follow_up(n).unwrap();
        }
        let side = session.cmds.side.as_ref().unwrap();
        assert_eq!(side.history.len(), SIDE_HISTORY);
        assert_eq!(side.history.last().unwrap(), &("q28".to_owned(), "a29".to_owned()));
        // A new /btw starts the panel over.
        session.typed_command("/btw fresh", 1, 40).unwrap();
        assert!(session.cmds.side.as_ref().unwrap().history.is_empty());
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
        let listed = |names: &[&str]| names.iter().map(|name| Command { name: (*name).into(), ..Command::default() }).collect::<Vec<_>>();
        let own = |names: &[&str]| runnable(&listed(names)).map(|command| command.name).skip(names.len()).collect::<Vec<_>>();
        let every = ["status", "export", "btw", "plan", "remote-control", "plugins", "mcp", "usage", "context", "hooks", "memory", "permissions", "skills", "model", "effort", "rewind", "resume", "fork", "branch", "tasks", "workflows", "copy", "bookmarks", "diff", "add-dir", "help", "login", "release-notes", "exit"];
        assert_eq!(own(&["clear"]), every);
        let without: Vec<&str> = every.into_iter().filter(|name| !["plan", "status", "usage", "model"].contains(name)).collect();
        assert_eq!(own(&["plan", "status", "usage", "model"]), without);
    }

    #[test]
    fn a_bare_plan_only_switches_to_plan_mode() {
        let mut session = Session::default();
        let lines = sent(&session.typed_command("/plan", 7, 2).unwrap());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["request"], json!({ "subtype": "set_permission_mode", "mode": "plan" }));
        assert!(session.items.is_empty() && !session.working);
        // A bare /btw asks nothing: the chat says how it is typed.
        assert_eq!(session.typed_command("/btw", 7, 3), None);
    }
}
