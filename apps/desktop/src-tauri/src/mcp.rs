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
        app.state::<AppState>().mcp.clients.lock().unwrap().push(events);
    }
    let writer = Mutex::new(writer);
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let reply = match request.get("op").and_then(Value::as_str) {
            Some("list") => json!({ "id": id, "tools": tools(app) }),
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
    crate::history::mcp_tools().into_iter().chain(plugin_tools).collect()
}

/// Runs one of Wings' own tools. Their names have no `__`, so they can't be a plugin's.
fn builtin(app: &AppHandle, name: &str, arguments: &Value) -> Option<ToolResult> {
    let state = app.state::<AppState>();
    let result = crate::history::call_tool(&state.history, &state.claude_dir, name, arguments)?;
    Some(match result {
        Ok(text) => ToolResult { text, is_error: false },
        Err(text) => ToolResult { text, is_error: true },
    })
}

/// Asks the plugin, through the webview, to run one of its tools, and waits for its answer.
fn call(app: &AppHandle, name: &str, arguments: Value, ppid: Option<u32>) -> Result<ToolResult, String> {
    if let Some(result) = builtin(app, name, &arguments) {
        return Ok(result);
    }
    let (plugin_id, tool) = name.split_once(SEPARATOR).ok_or_else(|| format!("unknown tool {name}"))?;
    let state = app.state::<AppState>();
    let plugin = state.plugins.lock().unwrap().active(plugin_id).ok_or_else(|| format!("{plugin_id} is turned off in Wings"))?;
    if !plugin.manifest.contributes.mcp_tools.iter().any(|t| t.name == tool) {
        return Err(format!("{plugin_id} has no tool {tool}"));
    }
    let call_id = state.mcp.next.fetch_add(1, Ordering::Relaxed);
    let (done, answer) = mpsc::channel();
    state.mcp.pending.lock().unwrap().insert(call_id, done);
    let pane_id = ppid.and_then(|pid| pane_of(app, pid));
    let payload = json!({ "callId": call_id, "pluginId": plugin_id, "tool": tool, "arguments": arguments, "paneId": pane_id });
    if let Err(e) = app.emit_to("main", "mcp-call", payload) {
        state.mcp.pending.lock().unwrap().remove(&call_id);
        return Err(e.to_string());
    }
    let result = answer.recv_timeout(CALL_TIMEOUT).map_err(|_| format!("{plugin_id} didn't answer in time"));
    state.mcp.pending.lock().unwrap().remove(&call_id);
    result
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
