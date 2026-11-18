//! The lines Claude Code prints about the session itself rather than the turn: compacting, a request it retries, the
//! notes it adds, its name for the session, and the conversation starting over. `Session::hear` hands each line here
//! first. Shapes are from Claude Code 2.1.327: `status`, `compact_boundary`, `session_title_changed` and
//! `conversation_reset` as it printed them, the rest as its binary builds them (`api_retry`, `model_fallback`,
//! `notification`, `informational`).

use serde_json::Value;
use wings_plugin::format::tokens;

use crate::session::{Item, Session};

/// The most of a note's text the chat keeps.
const NOTE_KEPT: usize = 400;

/// Takes in `line` when it is one of these, and says whether it was.
pub fn hear(session: &mut Session, line: &Value) -> bool {
    if line["type"] == "conversation_reset" {
        // Without the new id the chat would not know which session it is in.
        if let Some(id) = line["new_conversation_id"].as_str() {
            session.reset(id);
        }
        return true;
    }
    if line["type"] != "system" {
        return false;
    }
    let text = |key: &str| line[key].as_str().map(str::trim).filter(|said| !said.is_empty());
    match line["subtype"].as_str().unwrap_or_default() {
        // `status` is null once what it named is over.
        "status" => session.status = (line["status"] == "compacting").then(|| "Compacting the conversation".to_owned()),
        "compact_boundary" => {
            session.status = None;
            let meta = &line["compact_metadata"];
            session.items.push(Item::Divider(compacted(meta["trigger"].as_str(), meta["pre_tokens"].as_u64(), meta["post_tokens"].as_u64())));
        }
        "api_retry" => {
            let seconds = line["retry_delay_ms"].as_f64().map_or(0, |ms| (ms / 1000.0).ceil() as u64);
            let attempt = line["attempt"].as_u64().unwrap_or(1);
            session.status = Some(match line["max_retries"].as_u64() {
                Some(most) => format!("Retrying in {seconds}s, attempt {attempt} of {most}"),
                None => format!("Retrying in {seconds}s, attempt {attempt}"),
            });
        }
        "api_error" => note(session, line["error"]["formatted"].as_str().map(str::trim).filter(|said| !said.is_empty()).map(str::to_owned)),
        "model_fallback" => {
            let said = text("content").map(str::to_owned).or_else(|| match (text("original_model"), text("fallback_model")) {
                (Some(from), Some(to)) => Some(format!("Switched from {from} to {to}")),
                _ => None,
            });
            note(session, said);
        }
        "notification" => note(session, text("text").map(str::to_owned)),
        "informational" => note(session, text("content").map(str::to_owned)),
        // A summary of what happened while you were away, which can run to a paragraph.
        "away_summary" => {
            if let Some(said) = text("content") {
                session.items.push(Item::Note("While you were away".into(), said.to_owned()));
            }
        }
        "session_title_changed" => session.title = text("title").map(str::to_owned),
        _ => return false,
    }
    true
}

/// A one-line note in the conversation, left out when it repeats the one before it.
fn note(session: &mut Session, said: Option<String>) {
    let Some(said) = said else {
        return;
    };
    let said = if said.len() > NOTE_KEPT { format!("{}…", said.chars().take(NOTE_KEPT).collect::<String>()) } else { said };
    if session.items.last() != Some(&Item::Notice(said.clone())) {
        session.items.push(Item::Notice(said));
    }
}

/// The divider for a compaction: "Compacted · 120k → 18k tokens", with "automatically" when Claude Code did it itself.
fn compacted(trigger: Option<&str>, before: Option<u64>, after: Option<u64>) -> String {
    let what = if trigger == Some("auto") { "Compacted automatically" } else { "Compacted" };
    match (before, after) {
        (Some(before), Some(after)) => format!("{what} · {} → {} tokens", tokens(before as f64), tokens(after as f64)),
        (Some(before), None) => format!("{what} · from {} tokens", tokens(before as f64)),
        _ => what.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A compaction, a retry, a rename and a clear as Claude Code 2.1.327 printed them, with the notes it builds.
    const LINES: &str = include_str!("../tests/notices.jsonl");

    fn heard(lines: &str) -> Session {
        let mut session = Session::default();
        for line in lines.lines().filter(|line| !line.trim().is_empty()) {
            session.hear(&serde_json::from_str(line).unwrap());
        }
        session
    }

    fn lines(from: &str, to: &str) -> String {
        LINES.lines().skip_while(|line| !line.contains(from)).take_while(|line| !line.contains(to)).chain(LINES.lines().filter(|line| line.contains(to)).take(1)).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_compaction_shows_its_status_then_a_divider_with_the_tokens_before_and_after() {
        let mut session = Session::default();
        session.hear(&json!({ "type": "system", "subtype": "status", "status": "compacting", "session_id": "s" }));
        assert_eq!(session.activity(), "Compacting the conversation");
        let session = heard(&lines("compacting", "compact_boundary"));
        assert_eq!(session.status, None);
        assert_eq!(session.items, [Item::Divider("Compacted · 43k → 16k tokens".into())]);
    }

    #[test]
    fn an_automatic_compaction_says_so_and_the_replayed_notice_adds_nothing() {
        let mut session = heard(&lines("compact_boundary", "Compacted "));
        session.hear(&json!({ "type": "system", "subtype": "compact_boundary", "compact_metadata": { "trigger": "auto", "pre_tokens": 900_000, "post_tokens": 18_000 } }));
        assert_eq!(session.items, [Item::Divider("Compacted · 43k → 16k tokens".into()), Item::Divider("Compacted automatically · 900k → 18k tokens".into())]);
    }

    #[test]
    fn a_retry_goes_to_the_status_line_and_the_next_answer_takes_it_away() {
        let mut session = Session::default();
        session.hear(&json!({ "type": "system", "subtype": "api_retry", "attempt": 2, "max_retries": 10, "retry_delay_ms": 4100, "error_status": 529 }));
        assert_eq!(session.activity(), "Retrying in 5s, attempt 2 of 10");
        session.hear(&json!({ "type": "stream_event", "event": { "type": "content_block_start", "content_block": { "type": "text" } } }));
        assert_eq!(session.status, None);
        assert_eq!(session.activity(), "Working");
    }

    #[test]
    fn the_notes_become_short_lines_in_the_conversation() {
        let session = heard(&lines("api_error", "away_summary"));
        assert_eq!(
            session.items,
            [
                Item::Notice("Request timed out".into()),
                Item::Notice("Opus is busy, so this reply comes from Sonnet".into()),
                Item::Notice("Fast mode is off for this account".into()),
                Item::Notice("Reminder: the build is red".into()),
                Item::Note("While you were away".into(), "You asked for a refactor of the parser. It is half done.".into()),
            ]
        );
        // A note that repeats the one before it is shown once.
        let mut again = heard(&lines("notification", "notification"));
        let line = json!({ "type": "system", "subtype": "notification", "key": "fast", "text": "Fast mode is off for this account", "priority": "high" });
        again.hear(&line);
        again.hear(&line);
        assert_eq!(again.items.len(), 1);
    }

    #[test]
    fn a_model_fallback_without_text_names_both_models() {
        let session = heard(r#"{"type":"system","subtype":"model_fallback","original_model":"claude-opus-5-5","fallback_model":"claude-sonnet-5-5","trigger":"overload"}"#);
        assert_eq!(session.items, [Item::Notice("Switched from claude-opus-5-5 to claude-sonnet-5-5".into())]);
    }

    #[test]
    fn the_sessions_name_is_the_one_claude_code_says() {
        let session = heard(&lines("session_title_changed", "session_title_changed"));
        assert_eq!(session.title.as_deref(), Some("Fix the checkout total"));
        assert_eq!(heard(r#"{"type":"system","subtype":"session_title_changed","title":"  "}"#).title, None);
    }

    #[test]
    fn a_clear_empties_the_conversation_and_moves_to_the_new_session() {
        let mut session = Session::default();
        session.send("hello", &[], 1, 1);
        session.hear(&json!({ "type": "system", "subtype": "init", "session_id": "old", "model": "claude-haiku-5-5" }));
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "t1", "name": "TodoWrite", "input": { "todos": [{ "content": "a", "status": "pending" }] } }] } }));
        session.hear(&json!({ "type": "system", "subtype": "session_title_changed", "title": "Hello" }));
        session.hear(&json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": "b1", "task_type": "local_bash", "description": "dev" }] }));
        session.hear(&json!({ "type": "assistant", "parent_tool_use_id": "agent1", "message": { "content": [{ "type": "text", "text": "sub" }] } }));
        assert!(!session.items.is_empty() && !session.todos.is_empty() && !session.tasks.is_empty() && !session.subagents.is_empty() && !session.prompts.is_empty());
        session.hear(&serde_json::from_str(LINES.lines().find(|line| line.contains("conversation_reset")).unwrap()).unwrap());
        assert!(session.items.is_empty() && session.todos.is_empty() && session.tasks.is_empty() && session.subagents.is_empty() && session.prompts.is_empty());
        assert_eq!((session.id.as_deref(), session.title.as_deref(), session.cleared), (Some("11111111-2222-4333-8444-555555555555"), None, true));
        // What Claude Code said about itself stays.
        assert_eq!(session.model.as_deref(), Some("claude-haiku-5-5"));
    }
}
