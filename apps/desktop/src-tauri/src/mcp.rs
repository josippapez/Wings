//! The app side of the Wings MCP server. Claude Code starts `wings --mcp`, which connects to the socket that
//! `wings_connect::server` serves for this user only. Here it lists the tools of plugins that are on and runs them in
//! the plugin's own code, so plugin tools come and go without touching Claude Code's settings. Wings' own tools
//! (history.rs, mcp_panes.rs, project_info.rs) run here directly. The `wings plugin` CLI uses the same socket (control.rs).

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use wings_connect::server::{self, caller, hook_event, hook_session, merge_hook_answers, session_pane, Clients, Handler, SEPARATOR};

pub use wings_connect::{cli::socket_path, server::ToolResult};

use crate::AppState;

/// A slow plugin call (one that waits on a network API, say) still gets an answer back to Claude.
/// What `ask_webview_within` says when the window doesn't answer in time.
const TIMED_OUT: &str = "Wings didn't answer in time";
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a plugin has to answer a Claude Code hook. `wings hook` gives up on Wings after 2 s.
const HOOK_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct Mcp {
    /// Connected bridges, told when the tool list changes.
    clients: Arc<Clients>,
    /// Calls waiting for the plugin to answer, by call id.
    pending: Mutex<HashMap<u64, mpsc::Sender<ToolResult>>>,
    next: AtomicU64,
}

/// Listens on the socket, replacing a stale one from an earlier run.
pub fn start(app: AppHandle, path: PathBuf) {
    let clients = app.state::<AppState>().mcp.clients.clone();
    if let Err(e) = server::start(&path, Arc::new(Desktop(app)), clients) {
        crate::applog::log_line!("[mcp] couldn't listen on {}: {e}", path.display());
    }
}

/// The running desktop app, as the socket's requests reach it.
struct Desktop(AppHandle);

impl Handler for Desktop {
    /// Wings' own tools, then the tools of every plugin that's on and approved, as MCP tool definitions.
    fn tools(&self) -> Vec<Value> {
        let state = self.0.state::<AppState>();
        let store = state.plugins.lock().unwrap();
        let plugin_tools = store
            .list()
            .into_iter()
            .filter(|p| p.enabled && p.approved)
            .flat_map(|p| {
                let id = p.manifest.id.clone();
                p.manifest.contributes.mcp_tools.into_iter().map(move |t| {
                    json!({ "name": format!("{id}{SEPARATOR}{}", t.name), "description": t.description, "inputSchema": t.input_schema })
                })
            });
        crate::history::mcp_tools().into_iter().chain(crate::mcp_panes::tools()).chain(crate::project_info::mcp_tools()).chain(plugin_tools).collect()
    }

    /// The `mcpInstructions` line of every plugin that's on and approved, for the bridge to add to the server's
    /// instructions: Claude sees only those and tool names until it searches for a tool.
    fn instructions(&self) -> Vec<String> {
        let state = self.0.state::<AppState>();
        let store = state.plugins.lock().unwrap();
        store.list().into_iter().filter(|p| p.enabled && p.approved).map(|p| p.manifest.contributes.mcp_instructions).filter(|l| !l.is_empty()).collect()
    }

    fn call(&self, name: &str, arguments: Value, ppid: Option<u32>, session_id: Option<&str>) -> Result<ToolResult, String> {
        call(&self.0, name, arguments, ppid, session_id)
    }

    fn hook(&self, input: &Value, ppid: Option<u32>) -> Value {
        hook(&self.0, input, ppid)
    }

    fn control(&self, op: &str, request: &Value) -> Option<Result<Value, String>> {
        crate::control::handle(&self.0, op, request)
    }
}

/// Runs one of Wings' own tools. Their names have no `__`, so they can't be a plugin's.
fn builtin(app: &AppHandle, name: &str, arguments: &Value, caller: &dyn Fn() -> Option<String>) -> Option<ToolResult> {
    let state = app.state::<AppState>();
    if matches!(name, "search_history" | "read_session") {
        // The caller can't ask again later, so OpenCode's sessions are loaded before it searches.
        state.history.sync_opencode();
    }
    let result = crate::history::call_tool(&state.history, &crate::claude_accounts::dirs(), name, arguments)
        .or_else(|| project_info_tool(app, name, arguments, caller))
        .or_else(|| crate::mcp_panes::call_tool(&WingsHost(app), name, arguments, caller))?;
    Some(match result {
        Ok(text) => ToolResult { text, is_error: false },
        Err(text) => ToolResult { text, is_error: true },
    })
}

/// The `project_info_*` tools, for the project of the pane the call came from.
fn project_info_tool(app: &AppHandle, name: &str, arguments: &Value, caller: &dyn Fn() -> Option<String>) -> Option<Result<String, String>> {
    if !name.starts_with("project_info_") {
        return None;
    }
    let state = app.state::<AppState>();
    let space = caller().and_then(|id| state.terminals.meta(&id).map(|p| p.space_id.clone()));
    let spaces = state.spaces.lock().unwrap().spaces.clone();
    crate::project_info::call_tool(&state.project_info, &spaces, space.as_deref(), name, arguments, &|id| {
        let _ = app.emit("project-info-changed", json!({ "projectId": id }));
    })
}

/// Sends the webview `event` with a call id added to `payload`, and waits for it to answer through `mcp_tool_result`.
fn ask_webview(app: &AppHandle, event: &str, payload: Value) -> Result<ToolResult, String> {
    ask_webview_within(app, event, payload, CALL_TIMEOUT)
}

fn ask_webview_within(app: &AppHandle, event: &str, mut payload: Value, timeout: Duration) -> Result<ToolResult, String> {
    let state = app.state::<AppState>();
    let call_id = state.mcp.next.fetch_add(1, Ordering::Relaxed);
    payload["callId"] = json!(call_id);
    let (done, answer) = mpsc::channel();
    state.mcp.pending.lock().unwrap().insert(call_id, done);
    if let Err(e) = app.emit_to("main", event, payload) {
        state.mcp.pending.lock().unwrap().remove(&call_id);
        return Err(e.to_string());
    }
    let result = answer.recv_timeout(timeout).map_err(|_| TIMED_OUT.to_string());
    state.mcp.pending.lock().unwrap().remove(&call_id);
    result
}

/// The running app for the pane tools (mcp_panes.rs).
struct WingsHost<'a>(&'a AppHandle);

impl crate::mcp_panes::Host for WingsHost<'_> {
    fn panes(&self) -> Vec<crate::mcp_panes::PaneRow> {
        let state = self.0.state::<AppState>();
        // Never both locks at once; other code takes them in either order.
        let mut panes = Vec::new();
        state.terminals.each_mirror(&mut |p| panes.push((p.meta.clone(), p.title.to_string())));
        let info = state.pane_info.lock().unwrap();
        let mut rows: Vec<_> = panes
            .into_iter()
            .map(|(p, title)| {
                let live = info.get(&p.id);
                crate::mcp_panes::PaneRow {
                    id: p.id.clone(),
                    space_id: p.space_id.clone(),
                    cwd: live.and_then(|i| i.cwd.clone()).map_or_else(|| p.cwd.clone(), PathBuf::from),
                    command: live.map(|i| i.command.clone()),
                    title,
                }
            })
            .collect();
        rows.sort_by(|a, b| a.id.len().cmp(&b.id.len()).then_with(|| a.id.cmp(&b.id)));
        rows
    }

    fn agents(&self) -> Vec<crate::detect::Agent> {
        self.0.state::<AppState>().agents.lock().unwrap().clone()
    }

    fn spaces(&self) -> Vec<crate::spaces::Space> {
        self.0.state::<AppState>().spaces.lock().unwrap().spaces.clone()
    }

    fn set_worktree_setup(&self, space_id: &str, include: Option<&str>, setup: Option<&str>) -> Result<(), String> {
        crate::set_worktree_setup(self.0, space_id, include, setup)
    }

    fn approve_setup(&self, project: &str, pane: &str, current: Option<&str>, proposed: &str) -> Result<bool, String> {
        // One prompt per project at a time, so a second request can't take the place of the one the user is reading.
        static WAITING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        struct Waiting(String);
        impl Drop for Waiting {
            fn drop(&mut self) {
                WAITING.lock().unwrap().retain(|p| p != &self.0);
            }
        }
        let _waiting = {
            let mut waiting = WAITING.lock().unwrap();
            if waiting.iter().any(|p| p == project) {
                return Err(format!("A setup command for {project} is already waiting for the user. Try again after they answer."));
            }
            waiting.push(project.to_string());
            Waiting(project.to_string())
        };
        let focused = self.0.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
        if !focused {
            // So the user comes back to answer; a refused one (rate limit) doesn't stop the prompt.
            let _ = crate::mcp_panes::Host::notify(self, "Claude: Setup command".into(), crate::plugins::clip(&format!("An agent asks to set the setup command for {project}"), 200));
        }
        let wait = crate::mcp_panes::SETUP_APPROVAL_WAIT;
        let payload = json!({ "project": project, "pane": pane, "current": current, "proposed": proposed, "timeoutMs": wait.as_millis() as u64 });
        let answer = ask_webview_within(self.0, "mcp-setup-approval", payload, wait).map_err(|e| {
            if e == TIMED_OUT { "The user didn't answer".to_string() } else { e }
        })?;
        if answer.is_error {
            return Err(answer.text);
        }
        let reply: Value = serde_json::from_str(&answer.text).map_err(|e| e.to_string())?;
        Ok(reply["allow"] == true)
    }

    fn output(&self, pane_id: &str) -> Option<Vec<u8>> {
        self.0.state::<AppState>().terminals.recent_output(pane_id)
    }

    fn open(&self, plan: crate::mcp_panes::OpenPlan) -> Result<crate::mcp_panes::Opened, String> {
        let payload = json!({ "spaceId": plan.space_id, "cwd": plan.cwd, "input": plan.input, "run": plan.run, "nextTo": plan.next_to });
        // `pane_create` lets this tab start outside its project. The grant goes once the window answers, used or not.
        let grants = &self.0.state::<AppState>().pane_grants;
        let grant = plan.cwd.as_ref().map(|dir| (plan.space_id.clone(), PathBuf::from(dir)));
        if let Some(g) = &grant {
            grants.lock().unwrap().push(g.clone());
        }
        let answer = ask_webview(self.0, "mcp-open-pane", payload);
        if let Some(g) = grant {
            let mut grants = grants.lock().unwrap();
            if let Some(at) = grants.iter().position(|x| *x == g) {
                grants.remove(at);
            }
        }
        let answer = answer?;
        if answer.is_error {
            return Err(answer.text);
        }
        // The window answers `{ paneId, ran }`: it alone knows whether the user lets Claude press Enter.
        let reply: Value = serde_json::from_str(&answer.text).map_err(|e| e.to_string())?;
        let pane_id = reply["paneId"].as_str().ok_or("The window didn't say which pane it opened")?.to_string();
        Ok(crate::mcp_panes::Opened { pane_id, ran: reply["ran"].as_bool().unwrap_or(false), reused: reply["reused"] == true })
    }

    fn close(&self, pane_id: &str) -> Result<(), String> {
        let answer = ask_webview(self.0, "mcp-close-pane", json!({ "paneId": pane_id }))?;
        if answer.is_error {
            return Err(answer.text);
        }
        Ok(())
    }

    fn notify(&self, title: String, body: String) -> Result<(), String> {
        use tauri_plugin_notification::NotificationExt;
        // One allowance for every Claude session together, the same size as a plugin's.
        self.0.state::<AppState>().notified.lock().unwrap().allow("claude", std::time::Instant::now()).map_err(|wait| {
            format!("Already showed {} notifications this minute. Try again in {} s.", crate::plugins::NOTIFY_LIMIT, wait.as_secs() + 1)
        })?;
        self.0.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
    }

    fn send(&self, pane_id: &str, text: &str) -> Result<bool, String> {
        let answer = ask_webview(self.0, "mcp-send-to-agent", json!({ "paneId": pane_id, "text": text }))?;
        if answer.is_error {
            return Err(answer.text);
        }
        // The window answers `{ submitted }`: it alone knows whether the user lets agents message each other.
        let reply: Value = serde_json::from_str(&answer.text).map_err(|e| e.to_string())?;
        Ok(reply["submitted"].as_bool().unwrap_or(false))
    }
}


/// Asks the plugin, through the webview, to run one of its tools, and waits for its answer.
fn call(app: &AppHandle, name: &str, arguments: Value, ppid: Option<u32>, session_id: Option<&str>) -> Result<ToolResult, String> {
    let caller = || match session_id {
        Some(id) => session_pane(&app.state::<AppState>().agents.lock().unwrap(), id),
        None => ppid.and_then(|pid| pane_of(app, pid)),
    };
    if let Some(result) = builtin(app, name, &arguments, &caller) {
        return Ok(result);
    }
    let (plugin_id, tool) = name.split_once(SEPARATOR).ok_or_else(|| format!("unknown tool {name}"))?;
    let state = app.state::<AppState>();
    let plugin = state.plugins.lock().unwrap().active(plugin_id).ok_or_else(|| format!("{plugin_id} is turned off in Wings"))?;
    if !plugin.manifest.contributes.mcp_tools.iter().any(|t| t.name == tool) {
        return Err(format!("{plugin_id} has no tool {tool}"));
    }
    let pane_id = caller();
    let (project, session) = caller_of(app, pane_id.as_deref());
    ask_webview(app, "mcp-call", json!({ "pluginId": plugin_id, "tool": tool, "arguments": arguments, "paneId": pane_id, "project": project, "session": session }))
        .map_err(|_| format!("{plugin_id} didn't answer in time"))
}

/// What the plugins that answer a Claude Code hook say, as `{ context?, deny? }` for `wings hook` to print. Only
/// plugins that are on and declare the event are asked, so with none the reply is empty and the webview isn't
/// involved. They're asked together, each with `HOOK_TIMEOUT`; one that is slow, throws or says nothing adds nothing.
fn hook(app: &AppHandle, input: &Value, ppid: Option<u32>) -> Value {
    let Some(event) = hook_event(input) else { return json!({}) };
    let plugins: Vec<String> = {
        let state = app.state::<AppState>();
        let store = state.plugins.lock().unwrap();
        store.list().into_iter().filter(|p| p.enabled && p.approved && p.manifest.contributes.hooks.iter().any(|h| h == event)).map(|p| p.manifest.id).collect()
    };
    if plugins.is_empty() {
        return json!({});
    }
    let pane_id = ppid.and_then(|pid| pane_of(app, pid));
    let (project, session) = caller_of(app, pane_id.as_deref());
    let session = hook_session(event, input, session);
    let answers: Vec<Value> = thread::scope(|scope| {
        let asked: Vec<_> = plugins
            .iter()
            .map(|plugin| {
                let payload = json!({ "pluginId": plugin, "tool": event, "hook": true, "arguments": input, "paneId": pane_id, "project": project, "session": session });
                scope.spawn(move || ask_webview_within(app, "mcp-call", payload, HOOK_TIMEOUT))
            })
            .collect();
        asked
            .into_iter()
            .filter_map(|h| h.join().ok()?.ok())
            .filter(|r| !r.is_error)
            .filter_map(|r| serde_json::from_str(&r.text).ok())
            .collect()
    });
    merge_hook_answers(event, &answers)
}

/// `caller` for the pane a call came from, read where Wings keeps its panes, projects and agents.
fn caller_of(app: &AppHandle, pane_id: Option<&str>) -> (Value, Value) {
    let state = app.state::<AppState>();
    let space_id = pane_id.and_then(|id| state.terminals.meta(id).map(|p| p.space_id.clone()));
    let space = space_id.and_then(|id| state.spaces.lock().unwrap().get(&id).cloned());
    let agent = pane_id.and_then(|id| state.agents.lock().unwrap().iter().find(|a| a.pane_id == id).cloned());
    caller(space.as_ref(), agent.as_ref())
}

/// The webview hands back a plugin's answer.
pub fn resolve(app: &AppHandle, call_id: u64, result: ToolResult) {
    if let Some(done) = app.state::<AppState>().mcp.pending.lock().unwrap().remove(&call_id) {
        let _ = done.send(result);
    }
}

/// Tells every connected bridge to fetch the tool list again; Claude Code then refreshes its tools.
pub fn notify_changed(app: &AppHandle) {
    app.state::<AppState>().mcp.clients.notify_changed();
}

/// The Wings pane a bridge runs in: Claude Code is its parent, and somewhere above that is a pane's shell.
fn pane_of(app: &AppHandle, pid: u32) -> Option<String> {
    let shells: HashMap<u32, String> =
        app.state::<AppState>().terminals.list().iter().filter_map(|p| Some((p.shell_pid?, p.id.clone()))).collect();
    server::pane_of(&shells, pid)
}
