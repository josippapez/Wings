//! What the window gives its plugins, and what it takes from them: the panes plugins are told of, the badges, status
//! strips, notifications and URLs they ask for, the transcript entries Claude writes and the clicks on a badge's buttons.
//!
//! Plugins run on threads of their own, so they reach the window through `AppEnv`, which hands each request to the
//! window as an event; the window applies them on its own thread. The checks are `wings-plugins`' and the host's
//! (`page::plugin::calls`): by the time a request gets here the plugin was allowed to make it.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_channel::Sender;
use gpui::{Context, SharedString, Task};
use wings_plugins::transcript::{Line, transcript_event};
use wings_spaces::{
    claude_accounts::{self, Accounts},
    spaces::WorktreeSetup,
};

use crate::{
    agents::AgentKind,
    page::plugin::{
        Env, OpenPane, OpenView, PaneEntry, PanePort, Project, Running, SessionEntry, ToolCall,
        ui::{Badge, PaneStatus},
    },
    ports,
    projects::{Projects, with_setup},
};

use super::{Active, Workspace};

/// How long a badge's button waits for the plugin before it says the plugin didn't respond: just over the 5 minutes
/// `exec` may take, so an action waiting on a long exec can finish (the desktop's `ask`).
pub(super) const ACTION_TIMEOUT: Duration = Duration::from_secs(310);
/// How long a notice the system couldn't show stays in the window.
const NOTICE_FOR: Duration = Duration::from_secs(8);

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

/// What a plugin asked of the window, for the window to do.
pub(super) enum UiEvent {
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
    CloseView {
        plugin: String,
        view: String,
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
    /// A notification the system couldn't show, for the window to show itself.
    Notice {
        title: String,
        body: String,
    },
}

/// What the system does for a plugin: open a URL, show a notification. Faked in tests, which must not open anything.
pub(super) trait System: Send + Sync + 'static {
    fn open_url(&self, url: &str) -> Result<(), String>;
    fn notify(&self, title: &str, body: &str) -> Result<(), String>;
}

/// The real system. A bare binary has no bundle to post notifications from (`UNUserNotificationCenter` needs one), so
/// a notification is `osascript`'s `display notification`, which any process may use. The title and body go in as the
/// script's arguments, never in its text, so nothing a plugin writes is read as script.
struct Os;

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

    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
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
    running: Running,
    claude_dir: PathBuf,
    system: Mutex<Arc<dyn System>>,
}

impl AppEnv {
    pub(super) fn new(
        events: Sender<UiEvent>,
        claude_dir: PathBuf,
        projects: Arc<Projects>,
    ) -> Self {
        Self {
            events,
            known: Mutex::default(),
            projects,
            running: Running::default(),
            claude_dir,
            system: Mutex::new(Arc::new(Os)),
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

    fn send(&self, event: UiEvent) {
        // Closed when the window is, and a plugin that outlives it has no one to show anything to.
        let _ = self.events.try_send(event);
    }
}

impl Env for AppEnv {
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

    fn claude_accounts(&self) -> Result<Accounts, String> {
        Ok(self.projects.claude_accounts())
    }

    fn set_claude_account(&self, project: &str, name: Option<&str>) -> Result<(), String> {
        self.projects.set_claude_account(project, name, |name| {
            claude_accounts::create_dir(name)
                .map(drop)
                .map_err(|error| error.to_string())
        })
    }

    fn ports(&self) -> Result<Vec<PanePort>, String> {
        let shells = self.known.lock().unwrap().shells.clone();
        ports::scan(&shells)
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.system().open_url(url)
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        // Where the system has no way to show it, the window does, so a notification is never lost.
        if self.system().notify(title, body).is_err() {
            self.send(UiEvent::Notice {
                title: title.to_owned(),
                body: body.to_owned(),
            });
        }
        Ok(())
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
    pub(super) windows:
        HashMap<(String, String), gpui::WindowHandle<super::plugin_windows::PluginViewWindow>>,
    pub(super) windows_release: Option<gpui::Subscription>,
    pub(super) windows_closed: Option<gpui::Subscription>,
    watcher: Option<wings_gitwatch::Watcher>,
    watched_projects: Vec<PathBuf>,
    /// A hash of what the plugins were last told of the panes and projects, to tell them only of a change.
    signature: u64,
    /// By pane, then plugin id, which is also the order they are drawn in.
    pub(super) badges: HashMap<usize, BTreeMap<String, Badge>>,
    pub(super) statuses: HashMap<usize, BTreeMap<String, PaneStatus>>,
    /// The notifications the system couldn't show, newest last.
    pub(super) notices: Vec<Notice>,
    next_notice: usize,
    notice_timers: Vec<Task<()>>,
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
            page.read(cx).set_visible(self.right.as_ref() == Some(key));
        }
    }
    /// What the window is doing for plugins, on its own thread.
    pub(super) fn apply_plugin_event(
        &mut self,
        event: UiEvent,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            UiEvent::OpenDiff(view) => self.open_diff_view(view, window, cx),
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
            UiEvent::Projects => self.projects = self.spaces.entries(),
            UiEvent::Git(ids) => {
                for installed in &self.plugins {
                    installed.host.publish_git(&ids);
                }
            }
            UiEvent::OpenPane { plan, reply } => {
                self.open_plugin_pane(plan, reply, window, cx);
            }
            UiEvent::OpenView { plan, reply } => {
                if plan.args.is_some() {
                    reply.err("Native view arguments aren't available yet".into());
                } else if plan.place == "sidebar" {
                    self.show_sidebar(&plan.plugin, &plan.id, cx);
                    let shown = self
                        .right
                        .as_ref()
                        .is_some_and(|(plugin, id)| plugin == &plan.plugin && id == &plan.id);
                    reply.done(if shown {
                        Ok("null".into())
                    } else {
                        Err("The view has no native sidebar page".into())
                    });
                } else if plan.place == "tab" {
                    let opened = if plan.new_tab {
                        self.new_plugin_tab(&plan.plugin, &plan.id, cx)
                            .map(|id| {
                                self.show(Active::Plugin(id), window, cx);
                            })
                            .is_some()
                    } else {
                        self.open_plugin_view(&plan.plugin, &plan.id, window, cx)
                    };
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
                } else {
                    reply.err(format!("Native views don't open as {} yet", plan.place));
                }
            }
            UiEvent::CloseView { plugin, view } => {
                self.close_plugin_view(&plugin, &view, window, cx)
            }
            UiEvent::Badge {
                plugin,
                pane,
                badge,
            } => {
                let Some(pane) = self.plugin_pane(&pane) else {
                    return;
                };
                set(&mut self.plugin_ui.badges, pane, plugin, badge);
            }
            UiEvent::Status {
                plugin,
                pane,
                status,
            } => {
                let Some(pane) = self.plugin_pane(&pane) else {
                    return;
                };
                set(&mut self.plugin_ui.statuses, pane, plugin, status);
            }
            UiEvent::Notice { title, body } => self.show_notice(title, body, cx),
        }
        cx.notify();
    }

    fn open_plugin_pane(
        &mut self,
        plan: OpenPane,
        reply: crate::page::runtime::Reply,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::EntityInputHandler;
        if !self
            .projects
            .iter()
            .any(|project| project.path == plan.project)
        {
            return reply.err("The pane's project is no longer open".into());
        }
        let roots: Vec<_> = self
            .projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
        let cwd = match wings_plugins::panes::pane_cwd(&plan.cwd, &roots, None) {
            Ok((_, cwd)) => cwd,
            Err(error) => return reply.err(error),
        };
        self.switch_project(plan.project, window, cx);
        let pane = self.open_pane(Some(cwd), None, cx);
        if let Err(error) = &self.panes[&pane].view {
            let error = error.to_string();
            self.panes.remove(&pane);
            return reply.err(error);
        }
        let split =
            (plan.placement != "tab")
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
            self.terminal(pane).unwrap().update(cx, |view, cx| {
                view.replace_text_in_range(None, &input, window, cx)
            });
        }
        self.show(active, window, cx);
        self.sync_plugins(cx);
        reply.ok(serde_json::json!({ "paneId": pane.to_string() }).to_string());
    }

    /// The pane a plugin named, if it is still open: one that closed since the call has nothing to draw on.
    fn plugin_pane(&self, pane: &str) -> Option<usize> {
        let pane = pane.parse().ok()?;
        self.panes.contains_key(&pane).then_some(pane)
    }

    fn show_notice(&mut self, title: String, body: String, cx: &mut Context<Self>) {
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
        cx.notify();
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
            installed.host.publish_projects(
                self.projects
                    .iter()
                    .map(|project| Project {
                        id: project.path.to_string_lossy().into_owned(),
                        name: project.name.clone(),
                        path: project.path.to_string_lossy().into_owned(),
                    })
                    .collect(),
            );
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

    /// Every pane of every project, shown or not, with its project's folder, by pane id.
    fn pane_projects(&self) -> Vec<(usize, &std::path::Path)> {
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

    /// What decides what the plugins are told: the projects, and each pane's place, folder, program, agent and focus.
    fn plugin_signature(&self, panes: &[(usize, &std::path::Path)]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.config.project.hash(&mut hasher);
        for project in &self.projects {
            (&project.path, &project.name).hash(&mut hasher);
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
                        })
                    }),
                    focused: focused == Some(id),
                }
            })
            .collect()
    }

    /// Each pane's shell, for `wings.ports`.
    fn pane_shells(&self, cx: &gpui::App) -> Vec<(String, Option<u32>)> {
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
fn set<T>(
    all: &mut HashMap<usize, BTreeMap<String, T>>,
    pane: usize,
    plugin: String,
    value: Option<T>,
) {
    match value {
        Some(value) => {
            all.entry(pane).or_default().insert(plugin, value);
        }
        None => {
            if let Some(shown) = all.get_mut(&pane) {
                shown.remove(&plugin);
                if shown.is_empty() {
                    all.remove(&pane);
                }
            }
        }
    }
}
