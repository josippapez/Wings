mod claude;
mod detect;
mod menu;
mod plugin_store;
mod plugins;
mod pty;
mod spaces;

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
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use claude::SessionSummary;
use detect::{Agent, AgentState, Detector, PaneInfo, PaneProbe};
use pty::{Pane, SpawnRequest};
use plugin_store::{PluginView, Source, Store};
use plugins::{ExecResult, Plugin};
use spaces::{GitStatus, SpaceStore, SpaceView};

struct AppState {
    claude_dir: PathBuf,
    spaces: Mutex<SpaceStore>,
    panes: Mutex<HashMap<String, Arc<Pane>>>,
    focused: Mutex<Option<String>>,
    agents: Mutex<Vec<Agent>>,
    pane_info: Mutex<HashMap<String, PaneInfo>>,
    git: Mutex<HashMap<String, GitStatus>>,
    plugins: Mutex<Store>,
    next_pane: AtomicU64,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn spaces_list(state: State<AppState>) -> Vec<SpaceView> {
    state.spaces.lock().unwrap().spaces.iter().map(spaces::view).collect()
}

#[tauri::command]
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
fn pane_create(
    app: AppHandle,
    state: State<AppState>,
    space_id: String,
    cols: u16,
    rows: u16,
    initial_input: Option<String>,
    on_output: Channel<InvokeResponseBody>,
) -> Res<String> {
    let cwd = PathBuf::from(&state.spaces.lock().unwrap().get(&space_id).ok_or("unknown space")?.path);
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

/// Called when the UI loads. Panes from a previous load (a webview reload) have no terminal left, so kill them.
#[tauri::command]
fn panes_reset(state: State<AppState>) {
    for (_, pane) in state.panes.lock().unwrap().drain() {
        pane.kill();
    }
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
fn plugin_install_file(state: State<AppState>, path: String) -> Res<PluginView> {
    let size = std::fs::metadata(&path).map_err(err)?.len();
    if size > PACKAGE_LIMIT {
        return Err("The package is larger than 50 MB".into());
    }
    let bytes = std::fs::read(&path).map_err(err)?;
    state.plugins.lock().unwrap().install(&bytes, Source::File)
}

#[tauri::command(async)]
fn plugin_install_github(state: State<AppState>, url: String) -> Res<PluginView> {
    let repo = plugin_store::parse_repo(&url).ok_or("That isn't a GitHub repo link, like github.com/owner/name")?;
    let bytes = plugin_store::download_github(&repo)?;
    state.plugins.lock().unwrap().install(&bytes, Source::Github { repo })
}

/// Reinstalls a GitHub plugin from its newest release or default branch.
#[tauri::command(async)]
fn plugin_update(state: State<AppState>, id: String) -> Res<PluginView> {
    let Some(Source::Github { repo }) = state.plugins.lock().unwrap().source(&id) else {
        return Err("Only plugins installed from GitHub can update. Install the new .wings-plugin file instead.".into());
    };
    let bytes = plugin_store::download_github(&repo)?;
    state.plugins.lock().unwrap().install(&bytes, Source::Github { repo })
}

/// The newest release version on GitHub, if the plugin came from there and the repo has releases.
#[tauri::command(async)]
fn plugin_latest_version(state: State<AppState>, id: String) -> Res<Option<String>> {
    let Some(Source::Github { repo }) = state.plugins.lock().unwrap().source(&id) else { return Ok(None) };
    plugin_store::latest_version(&repo)
}

/// Turning a plugin on also approves the permissions it asks for, so the UI shows them first.
#[tauri::command]
fn plugin_set_enabled(state: State<AppState>, id: String, enabled: bool) -> Res<PluginView> {
    state.plugins.lock().unwrap().set_enabled(&id, enabled)
}

#[tauri::command]
fn plugin_remove(state: State<AppState>, id: String) -> Res<()> {
    state.plugins.lock().unwrap().remove(&id)
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
fn plugin_transcript(state: State<AppState>, plugin_id: String, session_id: String, types: Vec<String>) -> Res<Vec<serde_json::Value>> {
    plugins::transcript_entries(&plugin(&state, &plugin_id)?, &state.claude_dir, &session_id, &types)
}

#[tauri::command]
fn plugin_open_url(app: AppHandle, state: State<AppState>, plugin_id: String, url: String) -> Res<()> {
    let plugin = plugin(&state, &plugin_id)?;
    if !plugins::may_open_url(&plugin, &url) {
        return Err(format!("{plugin_id} may not open {url}"));
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

/// Serves plugin files at `wings-plugin://localhost/<id>/<path>`.
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
    let (id, file) = path.split_once('/').unwrap_or((path, ""));
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
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

/// Polls panes for Claude processes and emits `agents` whenever the list changes.
fn start_detection(app: AppHandle) {
    thread::spawn(move || {
        let state = app.state::<AppState>();
        let mut detector = Detector::new(&state.claude_dir);
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
            let mut current = state.agents.lock().unwrap();
            if *current != agents {
                notify_transitions(&app, &current, &agents);
                *current = agents.clone();
                drop(current);
                let _ = app.emit("agents", agents);
            }
            let mut info = state.pane_info.lock().unwrap();
            if *info != scan.panes {
                *info = scan.panes.clone();
                drop(info);
                let _ = app.emit("pane-info", scan.panes);
            }
            thread::sleep(Duration::from_millis(500));
        }
    });
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
                claude_dir,
                spaces: Mutex::new(store),
                panes: Mutex::new(HashMap::new()),
                focused: Mutex::new(None),
                agents: Mutex::new(Vec::new()),
                pane_info: Mutex::new(HashMap::new()),
                git: Mutex::new(HashMap::new()),
                next_pane: AtomicU64::new(1),
            });
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
            pane_create,
            pane_write,
            pane_resize,
            pane_close,
            panes_reset,
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
            plugin_exec,
            plugin_transcript,
            plugin_open_url,
            bench_mode,
            bench_report,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
