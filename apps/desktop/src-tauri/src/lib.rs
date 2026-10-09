mod claude;
#[cfg(unix)]
mod claude_settings;
mod detect;
mod history;
mod menu;
#[cfg(unix)]
pub mod bridge;
#[cfg(unix)]
pub mod cli;
#[cfg(unix)]
mod control;
// debt: the MCP socket is Unix-only, like plugins themselves; Windows needs a named pipe here.
#[cfg(unix)]
mod mcp;
mod plugin_store;
mod plugin_storage;
mod plugins;
#[cfg(target_os = "macos")]
mod privacy;
mod secrets;
mod pty;
mod spaces;
mod statusline;
mod tail;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use tauri::{
    ipc::{Channel, InvokeResponseBody},
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use claude::SessionSummary;
use detect::{Agent, AgentState, Detector, PaneInfo, PaneProbe};
use pty::{Pane, SpawnRequest};
use plugin_store::{Grant, PluginView, Source, Store};
use plugins::{ExecResult, Plugin};
use spaces::{GitStatus, SpaceStore, SpaceView};

struct AppState {
    claude_dir: PathBuf,
    history: history::History,
    spaces: Mutex<SpaceStore>,
    panes: Mutex<HashMap<String, Arc<Pane>>>,
    focused: Mutex<Option<String>>,
    agents: Mutex<Vec<Agent>>,
    pane_info: Mutex<HashMap<String, PaneInfo>>,
    git: Mutex<HashMap<String, GitStatus>>,
    #[cfg(unix)]
    mcp: mcp::Mcp,
    plugins: Mutex<Store>,
    storage: plugin_storage::Storage,
    next_pane: AtomicU64,
    /// Recent plugin notifications, for their rate limit.
    notified: Mutex<plugins::NotifyLog>,
    /// What Claude Code last told `wings statusline`.
    statusline: Mutex<statusline::Statusline>,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// Async, like every command that reads project folders: a folder on the Desktop or in Documents can wait on a
// macOS permission prompt, and on the main thread that freezes the whole window until it's answered.
#[tauri::command(async)]
fn spaces_list(state: State<AppState>) -> Vec<SpaceView> {
    state.spaces.lock().unwrap().spaces.iter().map(spaces::view).collect()
}

#[tauri::command(async)]
fn spaces_add(state: State<AppState>, path: String) -> SpaceView {
    spaces::view(&state.spaces.lock().unwrap().add(&path))
}

#[tauri::command]
fn spaces_remove(state: State<AppState>, id: String) {
    state.spaces.lock().unwrap().remove(&id);
}

#[tauri::command(async)]
fn sessions_list(state: State<AppState>, space_id: String) -> Res<Vec<SessionSummary>> {
    let path = state.spaces.lock().unwrap().get(&space_id).ok_or("unknown space")?.path.clone();
    Ok(claude::list_sessions(&state.claude_dir, &path))
}

#[tauri::command]
#[expect(clippy::too_many_arguments, reason = "Tauri passes the app and state as arguments too")]
fn pane_create(
    app: AppHandle,
    state: State<AppState>,
    space_id: String,
    cols: u16,
    rows: u16,
    initial_input: Option<String>,
    on_output: Channel<InvokeResponseBody>,
    cwd: Option<String>,
) -> Res<String> {
    let root = PathBuf::from(&state.spaces.lock().unwrap().get(&space_id).ok_or("unknown space")?.path);
    // A plugin's pane can start in a subfolder, but never outside the project it belongs to.
    let cwd = match cwd {
        Some(dir) => plugins::pane_cwd(std::path::Path::new(&dir), &[root], Some(0))?.1,
        None => root,
    };
    let id = format!("p{}", state.next_pane.fetch_add(1, Ordering::Relaxed));
    if cfg!(debug_assertions) {
        eprintln!("[pane] create {id} in {space_id}");
    }
    let exit_app = app.clone();
    let pane = Pane::spawn(
        SpawnRequest { id: id.clone(), space_id, cwd: &cwd, cols, rows },
        on_output,
        move |pane_id| {
            exit_app.state::<AppState>().panes.lock().unwrap().remove(pane_id);
            let _ = exit_app.emit("pane-exited", pane_id);
        },
    )
    .map_err(err)?;
    if let Some(input) = initial_input {
        // Typed into the shell, so you land back at a prompt when the command exits.
        pane.write(input.as_bytes()).map_err(err)?;
    }
    state.panes.lock().unwrap().insert(id.clone(), pane);
    Ok(id)
}

fn pane(state: &AppState, id: &str) -> Res<Arc<Pane>> {
    state.panes.lock().unwrap().get(id).cloned().ok_or_else(|| format!("unknown pane {id}"))
}

#[tauri::command]
fn pane_write(state: State<AppState>, id: String, data: String) -> Res<()> {
    pane(&state, &id)?.write(data.as_bytes()).map_err(err)
}

#[tauri::command]
fn pane_resize(state: State<AppState>, id: String, cols: u16, rows: u16) -> Res<()> {
    pane(&state, &id)?.resize(cols, rows).map_err(err)
}

#[tauri::command]
fn pane_close(state: State<AppState>, id: String) {
    if let Some(pane) = state.panes.lock().unwrap().remove(&id) {
        pane.kill();
    }
}

/// A running pane, for a UI that loads again (a webview reload) to find its terminals.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PaneView {
    id: String,
    space_id: String,
    cwd: PathBuf,
}

#[tauri::command]
fn panes_list(state: State<AppState>) -> Vec<PaneView> {
    let panes = state.panes.lock().unwrap();
    panes.values().map(|p| PaneView { id: p.id.clone(), space_id: p.space_id.clone(), cwd: p.cwd.clone() }).collect()
}

/// Streams a running pane to a UI that loaded since, after the output it missed.
#[tauri::command]
fn pane_attach(state: State<AppState>, id: String, on_output: Channel<InvokeResponseBody>) -> Res<()> {
    if cfg!(debug_assertions) {
        eprintln!("[pane] attach {id}");
    }
    pane(&state, &id)?.attach(on_output).map_err(err)
}

/// Called when the UI loads. Panes from a previous load (a webview reload) that it doesn't attach to again have no
/// terminal left, so kill them.
#[tauri::command]
fn panes_reset(state: State<AppState>, keep: Vec<String>) {
    state.panes.lock().unwrap().retain(|id, pane| {
        let kept = keep.contains(id);
        if !kept {
            pane.kill();
        }
        kept
    });
}

#[tauri::command]
fn pane_focus(state: State<AppState>, id: Option<String>) {
    if cfg!(debug_assertions) {
        eprintln!("[focus] {id:?}");
    }
    *state.focused.lock().unwrap() = id;
}

#[tauri::command]
fn plugins_list(state: State<AppState>) -> Vec<PluginView> {
    state.plugins.lock().unwrap().list()
}

/// Packages are read whole, so a huge file is refused before it's loaded.
const PACKAGE_LIMIT: u64 = 50 * 1024 * 1024;

#[tauri::command(async)]
fn plugin_install_file(app: AppHandle, path: String) -> Res<PluginView> {
    install_file(&app, &path)
}

// The Plugins sheet and the `wings plugin` CLI (control.rs) both change plugins through these.

fn install_file(app: &AppHandle, path: &str) -> Res<PluginView> {
    let size = std::fs::metadata(path).map_err(err)?.len();
    if size > PACKAGE_LIMIT {
        return Err("The package is larger than 50 MB".into());
    }
    let bytes = std::fs::read(path).map_err(err)?;
    install(app, &bytes, Source::File, None)
}

fn install_github(app: &AppHandle, url: &str) -> Res<PluginView> {
    let repo = plugin_store::parse_repo(url).ok_or("That isn't a GitHub repo link, like github.com/owner/name")?;
    let bytes = plugin_store::download_github(&repo)?;
    install(app, &bytes, Source::Github { repo }, None)
}

/// Reinstalls a GitHub plugin from its newest release or default branch.
fn update_plugin(app: &AppHandle, id: &str) -> Res<PluginView> {
    let Some(Source::Github { repo }) = app.state::<AppState>().plugins.lock().unwrap().source(id) else {
        return Err("Only plugins installed from GitHub can update. Install the new .wings-plugin file instead.".into());
    };
    let bytes = plugin_store::download_github(&repo)?;
    install(app, &bytes, Source::Github { repo }, Some(id))
}

/// Turning a plugin on approves `shown`, the access and additions the UI showed, if they still match.
fn set_enabled(app: &AppHandle, id: &str, enabled: bool, shown: Option<Grant>) -> Res<PluginView> {
    let view = app.state::<AppState>().plugins.lock().unwrap().set_enabled(id, enabled, shown)?;
    if !enabled {
        close_panels(app, id);
    }
    tools_changed(app);
    Ok(view)
}

fn remove_plugin(app: &AppHandle, id: &str) -> Res<()> {
    close_panels(app, id);
    if let Some(old) = app.state::<AppState>().plugins.lock().unwrap().remove(id)? {
        delete_secrets(id, &old);
        delete_storage(app, id, &old);
    }
    tools_changed(app);
    Ok(())
}

/// Installs a package and deletes the secrets of a plugin it replaced from somewhere else.
fn install(app: &AppHandle, bytes: &[u8], source: Source, expected_id: Option<&str>) -> Res<PluginView> {
    let (view, dropped) = app.state::<AppState>().plugins.lock().unwrap().install(bytes, source, expected_id)?;
    if let Some(old) = dropped {
        delete_secrets(&view.manifest.id, &old);
        delete_storage(app, &view.manifest.id, &old);
    }
    tools_changed(app);
    Ok(view)
}

/// Plugins changed, so Claude Code sessions should refetch the Wings MCP tools.
fn tools_changed(app: &AppHandle) {
    #[cfg(unix)]
    mcp::notify_changed(app);
    #[cfg(not(unix))]
    let _ = app;
}

/// Tries every name. A failure only leaves an orphan: the scope is gone, so nothing can read it.
fn delete_secrets(plugin_id: &str, old: &plugin_store::SecretScope) {
    for name in &old.names {
        if let Err(e) = secrets::delete(plugin_id, &old.scope, name) {
            eprintln!("[plugins] couldn't delete {plugin_id} secret {name}: {e}");
        }
    }
}

/// A failure only leaves an orphan file: the scope is gone, so nothing reads it again.
fn delete_storage(app: &AppHandle, plugin_id: &str, old: &plugin_store::SecretScope) {
    if let Err(e) = app.state::<AppState>().storage.remove(plugin_id, &old.scope) {
        eprintln!("[plugins] couldn't delete {plugin_id} storage: {e}");
    }
}

#[tauri::command(async)]
fn plugin_install_github(app: AppHandle, url: String) -> Res<PluginView> {
    install_github(&app, &url)
}

#[tauri::command(async)]
fn plugin_update(app: AppHandle, id: String) -> Res<PluginView> {
    update_plugin(&app, &id)
}

/// The newest release version on GitHub, if the plugin came from there and the repo has releases.
#[tauri::command(async)]
fn plugin_latest_version(state: State<AppState>, id: String) -> Res<Option<String>> {
    let Some(Source::Github { repo }) = state.plugins.lock().unwrap().source(&id) else { return Ok(None) };
    plugin_store::latest_version(&repo)
}

#[tauri::command]
fn plugin_set_enabled(app: AppHandle, id: String, enabled: bool, shown: Option<Grant>) -> Res<PluginView> {
    set_enabled(&app, &id, enabled, shown)
}

#[tauri::command]
fn plugin_remove(app: AppHandle, id: String) -> Res<()> {
    remove_plugin(&app, &id)
}

/// A plugin's answer to an MCP tool call the webview handed it.
#[tauri::command]
fn mcp_tool_result(app: AppHandle, call_id: u64, text: String, is_error: bool) {
    #[cfg(unix)]
    mcp::resolve(&app, call_id, mcp::ToolResult { text, is_error });
    #[cfg(not(unix))]
    let _ = (app, call_id, text, is_error);
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct McpStatus {
    /// Claude Code's CLI is installed.
    claude: bool,
    /// The Wings MCP server is registered with it.
    connected: bool,
    /// Claude Code's status line goes through Wings, in front of yours.
    statusline: bool,
}

/// You disconnected Claude Code in the Plugins sheet, so Wings doesn't connect it again when it starts.
const CLAUDE_DISCONNECTED: &str = "claude-disconnected";

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CliStatus {
    /// Release builds on macOS and Linux. A dev build would point the command at its own binary.
    available: bool,
    /// The command is there and runs this copy of Wings.
    installed: bool,
    /// Its folder is on the login shell's PATH.
    on_path: bool,
    /// You answered the prompt on first start.
    asked: bool,
}

const CLI_ASKED: &str = "cli-prompted";

/// Whether the `wings` command is set up, for the first-start prompt and the Plugins sheet.
#[tauri::command(async)]
fn cli_status(app: AppHandle) -> CliStatus {
    let asked = app.path().app_data_dir().is_ok_and(|d| d.join(CLI_ASKED).exists());
    #[cfg(unix)]
    {
        let path = cli::command_path();
        let installed = path.as_ref().zip(std::env::current_exe().ok()).is_some_and(|(path, exe)| {
            std::fs::read_to_string(path).is_ok_and(|s| s == cli::command_script(&exe))
        });
        let dir = path.as_ref().and_then(|p| p.parent()).map(|d| d.to_string_lossy().into_owned());
        let on_path = dir.is_some_and(|dir| plugins::search_path().split(':').any(|p| p.trim_end_matches('/') == dir));
        CliStatus { available: !cfg!(debug_assertions), installed, on_path, asked }
    }
    #[cfg(not(unix))]
    CliStatus { available: false, installed: false, on_path: false, asked }
}

/// Adds the `wings` command, from the first-start prompt or the Plugins sheet.
#[tauri::command(async)]
fn cli_install(app: AppHandle) -> Res<CliStatus> {
    cli_dismiss(app.clone())?;
    #[cfg(unix)]
    cli::install_command(&std::env::current_exe().map_err(err)?)?;
    Ok(cli_status(app))
}

/// "Not now" on the first-start prompt. The Plugins sheet still offers the command.
#[tauri::command(async)]
fn cli_dismiss(app: AppHandle) -> Res<()> {
    let dir = app.path().app_data_dir().map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    std::fs::write(dir.join(CLI_ASKED), "").map_err(err)
}

/// Whether Claude Code has the Wings MCP server and status line. Reading Claude's own list keeps Wings from guessing.
#[tauri::command(async)]
fn mcp_status() -> McpStatus {
    #[cfg(unix)]
    let statusline = claude_settings::connected(&claude_settings::settings_path());
    #[cfg(not(unix))]
    let statusline = false;
    let Some(claude) = plugins::find_program("claude") else { return McpStatus { claude: false, connected: false, statusline } };
    let connected = claude_mcp(&claude, &["get", "wings"]).is_ok_and(|out| out.status.success());
    McpStatus { claude: true, connected, statusline }
}

fn claude_mcp(claude: &std::path::Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    std::process::Command::new(claude).arg("mcp").args(args).stdin(std::process::Stdio::null()).output()
}

#[tauri::command(async)]
fn mcp_connect(app: AppHandle) -> Res<()> {
    connect_claude(&app)?;
    let _ = std::fs::remove_file(app.path().app_data_dir().map_err(err)?.join(CLAUDE_DISCONNECTED));
    Ok(())
}

/// Registers `wings --mcp` with Claude Code at user scope, so every session gets the plugin tools, and puts
/// `wings statusline` in front of your status line. The server is removed first so a moved app is repointed
/// rather than left dangling.
fn connect_claude(app: &AppHandle) -> Res<()> {
    #[cfg(not(unix))]
    {
        let _ = app;
        return Err("Plugin tools aren't available on Windows yet".into());
    }
    #[cfg(unix)]
    {
        let claude = plugins::find_program("claude").ok_or("Claude Code isn't installed, or `claude` isn't on your PATH")?;
        let exe = std::env::current_exe().map_err(err)?;
        let socket = mcp::socket_path(&app.path().app_data_dir().map_err(err)?);
        let _ = claude_mcp(&claude, &["remove", "--scope", "user", "wings"]);
        let out = std::process::Command::new(&claude)
            .args(["mcp", "add", "--scope", "user", "--transport", "stdio", "wings", "--"])
            .arg(&exe)
            .arg("--mcp")
            .arg(&socket)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(err)?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        claude_settings::add(&claude_settings::settings_path(), &exe)?;
        Ok(())
    }
}

/// Opens Privacy & Security > Files & Folders, where you let Wings read another app's data.
#[tauri::command(async)]
fn privacy_open_settings() -> Res<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("/usr/bin/open").arg(privacy::SETTINGS_URL).status().map_err(err)?;
    Ok(())
}

/// Whether Wings can read a folder macOS kept from it before, once you've changed the setting.
#[tauri::command(async)]
fn privacy_check(folder: String) -> bool {
    #[cfg(target_os = "macos")]
    return privacy::readable(std::path::Path::new(&folder));
    #[cfg(not(target_os = "macos"))]
    {
        let _ = folder;
        true
    }
}

/// Takes Wings back out of Claude Code: the MCP server goes, and your own status line is put back as it was.
#[tauri::command(async)]
fn mcp_disconnect(app: AppHandle) -> Res<()> {
    #[cfg(unix)]
    {
        if let Some(claude) = plugins::find_program("claude") {
            claude_mcp(&claude, &["remove", "--scope", "user", "wings"]).map_err(err)?;
        }
        claude_settings::remove(&claude_settings::settings_path())?;
    }
    let dir = app.path().app_data_dir().map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    std::fs::write(dir.join(CLAUDE_DISCONNECTED), "").map_err(err)
}

/// Wings adds itself to Claude Code when it starts, unless you disconnected it. Only release builds: a dev build
/// would point Claude Code at its own binary.
fn connect_claude_on_start(app: AppHandle) {
    #[cfg(unix)]
    thread::spawn(move || {
        if cfg!(debug_assertions) || app.path().app_data_dir().is_ok_and(|d| d.join(CLAUDE_DISCONNECTED).exists()) {
            return;
        }
        let (Some(claude), Ok(exe)) = (plugins::find_program("claude"), std::env::current_exe()) else { return };
        let registered = claude_mcp(&claude, &["get", "wings"])
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).lines().any(|l| l.trim() == format!("Command: {}", exe.display())));
        let result = if registered { claude_settings::add(&claude_settings::settings_path(), &exe).map(|_| ()) } else { connect_claude(&app) };
        if let Err(e) = result {
            eprintln!("[claude] couldn't connect Claude Code: {e}");
        }
    });
    #[cfg(not(unix))]
    let _ = app;
}

/// Stores a secret, like an API token, in the keychain under the plugin's name. Plugins can't read it
/// back; `plugin_fetch` sends it.
#[tauri::command]
fn plugin_secret_set(state: State<AppState>, plugin_id: String, name: String, value: String) -> Res<()> {
    plugin(&state, &plugin_id)?;
    if !secrets::valid_name(&name) || value.is_empty() || value.len() > 8192 {
        return Err("A secret needs a name of letters, digits, - and _, and a value up to 8 KB".into());
    }
    let scope = state.plugins.lock().unwrap().secret_scope(&plugin_id)?.scope;
    secrets::set(&plugin_id, &scope, &name, &value)?;
    state.plugins.lock().unwrap().note_secret(&plugin_id, &name, true)
}

/// A secret the plugin stored in its current scope, if any.
fn stored_secret(state: &AppState, plugin_id: &str, name: &str) -> Res<Option<String>> {
    let scope = state.plugins.lock().unwrap().secret_scope(plugin_id)?;
    if !scope.names.iter().any(|n| n == name) {
        return Ok(None);
    }
    secrets::get(plugin_id, &scope.scope, name)
}

#[tauri::command]
fn plugin_secret_delete(state: State<AppState>, plugin_id: String, name: String) -> Res<()> {
    plugin(&state, &plugin_id)?;
    let scope = state.plugins.lock().unwrap().secret_scope(&plugin_id)?.scope;
    secrets::delete(&plugin_id, &scope, &name)?;
    state.plugins.lock().unwrap().note_secret(&plugin_id, &name, false)
}

#[tauri::command(async)]
fn plugin_secret_has(state: State<AppState>, plugin_id: String, name: String) -> Res<bool> {
    plugin(&state, &plugin_id)?;
    Ok(stored_secret(&state, &plugin_id, &name)?.is_some())
}

/// The plugin's storage scope, looked up in the same lock as the check that it's on, so a plugin removed
/// in between can't be given a new one.
fn storage_scope(state: &AppState, plugin_id: &str) -> Res<String> {
    let mut plugins = state.plugins.lock().unwrap();
    plugins.active(plugin_id).ok_or_else(|| format!("{plugin_id} is turned off or not installed"))?;
    plugins.storage_scope(plugin_id)
}

#[tauri::command(async)]
fn plugin_storage_get(state: State<AppState>, plugin_id: String, key: String) -> Res<Option<serde_json::Value>> {
    let scope = storage_scope(&state, &plugin_id)?;
    state.storage.get(&plugin_id, &scope, &key)
}

#[tauri::command(async)]
fn plugin_storage_set(state: State<AppState>, plugin_id: String, key: String, value: serde_json::Value) -> Res<()> {
    let scope = storage_scope(&state, &plugin_id)?;
    state.storage.set(&plugin_id, &scope, &key, value)
}

#[tauri::command(async)]
fn plugin_storage_delete(state: State<AppState>, plugin_id: String, key: String) -> Res<()> {
    let scope = storage_scope(&state, &plugin_id)?;
    state.storage.delete(&plugin_id, &scope, &key)
}

#[tauri::command(async)]
fn plugin_storage_keys(state: State<AppState>, plugin_id: String) -> Res<Vec<String>> {
    let scope = storage_scope(&state, &plugin_id)?;
    state.storage.keys(&plugin_id, &scope)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchRequest {
    url: String,
    method: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    body: Option<String>,
    /// A secret's name: Rust sends it as `Authorization: Bearer <secret>`.
    bearer: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FetchResponse {
    status: u16,
    content_type: Option<String>,
    body: String,
}

/// An HTTP call to a URL under the plugin's `permissions.fetch`. It doesn't follow redirects, so a
/// response can't send the request (and its token) to another host.
#[tauri::command(async)]
fn plugin_fetch(state: State<AppState>, plugin_id: String, request: FetchRequest) -> Res<FetchResponse> {
    let plugin = plugin(&state, &plugin_id)?;
    let url = plugins::fetch_url(&plugin, &request.url).ok_or_else(|| format!("{plugin_id} may not fetch {}", request.url))?;
    let method = request.method.as_deref().unwrap_or("GET").to_uppercase();
    let mut builder = tauri::http::Request::builder().method(method.as_str()).uri(&url);
    for (name, value) in &request.headers {
        if !plugins::fetch_header_ok(name) {
            return Err(format!("{plugin_id} may not set the {name} header"));
        }
        builder = builder.header(name, value);
    }
    if let Some(name) = &request.bearer {
        let token = stored_secret(&state, &plugin_id, name)?.ok_or_else(|| format!("no secret named {name}"))?;
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut response = agent
        .run(builder.body(request.body.unwrap_or_default()).map_err(err)?)
        .map_err(|e| format!("Couldn't reach {}: {e}", request.url))?;
    let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).map(str::to_string);
    let body = response.body_mut().read_to_string().map_err(err)?;
    Ok(FetchResponse { status: response.status().as_u16(), content_type, body })
}

fn panel_label(plugin_id: &str, panel_id: &str) -> String {
    format!("panel-{plugin_id}-{panel_id}")
}

fn close_panels(app: &AppHandle, plugin_id: &str) {
    let prefix = panel_label(plugin_id, "");
    for (label, window) in app.webview_windows() {
        if label.starts_with(&prefix) {
            let _ = window.close();
        }
    }
}

/// Shows or hides a plugin's panel: its web page in a small window just under the title bar button
/// (`right` and `bottom` are the button's edges in the main window, in CSS pixels). It hides when you
/// click away, and keeps its cookies, so you stay signed in.
#[tauri::command]
fn plugin_panel_toggle(app: AppHandle, state: State<AppState>, plugin_id: String, panel_id: String, right: f64, bottom: f64) -> Res<()> {
    let plugin = plugin(&state, &plugin_id)?;
    let panel = plugin.manifest.contributes.panels.iter().find(|p| p.id == panel_id).ok_or("unknown panel")?;
    let label = panel_label(&plugin_id, &panel_id);
    let main = app.get_webview_window("main").ok_or("no main window")?;
    let (width, height) = (f64::from(panel.width.unwrap_or(420).clamp(280, 900)), f64::from(panel.height.unwrap_or(640).clamp(240, 1000)));
    let scale = main.scale_factor().map_err(err)?;
    let origin = main.inner_position().map_err(err)?.to_logical::<f64>(scale);
    let (x, y) = (origin.x + right - width, origin.y + bottom + 6.0);
    if let Some(window) = app.get_webview_window(&label) {
        if window.is_visible().unwrap_or(false) {
            return window.hide().map_err(err);
        }
        window.set_position(tauri::LogicalPosition::new(x, y)).map_err(err)?;
        window.show().map_err(err)?;
        return window.set_focus().map_err(err);
    }
    let url = tauri::Url::parse(&panel.url).map_err(err)?;
    let window = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(url))
        // Only web pages: Wings' own schemes (tauri://, wings-plugin://) count as local to Tauri.
        .on_navigation(|url| url.scheme() == "https" || url.as_str() == "about:blank")
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .title(&panel.title)
        .inner_size(width, height)
        .position(x, y)
        .parent(&main)
        .map_err(err)?
        .build()
        .map_err(err)?;
    let popover = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            let _ = popover.hide();
        }
    });
    Ok(())
}

/// A plugin that's turned on and approved. Every call a plugin makes is checked here.
fn plugin(state: &AppState, id: &str) -> Res<Plugin> {
    state.plugins.lock().unwrap().active(id).ok_or_else(|| format!("{id} is turned off or not installed"))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecRequest {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    timeout_ms: Option<u64>,
    /// Send each output line to `on_output` while the program runs.
    stream: bool,
}

#[tauri::command(async)]
fn plugin_exec(state: State<AppState>, plugin_id: String, request: ExecRequest, on_output: Channel<String>) -> Res<ExecResult> {
    let ExecRequest { program, args, cwd, timeout_ms, stream } = request;
    let start = std::time::Instant::now();
    let timeout = plugins::exec_timeout(timeout_ms);
    let on_line = stream.then(|| -> plugins::OnLine {
        Arc::new(move |line: &str| {
            let _ = on_output.send(line.to_string());
        })
    });
    let result = plugins::exec(&plugin(&state, &plugin_id)?, &program, &args, cwd.as_deref().map(std::path::Path::new), timeout, on_line);
    if cfg!(debug_assertions) {
        eprintln!("[plugin] {plugin_id} {program} {} took {} ms", args.first().map(String::as_str).unwrap_or(""), start.elapsed().as_millis());
    }
    result
}

#[tauri::command(async)]
fn plugin_transcript(state: State<AppState>, plugin_id: String, session_id: String, types: Vec<String>, last: Option<usize>) -> Res<Vec<serde_json::Value>> {
    plugins::transcript_entries(&plugin(&state, &plugin_id)?, &state.claude_dir, &session_id, &types, last)
}

/// Your usage limits and each session's context and cache, as Claude Code last told `wings statusline`.
#[tauri::command]
fn plugin_statusline(state: State<AppState>, plugin_id: String) -> Res<serde_json::Value> {
    if !plugins::may_read_statusline(&plugin(&state, &plugin_id)?) {
        return Err(format!("{plugin_id} may not read the status line"));
    }
    Ok(state.statusline.lock().unwrap().view())
}

#[tauri::command]
fn plugin_open_url(app: AppHandle, state: State<AppState>, plugin_id: String, url: String) -> Res<()> {
    let plugin = plugin(&state, &plugin_id)?;
    if !plugins::may_open_url(&plugin, &url) {
        return Err(format!("{plugin_id} may not open {url}"));
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenPaneRequest {
    command: Option<String>,
    cwd: Option<String>,
    /// The project on screen, where a pane without a `cwd` opens.
    space_id: Option<String>,
}

/// Where the UI should open a plugin's pane, and what to type into its shell.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PanePlan {
    space_id: String,
    cwd: Option<String>,
    input: Option<String>,
}

/// Checks a plugin's `openPane` against its manifest and your projects. The UI then opens the pane, and
/// `pane_create` checks its folder again.
#[tauri::command(async)]
fn plugin_open_pane(state: State<AppState>, plugin_id: String, request: OpenPaneRequest) -> Res<PanePlan> {
    let plugin = plugin(&state, &plugin_id)?;
    let input = plugins::pane_input(&plugin, request.command.as_deref())?;
    let spaces = state.spaces.lock().unwrap().spaces.clone();
    let current = request.space_id.and_then(|id| spaces.iter().position(|s| s.id == id));
    let (project, cwd) = match request.cwd {
        Some(cwd) => {
            let roots: Vec<PathBuf> = spaces.iter().map(|s| PathBuf::from(&s.path)).collect();
            let (project, dir) = plugins::pane_cwd(std::path::Path::new(&cwd), &roots, current)?;
            (project, Some(dir.to_string_lossy().into_owned()))
        }
        None => (current.ok_or("No project is open in Wings")?, None),
    };
    // Typed like a resumed session, so the shell is still there when the command exits.
    Ok(PanePlan { space_id: spaces[project].id.clone(), cwd, input: input.map(|line| line + "\r") })
}

/// A plugin may move focus to a pane that's open, if it may use panes at all.
#[tauri::command]
fn plugin_focus_pane(state: State<AppState>, plugin_id: String, pane_id: String) -> Res<()> {
    if !plugins::may_use_panes(&plugin(&state, &plugin_id)?) {
        return Err(format!("{plugin_id} may not focus panes"));
    }
    pane(&state, &pane_id).map(|_| ())
}

/// A desktop notification from a plugin, up to `NOTIFY_LIMIT` a minute. Desktop notifications only show a
/// title and body, so there's no click action.
#[tauri::command]
fn plugin_notify(app: AppHandle, state: State<AppState>, plugin_id: String, title: String, body: String) -> Res<()> {
    let (title, body) = plugins::notification(&plugin(&state, &plugin_id)?, &title, &body)?;
    state.notified.lock().unwrap().allow(&plugin_id, std::time::Instant::now()).map_err(|wait| {
        format!("{plugin_id} already showed {} notifications this minute. Try again in {} s.", plugins::NOTIFY_LIMIT, wait.as_secs() + 1)
    })?;
    app.notification().builder().title(title).body(body).show().map_err(err)
}

/// Serves plugin files at `wings-plugin://localhost/<id>/<run>/<path>`. `run` only keeps WebKit's cache apart.
fn serve_plugin_file(app: &AppHandle, request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let respond = |status: u16, kind: &str, body: Vec<u8>| {
        tauri::http::Response::builder()
            .status(status)
            .header("Content-Type", kind)
            // Plugin frames are sandboxed with an opaque origin, so their script loads are cross-origin.
            .header("Access-Control-Allow-Origin", "*")
            .body(body)
            .unwrap()
    };
    let path = request.uri().path().trim_start_matches('/');
    let mut parts = path.splitn(3, '/');
    let (id, file) = (parts.next().unwrap_or_default(), parts.nth(1).unwrap_or_default());
    let state = app.state::<AppState>();
    let Some(file) = plugin(&state, id).ok().and_then(|p| plugins::resolve(&p.dir, file)) else {
        return respond(404, "text/plain", b"not found".to_vec());
    };
    match std::fs::read(&file) {
        Ok(bytes) => respond(200, plugins::content_type(&file), bytes),
        Err(_) => respond(404, "text/plain", b"not found".to_vec()),
    }
}

/// The UI's tabs and splits, saved as opaque JSON so a restart can rebuild them.
#[tauri::command]
fn workspace_load(app: AppHandle) -> Option<String> {
    std::fs::read_to_string(app.path().app_data_dir().ok()?.join("workspace.json")).ok()
}

#[tauri::command]
fn workspace_save(app: AppHandle, json: String) -> Res<()> {
    let dir = app.path().app_data_dir().map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    // Write then rename, so a crash mid-write can't leave a half-written workspace behind.
    let tmp = dir.join("workspace.json.tmp");
    std::fs::write(&tmp, json).map_err(err)?;
    std::fs::rename(tmp, dir.join("workspace.json")).map_err(err)
}

#[tauri::command]
fn agents_list(state: State<AppState>) -> Vec<Agent> {
    state.agents.lock().unwrap().clone()
}

#[tauri::command]
fn pane_info(state: State<AppState>) -> HashMap<String, PaneInfo> {
    state.pane_info.lock().unwrap().clone()
}

/// `WINGS_BENCH=1` swaps the UI for the terminal rendering benchmark in `src/bench.ts`.
#[tauri::command]
fn bench_mode() -> bool {
    std::env::var_os("WINGS_BENCH").is_some()
}

/// Prints the benchmark report (and writes it to `WINGS_BENCH_OUT` if set), then quits.
#[tauri::command]
fn bench_report(app: AppHandle, report: String) {
    println!("{report}");
    if let Some(path) = std::env::var_os("WINGS_BENCH_OUT") {
        let _ = std::fs::write(path, &report);
    }
    app.exit(0);
}

/// A system notification when an agent finishes or needs you while you're in another app.
fn notify_transitions(app: &AppHandle, before: &[Agent], after: &[Agent]) {
    let window_focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if window_focused {
        return;
    }
    for agent in after {
        let was = before.iter().find(|a| a.pane_id == agent.pane_id).map(|a| a.state);
        if was == Some(agent.state) {
            continue;
        }
        let name = agent.name.as_deref().unwrap_or("Claude");
        let project = std::path::Path::new(&agent.space_id).file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        let (title, body) = match agent.state {
            AgentState::Blocked => (
                format!("{name} needs you"),
                format!("{project}: {}", agent.waiting_for.as_deref().unwrap_or("waiting for input")),
            ),
            AgentState::Done => (format!("{name} finished"), project.into_owned()),
            _ => continue,
        };
        let mut notification = app.notification().builder().title(title).body(body);
        // macOS plays no sound unless the notification names one.
        if cfg!(target_os = "macos") {
            notification = notification.sound("NSUserNotificationDefaultSoundName");
        }
        let _ = notification.show();
    }
}

/// Polls panes for Claude processes and emits `agents` whenever the list changes.
fn start_detection(app: AppHandle) {
    thread::spawn(move || {
        let state = app.state::<AppState>();
        let mut detector = Detector::new(&state.claude_dir);
        let mut tailer = tail::Tailer::new(&state.claude_dir);
        loop {
            let probes: Vec<PaneProbe> = state
                .panes
                .lock()
                .unwrap()
                .values()
                .map(|p| PaneProbe {
                    pane_id: p.id.clone(),
                    space_id: p.space_id.clone(),
                    shell_pid: p.shell_pid,
                    foreground_pid: p.foreground_pid(),
                    title: p.title(),
                })
                .collect();
            let focused = state.focused.lock().unwrap().clone();
            let scan = detector.scan(&probes, focused.as_deref());
            let mut agents = scan.agents;
            agents.sort_by(|a, b| (&a.space_id, &a.pane_id).cmp(&(&b.space_id, &b.pane_id)));
            // Each lock is let go before anything else runs. `agents_list` and `pane_info` wait for them on the
            // main thread, `tail_transcripts` takes the agents lock again, and asking whether the window is
            // focused waits for the main thread.
            let before = {
                let mut current = state.agents.lock().unwrap();
                (*current != agents).then(|| std::mem::replace(&mut *current, agents.clone()))
            };
            if let Some(before) = before {
                notify_transitions(&app, &before, &agents);
                let _ = app.emit("agents", agents);
            }
            let info_changed = {
                let mut info = state.pane_info.lock().unwrap();
                (*info != scan.panes).then(|| *info = scan.panes.clone()).is_some()
            };
            if info_changed {
                let _ = app.emit("pane-info", scan.panes);
            }
            tail_transcripts(&app, &mut tailer);
            thread::sleep(Duration::from_millis(500));
        }
    });
}

/// Sends the running plugins the transcript entries Claude wrote since the last tick, in the sessions running in
/// panes, each to the plugins allowed its type. Reads nothing while no running plugin may read transcripts.
fn tail_transcripts(app: &AppHandle, tailer: &mut tail::Tailer) {
    let state = app.state::<AppState>();
    let running: Vec<Plugin> = {
        let store = state.plugins.lock().unwrap();
        store.list().iter().filter_map(|p| store.active(&p.manifest.id)).filter(|p| !p.manifest.permissions.transcript.is_empty()).collect()
    };
    if running.is_empty() {
        tailer.clear();
        return;
    }
    let sessions: Vec<(String, String)> =
        state.agents.lock().unwrap().iter().filter_map(|a| Some((a.pane_id.clone(), a.session_id.clone()?))).collect();
    let events: Vec<plugins::TranscriptEvent> = tailer.poll(&sessions).into_iter().filter_map(|line| plugins::transcript_event(&running, line)).collect();
    if !events.is_empty() {
        let _ = app.emit_to("main", "plugin-transcript", &events);
    }
}

#[tauri::command]
fn git_status(state: State<AppState>) -> HashMap<String, GitStatus> {
    state.git.lock().unwrap().clone()
}

/// Checks every project now, for the refresh button.
#[tauri::command(async)]
fn git_refresh(app: AppHandle) -> HashMap<String, GitStatus> {
    git_sweep(&app)
}

/// Git status for every project, keyed by space id; sends "git-status" when it changes.
fn git_sweep(app: &AppHandle) -> HashMap<String, GitStatus> {
    let state = app.state::<AppState>();
    let Some(git) = plugins::find_program("git") else { return HashMap::new() };
    let spaces = state.spaces.lock().unwrap().spaces.clone();
    let status: HashMap<String, GitStatus> =
        spaces.iter().filter_map(|s| Some((s.id.clone(), spaces::git_status(&git, std::path::Path::new(&s.path))?))).collect();
    let mut current = state.git.lock().unwrap();
    if *current != status {
        *current = status.clone();
        drop(current);
        let _ = app.emit("git-status", &status);
    }
    status
}

/// A sweep of 11 repos costs ~200 ms CPU, so it runs once a minute while Wings is focused, and right
/// away when it gets focus.
fn start_git_status(app: AppHandle) {
    thread::spawn(move || {
        let mut last: Option<std::time::Instant> = None;
        loop {
            let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
            if !focused {
                last = None;
            } else if last.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
                last = Some(std::time::Instant::now());
                git_sweep(&app);
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .menu(menu::build)
        .on_menu_event(menu::handle)
        .register_uri_scheme_protocol("wings-plugin", |ctx, request| serve_plugin_file(ctx.app_handle(), &request))
        .setup(|app| {
            let claude_dir = claude::claude_dir();
            let file = app.path().app_data_dir()?.join("spaces.json");
            let store = SpaceStore::load(file);
            let data = app.path().app_data_dir()?;
            // The example plugins in the repo, so `pnpm tauri dev` picks up edits to them.
            let dev = cfg!(debug_assertions).then(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugins")));
            let plugins = Mutex::new(Store::load(data.join("plugins"), data.join("plugins.json"), dev.as_deref()));
            app.manage(AppState {
                plugins,
                storage: plugin_storage::Storage::new(data.join("plugin-storage")),
                claude_dir,
                history: history::History::default(),
                spaces: Mutex::new(store),
                panes: Mutex::new(HashMap::new()),
                focused: Mutex::new(None),
                agents: Mutex::new(Vec::new()),
                pane_info: Mutex::new(HashMap::new()),
                git: Mutex::new(HashMap::new()),
                #[cfg(unix)]
                mcp: mcp::Mcp::default(),
                next_pane: AtomicU64::new(1),
                notified: Mutex::default(),
                statusline: Mutex::default(),
            });
            #[cfg(unix)]
            mcp::start(app.handle().clone(), mcp::socket_path(&data));
            connect_claude_on_start(app.handle().clone());
            #[cfg(target_os = "macos")]
            privacy::watch(app.handle().clone());
            if bench_mode() {
                // Keep the window on screen without taking focus, so rendering is not throttled.
                if let Some(window) = app.get_webview_window("main") {
                    window.set_always_on_top(true)?;
                }
            } else {
                start_detection(app.handle().clone());
                start_git_status(app.handle().clone());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            spaces_list,
            spaces_add,
            spaces_remove,
            sessions_list,
            history::history_search,
            pane_create,
            pane_write,
            pane_resize,
            pane_close,
            panes_reset,
            panes_list,
            pane_attach,
            pane_focus,
            agents_list,
            pane_info,
            git_status,
            git_refresh,
            workspace_load,
            workspace_save,
            plugins_list,
            plugin_install_file,
            plugin_install_github,
            plugin_update,
            plugin_latest_version,
            plugin_set_enabled,
            plugin_remove,
            plugin_panel_toggle,
            plugin_secret_set,
            plugin_secret_delete,
            plugin_secret_has,
            plugin_storage_get,
            plugin_storage_set,
            plugin_storage_delete,
            plugin_storage_keys,
            plugin_fetch,
            mcp_tool_result,
            mcp_status,
            mcp_connect,
            mcp_disconnect,
            privacy_open_settings,
            privacy_check,
            cli_status,
            cli_install,
            cli_dismiss,
            plugin_exec,
            plugin_transcript,
            plugin_statusline,
            plugin_open_url,
            plugin_open_pane,
            plugin_focus_pane,
            plugin_notify,
            bench_mode,
            bench_report,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Exit = event {
                privacy::stop();
            }
            #[cfg(not(target_os = "macos"))]
            let _ = event;
        });
}

#[cfg(test)]
mod tests {
    /// A command that isn't in build.rs and the capabilities is refused for the window at run time, while the
    /// browser stub and the MCP path don't notice. It left Past sessions loading forever once.
    #[test]
    fn every_command_is_allowed_for_the_window() {
        let lib = include_str!("lib.rs");
        let start = lib.find("generate_handler![").unwrap();
        let end = start + lib[start..].find(']').unwrap();
        let build = include_str!("../build.rs");
        let caps = include_str!("../capabilities/default.json");
        for command in lib[start + 18..end].split(',').map(|c| c.trim().rsplit("::").next().unwrap()).filter(|c| !c.is_empty()) {
            assert!(build.contains(&format!("\"{command}\"")), "{command} is missing from build.rs");
            assert!(caps.contains(&format!("\"allow-{}\"", command.replace('_', "-"))), "{command} is missing from capabilities/default.json");
        }
    }
}
