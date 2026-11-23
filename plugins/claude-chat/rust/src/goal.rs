//! `/goal`: a condition Claude Code checks after every turn, and keeps Claude working until it holds. Shapes are from
//! Claude Code 2.1.330 run headless. Setting one prints a message of the model `<synthetic>` whose `local_command_run` is
//! `{ "command": "goal", "args": … }` and whose text is `Goal set: <condition>`; `/goal` with no argument prints
//! `Goal active: <condition> (not yet evaluated)`, `/goal clear` prints `Goal cleared: <condition>` or `No goal set`. Each
//! check that fails comes as a message in your name (`isSynthetic`) with the text `Stop hook feedback:\n[<condition>]:
//! <reason>`. A check that passes prints nothing: the turn just ends with a successful `result`, and the goal is gone
//! (a `/goal` after it says `No goal set`).

use serde_json::Value;

use crate::session::{Item, Session};

/// The goal Claude Code is working to meet.
#[derive(Debug, Clone, PartialEq)]
pub struct Goal {
    pub condition: String,
    /// When it was set (ms), which the chip counts from.
    pub since: u64,
    /// How many checks said it isn't met yet.
    pub checks: u32,
    /// Why the latest check said so.
    pub reason: Option<String>,
    /// Claude has written since the last check, so the turn's end is the next check's answer.
    pub awaiting: bool,
}

/// Takes in `line` when it is about the goal, and says whether it was. A `result` is only watched, so it is never taken.
pub fn hear(session: &mut Session, line: &Value) -> bool {
    match line["type"].as_str().unwrap_or_default() {
        "assistant" => assistant(session, line),
        "user" if line["isSynthetic"] == true => feedback(session, line),
        "result" => {
            let met = session.goal.as_ref().is_some_and(|goal| goal.awaiting) && line["is_error"] != true && line["subtype"] == "success";
            if met {
                end(session, |goal, took| format!("Goal met: {} · {}", goal.condition, after(goal.checks, took)));
            }
            false
        }
        "system" if matches!(line["subtype"].as_str(), Some("informational" | "notification")) => {
            let said = line["content"].as_str().or(line["text"].as_str()).unwrap_or_default();
            failed(session, said)
        }
        _ => false,
    }
}

fn assistant(session: &mut Session, line: &Value) -> bool {
    let text = line["message"]["content"][0]["text"].as_str().unwrap_or_default().trim();
    if line["message"]["model"] != "<synthetic>" {
        if let Some(goal) = &mut session.goal {
            goal.awaiting = true;
        }
        return false;
    }
    if failed(session, text) {
        return true;
    }
    if line["local_command_run"]["command"] != "goal" {
        return false;
    }
    if let Some(condition) = text.strip_prefix("Goal set: ") {
        session.goal = Some(Goal { condition: condition.to_owned(), since: session.now, checks: 0, reason: None, awaiting: false });
    } else if let Some(cleared) = text.strip_prefix("Goal cleared: ") {
        end(session, |_, _| format!("Goal cleared: {cleared}"));
        session.goal = None;
    } else if let Some(active) = text.strip_prefix("Goal active: ") {
        // What a resumed session or a chat that missed the setting learns from the status.
        let condition = active.strip_suffix(" (not yet evaluated)").unwrap_or(active);
        if session.goal.as_ref().is_none_or(|goal| goal.condition != condition) {
            session.goal = Some(Goal { condition: condition.to_owned(), since: session.now, checks: 0, reason: None, awaiting: false });
        }
        session.items.push(Item::Notice(text.to_owned()));
    } else {
        // "No goal set. Usage: `/goal <condition>`", for a goal that was met or never set.
        session.goal = None;
        session.items.push(Item::Notice(text.to_owned()));
    }
    true
}

/// The goal ended in an error: Claude Code says "Goal cleared after an unrecoverable error … Run /goal again to continue".
fn failed(session: &mut Session, text: &str) -> bool {
    let text = text.trim();
    if !text.starts_with("Goal cleared after") {
        return false;
    }
    session.goal = None;
    session.items.push(Item::Notice(text.to_owned()));
    true
}

/// A check that failed: its reason is the goal's, shown in the conversation as that and not as something you typed.
fn feedback(session: &mut Session, line: &Value) -> bool {
    let Some(goal) = &mut session.goal else {
        return false;
    };
    let content = &line["message"]["content"];
    let text = content.as_str().or_else(|| content[0]["text"].as_str()).unwrap_or_default();
    let Some(reason) = text.strip_prefix("Stop hook feedback:\n[").and_then(|rest| rest.strip_prefix(goal.condition.as_str())).and_then(|rest| rest.strip_prefix("]: ")) else {
        return false;
    };
    let reason = reason.trim().to_owned();
    goal.checks += 1;
    goal.awaiting = false;
    goal.reason = Some(reason.clone());
    session.items.push(Item::Note("Goal not met yet".into(), reason));
    true
}

/// Ends the goal with a notice in the conversation.
fn end(session: &mut Session, said: impl FnOnce(&Goal, u64) -> String) {
    if let Some(goal) = session.goal.take() {
        let took = session.now.saturating_sub(goal.since);
        session.items.push(Item::Notice(said(&goal, took)));
    }
}

/// How long, as "21s", "2m 5s", "1h 4m".
fn lasted(ms: u64) -> String {
    let seconds = ms / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

/// "after 1 miss, 2m 5s".
fn after(checks: u32, took: u64) -> String {
    let checks = match checks {
        0 => "on the first check".to_owned(),
        1 => "after 1 miss".to_owned(),
        n => format!("after {n} misses"),
    };
    format!("{checks}, {}", lasted(took))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn said(text: &str, command: &str, args: &str) -> Value {
        json!({ "type": "assistant", "message": { "model": "<synthetic>", "content": [{ "type": "text", "text": text }] },
            "local_command_source": format!("<local-command-stdout>{text}</local-command-stdout>"), "local_command_run": { "command": command, "args": args } })
    }

    fn miss(condition: &str, reason: &str) -> Value {
        json!({ "type": "user", "isSynthetic": true, "message": { "role": "user", "content": [{ "type": "text", "text": format!("Stop hook feedback:\n[{condition}]: {reason}") }] } })
    }

    fn reply() -> Value {
        json!({ "type": "assistant", "message": { "model": "claude-haiku-5-5", "content": [{ "type": "text", "text": "Working" }] } })
    }

    fn ended() -> Value {
        json!({ "type": "result", "subtype": "success", "is_error": false, "result": "x" })
    }

    #[test]
    fn setting_a_goal_holds_it_and_says_nothing_in_the_conversation() {
        let mut session = Session::default();
        session.now = 5000;
        session.hear(&said("Goal set: the build is green", "goal", "the build is green"));
        let goal = session.goal.as_ref().expect("a goal");
        assert_eq!((goal.condition.as_str(), goal.since, goal.checks), ("the build is green", 5000, 0));
        assert!(session.items.is_empty(), "{:?}", session.items);
    }

    #[test]
    fn a_failed_check_is_the_goals_reason_not_a_message_you_typed() {
        let mut session = Session::default();
        session.hear(&said("Goal set: the build is green", "goal", "x"));
        session.hear(&reply());
        session.hear(&miss("the build is green", "Two tests still fail"));
        let goal = session.goal.as_ref().unwrap();
        assert_eq!((goal.checks, goal.reason.as_deref(), goal.awaiting), (1, Some("Two tests still fail"), false));
        assert_eq!(session.items.last(), Some(&Item::Note("Goal not met yet".into(), "Two tests still fail".into())));
        assert_eq!(session.items.len(), 2, "the reply and the reason, and not a message you typed");
        // A stop hook of another kind is not the goal's.
        let mut other = Session::default();
        other.hear(&said("Goal set: the build is green", "goal", "x"));
        other.hear(&miss("lint", "x"));
        assert_eq!(other.goal.as_ref().unwrap().checks, 0);
    }

    #[test]
    fn a_turn_that_ends_after_a_reply_with_no_failed_check_met_the_goal() {
        let mut session = Session::default();
        session.now = 1000;
        session.hear(&said("Goal set: reply DONE", "goal", "x"));
        session.hear(&reply());
        session.hear(&miss("reply DONE", "Not exact"));
        session.hear(&reply());
        session.now = 66_000;
        session.hear(&ended());
        assert_eq!(session.goal, None);
        assert_eq!(session.items.last(), Some(&Item::Notice("Goal met: reply DONE · after 1 miss, 1m 5s".into())));
    }

    #[test]
    fn a_turn_that_ends_in_error_or_with_no_reply_keeps_the_goal() {
        let mut session = Session::default();
        session.hear(&said("Goal set: reply DONE", "goal", "x"));
        session.hear(&ended());
        session.hear(&reply());
        session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true }));
        assert!(session.goal.is_some());
    }

    #[test]
    fn clearing_and_an_unrecoverable_error_end_it_with_their_words() {
        let mut session = Session::default();
        session.hear(&said("Goal set: reply DONE", "goal", "x"));
        session.hear(&said("Goal cleared: reply DONE", "goal", "clear"));
        assert_eq!((session.goal.as_ref(), session.items.last()), (None, Some(&Item::Notice("Goal cleared: reply DONE".into()))));
        session.hear(&said("Goal set: reply DONE", "goal", "x"));
        let error = "Goal cleared after an unrecoverable error (overloaded). Run /goal again to continue.";
        session.hear(&said(error, "goal", ""));
        assert_eq!((session.goal.as_ref(), session.items.last()), (None, Some(&Item::Notice(error.into()))));
    }

    #[test]
    fn the_status_of_a_goal_the_chat_did_not_see_set_brings_it_in() {
        let mut session = Session::default();
        session.hear(&said("Goal active: reply DONE (not yet evaluated)", "goal", ""));
        assert_eq!(session.goal.as_ref().map(|goal| goal.condition.as_str()), Some("reply DONE"));
        session.hear(&said("No goal set. Usage: `/goal <condition>`", "goal", ""));
        assert_eq!(session.goal, None);
    }
}
