//! The app side of the Wings MCP server. Claude Code starts `wings --mcp` (bridge.rs), which connects here
//! over a socket only this user can open. Here it lists the tools of plugins that are on and runs them in
//! the plugin's own code, so plugin tools come and go without touching Claude Code's settings. Wings' own
//! tools (history.rs) run here directly. The `wings plugin` CLI uses the same socket (control.rs).

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Mutex,
    },
    thread,
    time::Duration,
};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

/// Tool names are `<plugin id>__<tool>`, so Claude can tell which plugin a tool comes from.
const SEPARATOR: &str = "__";
/// A slow plugin call (one that waits on a network API, say) still gets an answer back to Claude.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// What a tool call returns to Claude: text, and whether it failed.
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

#[derive(Default)]
pub struct Mcp {
    /// Connected bridges, told when the tool list changes.
    clients: Mutex<Vec<UnixStream>>,
    /// Calls waiting for the plugin to answer, by call id.
    pending: Mutex<HashMap<u64, mpsc::Sender<ToolResult>>>,
    next: AtomicU64,
}

pub fn socket_path(app_data: &Path) -> PathBuf {
    app_data.join("mcp.sock")
}

/// Listens on the socket. A stale socket from an earlier run is replaced, and the new one is made
/// readable by this user only.
pub fn start(app: AppHandle, path: PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::remove_file(&path);
    // On first launch the app data folder doesn't exist yet, and binding in a missing folder fails.
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(e) => return eprintln!("[mcp] couldn't listen on {}: {e}", path.display()),
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let app = app.clone();
            thread::spawn(move || serve(&app, stream));
        }
    });
}

/// One bridge: newline-delimited JSON requests in, responses and `tools_changed` events out.
fn serve(app: &AppHandle, stream: UnixStream) {
    let Ok(writer) = stream.try_clone() else { return };
    if let Ok(events) = stream.try_clone() {
        let state = app.state::<AppState>();
        let mut clients = state.mcp.clients.lock().unwrap();
        drop_closed(&mut clients);
        clients.push(events);
    }
    let writer = Mutex::new(writer);
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let reply = match request.get("op").and_then(Value::as_str) {
            Some("list") => json!({ "id": id, "tools": tools(app), "instructions": plugin_instructions(app) }),
            Some("call") => {
                let name = request.get("name").and_then(Value::as_str).unwrap_or_default();
                let arguments = request.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let ppid = request.get("ppid").and_then(Value::as_u64).map(|p| p as u32);
                let result = call(app, name, arguments, ppid).unwrap_or_else(|text| ToolResult { text, is_error: true });
                json!({ "id": id, "result": { "content": [{ "type": "text", "text": result.text }], "isError": result.is_error } })
            }
            Some(op) => match crate::control::handle(app, op, &request) {
                Some(Ok(result)) => json!({ "id": id, "result": result }),
                Some(Err(error)) => json!({ "id": id, "error": error }),
                None => json!({ "id": id, "error": "unknown op" }),
            },
            None => json!({ "id": id, "error": "unknown op" }),
        };
        let mut out = writer.lock().unwrap();
        if writeln!(out, "{reply}").is_err() {
            break;
        }
    }
}

/// Lets go of connections that hung up. `wings statusline` connects for every Claude reply and leaves right away,
/// so without this each one would keep a socket open until plugins next change. Bridges and the CLI skip the
/// empty line.
fn drop_closed(clients: &mut Vec<UnixStream>) {
    clients.retain_mut(|client| writeln!(client).is_ok());
}

/// Wings' own tools, then the tools of every plugin that's on and approved, as MCP tool definitions.
fn tools(app: &AppHandle) -> Vec<Value> {
    let state = app.state::<AppState>();
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
fn plugin_instructions(app: &AppHandle) -> Vec<String> {
    let state = app.state::<AppState>();
    let store = state.plugins.lock().unwrap();
    store.list().into_iter().filter(|p| p.enabled && p.approved).map(|p| p.manifest.contributes.mcp_instructions).filter(|l| !l.is_empty()).collect()
}

/// Runs one of Wings' own tools. Their names have no `__`, so they can't be a plugin's.
fn builtin(app: &AppHandle, name: &str, arguments: &Value, ppid: Option<u32>) -> Option<ToolResult> {
    let state = app.state::<AppState>();
    let result = crate::history::call_tool(&state.history, &state.claude_dir, name, arguments)
        .or_else(|| project_info_tool(app, name, arguments, ppid))
        .or_else(|| crate::mcp_panes::call_tool(&WingsHost(app), name, arguments, &|| ppid.and_then(|pid| pane_of(app, pid))))?;
    Some(match result {
        Ok(text) => ToolResult { text, is_error: false },
        Err(text) => ToolResult { text, is_error: true },
    })
}

/// The `project_info_*` tools, for the project of the pane the call came from.
fn project_info_tool(app: &AppHandle, name: &str, arguments: &Value, ppid: Option<u32>) -> Option<Result<String, String>> {
    if !name.starts_with("project_info_") {
        return None;
    }
    let state = app.state::<AppState>();
    let space = ppid.and_then(|pid| pane_of(app, pid)).and_then(|id| state.panes.lock().unwrap().get(&id).map(|p| p.space_id.clone()));
    let spaces = state.spaces.lock().unwrap().spaces.clone();
    crate::project_info::call_tool(&state.project_info, &spaces, space.as_deref(), name, arguments, &|id| {
        let _ = app.emit("project-info-changed", json!({ "projectId": id }));
    })
}

/// Sends the webview `event` with a call id added to `payload`, and waits for it to answer through `mcp_tool_result`.
fn ask_webview(app: &AppHandle, event: &str, mut payload: Value) -> Result<ToolResult, String> {
    let state = app.state::<AppState>();
    let call_id = state.mcp.next.fetch_add(1, Ordering::Relaxed);
    payload["callId"] = json!(call_id);
    let (done, answer) = mpsc::channel();
    state.mcp.pending.lock().unwrap().insert(call_id, done);
    if let Err(e) = app.emit_to("main", event, payload) {
        state.mcp.pending.lock().unwrap().remove(&call_id);
        return Err(e.to_string());
    }
    let result = answer.recv_timeout(CALL_TIMEOUT).map_err(|_| "Wings didn't answer in time".to_string());
    state.mcp.pending.lock().unwrap().remove(&call_id);
    result
}

/// The running app for the pane tools (mcp_panes.rs).
struct WingsHost<'a>(&'a AppHandle);

impl crate::mcp_panes::Host for WingsHost<'_> {
    fn panes(&self) -> Vec<crate::mcp_panes::PaneRow> {
        let state = self.0.state::<AppState>();
        // Never both locks at once; other code takes them in either order.
        let panes: Vec<_> = state.panes.lock().unwrap().values().cloned().collect();
        let info = state.pane_info.lock().unwrap();
        let mut rows: Vec<_> = panes
            .iter()
            .map(|p| {
                let live = info.get(&p.id);
                crate::mcp_panes::PaneRow {
                    id: p.id.clone(),
                    space_id: p.space_id.clone(),
                    cwd: live.and_then(|i| i.cwd.clone()).map_or_else(|| p.cwd.clone(), PathBuf::from),
                    command: live.map(|i| i.command.clone()),
                    title: p.title(),
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

    fn output(&self, pane_id: &str) -> Option<Vec<u8>> {
        let pane = self.0.state::<AppState>().panes.lock().unwrap().get(pane_id).cloned()?;
        Some(pane.recent_output())
    }

    fn open(&self, plan: crate::mcp_panes::OpenPlan) -> Result<String, String> {
        let payload = json!({ "spaceId": plan.space_id, "cwd": plan.cwd, "input": plan.input });
        let answer = ask_webview(self.0, "mcp-open-pane", payload)?;
        if answer.is_error {
            Err(answer.text)
        } else {
            Ok(answer.text)
        }
    }

    fn notify(&self, title: String, body: String) -> Result<(), String> {
        use tauri_plugin_notification::NotificationExt;
        // One allowance for every Claude session together, the same size as a plugin's.
        self.0.state::<AppState>().notified.lock().unwrap().allow("claude", std::time::Instant::now()).map_err(|wait| {
            format!("Already showed {} notifications this minute. Try again in {} s.", crate::plugins::NOTIFY_LIMIT, wait.as_secs() + 1)
        })?;
        self.0.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
    }
}

/// Asks the plugin, through the webview, to run one of its tools, and waits for its answer.
fn call(app: &AppHandle, name: &str, arguments: Value, ppid: Option<u32>) -> Result<ToolResult, String> {
    if let Some(result) = builtin(app, name, &arguments, ppid) {
        return Ok(result);
    }
    let (plugin_id, tool) = name.split_once(SEPARATOR).ok_or_else(|| format!("unknown tool {name}"))?;
    let state = app.state::<AppState>();
    let plugin = state.plugins.lock().unwrap().active(plugin_id).ok_or_else(|| format!("{plugin_id} is turned off in Wings"))?;
    if !plugin.manifest.contributes.mcp_tools.iter().any(|t| t.name == tool) {
        return Err(format!("{plugin_id} has no tool {tool}"));
    }
    let pane_id = ppid.and_then(|pid| pane_of(app, pid));
    ask_webview(app, "mcp-call", json!({ "pluginId": plugin_id, "tool": tool, "arguments": arguments, "paneId": pane_id }))
        .map_err(|_| format!("{plugin_id} didn't answer in time"))
}

/// The webview hands back a plugin's answer.
pub fn resolve(app: &AppHandle, call_id: u64, result: ToolResult) {
    if let Some(done) = app.state::<AppState>().mcp.pending.lock().unwrap().remove(&call_id) {
        let _ = done.send(result);
    }
}

/// Tells every connected bridge to fetch the tool list again; Claude Code then refreshes its tools.
pub fn notify_changed(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.mcp.clients.lock().unwrap().retain_mut(|client| writeln!(client, "{}", json!({ "event": "tools_changed" })).is_ok());
}

/// The Wings pane a bridge runs in: Claude Code is its parent, and somewhere above that is a pane's shell.
fn pane_of(app: &AppHandle, pid: u32) -> Option<String> {
    let shells: HashMap<u32, String> =
        app.state::<AppState>().panes.lock().unwrap().values().filter_map(|p| Some((p.shell_pid?, p.id.clone()))).collect();
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
