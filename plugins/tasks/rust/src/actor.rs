//! Who wrote an entry: the user, git, or the agent session behind a tool call, and how a call's context says which.

use crate::{
    commands::agent_label,
    domain::{Actor, Agent, Issue, SessionRef},
    home::home_of,
};
use serde_json::Value;

/// The project (its main project, for a worktree) a tool call came from, and its agent session as an Actor, from what
/// Wings tells the handler.
pub struct Caller {
    pub project_id: String,
    pub project_name: String,
    pub actor: Actor,
}

pub async fn caller_of(context: Value) -> Result<Caller, String> {
    let project = &context["project"];
    if !project.is_object() {
        return Err("Can't tell which project this call comes from. Call it from a terminal in Wings.".into());
    }
    // A tool call comes from an agent, so a call with no session is a race at startup: still an agent, just without an id.
    let actor = match context["session"].as_object() {
        Some(_) => Actor::Agent(agent_of(&context["session"], context["session"]["id"].as_str().unwrap_or_default())),
        None => Actor::Agent(Agent::default()),
    };
    // A worktree Wings made works in its main project's issues.
    let home = home_of(project).await;
    Ok(Caller { project_id: home.id, project_name: home.name, actor })
}

/// The agent session a hook came from: the session Wings found for the pane, with the id the hook's input gave.
pub fn agent_of(session: &Value, session_id: &str) -> Agent {
    let text = |name: &str| session[name].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    Agent { id: session_id.to_owned(), name: text("name"), kind: text("kind"), via: None }
}

/// Who wrote an entry, by name. An agent goes by its session's latest name in `sessions`, so one session reads as one
/// author after its agent restarts under another name; its name at the time otherwise.
pub fn actor_name(by: &Actor, sessions: &[SessionRef]) -> String {
    match by {
        Actor::You => "You".into(),
        Actor::Git => "Git".into(),
        Actor::Agent(agent) => sessions
            .iter()
            .find(|s| s.id == agent.id)
            .and_then(|s| s.name.clone())
            .filter(|name| !name.is_empty())
            .or_else(|| agent.name.clone().filter(|name| !name.is_empty()))
            .unwrap_or_else(|| agent_label(agent.kind.as_deref())),
    }
}

/// The session that touched the issue last, or none. Resume picks this one.
pub fn last_session(issue: &Issue) -> Option<&SessionRef> {
    issue.sessions.iter().fold(None, |latest: Option<&SessionRef>, s| if latest.is_none_or(|a| s.seen > a.seen) { Some(s) } else { latest })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wings_plugin::task::block_on;

    #[test]
    fn a_tool_calls_context_maps_to_its_project_and_agent_session_with_the_agents_kind() {
        let project = json!({ "id": "proj1", "name": "web", "path": "/work/web" });
        let actor = |session: Value| block_on(caller_of(json!({ "paneId": "p1", "project": project, "session": session }))).unwrap().actor;
        assert_eq!(actor(json!({ "id": "s-1", "name": "fix totals" })), Actor::Agent(Agent { id: "s-1".into(), name: Some("fix totals".into()), ..Agent::default() }));
        assert_eq!(actor(json!({ "id": "s-2", "name": null })), Actor::Agent(Agent { id: "s-2".into(), ..Agent::default() }));
        assert_eq!(actor(json!({ "id": "ses_abc12345", "name": null, "kind": "opencode" })), Actor::Agent(Agent { id: "ses_abc12345".into(), kind: Some("opencode".into()), ..Agent::default() }));
        // An unnamed session is called by its agent; one from before kinds is Claude Code's.
        let named = |agent: Agent| actor_name(&Actor::Agent(agent), &[]);
        assert_eq!(named(Agent { id: "ses_abc12345".into(), kind: Some("opencode".into()), ..Agent::default() }), "OpenCode");
        assert_eq!(named(Agent { id: "s-2".into(), ..Agent::default() }), "Claude Code");
        assert_eq!(actor(Value::Null), Actor::Agent(Agent::default()));
        assert!(block_on(caller_of(json!({ "paneId": null, "project": null, "session": null }))).err().unwrap().starts_with("Can't tell which project"));
    }

    #[test]
    fn an_agent_goes_by_its_sessions_latest_name_so_one_session_reads_as_one_author_after_a_restart() {
        let sessions = [SessionRef { id: "c6".into(), name: Some("Azure DevOps work item 189501".into()), kind: None, seen: 3.0 }];
        let by = |id: &str, name: &str| Actor::Agent(Agent { id: id.into(), name: Some(name.into()), ..Agent::default() });
        assert_eq!(actor_name(&by("c6", "sciensus-digital-core-be-33"), &sessions), "Azure DevOps work item 189501");
        assert_eq!(actor_name(&by("other", "old name"), &sessions), "old name");
        assert_eq!(actor_name(&Actor::You, &sessions), "You");
    }
}
