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

use crate::workspace::NotifyWorkspace;
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
    panes::{self, Host, OpenPlan, Opened, PaneRow, TabRow},
    register::{self, Status},
    server::{
        self, Handler, SEPARATOR, ToolResult, caller, hook_event, hook_session, merge_hook_answers,
        session_pane,
    },
};
use wings_plugins::permissions::NotifyLog;
use wings_spaces::{
    claude_accounts,
    project_info::Store,
    spaces::{Made, Space},
    worktree::Existing,
};

use crate::{
    page::{
        plugin::{ClaudeCaller, OpenPane},
        runtime::Reply,
    },
    projects::with_setup,
};

use super::{
    Workspace,
    history::Sessions,
    plugin_env::{AppEnv, UiEvent},
};

/// How long a plugin's tool has to answer Claude, as on the desktop.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a plugin has to answer a Claude Code hook. `wings hook` gives up on Wings after 2 s.
const HOOK_TIMEOUT: Duration = Duration::from_millis(1500);
/// How long a request waits for the window. Opening a pane or reading one is quick; a window that is stuck says so.
const WINDOW_WAIT: Duration = Duration::from_secs(10);

/// The shortest and longest measurement the `performance` tool makes, and what it makes without `seconds`.
const PERFORMANCE_SECONDS: (f64, f64, f64) = (1.0, 30.0, 3.0);
/// After the frame counter is put in the window and before the measured window starts, so the draw that puts it in is not
/// counted.
const PERFORMANCE_SETTLE: Duration = Duration::from_millis(250);

/// Wings' `performance` tool, which only this app has.
fn performance_tool() -> Value {
    wings_connect::server::cli_only(json!({
        "name": "performance",
        "description": "Measure Wings' own CPU, window frames per second and which views render most, over a few seconds, for checking a change's cost in the running app. Turns counting on for that window if the performance readout is off (without showing its card), then puts it back. Returns JSON: seconds, cpuPercent (of one core), frames, framesPerSecond, renders (view, count, perSecond; most first), causes (at: file:line that notified the workspace, which redraws all its pieces, with count, perSecond; most first), cardShown (the readout's card was on, so its own update is in the numbers) and a one-line summary. Does not draw frames itself. Leave Wings alone while it runs for an idle number: a Claude session working in Wings draws too.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "seconds": { "type": "number", "description": format!("How long to measure, {} to {}. Default {}.", PERFORMANCE_SECONDS.0, PERFORMANCE_SECONDS.1, PERFORMANCE_SECONDS.2) }
            }
        }
    }))
}

/// `performance`: counts for `seconds` (a number, clamped) and answers the measurement as JSON. `wait` sleeps, off the
/// window's thread, so the window draws as it does.
pub(super) fn performance(env: &AppEnv, arguments: &Value, wait: &dyn Fn(Duration)) -> Result<String, String> {
    let (least, most, default) = PERFORMANCE_SECONDS;
    let seconds = match arguments.get("seconds") {
        None | Some(Value::Null) => default,
        Some(value) => value
            .as_f64()
            .filter(|s| s.is_finite())
            .ok_or_else(|| format!("seconds is a number, {least} to {most}"))?
            .clamp(least, most),
    };
    env.on_window(WINDOW_WAIT, |workspace, _, cx| workspace.measure_begin(cx))??;
    let waited = (|| {
        wait(PERFORMANCE_SETTLE);
        env.on_window_quietly(WINDOW_WAIT, |workspace, _, cx| workspace.measure_restart(cx))??;
        wait(Duration::from_secs_f64(seconds));
        Ok::<_, String>(())
    })();
    // Always put the readout back, even when the window stopped answering in between.
    let measured = env.on_window(WINDOW_WAIT, |workspace, _, cx| workspace.measure_end(cx));
    waited?;
    Ok(performance_json(&measured??).to_string())
}

fn performance_json(measured: &crate::ui::readout::Measured) -> Value {
    let tenth = |n: f64| (n * 10.0).round() / 10.0;
    let per_second = |count: u32| tenth(f64::from(count) / measured.seconds);
    let renders: Vec<Value> = measured
        .renders
        .iter()
        .map(|&(view, count)| json!({ "view": view, "count": count, "perSecond": per_second(count) }))
        .collect();
    let causes: Vec<Value> = measured
        .causes
        .iter()
        .map(|(at, count)| json!({ "at": at, "count": count, "perSecond": per_second(*count) }))
        .collect();
    let notifiers: Vec<String> = measured.causes.iter().take(3).map(|(at, count)| format!("{at} {}", per_second(*count))).collect();
    let busiest: Vec<String> = measured.renders.iter().take(5).map(|&(view, count)| format!("{view} {}", per_second(count))).collect();
    let summary = format!(
        "Over {:.1} s Wings used {:.1}% of one core and drew {} frames/s ({} frames). Renders/s: {}. Workspace notified/s by: {}.{}",
        measured.seconds,
        measured.cpu_percent,
        per_second(measured.frames),
        measured.frames,
        if busiest.is_empty() { "none".to_string() } else { busiest.join(", ") },
        if notifiers.is_empty() { "none".to_string() } else { notifiers.join(", ") },
        if measured.card_shown { " The readout card was on, so its update is counted." } else { "" },
    );
    json!({
        "seconds": tenth(measured.seconds),
        "cpuPercent": tenth(measured.cpu_percent),
        "frames": measured.frames,
        "framesPerSecond": per_second(measured.frames),
        "renders": renders,
        "causes": causes,
        "cardShown": measured.card_shown,
        "summary": summary,
    })
}

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
    /// The plugin tabs whose agent, like a chat's Claude, has the terminals it opens in the tab's page. Its programs are
    /// in `shells`, so a call from it comes from the tab, though it isn't a pane.
    pages: Vec<PaneRow>,
    /// The tabs whose page runs an agent, which `send_to_agent` reaches.
    tabs: Vec<TabRow>,
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
            .and_then(|id| self.panes.iter().chain(&self.pages).find(|p| &p.id == id));
        let space = row.and_then(|row| self.spaces.iter().find(|s| s.id == row.space_id));
        let agent = pane
            .as_ref()
            .and_then(|id| self.agents.iter().find(|a| &a.pane_id == id));
        let (project, mut session) = caller(space, agent);
        // Claude in a chat tab is no pane's agent: the tab's page says which session it runs, so issues and the journal
        // know who called. Claude chat is the only page that reports an agent.
        if let Some(tab) = session
            .is_null()
            .then(|| pane.as_ref().and_then(|id| self.tabs.iter().find(|tab| &tab.id == id)))
            .flatten()
            && let Some(id) = &tab.session_id
        {
            session = serde_json::json!({ "id": id, "name": tab.name, "kind": "claude" });
        }
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
        let mut shells: HashMap<u32, String> = self
            .pane_shells(cx)
            .into_iter()
            .filter_map(|(id, pid)| Some((pid?, id)))
            .collect();
        let pages = self.page_callers();
        for (row, pids) in &pages {
            for pid in pids {
                shells.insert(*pid, row.id.clone());
            }
        }
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
            pages: pages.into_iter().map(|(row, _)| row).collect(),
            tabs: self.agent_tabs(),
        }
    }

    /// `open_pane`: a terminal in the project, as a split beside the caller's pane when it is given, where Settings,
    /// Agents says (`open_agent_split`), else as a tab, with the command typed, and run only while "Let Claude run
    /// commands in new tabs" is on. A command goes to a terminal the same agent opened before that sits idle instead,
    /// as the desktop's `onMcpOpenPane`. The user didn't ask for it, so it opens out of sight of what they look at.
    pub(super) fn claude_open_pane(
        &mut self,
        plan: OpenPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Opened, String> {
        let project = PathBuf::from(&plan.space_id);
        let opened = self.in_project(project, window, cx, |this, window, cx| {
            this.open_for_claude(plan, window, cx)
        });
        self.sync_plugins(cx);
        opened
    }

    fn open_for_claude(
        &mut self,
        plan: OpenPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Opened, String> {
        let project = PathBuf::from(&plan.space_id);
        let owner = plan
            .next_to
            .as_deref()
            .and_then(|pane| self.plugin_pane(pane));
        // From the agent of a plugin tab, like a chat, whose page shows the terminals it opens.
        let page = plan
            .next_to
            .as_deref()
            .and_then(super::agent_terminals::page_of_caller)
            .filter(|tab| self.shows_terminals(*tab));
        let title = plan.input.clone().unwrap_or_else(|| {
            let dir = plan.cwd.as_deref().unwrap_or(&plan.space_id);
            Path::new(dir)
                .file_name()
                .map_or_else(|| dir.to_owned(), |name| name.to_string_lossy().into_owned())
        });
        // Enter only while the user lets Claude run commands in new tabs, as the desktop's `onMcpOpenPane`.
        let ran = plan.run && plan.input.is_some() && self.prefs.agents_run_commands;
        let typed = if ran {
            plan.input.map(|input| input + "\r")
        } else {
            plan.input
        };
        if let Some(typed) = &typed
            && let Some(pane) =
                self.reuse_agent_terminal(&project, plan.cwd.clone(), owner, page, typed, cx)
        {
            return Ok(Opened {
                pane_id: pane.to_string(),
                ran,
                reused: true,
            });
        }
        let tab = owner.and_then(|pane| {
            self.terminals
                .iter()
                .find(|tab| tab.layout.panes().contains(&pane))
                .map(|tab| tab.id)
        });
        // The setup line runs in an agent's pane only while the user lets Claude run commands in new tabs, and not before a
        // command that waits for the user's Enter. Otherwise it stays for the next pane, like one the user opens.
        let setup = match &typed {
            _ if !self.prefs.agents_run_commands => None,
            Some(typed) if !typed.ends_with('\r') => None,
            _ => self.spaces.take_setup(&project),
        };
        let input = with_setup(setup.as_deref(), typed);
        let cwd = plan
            .cwd
            .clone()
            .map_or_else(|| project.clone(), PathBuf::from);
        let open = OpenPane {
            cwd,
            project: project.clone(),
            input,
            placement: "tab".into(),
            claude_account: None,
            replace: None,
        };
        let owner = owner.filter(|_| tab.is_some());
        let pane = match (page, tab, owner) {
            (Some(page), ..) => self.open_page_terminal(page, open, title, window, cx)?,
            (None, Some(tab), Some(owner)) => self.open_agent_split(tab, owner, open, window, cx)?,
            _ => self.place_plugin_pane(open, window, cx)?.0,
        };
        self.note_agent_terminal(pane, project, plan.cwd, owner, page);
        Ok(Opened {
            pane_id: pane.to_string(),
            ran,
            reused: false,
        })
    }

    /// The tabs whose page runs an agent and says so, like Claude chat's, in every project, as Wings' MCP tools list them.
    pub(super) fn agent_tabs(&self) -> Vec<TabRow> {
        let shown = self.plugin_tabs.iter().map(|tab| (&self.config.project, tab));
        let parked = self
            .parked
            .iter()
            .flat_map(|(path, parked)| parked.plugin_tabs.iter().map(move |tab| (path, tab)));
        shown
            .chain(parked)
            .filter_map(|(project, tab)| {
                let agent = tab.state.agent()?;
                Some(TabRow {
                    id: super::agent_terminals::page_caller(tab.id),
                    space_id: project.to_string_lossy().into_owned(),
                    cwd: project.clone(),
                    plugin: tab.plugin.clone(),
                    title: tab.label().to_string(),
                    state: super::agent_list::tab_state(&agent.state),
                    name: agent.name,
                    summary: agent.summary,
                    session_id: agent.session_id,
                    account: agent.account,
                })
            })
            .collect()
    }

    /// `send_to_agent` to the agent of the plugin tab `tab`, like a chat's Claude: its page puts `text` in its message
    /// box, and sends it only while "Let agents message each other" is on. `reply` gets the page's `{ text, isError }`.
    pub(super) fn send_to_tab_agent(&self, tab: usize, text: &str, reply: Reply) {
        let parked = self.parked.values().flat_map(|parked| parked.plugin_tabs.iter());
        let Some(found) = self.plugin_tabs.iter().chain(parked).find(|t| t.id == tab) else {
            return reply.err(format!("No tab {}. It was closed.", super::agent_terminals::page_caller(tab)));
        };
        let Some(installed) = self.plugins.iter().find(|installed| installed.id == found.plugin) else {
            return reply.err(format!("{} isn't running", found.plugin));
        };
        installed.host.send_agent_message(&found.state, text, self.prefs.agents_message, reply);
    }

    /// Closes a pane of any project, by the id plugins and Claude know it by: Claude's `close_pane` and a plugin's
    /// `closePane`. One in a project that isn't on screen closes there, out of sight.
    pub(super) fn close_named_pane(
        &mut self,
        pane: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let id = self
            .plugin_pane(pane)
            .ok_or_else(|| format!("No pane {pane}. It was closed."))?;
        let hidden = self.parked.iter().find_map(|(project, parked)| {
            parked
                .terminals
                .iter()
                .any(|tab| tab.layout.panes().contains(&id))
                .then(|| project.clone())
        });
        if let Some(project) = hidden {
            self.close_hidden_pane(&project, id, cx);
        } else if self.terminals.iter().any(|tab| tab.layout.panes().contains(&id)) || self.is_page_terminal(id) {
            self.close_pane(id, window, cx);
        } else {
            return Err(format!("Pane {pane} isn't in a tab. Close it in Wings."));
        }
        Ok(())
    }
}

/// The app's side of the socket.
pub(super) struct Server {
    env: Arc<AppEnv>,
    sessions: Sessions,
    info: Option<Arc<Store>>,
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
) -> std::io::Result<()> {
    let clients = env.clients.clone();
    let server = Arc::new(Server {
        env,
        sessions,
        info,
        opened: Default::default(),
        notified: Default::default(),
    });
    server::start(path, server, clients)
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
        if name == "performance" {
            return Some(performance(&self.env, arguments, &std::thread::sleep));
        }
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
        if let Some(tab) = snapshot.tabs.iter().find(|t| t.id == pane) {
            return Ok(
                json!({ "busy": tab.state == AgentState::Working, "agent": true, "tab": true, "sessionId": tab.session_id }),
            );
        }
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
        images: answer["images"].as_array().cloned().unwrap_or_default(),
    })
}

impl Handler for Server {
    fn tools(&self) -> Vec<Value> {
        wings_agents::history::mcp_tools()
            .into_iter()
            .chain(panes::tools())
            .chain([performance_tool()])
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
                    images: Vec::new(),
                },
                Err(text) => ToolResult {
                    text,
                    is_error: true,
                    images: Vec::new(),
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
        // A chat's Claude Code never runs the status line, so plugins learn its account from the tab that runs it.
        let id = input.get("session_id").and_then(Value::as_str);
        if let Some(account) = snapshot
            .tabs
            .iter()
            .find(|tab| id.is_some() && tab.session_id.as_deref() == id)
            .and_then(|tab| tab.account.clone())
        {
            caller.session["account"] = json!(account);
        }
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
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                let account = request.get("account").unwrap_or(&Value::Null);
                self.env.statusline.lock().unwrap().record(
                    request.get("status").unwrap_or(&Value::Null),
                    account,
                    login_of(account.as_str()).as_deref(),
                    now,
                );
                Ok(Value::Null)
            }
            "pane_busy" => self.pane_busy(
                request
                    .get("pane")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            "plugins" => Ok(self
                .env
                .store
                .as_ref()
                .map_or_else(|| json!([]), |store| json!(store.lock().unwrap().list()))),
            "install" | "link" | "update" | "enable" | "disable" | "remove" => {
                let arg = |key: &str| {
                    request
                        .get(key)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                self.change_plugin(op, arg)
            }
            _ => return None,
        })
    }
}

impl Server {
    /// The `wings plugin` CLI's changes, through the same code as Settings, Plugins, as the desktop's `control.rs`. A
    /// plugin still turns on only once you approve it in the window, which opens its review when it needs one.
    fn change_plugin(&self, op: &str, arg: impl Fn(&str) -> String) -> Result<Value, String> {
        let store = self
            .env
            .store
            .clone()
            .ok_or("This Wings has no plugin folder")?;
        // Downloads and reads happen here, off the window's thread.
        let package = match op {
            // The CLI sends files as absolute paths; anything else is a GitHub link.
            "install" => {
                let source = arg("source");
                Some(if Path::new(&source).is_absolute() {
                    wings_plugins::store::read_package(Path::new(&source))
                        .map(|bytes| (bytes, wings_plugins::store::Source::File))?
                } else {
                    wings_plugins::store::fetch_github(&source)?
                })
            }
            "update" => {
                let source = store.lock().unwrap().source(&arg("plugin"));
                let (repo, folder) = wings_plugins::store::update_source(source)?;
                let bytes = wings_plugins::store::download_github(&repo, folder.as_deref())?;
                Some((bytes, wings_plugins::store::Source::Github { repo, folder }))
            }
            _ => None,
        };
        let (op, id, folder) = (op.to_owned(), arg("plugin"), arg("folder"));
        self.env.on_window(WINDOW_WAIT, move |workspace, window, cx| -> Result<Value, String> {
            // The window lists it again, and opens the review of one that isn't on yet.
            let installed = |workspace: &mut Workspace, view: wings_plugins::store::PluginView, window: &mut Window, cx: &mut Context<Workspace>| {
                if !(view.enabled && view.approved) {
                    workspace.review_plugin(&view.manifest.id, window, cx);
                }
                json!(view)
            };
            match op.as_str() {
                "install" | "update" => {
                    let (bytes, source) = package.expect("read above");
                    let expected = (op == "update").then_some(id.as_str());
                    let view = workspace.install_package(&bytes, source, expected, window, cx)?;
                    Ok(installed(workspace, view, window, cx))
                }
                // `cargo wings-plugin dev` sends this after each build. Linking a linked folder again is the reload.
                "link" => {
                    let linked = workspace.link_plugin(Path::new(&folder), window, cx)?;
                    if linked.review {
                        workspace.review_plugin(&linked.view.manifest.id, window, cx);
                    }
                    Ok(json!(linked.view))
                }
                "enable" => {
                    let grant = store.lock().unwrap().current_grant(&id)?;
                    match grant {
                        Some(grant) => workspace.set_plugin_enabled(&id, true, Some(grant), window, cx).map(|view| json!(view)),
                        None => {
                            workspace.review_plugin(&id, window, cx);
                            Err(format!("{id} needs your approval first. Review its access in Wings, which has it open now."))
                        }
                    }
                }
                "disable" => workspace.set_plugin_enabled(&id, false, None, window, cx).map(|view| json!(view)),
                _ => workspace.remove_plugin(&id, window, cx).map(|()| Value::Null),
            }
        })?
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

    fn row(&self, id: &str) -> Option<PaneRow> {
        let mut rows = self.snapshot.panes.iter().chain(&self.snapshot.pages);
        rows.find(|row| row.id == id).cloned()
    }

    fn agents(&self) -> Vec<Agent> {
        self.snapshot.agents.clone()
    }

    fn tabs(&self) -> Vec<TabRow> {
        self.snapshot.tabs.clone()
    }

    fn spaces(&self) -> Vec<Space> {
        self.snapshot.spaces.clone()
    }

    fn output(&self, pane_id: &str) -> Option<Vec<u8>> {
        let pane = pane_id.to_owned();
        self.server
            .env
            .on_window(WINDOW_WAIT, move |workspace, _, cx| workspace.pane_output(&pane, cx))
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
                workspace.close_named_pane(&pane, window, cx)
            })??;
        self.server.opened.lock().unwrap().retain(|p| p != pane_id);
        Ok(())
    }

    fn notify(&self, title: String, body: String, pane: Option<String>) -> Result<(), String> {
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
        self.server.env.show_notification(title, body, pane);
        Ok(())
    }

    fn send(&self, pane_id: &str, text: &str) -> Result<bool, String> {
        let tab = super::agent_terminals::page_of_caller(pane_id);
        let (pane, text) = (pane_id.to_owned(), text.to_owned());
        let answer = self
            .server
            .env
            .on_window(WINDOW_WAIT, move |workspace, _, cx| {
                let (reply, answer) = Reply::channel();
                match tab {
                    Some(tab) => workspace.send_to_tab_agent(tab, &text, reply),
                    None => workspace.send_to_agent(&pane, &text, reply, cx),
                }
                answer
            })?;
        // With Enter on, the answer comes once Enter was pressed or skipped, a moment after the paste.
        let answer = answer.wait(WINDOW_WAIT);
        let mut answer: Value = serde_json::from_str(&answer.ok_or("The window didn't answer")??)
            .map_err(|e| e.to_string())?;
        // A tab's page answers as a tool does: `{ text, isError }`, the text being its `{ submitted }`.
        if tab.is_some() {
            let said = answer["text"].as_str().unwrap_or_default().to_owned();
            if answer["isError"] == true {
                return Err(said);
            }
            answer = serde_json::from_str(&said).map_err(|e| e.to_string())?;
        }
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

    fn create_worktree(
        &self,
        space_id: &str,
        branch: &str,
        base: Option<&str>,
    ) -> Result<Made, String> {
        let made = self.server.env.spaces().create_worktree_with(
            space_id,
            branch,
            base,
            Existing::Use,
        )?;
        // Before the answer, so the window lists it by the time the agent opens a pane there.
        self.server.env.send(UiEvent::Projects);
        Ok(made)
    }

    /// Asks in the window, as the desktop's `approve_setup`: one prompt per project at a time, so a second request can't
    /// take the place of the one the user is reading, and a notification when the window isn't in front.
    fn approve_setup(
        &self,
        project: &str,
        pane: &str,
        current: Option<&str>,
        proposed: &str,
    ) -> Result<bool, String> {
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
                return Err(format!(
                    "A setup command for {project} is already waiting for the user. Try again after they answer."
                ));
            }
            waiting.push(project.to_owned());
            Waiting(project.to_owned())
        };
        let ask = (
            project.to_owned(),
            pane.to_owned(),
            current.map(str::to_owned),
            proposed.to_owned(),
        );
        let (answered, behind) =
            self.server
                .env
                .on_window(WINDOW_WAIT, move |workspace, _, cx| {
                    let (project, pane, current, proposed) = ask;
                    let behind = !workspace.window_active;
                    (
                        workspace.ask_setup_approval(project, pane, current, proposed, cx),
                        behind,
                    )
                })?;
        if behind {
            // So the user comes back to answer; a refused one (rate limit) doesn't stop the prompt.
            let _ = self.notify(
                "Claude: Setup command".into(),
                wings_plugins::permissions::clip(
                    &format!("An agent asks to set the setup command for {project}"),
                    200,
                ),
                None,
            );
        }
        answered
            .recv_timeout(panes::SETUP_APPROVAL_WAIT)
            .map_err(|_| "The user didn't answer".to_string())
    }

    fn approve_worktree_setup(
        &self,
        project: &str,
        pane: &str,
        folder: &std::path::Path,
        files: &[String],
        command: Option<&str>,
    ) -> Result<bool, String> {
        let ask = (
            project.to_owned(),
            pane.to_owned(),
            folder.to_string_lossy().into_owned(),
            files.to_vec(),
            command.map(str::to_owned),
        );
        let (answered, behind) = self
            .server
            .env
            .on_window(WINDOW_WAIT, move |workspace, _, cx| {
                let (project, pane, folder, files, command) = ask;
                let behind = !workspace.window_active;
                (
                    workspace.ask_existing_worktree_setup_approval(project, pane, folder, files, command, cx),
                    behind,
                )
            })?;
        if behind {
            let _ = self.notify(
                "Claude: Worktree setup".into(),
                wings_plugins::permissions::clip(
                    &format!("An agent asks to set up an existing worktree for {project}"),
                    200,
                ),
                None,
            );
        }
        answered
            .recv_timeout(panes::SETUP_APPROVAL_WAIT)
            .map_err(|_| "The user didn't answer".to_string())
    }
}

// ---- Connecting Claude Code ----

/// The `wings` program Claude Code's config points at: the one beside this app's binary.
pub(super) fn wings_program() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.with_file_name("wings")).filter(|wings| wings.is_file())
}

/// Who is signed in to the account a status line came from. A status comes every few seconds from every session and
/// the config it is read from is some hundred KB, so it is read again only when the file changed.
pub(super) fn login_of(account: Option<&str>) -> Option<String> {
    type Read = (Option<std::time::SystemTime>, Option<String>);
    static KNOWN: std::sync::Mutex<Option<std::collections::HashMap<Option<String>, Read>>> = std::sync::Mutex::new(None);
    let account = account.filter(|name| claude_accounts::valid_name(name));
    let changed = std::fs::metadata(claude_accounts::user_config(account)).and_then(|file| file.modified()).ok();
    let mut known = KNOWN.lock().unwrap();
    let read = known.get_or_insert_with(Default::default).entry(account.map(str::to_string)).or_insert((None, None));
    if read.0 != changed || changed.is_none() {
        *read = (changed, claude_accounts::login(account));
    }
    read.1.clone()
}

/// The file that says you disconnected Claude Code, beside the app's own state, so it isn't connected again on start.
pub(super) fn disconnected_marker(state: &Path) -> PathBuf {
    state.with_file_name(register::DISCONNECTED)
}

/// What starting the app does, as the desktop does: in a release build of the installed app, unless you disconnected
/// it, Claude Code gets Wings in the default account, in each one a project uses and in each one you are signed in to. Runs `claude`, so off the window's
/// thread.
pub(super) fn connect_on_start(socket: PathBuf, state: Option<PathBuf>, spaces: Vec<Space>) {
    let Some((claude, exe)) = to_connect(state.as_deref()) else {
        return;
    };
    std::thread::spawn(move || {
        for account in claude_accounts::connect_targets_with(&spaces, claude_accounts::signed_in_names()) {
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

/// Wings in the Claude Code of a named account as a plugin picks it, as the desktop's `connect_claude_account_if_needed`:
/// only where `connect_on_start` would connect, and only when the account's files don't show Wings there already. A
/// failure is logged: the account is used all the same. May run `claude`, so off the window's thread.
pub(super) fn connect_account(socket: &Path, state: Option<&Path>, name: &str) {
    let Some((claude, exe)) = to_connect(state) else {
        return;
    };
    if let Err(error) = register::connect_if_needed(&claude, &exe, socket, Some(name)) {
        eprintln!("Couldn't connect Claude Code ({name}): {error}");
    }
}

/// Brings the `settings.json` copy of the account a terminal opens in, the folder `account`, up to date with the default
/// account's in `default`, as the desktop's `pane_create` does: what you change there reaches a named account the next
/// time a terminal opens for it. When the default's had changed, writing the copy took 5 ms (median) and once 0.5 s on a
/// full disk, so not on the window's thread; it is written whole, so Claude Code reads the old copy or the new one.
pub(super) fn sync_account_settings(
    default: Option<PathBuf>,
    account: Option<PathBuf>,
    cx: &gpui::App,
) {
    let (Some(default), Some(account)) = (default, account) else {
        return;
    };
    cx.background_executor()
        .spawn(async move {
            if let Err(error) = claude_accounts::sync_shared_settings_with(&default, &account) {
                eprintln!(
                    "Couldn't copy the default account's settings to {}: {error}",
                    account.display()
                );
            }
        })
        .detach();
}

#[cfg(all(test, feature = "test-support"))]
#[test]
fn a_terminal_opening_in_an_account_brings_its_settings_copy_up_to_date() {
    let dir = std::env::temp_dir().join(format!("wings-account-settings-{}", std::process::id()));
    let (default, account) = (dir.join("default"), dir.join("account"));
    std::fs::create_dir_all(&default).unwrap();
    std::fs::create_dir_all(&account).unwrap();
    let app = gpui::TestApp::new();
    let open = |theme: &str| {
        std::fs::write(
            default.join("settings.json"),
            json!({ "theme": theme }).to_string(),
        )
        .unwrap();
        app.read(|cx| sync_account_settings(Some(default.clone()), Some(account.clone()), cx));
        app.run_until_parked();
        let copy: Value =
            serde_json::from_str(&std::fs::read_to_string(account.join("settings.json")).unwrap())
                .unwrap();
        copy["theme"].clone()
    };
    assert_eq!(open("dark"), "dark");
    assert_eq!(
        open("light"),
        "light",
        "what changed in the default reaches the account"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The `claude` program and the `wings` program to write into Claude Code's config without being asked: only a release
/// build of the installed app, as a dev build would point Claude Code at itself, and not once you disconnected it.
fn to_connect(state: Option<&Path>) -> Option<(PathBuf, PathBuf)> {
    if cfg!(debug_assertions) || disconnected(state) {
        return None;
    }
    let exe = register::installed_wings(&std::env::current_exe().ok()?)?;
    Some((wings_plugins::exec::find_program("claude")?, exe))
}

/// Whether you disconnected Claude Code, so Wings leaves its config alone. An app that keeps no state never connects it.
fn disconnected(state: Option<&Path>) -> bool {
    state.is_none_or(|state| disconnected_marker(state).exists())
}

#[cfg(test)]
#[test]
fn claude_code_you_disconnected_is_not_connected_again() {
    let dir = std::env::temp_dir().join(format!("wings-disconnected-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let state = dir.join("state.json");
    assert!(!disconnected(Some(&state)));
    std::fs::write(disconnected_marker(&state), "").unwrap();
    assert!(disconnected(Some(&state)));
    assert!(disconnected(None), "an app keeping no state, as in tests");
    std::fs::remove_dir_all(&dir).unwrap();
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
        let state = self.config.state.clone();
        let marker = state.as_deref().map(disconnected_marker);
        self.claude.busy = true;
        if change.is_some() {
            self.claude.error = None;
        }
        cx.notify_workspace();
        let work = cx.background_executor().spawn(async move {
            let claude = wings_plugins::exec::find_program("claude");
            let result = match change {
                None => Ok(()),
                Some(Change::Connect(always_load)) => (|| {
                    let claude = claude.as_deref().ok_or("Claude Code isn't installed, or `claude` isn't on your PATH")?;
                    let exe = wings_program().ok_or("The `wings` program isn't beside this app, so Claude Code would have nothing to run")?;
                    register::connect(claude, &exe, &socket, &claude_accounts::connect_targets_with(&spaces, claude_accounts::signed_in_names()), always_load)?;
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
            if change.is_some() && result.is_ok() {
                super::journal::sync(state.as_deref());
            }
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
                cx.notify_workspace();
            });
        }));
    }

    /// Shows these accounts in Settings, Agents instead of reading Claude Code's, for screenshots.
    #[cfg(feature = "test-support")]
    pub fn show_claude_accounts(&mut self, accounts: Vec<AccountStatus>, cx: &mut Context<Self>) {
        self.claude.accounts = Some(accounts);
        cx.notify_workspace();
    }
}

/// Claude in a chat tab runs as a program the tab's page started, under no pane's shell: a call from it comes from the tab,
/// in the tab's project, which plugins' tools are told.
#[cfg(test)]
#[test]
fn a_call_from_a_program_a_tabs_page_runs_comes_from_that_tab() {
    let program = std::process::id();
    let snapshot = Snapshot {
        shells: HashMap::from([(program, "tab-4".to_owned())]),
        spaces: vec![serde_json::from_value(json!({ "id": "/work/shop", "name": "shop", "path": "/work/shop" })).unwrap()],
        pages: vec![PaneRow {
            id: "tab-4".into(),
            space_id: "/work/shop".into(),
            cwd: "/work/shop".into(),
            command: None,
            title: "Chat".into(),
        }],
        ..Snapshot::default()
    };
    let pane = snapshot.pane(Some(program), None);
    assert_eq!(pane.as_deref(), Some("tab-4"));
    assert_eq!(snapshot.caller(pane.clone()).project["path"], "/work/shop");
    // Its session is the one the tab's page says it runs, so an issue it touches knows it, and the journal links its turns.
    assert!(snapshot.caller(pane.clone()).session.is_null());
    let snapshot = Snapshot {
        tabs: vec![TabRow {
            id: "tab-4".into(),
            space_id: "/work/shop".into(),
            cwd: "/work/shop".into(),
            plugin: "claude-chat".into(),
            title: "Chat".into(),
            state: AgentState::Working,
            name: "Fix the checkout".into(),
            summary: None,
            session_id: Some("s-1".into()),
            account: None,
        }],
        ..snapshot
    };
    assert_eq!(snapshot.caller(pane).session, json!({ "id": "s-1", "name": "Fix the checkout", "kind": "claude" }));
}
