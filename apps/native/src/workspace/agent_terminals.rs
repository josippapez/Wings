//! The terminals an agent opens with `open_pane`, as the desktop's: the first opens below the agent's pane, to its
//! right or floating over it, as Settings, Agents says, and its next ones beside the last. A command goes to one of them
//! that sits idle instead of another. On each, the buttons hide them (the agent's pane fills the tab while they keep
//! running, and the button on the agent's pane brings them back), move them below or to the right, or float them over
//! the agent, in a panel that docks them back. Which pane opened which, and whose terminals float, last while the app
//! runs, as on the desktop.

use crate::workspace::NotifyWorkspace;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{
    AnyElement, Bounds, Context, MouseButton, MouseDownEvent, Pixels, Point, Role, SharedString,
    StyleRefinement, Window, canvas, div, prelude::*, px, rgb, rgba,
};

use crate::{
    icons::{Icon, icon},
    layout::{Dir, Layout},
    page::plugin::OpenPane,
    projects::shell_quote,
    theme,
    ui::{self, ButtonSize, motion},
};

use wings_connect::panes::PaneRow;

use super::{
    Active, TerminalTab, Workspace,
    floating::{EDGE, Gesture, Panel, Rect},
    pane_card::PaneAction,
};

/// The layer of the window that draws the terminals floating over their agent, over the panes' terminals.
pub(super) const FLOATS: &str = "agent-floats";

/// The name of an agent pane's slot in the floating layer.
pub(super) fn float_slot(owner: usize) -> SharedString {
    format!("float-{owner}").into()
}

/// Where the first terminal an agent opens goes, the desktop's `AgentTerminalsOpen`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Opens {
    #[default]
    Below,
    Right,
    Float,
}

impl Opens {
    /// Each choice as Settings names it.
    pub(super) const CHOICES: [(Opens, &str); 3] = [
        (Opens::Below, "Below the agent"),
        (Opens::Right, "Right of the agent"),
        (Opens::Float, "Floating over the agent"),
    ];

    /// The saved choice, or below the agent for anything else, as the desktop's `parseAgentTerminalsOpen`.
    pub(super) fn parse(saved: Option<&str>) -> Opens {
        match saved {
            Some("right") => Opens::Right,
            Some("float") => Opens::Float,
            _ => Opens::Below,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Opens::Below => "below",
            Opens::Right => "right",
            Opens::Float => "float",
        }
    }
}

/// Where the terminals an agent opened sit, the desktop's `agentView`: split below its pane or to its right, or
/// floating over it. `Split` brings floating ones back to the split they left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum View {
    Below,
    Right,
    Float,
    Split,
}

/// A terminal an agent opened, the desktop's `AgentTab`: the project and folder it opened in, and the agent's pane
/// when it opened as a split beside it.
struct Opened {
    project: PathBuf,
    cwd: Option<String>,
    owner: Option<usize>,
    /// The plugin tab whose agent opened it, when it shows in that tab's page.
    page: Option<usize>,
}

/// A terminal a plugin tab's agent opened, which shows in the tab's page instead of a split or a tab of its own: the tab,
/// the project, and what it was opened to run.
struct PageTerminal {
    tab: usize,
    project: PathBuf,
    title: String,
}

/// How Wings' MCP tools name a plugin tab a call came from, as they name a pane by its id.
pub(super) fn page_caller(tab: usize) -> String {
    format!("tab-{tab}")
}

/// The plugin tab a caller named by `page_caller` is.
pub(super) fn page_of_caller(caller: &str) -> Option<usize> {
    caller.strip_prefix("tab-")?.parse().ok()
}

/// The panel over an agent's pane, the desktop's `PanelState`.
#[derive(Clone, Copy)]
struct Float {
    rect: Rect,
    collapsed: bool,
    /// The terminal it shows, of several.
    active: Option<usize>,
    /// Whether the menu that picks it is open, and where its button is while it is.
    menu: bool,
    picker: Option<Bounds<Pixels>>,
    gesture: Option<Gesture>,
    /// The agent's terminal area it floats over, measured every frame.
    area: Option<Bounds<Pixels>>,
}

impl Default for Float {
    fn default() -> Self {
        Self {
            rect: Rect {
                right: EDGE,
                bottom: EDGE,
                w: 520.0,
                h: 300.0,
            },
            collapsed: false,
            active: None,
            menu: false,
            picker: None,
            gesture: None,
            area: None,
        }
    }
}

#[derive(Default)]
pub(super) struct AgentTerminals {
    opened: HashMap<usize, Opened>,
    /// The agent panes whose terminals float over them, the desktop's `floatedAgents`.
    floated: HashSet<usize>,
    /// Each agent pane's panel, kept for as long as the app runs.
    panels: HashMap<usize, Float>,
    /// The terminals plugin tabs' agents opened, by pane, which show in those tabs' pages.
    pages: HashMap<usize, PageTerminal>,
}

impl AgentTerminals {
    /// Whether the terminals `owner` opened float over it.
    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn floats(&self, owner: usize) -> bool {
        self.floated.contains(&owner)
    }

    /// Whether the menu picking the terminal `owner`'s panel shows is open.
    #[cfg(all(test, feature = "test-support"))]
    pub(super) fn menu_open(&self, owner: usize) -> bool {
        self.panels.get(&owner).is_some_and(|panel| panel.menu)
    }
}

impl Workspace {
    /// The agent pane that opened `pane` as a split beside it, while that agent's pane is open: the desktop's `openedBy`.
    pub(super) fn opened_by(&self, pane: usize) -> Option<usize> {
        let owner = self.agent_terminals.opened.get(&pane)?.owner?;
        self.panes.contains_key(&owner).then_some(owner)
    }

    /// The terminals `owner` opened that are in `layout`, oldest first.
    fn opened_in(&self, owner: usize, layout: &Layout) -> Vec<usize> {
        let mut panes: Vec<usize> = layout
            .panes()
            .into_iter()
            .filter(|pane| self.opened_by(*pane) == Some(owner))
            .collect();
        // Pane ids only grow, so this is the order they were opened in.
        panes.sort_unstable();
        panes
    }

    /// The terminals in `layout` floating over their agent instead of sitting in the split, the desktop's `floating`.
    pub(super) fn floating_in(&self, layout: &Layout) -> Vec<usize> {
        let panes = layout.panes();
        panes
            .iter()
            .copied()
            .filter(|pane| {
                self.opened_by(*pane).is_some_and(|owner| {
                    self.agent_terminals.floated.contains(&owner) && panes.contains(&owner)
                })
            })
            .collect()
    }

    /// The panes of `tab` drawn as cards: the zoomed one, or the layout's but those floating over their agent.
    pub(super) fn card_panes(&self, tab: &TerminalTab) -> Vec<usize> {
        if let Some(pane) = tab.zoomed {
            return vec![pane];
        }
        let floating = self.floating_in(&tab.layout);
        tab.layout
            .panes()
            .into_iter()
            .filter(|pane| !floating.contains(pane))
            .collect()
    }

    /// The panes of `tab` that show: its cards, and the terminal each floating panel over one of them shows unless
    /// it's minimised, as the desktop's `Host` is visible.
    pub(super) fn shown_panes(&self, tab: &TerminalTab) -> Vec<usize> {
        let cards = self.card_panes(tab);
        let floating = cards
            .iter()
            .filter_map(|owner| {
                let (here, panel) = self.floated_here(*owner, &tab.layout)?;
                (!panel.collapsed).then(|| active_of(&panel, &here))
            })
            .collect::<Vec<_>>();
        [cards, floating].concat()
    }

    /// The terminals floating over `owner` in `layout`, and their panel, while there are any.
    fn floated_here(&self, owner: usize, layout: &Layout) -> Option<(Vec<usize>, Float)> {
        if !self.agent_terminals.floated.contains(&owner) {
            return None;
        }
        let here = self.opened_in(owner, layout);
        let panel = self
            .agent_terminals
            .panels
            .get(&owner)
            .copied()
            .unwrap_or_default();
        (!here.is_empty()).then_some((here, panel))
    }

    /// Whether the agent pane `owner`, on screen in `layout`, has terminals floating over it, so its card leaves them
    /// a slot.
    pub(super) fn has_floating(&self, owner: usize, layout: &Layout) -> bool {
        self.floated_here(owner, layout).is_some()
    }

    fn tab_of(&self, pane: usize) -> Option<&TerminalTab> {
        self.terminals
            .iter()
            .find(|tab| tab.layout.panes().contains(&pane))
    }

    /// `open_pane` beside the agent's pane `owner` in the tab `tab`: the shell starts in the split without the keys, and
    /// the tab isn't shown.
    pub(super) fn open_agent_split(
        &mut self,
        tab: usize,
        owner: usize,
        plan: OpenPane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let index = self
            .terminals
            .iter()
            .position(|t| t.id == tab)
            .ok_or("The agent's tab closed")?;
        let pane = self.start_plugin_pane(&plan, window, cx)?;
        let terminal = self.terminal(pane).unwrap();
        terminal.update(cx, |view, _| view.keep_focus_away());
        // The first one opens where Settings, Agents says, the agent keeping two thirds of its place. Its next ones go
        // beside the last, so the agent's pane doesn't shrink with each command.
        let opens = self.prefs.agent_terminals_open;
        let beside = self
            .opened_in(owner, &self.terminals[index].layout)
            .last()
            .copied();
        if beside.is_none() && opens != Opens::Below {
            if opens == Opens::Float {
                self.agent_terminals.floated.insert(owner);
            } else {
                self.agent_terminals.floated.remove(&owner);
            }
        }
        let tab = &mut self.terminals[index];
        match beside {
            Some(last) => tab.layout.split(last, Dir::Row, pane),
            None => {
                let dir = if opens == Opens::Right {
                    Dir::Row
                } else {
                    Dir::Column
                };
                tab.layout.split_at(owner, dir, pane, 0.65)
            }
        }
        tab.zoomed = None;
        if let Some(input) = plan.input {
            terminal.update(cx, |view, cx| view.write_input(&input, cx));
        }
        Ok(pane)
    }

    /// Keeps `pane` as a terminal an agent opened, the agent's own pane `owner` when it opened as a split beside it, or
    /// the plugin tab `page` whose page shows it.
    pub(super) fn note_agent_terminal(
        &mut self,
        pane: usize,
        project: PathBuf,
        cwd: Option<String>,
        owner: Option<usize>,
        page: Option<usize>,
    ) {
        self.agent_terminals.opened.insert(
            pane,
            Opened {
                project,
                cwd,
                owner,
                page,
            },
        );
    }

    /// A terminal the same agent opened before in `project` that is open, at its prompt and running no agent, for the
    /// next command instead of another terminal, as the desktop's `idleAgentTab`. One already in `cwd` comes first.
    fn idle_agent_terminal(
        &self,
        project: &Path,
        cwd: Option<&str>,
        owner: Option<usize>,
        page: Option<usize>,
        cx: &gpui::App,
    ) -> Option<usize> {
        let mut idle: Vec<(usize, &Opened)> = self
            .agent_terminals
            .opened
            .iter()
            .filter(|(pane, at)| {
                at.project == project
                    && at.owner == owner
                    && at.page == page
                    && self.panes.contains_key(pane)
                    && self.pane_info.get(pane).is_some_and(|info| !info.busy)
                    && !self.agents.contains_key(pane)
                    // The scan above is up to half a second old: the shell itself has to have the terminal now, or
                    // the command would be typed into whatever was just started there.
                    && self.panes.get(pane).and_then(|p| p.view.as_ref().ok()).is_some_and(|view| {
                        let view = view.read(cx);
                        view.foreground_pid().is_some() && view.foreground_pid() == view.process_id()
                    })
            })
            .map(|(pane, at)| (*pane, at))
            .collect();
        idle.sort_unstable_by_key(|(pane, _)| *pane);
        idle.iter()
            .find(|(_, at)| at.cwd.as_deref() == cwd)
            .or(idle.first())
            .map(|(pane, _)| *pane)
    }

    /// Types `input` into an idle terminal the agent opened before (`idle_agent_terminal`), in `cwd` or the project's
    /// folder: a shell in another folder goes there first. Its agent's terminals come back into view if hidden.
    pub(super) fn reuse_agent_terminal(
        &mut self,
        project: &Path,
        cwd: Option<String>,
        owner: Option<usize>,
        page: Option<usize>,
        input: &str,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let pane = self.idle_agent_terminal(project, cwd.as_deref(), owner, page, cx)?;
        let dir = cwd
            .clone()
            .unwrap_or_else(|| project.to_string_lossy().into_owned());
        let here = self
            .pane_info
            .get(&pane)
            .and_then(|info| info.cwd.as_deref());
        let typed = match here {
            Some(here) if here != dir => format!("cd {} && {input}", shell_quote(&dir)),
            _ => input.to_owned(),
        };
        self.terminal(pane)?
            .update(cx, |view, cx| view.write_input(&typed, cx));
        self.note_agent_terminal(pane, project.to_owned(), cwd, owner, page);
        if let Some(at) = self.agent_terminals.pages.get_mut(&pane) {
            at.title = input.trim_end_matches('\r').to_owned();
            self.sync_page_terminals(cx);
        }
        if let Some(owner) = owner {
            let parked = self
                .parked
                .values_mut()
                .flat_map(|p| p.terminals.iter_mut());
            if let Some(tab) = self
                .terminals
                .iter_mut()
                .chain(parked)
                .find(|tab| tab.zoomed == Some(owner))
            {
                tab.zoomed = None;
            }
            self.sync_pane_visibility(cx);
            cx.notify_workspace();
        }
        Some(pane)
    }

    /// Moves the terminals the agent pane `owner` opened, as the desktop's `agentView`: floating over it, back into
    /// the split, or the split turned to below or beside it.
    pub(super) fn agent_view(
        &mut self,
        owner: usize,
        view: View,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .terminals
            .iter()
            .position(|tab| tab.layout.panes().contains(&owner))
        else {
            return;
        };
        if view == View::Float {
            self.agent_terminals.floated.insert(owner);
        } else {
            self.agent_terminals.floated.remove(&owner);
        }
        let dir = match view {
            View::Below => Some(Dir::Column),
            View::Right => Some(Dir::Row),
            View::Float | View::Split => None,
        };
        let split = self.terminals[index]
            .layout
            .agent_split(owner, &|pane| self.opened_by(pane).is_some());
        if let (Some(dir), Some((path, _))) = (dir, split) {
            self.terminals[index].layout.set_dir(&path, dir);
            self.save(cx);
        }
        self.show(self.active, window, cx);
        cx.notify_workspace();
    }

    fn change_float(
        &mut self,
        owner: usize,
        change: impl FnOnce(&mut Float),
        cx: &mut Context<Self>,
    ) {
        change(self.agent_terminals.panels.entry(owner).or_default());
        self.sync_pane_visibility(cx);
        cx.notify_workspace();
    }

    fn float_gesture(&mut self, owner: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(panel) = self.agent_terminals.panels.get_mut(&owner) else {
            return;
        };
        if let (Some(gesture), Some(area)) = (panel.gesture, panel.area) {
            panel.rect = gesture.rect_at(position, area);
            cx.notify_workspace();
        }
    }

    /// On a terminal an agent opened, the pill naming the agent, as the desktop's "Opened by".
    pub(super) fn opened_by_pill(&self, pane: usize, cx: &gpui::App) -> Option<AnyElement> {
        let owner = self.opened_by(pane)?;
        let title = self.pane_title(owner, cx);
        Some(
            div()
                .id(("opened-by", pane))
                .min_w_0()
                // In a narrow pane it gives way before the title, after the folder; its tooltip says it in full.
                .flex_shrink(10.0)
                .max_w(px(160.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(8.0))
                .rounded_full()
                .bg(rgba(theme::hover()))
                .text_size(px(11.0))
                .text_color(rgb(theme::muted_foreground()))
                .whitespace_nowrap()
                .tooltip(ui::tooltip(
                    format!("This terminal was opened by {title}"),
                    None,
                ))
                .child("↑")
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(format!("Opened by {title}")),
                )
                .into_any_element(),
        )
    }

    /// The glyph, tooltip and view of a terminal an agent opened's "move" button: the split turns to the other way.
    pub(super) fn opened_move(&self, owner: usize) -> (Icon, &'static str, View) {
        let beside = self
            .tab_of(owner)
            .and_then(|tab| {
                tab.layout
                    .agent_split(owner, &|pane| self.opened_by(pane).is_some())
            })
            .is_some_and(|(_, dir)| dir == Dir::Row);
        if beside {
            (Icon::PanelBottom, "Move below", View::Below)
        } else {
            (Icon::PanelRight, "Move to the right", View::Right)
        }
    }

    /// On a terminal an agent opened, the desktop's buttons: hide it, turn the split, or float it over the agent. Only
    /// those of `shown` that a narrow pane has room for; the rest are in its "…" menu.
    pub(super) fn opened_buttons(
        &self,
        pane: usize,
        shown: &[PaneAction],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let owner = self.opened_by(pane)?;
        if ![PaneAction::Hide, PaneAction::Move, PaneAction::Float]
            .iter()
            .any(|action| shown.contains(action))
        {
            return None;
        }
        let title = self.pane_title(owner, cx);
        let (glyph, label, to) = self.opened_move(owner);
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(2.0))
                .when(shown.contains(&PaneAction::Hide), |group| {
                    group.child(
                        ui::icon_button(
                            ("agent-hide", pane),
                            Icon::EyeOff,
                            format!(
                                "Hide. It keeps running, and comes back from the button on {title}"
                            ),
                            ButtonSize::Sm,
                        )
                        .children(ui::spot(|| format!("agent-hide:{pane}")))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_zoom(owner, window, cx)
                        })),
                    )
                })
                .when(shown.contains(&PaneAction::Move), |group| {
                    group.child(
                        ui::icon_button(("agent-move", pane), glyph, label, ButtonSize::Sm)
                            .children(ui::spot(|| format!("agent-move:{pane}")))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.agent_view(owner, to, window, cx)
                            })),
                    )
                })
                .when(shown.contains(&PaneAction::Float), |group| {
                    group.child(
                        ui::icon_button(
                            ("agent-float", pane),
                            Icon::PictureInPicture,
                            "Float over the agent",
                            ButtonSize::Sm,
                        )
                        .children(ui::spot(|| format!("agent-float:{pane}")))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.agent_view(owner, View::Float, window, cx)
                        })),
                    )
                })
                .into_any_element(),
        )
    }

    /// On an agent's pane, the desktop's button for the terminals it opened in the split: it hides them, as zooming
    /// the agent's pane does while they keep running, and shows them again.
    pub(super) fn agent_terminals_button(
        &self,
        owner: usize,
        zoomed: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let count = self
            .agent_terminals
            .opened
            .keys()
            .filter(|pane| self.panes.contains_key(pane) && self.opened_by(**pane) == Some(owner))
            .count();
        if count == 0 || self.agent_terminals.floated.contains(&owner) {
            return None;
        }
        let label = if zoomed {
            "Show the terminals this agent opened below it"
        } else {
            "Hide the terminals this agent opened below it. They keep running."
        };
        let text = match (zoomed, count) {
            (true, _) => format!("{count} hidden"),
            (false, 1) => "1 terminal".to_owned(),
            (false, _) => format!("{count} terminals"),
        };
        Some(
            div()
                .id(("agent-terminals", owner))
                .role(Role::Button)
                .aria_label(label)
                .aria_toggled(if zoomed {
                    gpui::Toggled::False
                } else {
                    gpui::Toggled::True
                })
                .h(px(24.0))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(8.0))
                .rounded_full()
                .bg(rgba(SECONDARY))
                .text_size(px(12.0))
                .text_color(rgb(theme::muted_foreground()))
                .whitespace_nowrap()
                .cursor_pointer()
                .hover(|style| {
                    style
                        .bg(rgba(SECONDARY_HOVER))
                        .text_color(rgb(theme::foreground()))
                })
                .tooltip(ui::tooltip(label, None))
                .children(ui::spot(|| format!("agent-terminals:{owner}")))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(
                    cx.listener(move |this, _, window, cx| this.toggle_zoom(owner, window, cx)),
                )
                .child(icon(Icon::SquareTerminal, 14.0, theme::muted_foreground()))
                .child(text)
                .into_any_element(),
        )
    }

    /// The panels floating over the agents' panes in the tab on screen, each at the slot its agent's card leaves.
    pub(super) fn float_layer(&self, cx: &mut Context<Self>) -> Vec<(SharedString, AnyElement)> {
        let Active::Terminal(id) = self.active else {
            return Vec::new();
        };
        let Some(tab) = self.terminals.iter().find(|tab| tab.id == id) else {
            return Vec::new();
        };
        if self.welcome || self.loose_page().is_some() {
            return Vec::new();
        }
        self.card_panes(tab)
            .into_iter()
            .filter_map(|owner| {
                let (here, panel) = self.floated_here(owner, &tab.layout)?;
                Some((float_slot(owner), self.float_panel(owner, here, panel, cx)))
            })
            .collect()
    }

    /// The desktop's `FloatingTerminals`: the terminals an agent opened, in a panel over its pane, with a menu to pick
    /// the one it shows when there are several.
    fn float_panel(
        &self,
        owner: usize,
        here: Vec<usize>,
        panel: Float,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = active_of(&panel, &here);
        let collapsed = panel.collapsed;
        let owner_title = self.pane_title(owner, cx);
        let mut header =
            vec![icon(Icon::SquareTerminal, 14.0, theme::muted_foreground()).into_any_element()];
        if here.len() > 1 && !collapsed {
            header.push(self.float_picker(owner, &here, active, panel.menu, cx));
        } else {
            header.push(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(self.pane_title(active, cx))
                    .into_any_element(),
            );
        }
        let more = if collapsed && here.len() > 1 {
            format!("+{} · ", here.len() - 1)
        } else {
            String::new()
        };
        let detail = self
            .pane_detail(active)
            .map_or_else(String::new, |detail| format!("{detail} · "));
        header.push(
            div()
                .min_w_0()
                // The folder and the agent give way before the tabs and the title.
                .flex_shrink(100.0)
                .truncate()
                .text_color(rgb(theme::muted_foreground()))
                .child(format!("{more}{detail}by {owner_title}"))
                .into_any_element(),
        );
        header.push(div().flex_1().into_any_element());
        let size = if collapsed {
            ButtonSize::Tiny
        } else {
            ButtonSize::Sm
        };
        if !collapsed {
            header.push(
                ui::icon_button(
                    ("float-minimise", owner),
                    Icon::Minus,
                    "Minimise to a bar",
                    size,
                )
                .children(ui::spot(|| format!("float-minimise:{owner}")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_float(owner, |panel| panel.collapsed = true, cx)
                }))
                .into_any_element(),
            );
        }
        header.push(
            ui::icon_button(
                ("float-dock", owner),
                Icon::PanelBottom,
                "Dock back into the split",
                size,
            )
            .children(ui::spot(|| format!("float-dock:{owner}")))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.agent_view(owner, View::Split, window, cx)
            }))
            .into_any_element(),
        );
        header.push(
            ui::icon_button(
                ("float-close", owner),
                Icon::X,
                format!("Close {}", self.pane_title(active, cx)),
                size,
            )
            .on_click(cx.listener(move |this, _, window, cx| this.close_pane(active, window, cx)))
            .into_any_element(),
        );
        let body = match self.terminal(active) {
            Some(view) => div()
                .id(("terminal-pane", active))
                .size_full()
                .role(Role::Group)
                .aria_label(self.pane_title(active, cx))
                .child(view.cached(StyleRefinement::default().size_full()))
                .into_any_element(),
            None => div().into_any_element(),
        };
        let frame = Panel {
            id: format!("agent-float-{owner}").into(),
            label: format!("Terminals opened by {owner_title}").into(),
            rect: panel.rect,
            area: panel.area,
            collapsed,
            full: false,
        }
        .render(
            header,
            body,
            Rc::new(move |this: &mut Workspace, sides, from| {
                let panel = this.agent_terminals.panels.entry(owner).or_default();
                panel.gesture = Some(Gesture::new(sides, from, panel.rect));
            }),
            Rc::new(move |this: &mut Workspace, _, cx| {
                this.change_float(owner, |panel| panel.collapsed = false, cx)
            }),
            cx,
        );
        let entity = cx.entity();
        let measure = canvas(
            move |bounds, _, cx| {
                entity.update(cx, |this, _| {
                    this.agent_terminals.panels.entry(owner).or_default().area = Some(bounds)
                })
            },
            {
                let entity = cx.entity();
                move |_, _, window, _| {
                    ui::window_drag(
                        window,
                        &entity,
                        move |this, event, _, cx| {
                            if event.dragging() {
                                this.float_gesture(owner, event.position, cx)
                            }
                        },
                        move |this, _, _| {
                            if let Some(panel) = this.agent_terminals.panels.get_mut(&owner) {
                                panel.gesture = None;
                            }
                        },
                    );
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .relative()
            .size_full()
            .child(measure)
            .child(frame)
            .into_any_element()
    }
}

impl Workspace {
    /// With several terminals in `owner`'s panel, the header's button naming the one it shows, and its menu of them
    /// all to pick another.
    fn float_picker(
        &self,
        owner: usize,
        here: &[usize],
        active: usize,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let items: Vec<AnyElement> = here
            .iter()
            .map(|&pane| {
                ui::select_item(
                    ("float-pick", pane),
                    self.pane_title(pane, cx),
                    pane == active,
                )
                .children(ui::spot(|| format!("float-pick:{pane}")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_float(
                        owner,
                        |panel| {
                            panel.active = Some(pane);
                            panel.menu = false;
                        },
                        cx,
                    )
                }))
                .into_any_element()
            })
            .collect();
        let entity = cx.entity();
        // While the menu is open, a press on the button closes it: the menu's press outside lets that one be.
        let measure = open.then(|| {
            canvas(
                move |bounds, _, cx| {
                    entity.update(cx, |this, _| {
                        this.agent_terminals.panels.entry(owner).or_default().picker = Some(bounds)
                    })
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        });
        let trigger = div()
            .id(("float-picker", owner))
            .role(Role::ComboBox)
            .aria_label("Terminal shown")
            .aria_expanded(open)
            .relative()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(4.0))
            .h(px(22.0))
            .pl(px(6.0))
            .pr(px(4.0))
            .rounded(px(6.0))
            .font_weight(gpui::FontWeight::MEDIUM)
            .cursor_pointer()
            .hover(|style| style.bg(rgba(theme::hover())))
            .when(open, |trigger| trigger.bg(rgba(theme::hover())))
            .children(ui::spot(|| format!("float-picker:{owner}")))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                let panel = this.agent_terminals.panels.entry(owner).or_default();
                panel.menu = !panel.menu;
                cx.notify_workspace();
            }))
            .children(measure)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .child(self.pane_title(active, cx)),
            )
            .child(icon(Icon::ChevronDown, 14.0, theme::muted_foreground()));
        let menu = self
            .popover_phase(format!("float-menu-{owner}"), open, cx)
            .map(|phase| {
                motion::with_phase(phase, || {
                    // It drops over the terminal, so a press on it mustn't reach the terminal too.
                    ui::popover(
                        ui::select_menu(items)
                            .mt(px(26.0))
                            .occlude()
                            .on_mouse_down_out(cx.listener(
                                move |this, event: &MouseDownEvent, _, cx| {
                                    let panel =
                                        this.agent_terminals.panels.entry(owner).or_default();
                                    if !panel.picker.is_some_and(|at| at.contains(&event.position))
                                    {
                                        panel.menu = false;
                                        cx.notify_workspace();
                                    }
                                },
                            )),
                    )
                })
            });
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .child(trigger)
            .children(menu)
            .into_any_element()
    }
}

impl Workspace {
    /// Whether the agent of the plugin tab `tab`, in the project on screen, has its terminals in the tab's page: its plugin
    /// declares `terminals` in `contributes.ui`.
    pub(super) fn shows_terminals(&self, tab: usize) -> bool {
        let Some(tab) = self.plugin_tabs.iter().find(|t| t.id == tab) else {
            return false;
        };
        self.plugins.iter().any(|installed| {
            installed.id == tab.plugin
                && installed.plugin.manifest.contributes.ui.iter().any(|ui| ui == "terminals")
        })
    }

    /// The plugin tabs whose agent has its terminals in their page, as Wings' MCP tools see a pane (`page_caller`), each in
    /// its project and with the programs its page runs: a call from its agent comes from one of them.
    pub(super) fn page_callers(&self) -> Vec<(PaneRow, Vec<u32>)> {
        let shown = self.plugin_tabs.iter().map(|tab| (&self.config.project, tab));
        let parked = self
            .parked
            .iter()
            .flat_map(|(path, parked)| parked.plugin_tabs.iter().map(move |tab| (path, tab)));
        shown
            .chain(parked)
            .filter_map(|(project, tab)| {
                let installed = self.plugins.iter().find(|installed| installed.id == tab.plugin)?;
                if !installed.plugin.manifest.contributes.ui.iter().any(|ui| ui == "terminals") {
                    return None;
                }
                let pids = installed.host.pids(&tab.state);
                let row = PaneRow {
                    id: page_caller(tab.id),
                    space_id: project.to_string_lossy().into_owned(),
                    cwd: project.clone(),
                    command: None,
                    title: tab.label().to_string(),
                };
                (!pids.is_empty()).then_some((row, pids))
            })
            .collect()
    }

    /// `open_pane` from the agent of the plugin tab `tab`: a terminal that shows in the tab's page. Wings keeps it as it
    /// keeps any pane, so it outlives the page and its plugin. It starts without the keys, and the tab isn't shown.
    pub(super) fn open_page_terminal(
        &mut self,
        tab: usize,
        plan: OpenPane,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let pane = self.start_plugin_pane(&plan, window, cx)?;
        let terminal = self.terminal(pane).unwrap();
        terminal.update(cx, |view, _| view.keep_focus_away());
        if let Some(input) = plan.input {
            terminal.update(cx, |view, cx| view.write_input(&input, cx));
        }
        self.agent_terminals.pages.insert(
            pane,
            PageTerminal {
                tab,
                project: plan.project,
                title,
            },
        );
        self.sync_page_terminals(cx);
        self.sync_pane_visibility(cx);
        Ok(pane)
    }

    /// The open terminals the agent of the plugin tab `tab` opened in its page, oldest first.
    pub(super) fn page_terminals_of(&self, tab: usize) -> Vec<usize> {
        let mut panes: Vec<usize> = self
            .agent_terminals
            .pages
            .iter()
            .filter(|(pane, at)| at.tab == tab && self.panes.contains_key(pane))
            .map(|(pane, _)| *pane)
            .collect();
        panes.sort_unstable();
        panes
    }

    pub(super) fn is_page_terminal(&self, pane: usize) -> bool {
        self.agent_terminals.pages.contains_key(&pane)
    }

    /// The terminals in plugin tabs' pages, each with its project, as `pane_projects` lists the panes of the tabs.
    pub(super) fn page_terminal_projects(&self) -> impl Iterator<Item = (usize, &Path)> + '_ {
        self.agent_terminals
            .pages
            .iter()
            .filter(|(pane, _)| self.panes.contains_key(pane))
            .map(|(pane, at)| (*pane, at.project.as_path()))
    }

    /// Tells the page of each plugin tab whose agent's terminals show there which they are, what each was opened to run
    /// and whether it still runs (`terminals`). A page hears the list again only when it changed.
    ///
    /// The sidebar pages of the plugin of the tab on screen hear that tab's list too, and the others an empty one: the
    /// chat's session sidebar shows the terminals of the chat in front, where the chat's page no longer does.
    pub(super) fn sync_page_terminals(&self, cx: &mut Context<Self>) {
        let listed = |tab: usize| {
            let panes = self.page_terminals_of(tab);
            let list: Vec<serde_json::Value> = panes
                .iter()
                .map(|pane| {
                    let running = self.pane_info.get(pane).is_some_and(|info| info.busy);
                    let title = self.agent_terminals.pages.get(pane).map_or("", |at| at.title.as_str());
                    serde_json::json!({ "paneId": pane.to_string(), "title": title, "running": running })
                })
                .collect();
            (panes, serde_json::Value::Array(list).to_string())
        };
        for tab in &self.plugin_tabs {
            if !self.shows_terminals(tab.id) {
                continue;
            }
            let (panes, json) = listed(tab.id);
            tab.page.update(cx, |page, cx| page.set_terminals(panes, json, cx));
        }
        let front = match self.active {
            Active::Plugin(id) => self.plugin_tabs.iter().find(|tab| tab.id == id && self.shows_terminals(id)),
            _ => None,
        };
        for ((plugin, _), page) in &self.sidebar_pages {
            let shows = self.plugins.iter().any(|installed| {
                installed.id == *plugin && installed.plugin.manifest.contributes.ui.iter().any(|ui| ui == "terminals")
            });
            if !shows {
                continue;
            }
            let (panes, json) = match front.filter(|tab| tab.plugin == *plugin) {
                Some(tab) => listed(tab.id),
                None => (Vec::new(), "[]".to_owned()),
            };
            page.update(cx, |page, cx| page.set_terminals(panes, json, cx));
        }
    }

    /// Ends a terminal a plugin tab's agent opened in its page: whether `pane` was one.
    pub(super) fn close_page_terminal(&mut self, pane: usize, cx: &mut Context<Self>) -> bool {
        if !self.agent_terminals.pages.contains_key(&pane) {
            return false;
        }
        #[cfg(unix)]
        let older = self.on_older_server(pane, cx);
        self.agent_terminals.pages.remove(&pane);
        self.agent_terminals.opened.remove(&pane);
        self.panes.remove(&pane);
        self.sync_page_terminals(cx);
        #[cfg(unix)]
        if older {
            self.tidy_older_servers(cx);
        }
        cx.notify_workspace();
        true
    }

    /// Moves a terminal out of a plugin tab's page into a tab of its own in its project, which the agent no longer
    /// types into: the tab's id.
    fn release_page_terminal(&mut self, pane: usize, cx: &mut Context<Self>) -> Option<usize> {
        let at = self.agent_terminals.pages.remove(&pane)?;
        self.agent_terminals.opened.remove(&pane);
        if !self.panes.contains_key(&pane) {
            return None;
        }
        let id = self.next_tab;
        self.next_tab += 1;
        let tab = TerminalTab {
            id,
            layout: Layout::Pane(pane),
            focused: pane,
            zoomed: None,
            focus: cx.focus_handle().tab_stop(true),
        };
        match self.parked.get_mut(&at.project) {
            Some(parked) if at.project != self.config.project => parked.terminals.push(tab),
            _ => self.terminals.push(tab),
        }
        Some(id)
    }

    /// A page terminal's button that opens it as a tab of its own, and shows that tab.
    pub(super) fn pop_page_terminal(&mut self, pane: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.release_page_terminal(pane, cx) {
            self.sync_page_terminals(cx);
            self.show(Active::Terminal(tab), window, cx);
        }
    }

    /// The terminals of the plugin tab `tab`, which is closing, go on as tabs of their own, out of sight.
    pub(super) fn release_page_terminals(&mut self, tab: usize, cx: &mut Context<Self>) {
        for pane in self.page_terminals_of(tab) {
            self.release_page_terminal(pane, cx);
        }
    }

    /// The page terminal of the plugin tab on screen that has the keyboard, which Close Pane closes before the tab.
    pub(super) fn focused_page_terminal(&self, window: &Window, cx: &gpui::App) -> Option<usize> {
        use gpui::Focusable as _;
        let Active::Plugin(tab) = self.active else {
            return None;
        };
        self.page_terminals_of(tab).into_iter().find(|pane| {
            self.terminal(*pane)
                .is_some_and(|view| view.focus_handle(cx).contains_focused(window, cx))
        })
    }

    /// The terminals of the plugin tab on screen, each at the place its page leaves for it (`page::TERMINALS`), with a
    /// header naming what it runs and whether it still does, and buttons that open it as a tab and close it.
    pub(super) fn page_terminals_layer(&self, cx: &mut Context<Self>) -> Vec<(SharedString, AnyElement)> {
        let Active::Plugin(tab) = self.active else {
            return Vec::new();
        };
        if self.welcome || self.loose_page().is_some() {
            return Vec::new();
        }
        let workspace = cx.weak_entity();
        self.page_terminals_of(tab)
            .into_iter()
            .filter_map(|pane| {
                let view = self.terminal(pane)?;
                let title = self.agent_terminals.pages.get(&pane)?.title.clone();
                let running = self.pane_info.get(&pane).is_some_and(|info| info.busy);
                let (pop, close) = (workspace.clone(), workspace.clone());
                let card = crate::page::terminal_card(
                    pane,
                    title,
                    running,
                    view,
                    move |window, cx| {
                        let _ = pop.update(cx, |this, cx| this.pop_page_terminal(pane, window, cx));
                    },
                    move |window, cx| {
                        let _ = close.update(cx, |this, cx| this.close_pane(pane, window, cx));
                    },
                );
                Some((crate::page::terminal_slot(pane), card))
            })
            .collect()
    }


    /// The agent of the plugin tab `tab` runs `command` in a terminal, for the screenshots.
    #[cfg(feature = "test-support")]
    pub fn sample_page_terminal(
        &mut self,
        tab: usize,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let allowed = self.prefs.agents_run_commands;
        self.prefs.agents_run_commands = true;
        let plan = wings_connect::panes::OpenPlan {
            space_id: self.config.project.to_string_lossy().into_owned(),
            cwd: None,
            input: Some(command.into()),
            run: true,
            next_to: Some(page_caller(tab)),
        };
        if let Err(error) = self.claude_open_pane(plan, window, cx) {
            eprintln!("Couldn't open the sample terminal: {error}");
        }
        self.prefs.agents_run_commands = allowed;
    }

    /// The plugin tabs of the project on screen, for the screenshots.
    #[cfg(feature = "test-support")]
    pub fn plugin_tab_ids(&self) -> Vec<usize> {
        self.plugin_tabs.iter().map(|tab| tab.id).collect()
    }
}

/// shadcn's secondary button fill, and when pointed at.
const SECONDARY: u32 = 0xffffff14;
const SECONDARY_HOVER: u32 = 0xffffff1d;

/// The terminal a panel shows: the one picked while it's still there, else the newest, as the desktop's.
fn active_of(panel: &Float, here: &[usize]) -> usize {
    panel
        .active
        .filter(|pane| here.contains(pane))
        .unwrap_or_else(|| here[here.len() - 1])
}

impl Workspace {
    /// Claude in pane `caller` opens a terminal for `command`, for the screenshots. `run` presses Enter for it.
    #[cfg(feature = "test-support")]
    pub fn sample_agent_terminal(
        &mut self,
        caller: usize,
        command: &str,
        run: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let allowed = self.prefs.agents_run_commands;
        self.prefs.agents_run_commands = run;
        let plan = wings_connect::panes::OpenPlan {
            space_id: self.config.project.to_string_lossy().into_owned(),
            cwd: None,
            input: Some(command.into()),
            run,
            next_to: Some(caller.to_string()),
        };
        if let Err(error) = self.claude_open_pane(plan, window, cx) {
            eprintln!("Couldn't open the sample terminal: {error}");
        }
        self.prefs.agents_run_commands = allowed;
    }
}
