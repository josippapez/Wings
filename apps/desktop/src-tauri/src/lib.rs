mod agent_activity;
mod ai;
mod bench;
mod claude;
#[cfg(unix)]
mod claude_settings;
mod detect;
mod disk_usage;
mod editors;
mod gitwatch;
mod history;
mod hotkey;
mod menu;
mod opencode;
mod journal_sink;
mod opencode_config;
#[cfg(unix)]
pub mod bridge;
#[cfg(unix)]
pub mod cli;
#[cfg(unix)]
mod control;
// debt: the MCP socket is Unix-only, like plugins themselves; Windows needs a named pipe here.
#[cfg(unix)]
mod mcp;
#[cfg(unix)]
mod mcp_panes;
mod plugin_store;
mod plugin_storage;
mod project_info;
mod plugins;
mod ports;
#[cfg(target_os = "macos")]
mod privacy;
mod secrets;
mod pty;
mod spaces;
mod statusline;
mod tail;
mod tools;
mod worktree;

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
use detect::{Agent, AgentKind, AgentState, Detector, PaneInfo, PaneProbe};
use pty::{Pane, SpawnRequest};
use plugin_store::{Grant, PluginView, Source, Store};
use plugins::{ExecResult, Plugin};
use spaces::{GitStatus, SpaceStore, SpaceView};

struct AppState {
    ai: ai::Ai,
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
    project_info: project_info::Store,
    next_pane: AtomicU64,
    /// Recent plugin notifications, for their rate limit.
    notified: Mutex<plugins::NotifyLog>,
    /// What Claude Code last told `wings statusline`.
    statusline: Mutex<statusline::Statusline>,
    /// Folders outside its project that Claude's `open_pane` is opening a tab in, by project, while that call runs.
    pane_grants: Mutex<Vec<(String, PathBuf)>>,
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
    let view = spaces::view(&state.spaces.lock().unwrap().add(&path));
    gitwatch::resync();
    view
}

#[tauri::command(async)]
fn worktree_info(state: State<AppState>, space_id: String) -> Res<worktree::WorktreeInfo> {
    let path = state.spaces.lock().unwrap().get(&space_id).ok_or("unknown space")?.path.clone();
    worktree::info(std::path::Path::new(&path))
}

/// Makes a git worktree for a new branch and adds it to the sidebar under its project.
#[tauri::command(async)]
fn worktree_create(state: State<AppState>, space_id: String, branch: String, base: String) -> Res<SpaceView> {
    let path = state.spaces.lock().unwrap().get(&space_id).ok_or("unknown space")?.path.clone();
    let folder = worktree::create(std::path::Path::new(&path), &branch, &base)?;
    let view = spaces::view(&state.spaces.lock().unwrap().add_worktree(&folder.to_string_lossy(), &space_id));
    gitwatch::resync();
    Ok(view)
}

#[tauri::command(async)]
fn editors_list() -> Vec<editors::EditorView> {
    editors::list()
}

/// Opens one of your projects in an editor. Takes a project, not a path, so the page can't open other folders.
#[tauri::command(async)]
fn editor_open(state: State<AppState>, editor_id: String, space_id: String) -> Res<()> {
    let path = state.spaces.lock().unwrap().get(&space_id).ok_or("unknown space")?.path.clone();
    editors::open(&editor_id, std::path::Path::new(&path))
}

#[tauri::command]
fn spaces_remove(state: State<AppState>, id: String) {
    state.spaces.lock().unwrap().remove(&id);
    gitwatch::resync();
}

#[tauri::command]
fn spaces_set_pinned(state: State<AppState>, id: String, pinned: bool) {
    state.spaces.lock().unwrap().set_pinned(&id, pinned);
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
    history_id: Option<String>,
) -> Res<String> {
    let root = PathBuf::from(&state.spaces.lock().unwrap().get(&space_id).ok_or("unknown space")?.path);
    // A plugin's pane can start in a subfolder, but never outside the project it belongs to. Only a tab Claude's
    // `open_pane` asked for can.
    let cwd = match cwd {
        Some(dir) => match plugins::pane_cwd(std::path::Path::new(&dir), &[root], Some(0)) {
            Ok((_, dir)) => dir,
            Err(e) => plugins::take_grant(&mut state.pane_grants.lock().unwrap(), &space_id, std::path::Path::new(&dir)).ok_or(e)?,
        },
        None => root,
    };
    let id = format!("p{}", state.next_pane.fetch_add(1, Ordering::Relaxed));
    if cfg!(debug_assertions) {
        eprintln!("[pane] create {id} in {space_id}");
    }
    let exit_app = app.clone();
    let pane = Pane::spawn(
        SpawnRequest { id: id.clone(), space_id, cwd: &cwd, cols, rows, history: history_id.as_deref().and_then(|h| saved_history(&app, h)).map(pty::restored).unwrap_or_default(), history_id },
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

/// Runs a plugin folder where it is, or reads one that's linked again, which restarts it. Claude's sessions only
/// hear about it when the plugin's tools changed.
fn link_folder(app: &AppHandle, folder: &str) -> Res<plugin_store::Linked> {
    let linked = app.state::<AppState>().plugins.lock().unwrap().link(std::path::Path::new(folder))?;
    if linked.tools_changed {
        tools_changed(app);
    }
    Ok(linked)
}

fn install_github(app: &AppHandle, url: &str) -> Res<PluginView> {
    let (repo, folder) = plugin_store::parse_github(url).ok_or("That isn't a GitHub repo link, like github.com/owner/name or github.com/owner/name/plugins/<id>")?;
    let bytes = plugin_store::download_github(&repo, folder.as_deref())?;
    install(app, &bytes, Source::Github { repo, folder }, None)
}

/// Reinstalls a GitHub plugin from its newest release or default branch.
fn update_plugin(app: &AppHandle, id: &str) -> Res<PluginView> {
    let Some(Source::Github { repo, folder }) = app.state::<AppState>().plugins.lock().unwrap().source(id) else {
        return Err("Only plugins installed from GitHub can update. Install the new .wings-plugin file instead.".into());
    };
    let bytes = plugin_store::download_github(&repo, folder.as_deref())?;
    install(app, &bytes, Source::Github { repo, folder }, Some(id))
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

/// The version an update from GitHub would install, if the plugin came from there.
#[tauri::command(async)]
fn plugin_latest_version(state: State<AppState>, id: String) -> Res<Option<String>> {
    let Some(Source::Github { repo, folder }) = state.plugins.lock().unwrap().source(&id) else { return Ok(None) };
    plugin_store::latest_version(&repo, folder.as_deref())
}

/// The plugins in the Wings repo, installed or not, for Settings to offer.
#[tauri::command(async)]
fn plugins_available() -> Res<Vec<plugin_store::Available>> {
    if cfg!(windows) {
        return Ok(Vec::new());
    }
    plugin_store::available()
}

/// The README.md of an installed plugin, for its page in Settings. Works whether or not the plugin is on.
#[tauri::command(async)]
fn plugin_readme(state: State<AppState>, id: String) -> Res<Option<String>> {
    // Only ids of installed plugins get a path, so `id` can't point outside the plugin's own folder.
    let dir = state.plugins.lock().unwrap().dir(&id).ok_or_else(|| format!("{id} isn't installed"))?;
    Ok(read_readme(&dir))
}

const README_MAX_BYTES: u64 = 256 * 1024;

/// `README.md` (any case) from a plugin folder, if it's there, inside the folder and not over 256 KB.
fn read_readme(dir: &std::path::Path) -> Option<String> {
    let name = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_str().is_some_and(|n| n.eq_ignore_ascii_case("readme.md")))?
        .file_name();
    // A symlink out of the folder is refused.
    let path = plugins::resolve(dir, name.to_str()?)?;
    let file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > README_MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(file, README_MAX_BYTES), &mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
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
    /// The server's tools load at session start instead of waiting behind Claude's tool search.
    always_load: bool,
}

/// You disconnected Claude Code in the Plugins sheet, so Wings doesn't connect it again when it starts.
const CLAUDE_DISCONNECTED: &str = "claude-disconnected";

#[derive(serde::Serialize)]
struct OpencodeMcpStatus {
    /// OpenCode's CLI is installed.
    opencode: bool,
    /// Its global config has the Wings MCP server.
    connected: bool,
}

/// You disconnected OpenCode in the Plugins sheet, so Wings doesn't connect it again when it starts.
const OPENCODE_DISCONNECTED: &str = "opencode-disconnected";

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
    let always_load = wings_mcp_entry(&claude_user_config()).is_some_and(|e| e["alwaysLoad"] == true);
    let Some(claude) = plugins::find_program("claude") else { return McpStatus { claude: false, connected: false, statusline, always_load } };
    let connected = claude_mcp(&claude, &["get", "wings"]).is_ok_and(|out| out.status.success());
    McpStatus { claude: true, connected, statusline, always_load }
}

fn claude_mcp(claude: &std::path::Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    std::process::Command::new(claude).arg("mcp").args(args).stdin(std::process::Stdio::null()).output()
}

#[tauri::command(async)]
fn mcp_connect(app: AppHandle) -> Res<()> {
    connect_claude(&app, None)?;
    let _ = std::fs::remove_file(app.path().app_data_dir().map_err(err)?.join(CLAUDE_DISCONNECTED));
    journal_sync(&app);
    Ok(())
}

/// Re-registers the server with `alwaysLoad` on or off. Claude Code defers MCP tools behind tool search, so
/// without it Claude only reaches for a Wings tool when something tells it to look.
#[tauri::command(async)]
fn mcp_set_always_load(app: AppHandle, on: bool) -> Res<()> {
    connect_claude(&app, Some(on))
}

/// Claude Code's user-scope config, where `mcpServers` lives. Read only: every write goes through `claude mcp`.
fn claude_user_config() -> std::path::PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => std::path::PathBuf::from(dir).join(".claude.json"),
        None => std::env::home_dir().unwrap_or_default().join(".claude.json"),
    }
}

/// The JSON for re-adding the Wings server with the fields you set on it kept, and `alwaysLoad` set when given,
/// or `None` when there are none and a plain `claude mcp add` will do. Wings owns `type`, `command`, `args` and `env`.
fn mcp_add_json(existing: Option<&serde_json::Value>, always_load: Option<bool>, exe: &std::path::Path, socket: &std::path::Path) -> Option<serde_json::Value> {
    let mut entry = existing.and_then(|e| e.as_object()).cloned().unwrap_or_default();
    for owned in ["type", "command", "args", "env"] {
        entry.remove(owned);
    }
    match always_load {
        Some(true) => _ = entry.insert("alwaysLoad".into(), true.into()),
        Some(false) => _ = entry.remove("alwaysLoad"),
        None => {}
    }
    if entry.is_empty() {
        return None;
    }
    entry.insert("type".into(), "stdio".into());
    entry.insert("command".into(), exe.to_string_lossy().into());
    entry.insert("args".into(), serde_json::json!(["--mcp", socket.to_string_lossy()]));
    entry.insert("env".into(), serde_json::json!({}));
    Some(entry.into())
}

fn wings_mcp_entry(config: &std::path::Path) -> Option<serde_json::Value> {
    let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(config).ok()?).ok()?;
    config.pointer("/mcpServers/wings").cloned()
}

/// Registers `wings --mcp` with Claude Code at user scope, so every session gets the plugin tools, and puts
/// `wings statusline` in front of your status line. The server is removed first so a moved app is repointed
/// rather than left dangling.
fn connect_claude(app: &AppHandle, always_load: Option<bool>) -> Res<()> {
    #[cfg(not(unix))]
    {
        let _ = (app, always_load);
        return Err("Plugin tools aren't available on Windows yet".into());
    }
    #[cfg(unix)]
    {
        let claude = plugins::find_program("claude").ok_or("Claude Code isn't installed, or `claude` isn't on your PATH")?;
        let exe = std::env::current_exe().map_err(err)?;
        let socket = mcp::socket_path(&app.path().app_data_dir().map_err(err)?);
        // Read before the remove, which drops the entry: what you set on it yourself (`alwaysLoad`, say) goes back on.
        let kept = mcp_add_json(wings_mcp_entry(&claude_user_config()).as_ref(), always_load, &exe, &socket);
        let _ = claude_mcp(&claude, &["remove", "--scope", "user", "wings"]);
        let mut add = std::process::Command::new(&claude);
        match &kept {
            Some(entry) => add.args(["mcp", "add-json", "--scope", "user", "wings", &entry.to_string()]),
            None => add.args(["mcp", "add", "--scope", "user", "--transport", "stdio", "wings", "--"]).arg(&exe).arg("--mcp").arg(&socket),
        };
        let out = add.stdin(std::process::Stdio::null()).output().map_err(err)?;
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
    std::fs::write(dir.join(CLAUDE_DISCONNECTED), "").map_err(err)?;
    journal_sync(&app);
    Ok(())
}

/// Whether this is an installed app, `<name>.app/Contents/MacOS/<binary>`, the only copy that may write itself into
/// the agents' configs on start. A release binary run from `target/` would point them at the build folder.
fn installed_app(exe: &std::path::Path) -> bool {
    let mut parents = exe.ancestors().skip(1);
    let (macos, contents, bundle) = (parents.next(), parents.next(), parents.next());
    macos.and_then(|p| p.file_name()).is_some_and(|n| n == "MacOS")
        && contents.and_then(|p| p.file_name()).is_some_and(|n| n == "Contents")
        && bundle.and_then(|p| p.extension()).is_some_and(|e| e == "app")
}

/// The binary to write into the agents' configs on start, or `None` when this isn't the installed app.
fn exe_to_connect() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok().filter(|exe| installed_app(exe))
}

/// Wings adds itself to Claude Code when it starts, unless you disconnected it. Only the installed app: a dev build
/// or a binary in `target/` would point Claude Code at itself.
fn connect_claude_on_start(app: AppHandle) {
    #[cfg(unix)]
    thread::spawn(move || {
        if cfg!(debug_assertions) || app.path().app_data_dir().is_ok_and(|d| d.join(CLAUDE_DISCONNECTED).exists()) {
            return;
        }
        let (Some(claude), Some(exe)) = (plugins::find_program("claude"), exe_to_connect()) else { return };
        let registered = claude_mcp(&claude, &["get", "wings"])
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).lines().any(|l| l.trim() == format!("Command: {}", exe.display())));
        let result = if registered { claude_settings::add(&claude_settings::settings_path(), &exe).map(|_| ()) } else { connect_claude(&app, None) };
        if let Err(e) = result {
            eprintln!("[claude] couldn't connect Claude Code: {e}");
        }
    });
    #[cfg(not(unix))]
    let _ = app;
}

/// Keeps Wings registered as the agents' change journal sink while either agent is connected: it adds itself, or
/// repoints its own entry after the app moved, and takes its own entry out once you have disconnected both. A sink
/// of yours is never touched. Run on start (release builds only, like the agent connections) and after each
/// Connect or Disconnect.
fn journal_sync(app: &AppHandle) {
    #[cfg(unix)]
    {
        let Ok(dir) = app.path().app_data_dir() else { return };
        let both_off = dir.join(CLAUDE_DISCONNECTED).exists() && dir.join(OPENCODE_DISCONNECTED).exists();
        let path = journal_sink::sink_path();
        // Only the installed app: a release build run from `target/` would point the journal at the build folder.
        let result = match exe_to_connect() {
            _ if both_off => journal_sink::remove(&path),
            Some(exe) => journal_sink::add(&path, &exe),
            None => return,
        };
        if let Err(e) = result {
            eprintln!("[journal] couldn't set the journal sink: {e}");
        }
    }
    #[cfg(not(unix))]
    let _ = app;
}

/// Whether OpenCode is installed and its global config has the Wings MCP server.
#[tauri::command(async)]
fn opencode_mcp_status() -> OpencodeMcpStatus {
    OpencodeMcpStatus { opencode: plugins::find_program("opencode").is_some(), connected: opencode_config::entry(&opencode_config::config_path()).is_some() }
}

#[tauri::command(async)]
fn opencode_mcp_connect(app: AppHandle) -> Res<()> {
    connect_opencode(&app)?;
    let _ = std::fs::remove_file(app.path().app_data_dir().map_err(err)?.join(OPENCODE_DISCONNECTED));
    journal_sync(&app);
    Ok(())
}

/// Takes the Wings server out of OpenCode's config, and keeps Wings from adding it back when it starts.
#[tauri::command(async)]
fn opencode_mcp_disconnect(app: AppHandle) -> Res<()> {
    if opencode_config::remove(&opencode_config::config_path())? {
        reload_opencode();
    }
    let dir = app.path().app_data_dir().map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    std::fs::write(dir.join(OPENCODE_DISCONNECTED), "").map_err(err)?;
    journal_sync(&app);
    Ok(())
}

/// Points the Wings server in OpenCode's global config at this binary.
fn connect_opencode(app: &AppHandle) -> Res<()> {
    #[cfg(not(unix))]
    {
        let _ = app;
        return Err("Plugin tools aren't available on Windows yet".into());
    }
    #[cfg(unix)]
    {
        let exe = std::env::current_exe().map_err(err)?;
        let socket = mcp::socket_path(&app.path().app_data_dir().map_err(err)?);
        if opencode_config::add(&opencode_config::config_path(), &exe, &socket)? {
            reload_opencode();
        }
        Ok(())
    }
}

/// OpenCode's background service only reads its config on start or on reload. Without a running service there's
/// nothing to reload, and the next one reads the file.
fn reload_opencode() {
    let Some(opencode) = plugins::find_program("opencode") else { return };
    let out = std::process::Command::new(opencode).arg("reload").current_dir(std::env::home_dir().unwrap_or_default()).stdin(std::process::Stdio::null()).output();
    match out {
        Ok(out) if !out.status.success() => eprintln!("[opencode] reload failed: {}", String::from_utf8_lossy(&out.stderr).trim()),
        Err(e) => eprintln!("[opencode] reload failed: {e}"),
        Ok(_) => {}
    }
}

/// Wings adds itself to OpenCode when it starts, unless you disconnected it or it already points at this binary.
/// Only release builds: a dev build would point OpenCode at its own binary.
fn connect_opencode_on_start(app: AppHandle) {
    #[cfg(unix)]
    thread::spawn(move || {
        if cfg!(debug_assertions) || app.path().app_data_dir().is_ok_and(|d| d.join(OPENCODE_DISCONNECTED).exists()) {
            return;
        }
        let (Some(_), Some(exe)) = (plugins::find_program("opencode"), exe_to_connect()) else { return };
        if opencode_config::points_at(&opencode_config::config_path(), &exe) {
            return;
        }
        if let Err(e) = connect_opencode(&app) {
            eprintln!("[opencode] couldn't connect OpenCode: {e}");
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

// The Project page's data (project_info.rs). Async like the commands that touch disk.
#[tauri::command(async)]
fn project_info_get(state: State<AppState>, project_id: String) -> Res<project_info::Info> {
    state.project_info.get(&project_id)
}

#[tauri::command(async)]
fn project_info_apply(app: AppHandle, state: State<AppState>, project_id: String, op: project_info::Op) -> Res<project_info::Info> {
    let info = state.project_info.apply(&project_id, op)?;
    let _ = app.emit("project-info-changed", serde_json::json!({ "projectId": project_id }));
    Ok(info)
}

/// The image's bytes are the request body, so a 5 MB image isn't sent as a JSON array. Headers: `x-project-id`
/// (URI-encoded, ids are paths) and `x-ext`. Returns the image's name for `![](wings-image:<name>)`.
#[tauri::command(async)]
fn project_info_add_image(state: State<AppState>, request: tauri::ipc::Request<'_>) -> Res<String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("Send the image as bytes.".into()) };
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let id = header("x-project-id").ok_or("x-project-id is missing")?;
    let id = project_info::percent_decode(&id)?;
    state.project_info.add_image(&id, bytes, &header("x-ext").unwrap_or_default())
}

/// An image's bytes. The asset protocol isn't enabled, so the page turns these into a blob URL.
#[tauri::command(async)]
fn project_info_image(state: State<AppState>, project_id: String, name: String) -> Res<tauri::ipc::Response> {
    state.project_info.image(&project_id, &name).map(tauri::ipc::Response::new)
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

#[tauri::command(async)]
fn plugin_storage_entries(state: State<AppState>, plugin_id: String, prefix: String) -> Res<Vec<(String, serde_json::Value)>> {
    let scope = storage_scope(&state, &plugin_id)?;
    state.storage.entries(&plugin_id, &scope, &prefix)
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
    let method = request.method.as_deref().unwrap_or("GET").to_uppercase();
    let url = plugins::fetch_url(&plugin, &method, &request.url, request.body.as_deref()).ok_or_else(|| format!("{plugin_id} may not {method} {}", request.url))?;
    let mut builder = tauri::http::Request::builder().method(method.as_str()).uri(&url);
    let json_only = plugins::fetch_needs_json(&plugin, &method, &url);
    for (name, value) in &request.headers {
        if !plugins::fetch_header_ok(name) {
            return Err(format!("{plugin_id} may not set the {name} header"));
        }
        if json_only && name.eq_ignore_ascii_case("content-type") {
            if !value.trim().eq_ignore_ascii_case("application/json") {
                return Err(format!("{plugin_id} must send this body as application/json"));
            }
            continue;
        }
        builder = builder.header(name, value);
    }
    if json_only {
        builder = builder.header("Content-Type", "application/json");
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

/// Plugin views opened as windows. `:` can't be in an id, so no two plugins' views share a label.
fn view_label(plugin_id: &str, view_id: &str) -> String {
    format!("{VIEW_WINDOW}{plugin_id}:{view_id}")
}

/// The label prefix of plugin view windows, which `capabilities/plugin-view.json` matches.
const VIEW_WINDOW: &str = "view:";

/// Closes a plugin's panels and view windows.
fn close_panels(app: &AppHandle, plugin_id: &str) {
    let (panels, views) = (panel_label(plugin_id, ""), view_label(plugin_id, ""));
    for (label, window) in app.webview_windows() {
        if label.starts_with(&panels) || label.starts_with(&views) {
            let _ = window.close();
        }
    }
}

/// Opens a plugin view in a window of its own, or brings it to the front. The window loads the Wings UI,
/// which sees its label and shows only the plugin's sandboxed page. That page's calls are sent to the main
/// window's plugin host, which checks them like any other, so this window's capability allows events only.
/// The window plugin remembers its size and position by label.
#[tauri::command]
fn plugin_view_open(app: AppHandle, state: State<AppState>, plugin_id: String, view_id: String) -> Res<String> {
    let plugin = plugin(&state, &plugin_id)?;
    let view = plugin.manifest.contributes.views.iter().find(|v| v.id == view_id).ok_or("unknown view")?;
    if !view.opens.iter().any(|o| o == "window") {
        return Err(format!("{view_id} doesn't list \"window\" in opens"));
    }
    let label = view_label(&plugin_id, &view_id);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        window.show().map_err(err)?;
        window.set_focus().map_err(err)?;
        return Ok(label);
    }
    let (width, height) = (f64::from(view.width.unwrap_or(520).clamp(280, 1600)), f64::from(view.height.unwrap_or(640).clamp(200, 1200)));
    let window = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()))
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .title(&view.title)
        .inner_size(width, height)
        .min_inner_size(280.0, 200.0)
        .center()
        .build()
        .map_err(err)?;
    let (app, closed) = (app.clone(), label.clone());
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            let _ = app.emit_to("main", "plugin-view:closed", &closed);
        }
    });
    Ok(label)
}

#[tauri::command]
fn plugin_view_close(app: AppHandle, plugin_id: String, view_id: String) {
    if let Some(window) = app.get_webview_window(&view_label(&plugin_id, &view_id)) {
        let _ = window.close();
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

/// What an OpenCode session is doing, if the plugin may read it. The service address and password stay here.
#[tauri::command(async)]
fn plugin_agent_activity(state: State<AppState>, plugin_id: String, session_id: String) -> Res<agent_activity::Activity> {
    if !plugins::may_read_agent_activity(&plugin(&state, &plugin_id)?) {
        return Err(format!("{plugin_id} may not read agent activity"));
    }
    let start = std::time::Instant::now();
    let activity = agent_activity::fetch_live(&session_id);
    if cfg!(debug_assertions) {
        eprintln!("[plugin] {plugin_id} agentActivity took {} ms", start.elapsed().as_millis());
    }
    activity
}

/// Your usage limits and each session's context and cache, as Claude Code last told `wings statusline`.
#[tauri::command]
fn plugin_statusline(state: State<AppState>, plugin_id: String) -> Res<serde_json::Value> {
    if !plugins::may_read_statusline(&plugin(&state, &plugin_id)?) {
        return Err(format!("{plugin_id} may not read the status line"));
    }
    Ok(state.statusline.lock().unwrap().view())
}

/// The local ports each pane's programs listen on, if the plugin may read them.
#[tauri::command(async)]
fn plugin_ports(state: State<AppState>, plugin_id: String) -> Res<Vec<ports::PanePort>> {
    if !plugins::may_read_ports(&plugin(&state, &plugin_id)?) {
        return Err(format!("{plugin_id} may not read listening ports"));
    }
    let panes: Vec<(String, Option<u32>)> = state.panes.lock().unwrap().values().map(|p| (p.id.clone(), p.shell_pid)).collect();
    let start = std::time::Instant::now();
    let found = ports::scan(&panes);
    if cfg!(debug_assertions) {
        eprintln!("[plugin] {plugin_id} ports took {} ms", start.elapsed().as_millis());
    }
    found
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiskUsageRequest {
    path: String,
    min_bytes: Option<u64>,
    depth: Option<u32>,
    max_entries: Option<usize>,
    /// Names the scan, so `plugin_disk_usage_cancel` can stop it.
    token: String,
    /// Send how far the scan has got to `on_progress`, as JSON.
    stream: bool,
}

/// Scans a folder or a disk for what takes the space, if the plugin may read disk usage.
#[tauri::command(async)]
fn plugin_disk_usage(state: State<AppState>, plugin_id: String, request: DiskUsageRequest, on_progress: Channel<String>) -> Res<disk_usage::Scan> {
    if !plugins::may_scan_disks(&plugin(&state, &plugin_id)?) {
        return Err(format!("{plugin_id} may not read disk usage"));
    }
    let DiskUsageRequest { path, min_bytes, depth, max_entries, token, stream } = request;
    let defaults = disk_usage::Options::default();
    let options = disk_usage::Options {
        min_bytes: min_bytes.unwrap_or(defaults.min_bytes),
        depth: depth.map_or(defaults.depth, |d| d.min(disk_usage::MAX_DEPTH)),
        max_entries: max_entries.map_or(defaults.max_entries, |n| n.clamp(1, disk_usage::MAX_ENTRIES)),
    };
    let start = std::time::Instant::now();
    let running = disk_usage::Running::new(format!("{plugin_id}:{token}"));
    let scan = disk_usage::scan(&path, options, running.flag(), |progress| {
        if stream {
            if let Ok(line) = serde_json::to_string(progress) {
                let _ = on_progress.send(line);
            }
        }
    });
    if cfg!(debug_assertions) {
        eprintln!("[plugin] {plugin_id} diskUsage took {} ms", start.elapsed().as_millis());
    }
    scan
}

/// Stops a scan the plugin started with that token. One that has already finished is nothing to stop.
#[tauri::command]
fn plugin_disk_usage_cancel(state: State<AppState>, plugin_id: String, token: String) -> Res<()> {
    if !plugins::may_scan_disks(&plugin(&state, &plugin_id)?) {
        return Err(format!("{plugin_id} may not read disk usage"));
    }
    disk_usage::stop(&format!("{plugin_id}:{token}"));
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiskFindRequest {
    path: String,
    names: Vec<String>,
    skip: Vec<String>,
    /// Names the search, so `plugin_disk_usage_cancel` can stop it.
    token: String,
    /// Send how far the search has got to `on_progress`, as JSON.
    stream: bool,
}

/// Looks under a folder or a disk for entries with certain names, if the plugin may read disk usage.
#[tauri::command(async)]
fn plugin_disk_find(state: State<AppState>, plugin_id: String, request: DiskFindRequest, on_progress: Channel<String>) -> Res<disk_usage::Found> {
    if !plugins::may_scan_disks(&plugin(&state, &plugin_id)?) {
        return Err(format!("{plugin_id} may not read disk usage"));
    }
    let DiskFindRequest { path, names, skip, token, stream } = request;
    let start = std::time::Instant::now();
    let running = disk_usage::Running::new(format!("{plugin_id}:{token}"));
    let found = disk_usage::find(&path, &names, &skip, running.flag(), |progress| {
        if stream {
            if let Ok(line) = serde_json::to_string(progress) {
                let _ = on_progress.send(line);
            }
        }
    });
    if cfg!(debug_assertions) {
        eprintln!("[plugin] {plugin_id} diskFind took {} ms", start.elapsed().as_millis());
    }
    found
}

/// The mounted disks, if the plugin may read disk usage.
#[tauri::command(async)]
fn plugin_disks(state: State<AppState>, plugin_id: String) -> Res<Vec<disk_usage::Disk>> {
    if !plugins::may_scan_disks(&plugin(&state, &plugin_id)?) {
        return Err(format!("{plugin_id} may not read disk usage"));
    }
    let start = std::time::Instant::now();
    let found = disk_usage::disks();
    if cfg!(debug_assertions) {
        eprintln!("[plugin] {plugin_id} disks took {} ms", start.elapsed().as_millis());
    }
    Ok(found)
}

#[tauri::command]
fn plugin_open_url(app: AppHandle, state: State<AppState>, plugin_id: String, url: String) -> Res<()> {
    let plugin = plugin(&state, &plugin_id)?;
    if !plugins::may_open_url(&plugin, &url) {
        return Err(format!("{plugin_id} may not open {url}"));
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

#[tauri::command(async)]
fn plugin_post(state: State<AppState>, plugin_id: String, request: plugins::PostRequest) -> Res<ExecResult> {
    plugins::post(&plugin(&state, &plugin_id)?, &request)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenPaneRequest {
    command: Option<String>,
    /// Values after the command, each one shell word even with spaces in it.
    #[serde(default)]
    args: Vec<String>,
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
    let input = plugins::pane_input(&plugin, request.command.as_deref(), &request.args)?;
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

/// The command-line tools an installed plugin runs, on or off, and whether each is installed.
#[tauri::command(async)]
fn plugin_tools(state: State<AppState>, id: String) -> Res<Vec<tools::ToolStatus>> {
    let view = state.plugins.lock().unwrap().view(&id).ok_or_else(|| format!("{id} isn't installed"))?;
    Ok(tools::statuses(&view.manifest.permissions, &plugins::find_program, cfg!(target_os = "macos")))
}

/// A plugin asks Wings to install a tool it runs. The UI then asks you, and on yes runs the table's own command.
#[tauri::command(async)]
fn plugin_tool(state: State<AppState>, plugin_id: String, program: String) -> Res<tools::ToolStatus> {
    let plugin = plugin(&state, &plugin_id)?;
    tools::requested(&plugin_id, &plugin.manifest.permissions, &program, &plugins::find_program, cfg!(target_os = "macos"))
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

/// The file a pane's output is kept in across a restart. The id comes from the UI, so only a plain name is one.
fn history_file(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let plain = !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    plain.then(|| app.path().app_data_dir().ok().map(|d| d.join("scrollback").join(id))).flatten()
}

fn saved_history(app: &AppHandle, id: &str) -> Option<Vec<u8>> {
    std::fs::read(history_file(app, id)?).ok()
}

/// A pane's output from before the restart, for the UI to write before the new shell starts.
#[tauri::command]
fn history_load(app: AppHandle, id: String) -> tauri::ipc::Response {
    tauri::ipc::Response::new(saved_history(&app, &id).map(pty::restored).unwrap_or_default())
}

/// On quit: each pane's kept output, for the same pane when Wings starts again.
fn save_histories(app: &AppHandle) {
    let panes: Vec<Arc<Pane>> = app.state::<AppState>().panes.lock().unwrap().values().cloned().collect();
    for pane in panes {
        let Some(file) = pane.history_id.as_deref().and_then(|id| history_file(app, id)) else { continue };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, pane.recent_output());
    }
}

/// Called when the UI loads: deletes saved output no pane in the saved workspace will show.
#[tauri::command]
fn histories_keep(app: AppHandle, keep: Vec<String>) {
    let Ok(dir) = app.path().app_data_dir().map(|d| d.join("scrollback")) else { return };
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if !keep.iter().any(|k| entry.file_name() == k.as_str()) {
            let _ = std::fs::remove_file(entry.path());
        }
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

/// Whether an agent process has a child doing work, asked right before Wings puts an idle agent to sleep.
#[tauri::command]
async fn agent_busy(pid: u32) -> bool {
    detect::agent_busy(pid)
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
        let kind_name = if agent.kind == AgentKind::Opencode { "opencode" } else { "Claude" };
        let name = agent.name.as_deref().unwrap_or(kind_name);
        let project = std::path::Path::new(&agent.space_id).file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        let (title, body) = match agent.state {
            AgentState::Blocked => (
                format!("{name} needs you"),
                format!("{project}: {}", agent.waiting_for.as_deref().unwrap_or("waiting for input")),
            ),
            AgentState::Done => (format!("{name} finished"), project.into_owned()),
            _ => continue,
        };
        send_notification(app, &agent.pane_id, title, body);
    }
}

/// How many notifications wait for a click at once on macOS. Each waits on its own thread until it's clicked,
/// dismissed or cleared from Notification Center, which can be never, so past this a notification is sent
/// without waiting and a click on it only brings Wings forward.
#[cfg(target_os = "macos")]
const MAX_CLICK_WAITS: usize = 8;

/// Counts the threads waiting for a notification click, so there are never more than a given number.
struct ClickWaits(std::sync::atomic::AtomicUsize);

/// One of the waits, given back on drop.
struct ClickWait<'a>(&'a ClickWaits);

impl ClickWaits {
    fn take(&self, max: usize) -> Option<ClickWait<'_>> {
        use std::sync::atomic::Ordering::{AcqRel, Acquire};
        self.0.try_update(AcqRel, Acquire, |n| (n < max).then_some(n + 1)).ok().map(|_| ClickWait(self))
    }
}

impl Drop for ClickWait<'_> {
    fn drop(&mut self) {
        self.0 .0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

#[cfg(target_os = "macos")]
static CLICK_WAITS: ClickWaits = ClickWaits(std::sync::atomic::AtomicUsize::new(0));

/// Brings Wings to the front and tells the UI to go to a pane.
#[cfg(target_os = "macos")]
fn focus_pane(app: &AppHandle, pane_id: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit("focus-pane", pane_id);
}

/// Shows a system notification for a pane. On macOS, clicking it focuses that pane: `mac-notification-sys`
/// reports a click only to a send that waits, so that one waits on its own thread. Elsewhere the plugin shows
/// it and a click does what the OS does for it.
fn send_notification(app: &AppHandle, pane_id: &str, title: String, body: String) {
    #[cfg(target_os = "macos")]
    if let Some(wait) = CLICK_WAITS.take(MAX_CLICK_WAITS) {
        let (app, pane_id) = (app.clone(), pane_id.to_string());
        let identifier = if tauri::is_dev() { "com.apple.Terminal".to_string() } else { app.config().identifier.clone() };
        thread::spawn(move || {
            let _wait = wait;
            let _ = notify_rust::set_application(&identifier);
            let shown = notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                // No system sound: the web view plays the chosen one (`notificationSound`), so it isn't doubled.
                .show();
            if let Ok(handle) = shown {
                let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
                    if matches!(response, notify_rust::NotificationResponse::Default) {
                        focus_pane(&app, &pane_id);
                    }
                });
            }
        });
        return;
    }
    let _ = pane_id;
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Polls panes for Claude processes and emits `agents` whenever the list changes.
fn start_detection(app: AppHandle) {
    thread::spawn(move || {
        let state = app.state::<AppState>();
        let mut detector = Detector::new(&state.claude_dir);
        let mut tailer = tail::Tailer::new(&state.claude_dir);
        opencode::spawn(detector.opencode());
        forward_agent_activity(&app, &detector.opencode());
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
            state.ai.observe(&mut agents);
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

/// Tells the running plugins that may read agent activity which OpenCode session just did something. Tool calls
/// come in bursts, so each burst is one event per session, sent a moment after it starts.
fn forward_agent_activity(app: &AppHandle, opencode: &opencode::Shared) {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    opencode.on_activity(move |session| {
        let _ = tx.send(session.to_string());
    });
    let app = app.clone();
    thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            thread::sleep(Duration::from_millis(700));
            let mut sessions = vec![first];
            sessions.extend(rx.try_iter());
            sessions.sort();
            sessions.dedup();
            let state = app.state::<AppState>();
            let plugins: Vec<String> = {
                let store = state.plugins.lock().unwrap();
                store.list().iter().filter_map(|p| store.active(&p.manifest.id)).filter(plugins::may_read_agent_activity).map(|p| p.manifest.id.clone()).collect()
            };
            if plugins.is_empty() {
                continue;
            }
            let events: Vec<plugins::AgentActivityEvent> = sessions.into_iter().map(|session_id| plugins::AgentActivityEvent { session_id, plugins: plugins.clone() }).collect();
            let _ = app.emit_to("main", "plugin-agent-activity", &events);
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
        state.agents.lock().unwrap().iter().filter(|a| a.kind == AgentKind::Claude).filter_map(|a| Some((a.pane_id.clone(), a.session_id.clone()?))).collect();
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

/// Checks only the given projects, for when their repo just changed. Sends "git-status" when that changes the result.
fn git_refresh_spaces(app: &AppHandle, ids: &[String]) {
    let state = app.state::<AppState>();
    let Some(git) = plugins::find_program("git") else { return };
    let spaces: Vec<_> = state.spaces.lock().unwrap().spaces.iter().filter(|s| ids.contains(&s.id)).cloned().collect();
    let fresh: Vec<_> = spaces.iter().map(|s| (s.id.clone(), spaces::git_status(&git, std::path::Path::new(&s.path)))).collect();
    let mut current = state.git.lock().unwrap();
    let before = current.clone();
    for (id, status) in fresh {
        match status {
            Some(status) => current.insert(id, status),
            None => current.remove(&id),
        };
    }
    if *current != before {
        let all = current.clone();
        drop(current);
        let _ = app.emit("git-status", &all);
    }
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
    let mut builder = tauri::Builder::default();
    // A benchmark run would save its window size over yours.
    if !bench_mode() {
        builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // No shortcut until the window turns the setting on (`hotkey::hotkey_set`).
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .menu(menu::build)
        .on_menu_event(menu::handle)
        .register_uri_scheme_protocol("wings-plugin", |ctx, request| serve_plugin_file(ctx.app_handle(), &request))
        // A view window only relays to the main window's plugin host, so it goes when that does.
        .on_window_event(|window, event| {
            // Plugins pause their polling while no Wings window is in front. A view window or popover counts.
            if matches!(event, WindowEvent::Focused(_)) {
                let app = window.app_handle();
                let any = app.webview_windows().values().any(|w| w.is_focused().unwrap_or(false));
                let _ = app.emit_to("main", "wings-focus", any);
            }
            if window.label() == "main" && matches!(event, WindowEvent::Destroyed) {
                for (label, view) in window.app_handle().webview_windows() {
                    if label.starts_with(VIEW_WINDOW) {
                        let _ = view.close();
                    }
                }
            }
        })
        .setup(|app| {
            let claude_dir = claude::claude_dir();
            let file = app.path().app_data_dir()?.join("spaces.json");
            let store = SpaceStore::load(file);
            let data = app.path().app_data_dir()?;
            // The example plugins in the repo, so `pnpm tauri dev` picks up edits to them.
            let dev = cfg!(debug_assertions).then(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugins")));
            let plugins = Mutex::new(Store::load(data.join("plugins"), data.join("plugins.json"), dev.as_deref()));
            app.manage(AppState {
                ai: ai::Ai::new(data.join("ai")),
                plugins,
                storage: plugin_storage::Storage::new(data.join("plugin-storage")),
                project_info: project_info::Store::new(data.join("projects"), data.join("plugin-storage")),
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
                pane_grants: Mutex::default(),
            });
            if bench_mode() {
                // Keep the window on screen without taking focus, so rendering is not throttled. A fixed size makes
                // runs comparable.
                if let Some(window) = app.get_webview_window("main") {
                    window.set_always_on_top(true)?;
                    window.set_size(tauri::LogicalSize::new(1440.0, 900.0))?;
                    // `WINGS_BENCH_DPR=2` opens it on a Retina screen, `1` on a standard one.
                    let dpr = std::env::var("WINGS_BENCH_DPR").ok().and_then(|v| v.parse::<f64>().ok());
                    if let Some(screen) = dpr.and_then(|dpr| window.available_monitors().ok()?.into_iter().find(|m| m.scale_factor() == dpr)) {
                        window.set_position(*screen.position())?;
                    }
                }
            } else {
                // Not while benchmarking: the MCP socket would replace the one of the Wings you're using.
                #[cfg(unix)]
                mcp::start(app.handle().clone(), mcp::socket_path(&data));
                connect_claude_on_start(app.handle().clone());
                connect_opencode_on_start(app.handle().clone());
                if !cfg!(debug_assertions) && exe_to_connect().is_some() {
                    let handle = app.handle().clone();
                    thread::spawn(move || journal_sync(&handle));
                }
                #[cfg(target_os = "macos")]
                privacy::watch(app.handle().clone());
                ai::start(app.handle().clone());
                start_detection(app.handle().clone());
                start_git_status(app.handle().clone());
                gitwatch::start(app.handle().clone());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ai::ai_status,
            ai::ai_set,
            project_info_get,
            project_info_apply,
            project_info_add_image,
            project_info_image,
            spaces_list,
            spaces_add,
            spaces_remove,
            spaces_set_pinned,
            worktree_info,
            worktree_create,
            editors_list,
            editor_open,
            sessions_list,
            history::history_search,
            hotkey::hotkey_set,
            pane_create,
            pane_write,
            pane_resize,
            pane_close,
            panes_reset,
            histories_keep,
            history_load,
            panes_list,
            pane_attach,
            pane_focus,
            agents_list,
            agent_busy,
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
            plugins_available,
            plugin_readme,
            plugin_set_enabled,
            plugin_remove,
            plugin_panel_toggle,
            plugin_view_open,
            plugin_view_close,
            plugin_secret_set,
            plugin_secret_delete,
            plugin_secret_has,
            plugin_storage_get,
            plugin_storage_set,
            plugin_storage_delete,
            plugin_storage_keys,
            plugin_storage_entries,
            plugin_fetch,
            mcp_tool_result,
            mcp_status,
            mcp_connect,
            mcp_set_always_load,
            mcp_disconnect,
            opencode_mcp_status,
            opencode_mcp_connect,
            opencode_mcp_disconnect,
            privacy_open_settings,
            privacy_check,
            cli_status,
            cli_install,
            cli_dismiss,
            plugin_exec,
            plugin_post,
            plugin_transcript,
            plugin_statusline,
            plugin_ports,
            plugin_agent_activity,
            plugin_disk_usage,
            plugin_disk_usage_cancel,
            plugin_disk_find,
            plugin_disks,
            plugin_open_url,
            plugin_open_pane,
            plugin_focus_pane,
            plugin_notify,
            plugin_tools,
            plugin_tool,
            bench_mode,
            bench_report,
            bench::bench_config,
            bench::bench_recording,
            bench::bench_replay,
            bench::bench_sample,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                save_histories(app);
                #[cfg(target_os = "macos")]
                privacy::stop();
            }
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_the_installed_app_connects_the_agents_on_start() {
        use std::path::Path;
        assert!(super::installed_app(Path::new("/Applications/Wings.app/Contents/MacOS/wings")));
        // A release binary in the build folder, the test harness, or a binary copied anywhere else does not.
        for other in ["/w/apps/desktop/src-tauri/target/release/deps/wings-224b4daf414bfdd7", "/w/target/release/wings", "/usr/local/bin/wings", "/x/MacOS/wings", "wings"] {
            assert!(!super::installed_app(Path::new(other)), "{other}");
        }
    }

    #[test]
    fn re_registering_keeps_what_you_set_on_the_server() {
        use serde_json::json;
        let (exe, socket) = (std::path::Path::new("/Apps/Wings"), std::path::Path::new("/data/mcp.sock"));
        let old = json!({ "type": "stdio", "command": "/Old/Wings", "args": ["--mcp", "/old.sock"], "env": {}, "alwaysLoad": true });
        assert_eq!(
            super::mcp_add_json(Some(&old), None, exe, socket),
            Some(json!({ "type": "stdio", "command": "/Apps/Wings", "args": ["--mcp", "/data/mcp.sock"], "env": {}, "alwaysLoad": true }))
        );
        // Nothing of yours on it, or no entry at all: the plain `claude mcp add`.
        let plain = json!({ "type": "stdio", "command": "/Old/Wings", "args": ["--mcp", "/old.sock"], "env": {} });
        assert_eq!(super::mcp_add_json(Some(&plain), None, exe, socket), None);
        assert_eq!(super::mcp_add_json(None, None, exe, socket), None);
    }

    #[test]
    fn the_always_load_switch_sets_or_clears_only_that_field() {
        use serde_json::json;
        let (exe, socket) = (std::path::Path::new("/Apps/Wings"), std::path::Path::new("/data/mcp.sock"));
        let plain = json!({ "type": "stdio", "command": "/Old/Wings", "args": ["--mcp", "/old.sock"], "env": {}, "timeout": 9 });
        assert_eq!(
            super::mcp_add_json(Some(&plain), Some(true), exe, socket),
            Some(json!({ "type": "stdio", "command": "/Apps/Wings", "args": ["--mcp", "/data/mcp.sock"], "env": {}, "timeout": 9, "alwaysLoad": true }))
        );
        assert_eq!(super::mcp_add_json(None, Some(true), exe, socket).unwrap()["alwaysLoad"], true);
        let on = json!({ "type": "stdio", "command": "/Old/Wings", "args": [], "env": {}, "alwaysLoad": true });
        assert_eq!(super::mcp_add_json(Some(&on), Some(false), exe, socket), None);
    }

    #[test]
    fn readme_is_found_in_any_case_and_capped_and_kept_inside_the_folder() {
        let root = std::env::temp_dir().join(format!("wings-readme-test-{}", std::process::id()));
        let dir = root.join("plugin");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(super::read_readme(&dir), None);
        std::fs::write(dir.join("ReadMe.MD"), "# Hi").unwrap();
        assert_eq!(super::read_readme(&dir).as_deref(), Some("# Hi"));
        std::fs::write(dir.join("ReadMe.MD"), vec![b'a'; super::README_MAX_BYTES as usize + 1]).unwrap();
        assert_eq!(super::read_readme(&dir), None);
        std::fs::remove_file(dir.join("ReadMe.MD")).unwrap();
        std::fs::write(root.join("secret.md"), "outside").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("secret.md"), dir.join("README.md")).unwrap();
            assert_eq!(super::read_readme(&dir), None);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Notifications that are never clicked or cleared keep their waiting thread, so the count has a ceiling
    /// and a finished wait gives its place back.
    #[test]
    fn click_waits_stop_at_the_limit_and_are_given_back() {
        let waits = super::ClickWaits(std::sync::atomic::AtomicUsize::new(0));
        let held: Vec<_> = (0..3).map(|_| waits.take(3).expect("under the limit")).collect();
        assert!(waits.take(3).is_none());
        drop(held);
        assert!(waits.take(3).is_some());
    }

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
