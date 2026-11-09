//! The sidebar's Agents section, like the desktop's: every agent in every project, with what it's
//! doing, and a click goes to its pane.

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
    icons::{Icon, icon},
    saved::SessionKind,
    theme,
};

use super::{Active, HAIRLINE, Workspace, context_menu::MenuTarget, project::SIDEBAR_ACCENT};

/// The desktop's `bg-blocked/10` behind an agent that needs you, and `/15` on hover.
const BLOCKED_ROW: u32 = theme::BLOCKED << 8 | 0x1a;
const BLOCKED_ROW_HOVER: u32 = theme::BLOCKED << 8 | 0x26;
/// The Claude spark's brand orange, as in the desktop's `PaneIcon`.
const CLAUDE: u32 = 0xd97757;

impl Workspace {
    /// The agents in the desktop list's order (needs you, done, working, idle), or nothing while
    /// there are none.
    pub(super) fn agent_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.pending_agents(cx.background_executor().now());
        if self.agents.is_empty() && pending.is_empty() {
            return None;
        }
        let count = self.agents.len() + pending.len();
        let mut agents: Vec<(usize, &Agent)> = self.agents.iter().map(|(p, a)| (*p, a)).collect();
        let rank = |state: AgentState| match state {
            AgentState::Blocked => 0,
            AgentState::Done => 1,
            AgentState::Working => 2,
            AgentState::Idle => 3,
        };
        agents.sort_by_key(|(pane, agent)| (rank(agent.state), *pane));
        let focused = self.focused_pane();
        let mut rows: Vec<_> = agents
            .into_iter()
            .map(|(pane, agent)| self.agent_row(pane, agent, focused == Some(pane), cx))
            .collect();
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
        pane: usize,
        agent: &Agent,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = agent
            .name
            .clone()
            .unwrap_or_else(|| agents::label(agent.kind).into());
        let project = self
            .projects
            .iter()
            .find(|p| p.path.as_os_str() == agent.space_id.as_str())
            .map_or_else(|| agent.space_id.clone(), |p| p.name.clone());
        let blocked = agent.state == AgentState::Blocked;
        let path = PathBuf::from(&agent.space_id);
        let account = self.agent_accounts.get(&pane).cloned();
        let label = match &account {
            Some(account) => format!("{name}, {account} account, {project}, {}", status(agent)),
            None => format!("{name}, {project}, {}", status(agent)),
        };
        div()
            .id(("agent", pane))
            .role(Role::Button)
            .aria_label(label)
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
            .on_click(cx.listener(move |this, _, window, cx| {
                this.go_to_pane(path.clone(), pane, window, cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_menu(MenuTarget::Agent(pane), event.position, cx)
                }),
            )
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
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(name),
                            )
                            .children(account.map(account_badge)),
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
                            .child(div().flex_none().child(format!("{project},")))
                            .child(crate::ui::status_dot_slot(
                                agent.state,
                                format!("agent-{pane}"),
                            ))
                            .child(div().min_w_0().truncate().child(status(agent))),
                    ),
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
        self.switch_project(project.clone(), window, cx);
        self.reveal_project(&project);
        if let Some(tab) = self
            .terminals
            .iter()
            .find(|tab| tab.layout.panes().contains(&pane))
        {
            let id = tab.id;
            self.show(Active::Terminal(id), window, cx);
            self.focus_pane(pane, window, cx);
        }
        cx.notify();
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
                });
            }
        }
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
            Phase::Restored | Phase::Mounting(_) => {
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
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .when(!resuming, |name| {
                                name.text_color(rgb(theme::MUTED_FOREGROUND))
                            })
                            .child(name),
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
}

/// What a session with no live agent waits for, as the desktop's rows said it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    /// Its project hasn't been opened since launch.
    Restored,
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

/// "needs you", "working 42s" or "done", as the desktop's agent rows say it.
fn status(agent: &Agent) -> String {
    let label = agents::state_label(agent.state).to_lowercase();
    match (agent.state, agent.state_since) {
        (AgentState::Blocked, _) => match &agent.waiting_for {
            Some(what) => format!("{label}: {what}"),
            None => label,
        },
        (AgentState::Working, Some(since)) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            format!("{label} {}", elapsed(now.saturating_sub(since)))
        }
        _ => label,
    }
}

/// "42s", "7m", "1h 5m", like the desktop's `elapsed`.
fn elapsed(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        _ => format!("{}h {}m", s / 3600, s / 60 % 60),
    }
}

/// An agent's icon and its color, as the desktop's `PaneIcon` draws it.
fn glyph(kind: AgentKind) -> (Icon, u32) {
    match kind {
        AgentKind::Claude => (Icon::Claude, CLAUDE),
        AgentKind::Opencode => (Icon::Opencode, theme::FOREGROUND),
        _ => (Icon::Terminal, theme::FOREGROUND),
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentKind, Icon, elapsed, glyph};

    #[test]
    fn opencode_shows_its_own_icon() {
        assert_eq!(glyph(AgentKind::Opencode).0, Icon::Opencode);
        assert_eq!(glyph(AgentKind::Claude).0, Icon::Claude);
    }

    #[test]
    fn elapsed_reads_like_the_desktop() {
        assert_eq!(elapsed(400), "0s");
        assert_eq!(elapsed(42_000), "42s");
        assert_eq!(elapsed(7 * 60_000 + 5_000), "7m");
        assert_eq!(elapsed(65 * 60_000), "1h 5m");
    }
}
