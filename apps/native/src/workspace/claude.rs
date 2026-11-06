//! The native app's side of Claude Code, the desktop's (`apps/desktop/src-tauri/src/mcp.rs` and its Connect and
//! Disconnect): the socket the `wings` command talks to, served with `wings_connect::server` at the desktop's path, so
//! `wings --mcp`, `wings statusline`, `wings hook` and the `wings` CLI reach this app the same way. Its tools are Wings'
//! own (`wings_connect::panes` and `project_info`, and the history search) and those of the plugins that run here; its
//! hooks go to the plugins' `onHook`; its status lines are kept for `wings.statusline()`.
//!
//! On start the app takes the socket as the desktop does, replacing one left from an earlier run, and connects Claude
//! Code (the MCP server, and the status line and hooks chained with yours) to its own `wings` program, the one beside
//! `wings-native` in the bundle. Settings, Agents connects and disconnects it.
//!
//! Requests come on the socket's threads. What they need of the window is read or done on its thread (`AppEnv::on_window`).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{Context, Window};
use serde_json::{Value, json};
use wings_agents::detect::{Agent, AgentState};
use wings_connect::{
    panes::{self, Host, OpenPlan, Opened, PaneRow},
    register::{self, Status},
    server::{
        self, Clients, Handler, SEPARATOR, ToolResult, caller, hook_event, hook_session,
        merge_hook_answers, session_pane,
    },
};
use wings_plugins::permissions::NotifyLog;
use wings_spaces::{claude_accounts, project_info::Store, spaces::Space};

use crate::{
    host::PluginPaths,
    page::{
        plugin::{ClaudeCaller, OpenPane},
        runtime::Reply,
    },
    projects::with_setup,
};

use super::{Active, Workspace, history::Sessions, plugin_env::AppEnv};

/// How long a plugin's tool has to answer Claude, as on the desktop.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a plugin has to answer a Claude Code hook. `wings hook` gives up on Wings after 2 s.
const HOOK_TIMEOUT: Duration = Duration::from_millis(1500);
/// How long a request waits for the window. Opening a pane or reading one is quick; a window that is stuck says so.
const WINDOW_WAIT: Duration = Duration::from_secs(10);

/// What the window knows of its panes, read on its thread for one request.
#[derive(Default)]
pub(super) struct Snapshot {
    panes: Vec<PaneRow>,
    agents: Vec<Agent>,
    spaces: Vec<Space>,
    /// Each pane's shell, by pid.
    shells: HashMap<u32, String>,
    /// The panes whose program is running something rather than sitting at its prompt.
    busy: Vec<String>,
}

impl Snapshot {
    /// The pane a call came from: the OpenCode session's when it names one, else the pane whose shell is above `ppid`.
    fn pane(&self, ppid: Option<u32>, session_id: Option<&str>) -> Option<String> {
        match session_id {
            Some(id) => session_pane(&self.agents, id),
            None => ppid.and_then(|pid| server::pane_of(&self.shells, pid)),
        }
    }

    /// Who called from `pane`, as a plugin's handler is told.
    fn caller(&self, pane: Option<String>) -> ClaudeCaller {
        let row = pane
            .as_ref()
            .and_then(|id| self.panes.iter().find(|p| &p.id == id));
        let space = row.and_then(|row| self.spaces.iter().find(|s| s.id == row.space_id));
        let agent = pane
            .as_ref()
            .and_then(|id| self.agents.iter().find(|a| &a.pane_id == id));
        let (project, session) = caller(space, agent);
        ClaudeCaller {
            pane_id: pane,
            project,
            session,
        }
    }
}

impl Workspace {
    /// The panes of every project with what runs in them, the agents, the projects and the shells.
    pub(super) fn claude_snapshot(&self, cx: &gpui::App) -> Snapshot {
        let panes = self
            .pane_projects()
            .into_iter()
            .map(|(id, path)| {
                let info = self.pane_info.get(&id);
                let pane = self.panes.get(&id);
                let title = pane
                    .and_then(|pane| pane.name.as_ref().map(ToString::to_string))
                    .or_else(|| pane.and_then(|pane| pane.view.as_ref().ok()?.read(cx).title()))
                    .unwrap_or_default();
                PaneRow {
                    id: id.to_string(),
                    space_id: path.to_string_lossy().into_owned(),
                    cwd: info
                        .and_then(|info| info.cwd.clone())
                        .map_or_else(|| path.to_owned(), PathBuf::from),
                    command: info.map(|info| info.command.clone()),
                    title,
                }
            })
            .collect();
        let shells = self
            .pane_shells(cx)
            .into_iter()
            .filter_map(|(id, pid)| Some((pid?, id)))
            .collect();
        Snapshot {
            panes,
            agents: self.agents.values().cloned().collect(),
            spaces: self.spaces.spaces(),
            shells,
            busy: self
                .pane_info
                .iter()
                .filter(|(_, info)| info.busy)
                .map(|(id, _)| id.to_string())
                .collect(),
        }
    }

    /// `open_pane`: a terminal in the project, as a split below the caller's pane when it is given and on screen,
    /// with the command typed and never run (this app has no "Let Claude run commands in new tabs" yet).
    fn claude_open_pane(
        &mut self,
        plan: OpenPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Opened, String> {
        let project = PathBuf::from(&plan.space_id);
        let below = plan
            .next_to
            .as_deref()
            .and_then(|pane| self.plugin_pane(pane));
        if let Some(pane) = below {
            self.switch_project(project.clone(), window, cx);
            if let Some(tab) = self
                .terminals
                .iter_mut()
                .find(|tab| tab.layout.panes().contains(&pane))
            {
                tab.focused = pane;
                self.active = Active::Terminal(tab.id);
            }
        }
        let placement = if below.is_some() { "below" } else { "tab" };
        let input = with_setup(self.spaces.take_setup(&project).as_deref(), plan.input);
        let cwd = plan.cwd.map_or_else(|| project.clone(), PathBuf::from);
        let (reply, answer) = Reply::channel();
        let plan = OpenPane {
            cwd,
            project,
            input,
            placement: placement.into(),
        };
        self.open_plugin_pane(plan, reply, window, cx);
        let opened = answer
            .wait(Duration::ZERO)
            .ok_or("The window didn't open the pane")??;
        let opened: Value = serde_json::from_str(&opened).map_err(|e| e.to_string())?;
        let pane_id = opened["paneId"]
            .as_str()
            .ok_or("The window didn't say which pane it opened")?;
        Ok(Opened {
            pane_id: pane_id.to_owned(),
            ran: false,
            reused: false,
        })
    }

    /// `close_pane`, for a pane of the project on screen.
    fn claude_close_pane(
        &mut self,
        pane: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let id = self
            .plugin_pane(pane)
            .ok_or_else(|| format!("No pane {pane}. It was closed."))?;
        if !self
            .terminals
            .iter()
            .any(|tab| tab.layout.panes().contains(&id))
        {
            return Err(format!(
                "Pane {pane} is in a project that isn't on screen. Close it there."
            ));
        }
        self.close_pane(id, window, cx);
        Ok(())
    }
}

/// The app's side of the socket.
pub(super) struct Server {
    env: Arc<AppEnv>,
    sessions: Sessions,
    info: Option<Arc<Store>>,
    plugins: Option<PluginPaths>,
    /// The panes Claude opened with `open_pane`, the only ones `close_pane` closes.
    opened: std::sync::Mutex<Vec<String>>,
    /// One allowance for every Claude session together, the size of a plugin's.
    notified: std::sync::Mutex<NotifyLog>,
}

/// Serves the socket at `path` for the window's `env`, replacing a stale one from an earlier run.
pub(super) fn start(
    path: &Path,
    env: Arc<AppEnv>,
    sessions: Sessions,
    info: Option<Arc<Store>>,
    plugins: Option<PluginPaths>,
) -> std::io::Result<()> {
    let server = Arc::new(Server {
        env,
        sessions,
        info,
        plugins,
        opened: Default::default(),
        notified: Default::default(),
    });
    server::start(path, server, Arc::new(Clients::default()))
}

impl Server {
    fn snapshot(&self, wait: Duration) -> Result<Snapshot, String> {
        self.env
            .on_window(wait, |workspace, _, cx| workspace.claude_snapshot(cx))
    }

    /// Wings' own tools, or `None` for a plugin's.
    fn builtin(
        &self,
        snapshot: &Snapshot,
        name: &str,
        arguments: &Value,
        pane: &dyn Fn() -> Option<String>,
    ) -> Option<Result<String, String>> {
        self.sessions
            .call_tool(name, arguments)
            .or_else(|| {
                let store = self
                    .info
                    .as_ref()
                    .filter(|_| name.starts_with("project_info_"))?;
                let space = pane().and_then(|id| {
                    snapshot
                        .panes
                        .iter()
                        .find(|p| p.id == id)
                        .map(|p| p.space_id.clone())
                });
                wings_connect::project_info::call_tool(
                    store,
                    &snapshot.spaces,
                    space.as_deref(),
                    name,
                    arguments,
                    &|_| {},
                )
            })
            .or_else(|| {
                panes::call_tool(
                    &NativeHost {
                        server: self,
                        snapshot,
                    },
                    name,
                    arguments,
                    pane,
                )
            })
    }

    /// `pane_busy`, for `wings wait`: whether the pane runs a command, or for an agent's pane whether the agent works.
    fn pane_busy(&self, pane: &str) -> Result<Value, String> {
        let snapshot = self.snapshot(WINDOW_WAIT)?;
        if !snapshot.panes.iter().any(|p| p.id == pane) {
            return Err(format!("No pane {pane}. It was closed."));
        }
        Ok(match snapshot.agents.iter().find(|a| a.pane_id == pane) {
            Some(agent) => {
                json!({ "busy": agent.state == AgentState::Working, "agent": true, "sessionId": agent.session_id })
            }
            None => json!({ "busy": snapshot.busy.iter().any(|p| p == pane) }),
        })
    }
}

/// A plugin's answer `{ text, isError }`, as Claude gets it.
fn tool_result(answer: Option<Result<String, String>>, plugin: &str) -> Result<ToolResult, String> {
    let answer: Value =
        serde_json::from_str(&answer.ok_or_else(|| format!("{plugin} didn't answer in time"))??)
            .map_err(|e| e.to_string())?;
    Ok(ToolResult {
        text: answer["text"].as_str().unwrap_or_default().to_owned(),
        is_error: answer["isError"] == true,
    })
}

impl Handler for Server {
    fn tools(&self) -> Vec<Value> {
        wings_agents::history::mcp_tools()
            .into_iter()
            .chain(panes::tools())
            .chain(
                wings_connect::project_info::mcp_tools()
                    .into_iter()
                    .filter(|_| self.info.is_some()),
            )
            .chain(self.env.running.mcp_tools())
            .collect()
    }

    fn instructions(&self) -> Vec<String> {
        self.env.running.mcp_instructions()
    }

    fn call(
        &self,
        name: &str,
        arguments: Value,
        ppid: Option<u32>,
        session_id: Option<&str>,
    ) -> Result<ToolResult, String> {
        let snapshot = self.snapshot(WINDOW_WAIT)?;
        let pane = || snapshot.pane(ppid, session_id);
        if let Some(result) = self.builtin(&snapshot, name, &arguments, &pane) {
            return Ok(match result {
                Ok(text) => ToolResult {
                    text,
                    is_error: false,
                },
                Err(text) => ToolResult {
                    text,
                    is_error: true,
                },
            });
        }
        let (plugin, tool) = name
            .split_once(SEPARATOR)
            .ok_or_else(|| format!("unknown tool {name}"))?;
        let (reply, answer) = Reply::channel();
        self.env.running.call_for_claude(
            plugin,
            tool,
            arguments,
            &snapshot.caller(pane()),
            false,
            reply,
        );
        tool_result(answer.wait(CALL_TIMEOUT), plugin)
    }

    /// What the plugins that answer the hook say, asked together, each within `HOOK_TIMEOUT`; one that is slow,
    /// throws or says nothing adds nothing.
    fn hook(&self, input: &Value, ppid: Option<u32>) -> Value {
        let Some(event) = hook_event(input) else {
            return json!({});
        };
        let plugins = self.env.running.hooked(event);
        if plugins.is_empty() {
            return json!({});
        }
        let snapshot = self.snapshot(HOOK_TIMEOUT).unwrap_or_default();
        let mut caller = snapshot.caller(snapshot.pane(ppid, None));
        caller.session = hook_session(event, input, caller.session);
        let started = Instant::now();
        let asked: Vec<_> = plugins
            .iter()
            .map(|plugin| {
                let (reply, answer) = Reply::channel();
                self.env.running.call_for_claude(
                    plugin,
                    event,
                    input.clone(),
                    &caller,
                    true,
                    reply,
                );
                answer
            })
            .collect();
        let answers: Vec<Value> = asked
            .into_iter()
            .filter_map(|answer| {
                answer
                    .wait(HOOK_TIMEOUT.saturating_sub(started.elapsed()))?
                    .ok()
            })
            .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
            .filter(|result| result["isError"] != true)
            .filter_map(|result| serde_json::from_str(result["text"].as_str()?).ok())
            .collect();
        merge_hook_answers(event, &answers)
    }

    fn control(&self, op: &str, request: &Value) -> Option<Result<Value, String>> {
        Some(match op {
            "statusline" => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
                self.env.statusline.lock().unwrap().record(request.get("status").unwrap_or(&Value::Null), now);
                Ok(Value::Null)
            }
            "pane_busy" => self.pane_busy(request.get("pane").and_then(Value::as_str).unwrap_or_default()),
            // The desktop's plugin list, which this app reads and never writes.
            "plugins" => Ok(self.plugins.as_ref().map_or_else(
                || json!([]),
                |paths| json!(wings_plugins::store::Store::load(paths.dir.clone(), paths.state.clone(), paths.dev.as_deref()).list()),
            )),
            "install" | "link" | "update" | "enable" | "disable" | "remove" => {
                Err("This Wings doesn't change plugins yet. Install, update, turn on or off and remove them in the desktop app.".into())
            }
            _ => return None,
        })
    }
}

/// The window, as Wings' pane tools see it for one request.
struct NativeHost<'a> {
    server: &'a Server,
    snapshot: &'a Snapshot,
}

impl Host for NativeHost<'_> {
    fn panes(&self) -> Vec<PaneRow> {
        self.snapshot.panes.clone()
    }

    fn agents(&self) -> Vec<Agent> {
        self.snapshot.agents.clone()
    }

    fn spaces(&self) -> Vec<Space> {
        self.snapshot.spaces.clone()
    }

    fn output(&self, pane_id: &str) -> Option<Vec<u8>> {
        let pane = pane_id.to_owned();
        self.server
            .env
            .on_window(WINDOW_WAIT, move |workspace, _, cx| {
                let id = workspace.plugin_pane(&pane)?;
                Some(workspace.terminal(id)?.read(cx).recent_output())
            })
            .ok()
            .flatten()
    }

    fn open(&self, plan: OpenPlan) -> Result<Opened, String> {
        let opened = self
            .server
            .env
            .on_window(WINDOW_WAIT, move |workspace, window, cx| {
                workspace.claude_open_pane(plan, window, cx)
            })??;
        self.server
            .opened
            .lock()
            .unwrap()
            .push(opened.pane_id.clone());
        Ok(opened)
    }

    fn close(&self, pane_id: &str) -> Result<(), String> {
        if !self
            .server
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == pane_id)
        {
            return Err(format!(
                "Pane {pane_id} wasn't opened with open_pane, so it stays open. Ask the user to close it."
            ));
        }
        let pane = pane_id.to_owned();
        self.server
            .env
            .on_window(WINDOW_WAIT, move |workspace, window, cx| {
                workspace.claude_close_pane(&pane, window, cx)
            })??;
        self.server.opened.lock().unwrap().retain(|p| p != pane_id);
        Ok(())
    }

    fn notify(&self, title: String, body: String) -> Result<(), String> {
        self.server
            .notified
            .lock()
            .unwrap()
            .allow("claude", Instant::now())
            .map_err(|wait| {
                format!(
                    "Already showed {} notifications this minute. Try again in {} s.",
                    wings_plugins::permissions::NOTIFY_LIMIT,
                    wait.as_secs() + 1
                )
            })?;
        self.server.env.show_notification(title, body);
        Ok(())
    }

    fn send(&self, pane_id: &str, text: &str) -> Result<bool, String> {
        let (pane, text) = (pane_id.to_owned(), text.to_owned());
        let answer = self
            .server
            .env
            .on_window(WINDOW_WAIT, move |workspace, _, cx| {
                let (reply, answer) = Reply::channel();
                workspace.send_to_agent(&pane, &text, reply, cx);
                answer.wait(Duration::ZERO)
            })?;
        let answer: Value = serde_json::from_str(&answer.ok_or("The window didn't answer")??)
            .map_err(|e| e.to_string())?;
        Ok(answer["submitted"] == true)
    }

    fn set_worktree_setup(
        &self,
        space_id: &str,
        include: Option<&str>,
        setup: Option<&str>,
    ) -> Result<(), String> {
        self.server
            .env
            .spaces()
            .update_worktree_settings(space_id, include, setup)
    }

    fn approve_setup(
        &self,
        _project: &str,
        _pane: &str,
        _current: Option<&str>,
        _proposed: &str,
    ) -> Result<bool, String> {
        Err("This Wings can't ask the user to approve a setup command yet. Ask them to set it in the desktop app, or change only include.".into())
    }
}

// ---- Connecting Claude Code ----

/// The `wings` program Claude Code's config points at: the one beside this app's binary.
pub(super) fn wings_program() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.with_file_name("wings")).filter(|wings| wings.is_file())
}

/// The file that says you disconnected Claude Code, beside the app's own state, so it isn't connected again on start.
pub(super) fn disconnected_marker(state: &Path) -> PathBuf {
    state.with_file_name(register::DISCONNECTED)
}

/// What starting the app does, as the desktop does: in a release build of the installed app, unless you disconnected
/// it, Claude Code gets Wings in the default account and in each one a project uses. Runs `claude`, so off the window's
/// thread.
pub(super) fn connect_on_start(socket: PathBuf, state: Option<PathBuf>, spaces: Vec<Space>) {
    if cfg!(debug_assertions)
        || state
            .as_deref()
            .is_none_or(|s| disconnected_marker(s).exists())
    {
        return;
    }
    let Some(exe) = std::env::current_exe()
        .ok()
        .and_then(|exe| register::installed_wings(&exe))
    else {
        return;
    };
    let Some(claude) = wings_plugins::exec::find_program("claude") else {
        return;
    };
    std::thread::spawn(move || {
        for account in claude_accounts::connect_targets(&spaces) {
            if let Err(error) =
                register::ensure_connected(&claude, &exe, &socket, account.as_deref())
            {
                eprintln!(
                    "Couldn't connect Claude Code ({}): {error}",
                    account.as_deref().unwrap_or("default account")
                );
            }
        }
    });
}

/// One Claude Code account as Settings, Agents shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct AccountStatus {
    /// `None` for the default account.
    pub name: Option<String>,
    pub status: Status,
}

/// Claude Code's connection as Settings, Agents shows it, and what it is doing.
#[derive(Default)]
pub(super) struct Connection {
    /// Each account Wings connects to or is in, the default one first; `None` until read.
    pub(super) accounts: Option<Vec<AccountStatus>>,
    pub(super) busy: bool,
    pub(super) error: Option<String>,
    task: Option<gpui::Task<()>>,
}

/// What Settings' Connect, Reconnect, Disconnect and "Load Wings tools at session start" do.
#[derive(Clone, Copy)]
pub(super) enum Change {
    Connect(Option<bool>),
    Disconnect,
}

impl Workspace {
    /// Reads each account's state again, off the window's thread: it runs `claude mcp get`.
    pub(super) fn load_claude_status(&mut self, cx: &mut Context<Self>) {
        if self.config.socket.is_none() || self.claude.busy {
            return;
        }
        self.run_claude(None, cx);
    }

    /// Connects or disconnects Claude Code in every account, then reads their state again.
    pub(super) fn change_claude(&mut self, change: Change, cx: &mut Context<Self>) {
        if self.claude.busy {
            return;
        }
        self.run_claude(Some(change), cx);
    }

    fn run_claude(&mut self, change: Option<Change>, cx: &mut Context<Self>) {
        let Some(socket) = self.config.socket.clone() else {
            return;
        };
        let spaces = self.spaces.spaces();
        let marker = self.config.state.as_deref().map(disconnected_marker);
        self.claude.busy = true;
        if change.is_some() {
            self.claude.error = None;
        }
        cx.notify();
        let work = cx.background_executor().spawn(async move {
            let claude = wings_plugins::exec::find_program("claude");
            let result = match change {
                None => Ok(()),
                Some(Change::Connect(always_load)) => (|| {
                    let claude = claude.as_deref().ok_or("Claude Code isn't installed, or `claude` isn't on your PATH")?;
                    let exe = wings_program().ok_or("The `wings` program isn't beside this app, so Claude Code would have nothing to run")?;
                    register::connect(claude, &exe, &socket, &claude_accounts::connect_targets(&spaces), always_load)?;
                    if let Some(marker) = &marker {
                        let _ = std::fs::remove_file(marker);
                    }
                    Ok(())
                })(),
                Some(Change::Disconnect) => register::disconnect(claude.as_deref(), &spaces).and_then(|()| match &marker {
                    Some(marker) => std::fs::write(marker, "").map_err(|e| e.to_string()),
                    None => Ok(()),
                }),
            };
            let accounts = register::disconnect_targets(&spaces)
                .into_iter()
                .map(|name| AccountStatus { status: register::status(name.as_deref()), name })
                .collect();
            (result, accounts)
        });
        self.claude.task = Some(cx.spawn(async move |this, cx| {
            let (result, accounts) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.claude.busy = false;
                this.claude.accounts = Some(accounts);
                if let Err(error) = result {
                    this.claude.error = Some(error);
                }
                cx.notify();
            });
        }));
    }

    /// Shows these accounts in Settings, Agents instead of reading Claude Code's, for screenshots.
    #[cfg(feature = "test-support")]
    pub fn show_claude_accounts(&mut self, accounts: Vec<AccountStatus>, cx: &mut Context<Self>) {
        self.claude.accounts = Some(accounts);
        cx.notify();
    }
}
