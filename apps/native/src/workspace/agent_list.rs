//! The sidebar's Agents section, like the desktop's: every agent in every project, with what it's
//! doing, and a click goes to its pane.

use crate::workspace::NotifyWorkspace;
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use gpui::{
    AnyElement, Context, Div, FontWeight, MouseButton, MouseDownEvent, Role, SharedString, Window,
    div, prelude::*, px, relative, rgb, rgba,
};

use crate::{
    agents::{self, Agent, AgentKind, AgentState},
    page::plugin::TabAgent,
    icons::{Icon, icon},
    saved::SessionKind,
    theme,
};

use super::{Active, HAIRLINE, Workspace, context_menu::MenuTarget, project::SIDEBAR_ACCENT, sidebar::AgentKey};

/// The desktop's `bg-blocked/10` behind an agent that needs you, and `/15` on hover.
const BLOCKED_ROW: u32 = theme::BLOCKED << 8 | 0x1a;
const BLOCKED_ROW_HOVER: u32 = theme::BLOCKED << 8 | 0x26;

impl Workspace {
    /// The agents in the order they were made (terminals', then tabs', then those coming back), or nothing
    /// while there are none.
    pub(super) fn agent_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.pending_agents(cx.background_executor().now());
        let tabs = self.tab_agents();
        if self.agents.is_empty() && self.loose_agents.is_empty() && pending.is_empty() && tabs.is_empty() {
            return None;
        }
        let count = self.agents.len() + self.loose_agents.len() + pending.len() + tabs.len();
        let focused = self.focused_pane();
        // Rows keep their place: a click, a state change or a project switch never moves one. A blocked agent shows
        // in its row's colour instead.
        let mut rows: Vec<AnyElement> = in_place(&self.agents)
            .into_iter()
            .map(|(pane, agent)| self.agent_row(At::Pane(pane), agent, focused == Some(pane), cx))
            .collect();
        let shown_loose = self.loose_page();
        rows.extend(
            in_place(&self.loose_agents)
                .into_iter()
                .map(|(key, agent)| self.agent_row(At::Loose(key), agent, shown_loose == Some(key), cx)),
        );
        rows.extend(tabs.into_iter().map(|(project, tab, agent)| {
            let state = tab_state(&agent.state);
            self.tab_agent_row(project, tab, agent, state, cx)
        }));
        rows.extend(
            pending
                .into_iter()
                .map(|row| self.pending_row(row, focused, cx)),
        );
        Some(
            div()
                .id("agents")
                .role(Role::List)
                .aria_label("Agents")
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(1.0))
                // The desktop's `mx-2 border-t pt-2`: the line stops 8px short of each side.
                .mx(px(8.0))
                .pt(px(8.0))
                .border_t_1()
                .border_color(rgba(HAIRLINE))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(8.0))
                        .pb(px(4.0))
                        .line_height(px(18.0))
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child("Agents")
                        .child(count.to_string()),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    fn agent_row(
        &self,
        at: At,
        agent: &Agent,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = agent
            .name
            .clone()
            .unwrap_or_else(|| agents::label(agent.kind).into());
        // A one-off terminal's agent is in no project: its row names the folder the terminal is in, as its sidebar row does.
        let project = match at {
            At::Pane(_) => self
                .projects
                .iter()
                .find(|p| p.path.as_os_str() == agent.space_id.as_str())
                .map_or_else(|| agent.space_id.clone(), |p| p.name.clone()),
            At::Loose(key) => self.loose_terminal(key).map_or_else(|| "~".to_owned(), |quick| quick.title(cx)),
        };
        let blocked = agent.state == AgentState::Blocked;
        let key = match at { At::Pane(pane) => AgentKey::Pane(pane), At::Loose(key) => AgentKey::Loose(key) };
        let unread = self.agent_unread(key, agent.state);
        let model = self.known_agent_model(agent.session_id.as_deref()).or_else(|| model_flag(&agent.args));
        let path = PathBuf::from(&agent.space_id);
        let account = match at {
            At::Pane(pane) => self.agent_accounts.get(&pane).cloned(),
            At::Loose(_) => agent
                .claude_dir
                .as_deref()
                .zip(self.config.agents.as_deref())
                .and_then(|(dir, default)| wings_spaces::claude_accounts::label(dir, default))
                .map(SharedString::new),
        };
        // A one-off terminal's row counts down from the top of the ids, clear of the panes' numbers.
        let (id, slot, summary_spot) = match at {
            At::Pane(pane) => (pane, format!("agent-{pane}"), format!("agent-summary:{pane}")),
            At::Loose(key) => (usize::MAX - key, format!("loose-agent-{key}"), format!("loose-agent-summary:{key}")),
        };
        let mut label = match &account {
            Some(account) => format!("{name}, {account} account, {project}, {}", status(agent)),
            None => format!("{name}, {project}, {}", status(agent)),
        };
        if let Some(summary) = &agent.summary {
            label.push_str(&format!(". {summary}"));
        }
        div()
            .id(("agent", id))
            .role(Role::Button)
            .aria_label(label)
            .relative()
            .children(crate::ui::spot(move || format!("agent-row:{id}")))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(8.0))
            .py(px(6.0))
            .border_1()
            .border_color(rgba(0))
            .rounded(px(theme::RADIUS))
            .cursor_pointer()
            .when(focused, |row| row.bg(rgba(SIDEBAR_ACCENT)))
            .when(!focused && !blocked, |row| {
                row.hover(|style| style.bg(rgba(theme::HOVER)))
            })
            .when(blocked, |row| {
                row.bg(rgba(BLOCKED_ROW))
                    .hover(|style| style.bg(rgba(BLOCKED_ROW_HOVER)))
            })
            .on_click(cx.listener(move |this, _, window, cx| match at {
                At::Pane(pane) => this.go_to_pane(path.clone(), pane, window, cx),
                At::Loose(key) => this.place_quick(key, super::quick::Place::Full, false, window, cx),
            }))
            .when_some(at.pane(), |row, pane| {
                row.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.open_menu(MenuTarget::Agent(pane), event.position, cx)
                    }),
                )
            })
            .child(
                div()
                    .size(px(16.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child({
                        let (glyph, color) = glyph(agent.kind);
                        icon(glyph, 14.0, color)
                    }),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                crate::ui::behaviors::cut_text("name", name)
                                    .min_w_0().flex_1()
                                    .text_size(px(13.0))
                                    .font_weight(if unread { FontWeight::BOLD } else { FontWeight::MEDIUM })
                                    .when(unread, |name| name.children(crate::ui::spot(move || format!("agent-unread:{id}")))),
                             )
                            .children(account.map(account_badge))
                            .child(agent_metadata(format!("agent-age-{id}"), model, agent.state_since.or((agent.started_at > 0).then_some(agent.started_at)))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .min_w_0()
                            .text_size(px(11.0))
                            .line_height(px(16.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(div().min_w_0().truncate().child(format!("{project},")))
                            .child(crate::ui::status_dot_slot(agent.state, slot))
                            .child(div().min_w_0().truncate().child(status(agent))),
                    )
                    // The desktop's `text-muted-foreground/70`: what the agent just did, from Settings' model.
                    .children(agent.summary.clone().map(|summary| {
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.0))
                            .line_height(px(16.0))
                            .text_color(rgba(theme::MUTED_FOREGROUND << 8 | 0xb3))
                            .children(crate::ui::spot(|| summary_spot))
                            .child(summary)
                    })),
            )
            .into_any_element()
    }

    /// The tabs whose page runs an agent and says so, like Claude chat's, in every project: its project, tab and agent.
    pub(super) fn tab_agents(&self) -> Vec<(PathBuf, usize, TabAgent)> {
        let shown = self.plugin_tabs.iter().map(|tab| (&self.config.project, tab));
        let parked = self
            .parked
            .iter()
            .flat_map(|(path, parked)| parked.plugin_tabs.iter().map(move |tab| (path, tab)));
        let mut tabs: Vec<(PathBuf, usize, TabAgent)> = shown
            .chain(parked)
            .filter_map(|(project, tab)| Some((project.clone(), tab.id, tab.state.agent()?)))
            .collect();
        // Shown project first, then the parked ones in a map's order, would move a row to the top when its project
        // is switched to. Tab ids are never reused, so they keep each row where it was.
        tabs.sort_by_key(|(_, tab, _)| *tab);
        tabs
    }

    /// Like `agent_row`, for a tab's agent: its name, project, state and what it does, and a click shows its tab.
    fn tab_agent_row(
        &self,
        path: PathBuf,
        tab: usize,
        agent: TabAgent,
        state: AgentState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let project = self
            .projects
            .iter()
            .find(|p| p.path == path)
            .map_or_else(|| path.to_string_lossy().into_owned(), |p| p.name.clone());
        let shown = self.active == Active::Plugin(tab) && self.config.project == path;
        let blocked = state == AgentState::Blocked;
        let unread = self.agent_unread(AgentKey::Tab(tab), state);
        let model = self.known_agent_model(agent.session_id.as_deref());
        let status = agents::state_label(state).to_lowercase();
        let label = match &agent.summary {
            Some(summary) => format!("{}, {project}, {status}. {summary}", agent.name),
            None => format!("{}, {project}, {status}", agent.name),
        };
        let (glyph, color) = glyph(AgentKind::Claude);
        div()
            .id(("tab-agent", tab))
            .role(Role::Button)
            .aria_label(label)
            .relative()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(8.0))
            .py(px(6.0))
            .border_1()
            .border_color(rgba(0))
            .rounded(px(theme::RADIUS))
            .cursor_pointer()
            .when(shown, |row| row.bg(rgba(SIDEBAR_ACCENT)))
            .when(!shown && !blocked, |row| row.hover(|style| style.bg(rgba(theme::HOVER))))
            .when(blocked, |row| row.bg(rgba(BLOCKED_ROW)).hover(|style| style.bg(rgba(BLOCKED_ROW_HOVER))))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.switch_project(path.clone(), window, cx);
                this.reveal_project(&path);
                this.leave_loose(cx);
                this.show(Active::Plugin(tab), window, cx);
                cx.notify_workspace();
            }))
            .child(
                div()
                    .size(px(16.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(glyph, 14.0, color)),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(div().flex().items_center().gap(px(6.0))
                        .child(crate::ui::behaviors::cut_text("name", agent.name).min_w_0().flex_1().text_size(px(13.0))
                            .font_weight(if unread { FontWeight::BOLD } else { FontWeight::MEDIUM })
                            .when(unread, |name| name.children(crate::ui::spot(move || format!("tab-agent-unread:{tab}")))))
                        .child(agent_metadata(format!("tab-agent-age-{tab}"), model, self.tab_agent_since(tab))))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .min_w_0()
                            .text_size(px(11.0))
                            .line_height(px(16.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(div().min_w_0().truncate().child(format!("{project},")))
                            .child(crate::ui::status_dot_slot(state, format!("tab-agent-{tab}")))
                            .child(div().min_w_0().truncate().child(status)),
                    )
                    .children(agent.summary.map(|summary| {
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.0))
                            .line_height(px(16.0))
                            .text_color(rgba(theme::MUTED_FOREGROUND << 8 | 0xb3))
                            .child(summary)
                    })),
            )
            .into_any_element()
    }

    /// Shows the agent's project and tab, and focuses its pane.
    pub(super) fn go_to_pane(
        &mut self,
        project: PathBuf,
        pane: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.see_agent(AgentKey::Pane(pane));
        self.switch_project(project.clone(), window, cx);
        self.reveal_project(&project);
        if let Some(tab) = self
            .terminals
            .iter()
            .find(|tab| tab.layout.panes().contains(&pane))
        {
            let id = tab.id;
            // `show` puts the one-off terminal away only when the tab changes, and this one may be the tab behind it.
            self.leave_loose(cx);
            self.show(Active::Terminal(id), window, cx);
            self.focus_pane(pane, window, cx);
        }
        cx.notify_workspace();
    }

    fn known_agent_model(&self, session: Option<&str>) -> Option<String> {
        let session = session?;
        let status = self.plugin_env.statusline.lock().unwrap().view();
        status["sessions"][session]["model"].as_str().map(compact_model)
    }

    /// The panes with an agent session to come back to and no live agent, in every project, in tab order: restored
    /// ones whose project you haven't opened yet, ones being resumed or that couldn't be, and ones waiting for their
    /// drive. So the Agents list keeps a session from before a restart until it's back, as the desktop's did.
    pub(super) fn pending_agents(&self, now: std::time::Instant) -> Vec<PendingAgent> {
        let shown = self.terminals.iter().map(|tab| (&self.config.project, tab));
        let parked = self
            .parked
            .iter()
            .flat_map(|(path, parked)| parked.terminals.iter().map(move |tab| (path, tab)));
        let restored: std::collections::HashMap<usize, &super::persist::WaitingPane> = self
            .parked
            .values()
            .filter_map(|parked| parked.waiting.as_ref())
            .flat_map(|waiting| waiting.panes.iter().map(|pane| (pane.id, pane)))
            .collect();
        let mut rows = Vec::new();
        for (project, tab) in shown.chain(parked) {
            for pane in tab.layout.panes() {
                if self.agents.contains_key(&pane) {
                    continue;
                }
                let (session, name, phase) = match (self.panes.get(&pane), restored.get(&pane)) {
                    (Some(open), _) => {
                        let phase = match (&open.mount, open.resuming) {
                            (Some(wait), _) => Phase::Mounting(
                                wait.volume
                                    .file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                            ),
                            (None, Some(at)) if now - at >= super::panes::RESUME_TIMEOUT => {
                                Phase::Failed
                            }
                            (None, Some(_)) => Phase::Resuming,
                            (None, None) if open.asleep => Phase::Sleeping,
                            (None, None) => continue,
                        };
                        (&open.session, &open.name, phase)
                    }
                    (None, Some(waiting)) => (&waiting.session, &waiting.name, Phase::Restored),
                    (None, None) => continue,
                };
                let Some(session) = session else {
                    continue;
                };
                let kind = match session.kind {
                    SessionKind::Claude => AgentKind::Claude,
                    SessionKind::Opencode => AgentKind::Opencode,
                };
                rows.push(PendingAgent {
                    pane,
                    project: project.clone(),
                    name: name
                        .clone()
                        .or_else(|| session.name.clone().map(Into::into))
                        .unwrap_or_else(|| agents::label(kind).into()),
                    phase,
                    model: self.known_agent_model(Some(&session.id)).or_else(|| model_flag(&session.args)),
                });
            }
        }
        // The shown project's tabs come first and the parked ones in a map's order, so a project switch would move rows.
        rows.sort_by_key(|row| row.pane);
        rows
    }

    /// Like `agent_row`, quieter, saying what the session waits for. A click goes to its pane, which starts it.
    fn pending_row(
        &self,
        row: PendingAgent,
        focused: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let PendingAgent {
            pane,
            project: path,
            name,
            phase,
            model,
        } = row;
        let project = self
            .projects
            .iter()
            .find(|p| p.path == path)
            .map_or_else(|| path.to_string_lossy().into_owned(), |p| p.name.clone());
        let status = phase.label();
        let (glyph, color) = match phase {
            Phase::Resuming => (Icon::RotateCcwClock, theme::WORKING),
            Phase::Failed => (Icon::CircleX, theme::BLOCKED),
            Phase::Restored | Phase::Sleeping | Phase::Mounting(_) => {
                (Icon::CircleDotDashed, theme::MUTED_FOREGROUND)
            }
        };
        let resuming = phase == Phase::Resuming;
        div()
            .id(("agent", pane))
            .role(Role::Button)
            .aria_label(format!("{name}, {project}, {status}"))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(8.0))
            .py(px(6.0))
            .border_1()
            .border_color(rgba(0))
            .rounded(px(theme::RADIUS))
            .cursor_pointer()
            .when(focused == Some(pane), |row| row.bg(rgba(SIDEBAR_ACCENT)))
            .when(focused != Some(pane), |row| {
                row.hover(|style| style.bg(rgba(theme::HOVER)))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.go_to_pane(path.clone(), pane, window, cx)
            }))
            .child(
                div()
                    .size(px(16.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(glyph, 14.0, color)),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div().flex().items_center().gap(px(6.0))
                            .child(crate::ui::behaviors::cut_text("name", name)
                                .min_w_0().flex_1()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .when(!resuming, |name| name.text_color(rgb(theme::MUTED_FOREGROUND))))
                            .child(agent_metadata(format!("pending-agent-{pane}"), model, None)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.0))
                            .line_height(px(16.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(format!("{project}, {status}")),
                    ),
            )
            .into_any_element()
    }
}

/// An Agents row for a session with no live agent (`pending_agents`).
pub(super) struct PendingAgent {
    pub(super) pane: usize,
    pub(super) project: PathBuf,
    pub(super) name: SharedString,
    pub(super) phase: Phase,
    model: Option<String>,
}

/// What a session with no live agent waits for, as the desktop's rows said it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    /// Its project hasn't been opened since launch.
    Restored,
    /// Wings ended its idle agent to free memory; showing the pane resumes it.
    Sleeping,
    Resuming,
    /// Its agent didn't show within `RESUME_TIMEOUT`.
    Failed,
    /// Its pane waits for the drive of this name.
    Mounting(String),
}

impl Phase {
    fn label(&self) -> String {
        match self {
            Phase::Restored => "starts when you open it".into(),
            Phase::Sleeping => "sleeping".into(),
            Phase::Resuming => "resuming…".into(),
            Phase::Failed => "couldn't resume".into(),
            Phase::Mounting(drive) => format!("waiting for {drive}"),
        }
    }
}

/// The Claude Code account a session runs under, after its name: a small quiet pill that never takes more than half the
/// line, so the name keeps the rest.
pub(super) fn account_badge(name: SharedString) -> Div {
    crate::ui::badge(
        crate::ui::Variant::Outline,
        None,
        true,
        div().min_w_0().truncate().child(name),
    )
    .max_w(relative(0.5))
    .text_color(rgb(theme::MUTED_FOREGROUND))
}

/// The state stays still. Elapsed time has its own live slot at the row's right.
fn status(agent: &Agent) -> String {
    let label = agents::state_label(agent.state).to_lowercase();
    match (agent.state, agent.state_since) {
        (AgentState::Blocked, _) => match &agent.waiting_for {
            Some(what) => format!("{label}: {what}"),
            None => label,
        },
        _ => label,
    }
}

fn model_flag(args: &[String]) -> Option<String> {
    args.iter().enumerate().find_map(|(at, arg)| {
        arg.strip_prefix("--model=").or_else(|| (arg == "--model").then(|| args.get(at + 1).map(String::as_str)).flatten())
    }).filter(|model| !model.is_empty()).map(compact_model)
}

fn compact_model(model: &str) -> String {
    let lower = model.to_lowercase();
    ["opus", "sonnet", "haiku"].into_iter().find(|name| lower.contains(name))
        .map(str::to_owned).unwrap_or_else(|| model.to_owned())
}

fn age(since: u64, now: u64) -> String {
    let seconds = now.saturating_sub(since) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86400),
    }
}

fn agent_metadata(name: String, model: Option<String>, since: Option<u64>) -> Div {
    div().flex_none().flex().items_center().gap(px(4.0))
        .text_size(px(11.0)).line_height(px(16.0))
        .font_family("Menlo").font_weight(FontWeight::NORMAL)
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .children(model.map(|model| crate::ui::behaviors::cut_text("model", model).max_w(px(58.0))))
        .children(since.map(|since| {
            let said = move || -> SharedString { age(since, now_ms()).into() };
            div().relative().w(px(28.0)).h(px(16.0))
                .child(div().text_color(gpui::transparent_black()).child(said()))
                .child(crate::ui::count_slot(name, said))
        }))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_millis() as u64)
}

/// A tab agent's state, as `setAgent` checked it.
pub(super) fn tab_state(state: &str) -> AgentState {
    match state {
        "working" => AgentState::Working,
        "blocked" => AgentState::Blocked,
        "done" => AgentState::Done,
        _ => AgentState::Idle,
    }
}

/// An agent's icon and its color, as the desktop's `PaneIcon` draws it.
fn glyph(kind: AgentKind) -> (Icon, u32) {
    match kind {
        AgentKind::Claude => (Icon::Claude, theme::CLAUDE),
        AgentKind::Opencode => (Icon::Opencode, theme::FOREGROUND),
        _ => (Icon::Terminal, theme::FOREGROUND),
    }
}

/// Where an agent row's agent runs: a pane of a project's tab, or one of the sidebar's one-off terminals by its key.
#[derive(Clone, Copy)]
enum At {
    Pane(usize),
    Loose(usize),
}

impl At {
    fn pane(self) -> Option<usize> {
        match self {
            At::Pane(pane) => Some(pane),
            At::Loose(_) => None,
        }
    }
}

/// The terminals' agents in the order their panes were made, whatever each is doing.
fn in_place(agents: &std::collections::HashMap<usize, Agent>) -> Vec<(usize, &Agent)> {
    let mut agents: Vec<(usize, &Agent)> = agents.iter().map(|(pane, agent)| (*pane, agent)).collect();
    agents.sort_by_key(|(pane, _)| *pane);
    agents
}

#[cfg(test)]
mod tests {
    use super::{AgentKind, Icon, age, model_flag, glyph, in_place};
    use crate::agents::{Agent, AgentState};
    use std::collections::HashMap;

    fn agent(pane: usize, state: AgentState) -> Agent {
        Agent {
            pane_id: pane.to_string(),
            space_id: String::new(),
            pid: 1,
            kind: AgentKind::Claude,
            session_id: None,
            name: None,
            state,
            waiting_for: None,
            args: Vec::new(),
            launcher: None,
            started_at: 0,
            state_since: None,
            direct: true,
            summary: None,
            weak_name: false,
            claude_dir: None,
        }
    }

    #[test]
    fn clicking_an_agent_leaves_the_order_alone() {
        let order = |agents: &HashMap<usize, Agent>| in_place(agents).iter().map(|(pane, _)| *pane).collect::<Vec<_>>();
        let mut agents: HashMap<usize, Agent> = [
            (1, AgentState::Idle),
            (2, AgentState::Working),
            (3, AgentState::Done),
            (4, AgentState::Blocked),
        ]
        .into_iter()
        .map(|(pane, state)| (pane, agent(pane, state)))
        .collect();
        assert_eq!(order(&agents), [1, 2, 3, 4]);
        // Focusing the Done agent turns it Idle (a focused pane never turns Done); it stays third.
        agents.get_mut(&3).unwrap().state = AgentState::Idle;
        assert_eq!(order(&agents), [1, 2, 3, 4]);
    }

    #[test]
    fn opencode_shows_its_own_icon() {
        assert_eq!(glyph(AgentKind::Opencode).0, Icon::Opencode);
        assert_eq!(glyph(AgentKind::Claude).0, Icon::Claude);
    }

    #[test]
    fn sidebar_age_is_compact_and_model_is_only_shown_when_known() {
        assert_eq!(age(0, 400), "0s");
        assert_eq!(age(0, 42_000), "42s");
        assert_eq!(age(0, 7 * 60_000 + 5_000), "7m");
        assert_eq!(age(0, 65 * 60_000), "1h");
        assert_eq!(age(10, 0), "0s");
        assert_eq!(model_flag(&[]), None);
        assert_eq!(model_flag(&["--model".into()]), None);
        assert_eq!(model_flag(&["--model".into(), "claude-opus-4-6".into()]), Some("opus".into()));
        assert_eq!(model_flag(&["--model=sonnet".into()]), Some("sonnet".into()));
        assert_eq!(model_flag(&["--model=gpt-x".into()]), Some("gpt-x".into()));
    }
}
