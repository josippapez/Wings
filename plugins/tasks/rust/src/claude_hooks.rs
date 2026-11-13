//! The hooks: what a session is told as it starts, the check that keeps issue keys out of commits and PRs, and the agent's
//! change journal, which links the turn's commits and notes its files on the session's issue.

use crate::{
    actor::agent_of,
    domain::{Actor, Issue, Op, Update},
    home::home_of,
    journal::{journal_issue, journal_of},
    local_only::{local_only_denial, writes_to_history},
    session_start::{Start, in_review_text, session_start_text, stale_issues},
    settings,
    storage::read_store,
    writer::{known_prefix, now, prefixes, remember_prefix, write},
};
use serde_json::Value;
use wings_plugin::hooks::{Say, on_hook};

/// The issue this session touched last, in a project's issues.
fn session_issue<'a>(issues: &'a [Issue], session_id: &str) -> Option<&'a Issue> {
    issues.iter().filter(|i| i.sessions.iter().any(|s| s.id == session_id)).reduce(|best, i| if i.updated_at > best.updated_at { i } else { best })
}

pub async fn session_start(input: Value, context: Value) -> Option<Say> {
    if !context["project"].is_object() {
        return None;
    }
    let home = home_of(&context["project"]).await;
    let store = read_store(&home.id, &home.name).ok()?;
    let issue = session_issue(&store.issues, input["session_id"].as_str().unwrap_or_default());
    let stale = stale_issues(&store.issues, now());
    Some(Say::Context(session_start_text(&Start { project_name: &home.name, keep_local: settings::keep_local(), issue, stale: &stale, now: now() })))
}

/// Each message in a session: when the session's issue is in review, asks the agent to settle it from what the user says.
pub async fn prompt_submit(input: Value, context: Value) -> Option<Say> {
    if !context["project"].is_object() {
        return None;
    }
    let home = home_of(&context["project"]).await;
    let store = read_store(&home.id, &home.name).ok()?;
    let issue = session_issue(&store.issues, input["session_id"].as_str().unwrap_or_default())?;
    (issue.status == crate::domain::Status::InReview).then(|| Say::Context(in_review_text(issue)))
}

/// An agent's journal for a finished turn: links its commits to this session's open issue and records its files in the
/// issue's Files changed. Says nothing back.
pub async fn journal(input: Value, context: Value) -> Option<Say> {
    let session_id = input["session_id"].as_str().filter(|id| !id.is_empty())?;
    if !context["project"].is_object() {
        return None;
    }
    let home = home_of(&context["project"]).await;
    let report = journal_of(&input);
    let key = {
        let store = read_store(&home.id, &home.name).ok()?;
        journal_issue(&store.issues, session_id, report.started)?.key.clone()
    };
    // Written by the journal at the end of the turn, not by the agent.
    let mut agent = agent_of(&context["session"], session_id);
    agent.via = Some("journal".into());
    let by = Actor::Agent(agent);
    // One at a time, and one failing (a full log, say, for a commit link) doesn't stop the others.
    let commits = report.commits.into_iter().map(|commit| Op::Update(Update { key: key.clone(), commit: Some(commit), ..Update::default() }));
    let files = (!report.files.is_empty()).then(|| Op::Files { key: key.clone(), files: report.files });
    for op in commits.chain(files) {
        drop(write(&home.id, &home.name, &op, &by));
    }
    None
}

pub async fn command_check(input: Value, context: Value) -> Option<Say> {
    let command = input["tool_input"]["command"].as_str()?;
    if !writes_to_history(command) || !settings::keep_local() {
        return None;
    }
    if context["project"].is_object() {
        let home = home_of(&context["project"]).await;
        if !known_prefix(&home.id) {
            let store = read_store(&home.id, &home.name).ok()?;
            remember_prefix(&home.id, &store.prefix, store.issues.len());
        }
    }
    local_only_denial(command, &prefixes()).map(Say::Deny)
}

pub fn register() {
    on_hook("SessionStart", session_start);
    on_hook("PreToolUse", command_check);
    on_hook("UserPromptSubmit", prompt_submit);
    on_hook("Journal", journal);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::Agent,
        kv::memory,
        storage::slot_of,
        tool_defs::{context_of, run},
    };
    use serde_json::json;
    use wings_plugin::task::block_on;

    fn web() -> Value {
        json!({ "id": "proj1", "name": "web", "path": "/work/web" })
    }

    fn ctx(session: Value) -> Value {
        context_of(web(), session)
    }

    fn tool(name: &str, input: Value, session: Value) -> String {
        block_on(run(name.to_owned(), input, ctx(session))).unwrap()
    }

    fn context_text(say: Option<Say>) -> Option<String> {
        match say {
            Some(Say::Context(text)) => Some(text),
            _ => None,
        }
    }

    fn stored_issue(key: &str) -> Issue {
        Issue::from_value(&memory::peek(&format!("issues/{}/{key}", slot_of("proj1"))).unwrap()).unwrap()
    }

    #[test]
    fn the_session_start_line_and_the_commit_check_answer_from_storage() {
        memory::reset(&[]);
        let session = json!({ "id": "s-1", "name": "agent one" });
        tool("create", json!({ "title": "Do it" }), session.clone());
        let start = context_text(block_on(session_start(json!({ "session_id": "s-1" }), ctx(session.clone())))).unwrap();
        assert!(start.contains("Wings Issues is the local tracker for web"));
        assert!(start.contains("This session is on WEB-1 \""));
        assert!(block_on(session_start(json!({ "session_id": "s-1" }), json!({ "project": null, "session": null }))).is_none());
        let bash = |command: &str| block_on(command_check(json!({ "session_id": "s-1", "tool_name": "Bash", "tool_input": { "command": command } }), ctx(session.clone())));
        let Some(Say::Deny(reason)) = bash("git commit -m \"WEB-1 done\"") else { panic!("denied") };
        assert!(reason.starts_with("WEB-1 is a local Wings issue"));
        assert!(bash("git commit -m \"done\"").is_none());
        settings::set_keep_local(false).unwrap();
        assert!(bash("git commit -m \"WEB-1 done\"").is_none(), "with the setting off nothing is denied");
    }

    #[test]
    fn the_journal_links_a_turns_commits_and_fills_files_changed_on_this_sessions_open_issue_without_writing_log_notes_and_does_nothing_else() {
        memory::reset(&[]);
        let session = json!({ "id": "j-1", "name": "journal agent", "kind": "opencode" });
        let open = tool("create", json!({ "title": "Journaled", "status": "in_progress" }), session.clone()).split(' ').nth(1).unwrap().to_owned();
        // Put the session on the issue, as a tool call from it would.
        tool("log", json!({ "key": open, "text": "started" }), session.clone());
        let log_before = stored_issue(&open).log.len();
        let payload = |extra: Value| {
            let mut input = json!({ "session_id": "j-1", "cwd": "/work/web", "agent": "opencode", "prompt": "do it", "files": [{ "path": "src/a.ts", "what": "adds the X type" }, { "path": "b.md", "what": "" }], "more": 2, "commits": ["abc123def456", "1234567"] });
            for (key, value) in extra.as_object().unwrap() {
                input[key] = value.clone();
            }
            input
        };
        assert!(block_on(journal(payload(json!({})), ctx(session.clone()))).is_none(), "it answers with nothing");
        let mut now = stored_issue(&open);
        assert_eq!(now.links.as_ref().unwrap().commits.as_deref(), Some(&["abc123d".to_owned(), "1234567".to_owned()][..]));
        assert_eq!(now.log.len(), log_before + 2, "only the two commit links, no note for the files");
        assert_eq!(now.log.iter().filter(|e| e.kind == "note").count(), 1);
        let rows = |issue: &Issue| issue.files.as_ref().unwrap().iter().map(|f| (f.path.clone(), f.what.clone())).collect::<Vec<_>>();
        assert_eq!(rows(&now), [("src/a.ts".to_owned(), "adds the X type".to_owned()), ("b.md".to_owned(), String::new())]);
        assert_eq!(now.files.as_ref().unwrap()[0].session.as_ref().unwrap().kind.as_deref(), Some("opencode"));
        assert!(now.log.iter().any(|e| matches!(&e.by, Actor::Agent(Agent { via: Some(via), .. }) if via == "journal")));

        // A later turn: the same path keeps one row with the newest description, and the same commits add nothing.
        block_on(journal(payload(json!({ "files": [{ "path": "src/a.ts", "what": "renames X to Y" }] })), ctx(session.clone())));
        now = stored_issue(&open);
        assert_eq!(now.links.as_ref().unwrap().commits.as_ref().unwrap().len(), 2);
        assert_eq!(now.log.len(), log_before + 2);
        let mut sorted = rows(&now);
        sorted.sort();
        assert_eq!(sorted, [("b.md".to_owned(), String::new()), ("src/a.ts".to_owned(), "renames X to Y".to_owned())]);
        assert!(tool("get", json!({ "key": open }), session.clone()).contains("Files changed (2):\n  src/a.ts — renames X to Y (journal agent, "));

        // No files: nothing written for them. An unknown session, or an issue that is done, is left alone.
        block_on(journal(payload(json!({ "files": [], "more": 0 })), ctx(session.clone())));
        let snapshot = || memory::keys().iter().map(|k| (k.clone(), memory::peek(k))).collect::<Vec<_>>();
        let before = snapshot();
        block_on(journal(payload(json!({ "session_id": "nobody", "commits": ["fedcba9"] })), ctx(json!({ "id": "nobody", "name": null, "kind": "claude" }))));
        assert_eq!(snapshot(), before);
        tool("update", json!({ "key": open, "status": "done" }), session.clone());
        let done = snapshot();
        block_on(journal(payload(json!({ "commits": ["fedcba9"] })), ctx(session.clone())));
        assert_eq!(snapshot(), done);
        assert!(block_on(journal(payload(json!({})), json!({ "project": null, "session": session }))).is_none());
    }

    #[test]
    fn each_message_in_a_session_whose_issue_is_in_review_asks_the_agent_to_settle_it_other_statuses_and_sessions_get_nothing() {
        memory::reset(&[]);
        let session = json!({ "id": "r-1", "name": "review agent", "kind": "claude" });
        let say = |id: &str| context_text(block_on(prompt_submit(json!({ "session_id": id, "prompt": "it works, done" }), ctx(json!({ "id": id, "name": "review agent", "kind": "claude" })))));
        let made = tool("create", json!({ "title": "Wait for the user" }), session.clone());
        let key = made.split(' ').nth(1).unwrap().to_owned();
        assert_eq!(say("r-1"), None, "to do");
        tool("update", json!({ "key": key, "status": "in_review" }), session.clone());
        assert!(say("r-1").unwrap().starts_with(&format!("This session's {key} \"Wait for the user\" is In review. If this message says the work is done")));
        assert_eq!(say("someone-else"), None);
        tool("update", json!({ "key": key, "status": "done" }), session.clone());
        assert_eq!(say("r-1"), None, "done");
    }
}
