//! The tab strip: opening, closing, moving and switching tabs, and showing the active one.

use gpui::{
    AnyElement, App, BoxShadow, Context, FocusHandle, Focusable, FontWeight, MouseButton,
    MouseDownEvent, Role, SharedString, Window, div, hsla, point, prelude::*, px, rgb, rgba,
};

use crate::{
    agents::AgentState,
    icons::{Icon, icon, plugin_glyph},
    layout::{Dir, Layout},
    theme,
    ui::{self, ButtonSize},
};

use super::{Active, TerminalTab, Workspace, context_menu::MenuTarget, shortcut_label};

/// The desktop's shown tab, `bg-white/[0.07]`.
const SELECTED_TAB: u32 = 0xffffff12;
/// The close button's `top-1 right-1`, measured inside the tab's 1px border.
const CLOSE_INSET: f32 = 3.0;

impl Workspace {
    pub fn active(&self) -> Active {
        self.active
    }

    /// The tabs in strip order: each terminal tab's id, each plugin page's, the project page if it is open, then Issues.
    pub fn tabs(&self) -> Vec<Active> {
        self.terminals
            .iter()
            .map(|tab| Active::Terminal(tab.id))
            .chain(self.plugin_tabs.iter().map(|tab| Active::Plugin(tab.id)))
            .chain(self.project_page.is_some().then_some(Active::Project))
            .chain((self.issues_open && !self.has_tasks_board()).then_some(Active::Issues))
            .collect()
    }

    /// A tab is named after its focused pane.
    pub fn label(&self, tab: usize, cx: &App) -> SharedString {
        self.terminals
            .iter()
            .find(|t| t.id == tab)
            .map_or_else(|| "Terminal".into(), |t| self.pane_title(t.focused, cx))
    }

    /// The + button: what Settings, General says it opens. A plugin page that is gone opens a terminal.
    pub(super) fn new_tab_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prefs.new_tab == "project" && self.project_info.is_some() {
            return self.open_project_page(self.config.project.clone(), window, cx);
        }
        if let Some((plugin, view)) = self.prefs.new_tab.clone().split_once(':')
            && self.open_plugin_view(plugin, view, None, window, cx)
        {
            return;
        }
        self.new_terminal(window, cx);
    }

    pub fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // With no project there is no folder to open it in.
        if self.welcome {
            return;
        }
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
    /// its right takes over, else the one to its left, else Issues if it is open, else a new terminal.
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
                .copied();
            match next {
                Some(next) => self.show(next, window, cx),
                None if self.issues_open => self.show(Active::Issues, window, cx),
                None => self.new_terminal(window, cx),
            }
        } else {
            self.save(cx);
        }
    }

    pub fn show(&mut self, active: Active, window: &mut Window, cx: &mut Context<Self>) {
        if active == Active::Issues && self.has_tasks_board() {
            self.open_plugin_view("tasks", "board", None, window, cx);
            return;
        }
        let moved = self.active != active;
        self.active = active;
        self.issues_open |= active == Active::Issues;
        if moved {
            self.leave_loose(cx);
        }
        if let Some(index) = self.tabs().iter().position(|tab| *tab == active) {
            self.tab_scroll.scroll_to_item(index);
        }
        self.sync_plugin_visibility(cx);
        self.sync_pane_visibility(cx);
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
            Active::Project => self.project_page_focus.focus(window, cx),
            Active::Issues => self.issues.focus_handle(cx).focus(window, cx),
        }
        // A sidebar terminal filling the pane area is what's on screen, so the keys go back to it.
        if let Some(page) = self.loose_page().and_then(|key| self.loose_terminal(key)) {
            page.view().focus_handle(cx).focus(window, cx);
        }
        self.sync_project_page(cx);
        self.save(cx);
        cx.notify();
    }

    /// Paints the active tab's panes, unless a sidebar terminal fills the pane area over them, as the desktop's
    /// `invisible` tab under the terminal page. Hidden terminals keep parsing output and keep their size.
    pub(super) fn sync_pane_visibility(&mut self, cx: &mut Context<Self>) {
        let shown: Vec<usize> = match self.active {
            _ if self.loose_page().is_some() => Vec::new(),
            Active::Terminal(id) => self
                .terminals
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| {
                    tab.zoomed
                        .map_or_else(|| tab.layout.panes(), |pane| vec![pane])
                })
                .unwrap_or_default(),
            Active::Plugin(_) | Active::Project | Active::Issues => Vec::new(),
        };
        for (id, pane) in &self.panes {
            if let Ok(view) = &pane.view {
                let visible = shown.contains(id);
                view.update(cx, |view, cx| view.set_visible(visible, cx));
            }
        }
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
        self.move_tab(self.active, right, cx);
    }

    /// Where `tab` sits among its own kind, and whether it can move left and right.
    pub(super) fn tab_moves(&self, tab: Active) -> (bool, bool) {
        let (index, len) = match tab {
            Active::Terminal(id) => (
                self.terminals.iter().position(|t| t.id == id),
                self.terminals.len(),
            ),
            Active::Plugin(id) => (
                self.plugin_tabs.iter().position(|t| t.id == id),
                self.plugin_tabs.len(),
            ),
            Active::Project | Active::Issues => (None, 0),
        };
        index.map_or((false, false), |index| (index > 0, index + 1 < len))
    }

    pub(super) fn move_tab(&mut self, tab: Active, right: bool, cx: &mut Context<Self>) {
        let step = |index: usize| {
            if right {
                index + 1
            } else {
                index.wrapping_sub(1)
            }
        };
        match tab {
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
            Active::Project | Active::Issues => return,
        }
        self.save(cx);
        cx.notify();
    }

    pub(super) fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab_of(self.active, window, cx);
    }

    pub(super) fn close_tab_of(
        &mut self,
        tab: Active,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match tab {
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
            Active::Project => {
                self.close_project_page(window, cx);
                cx.notify();
            }
            Active::Issues => {
                self.close_issues(window, cx);
                cx.notify();
            }
        }
    }

    fn close_issues(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let position = self.tabs().len() - 1;
        self.issues_open = false;
        self.after_closing(Active::Issues, position, window, cx);
    }

    pub(super) fn tab(
        &self,
        active: Active,
        label: SharedString,
        state: Option<AgentState>,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // While a sidebar terminal fills the pane area, no tab is the one shown, as in the desktop app.
        let selected = self.active == active && self.loose_page().is_none();
        let id = match active {
            Active::Terminal(id) => SharedString::from(format!("terminal-tab-{id}")),
            Active::Plugin(id) => SharedString::from(format!("plugin-tab-{id}")),
            Active::Project => "project-tab".into(),
            Active::Issues => "issues-tab".into(),
        };
        let glyph = match (state, active) {
            (Some(state), _) => crate::ui::status_dot(state).into_any_element(),
            (None, Active::Terminal(_)) => {
                icon(Icon::Terminal, 14.0, theme::MUTED_FOREGROUND).into_any_element()
            }
            (None, Active::Plugin(tab)) => self.plugin_tab_glyph(tab),
            (None, Active::Project) => {
                icon(Icon::FileText, 14.0, theme::MUTED_FOREGROUND).into_any_element()
            }
            (None, Active::Issues) => {
                icon(Icon::List, 14.0, theme::MUTED_FOREGROUND).into_any_element()
            }
        };
        let color = if selected {
            theme::FOREGROUND
        } else {
            theme::MUTED_FOREGROUND
        };
        let splits = match active {
            Active::Terminal(id) => self
                .terminals
                .iter()
                .find(|tab| tab.id == id)
                .and_then(|tab| layout_glyph(tab.id, &tab.layout, tab.focused, color)),
            _ => None,
        };
        let group = SharedString::from(format!("{id}-group"));
        let close = match active {
            Active::Terminal(tab) => Some(
                ui::icon_button(
                    SharedString::from(format!("{id}-close")),
                    Icon::X,
                    "Close tab",
                    ButtonSize::Tiny,
                )
                .absolute()
                .top(px(CLOSE_INSET))
                .right(px(CLOSE_INSET))
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
                    "Close tab",
                    ButtonSize::Tiny,
                )
                .absolute()
                .top(px(CLOSE_INSET))
                .right(px(CLOSE_INSET))
                .opacity(0.0)
                .group_hover(group.clone(), |style| style.opacity(1.0))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(index) = this.plugin_tabs.iter().position(|t| t.id == tab) {
                        this.close_plugin_tab(index, window, cx);
                        cx.notify();
                    }
                })),
            ),
            Active::Project => Some(
                ui::icon_button(
                    SharedString::from(format!("{id}-close")),
                    Icon::X,
                    "Close tab",
                    ButtonSize::Tiny,
                )
                .absolute()
                .top(px(CLOSE_INSET))
                .right(px(CLOSE_INSET))
                .opacity(0.0)
                .group_hover(group.clone(), |style| style.opacity(1.0))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_project_page(window, cx);
                    cx.notify();
                })),
            ),
            Active::Issues => Some(
                ui::icon_button(
                    SharedString::from(format!("{id}-close")),
                    Icon::X,
                    "Close tab",
                    ButtonSize::Tiny,
                )
                .absolute()
                .top(px(CLOSE_INSET))
                .right(px(CLOSE_INSET))
                .opacity(0.0)
                .group_hover(group.clone(), |style| style.opacity(1.0))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_issues(window, cx);
                    cx.notify();
                })),
            ),
        };
        // Cmd-1 to Cmd-9 reach the first nine.
        let key = self
            .tabs()
            .iter()
            .position(|tab| *tab == active)
            .filter(|index| *index < 9)
            .map(|index| shortcut_label(&(index + 1).to_string(), false).into());
        // The desktop's tab: equal widths, and a raised pill for the one shown.
        div()
            .id(id)
            .group(group)
            .role(Role::Tab)
            .aria_label(label.clone())
            .tooltip(ui::tooltip(label.clone(), key))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_menu(MenuTarget::Tab(active), event.position, cx)
                }),
            )
            .aria_selected(selected)
            .track_focus(focus)
            .flex_1()
            .min_w(px(144.0))
            .h(px(28.0))
            .relative()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(10.0))
            // `pr-7`: the close button sits over this room, so the title never runs under it.
            .pr(px(28.0))
            .rounded(px(theme::RADIUS))
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
            .on_click(cx.listener(move |this, _, window, cx| {
                this.leave_loose(cx);
                this.show(active, window, cx)
            }))
            .child(glyph)
            .child(div().min_w_0().truncate().child(label))
            .children(splits)
            .children(close)
    }

    /// A plugin page's tab icon: the one its view names in the manifest, as the desktop's `PluginGlyph`.
    fn plugin_tab_glyph(&self, tab: usize) -> AnyElement {
        let page = self.plugin_tabs.iter().find(|page| page.id == tab);
        let installed = page.and_then(|page| self.plugins.iter().find(|p| p.id == page.plugin));
        let name = installed
            .zip(page)
            .and_then(|(installed, page)| {
                let views = &installed.plugin.manifest.contributes.views;
                views.iter().find(|view| view.id == page.view)
            })
            .map_or("", |view| view.icon.as_str());
        let own = installed.and_then(|installed| installed.plugin.icon_svg.as_deref());
        plugin_glyph(name, own, 14.0, theme::MUTED_FOREGROUND)
    }
}

/// The desktop's `LayoutGlyph`: a 16 by 12 map of a tab's splits after its title, the focused pane filled. Only for a
/// tab of more than one pane.
fn layout_glyph(tab: usize, layout: &Layout, focused: usize, color: u32) -> Option<AnyElement> {
    const W: f32 = 16.0;
    const H: f32 = 12.0;
    const GAP: f32 = 1.5;
    fn rects(node: &Layout, x: f32, y: f32, w: f32, h: f32, out: &mut Vec<(usize, [f32; 4])>) {
        match node {
            Layout::Pane(id) => out.push((*id, [x, y, w, h])),
            Layout::Split {
                dir: Dir::Row,
                ratio,
                a,
                b,
            } => {
                let first = (w - GAP) * ratio;
                rects(a, x, y, first, h, out);
                rects(b, x + first + GAP, y, w - first - GAP, h, out);
            }
            Layout::Split {
                dir: Dir::Column,
                ratio,
                a,
                b,
            } => {
                let first = (h - GAP) * ratio;
                rects(a, x, y, w, first, out);
                rects(b, x, y + first + GAP, w, h - first - GAP, out);
            }
        }
    }
    let mut boxes = Vec::new();
    rects(layout, 0.5, 0.5, W - 1.0, H - 1.0, &mut boxes);
    if boxes.len() < 2 {
        return None;
    }
    let count = boxes.len();
    Some(
        div()
            .id(SharedString::from(format!("layout-glyph-{tab}")))
            .role(Role::Image)
            .aria_label(format!("{count} panes"))
            .relative()
            .flex_none()
            .w(px(W))
            .h(px(H))
            .children(boxes.into_iter().map(|(id, [x, y, w, h])| {
                let (w, h) = (w.max(1.0), h.max(1.0));
                // An SVG stroke sits on the edge: half of it outside the box, so the outline is drawn half a point out.
                if id == focused {
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .w(px(w))
                        .h(px(h))
                        .rounded(px(1.5))
                        .bg(rgb(color))
                        .opacity(0.7)
                } else {
                    div()
                        .absolute()
                        .left(px(x - 0.5))
                        .top(px(y - 0.5))
                        .w(px(w + 1.0))
                        .h(px(h + 1.0))
                        .rounded(px(2.0))
                        .border_1()
                        .border_color(rgb(color))
                        .opacity(0.5)
                }
            }))
            .into_any_element(),
    )
}
