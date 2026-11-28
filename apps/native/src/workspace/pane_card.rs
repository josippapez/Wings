//! A tab's panes as cards: the split layout with its dividers, each pane's header and buttons.

use gpui::{
    AnyElement, Context, MouseButton, MouseDownEvent, Orientation, Role, SharedString, Window, div,
    prelude::*, px, relative, rgb, rgba,
};

use crate::{
    icons::{Icon, icon},
    layout::{Dir, Layout, SEAM},
    theme,
    ui::{self, ButtonSize},
};

use super::{
    HAIRLINE, Workspace,
    agent_terminals::{FLOATS, float_slot},
    context_menu::MenuTarget,
    shortcut_label,
};

/// A divider's line while the pointer is on it (`bg-white/25`).
pub(super) const DIVIDER_HOVER: u32 = 0xffffff40;
/// How far each side of its line a divider can be grabbed.
const GRAB: f32 = 3.0;
/// A pane header's height, and the rule along the top of the focused one's.
pub(super) const HEADER: f32 = 32.0;

/// The layer of the window that draws the panes' terminals, over their cards (`ui::slot_layer`).
pub(super) const TERMINALS: &str = "terminals";

/// The name of a pane's slot in the terminal layer.
pub(super) fn terminal_slot(pane: usize) -> SharedString {
    format!("pane-{pane}").into()
}

impl Workspace {
    /// The cards of `node`, but the `hidden` panes, which float over their agent.
    pub(super) fn layout_element(
        &self,
        tab: usize,
        node: &Layout,
        path: &mut Vec<bool>,
        focused: usize,
        hidden: &[usize],
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Layout::Split { dir, ratio, a, b } = node else {
            let Layout::Pane(pane) = node else {
                unreachable!()
            };
            return self.pane_card(*pane, *pane == focused, false, width, cx);
        };
        if let Some((second, side)) = Layout::only_side(a, b, hidden) {
            path.push(second);
            let shown = self.layout_element(tab, side, path, focused, hidden, width, cx);
            path.pop();
            return shown;
        }
        let row = *dir == Dir::Row;
        // As `Layout::place` sizes them: the first side takes its ratio of a row, then the divider, then the rest.
        let (first_width, second_width) = if row {
            let first = width * *ratio;
            (first, width - first - SEAM)
        } else {
            (width, width)
        };
        path.push(false);
        let first = self.layout_element(tab, a, path, focused, hidden, first_width, cx);
        path.pop();
        path.push(true);
        let second = self.layout_element(tab, b, path, focused, hidden, second_width, cx);
        path.pop();
        let at = path.clone();
        let group = SharedString::from(format!("divider-{tab}-{at:?}"));
        // The panes' shared line, one point wide and in the flow. Its grab area reaches over both panes beside it,
        // as the last child of the split, so it is over them.
        let group_name = group.clone();
        let line = div()
            .flex_none()
            .map(|d| if row { d.w(px(SEAM)).h_full() } else { d.h(px(SEAM)).w_full() })
            .bg(rgba(HAIRLINE));
        let grab = div()
            .id(group.clone())
            .group(group.clone())
            .role(Role::Splitter)
            .aria_label("Resize panes")
            .aria_orientation(if row {
                Orientation::Vertical
            } else {
                Orientation::Horizontal
            })
            .absolute()
            .map(|d| {
                if row {
                    d.left(relative(*ratio))
                        .ml(px(-GRAB))
                        .top_0()
                        .h_full()
                        .w(px(SEAM + 2.0 * GRAB))
                        .cursor_col_resize()
                } else {
                    d.top(relative(*ratio))
                        .mt(px(-GRAB))
                        .left_0()
                        .w_full()
                        .h(px(SEAM + 2.0 * GRAB))
                        .cursor_row_resize()
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.start_drag(tab, at.clone(), cx)),
            )
            // The line again over the shared one, lit while the pointer is on the grab area.
            .child(
                div()
                    .absolute()
                    .map(|d| {
                        if row {
                            d.top_0().bottom_0().left(px(GRAB)).w(px(SEAM))
                        } else {
                            d.left_0().right_0().top(px(GRAB)).h(px(SEAM))
                        }
                    })
                    .group_hover(group_name, |style| style.bg(rgba(DIVIDER_HOVER))),
            );
        div()
            .relative()
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .map(|d| if row { d.flex_row() } else { d.flex_col() })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .flex_basis(relative(*ratio))
                    .min_w_0()
                    .min_h_0()
                    .child(first),
            )
            .child(line)
            .child(div().flex().flex_1().min_w_0().min_h_0().child(second))
            .child(grab)
            .into_any_element()
    }

    /// Splits `pane` in `dir`: it takes the keys, then the new shell opens beside it.
    pub(super) fn split_pane(
        &mut self,
        pane: usize,
        dir: Dir,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_pane(pane, window, cx);
        self.split(dir, window, cx);
    }

    /// Like the desktop's pane card: a header with the title and the split and close buttons, which
    /// show on the focused pane and on hover.
    pub(super) fn pane_card(
        &self,
        pane: usize,
        focused: bool,
        zoomed: bool,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = self.pane_title(pane, cx);
        // Left out of a pane too narrow to show more than "…" of it, so the title keeps the room.
        let detail = self.pane_detail(pane).filter(|_| width >= DETAIL_MIN_WIDTH);
        let state = self.agents.get(&pane).map(|agent| agent.state);
        let renaming = self
            .panes
            .get(&pane)
            .and_then(|pane| pane.renaming.as_ref())
            .map(|(field, _)| field.clone());
        let group = SharedString::from(format!("pane-{pane}"));
        let strip = self.status_strip(pane, cx);
        let body = match self.panes.get(&pane).map(|p| (&p.view, &p.focus)) {
            // The terminal draws in the window's terminal layer at this slot, so its output draws neither the main
            // column again nor the other terminals in it (`Workspace::render`).
            Some((Ok(view), _)) => {
                view.read(cx).round_bottom(0.0);
                let terminal = ui::slot(TERMINALS, terminal_slot(pane), None);
                // The terminals the agent in it opened that float over it draw in a layer over its terminal.
                let floating = self
                    .terminals
                    .iter()
                    .find(|tab| tab.layout.panes().contains(&pane))
                    .is_some_and(|tab| self.has_floating(pane, &tab.layout));
                div()
                    .relative()
                    .size_full()
                    .child(terminal)
                    .when(floating, |body| {
                        body.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .child(ui::slot(FLOATS, float_slot(pane), None)),
                        )
                    })
                    .into_any_element()
            }
            Some((Err(error), focus)) => {
                let mount = self.panes.get(&pane).and_then(|p| p.mount.as_ref());
                let (heading, detail, color) = match mount {
                    Some(wait) => (
                        format!(
                            "Waiting for the drive “{}”",
                            wait.volume
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                        ),
                        "It starts in its folder once the drive is mounted.".into(),
                        theme::muted_foreground(),
                    ),
                    None => (
                        "Couldn't start the terminal".to_owned(),
                        error.to_string(),
                        theme::danger(),
                    ),
                };
                div()
                    .track_focus(focus)
                    // A width of its own, so a long line wraps in a narrow pane.
                    .size_full()
                    .p(px(24.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(heading))
                    .child(div().text_color(rgb(color)).child(detail))
                    .into_any_element()
            }
            None => div().into_any_element(),
        };
        let badges = self.badge_pills(pane, cx);
        let opened_by = self.opened_by_pill(pane, width, cx);
        #[cfg(unix)]
        let older_server = self.older_server_marker(pane, cx);
        #[cfg(not(unix))]
        let older_server: Option<AnyElement> = None;
        let opened_terminals = self.agent_terminals_button(pane, zoomed, cx);
        let (shown, more) = header_plan(width, self.opened_by(pane).is_some());
        let opened_buttons = self.opened_buttons(pane, &shown, cx);
        // What those take at the header's right edge: their buttons and the gap before them.
        let opened_width = match [PaneAction::Hide, PaneAction::Move, PaneAction::Float]
            .into_iter()
            .filter(|action| opened_buttons.is_some() && shown.contains(action))
            .count()
        {
            0 => 0.0,
            n => n as f32 * (HEADER_BUTTON + HEADER_GAP) - HEADER_GAP + 8.0,
        };
        let header = div()
            .id(SharedString::from(format!("pane-header-{pane}")))
            .relative()
            .h(px(HEADER))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(12.0))
            .pr(px(6.0))
            .border_b_1()
            .border_color(rgba(HAIRLINE))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if event.click_count >= 2 {
                        this.start_rename(pane, window, cx);
                    } else {
                        this.focus_pane(pane, window, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_menu(MenuTarget::Pane(pane), event.position, cx)
                }),
            )
            .child(match state {
                Some(state) => crate::ui::status_dot_slot(state, format!("pane-{pane}")),
                None => icon(Icon::Terminal, 14.0, theme::muted_foreground()).into_any_element(),
            })
            .map(|header| match renaming {
                // Bordered like the desktop's InlineName, with its text where the title was.
                Some(field) => header.child(
                    div()
                        .ml(px(-9.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(rgba(theme::hairline_strong()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(field),
                ),
                // The desktop's `max-w-[50%] truncate`: in a narrow pane the title gives way, not the buttons.
                None => header.child(
                    div()
                        .id(SharedString::from(format!("pane-title-{pane}")))
                        .min_w_0()
                        .max_w(relative(0.5))
                        .truncate()
                        .text_size(px(13.0))
                        .font_weight(if focused {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::MEDIUM
                        })
                        .text_color(rgb(if focused {
                            theme::foreground()
                        } else {
                            theme::secondary_foreground()
                        }))
                        .tooltip(ui::tooltip(title.clone(), None))
                        .children(ui::spot(|| format!("pane-title:{pane}")))
                        .child(title.clone()),
                ),
            })
            .when_some(detail, |header, detail| {
                header.child(
                    div()
                        .min_w_0()
                        // The folder gives way before the title does: shrinking by their widths alone, a long path
                        // left the title "…" in a narrow pane.
                        .flex_shrink(100.0)
                        // A path is cut from its start, so the folder's own name stays; a project's name from its end.
                        .when(detail.contains('/'), |detail| {
                            detail.overflow_hidden().whitespace_nowrap().text_ellipsis_start()
                        })
                        .when(!detail.contains('/'), |detail| detail.truncate())
                        .text_size(px(12.0))
                        .text_color(rgb(theme::muted_foreground()))
                        .children(ui::spot(|| format!("pane-detail:{pane}")))
                        .child(detail),
                )
            })
            .children(opened_by)
            .children(older_server)
            .children(opened_terminals)
            .child(div().flex_1())
            .children(badges)
            .children(opened_buttons)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .opacity(if focused { 0.7 } else { 0.0 })
                    .group_hover(group.clone(), |style| style.opacity(1.0))
                    // Unfocused, they are out of the header's row, so they take no room from the title: they show over
                    // its end when the pointer is over the pane, left of the buttons an agent's terminal always shows.
                    .when(!focused, |group| {
                        group
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right(px(6.0 + opened_width))
                            .pl(px(4.0))
                            .bg(rgb(theme::surface()))
                    })
                    .when(shown.contains(&PaneAction::SplitRight), |group| {
                        group.child(pane_button(
                            pane,
                            "Split right",
                            Some(shortcut_label("D", false)),
                            Icon::Columns,
                            cx,
                            |this, pane, window, cx| this.split_pane(pane, Dir::Row, window, cx),
                        ))
                    })
                    .when(shown.contains(&PaneAction::SplitDown), |group| {
                        group.child(pane_button(
                            pane,
                            "Split down",
                            Some(shortcut_label("D", true)),
                            Icon::Rows,
                            cx,
                            |this, pane, window, cx| this.split_pane(pane, Dir::Column, window, cx),
                        ))
                    })
                    .when(shown.contains(&PaneAction::Zoom), |group| {
                        group.child(pane_button(
                            pane,
                            if zoomed { "Restore" } else { "Zoom" },
                            None,
                            if zoomed {
                                Icon::Minimize
                            } else {
                                Icon::Maximize
                            },
                            cx,
                            |this, pane, window, cx| this.toggle_zoom(pane, window, cx),
                        ))
                    })
                    // What a narrow pane has no room for goes behind this, in the pane's menu.
                    .when(!more.is_empty(), |group| {
                        let hidden = more.clone();
                        group.child(
                            ui::icon_button(
                                SharedString::from(format!("pane-more-{pane}")),
                                Icon::Ellipsis,
                                "More",
                                ButtonSize::Sm,
                            )
                            .children(ui::spot(|| format!("pane-more:{pane}")))
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                this.open_menu(
                                    MenuTarget::PaneMore(pane, hidden.clone()),
                                    event.position(),
                                    cx,
                                )
                            })),
                        )
                    })
                    .when(shown.contains(&PaneAction::Close), |group| {
                        group.child(pane_button(
                            pane,
                            "Close pane",
                            Some(shortcut_label("W", false)),
                            Icon::X,
                            cx,
                            |this, pane, window, cx| this.close_pane(pane, window, cx),
                        ))
                    }),
            )
            .children(ui::spot(|| format!("pane-header:{pane}")));
        div()
            .id(SharedString::from(format!("pane-{pane}")))
            .group(group)
            .role(Role::Group)
            .aria_label(title)
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(rgb(theme::surface()))
            .child(header)
            .child(div().flex().flex_1().min_h_0().child(body))
            .children(strip)
            .into_any_element()
    }
}

/// A pane header's buttons, and the entries of the menu a narrow pane moves them to. In the order they go in the menu's
/// priority: the last ones are the first to move behind "…".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaneAction {
    Close,
    /// On a terminal an agent opened: hide it, turn the split, or float it over the agent.
    Hide,
    Zoom,
    SplitRight,
    SplitDown,
    Float,
    Move,
}

/// The narrowest pane whose header shows the folder or project after the title.
const DETAIL_MIN_WIDTH: f32 = 300.0;

/// The width a header keeps for its icon, padding, gaps and a few letters of the title.
const HEADER_FIXED: f32 = 84.0;
/// A header button's width and the gap between two.
const HEADER_BUTTON: f32 = 28.0;
const HEADER_GAP: f32 = 2.0;

/// Which of a header's buttons show in a pane `width` wide, and which move into the "…" menu that takes the place of
/// the last one shown. `opened` is a terminal an agent opened, with its three buttons more. Each list is in priority
/// order, Close first.
pub(super) fn header_plan(width: f32, opened: bool) -> (Vec<PaneAction>, Vec<PaneAction>) {
    use PaneAction::*;
    let all: &[PaneAction] = if opened {
        &[Close, Hide, Zoom, SplitRight, SplitDown, Float, Move]
    } else {
        &[Close, Zoom, SplitRight, SplitDown]
    };
    let room = width - HEADER_FIXED + HEADER_GAP;
    let fits = (room / (HEADER_BUTTON + HEADER_GAP)).floor().max(0.0) as usize;
    if fits >= all.len() {
        return (all.to_vec(), Vec::new());
    }
    let shown = fits.saturating_sub(1);
    (all[..shown].to_vec(), all[shown..].to_vec())
}

/// A pane header's button: the desktop's `IconButton`, 28px with a 16px icon, aimed at this pane.
fn pane_button(
    pane: usize,
    label: &'static str,
    shortcut: Option<String>,
    glyph: Icon,
    cx: &mut Context<Workspace>,
    action: fn(&mut Workspace, usize, &mut Window, &mut Context<Workspace>),
) -> impl IntoElement + use<> {
    ui::shortcut_icon_button(
        SharedString::from(format!("{label}-{pane}")),
        glyph,
        label,
        shortcut.map(Into::into),
        ButtonSize::Sm,
    )
    .on_click(cx.listener(move |this, _, window, cx| action(this, pane, window, cx)))
}
