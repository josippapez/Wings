//! The app side of the Wings socket. Claude Code starts `wings --mcp` (bridge.rs), which connects here over a socket
//! only this user can open, and so do `wings hook`, `wings statusline` and the rest of the `wings` CLI (cli.rs). Each
//! connection sends newline-delimited JSON requests (`{ id, op, ... }`) and gets one reply per request, and the server
//! also writes `tools_changed` events to every open connection. What the ops do is the app's, behind `Handler`.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    sync::{Arc, Mutex},
    thread,
};

use serde_json::{json, Value};
use wings_agents::detect::{Agent, AgentKind};
use wings_spaces::spaces::Space;

/// Tool names are `<plugin id>__<tool>`, so Claude can tell which plugin a tool comes from.
pub const SEPARATOR: &str = "__";

/// What a tool call returns to Claude: text, and whether it failed.
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

/// What the app answers on the socket. Every method is called on a connection's own thread.
pub trait Handler: Send + Sync + 'static {
    /// Wings' own tools, then those of every plugin that is on, as MCP tool definitions.
    fn tools(&self) -> Vec<Value>;
    /// The `mcpInstructions` line of every plugin that is on.
    fn instructions(&self) -> Vec<String>;
    /// Runs a tool for the bridge or the CLI. `ppid` is the caller's parent process, `session_id` the OpenCode session
    /// that called, when it says.
    fn call(&self, name: &str, arguments: Value, ppid: Option<u32>, session_id: Option<&str>) -> Result<ToolResult, String>;
    /// What the plugins say to a Claude Code hook (or a `Journal` input), as `{ context?, deny? }`.
    fn hook(&self, input: &Value, ppid: Option<u32>) -> Value;
    /// A CLI request (`plugins`, `install`, `pane_busy`, `statusline`...), or `None` when `op` isn't one.
    fn control(&self, op: &str, request: &Value) -> Option<Result<Value, String>>;
}

/// The open connections, told when the tool list changes.
#[derive(Default)]
pub struct Clients(Mutex<Vec<UnixStream>>);

impl Clients {
    /// Tells every connected bridge to fetch the tool list again; Claude Code then refreshes its tools.
    pub fn notify_changed(&self) {
        self.0.lock().unwrap().retain_mut(|client| writeln!(client, "{}", json!({ "event": "tools_changed" })).is_ok());
    }

    fn add(&self, stream: UnixStream) {
        let mut clients = self.0.lock().unwrap();
        drop_closed(&mut clients);
        clients.push(stream);
    }
}

/// Whether another running app already answers on the socket at `path`. `start` replaces the socket file, which would
/// leave that app listening on a file no one can reach any more.
pub fn served(path: &Path) -> bool {
    UnixStream::connect(path).is_ok()
}

/// Listens on the socket at `path`. A stale socket from an earlier run is replaced, and the new one is made readable by
/// this user only.
pub fn start(path: &Path, handler: Arc<dyn Handler>, clients: Arc<Clients>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::remove_file(path);
    // On first launch the app data folder doesn't exist yet, and binding in a missing folder fails.
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let listener = UnixListener::bind(path)?;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (handler, clients) = (handler.clone(), clients.clone());
            thread::spawn(move || serve(&*handler, &clients, stream));
        }
    });
    Ok(())
}

/// One connection: newline-delimited JSON requests in, responses and `tools_changed` events out.
pub fn serve(handler: &dyn Handler, clients: &Clients, stream: UnixStream) {
    let Ok(writer) = stream.try_clone() else { return };
    if let Ok(events) = stream.try_clone() {
        clients.add(events);
    }
    let writer = Mutex::new(writer);
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        let reply = reply(handler, &request);
        let mut out = writer.lock().unwrap();
        if writeln!(out, "{reply}").is_err() {
            break;
        }
    }
}

fn reply(handler: &dyn Handler, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let ppid = request.get("ppid").and_then(Value::as_u64).map(|p| p as u32);
    match request.get("op").and_then(Value::as_str) {
        Some("list") => json!({ "id": id, "tools": handler.tools(), "instructions": handler.instructions() }),
        Some("call") => {
            let name = request.get("name").and_then(Value::as_str).unwrap_or_default();
            let arguments = request.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let session_id = request.get("sessionId").and_then(Value::as_str);
            let result = handler.call(name, arguments, ppid, session_id).unwrap_or_else(|text| ToolResult { text, is_error: true });
            json!({ "id": id, "result": { "content": [{ "type": "text", "text": result.text }], "isError": result.is_error } })
        }
        Some("hook") => {
            let input = request.get("input").cloned().unwrap_or(Value::Null);
            json!({ "id": id, "result": handler.hook(&input, ppid) })
        }
        Some(op) => match handler.control(op, request) {
            Some(Ok(result)) => json!({ "id": id, "result": result }),
            Some(Err(error)) => json!({ "id": id, "error": error }),
            None => json!({ "id": id, "error": "unknown op" }),
        },
        None => json!({ "id": id, "error": "unknown op" }),
    }
}

/// Lets go of connections that hung up. `wings statusline` connects for every Claude reply and leaves right away,
/// so without this each one would keep a socket open until plugins next change. Bridges and the CLI skip the
/// empty line.
fn drop_closed(clients: &mut Vec<UnixStream>) {
    clients.retain_mut(|client| writeln!(client).is_ok());
}

/// The event plugins are asked about for a hook input, or `None` when there is nothing to ask: no event, or a
/// `PreToolUse` of a tool other than Bash (it is installed for Bash only, but the input is Claude Code's to say).
pub fn hook_event(input: &Value) -> Option<&str> {
    let event = input.get("hook_event_name").and_then(Value::as_str)?;
    (event != "PreToolUse" || input.get("tool_name").and_then(Value::as_str) == Some("Bash")).then_some(event)
}

/// The session a hook's plugins are told of. The pane's agent may not know its session id yet at startup, but the
/// hook's input always does.
pub fn hook_session(event: &str, input: &Value, pane_session: Value) -> Value {
    match input.get("session_id").and_then(Value::as_str) {
        // Claude Code runs the hooks, but the journal reports for OpenCode too, and says which in `agent`.
        Some(id) => json!({ "id": id, "name": pane_session.get("name").cloned().unwrap_or(Value::Null), "kind": hook_agent(event, input) }),
        None => pane_session,
    }
}

/// The agent a hook's session belongs to: Claude Code, except for a `Journal` input whose `agent` is `opencode`.
fn hook_agent(event: &str, input: &Value) -> AgentKind {
    match (event, input.get("agent").and_then(Value::as_str)) {
        ("Journal", Some("opencode")) => AgentKind::Opencode,
        _ => AgentKind::Claude,
    }
}

/// The answers of several plugins as one: contexts joined by a blank line, and for `PreToolUse` the reasons to
/// refuse joined by a new line. Any refusal refuses, so the first plugin's doesn't hide the others' reasons.
pub fn merge_hook_answers(event: &str, answers: &[Value]) -> Value {
    let all = |key: &str| -> Vec<&str> { answers.iter().filter_map(|a| a.get(key)?.as_str()).filter(|t| !t.is_empty()).collect() };
    let mut out = serde_json::Map::new();
    let contexts = all("context");
    if !contexts.is_empty() {
        out.insert("context".into(), json!(contexts.join("\n\n")));
    }
    let denies = all("deny");
    if event == "PreToolUse" && !denies.is_empty() {
        out.insert("deny".into(), json!(denies.join("\n")));
    }
    Value::Object(out)
}

/// What a plugin's tool handler is told about who called, besides the pane: that pane's project and the agent session
/// running in it, with the agent's kind. Each is null when there is none: a call from outside Wings' panes has neither,
/// and a pane that runs no agent, or whose agent has no session id yet, has no session.
pub fn caller(space: Option<&Space>, agent: Option<&Agent>) -> (Value, Value) {
    let project = space.map(|s| {
        let mut project = json!({ "id": s.id, "name": s.name, "path": s.path });
        if let Some(parent) = &s.parent {
            project["parentId"] = json!(parent);
        }
        project
    });
    let session = agent.and_then(|a| a.session_id.as_ref().map(|id| json!({ "id": id, "name": a.name, "kind": a.kind })));
    (project.unwrap_or(Value::Null), session.unwrap_or(Value::Null))
}

/// The pane an OpenCode session runs in. A shared OpenCode service's ancestry points at its first pane, never at later
/// callers, so a call that names its session goes by that.
pub fn session_pane(agents: &[Agent], session_id: &str) -> Option<String> {
    let mut matching = agents.iter().filter(|a| a.kind == AgentKind::Opencode && a.session_id.as_deref() == Some(session_id));
    match (matching.next(), matching.next()) {
        (Some(agent), None) => Some(agent.pane_id.clone()),
        _ => None,
    }
}

/// The Wings pane a bridge, hook or CLI call runs in: Claude Code is its parent, and somewhere above that is a pane's
/// shell. `shells` is each pane's shell process, by pid.
pub fn pane_of(shells: &HashMap<u32, String>, pid: u32) -> Option<String> {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut current = sysinfo::Pid::from_u32(pid);
    for _ in 0..16 {
        if let Some(pane) = shells.get(&current.as_u32()) {
            return Some(pane.clone());
        }
        current = system.process(current)?.parent()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use wings_agents::detect::AgentState;

    #[test]
    fn a_socket_counts_as_served_only_while_an_app_listens_on_it() {
        let dir = std::env::temp_dir().join(format!("wings-served-{}", std::process::id()));
        let socket = dir.join("mcp.sock");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!served(&socket));
        std::fs::create_dir_all(&dir).unwrap();
        // A file left by an app that quit: bound once, nobody listening now.
        drop(UnixListener::bind(&socket).unwrap());
        assert!(!served(&socket));
        std::fs::remove_file(&socket).unwrap();
        let _listener = UnixListener::bind(&socket).unwrap();
        assert!(served(&socket));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn agent(kind: AgentKind, session_id: Option<&str>, name: Option<&str>) -> Agent {
        let (session_id, name) = (session_id.map(String::from), name.map(String::from));
        Agent { pane_id: "p1".into(), space_id: "s1".into(), pid: 1, kind, session_id, name, state: AgentState::Idle, waiting_for: None, args: vec![], started_at: 0, state_since: None, direct: true, summary: None, weak_name: false, claude_dir: None }
    }

    #[test]
    fn a_journal_input_names_its_agent_and_other_hooks_are_claude() {
        let journal = |agent: Value| json!({ "hook_event_name": "Journal", "agent": agent });
        assert_eq!(hook_agent("Journal", &journal(json!("opencode"))), AgentKind::Opencode);
        assert_eq!(hook_agent("Journal", &journal(json!("claude-code"))), AgentKind::Claude);
        assert_eq!(hook_agent("Journal", &json!({ "hook_event_name": "Journal" })), AgentKind::Claude);
        assert_eq!(hook_agent("SessionStart", &json!({ "agent": "opencode" })), AgentKind::Claude);
    }

    #[test]
    fn a_tool_call_carries_the_project_of_its_pane_and_the_agent_session_running_there() {
        let web = Space { id: "s1".into(), name: "web".into(), path: "/work/web".into(), parent: None, added_at: None, pinned: false, worktree_include: None, worktree_setup: None, claude_account: None };
        let claude = agent(AgentKind::Claude, Some("abc"), Some("fix totals"));
        assert_eq!(caller(Some(&web), Some(&claude)), (json!({ "id": "s1", "name": "web", "path": "/work/web" }), json!({ "id": "abc", "name": "fix totals", "kind": "claude" })));
        // A session without a name yet is still a session.
        assert_eq!(caller(Some(&web), Some(&agent(AgentKind::Claude, Some("abc"), None))).1, json!({ "id": "abc", "name": null, "kind": "claude" }));
        // opencode's session is passed the same way, with its kind.
        assert_eq!(caller(Some(&web), Some(&agent(AgentKind::Opencode, Some("ses_abc"), Some("plan")))).1, json!({ "id": "ses_abc", "name": "plan", "kind": "opencode" }));
        // No session: the agent hasn't got its id yet, or no agent runs in the pane.
        for without_session in [Some(agent(AgentKind::Claude, None, None)), Some(agent(AgentKind::Opencode, None, None)), None] {
            assert_eq!(caller(Some(&web), without_session.as_ref()).1, Value::Null);
        }
        // A worktree Wings made names the project it belongs to.
        let tree = Space { id: "s2".into(), name: "web-fix".into(), path: "/work/web-fix".into(), parent: Some("s1".into()), added_at: None, pinned: false, worktree_include: None, worktree_setup: None, claude_account: None };
        assert_eq!(caller(Some(&tree), None).0, json!({ "id": "s2", "name": "web-fix", "path": "/work/web-fix", "parentId": "s1" }));
        // A call from outside every pane has neither.
        assert_eq!(caller(None, None), (Value::Null, Value::Null));
    }

    #[test]
    fn shared_server_sessions_route_to_their_own_panes() {
        let first = agent(AgentKind::Opencode, Some("ses_setup"), Some("setup"));
        let mut later = agent(AgentKind::Opencode, Some("ses_wings"), Some("wings"));
        later.pane_id = "p2".into();
        later.space_id = "s2".into();
        let agents = [first, later];
        assert_eq!(session_pane(&agents, "ses_setup"), Some("p1".into()));
        assert_eq!(session_pane(&agents, "ses_wings"), Some("p2".into()));
        assert_eq!(session_pane(&agents, "ses_unknown"), None);
        assert_eq!(session_pane(&agents, ""), None);
        let mut duplicate = agents[1].clone();
        duplicate.pane_id = "p3".into();
        assert_eq!(session_pane(&[agents[1].clone(), duplicate], "ses_wings"), None);
        assert_eq!(session_pane(&[agent(AgentKind::Claude, Some("ses_wings"), None)], "ses_wings"), None);
    }

    #[test]
    fn hook_answers_merge_into_one() {
        let answers = [json!({ "context": "a" }), json!({}), json!({ "context": "b", "deny": "no, because x" }), json!({ "deny": "and y", "context": "" })];
        assert_eq!(merge_hook_answers("PreToolUse", &answers), json!({ "context": "a\n\nb", "deny": "no, because x\nand y" }));
        // A session can't be refused.
        assert_eq!(merge_hook_answers("SessionStart", &answers), json!({ "context": "a\n\nb" }));
        assert_eq!(merge_hook_answers("SessionStart", &[]), json!({}));
    }

    #[test]
    fn only_bash_tool_calls_are_asked_about() {
        assert_eq!(hook_event(&json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash" })), Some("PreToolUse"));
        assert_eq!(hook_event(&json!({ "hook_event_name": "PreToolUse", "tool_name": "Edit" })), None);
        assert_eq!(hook_event(&json!({ "hook_event_name": "SessionStart" })), Some("SessionStart"));
        assert_eq!(hook_event(&json!({})), None);
    }

    #[test]
    fn connections_that_hung_up_are_let_go() {
        let (open, open_peer) = UnixStream::pair().unwrap();
        let (closed, closed_peer) = UnixStream::pair().unwrap();
        drop(closed_peer);
        let mut clients = vec![open, closed];
        drop_closed(&mut clients);
        assert_eq!(clients.len(), 1);
        // The one still open got only an empty line, which a bridge reads as nothing.
        let mut line = String::new();
        BufReader::new(open_peer).read_line(&mut line).unwrap();
        assert_eq!(line, "\n");
    }
}
