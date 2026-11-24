//! What the window gives its plugins, and what it takes from them: the panes plugins are told of, the badges, status
//! strips, notifications and URLs they ask for, the transcript entries Claude writes and the clicks on a badge's buttons.
//!
//! Plugins run on threads of their own, so they reach the window through `AppEnv`, which hands each request to the
//! window as an event; the window applies them on its own thread. The checks are `wings-plugins`' and the host's
//! (`page::plugin::calls`): by the time a request gets here the plugin was allowed to make it.

use crate::workspace::NotifyWorkspace;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_channel::Sender;
use gpui::{Context, SharedString, Task, Window};
use wings_plugins::transcript::{Line, transcript_event};
use wings_spaces::{
    claude_accounts::{self, Accounts, AuthStatus},
    spaces::WorktreeSetup,
};

use crate::{
    agents::AgentKind,
    page::plugin::{
        Env, Notification, OpenPane, OpenView, PaneEntry, PanePort, Project, Running, SectionRow,
        SessionEntry, ToolCall,
        ui::{Badge, PaneStatus, SidebarLabel},
    },
    ports,
    projects::{Projects, with_setup},
};

use super::{Active, Workspace, parts::Piece};

/// How long a badge's button waits for the plugin before it says the plugin didn't respond: just over the 5 minutes
/// `exec` may take, so an action waiting on a long exec can finish (the desktop's `ask`).
pub(super) const ACTION_TIMEOUT: Duration = Duration::from_secs(310);
/// How long after pasting a message Enter is pressed, as the desktop's `sendToPane`.
const SUBMIT_AFTER: Duration = Duration::from_millis(150);
/// How long a notice the system couldn't show stays in the window.
const NOTICE_FOR: Duration = Duration::from_secs(8);
/// How long a plugin's `readPane` waits for the window to hand over a pane's output, as Claude Code's tools wait.
const PANE_WAIT: Duration = Duration::from_secs(10);

#[cfg(test)]
#[test]
fn dropping_plugin_ui_stops_its_git_watcher() {
    struct End(std::sync::mpsc::Sender<()>);
    impl Drop for End {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let (ended, rx) = std::sync::mpsc::channel();
    let end = End(ended);
    let ui = PluginUi {
        watcher: Some(wings_gitwatch::Watcher::start(
            move || {
                let _keep = &end;
                Vec::new()
            },
            |_| {},
        )),
        ..PluginUi::default()
    };
    drop(ui);
    rx.recv_timeout(Duration::from_secs(5))
        .expect("the watcher thread outlived the workspace's plugin UI");
}

/// The plugins that need a native build, as `(id, name, built for JavaScript)`, for `plugins.json` as written then and the plugins running.
pub(super) type NeedsUpdate = (
    Option<(Option<std::time::SystemTime>, Vec<String>)>,
    Vec<(String, String, bool)>,
);

/// Work for the window's thread, with the window.
pub(super) type WindowTask =
    Box<dyn FnOnce(&mut Workspace, &mut gpui::Window, &mut Context<Workspace>) + Send>;

/// What a plugin asked of the window, for the window to do.
pub(super) enum UiEvent {
    SaveTabState,
    /// Something to do on the window's thread for a call from Claude Code (`claude`), which waits for it.
    Run(WindowTask),
    /// `Run` that doesn't draw the window afterwards, for the `performance` tool, which must not make frames itself.
    RunQuietly(WindowTask),
    FocusPane {
        pane: String,
        reply: crate::page::runtime::Reply,
    },
    ClosePane {
        pane: String,
        reply: crate::page::runtime::Reply,
    },
    OpenDiff(crate::diff::View),
    UpdateDiff {
        plugin: String,
        id: String,
        update: crate::diff::Update,
    },
    CloseDiff {
        plugin: String,
        id: String,
    },
    OpenPane {
        plan: OpenPane,
        reply: crate::page::runtime::Reply,
    },
    OpenView {
        plan: OpenView,
        reply: crate::page::runtime::Reply,
    },
    /// A page's `openHistory`: the Past sessions sheet, resuming sessions of `agents` in the page.
    OpenHistory {
        agents: Vec<String>,
        reply: crate::page::runtime::Reply,
    },
    CloseView {
        plugin: String,
        view: String,
    },
    /// Paste a message into the prompt of the agent in a pane.
    SendToAgent {
        pane: String,
        text: String,
        reply: crate::page::runtime::Reply,
    },
    /// Show a plugin's settings page.
    OpenSettings {
        plugin: String,
    },
    /// The project list changed, as a plugin made a worktree.
    Projects,
    Git(Vec<String>),
    Badge {
        plugin: String,
        pane: String,
        badge: Option<Badge>,
    },
    Status {
        plugin: String,
        pane: String,
        status: Option<PaneStatus>,
    },
    SidebarLabel {
        plugin: String,
        page: String,
        label: Option<SidebarLabel>,
    },
    /// A notification for the system to show as the app's own, which only the window's thread can post, and the pane a
    /// click on it goes to.
    SystemNotification {
        notification: gpui::SystemNotification,
        pane: Option<String>,
    },
    /// A notification the system couldn't show, for the window to show itself.
    Notice {
        title: String,
        body: String,
    },
    /// A plugin's notification, for the inbox.
    Notified(Notification),
    /// A page's `copyText`.
    Copy(String),
    /// A page's `openFile`, checked to be a file in one of your projects.
    OpenFile {
        file: PathBuf,
        line: Option<u32>,
    },
    /// A page's `openLink`: what the plugins' links make of the text.
    OpenLink(String),
    ProjectSection {
        plugin: String,
        project: String,
        section: String,
        rows: Option<Vec<SectionRow>>,
    },
    SectionSort {
        plugin: String,
        section: String,
        sort: String,
    },
}

/// What the system does for a plugin: open a URL, show a notification. Faked in tests, which must not open anything.
pub(super) trait System: Send + Sync + 'static {
    fn open_url(&self, url: &str) -> Result<(), String>;
    /// `pane` is where a click on the notification goes.
    fn notify(&self, title: &str, body: &str, pane: Option<String>) -> Result<(), String>;
    /// Makes a named Claude Code account's folder and gives it its share of the default account's setup (unless that is
    /// off), as the desktop's `prepare_claude_account`. The terminal starts all the same if sharing fails.
    fn prepare_claude_account(&self, name: &str) -> Result<(), String>;
    /// Turns on or off that a named account uses the default account's setup (`claude_accounts::set_shared_setup`).
    fn share_claude_setup(&self, name: &str, on: bool) -> Result<(), String>;
    /// Wings in a named account's Claude Code unless you disconnected it or it is there already, as the desktop's
    /// `connect_claude_account_if_needed`. Logged when it fails: the account is used all the same.
    fn connect_claude_account(&self, name: &str);
    /// What `claude auth status` says for an account, `None` being the default one.
    fn claude_auth(&self, name: Option<&str>) -> AuthStatus;
}

/// The real system. In the app's bundle a notification is the app's own, so macOS shows Wings' name and icon and a
/// click opens Wings. A bare binary has no bundle to post notifications from (`UNUserNotificationCenter` needs one), so
/// there a notification is `osascript`'s `display notification`, which any process may use but macOS credits to Script
/// Editor. The title and body go in as the script's arguments, never in its text, so nothing a plugin writes is read as
/// script.
struct Os {
    /// The socket Claude Code's config points at, and the app's state beside which the disconnected marker is, as the
    /// app's `Config` has them. Without a socket Claude Code is left alone.
    socket: Option<PathBuf>,
    state: Option<PathBuf>,
    /// The window's events, to post notifications through when the app runs from its bundle.
    bundle: Option<Sender<UiEvent>>,
}

/// Whether the app runs from its bundle: gpui posts no notifications outside one, and this is the check it makes.
#[cfg(target_os = "macos")]
fn in_bundle() -> bool {
    objc2_foundation::NSBundle::mainBundle()
        .bundleIdentifier()
        .is_some()
}

#[cfg(not(target_os = "macos"))]
fn in_bundle() -> bool {
    false
}

fn succeeded(what: &str, result: std::io::Result<std::process::ExitStatus>) -> Result<(), String> {
    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{what} failed ({status})")),
        Err(error) => Err(format!("{what} isn't available: {error}")),
    }
}

impl System for Os {
    fn open_url(&self, url: &str) -> Result<(), String> {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        succeeded(opener, Command::new(opener).arg(url).status())
    }

    fn notify(&self, title: &str, body: &str, pane: Option<String>) -> Result<(), String> {
        if let Some(events) = &self.bundle {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let notification = gpui::SystemNotification {
                // A tag of its own, so it never replaces an earlier one still in Notification Center.
                tag: format!(
                    "{}.{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                )
                .into(),
                title: title.to_owned().into(),
                body: body.to_owned().into(),
                actions: Vec::new(),
            };
            return events
                .try_send(UiEvent::SystemNotification { notification, pane })
                .map_err(|_| "The Wings window is closed or busy".into());
        }
        if cfg!(target_os = "macos") {
            succeeded(
                "osascript",
                Command::new("osascript")
                    .args([
                        "-e",
                        "on run argv",
                        "-e",
                        "display notification (item 2 of argv) with title (item 1 of argv)",
                        "-e",
                        "end run",
                    ])
                    .args([title, body])
                    .status(),
            )
        } else {
            succeeded(
                "notify-send",
                Command::new("notify-send").args([title, body]).status(),
            )
        }
    }

    fn prepare_claude_account(&self, name: &str) -> Result<(), String> {
        claude_accounts::create_dir(name).map_err(|error| error.to_string())?;
        if let Err(error) = claude_accounts::sync_shared_setup(name) {
            eprintln!("[claude] couldn't share the default account's setup with {name}: {error}");
        }
        Ok(())
    }

    fn share_claude_setup(&self, name: &str, on: bool) -> Result<(), String> {
        claude_accounts::create_dir(name).map_err(|error| error.to_string())?;
        claude_accounts::set_shared_setup(name, on)
    }

    fn connect_claude_account(&self, name: &str) {
        if let Some(socket) = &self.socket {
            super::claude::connect_account(socket, self.state.as_deref(), name);
        }
    }

    /// Half a second, from `claude`; the answer is on stdout whatever the exit code, which is 1 when signed out. Not
    /// signed in when `claude` can't run.
    fn claude_auth(&self, name: Option<&str>) -> AuthStatus {
        let Some(claude) = wings_plugins::exec::find_program("claude") else {
            return AuthStatus::default();
        };
        let mut command = Command::new(claude);
        if let Some(name) = name {
            command.env("CLAUDE_CONFIG_DIR", claude_accounts::dir(Some(name)));
        }
        let out = command
            .args(["auth", "status"])
            .stdin(std::process::Stdio::null())
            .output();
        out.map(|out| claude_accounts::read_auth_status(&String::from_utf8_lossy(&out.stdout)))
            .unwrap_or_default()
    }
}

/// What the window tells the plugins' hosts it knows: the project on screen, and where each pane's shell is.
#[derive(Default)]
struct Known {
    current: Option<PathBuf>,
    shells: Vec<(String, Option<u32>)>,
}

/// The window, as the plugins' hosts see it.
pub(super) struct AppEnv {
    events: Sender<UiEvent>,
    known: Mutex<Known>,
    /// The native app's projects, which the window shows and plugins change.
    projects: Arc<Projects>,
    pub(super) running: Running,
    claude_dir: PathBuf,
    system: Mutex<Arc<dyn System>>,
    /// What Claude Code last told `wings statusline`, for `wings.statusline()`.
    pub(super) statusline: Mutex<wings_connect::statusline::Statusline>,
    /// The installed plugins, which the window and the `wings plugin` CLI change, as the desktop's `AppState.plugins`.
    pub(super) store: Option<Arc<Mutex<wings_plugins::store::Store>>>,
    /// The connections of the socket, told when the plugins' tools change.
    pub(super) clients: Arc<wings_connect::server::Clients>,
    /// The past sessions the Past Sessions sheet lists, for `wings.pastSessions`.
    sessions: super::history::Sessions,
    /// The words plugins narrowed their links to, by plugin and link (`wings.setLinkWords`).
    pub(super) link_words: Mutex<HashMap<(String, String), Arc<HashSet<String>>>>,
    /// The terminal servers, which run the programs tabs keep through a restart.
    #[cfg(unix)]
    servers: std::sync::OnceLock<crate::host::TerminalServers>,
}

impl AppEnv {
    pub(super) fn tab_state(
        &self,
        initial: Option<serde_json::Value>,
    ) -> crate::page::plugin::FrameState {
        let (events, redraw, closing, showing) = (self.events.clone(), self.events.clone(), self.events.clone(), self.events.clone());
        crate::page::plugin::FrameState::new(
            crate::page::plugin::StatePlace::Tab,
            initial,
            move |_| {
                events
                    .try_send(UiEvent::SaveTabState)
                    .map_err(|_| "The Wings window is closed or busy".into())
            },
        )
        // The sidebar's Agents shows what the tab's agent does and the tab strip its title; nothing in the main column does.
        .on_agent(move || {
            let _ = redraw.try_send(UiEvent::RunQuietly(Box::new(|workspace, _, cx| {
                workspace.notify_pieces(&[Piece::Sidebar, Piece::TitleBar], cx)
            })));
        })
        .on_close(move |state| {
            let state = state.clone();
            let _ = closing.try_send(UiEvent::Run(Box::new(move |workspace, window, cx| {
                if let Some(index) = workspace.plugin_tabs.iter().position(|tab| tab.state.same(&state)) {
                    workspace.close_plugin_tab(index, window, cx);
                }
            })));
        })
        .on_show(move |state| {
            let state = state.clone();
            let _ = showing.try_send(UiEvent::Run(Box::new(move |workspace, window, cx| workspace.show_tab_of(&state, window, cx))));
        })
    }
    pub(super) fn new(
        events: Sender<UiEvent>,
        claude_dir: PathBuf,
        projects: Arc<Projects>,
        store: Option<wings_plugins::store::Store>,
        socket: Option<PathBuf>,
        state: Option<PathBuf>,
        sessions: super::history::Sessions,
    ) -> Self {
        let os = Os {
            socket,
            state,
            bundle: in_bundle().then(|| events.clone()),
        };
        Self {
            events,
            known: Mutex::default(),
            projects,
            running: Running::default(),
            claude_dir,
            system: Mutex::new(Arc::new(os)),
            statusline: Mutex::default(),
            store: store.map(|store| Arc::new(Mutex::new(store))),
            clients: Arc::default(),
            sessions,
            link_words: Mutex::default(),
            #[cfg(unix)]
            servers: std::sync::OnceLock::new(),
        }
    }

    /// The terminal servers, which run the programs tabs keep through a restart. Set once, before any plugin starts.
    #[cfg(unix)]
    pub(super) fn use_servers(&self, servers: Option<crate::host::TerminalServers>) {
        if let Some(servers) = servers {
            let _ = self.servers.set(servers);
        }
    }

    /// Records in `plugins.json` that the plugin stored or deleted the secret, as the desktop does, so removing the
    /// plugin deletes it. Only under the scope on file: a dev plugin's `dev` scope has no entry to record it in.
    fn note_secret(
        &self,
        plugin: &str,
        scope: &str,
        name: &str,
        stored: bool,
    ) -> Result<(), String> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        let mut store = store.lock().unwrap();
        if store.known_storage_scope(plugin).as_deref() != Some(scope) {
            return Ok(());
        }
        store.note_secret(plugin, name, stored)
    }

    /// Runs `task` on the window's thread and waits up to `wait` for what it returns, for a call from Claude Code.
    pub(super) fn on_window<T: Send + 'static>(
        &self,
        wait: Duration,
        task: impl FnOnce(&mut Workspace, &mut gpui::Window, &mut Context<Workspace>) -> T
        + Send
        + 'static,
    ) -> Result<T, String> {
        self.run_on_window(UiEvent::Run, wait, task)
    }

    /// `on_window`, without the draw of the window that follows it: for what must not make a frame (`performance`).
    pub(super) fn on_window_quietly<T: Send + 'static>(
        &self,
        wait: Duration,
        task: impl FnOnce(&mut Workspace, &mut gpui::Window, &mut Context<Workspace>) -> T
        + Send
        + 'static,
    ) -> Result<T, String> {
        self.run_on_window(UiEvent::RunQuietly, wait, task)
    }

    fn run_on_window<T: Send + 'static>(
        &self,
        event: fn(WindowTask) -> UiEvent,
        wait: Duration,
        task: impl FnOnce(&mut Workspace, &mut gpui::Window, &mut Context<Workspace>) -> T
        + Send
        + 'static,
    ) -> Result<T, String> {
        let (done, answer) = std::sync::mpsc::channel();
        self.events
            .try_send(event(Box::new(move |workspace, window, cx| {
                let _ = done.send(task(workspace, window, cx));
            })))
            .map_err(|_| "The Wings window is closed or busy".to_string())?;
        answer
            .recv_timeout(wait)
            .map_err(|_| "The Wings window didn't answer in time".to_string())
    }

    /// The native app's projects, for Claude Code's tools.
    pub(super) fn spaces(&self) -> &Projects {
        &self.projects
    }

    /// Shows a notification for Claude Code: the system's, which a click on takes to `pane`, else the window's own.
    pub(super) fn show_notification(&self, title: String, body: String, pane: Option<String>) {
        if self.system().notify(&title, &body, pane).is_err() {
            self.send(UiEvent::Notice { title, body });
        }
    }

    /// Tests put their own in place of the system, which would open a browser.
    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn use_system(&self, system: Arc<dyn System>) {
        *self.system.lock().unwrap() = system;
    }

    fn system(&self) -> Arc<dyn System> {
        self.system.lock().unwrap().clone()
    }

    pub(super) fn send(&self, event: UiEvent) {
        // Closed when the window is, and a plugin that outlives it has no one to show anything to.
        let _ = self.events.try_send(event);
    }
}

impl Env for AppEnv {
    #[cfg(unix)]
    fn job_server(&self) -> Option<PathBuf> {
        self.servers.get()?.0.current()
    }

    #[cfg(unix)]
    fn replace_job_server(&self, gone: &std::path::Path, error: &str) -> Option<PathBuf> {
        let servers = &self.servers.get()?.0;
        servers.note(&format!(
            "A tab's program couldn't start in the terminal server at {}: {error}",
            gone.display()
        ));
        servers.replace(gone)
    }

    #[cfg(unix)]
    fn left_jobs(&self, plugin: &str) -> Vec<crate::page::plugin::LeftJob> {
        let Some(servers) = self.servers.get() else {
            return Vec::new();
        };
        servers
            .0
            .claim_jobs(plugin)
            .into_iter()
            .map(|(socket, info)| crate::page::plugin::LeftJob { socket, info })
            .collect()
    }

    fn past_sessions(&self, project: &std::path::Path, query: &str, limit: usize) -> Vec<serde_json::Value> {
        let filter = wings_agents::history::Filter {
            project: Some(project.to_string_lossy().into_owned()),
            ..Default::default()
        };
        self.sessions
            .search(query, &filter, limit)
            .sessions
            .into_iter()
            .filter(|hit| hit.agent == wings_agents::history::Agent::Claude)
            .map(|hit| {
                let account = self.sessions.account_label(&hit).map(|name| name.to_string());
                serde_json::json!({
                    "id": hit.id, "title": hit.name.or(hit.title), "firstPrompt": hit.first_prompt, "cwd": hit.cwd,
                    "account": account,
                    "gitBranch": hit.git_branch, "lastActiveMs": hit.last_active_ms, "messages": hit.messages,
                    "matches": hit.matches,
                })
            })
            .collect()
    }
    fn focus_pane(&self, pane: &str, reply: crate::page::runtime::Reply) {
        let event = UiEvent::FocusPane { pane: pane.into(), reply: reply.clone() };
        if self.events.try_send(event).is_err() {
            reply.err("The Wings window is closed or busy".into());
        }
    }
    fn close_pane(&self, pane: &str, reply: crate::page::runtime::Reply) {
        let event = UiEvent::ClosePane { pane: pane.into(), reply: reply.clone() };
        if self.events.try_send(event).is_err() {
            reply.err("The Wings window is closed or busy".into());
        }
    }
    fn open_diff(&self, view: crate::diff::View) -> Result<(), String> {
        self.events
            .try_send(UiEvent::OpenDiff(view))
            .map_err(|_| "The Wings window is closed or busy".into())
    }
    fn update_diff(
        &self,
        plugin: &str,
        id: &str,
        update: crate::diff::Update,
    ) -> Result<(), String> {
        self.events
            .try_send(UiEvent::UpdateDiff {
                plugin: plugin.into(),
                id: id.into(),
                update,
            })
            .map_err(|_| "The Wings window is closed or busy".into())
    }
    fn close_diff(&self, plugin: &str, id: &str) {
        self.send(UiEvent::CloseDiff {
            plugin: plugin.into(),
            id: id.into(),
        });
    }
    fn current_project(&self) -> Option<PathBuf> {
        self.known.lock().unwrap().current.clone()
    }

    fn open_pane(&self, mut plan: OpenPane, reply: crate::page::runtime::Reply) {
        // The first pane a plugin opens in a worktree just made runs the project's setup line before the command.
        plan.input = with_setup(
            self.projects.take_setup(&plan.project).as_deref(),
            plan.input,
        );
        if self
            .events
            .try_send(UiEvent::OpenPane {
                plan,
                reply: reply.clone(),
            })
            .is_err()
        {
            reply.err("The Wings window is closed or busy".into());
        }
    }

    fn open_history(&self, agents: Vec<String>, reply: crate::page::runtime::Reply) {
        if self
            .events
            .try_send(UiEvent::OpenHistory {
                agents,
                reply: reply.clone(),
            })
            .is_err()
        {
            reply.err("The Wings window is closed or busy".into());
        }
    }

    fn open_view(&self, plan: OpenView, reply: crate::page::runtime::Reply) {
        if self
            .events
            .try_send(UiEvent::OpenView {
                plan,
                reply: reply.clone(),
            })
            .is_err()
        {
            reply.err("The Wings window is closed or busy".into());
        }
    }
    fn close_view(&self, plugin: &str, view: &str) {
        self.send(UiEvent::CloseView {
            plugin: plugin.to_owned(),
            view: view.to_owned(),
        });
    }

    fn builtin(&self, plugin: &str) -> Option<bool> {
        self.running.builtin(plugin)
    }

    fn view_opens(&self, plugin: &str, id: &str) -> Option<Vec<String>> {
        self.running.view_opens(plugin, id)
    }

    fn emit(&self, from: &str, name: &str, data: &serde_json::Value) {
        self.running.emit(from, name, data);
    }

    fn secret_get(&self, plugin: &str, scope: &str, name: &str) -> Result<Option<String>, String> {
        crate::secrets::get(plugin, scope, name)
    }

    fn secret_set(&self, plugin: &str, scope: &str, name: &str, value: &str) -> Result<(), String> {
        crate::secrets::set(plugin, scope, name, value)?;
        self.note_secret(plugin, scope, name, true)
    }

    fn secret_delete(&self, plugin: &str, scope: &str, name: &str) -> Result<(), String> {
        crate::secrets::delete(plugin, scope, name)?;
        self.note_secret(plugin, scope, name, false)
    }

    fn send_to_agent(&self, pane: &str, text: &str, reply: crate::page::runtime::Reply) {
        if self
            .events
            .try_send(UiEvent::SendToAgent {
                pane: pane.to_owned(),
                text: text.to_owned(),
                reply: reply.clone(),
            })
            .is_err()
        {
            reply.err("The Wings window is closed or busy".into());
        }
    }

    fn open_settings(&self, plugin: &str) -> Result<(), String> {
        self.events
            .try_send(UiEvent::OpenSettings {
                plugin: plugin.to_owned(),
            })
            .map_err(|_| "The Wings window is closed or busy".into())
    }

    fn call_tool(&self, call: ToolCall, reply: crate::page::runtime::Reply) {
        self.running.call_tool(call, reply);
    }

    fn create_worktree(
        &self,
        project: &str,
        branch: &str,
        base: Option<&str>,
    ) -> Result<Project, String> {
        let made = self.projects.create_worktree(project, branch, base)?;
        // Before the answer, so the window lists it by the time the plugin opens a pane there.
        self.send(UiEvent::Projects);
        Ok(made.into())
    }

    fn worktree_setup(&self, project: &str) -> Result<WorktreeSetup, String> {
        self.projects.worktree_setup(project)
    }

    fn set_worktree_setup(
        &self,
        project: &str,
        include: Option<Vec<String>>,
        setup: Option<String>,
    ) -> Result<(), String> {
        self.projects.set_worktree_setup(project, include, setup)
    }

    fn projects(&self) -> Vec<PathBuf> {
        self.projects.paths()
    }

    fn claude_dirs(&self) -> Vec<PathBuf> {
        claude_accounts::dirs_with_default(self.claude_dir.clone())
    }

    fn claude_config_dir(&self, project: &std::path::Path) -> Option<PathBuf> {
        self.projects.claude_config_dir(project)
    }

    fn claude_accounts(&self) -> Result<Accounts, String> {
        let system = self.system();
        Ok(self
            .projects
            .claude_accounts(|name| system.claude_auth(name)))
    }

    fn claude_accounts_quick(&self) -> Result<Accounts, String> {
        Ok(self.projects.claude_accounts(|name| claude_accounts::AuthStatus {
            logged_in: claude_accounts::login(name).is_some(),
            method: None,
        }))
    }

    fn set_claude_account(&self, project: &str, name: Option<&str>) -> Result<(), String> {
        let system = self.system();
        self.projects
            .set_claude_account(project, name, |name| system.prepare_claude_account(name))?;
        // After the project list is let go of, as it may run `claude`. The assignment stands if this fails.
        if let Some(name) = name {
            system.connect_claude_account(name);
        }
        Ok(())
    }

    fn prepare_claude_account(&self, name: &str) -> Result<(), String> {
        let system = self.system();
        system.prepare_claude_account(name)?;
        system.connect_claude_account(name);
        Ok(())
    }

    fn share_claude_setup(&self, name: &str, on: bool) -> Result<(), String> {
        let system = self.system();
        system.share_claude_setup(name, on)?;
        // Turned off, the account has its own settings.json back, which may lack Wings.
        system.connect_claude_account(name);
        Ok(())
    }

    fn agent_activity(&self, session_id: &str) -> Result<serde_json::Value, String> {
        let activity = wings_agents::agent_activity::fetch_live(session_id)?;
        serde_json::to_value(activity).map_err(|error| error.to_string())
    }

    fn statusline(&self) -> serde_json::Value {
        self.statusline.lock().unwrap().view()
    }

    fn record_usage(
        &self,
        account: Option<&str>,
        five_hour: Option<wings_connect::statusline::Window>,
        seven_day: Option<wings_connect::statusline::Window>,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let login = super::claude::login_of(account);
        self.statusline
            .lock()
            .unwrap()
            .record_reported(account, login.as_deref(), five_hour, seven_day, now);
    }

    fn ports(&self) -> Result<Vec<PanePort>, String> {
        let shells = self.known.lock().unwrap().shells.clone();
        ports::scan(&shells)
    }

    fn pane_output(&self, pane: &str) -> Result<Vec<u8>, String> {
        let id = pane.to_owned();
        self.on_window(PANE_WAIT, move |workspace, _, cx| workspace.pane_output(&id, cx))?.ok_or_else(|| format!("no pane {pane}"))
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.system().open_url(url)
    }

    fn open_file(&self, file: &std::path::Path, line: Option<u32>) -> Result<(), String> {
        self.events
            .try_send(UiEvent::OpenFile { file: file.to_owned(), line })
            .map_err(|_| "The Wings window is closed or busy".into())
    }

    fn open_link(&self, text: &str) -> Result<(), String> {
        self.events.try_send(UiEvent::OpenLink(text.to_owned())).map_err(|_| "The Wings window is closed or busy".into())
    }

    fn notify(&self, notification: Notification) -> Result<(), String> {
        let title = notification.system_title();
        // Where the system has no way to show it, the window does, so a notification is never lost.
        if self
            .system()
            .notify(&title, &notification.body, None)
            .is_err()
        {
            self.send(UiEvent::Notice {
                title,
                body: notification.body.clone(),
            });
        }
        self.send(UiEvent::Notified(notification));
        Ok(())
    }

    fn copy_text(&self, text: String) -> Result<(), String> {
        self.events
            .try_send(UiEvent::Copy(text))
            .map_err(|_| "The Wings window is closed or busy".into())
    }

    fn pick_files(&self, multiple: bool, reply: crate::page::runtime::Reply) {
        let spare = reply.clone();
        let event = UiEvent::Run(Box::new(move |_, _, cx| {
            let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple,
                prompt: Some("Attach".into()),
            });
            // The files are read off the window's thread: a pick can be megabytes.
            cx.background_executor()
                .spawn(async move {
                    match picked.await {
                        Ok(Ok(paths)) => {
                            reply.ok(crate::page::plugin::picked_images(&paths.unwrap_or_default()))
                        }
                        Ok(Err(error)) => reply.err(format!("The file picker didn't open: {error}")),
                        Err(_) => reply.ok("[]".into()),
                    }
                })
                .detach();
        }));
        if self.events.try_send(event).is_err() {
            spare.err("The Wings window is closed or busy".into());
        }
    }

    fn set_badge(&self, plugin: &str, pane: &str, badge: Option<Badge>) {
        self.send(UiEvent::Badge {
            plugin: plugin.to_owned(),
            pane: pane.to_owned(),
            badge,
        });
    }

    fn set_pane_status(&self, plugin: &str, pane: &str, status: Option<PaneStatus>) {
        self.send(UiEvent::Status {
            plugin: plugin.to_owned(),
            pane: pane.to_owned(),
            status,
        });
    }

    fn set_sidebar_label(&self, plugin: &str, page: &str, label: Option<SidebarLabel>) {
        self.send(UiEvent::SidebarLabel {
            plugin: plugin.to_owned(),
            page: page.to_owned(),
            label,
        });
    }

    fn set_link_words(&self, plugin: &str, link: &str, words: Option<HashSet<String>>) {
        let key = (plugin.to_owned(), link.to_owned());
        {
            let mut known = self.link_words.lock().unwrap();
            match words {
                Some(words) => known.insert(key, Arc::new(words)),
                None => known.remove(&key),
            };
        }
        self.send(UiEvent::Run(Box::new(|workspace, _, cx| workspace.refresh_links(cx))));
    }

    fn set_project_section(
        &self,
        plugin: &str,
        project: &str,
        section: &str,
        rows: Option<Vec<SectionRow>>,
    ) -> Result<(), String> {
        self.events
            .try_send(UiEvent::ProjectSection {
                plugin: plugin.to_owned(),
                project: project.to_owned(),
                section: section.to_owned(),
                rows,
            })
            .map_err(|_| "The Wings window is closed or busy".into())
    }

    fn set_section_sort(&self, plugin: &str, section: &str, sort: &str) {
        self.send(UiEvent::SectionSort {
            plugin: plugin.to_owned(),
            section: section.to_owned(),
            sort: sort.to_owned(),
        });
    }
}

/// A notification the window shows itself.
pub(super) struct Notice {
    pub(super) id: usize,
    pub(super) title: SharedString,
    pub(super) body: SharedString,
}

/// The plugins' panes, badges and status strips of a window, kept by the window.
#[derive(Default)]
pub(super) struct PluginUi {
    pub(super) diff: Option<(
        gpui::Entity<crate::diff::viewer::Viewer>,
        gpui::Subscription,
    )>,
    /// Where the diff shows: beside the tab it opened from, or filling the window, and the dock's width.
    pub(super) diff_dock: super::diff::Dock,
    pub(super) windows:
        HashMap<(String, String), gpui::WindowHandle<super::plugin_windows::PluginViewWindow>>,
    pub(super) windows_release: Option<gpui::Subscription>,
    /// The plugin view that shows as a dialog over the window, if one does.
    pub(super) modal: Option<super::plugin_modal::PluginModal>,
    pub(super) windows_closed: Option<gpui::Subscription>,
    watcher: Option<wings_gitwatch::Watcher>,
    watched_projects: Vec<PathBuf>,
    /// A hash of what the plugins were last told of the panes and projects, to tell them only of a change.
    pub(super) signature: u64,
    /// By pane, then plugin id, which is also the order they are drawn in.
    pub(super) badges: HashMap<usize, BTreeMap<String, Badge>>,
    pub(super) statuses: HashMap<usize, BTreeMap<String, PaneStatus>>,
    /// The label by each sidebar's or view's title bar button, by plugin id and page id.
    pub(super) labels: HashMap<(String, String), SidebarLabel>,
    pub(super) popover: super::plugin_ui::HoverPopover,
    /// What `Workspace::plugins_without_native_build` last found, and when for.
    pub(super) needs_update: std::cell::RefCell<NeedsUpdate>,
    /// The notifications the system couldn't show, newest last.
    pub(super) notices: Vec<Notice>,
    /// The pane each system notification from one came from, by its tag, until it's clicked.
    notified_panes: HashMap<SharedString, String>,
    /// The plugin buttons kept in the project bar, as `plugin:sidebar`, in the order they were pinned.
    pub(super) pinned: Vec<String>,
    /// The project bar's menu of plugin buttons is open.
    pub(super) buttons_menu: bool,
    next_notice: usize,
    notice_timers: Vec<Task<()>>,
    pub(super) right_slide: super::plugin_ui::RightSlide,
    pub(super) right_size: super::right_edge::Size,
    pub(super) right_split: super::right_stack::SplitDrag,
}

impl AppEnv {
    /// Tells `callTool` which plugins run, once they have started.
    pub(super) fn set_plugins(&self, plugins: &[crate::installed::Installed]) {
        for installed in plugins {
            self.running
                .add(&installed.plugin, installed.builtin, &installed.host);
        }
    }
}

impl Workspace {
    pub(super) fn sync_plugin_visibility(&self, cx: &gpui::App) {
        for tab in &self.plugin_tabs {
            tab.page
                .read(cx)
                .set_visible(self.active == Active::Plugin(tab.id));
        }
        for parked in self.parked.values() {
            for tab in &parked.plugin_tabs {
                tab.page.read(cx).set_visible(false);
            }
        }
        for (key, page) in &self.sidebar_pages {
            page.read(cx).set_visible(self.right.contains(key));
        }
    }
    /// What the window is doing for plugins, on its own thread.
    pub(super) fn apply_plugin_event(
        &mut self,
        event: UiEvent,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        // Each event draws the whole window again, sidebar, title bar and main column, unless it names the pieces that
        // show what it changed, so one that changes nothing on screen returns before that: plugins send the same badge,
        // status or section rows again on every poll.
        let mut shown_in: Option<&[Piece]> = None;
        match event {
            UiEvent::SaveTabState => return self.save(cx),
            UiEvent::Run(task) => task(self, window, cx),
            UiEvent::RunQuietly(task) => return task(self, window, cx),
            UiEvent::FocusPane { pane, reply } => {
                let Some(id) = self.plugin_pane(&pane) else {
                    return reply.err(format!("no pane {pane}"));
                };
                let project = self.parked.iter().find_map(|(path, parked)| {
                    parked
                        .terminals
                        .iter()
                        .any(|tab| tab.layout.panes().contains(&id))
                        .then(|| path.clone())
                });
                if let Some(project) = project {
                    self.switch_project(project, window, cx);
                }
                let Some(tab) = self
                    .terminals
                    .iter()
                    .find(|tab| tab.layout.panes().contains(&id))
                else {
                    return reply.err(format!("no pane {pane}"));
                };
                // The desktop shows the pane's tab too.
                self.active = Active::Terminal(tab.id);
                self.focus_pane(id, window, cx);
                reply.ok("null".into());
            }
            UiEvent::ClosePane { pane, reply } => {
                reply.done(self.close_pane_on_screen(&pane, window, cx).map(|()| "null".into()))
            }
            UiEvent::OpenDiff(view) => self.open_diff_view(view, window, cx),
            UiEvent::OpenFile { file, line } => self.open_file_in_editor(file, line, cx),
            UiEvent::OpenLink(text) => self.open_link_text(&text, window, cx),
            UiEvent::UpdateDiff { plugin, id, update } => {
                if let Some((viewer, _)) = &self.plugin_ui.diff {
                    let current = &viewer.read(cx).view;
                    if current.plugin_id == plugin && current.id == id {
                        viewer.update(cx, |viewer, cx| viewer.update(update, cx));
                    }
                }
            }
            UiEvent::CloseDiff { plugin, id } => {
                if self.plugin_ui.diff.as_ref().is_some_and(|(viewer, _)| {
                    let view = &viewer.read(cx).view;
                    view.plugin_id == plugin && view.id == id
                }) {
                    self.close_diff_view(window, cx);
                }
            }
            UiEvent::Projects => self.projects_changed(cx),
            UiEvent::Git(ids) => {
                for installed in &self.plugins {
                    installed.host.publish_git(&ids);
                }
                return;
            }
            UiEvent::OpenPane { plan, reply } => {
                self.open_plugin_pane(plan, reply, window, cx);
            }
            UiEvent::OpenHistory { agents, reply } => self.open_history_resuming(agents, reply, window, cx),
            UiEvent::OpenView { plan, reply } => {
                // A tab's page in a project not on screen opens its own project's sidebar, which shows when you go there.
                if plan.place == "sidebar"
                    && let Some(parked) = plan.tab_project.as_ref().filter(|project| **project != self.config.project).and_then(|project| self.parked.get_mut(project))
                {
                    parked.right.get_or_insert_with(Default::default).add((plan.plugin.clone(), plan.id.clone()));
                    return reply.done(Ok("null".into()));
                }
                if plan.place == "sidebar" {
                    let shown = self.open_sidebar_from_plugin(&plan.plugin, &plan.id, cx);
                    reply.done(if shown {
                        Ok("null".into())
                    } else {
                        Err("The view has no native sidebar page".into())
                    });
                } else if plan.place == "tab" {
                    // In that project's tabs without showing it: the project on screen and its tab and keys stay.
                    if let Some(project) = plan.project.clone() {
                        if !self.projects.iter().any(|open| open.path == project) {
                            return reply.err("The tab's project is no longer open".into());
                        }
                        let shown = self.config.project.clone();
                        let opened = self.in_project(project, window, cx, |this, _, cx| {
                            this.new_plugin_tab(&plan.plugin, &plan.id, plan.args, cx).is_some()
                        });
                        self.show_project_again(shown, window, cx);
                        self.sync_plugins(cx);
                        return reply.done(if opened {
                            Ok("null".into())
                        } else {
                            Err("The view has no native tab page".into())
                        });
                    }
                    let before: Vec<usize> = self.plugin_tabs.iter().map(|tab| tab.id).collect();
                    let opened = if plan.new_tab {
                        self.new_plugin_tab(&plan.plugin, &plan.id, plan.args, cx)
                            .map(|id| {
                                self.show(Active::Plugin(id), window, cx);
                            })
                            .is_some()
                    } else {
                        self.open_plugin_view(&plan.plugin, &plan.id, plan.args, window, cx)
                    };
                    if let Some(frame) = plan.after.filter(|_| opened) {
                        self.place_after(&plan.plugin, frame, &before, cx);
                    }
                    reply.done(if opened {
                        Ok("null".into())
                    } else {
                        Err("The view has no native tab page".into())
                    });
                } else if plan.place == "window" {
                    reply.done(
                        self.open_plugin_window(&plan.plugin, &plan.id, cx)
                            .map(|_| "null".into()),
                    );
                } else if plan.place == "modal" {
                    reply.done(
                        self.open_plugin_modal(&plan.plugin, &plan.id, window, cx)
                            .map(|_| "null".into()),
                    );
                } else {
                    reply.err(format!("Native views don't open as {} yet", plan.place));
                }
            }
            UiEvent::CloseView { plugin, view } => {
                self.close_plugin_view(&plugin, &view, window, cx)
            }
            UiEvent::SendToAgent { pane, text, reply } => {
                self.send_to_agent(&pane, &text, reply, cx)
            }
            UiEvent::OpenSettings { plugin } => self.open_plugin_settings(&plugin, window, cx),
            UiEvent::Badge {
                plugin,
                pane,
                badge,
            } => {
                let Some(pane) = self.plugin_pane(&pane) else {
                    return;
                };
                if !set(&mut self.plugin_ui.badges, pane, plugin, badge) {
                    return;
                }
                // Pane cards are in the main column.
                shown_in = Some(&[Piece::Main]);
            }
            UiEvent::Status {
                plugin,
                pane,
                status,
            } => {
                let Some(pane) = self.plugin_pane(&pane) else {
                    return;
                };
                if !set(&mut self.plugin_ui.statuses, pane, plugin, status) {
                    return;
                }
                shown_in = Some(&[Piece::Main]);
            }
            UiEvent::SidebarLabel {
                plugin,
                page,
                label,
            } => {
                let key = (plugin, page);
                if self.plugin_ui.labels.get(&key) == label.as_ref() {
                    return;
                }
                match label {
                    Some(label) => self.plugin_ui.labels.insert(key, label),
                    None => self.plugin_ui.labels.remove(&key),
                };
                // The label is on the project bar's button, in the main column.
                shown_in = Some(&[Piece::Main]);
            }
            UiEvent::SystemNotification { notification, pane } => {
                if let Some(pane) = pane {
                    let tag = notification.tag.clone();
                    self.plugin_ui.notified_panes.insert(tag, pane);
                }
                cx.show_system_notification(notification)
            }
            UiEvent::Notice { title, body } => self.show_notice(title, body, cx),
            UiEvent::Notified(notification) => self.inbox_plugin(notification),
            UiEvent::Copy(text) => return cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)),
            UiEvent::ProjectSection {
                plugin,
                project,
                section,
                rows,
            } => {
                let key = super::project_sections::section_key(&plugin, &section);
                if !self.sections.set(std::path::Path::new(&project), &key, rows) {
                    return;
                }
                shown_in = Some(&[Piece::Sidebar]);
            }
            UiEvent::SectionSort {
                plugin,
                section,
                sort,
            } => {
                let key = super::project_sections::section_key(&plugin, &section);
                if !self.sections.set_sort(&key, &sort) {
                    return;
                }
                shown_in = Some(&[Piece::Sidebar]);
            }
        }
        match shown_in {
            Some(pieces) => self.notify_pieces(pieces, cx),
            None => cx.notify_workspace(),
        }
    }

    /// Draws only `pieces` again, or the whole window while they don't exist yet.
    pub(super) fn notify_pieces(&self, pieces: &[Piece], cx: &mut Context<Self>) {
        match &self.parts {
            Some(parts) => parts.notify(pieces, cx),
            None => cx.notify_workspace(),
        }
    }

    /// The shell `plan` asks for, in its project, which is then the one shown: what `open_plugin_pane` opens before it
    /// places it.
    pub(super) fn start_plugin_pane(
        &mut self,
        plan: &OpenPane,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        if !self
            .projects
            .iter()
            .any(|project| project.path == plan.project)
        {
            return Err("The pane's project is no longer open".into());
        }
        let roots: Vec<_> = self
            .projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
        let (_, cwd) = wings_plugins::panes::pane_cwd(&plan.cwd, &roots, None)?;
        self.switch_project(plan.project.clone(), window, cx);
        // The account `openPane` asked for, else the project's. The host checked the plugin may, and the name.
        if let Some(Err(error)) = plan.claude_account.as_ref().map(claude_accounts::pick_dir) {
            return Err(error);
        }
        let pane = self.open_pane_as(Some(cwd), None, plan.claude_account.clone(), cx);
        if let Err(error) = &self.panes[&pane].view {
            let error = error.to_string();
            self.panes.remove(&pane);
            return Err(error);
        }
        Ok(pane)
    }

    pub(super) fn open_plugin_pane(
        &mut self,
        plan: OpenPane,
        reply: crate::page::runtime::Reply,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if plan.placement == "background" {
            return self.open_plugin_pane_behind(plan, reply, window, cx);
        }
        let replace = plan.replace.clone();
        match self.place_plugin_pane(plan, window, cx) {
            Ok((pane, tab)) => {
                self.show(tab, window, cx);
                // The page's tab goes once its terminal is on screen, as a terminal goes when its session moves to a chat.
                let replaced = replace.and_then(|(plugin, frame)| {
                    self.plugin_tabs.iter().position(|tab| tab.plugin == plugin && tab.state.frame() == Some(frame))
                });
                if let Some(index) = replaced {
                    self.close_plugin_tab(index, window, cx);
                }
                self.sync_plugins(cx);
                reply.ok(serde_json::json!({ "paneId": pane.to_string() }).to_string());
            }
            Err(error) => reply.err(error),
        }
    }

    /// A tab of `plan`'s project with a new shell, the project on screen and its tab and keys left as they are, as the
    /// tabs Claude's `open_pane` opens.
    fn open_plugin_pane_behind(
        &mut self,
        plan: OpenPane,
        reply: crate::page::runtime::Reply,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let shown = self.config.project.clone();
        let project = plan.project.clone();
        if !self.projects.iter().any(|open| open.path == project) {
            return reply.err("The pane's project is no longer open".into());
        }
        let placed = self.in_project(project, window, cx, |this, window, cx| this.place_plugin_pane(plan, window, cx));
        self.show_project_again(shown, window, cx);
        self.sync_plugins(cx);
        match placed {
            Ok((pane, _)) => reply.ok(serde_json::json!({ "paneId": pane.to_string() }).to_string()),
            Err(error) => reply.err(error),
        }
    }

    /// Back to `shown` when working in another project had to show it, as one not entered yet has to be.
    fn show_project_again(&mut self, shown: PathBuf, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if self.config.project != shown {
            self.switch_project(shown, window, cx);
        }
    }

    /// Starts the shell `plan` asks for and puts it in a tab of its own, or in a split of the tab on screen, without
    /// showing it: the pane and its tab.
    pub(super) fn place_plugin_pane(
        &mut self,
        plan: OpenPane,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, Active), String> {
        let pane = self.start_plugin_pane(&plan, window, cx)?;
        let split =
            (!matches!(plan.placement.as_str(), "tab" | "background"))
                .then_some(self.active)
                .and_then(|active| match active {
                    Active::Terminal(id) => Some(id),
                    _ => None,
                });
        let active = if let Some(id) = split {
            let tab = self.terminals.iter_mut().find(|tab| tab.id == id).unwrap();
            let dir = if plan.placement == "right" {
                crate::layout::Dir::Row
            } else {
                crate::layout::Dir::Column
            };
            tab.layout.split(tab.focused, dir, pane);
            tab.focused = pane;
            tab.zoomed = None;
            Active::Terminal(id)
        } else {
            let id = self.next_tab;
            self.next_tab += 1;
            self.terminals.push(super::TerminalTab {
                id,
                layout: crate::layout::Layout::Pane(pane),
                focused: pane,
                zoomed: None,
                focus: cx.focus_handle().tab_stop(true),
            });
            Active::Terminal(id)
        };
        if let Some(input) = plan.input {
            self.terminal(pane)
                .unwrap()
                .update(cx, |view, cx| view.write_input(&input, cx));
        }
        Ok((pane, active))
    }

    /// Pastes a plugin's message into the prompt of the agent in `pane`, as the desktop's `sendToAgent` does: only an agent
    /// gets it, since a shell would run the text, and not one waiting on an answer, which the text would be taken for.
    /// Nor one where you typed into the prompt and haven't sent it, which the text would be sent with. Enter is pressed
    /// a moment after the paste only while "Let agents message each other" is on and the checks still pass then; the
    /// answer comes after that and says whether it was: `{ submitted }`.
    pub(super) fn send_to_agent(
        &mut self,
        pane: &str,
        text: &str,
        reply: crate::page::runtime::Reply,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.plugin_pane(pane) else {
            return reply.err(format!("unknown pane {pane}"));
        };
        let Some(agent) = self.agents.get(&id) else {
            return reply.err(format!(
                "Pane {pane} isn't running an agent, so Wings won't type into it: a shell would run the text."
            ));
        };
        if agent.state == crate::agents::AgentState::Blocked {
            return reply.err(format!(
                "The agent in pane {pane} is waiting for {}. Try again once it's past that.",
                agent.waiting_for.as_deref().unwrap_or("an answer")
            ));
        }
        let Some(view) = self.terminal(id) else {
            return reply.err(format!("unknown pane {pane}"));
        };
        if view.read(cx).has_typed() {
            return reply.err(format!(
                "Someone typed into the prompt of the agent in pane {pane} and hasn't sent it. Wings won't type over it."
            ));
        }
        view.update(cx, |view, cx| view.paste_text(text, cx));
        if !self.prefs.agents_message {
            return reply.ok(serde_json::json!({ "submitted": false }).to_string());
        }
        // Enter in a write of its own, a moment later, as the desktop's `sendToPane`: one in the paste would be text. The
        // checks run again first, since the agent may have come to a prompt or you may have typed meanwhile; the answer
        // waits for them, so `submitted` says whether Enter was pressed.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SUBMIT_AFTER).await;
            let submitted = this
                .update(cx, |this, cx| {
                    let agent = this.agents.get(&id)?;
                    let view = this.terminal(id)?;
                    if agent.state == crate::agents::AgentState::Blocked
                        || view.read(cx).has_typed()
                    {
                        return None;
                    }
                    view.update(cx, |view, cx| view.write_input("\r", cx));
                    Some(())
                })
                .ok()
                .flatten()
                .is_some();
            reply.ok(serde_json::json!({ "submitted": submitted }).to_string());
        })
        .detach();
    }

    /// Clicks on the system's notifications come to `notification_clicked`. Registering wakes the notification centre,
    /// which asks nothing of the user until a notification is posted.
    pub(super) fn listen_for_notification_clicks(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (workspace, handle) = (cx.entity().downgrade(), window.window_handle());
        cx.on_system_notification_response(move |response, cx| {
            let _ = handle.update(cx, |_, window, cx| {
                workspace.update(cx, |this, cx| {
                    this.notification_clicked(&response.tag, window, cx)
                })
            });
        });
    }

    /// Goes to the pane the notification came from, while it's open. macOS has brought Wings forward already.
    pub(super) fn notification_clicked(
        &mut self,
        tag: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.plugin_ui.notified_panes.remove(tag) else {
            return;
        };
        let Some(id) = self.plugin_pane(&pane) else {
            return;
        };
        let project = self
            .pane_projects()
            .into_iter()
            .find(|(pane, _)| *pane == id)
            .map(|(_, project)| project.to_path_buf());
        if let Some(project) = project {
            self.go_to_pane(project, id, window, cx);
        }
    }

    /// The pane a plugin named, if it is still open: one that closed since the call has nothing to draw on.
    pub(super) fn plugin_pane(&self, pane: &str) -> Option<usize> {
        let pane = pane.parse().ok()?;
        self.panes.contains_key(&pane).then_some(pane)
    }

    /// What the terminal pane `pane` printed lately, oldest first, for Wings' `read_pane` tool and a plugin's
    /// `wings.readPane`. A full-screen program's output is cursor moves, so the screen it leaves is what reads.
    pub(super) fn pane_output(&self, pane: &str, cx: &gpui::App) -> Option<Vec<u8>> {
        let terminal = self.terminal(self.plugin_pane(pane)?)?.read(cx);
        Some(terminal.screen_text().map_or_else(|| terminal.recent_output(), String::into_bytes))
    }

    pub(super) fn show_notice(&mut self, title: String, body: String, cx: &mut Context<Self>) {
        let ui = &mut self.plugin_ui;
        let id = ui.next_notice;
        ui.next_notice += 1;
        ui.notices.push(Notice {
            id,
            title: title.into(),
            body: body.into(),
        });
        ui.notice_timers.push(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_FOR).await;
            let _ = this.update(cx, |this, cx| this.dismiss_notice(id, cx));
        }));
    }

    pub(super) fn dismiss_notice(&mut self, id: usize, cx: &mut Context<Self>) {
        self.plugin_ui.notices.retain(|notice| notice.id != id);
        cx.notify_workspace();
    }

    /// Tells the plugins what changed in the panes and projects since they were last told, which is nothing most of
    /// the time, and drops the badges and strips of panes that closed. Called as the window draws, so every way a pane
    /// opens, closes, moves or gets an agent is covered without each saying so. A quiet frame only hashes what the
    /// plugins would be told, which allocates once.
    pub(super) fn sync_plugins(&mut self, cx: &mut Context<Self>) {
        self.sync_plugin_visibility(cx);
        if self.plugins.is_empty() {
            return;
        }
        let (signature, entries) = {
            let panes = self.pane_projects();
            let signature = self.plugin_signature(&panes);
            if signature == self.plugin_ui.signature {
                return;
            }
            (signature, self.plugin_panes_of(&panes))
        };
        self.plugin_ui.signature = signature;
        let live: HashSet<usize> = self.panes.keys().copied().collect();
        self.plugin_ui.badges.retain(|pane, _| live.contains(pane));
        self.plugin_ui
            .statuses
            .retain(|pane, _| live.contains(pane));
        if self
            .badge_card
            .as_ref()
            .is_some_and(|card| !live.contains(&card.pane))
        {
            self.badge_card = None;
        }
        {
            let mut known = self.plugin_env.known.lock().unwrap();
            known.current = Some(self.config.project.clone());
            known.shells = self.pane_shells(cx);
        }
        for installed in &self.plugins {
            installed.host.publish_panes(entries.clone());
            installed
                .host
                .publish_projects(published_projects(&self.projects, &self.side));
            installed
                .host
                .publish_project_in(Some(self.config.project.as_path()), self.page_tab_folder());
            installed.host.publish_focus(self.window_active);
        }
        let paths = self.plugin_env.projects();
        if let Some(watcher) = &self.plugin_ui.watcher {
            if self.plugin_ui.watched_projects != paths {
                watcher.resync();
            }
        } else {
            let env = self.plugin_env.clone();
            let events = env.events.clone();
            self.plugin_ui.watcher = Some(wings_gitwatch::Watcher::start(
                move || {
                    env.projects()
                        .into_iter()
                        .map(|path| (path.to_string_lossy().into_owned(), path))
                        .collect()
                },
                move |ids| {
                    let _ = events.try_send(UiEvent::Git(ids));
                },
            ));
        }
        self.plugin_ui.watched_projects = paths;
    }

    /// Every pane of every project, shown or not, with its project's folder, by pane id. Not one still waiting for its
    /// project to be shown to start (`start_waiting`), which has no terminal yet.
    pub(super) fn pane_projects(&self) -> Vec<(usize, &std::path::Path)> {
        let shown = self.terminals.iter().map(|tab| (&self.config.project, tab));
        let parked = self
            .parked
            .iter()
            .flat_map(|(path, parked)| parked.terminals.iter().map(move |tab| (path, tab)));
        let mut panes: Vec<_> = shown
            .chain(parked)
            .flat_map(|(path, tab)| {
                tab.layout
                    .panes()
                    .into_iter()
                    .map(move |pane| (pane, path.as_path()))
            })
            .chain(self.page_terminal_projects())
            .filter(|(pane, _)| self.panes.contains_key(pane))
            .collect();
        panes.sort_unstable_by_key(|(pane, _)| *pane);
        panes
    }

    /// The pane that has focus: the focused one of the tab on screen.
    fn focused_pane_of_tab(&self) -> Option<usize> {
        let Active::Terminal(id) = self.active else {
            return None;
        };
        self.terminals
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.focused)
    }

    /// Where the tab on screen works when it is a plugin page opened in a folder (`cwd` in its args), as a Claude chat
    /// tab: no pane is focused then, so plugins that follow the focused pane follow this.
    pub(super) fn page_tab_folder(&self) -> Option<&str> {
        let Active::Plugin(id) = self.active else {
            return None;
        };
        let tab = self.plugin_tabs.iter().find(|tab| tab.id == id)?;
        tab.args.as_ref()?.get("cwd")?.as_str().filter(|cwd| !cwd.is_empty())
    }

    /// What decides what the plugins are told: the projects, the folder of a page tab on screen, and each pane's place,
    /// folder, program, agent and focus.
    fn plugin_signature(&self, panes: &[(usize, &std::path::Path)]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.config.project.hash(&mut hasher);
        self.page_tab_folder().hash(&mut hasher);
        for project in &self.projects {
            (&project.path, &project.name).hash(&mut hasher);
            self.side.sections_off(&project.path).hash(&mut hasher);
        }
        let focused = self.focused_pane_of_tab();
        for (id, path) in panes {
            let info = self.pane_info.get(id);
            let agent = self.agents.get(id);
            (id, path, focused == Some(*id)).hash(&mut hasher);
            if let Some(info) = info {
                (&info.cwd, &info.command).hash(&mut hasher);
            }
            if let Some(agent) = agent {
                (
                    &agent.session_id,
                    &agent.name,
                    agent.started_at,
                    std::mem::discriminant(&agent.kind),
                    std::mem::discriminant(&agent.state),
                )
                    .hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    /// Every pane, as `wings.onPanes` lists them: the desktop's `WingsPane`.
    fn plugin_panes_of(&self, panes: &[(usize, &std::path::Path)]) -> Vec<PaneEntry> {
        let focused = self.focused_pane_of_tab();
        panes
            .iter()
            .map(|&(id, path)| {
                let info = self.pane_info.get(&id);
                let agent = self.agents.get(&id);
                let name = self
                    .projects
                    .iter()
                    .find(|project| project.path == path)
                    .map(|project| project.name.clone())
                    .unwrap_or_else(|| {
                        path.file_name()
                            .unwrap_or(path.as_os_str())
                            .to_string_lossy()
                            .into_owned()
                    });
                PaneEntry {
                    pane_id: id.to_string(),
                    cwd: info
                        .and_then(|info| info.cwd.clone())
                        .unwrap_or_else(|| path.to_string_lossy().into_owned()),
                    command: info.map(|info| info.command.clone()).unwrap_or_default(),
                    project: name,
                    project_id: path.to_string_lossy().into_owned(),
                    session: agent.and_then(|agent| {
                        Some(SessionEntry {
                            session_id: agent.session_id.clone()?,
                            kind: serde_json::to_value(agent.kind)
                                .ok()
                                .and_then(|kind| kind.as_str().map(str::to_owned))?,
                            name: agent.name.clone(),
                            state: serde_json::to_value(agent.state)
                                .ok()
                                .and_then(|state| state.as_str().map(str::to_owned))?,
                            started_at: agent.started_at,
                            launcher: agent.launcher.clone(),
                        })
                    }),
                    focused: focused == Some(id),
                }
            })
            .collect()
    }

    /// Each pane's shell, for `wings.ports`.
    pub(super) fn pane_shells(&self, cx: &gpui::App) -> Vec<(String, Option<u32>)> {
        self.panes
            .iter()
            .map(|(id, pane)| {
                let pid = pane
                    .view
                    .as_ref()
                    .ok()
                    .and_then(|view| view.read(cx).process_id());
                (id.to_string(), pid)
            })
            .collect()
    }

    /// The Claude sessions running in panes, `(pane id, session id)`, whose transcripts plugins may follow.
    pub(super) fn claude_sessions(&self) -> Vec<(String, String)> {
        self.agents
            .values()
            .filter(|agent| agent.kind == AgentKind::Claude)
            .filter_map(|agent| Some((agent.pane_id.clone(), agent.session_id.clone()?)))
            .collect()
    }

    /// Tells the plugins allowed to read agent activity which OpenCode sessions did something, as `wings.onAgentActivity`'s events.
    pub(super) fn publish_agent_activity(&self, sessions: &[String]) {
        for session in sessions {
            for installed in &self.plugins {
                installed.host.publish_agent_activity(session);
            }
        }
    }

    /// Gives each plugin allowed the entry's type the entries Claude wrote, as `wings.onTranscript`'s events.
    pub(super) fn publish_transcripts(&self, lines: Vec<Line>) {
        for line in lines {
            let Some(event) = transcript_event(&self.transcript_plugins, line) else {
                continue;
            };
            let json = serde_json::json!({
                "sessionId": event.session_id,
                "paneId": event.pane_id,
                "entry": event.entry,
            })
            .to_string();
            for installed in &self.plugins {
                if event.plugins.contains(&installed.id) {
                    installed.host.publish_transcript(&json);
                }
            }
        }
    }
}

/// Sets, or with `None` takes away, what `plugin` shows on `pane`.
/// Whether it changed what the pane shows.
fn set<T: PartialEq>(
    all: &mut HashMap<usize, BTreeMap<String, T>>,
    pane: usize,
    plugin: String,
    value: Option<T>,
) -> bool {
    match value {
        Some(value) => {
            let shown = all.entry(pane).or_default();
            if shown.get(&plugin) == Some(&value) {
                return false;
            }
            shown.insert(plugin, value);
            true
        }
        None => {
            let Some(shown) = all.get_mut(&pane) else {
                return false;
            };
            let removed = shown.remove(&plugin).is_some();
            if shown.is_empty() {
                all.remove(&pane);
            }
            removed
        }
    }
}

/// The project list as plugins know it. A worktree follows its project in the list, which is its `parentId`.
fn published_projects(
    projects: &[crate::projects::ProjectEntry],
    side: &super::sidebar::State,
) -> Vec<Project> {
    let mut top = None;
    projects
        .iter()
        .map(|project| {
            let id = project.path.to_string_lossy().into_owned();
            let parent_id = if project.child {
                top.clone()
            } else {
                top = Some(id.clone());
                None
            };
            Project {
                id: id.clone(),
                name: project.name.clone(),
                hidden_sections: side.sections_off(&project.path),
                path: id,
                parent_id,
            }
        })
        .collect()
}
