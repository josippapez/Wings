//! The lines Claude Code prints about the session itself rather than the turn: compacting, a request it retries, the
//! notes it adds, its name for the session, and the conversation starting over. `Session::hear` hands each line here
//! first. Shapes are from Claude Code 2.1.327: `status`, `compact_boundary`, `session_title_changed` and
//! `conversation_reset` as it printed them, the rest as its binary builds them (`api_retry`, `model_fallback`,
//! `notification`, `informational`). The rest are from the 2.1.328 schema (`permission_denied`, `hook_started`,
//! `hook_progress`, `hook_response`, `stop_hook_summary`, `memory_saved`, `instruction_size_warning`,
//! `file_attachments_missing`, `session_writes_lost`, `agents_killed`). Hook lines only come with `--include-hook-events`
//! (the chat passes it); without it Claude Code prints them for SessionStart and Setup hooks alone.

use serde_json::Value;
use wings_plugin::format::tokens;

use crate::session::{Item, Session};

/// The most of a note's text the chat keeps.
const NOTE_KEPT: usize = 400;

/// The most of a hook's output the chat keeps.
const HOOK_KEPT: usize = 200;

/// What `per_turn_effort_changed` means for you.
const EFFORT_NOTE: &str = "Effort is no longer sent with each message, so changing it now restarts the prompt cache";

/// Takes in `line` when it is one of these, and says whether it was.
pub fn hear(session: &mut Session, line: &Value) -> bool {
    if line["type"] == "conversation_reset" {
        // Without the new id the chat would not know which session it is in.
        if let Some(id) = line["new_conversation_id"].as_str() {
            session.reset(id);
        }
        return true;
    }
    if crate::goal::hear(session, line) {
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
        // The model ended the stream with a refusal: the turn runs again on another model, or it does not run again.
        "model_refusal_fallback" => note(
            session,
            Some(match (text("original_model"), text("fallback_model")) {
                (Some(from), Some(to)) => format!("{from} declined this request, so Claude retried it on {to}"),
                _ => "The model declined this request, so Claude retried it on another model".to_owned(),
            }),
        ),
        "model_refusal_no_fallback" => note(
            session,
            Some(match text("original_model") {
                Some(from) => format!("{from} declined this request and no other model was tried"),
                None => "The model declined this request and no other model was tried".to_owned(),
            }),
        ),
        // Skills and MCP prompts that load mid-chat: the whole list as it is now.
        "commands_changed" if line["commands"].is_array() => session.commands = crate::session::commands(&line["commands"]),
        "notification" => note(session, text("text").map(str::to_owned)),
        "informational" => note(session, text("content").map(str::to_owned)),
        // A summary of what happened while you were away, which can run to a paragraph.
        "away_summary" => {
            if let Some(said) = text("content") {
                session.items.push(Item::Note("While you were away".into(), said.to_owned()));
            }
        }
        "session_title_changed" => session.title = text("title").map(str::to_owned),
        // The server refused effort sent with each turn. It can repeat a change the chat already holds.
        "per_turn_effort_changed" => {
            if !std::mem::replace(&mut session.effort_rewrites_cache, true) {
                note(session, Some(EFFORT_NOTE.to_owned()));
            }
        }
        // Auto mode, `dontAsk` or a deny rule refused a call: `tool_name`, `tool_use_id`, `decision_reason_type` (such as
        // "classifier"), `decision_reason` (for people, optional) and `message` (what Claude was told).
        "permission_denied" => {
            let tool = text("tool_name").unwrap_or("A tool");
            let reason = text("decision_reason").or_else(|| text("message")).map(|said| cut(said, HOOK_KEPT));
            let who = if text("decision_reason_type") == Some("classifier") { format!("Auto mode blocked {tool}") } else { format!("{tool} was denied") };
            let said = match reason {
                Some(reason) => format!("{who}: {reason}"),
                None => who,
            };
            // Under the tool's row, where the call was.
            let at = text("tool_use_id").and_then(|id| session.items.iter().position(|item| matches!(item, Item::Tool(tool) if tool.id == id)));
            if let (Some(at), Some("classifier")) = (at, text("decision_reason_type")) {
                if let Item::Tool(tool) = &session.items[at] {
                    let blocked = crate::blocked::of(&said, tool, session.now);
                    crate::blocked::keep(session, blocked);
                }
            }
            match at {
                Some(at) if session.items.get(at + 1) != Some(&Item::Notice(said.clone())) => session.items.insert(at + 1, Item::Notice(said)),
                Some(_) => {}
                None => note(session, Some(said)),
            }
        }
        // A hook starts: `hook_id`, `hook_name`, `hook_event`. `hook_progress` repeats its output as it runs and says
        // nothing the end does not.
        "hook_started" => session.status = Some(format!("Running {} hook", text("hook_name").or_else(|| text("hook_event")).unwrap_or("a"))),
        "hook_progress" => {}
        // A hook ends: `outcome` ("success", "error", "cancelled"), `exit_code`, `output`, `stdout`, `stderr`.
        "hook_response" => {
            if session.status.as_deref().is_some_and(|status| status.starts_with("Running ") && status.ends_with(" hook")) {
                session.status = None;
            }
            // A stop hook that fails is in the summary that follows it.
            let stop = matches!(text("hook_event"), Some("Stop" | "SubagentStop"));
            if text("outcome") == Some("error") && !stop {
                let name = text("hook_name").or_else(|| text("hook_event")).unwrap_or("A");
                let output = text("stderr").or_else(|| text("output")).or_else(|| text("stdout"));
                note(session, Some(match (output, line["exit_code"].as_i64()) {
                    (Some(output), _) => format!("{name} hook failed: {}", cut(output, HOOK_KEPT)),
                    (None, Some(code)) => format!("{name} hook failed with exit code {code}"),
                    (None, None) => format!("{name} hook failed"),
                }));
            }
        }
        // Stop hooks at the end of a turn: `hook_count`, `hook_errors`, `prevented_continuation`, `stop_reason`,
        // `has_output`. One line, and only when a hook stopped Claude or failed.
        "stop_hook_summary" => {
            let errors: Vec<&str> = line["hook_errors"].as_array().map(|all| all.iter().filter_map(Value::as_str).map(str::trim).filter(|said| !said.is_empty()).collect()).unwrap_or_default();
            if line["prevented_continuation"] == true {
                note(session, Some(match text("stop_reason").or(errors.first().copied()) {
                    Some(reason) => format!("A stop hook kept Claude from finishing: {}", cut(reason, HOOK_KEPT)),
                    None => "A stop hook kept Claude from finishing".to_owned(),
                }));
            } else if let Some(first) = errors.first() {
                note(session, Some(format!("A stop hook failed: {}", cut(first, HOOK_KEPT))));
            }
        }
        // `written_paths`, and `verb` ("Saved" when absent).
        "memory_saved" => {
            let count = line["written_paths"].as_array().map_or(0, Vec::len);
            if count > 0 {
                note(session, Some(format!("{} {count} {}", text("verb").unwrap_or("Saved"), if count == 1 { "memory" } else { "memories" })));
            }
        }
        // Numbers only: `total_chars`, `total_limit_chars`, `file_count`, `largest_chars` (when one file is over by itself).
        "instruction_size_warning" => {
            let chars = |key: &str| line[key].as_u64().map(|n| tokens(n as f64));
            if let (Some(total), Some(limit)) = (chars("total_chars"), chars("total_limit_chars")) {
                note(session, Some(match (line["file_count"].as_u64(), chars("largest_chars")) {
                    (Some(1), _) => format!("Your instruction file is {total} characters, over the recommended {limit}, so it costs context on every turn"),
                    (_, Some(largest)) => format!("Your instruction files add up to {total} characters, over the recommended {limit}; the largest is {largest}"),
                    _ => format!("Your instruction files add up to {total} characters, over the recommended {limit}"),
                }));
            }
        }
        // `sent_count` files named, `missing`: [{ file_uuid, reason }] with a short code.
        "file_attachments_missing" => {
            let missing = line["missing"].as_array().map_or(0, Vec::len);
            if missing > 0 {
                let sent = line["sent_count"].as_u64().unwrap_or(missing as u64).max(missing as u64);
                let why = match line["missing"][0]["reason"].as_str() {
                    Some("too_large") => ": too large",
                    Some("not_found") => ": the file was deleted or is not yours to read",
                    Some("no_space") => ": the disk is full",
                    Some("download" | "no_token") => ": it could not be downloaded",
                    Some("write") => ": it could not be saved",
                    _ => "",
                };
                let which = if sent == 1 { "The attached file".to_owned() } else { format!("{missing} of {sent} attached files") };
                note(session, Some(format!("{which} did not reach Claude{why}")));
            }
        }
        // Data loss, so an error and not a note: `error_code` ("ENOSPC", "EDQUOT"), `lost` ("transcript",
        // "global_config"), `transcript_session_ids`.
        "session_writes_lost" => {
            let why = if text("error_code") == Some("EDQUOT") { "the disk quota is used up" } else { "the disk is full" };
            let lost: Vec<&str> = line["lost"].as_array().map(|all| all.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let what = match (lost.contains(&"transcript"), lost.contains(&"global_config")) {
                (true, true) => "Part of this conversation and some of your Claude settings were not saved",
                (false, true) => "Some changes to your Claude settings were not saved",
                _ => "Part of this conversation was not saved, so resuming it later may be missing messages",
            };
            let said = format!("{what} because {why}. Free up space on the disk.");
            if session.items.last() != Some(&Item::Error(said.clone())) {
                session.items.push(Item::Error(said));
            }
        }
        "agents_killed" => note(session, Some("Background agents were stopped".to_owned())),
        _ => return false,
    }
    true
}

/// `said` cut to `most` characters.
fn cut(said: &str, most: usize) -> String {
    let said = said.trim();
    if said.chars().count() > most { format!("{}…", said.chars().take(most).collect::<String>().trim_end()) } else { said.to_owned() }
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
    fn commands_changed_replaces_the_slash_menu_list() {
        let mut session = Session::default();
        session.hear(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "initialize", "response": { "commands": [{ "name": "old", "description": "", "argumentHint": "" }] } } }));
        session.hear(&json!({ "type": "system", "subtype": "commands_changed", "commands": [
            { "name": "old", "description": "", "argumentHint": "" },
            { "name": "mcp__notes__summarize", "description": "Summarize the notes", "argumentHint": "[topic]" }] }));
        assert_eq!(session.commands.iter().map(|command| command.name.as_str()).collect::<Vec<_>>(), ["old", "mcp__notes__summarize"]);
        assert!(session.items.is_empty());
    }

    #[test]
    fn a_model_refusal_is_a_note_with_or_without_a_fallback() {
        let session = heard(r#"{"type":"system","subtype":"model_refusal_fallback","original_model":"claude-opus-5-5","fallback_model":"claude-sonnet-5-5"}"#);
        assert_eq!(session.items, [Item::Notice("claude-opus-5-5 declined this request, so Claude retried it on claude-sonnet-5-5".into())]);
        let session = heard(r#"{"type":"system","subtype":"model_refusal_no_fallback","original_model":"claude-opus-5-5"}"#);
        assert_eq!(session.items, [Item::Notice("claude-opus-5-5 declined this request and no other model was tried".into())]);
    }

    #[test]
    fn effort_turning_off_per_turn_is_said_once_until_an_init_turns_it_back_on() {
        // As Claude Code 2.1.328's schema builds it.
        let changed = json!({ "type": "system", "subtype": "per_turn_effort_changed", "per_turn_effort_active": false, "uuid": "u", "session_id": "s" });
        let mut session = Session::default();
        session.hear(&changed);
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "hi" }] } }));
        session.hear(&changed);
        assert_eq!(session.items.iter().filter(|item| **item == Item::Notice(EFFORT_NOTE.into())).count(), 1);
        session.hear(&json!({ "type": "system", "subtype": "init", "session_id": "s", "per_turn_effort_active": true }));
        session.hear(&changed);
        assert_eq!(session.items.iter().filter(|item| **item == Item::Notice(EFFORT_NOTE.into())).count(), 2);
        // An init that starts with it off says nothing more when the change repeats it.
        let mut off = Session::default();
        off.hear(&json!({ "type": "system", "subtype": "init", "session_id": "s", "per_turn_effort_active": false }));
        off.hear(&changed);
        assert!(off.items.is_empty());
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

    #[test]
    fn a_call_auto_mode_blocked_says_so_under_its_tool_row_or_as_a_notice() {
        let denied = |id: &str| json!({ "type": "system", "subtype": "permission_denied", "tool_name": "Bash", "tool_use_id": id, "decision_reason_type": "classifier", "decision_reason": "It would push to main", "message": "Permission denied", "uuid": "u", "session_id": "s" });
        let mut session = Session::default();
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "git push" } }, { "type": "tool_use", "id": "t2", "name": "Read", "input": { "file_path": "/a" } }] } }));
        session.hear(&denied("t1"));
        session.hear(&denied("t1"));
        assert!(matches!(session.items[0], Item::Tool(_)));
        assert_eq!(session.items[1], Item::Notice("Auto mode blocked Bash: It would push to main".into()));
        assert!(matches!(session.items[2], Item::Tool(_)));
        assert_eq!(session.items.len(), 3);
        // A call it has no row for, refused by a rule, and without a reason of its own.
        let mut other = Session::default();
        other.hear(&json!({ "type": "system", "subtype": "permission_denied", "tool_name": "Write", "tool_use_id": "x", "decision_reason_type": "rule", "message": "Denied by a rule" }));
        assert_eq!(other.items, [Item::Notice("Write was denied: Denied by a rule".into())]);
    }

    #[test]
    fn a_running_hook_is_on_the_status_line_and_a_quiet_success_leaves_nothing() {
        let mut session = Session::default();
        session.hear(&json!({ "type": "system", "subtype": "hook_started", "hook_id": "h1", "hook_name": "PreToolUse:Bash", "hook_event": "PreToolUse" }));
        assert_eq!(session.activity(), "Running PreToolUse:Bash hook");
        session.hear(&json!({ "type": "system", "subtype": "hook_progress", "hook_id": "h1", "hook_name": "PreToolUse:Bash", "hook_event": "PreToolUse", "stdout": "x", "stderr": "", "output": "x" }));
        assert_eq!(session.activity(), "Running PreToolUse:Bash hook");
        session.hear(&json!({ "type": "system", "subtype": "hook_response", "hook_id": "h1", "hook_name": "PreToolUse:Bash", "hook_event": "PreToolUse", "output": "ok", "stdout": "ok", "stderr": "", "exit_code": 0, "outcome": "success" }));
        assert_eq!(session.status, None);
        assert!(session.items.is_empty());
    }

    #[test]
    fn a_hook_that_fails_shows_its_output_cut_and_once() {
        let failed = json!({ "type": "system", "subtype": "hook_response", "hook_id": "h2", "hook_name": "PostToolUse:Edit", "hook_event": "PostToolUse", "output": "", "stdout": "", "stderr": "lint failed", "exit_code": 2, "outcome": "error" });
        let mut session = Session::default();
        session.hear(&failed);
        session.hear(&failed);
        assert_eq!(session.items, [Item::Notice("PostToolUse:Edit hook failed: lint failed".into())]);
        let long = json!({ "type": "system", "subtype": "hook_response", "hook_name": "H", "hook_event": "PreToolUse", "output": "e".repeat(500), "stdout": "", "stderr": "", "outcome": "error" });
        let mut session = Session::default();
        session.hear(&long);
        let Item::Notice(said) = &session.items[0] else { panic!() };
        assert_eq!(said.chars().count(), "H hook failed: ".len() + HOOK_KEPT + 1);
        // A cancelled hook says nothing, and no exit code and no output still says it failed.
        session.hear(&json!({ "type": "system", "subtype": "hook_response", "hook_name": "C", "hook_event": "PreToolUse", "output": "", "stdout": "", "stderr": "", "outcome": "cancelled" }));
        session.hear(&json!({ "type": "system", "subtype": "hook_response", "hook_name": "D", "hook_event": "PreToolUse", "output": "", "stdout": "", "stderr": "", "outcome": "error" }));
        assert_eq!(session.items.len(), 2);
        assert_eq!(session.items[1], Item::Notice("D hook failed".into()));
    }

    #[test]
    fn a_stop_hook_summary_is_one_line_only_when_a_hook_stopped_claude_or_failed() {
        let summary = |extra: Value| {
            let mut line = json!({ "type": "system", "subtype": "stop_hook_summary", "hook_count": 1, "hook_infos": [{ "command": "check.sh" }], "hook_errors": [], "prevented_continuation": false, "has_output": false, "level": "info" });
            line.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            line
        };
        let mut quiet = Session::default();
        quiet.hear(&summary(json!({})));
        assert!(quiet.items.is_empty());
        let mut stopped = Session::default();
        stopped.hear(&summary(json!({ "prevented_continuation": true, "stop_reason": "Tests are red" })));
        assert_eq!(stopped.items, [Item::Notice("A stop hook kept Claude from finishing: Tests are red".into())]);
        let mut failed = Session::default();
        failed.hear(&summary(json!({ "hook_errors": ["check.sh exited 1"] })));
        assert_eq!(failed.items, [Item::Notice("A stop hook failed: check.sh exited 1".into())]);
        // The stop hook's own error line is left to the summary.
        failed.hear(&json!({ "type": "system", "subtype": "hook_response", "hook_name": "Stop", "hook_event": "Stop", "output": "x", "stdout": "", "stderr": "x", "outcome": "error" }));
        assert_eq!(failed.items.len(), 1);
    }

    #[test]
    fn the_dropped_notices_become_plain_lines() {
        let mut session = Session::default();
        session.hear(&json!({ "type": "system", "subtype": "memory_saved", "written_paths": ["/m/a.md", "/m/b.md"], "uuid": "u", "session_id": "s" }));
        session.hear(&json!({ "type": "system", "subtype": "memory_saved", "written_paths": ["/m/a.md"], "verb": "Updated" }));
        session.hear(&json!({ "type": "system", "subtype": "instruction_size_warning", "total_chars": 52_000, "total_limit_chars": 40_000, "file_count": 3, "largest_chars": 45_000 }));
        session.hear(&json!({ "type": "system", "subtype": "instruction_size_warning", "total_chars": 52_000, "total_limit_chars": 40_000, "file_count": 1 }));
        session.hear(&json!({ "type": "system", "subtype": "file_attachments_missing", "message_uuid": "m", "sent_count": 3, "missing": [{ "file_uuid": "f", "reason": "too_large" }] }));
        session.hear(&json!({ "type": "system", "subtype": "agents_killed" }));
        assert_eq!(
            session.items,
            [
                Item::Notice("Saved 2 memories".into()),
                Item::Notice("Updated 1 memory".into()),
                Item::Notice("Your instruction files add up to 52k characters, over the recommended 40k; the largest is 45k".into()),
                Item::Notice("Your instruction file is 52k characters, over the recommended 40k, so it costs context on every turn".into()),
                Item::Notice("1 of 3 attached files did not reach Claude: too large".into()),
                Item::Notice("Background agents were stopped".into()),
            ]
        );
    }

    #[test]
    fn lost_session_writes_are_an_error_and_not_a_note() {
        let mut session = Session::default();
        let lost = json!({ "type": "system", "subtype": "session_writes_lost", "error_code": "ENOSPC", "lost": ["transcript"], "transcript_session_ids": ["s"] });
        session.hear(&lost);
        session.hear(&lost);
        assert_eq!(session.items, [Item::Error("Part of this conversation was not saved, so resuming it later may be missing messages because the disk is full. Free up space on the disk.".into())]);
        session.hear(&json!({ "type": "system", "subtype": "session_writes_lost", "error_code": "EDQUOT", "lost": ["global_config"] }));
        assert_eq!(session.items[1], Item::Error("Some changes to your Claude settings were not saved because the disk quota is used up. Free up space on the disk.".into()));
    }
}
