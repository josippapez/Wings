//! A message that starts with `!` runs as a shell command, as in Claude Code's terminal. Claude Code's headless input has a
//! `bash_command` message for it: it runs the command in the chat's folder and prints the rows a terminal records for bash
//! mode, `<bash-input>` and then `<bash-stdout>`, `<bash-stderr>` and `<bash-exit-code>`. Those show as one folded command
//! row, and, as in the terminal, no turn starts: Claude reads them with the next message you send, which carries them before
//! its text.
//!
//! Only your own typing runs: `command` reads the message box when you press Enter, and a message another agent puts there
//! never gets here (`chat::take_agent_message`).

use serde_json::{Value, json};

/// The most of the commands' output the next message carries.
const KEPT: usize = 20_000;

/// The command a message names: what follows its `!`, when there is something.
pub fn command(text: &str) -> Option<&str> {
    let command = text.trim().strip_prefix('!')?.trim();
    (!command.is_empty()).then_some(command)
}

/// The line that has Claude Code run `command`.
pub fn line(command: &str) -> String {
    format!("{}\n", json!({ "type": "bash_command", "command": command }))
}

/// What the commands you ran printed, which Claude has not read yet.
#[derive(Debug, Default, PartialEq)]
pub struct Shell {
    context: String,
}

impl Shell {
    /// Takes in one of the rows Claude Code printed for a command (`<bash-input>…`, `<bash-stdout>…`) as it records it.
    /// False for any other text.
    pub fn heard(&mut self, text: &str) -> bool {
        let text = text.trim();
        if !(text.starts_with("<bash-input>") || text.starts_with("<bash-stdout>")) {
            return false;
        }
        if self.context.len() < KEPT {
            if !self.context.is_empty() {
                self.context.push('\n');
            }
            self.context.push_str(text);
        }
        true
    }

    /// What goes before your next message, once: the rows as the terminal records them.
    pub fn take(&mut self) -> String {
        let mut context = std::mem::take(&mut self.context);
        if context.len() > KEPT {
            let end = (0..=KEPT).rev().find(|at| context.is_char_boundary(*at)).unwrap_or(0);
            context.truncate(end);
        }
        context
    }
}

/// A message's `content` with `context` as a block of its own before it, as one array; unchanged when there is none.
pub fn with_context(content: Value, context: &str) -> Value {
    if context.is_empty() {
        return content;
    }
    let mut blocks = vec![json!({ "type": "text", "text": context })];
    match content {
        Value::Array(more) => blocks.extend(more),
        Value::String(text) => blocks.push(json!({ "type": "text", "text": text })),
        other => blocks.push(other),
    }
    Value::Array(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bang_and_a_command_is_a_shell_command_and_nothing_else_is() {
        assert_eq!(command("!echo hi"), Some("echo hi"));
        assert_eq!(command("  ! ls -la \n"), Some("ls -la"));
        assert_eq!(command("!"), None);
        assert_eq!(command("hello !echo"), None);
        assert_eq!(command("/status"), None);
        assert_eq!(line("echo hi"), "{\"command\":\"echo hi\",\"type\":\"bash_command\"}\n");
    }

    #[test]
    fn the_rows_wait_for_your_next_message_which_carries_them_first_once() {
        let mut shell = Shell::default();
        assert!(!shell.heard("hello"));
        assert!(shell.heard("<bash-input>echo hi</bash-input>"));
        assert!(shell.heard("<bash-stdout>hi</bash-stdout><bash-stderr></bash-stderr><bash-exit-code>0</bash-exit-code>"));
        let context = shell.take();
        let content = with_context(json!("what did it print?"), &context);
        assert_eq!(content[0]["text"], "<bash-input>echo hi</bash-input>\n<bash-stdout>hi</bash-stdout><bash-stderr></bash-stderr><bash-exit-code>0</bash-exit-code>");
        assert_eq!(content[1]["text"], "what did it print?");
        assert_eq!(shell.take(), "");
        assert_eq!(with_context(json!("plain"), ""), json!("plain"));
    }

    #[test]
    fn polish_shell_replay_rows_make_one_conversation_item_per_command_and_go_to_claude_once() {
        use crate::session::Session;
        let mut session = Session::default();
        // As Claude Code 2.1.330 prints them for a `bash_command`.
        let row = |text: &str| json!({ "type": "user", "message": { "role": "user", "content": text }, "parent_tool_use_id": null, "uuid": "u", "isReplay": true });
        session.hear(&row("<bash-input>echo hi; pwd</bash-input>"));
        session.hear(&row("<bash-stdout>hi\n/tmp</bash-stdout><bash-stderr></bash-stderr><bash-exit-code>0</bash-exit-code>"));
        assert_eq!(session.items.len(), 1, "{:?}", session.items);
        let sent: Value = serde_json::from_str(session.send("what was that?", &[], 1, 1).lines().next().unwrap()).unwrap();
        let blocks = sent["message"]["content"].as_array().expect("blocks");
        assert!(blocks[0]["text"].as_str().unwrap().starts_with("<bash-input>echo hi; pwd</bash-input>"));
        assert_eq!(blocks[1]["text"], "what was that?");
        // Once: the next one is plain.
        let next: Value = serde_json::from_str(session.send("and now?", &[], 2, 2).lines().next().unwrap()).unwrap();
        assert_eq!(next["message"]["content"], "and now?");
    }

    #[test]
    fn polish_shell_transcript_rows_make_one_conversation_item_per_command() {
        use crate::session::Session;
        let mut session = Session::default();
        let row = |text: &str| json!({ "type": "user", "message": { "role": "user", "content": text }, "parent_tool_use_id": null, "uuid": "u" });
        session.hear(&row("<bash-input>echo first</bash-input>"));
        session.hear(&row("<bash-stdout>first</bash-stdout><bash-stderr></bash-stderr><bash-exit-code>0</bash-exit-code>"));
        session.hear(&row("<bash-input>echo second</bash-input>"));
        session.hear(&row("<bash-stdout>second</bash-stdout><bash-stderr></bash-stderr><bash-exit-code>0</bash-exit-code>"));
        assert_eq!(session.items.len(), 2, "{:?}", session.items);
    }

    #[test]
    fn polish_shell_failure_keeps_stdout_stderr_and_the_nonzero_exit_code() {
        use crate::session::{Item, Session};
        let mut session = Session::default();
        let row = |text: &str| json!({ "type": "user", "message": { "role": "user", "content": text }, "parent_tool_use_id": null, "uuid": "u", "isReplay": true });
        session.hear(&row("<bash-input>sh -c 'echo starting && echo warning >&2 && exit 7'</bash-input>"));
        session.hear(&row("<bash-stdout>starting</bash-stdout><bash-stderr>warning</bash-stderr><bash-exit-code>7</bash-exit-code>"));
        let Some(Item::Shell(shell)) = session.items.first() else { panic!("{:?}", session.items) };
        assert_eq!(shell.summary, "$ sh -c 'echo starting && echo warning >&2 && exit 7'");
        assert_eq!(shell.output, Some(("Exit code 7\nstarting\nwarning".into(), true)));
    }

    #[test]
    fn polish_shell_tags_inside_an_ordinary_message_stay_user_text() {
        use crate::session::{Item, Session};
        let mut session = Session::default();
        let row = json!({ "type": "user", "message": { "role": "user", "content": "I pasted <bash-input>echo hi</bash-input> here" }, "parent_tool_use_id": null, "uuid": "u" });
        session.hear(&row);
        assert!(matches!(&session.items[..], [Item::User(text)] if text == "I pasted <bash-input>echo hi</bash-input> here"), "{:?}", session.items);
    }

    #[cfg(feature = "chat")]
    #[test]
    fn a_shell_command_another_agent_sends_is_refused_and_says_why() {
        let why = crate::chat::take_agent_message("!rm -rf x", true).unwrap_err();
        assert!(why.contains("shell command"), "{why}");
    }
}
