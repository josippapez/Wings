//! Puts a Chat badge on every terminal Claude Code runs in. Its Open as chat button ends that terminal and continues the
//! session in a chat tab, started the way the terminal started it (`claude` or a wrapper like `claude-rtk`). It also
//! fills the sidebar's Chats section under each project with its newest Claude Code sessions, a click on one continuing
//! it in a chat tab.

use std::{cell::RefCell, collections::HashSet};

use serde_json::{Value, json};
use wings_plugin::{
    format::{ago, truncate},
    host, messages, poll, task,
};

use crate::SESSION_SAVED;

/// The sessions each project lists, newest first.
const SHOWN: u32 = 5;
/// How often the lists are read again while Wings is in front. Reading a project's sessions scans its transcripts.
const POLL_MS: u32 = 5 * 60_000;
/// The longest a row's label gets.
const LABEL: usize = 60;

thread_local! {
    static PANES: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    /// What was last read: each project's id and folder, and its newest sessions.
    static CHATS: RefCell<Vec<(String, String, Vec<Value>)>> = const { RefCell::new(Vec::new()) };
}

/// The ids of the sessions Claude Code runs in a terminal right now.
fn running(panes: &[Value]) -> HashSet<String> {
    panes.iter().filter(|pane| pane["session"]["kind"] == "claude").filter_map(|pane| pane["session"]["sessionId"].as_str().map(str::to_owned)).collect()
}

/// The sidebar row that continues `session`, a chat tab in `cwd`. A row can't focus a pane, so a session a terminal runs
/// is marked: two Claude Code processes on one session would write over each other.
fn chat_row(session: &Value, cwd: &str, running: &HashSet<String>, now: f64) -> Option<Value> {
    let id = session["id"].as_str()?;
    let said = session["title"].as_str().or(session["firstPrompt"].as_str()).unwrap_or_default();
    let label = said.lines().map(str::trim).find(|line| !line.is_empty()).map_or_else(|| "Untitled session".to_owned(), |line| truncate(line, LABEL));
    let when = ago(session["lastActiveMs"].as_f64().unwrap_or(0.0), now);
    let detail = match session["gitBranch"].as_str().filter(|branch| !branch.is_empty()) {
        Some(branch) => format!("{when} · {branch}"),
        None => when,
    };
    let mut row = json!({ "id": id, "label": label, "detail": detail, "open": { "view": "chat", "args": { "resume": id, "cwd": cwd } } });
    if running.contains(id) {
        row["badge"] = json!({ "text": "running", "tone": "info" });
    }
    Some(row)
}

/// Shows what was last read under each project. Wings skips a section that repeats what it shows, so a change of panes
/// only costs the calls.
fn show() {
    let running = PANES.with(|panes| running(&panes.borrow()));
    let now = host::now();
    let sections: Vec<(String, Value)> = CHATS.with(|chats| {
        chats
            .borrow()
            .iter()
            .map(|(id, cwd, sessions)| {
                let rows: Vec<Value> = sessions.iter().filter_map(|session| chat_row(session, cwd, &running, now)).collect();
                (id.clone(), if rows.is_empty() { Value::Null } else { Value::Array(rows) })
            })
            .collect()
    });
    task::spawn(async move {
        for (project, rows) in sections {
            drop(host::set_project_section(&project, "chats", &rows).await);
        }
    });
}

/// Reads the newest sessions of every project the user didn't turn the section off for, and shows them.
fn refresh() {
    task::spawn(async {
        let Ok(projects) = host::projects().await else { return };
        let mut read = Vec::new();
        for project in projects.as_array().into_iter().flatten() {
            let off = project["hiddenSections"].as_array().is_some_and(|keys| keys.iter().any(|key| key == "claude-chat:chats"));
            let (Some(id), Some(path)) = (project["id"].as_str(), project["path"].as_str()) else { continue };
            if off {
                continue;
            }
            let sessions = host::past_sessions_in(id, "", SHOWN).await.ok().and_then(|list| list.as_array().cloned()).unwrap_or_default();
            read.push((id.to_owned(), path.to_owned(), sessions));
        }
        CHATS.with(|chats| *chats.borrow_mut() = read);
        show();
    });
}

/// The badge of a pane Claude Code runs in, or none.
fn badge(pane: &Value) -> Value {
    if pane["session"]["kind"] != "claude" {
        return Value::Null;
    }
    json!({
        "label": "Chat",
        "tone": "neutral",
        "title": "Claude chat (experimental)",
        "rows": [{ "label": "Open as chat", "value": "Once Claude has finished, ends this terminal and goes on with the session in a chat tab" }],
        "actions": [{ "id": "open", "label": "Open as chat", "primary": true }],
    })
}

/// What the chat tab opens with, to go on with the session in `pane`.
fn chat_args(pane: &Value) -> Option<Value> {
    let session = &pane["session"];
    let program = session["launcher"].as_str().unwrap_or("claude");
    Some(json!({ "resume": session["sessionId"].as_str()?, "cwd": pane["cwd"], "program": program }))
}

/// The data of the Changes plugin's event that carries a comment left on a line of its diff viewer, for the chat pages.
fn review_data(event: &Value) -> Option<&Value> {
    (event["from"] == "changes" && event["name"] == crate::review::EVENT).then(|| &event["data"])
}

#[unsafe(no_mangle)]
pub fn start() {
    crate::plan_limit::listen();
    // A chat page can't hear another plugin's events, so the main passes the comments on; each page keeps its chat's.
    host::on("pluginEvent", |_, event| {
        if let Some(data) = review_data(event) {
            messages::tell(crate::review::EVENT, data);
        }
    });
    host::on_panes(|list| {
        PANES.with(|panes| *panes.borrow_mut() = list.to_vec());
        show();
        let badges: Vec<_> = list.iter().map(|pane| (pane["paneId"].as_str().unwrap_or_default().to_owned(), badge(pane))).collect();
        task::spawn(async move {
            for (pane, badge) in badges {
                let _ = host::set_badge(&pane, &badge).await;
            }
        });
    });
    host::on_projects(|_| refresh());
    messages::on(SESSION_SAVED, |_| refresh());
    poll::while_focused(POLL_MS, false, refresh);
    host::on_action(|action| async move {
        let id = action["paneId"].as_str().unwrap_or_default().to_owned();
        let pane = PANES.with(|panes| panes.borrow().iter().find(|pane| pane["paneId"] == id.as_str()).cloned());
        let pane = pane.ok_or("Claude Code isn't running in this terminal any more")?;
        // Closing the terminal ends Claude Code there, and with it a turn it is on.
        if !matches!(pane["session"]["state"].as_str(), Some("idle" | "done")) {
            return Err("Claude is still working in this terminal. Open it as a chat once it has finished.".into());
        }
        let args = chat_args(&pane).ok_or("Claude Code isn't running in this terminal any more")?;
        // The terminal's Claude Code ends first, so the chat's is the only one writing to the session.
        host::close_pane(&id).await?;
        host::open_view_with("chat", "tab", &args).await.map(drop)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_changes_plugins_review_comments_are_passed_to_the_chats() {
        let comment = json!({ "from": "changes", "name": "review-comment", "data": { "chat": "c", "id": "1:-1" } });
        assert_eq!(review_data(&comment), Some(&json!({ "chat": "c", "id": "1:-1" })));
        assert_eq!(review_data(&json!({ "from": "pr-tracker", "name": "review-comment", "data": {} })), None);
        assert_eq!(review_data(&json!({ "from": "changes", "name": "open-changes", "data": {} })), None);
    }

    #[test]
    fn only_claude_panes_get_the_badge_and_it_resumes_with_their_wrapper() {
        let claude = json!({ "paneId": "3", "cwd": "/repo", "session": { "kind": "claude", "sessionId": "s1", "launcher": "claude-rtk" } });
        assert_eq!(badge(&claude)["actions"][0]["id"], "open");
        assert_eq!(chat_args(&claude), Some(json!({ "resume": "s1", "cwd": "/repo", "program": "claude-rtk" })));
        let plain = json!({ "paneId": "4", "cwd": "/repo", "session": { "kind": "claude", "sessionId": "s2" } });
        assert_eq!(chat_args(&plain).unwrap()["program"], "claude");
        assert_eq!(badge(&json!({ "paneId": "5", "session": { "kind": "opencode" } })), Value::Null);
        assert_eq!(badge(&json!({ "paneId": "6", "session": null })), Value::Null);
    }

    #[test]
    fn a_session_row_continues_it_in_a_chat_and_marks_one_a_terminal_runs() {
        let now = 1_800_000_000_000.0;
        let none = HashSet::new();
        let named = json!({ "id": "s1", "title": "Fix the build", "firstPrompt": "ignored", "gitBranch": "main", "lastActiveMs": now - 2.0 * 3_600_000.0 });
        let row = chat_row(&named, "/repo", &none, now).unwrap();
        assert_eq!((row["label"].as_str(), row["detail"].as_str()), (Some("Fix the build"), Some("2 h ago · main")));
        assert_eq!(row["open"], json!({ "view": "chat", "args": { "resume": "s1", "cwd": "/repo" } }));
        assert_eq!(row["badge"], Value::Null);
        // No title: the first prompt's first line, cut.
        let long = json!({ "id": "s2", "title": null, "firstPrompt": format!("\n  {}\nmore", "x".repeat(100)), "gitBranch": "", "lastActiveMs": now });
        let row = chat_row(&long, "/repo", &none, now).unwrap();
        assert_eq!(row["label"].as_str().unwrap().chars().count(), LABEL);
        assert_eq!(row["detail"], "just now");
        let panes = [json!({ "session": { "kind": "claude", "sessionId": "s1" } }), json!({ "session": { "kind": "opencode", "sessionId": "s2" } })];
        let live = running(&panes);
        assert_eq!(chat_row(&named, "/repo", &live, now).unwrap()["badge"]["text"], "running");
        assert_eq!(chat_row(&long, "/repo", &live, now).unwrap()["badge"], Value::Null);
        assert_eq!(chat_row(&json!({ "title": "no id" }), "/repo", &none, now), None);
    }
}
