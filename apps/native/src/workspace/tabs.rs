//! The tab strip: opening, closing, moving and switching tabs, and showing the active one.

use gpui::{
    App, BoxShadow, Context, FocusHandle, Focusable, FontWeight, MouseButton, Role, SharedString,
    Window, div, hsla, point, prelude::*, px, rgb, rgba,
};

use crate::{
    agents::AgentState,
    icons::{Icon, icon},
    layout::Layout,
    theme,
    ui::{self, ButtonSize},
};

use super::{Active, TerminalTab, Workspace, shortcut_label};

/// The desktop's shown tab, `bg-white/[0.07]`.
const SELECTED_TAB: u32 = 0xffffff12;

impl Workspace {
    pub fn active(&self) -> Active {
        self.active
    }

    /// The tabs in strip order: each terminal tab's id, each plugin page's, then Issues.
    pub fn tabs(&self) -> Vec<Active> {
        self.terminals
            .iter()
            .map(|tab| Active::Terminal(tab.id))
            .chain(self.plugin_tabs.iter().map(|tab| Active::Plugin(tab.id)))
            .chain([Active::Issues])
            .collect()
    }

    /// A tab is named after its focused pane.
    pub fn label(&self, tab: usize, cx: &App) -> SharedString {
        self.terminals
            .iter()
            .find(|t| t.id == tab)
            .map_or_else(|| "Terminal".into(), |t| self.pane_title(t.focused, cx))
    }

    pub fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.open_pane(None, None, cx);
        let id = self.next_tab;
        self.next_tab += 1;
        self.terminals.push(TerminalTab {
            id,
            layout: Layout::Pane(pane),
            focused: pane,
            zoomed: None,
            focus: cx.focus_handle().tab_stop(true),
        });
        self.show(Active::Terminal(id), window, cx);
    }

    pub(super) fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.terminals.remove(index);
        for pane in tab.layout.panes() {
            self.panes.remove(&pane);
        }
        self.after_closing(Active::Terminal(tab.id), index, window, cx);
    }

    /// Once `closed` has left the strip from `position` (Issues, last, never counts): when it was shown, the neighbour to
    /// its right takes over, else the one to its left, else Issues.
    pub(super) fn after_closing(
        &mut self,
        closed: Active,
        position: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active == closed {
            let strip: Vec<Active> = self
                .tabs()
                .into_iter()
                .filter(|tab| *tab != Active::Issues)
                .collect();
            let next = strip
                .get(position)
                .or_else(|| position.checked_sub(1).and_then(|left| strip.get(left)))
                .copied()
                .unwrap_or(Active::Issues);
            self.show(next, window, cx);
        } else {
            self.save(cx);
        }
    }

    pub fn show(&mut self, active: Active, window: &mut Window, cx: &mut Context<Self>) {
        self.active = active;
        let shown: Vec<usize> = match active {
            Active::Terminal(id) => self
                .terminals
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| {
                    tab.zoomed
                        .map_or_else(|| tab.layout.panes(), |pane| vec![pane])
                })
                .unwrap_or_default(),
            Active::Plugin(_) | Active::Issues => Vec::new(),
        };
        for (id, pane) in &self.panes {
            if let Ok(view) = &pane.view {
                // Hidden terminals keep parsing output but stop painting.
                let visible = shown.contains(id);
                view.update(cx, |view, cx| view.set_visible(visible, cx));
            }
        }
        match active {
            Active::Terminal(id) => {
                let focused = self
                    .terminals
                    .iter()
                    .find(|tab| tab.id == id)
                    .map(|t| t.focused);
                if let Some(pane) = focused.and_then(|pane| self.panes.get(&pane)) {
                    match &pane.view {
                        Ok(view) => view.focus_handle(cx).focus(window, cx),
                        Err(_) => pane.focus.focus(window, cx),
                    }
                }
            }
            // The page has no focus of its own to land on, so the tab does, and the shortcuts keep working.
            Active::Plugin(id) => {
                if let Some(tab) = self.plugin_tabs.iter().find(|tab| tab.id == id) {
                    tab.focus.focus(window, cx);
                }
            }
            Active::Issues => self.issues.focus_handle(cx).focus(window, cx),
        }
        self.save(cx);
        cx.notify();
    }

    /// Next or previous tab, wrapping round.
    pub(super) fn cycle(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let tabs = self.tabs();
        let current = tabs.iter().position(|tab| *tab == self.active).unwrap_or(0);
        let next = if forward {
            (current + 1) % tabs.len()
        } else {
            (current + tabs.len() - 1) % tabs.len()
        };
        self.show(tabs[next], window, cx);
    }

    /// Swaps the active tab with its neighbour of the same kind: terminals among terminals, plugin pages among plugin
    /// pages. Issues stays at the end.
    pub(super) fn move_active_tab(&mut self, right: bool, cx: &mut Context<Self>) {
        let step = |index: usize| {
            if right {
                index + 1
            } else {
                index.wrapping_sub(1)
            }
        };
        match self.active {
            Active::Terminal(id) => {
                let Some(index) = self.terminals.iter().position(|tab| tab.id == id) else {
                    return;
                };
                let other = step(index);
                if other >= self.terminals.len() {
                    return;
                }
                self.terminals.swap(index, other);
            }
            Active::Plugin(id) => {
                let Some(index) = self.plugin_tabs.iter().position(|tab| tab.id == id) else {
                    return;
                };
                let other = step(index);
                if other >= self.plugin_tabs.len() {
                    return;
                }
                self.plugin_tabs.swap(index, other);
            }
            Active::Issues => return,
        }
        self.save(cx);
        cx.notify();
    }

    pub(super) fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.active {
            Active::Terminal(id) => {
                if let Some(index) = self.terminals.iter().position(|tab| tab.id == id) {
                    self.close_tab(index, window, cx);
                    cx.notify();
                }
            }
            Active::Plugin(id) => {
                if let Some(index) = self.plugin_tabs.iter().position(|tab| tab.id == id) {
                    self.close_plugin_tab(index, window, cx);
                    cx.notify();
                }
            }
            Active::Issues => {}
        }
    }

    pub(super) fn tab(
        &self,
        active: Active,
        label: SharedString,
        state: Option<AgentState>,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let selected = self.active == active;
        let id = match active {
            Active::Terminal(id) => SharedString::from(format!("terminal-tab-{id}")),
            Active::Plugin(id) => SharedString::from(format!("plugin-tab-{id}")),
            Active::Issues => "issues-tab".into(),
        };
        let glyph = match (state, active) {
            (Some(state), _) => {
                crate::ui::status_dot(format!("{id}-dot"), state).into_any_element()
            }
            (None, Active::Terminal(_)) => {
                icon(Icon::Terminal, 14.0, theme::MUTED_FOREGROUND).into_any_element()
            }
            (None, Active::Plugin(_)) => {
                icon(Icon::Puzzle, 14.0, theme::MUTED_FOREGROUND).into_any_element()
            }
            (None, Active::Issues) => div().into_any_element(),
        };
        let group = SharedString::from(format!("{id}-group"));
        let close = match active {
            Active::Terminal(tab) => Some(
                ui::icon_button(
                    SharedString::from(format!("{id}-close")),
                    Icon::X,
                    format!("Close tab ({})", shortcut_label("W", true)),
                    ButtonSize::Tiny,
                )
                .opacity(0.0)
                .group_hover(group.clone(), |style| style.opacity(1.0))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(index) = this.terminals.iter().position(|t| t.id == tab) {
                        this.close_tab(index, window, cx);
                        cx.notify();
                    }
                })),
            ),
            Active::Plugin(tab) => Some(
                ui::icon_button(
                    SharedString::from(format!("{id}-close")),
                    Icon::X,
                    format!("Close tab ({})", shortcut_label("W", true)),
                    ButtonSize::Tiny,
                )
                .opacity(0.0)
                .group_hover(group.clone(), |style| style.opacity(1.0))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(index) = this.plugin_tabs.iter().position(|t| t.id == tab) {
                        this.close_plugin_tab(index, window, cx);
                        cx.notify();
                    }
                })),
            ),
            Active::Issues => None,
        };
        // The desktop's tab: equal widths, and a raised pill for the one shown.
        div()
            .id(id)
            .group(group)
            .role(Role::Tab)
            .aria_label(label.clone())
            .aria_selected(selected)
            .track_focus(focus)
            .flex_1()
            .min_w(px(144.0))
            .h(px(28.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(10.0))
            .pr(px(4.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(rgba(0))
            .cursor_pointer()
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(if selected {
                theme::FOREGROUND
            } else {
                theme::MUTED_FOREGROUND
            }))
            .when(selected, |tab| {
                tab.bg(rgba(SELECTED_TAB))
                    .border_color(rgba(theme::HAIRLINE_STRONG))
                    .shadow(vec![
                        BoxShadow {
                            color: hsla(0.0, 0.0, 1.0, 0.06),
                            offset: point(px(0.0), px(1.0)),
                            blur_radius: px(0.0),
                            spread_radius: px(0.0),
                            inset: true,
                        },
                        BoxShadow {
                            color: hsla(0.0, 0.0, 0.0, 0.25),
                            offset: point(px(0.0), px(1.0)),
                            blur_radius: px(3.0),
                            spread_radius: px(0.0),
                            inset: false,
                        },
                    ])
            })
            .when(!selected, |tab| {
                tab.hover(|style| {
                    style
                        .bg(rgba(theme::HOVER))
                        .text_color(rgb(theme::FOREGROUND))
                })
            })
            .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
            // Not a window drag.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| this.show(active, window, cx)))
            .child(glyph)
            .child(div().flex_1().min_w_0().truncate().child(label))
            .children(close)
    }
}
