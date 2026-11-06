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

use crate::{
    agents::AgentKind,
    page::plugin::{
        Env, PaneEntry, PanePort, SessionEntry,
        ui::{Badge, PaneStatus},
    },
    ports,
};

use super::{Active, Workspace};

/// How long a badge's button waits for the plugin before it says the plugin didn't respond: just over the 5 minutes
/// `exec` may take, so an action waiting on a long exec can finish (the desktop's `ask`).
pub(super) const ACTION_TIMEOUT: Duration = Duration::from_secs(310);
/// How long a notice the system couldn't show stays in the window.
const NOTICE_FOR: Duration = Duration::from_secs(8);

/// What a plugin asked of the window, for the window to do.
pub(super) enum UiEvent {
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
    Notice { title: String, body: String },
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

/// What the window tells the plugins' hosts it knows: the projects, and where each pane's shell is.
#[derive(Default)]
struct Known {
    projects: Vec<PathBuf>,
    shells: Vec<(String, Option<u32>)>,
}

/// The window, as the plugins' hosts see it.
pub(super) struct AppEnv {
    events: Sender<UiEvent>,
    known: Mutex<Known>,
    claude_dir: PathBuf,
    system: Mutex<Arc<dyn System>>,
}

impl AppEnv {
    pub(super) fn new(events: Sender<UiEvent>, claude_dir: PathBuf) -> Self {
        Self {
            events,
            known: Mutex::default(),
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
    fn projects(&self) -> Vec<PathBuf> {
        self.known.lock().unwrap().projects.clone()
    }

    fn claude_dir(&self) -> PathBuf {
        self.claude_dir.clone()
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

impl Workspace {
    /// What the window is doing for plugins, on its own thread.
    pub(super) fn apply_plugin_event(&mut self, event: UiEvent, cx: &mut Context<Self>) {
        match event {
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
        let projects: Vec<PathBuf> = self
            .projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
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
            known.projects = projects;
            known.shells = self.pane_shells(cx);
        }
        for installed in &self.plugins {
            installed.host.publish_panes(entries.clone());
        }
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
