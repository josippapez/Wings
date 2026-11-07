//! The rows at the top of the sidebar, as the desktop's `nav aria-label="Wings"`: Search, Terminal, the Inbox and Past
//! sessions, then the terminals the Terminal row opened, each filling the pane area as a page of its own.

use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, Role, SharedString, Stateful, div, prelude::*,
    px, rgb, rgba,
};

use crate::{
    actions::TogglePalette,
    icons::{Icon, icon},
    theme,
    ui::{self, ButtonSize},
};

use crate::workspace::{Workspace, palette::shortcut_label, project::SIDEBAR_ACCENT, quick::Place};

/// A nav row's height, and the 1px between rows: the desktop's `h-7` in a `gap-px` column.
pub(in crate::workspace) const NAV_ROW: f32 = 28.0;
pub(in crate::workspace) const NAV_GAP: f32 = 1.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::workspace) enum Look {
    Plain,
    /// The page on screen: the desktop's `bg-sidebar-accent font-semibold text-foreground`.
    Current,
    /// At half strength, taking no clicks.
    Disabled,
}

/// The desktop's `SidebarNavRow`: a 16px icon and 13px medium text, muted until the pointer is on it.
pub(in crate::workspace) fn nav_row(
    id: impl Into<ElementId>,
    glyph: Icon,
    label: impl Into<SharedString>,
    look: Look,
) -> Stateful<Div> {
    let label = label.into();
    let row = div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .h(px(NAV_ROW))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(8.0))
        .rounded(px(theme::RADIUS))
        .text_size(px(13.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .child(icon(glyph, 16.0, theme::MUTED_FOREGROUND))
        .child(div().flex_1().min_w_0().truncate().child(label));
    match look {
        Look::Plain => row.cursor_pointer().hover(|style| {
            style
                .bg(rgba(theme::HOVER))
                .text_color(rgb(theme::FOREGROUND))
        }),
        Look::Current => row
            .cursor_pointer()
            .bg(rgba(SIDEBAR_ACCENT))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(theme::FOREGROUND)),
        Look::Disabled => row.opacity(0.5).a11y_synthetic_children(|builder| {
            builder.parent_node().set_disabled();
        }),
    }
}

/// The desktop's `RowHint`: a shortcut at the row's right end, plain text at 60%, not a keycap.
pub(in crate::workspace) fn row_hint(text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .text_size(px(11.0))
        .font_weight(FontWeight::NORMAL)
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .opacity(0.6)
        .child(text.into())
}

impl Workspace {
    /// The desktop's `nav px-2 pb-2`, and the Terminals list under it while any is open.
    pub(super) fn sidebar_nav(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let no_project = self.welcome;
        let nav = div()
            .id("wings-nav")
            .role(Role::Navigation)
            .aria_label("Wings")
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(NAV_GAP))
            .px(px(8.0))
            .pb(px(8.0))
            .child(
                nav_row("nav-search", Icon::Search, "Search", Look::Plain)
                    .children(shortcut_label(&TogglePalette, cx).map(row_hint))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_palette(window, cx))),
            )
            .child(
                nav_row(
                    "nav-terminal",
                    Icon::SquareTerminal,
                    "Terminal",
                    Look::Plain,
                )
                .child(icon(Icon::Plus, 14.0, theme::MUTED_FOREGROUND).opacity(0.6))
                .on_click(
                    cx.listener(|this, _, window, cx| this.open_loose(Place::Full, window, cx)),
                ),
            )
            .child(self.inbox_row(cx))
            .child(
                nav_row(
                    "nav-history",
                    Icon::RotateCcwClock,
                    "Past sessions",
                    if no_project {
                        Look::Disabled
                    } else {
                        Look::Plain
                    },
                )
                .when(!no_project, |row| {
                    row.on_click(cx.listener(|this, _, window, cx| this.open_history(window, cx)))
                }),
            )
            .into_any_element();
        let mut column = vec![nav];
        if !self.loose.is_empty() {
            column.push(self.loose_list(cx));
        }
        column
    }

    /// The desktop's `ul aria-label="Terminals"`: each sidebar terminal, the one on screen highlighted, with a button
    /// that ends its shell, shown where the pointer or the keyboard focus is.
    fn loose_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let page = self.loose_page();
        div()
            .id("loose-terminals")
            .role(Role::List)
            .aria_label("Terminals")
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(NAV_GAP))
            .px(px(8.0))
            .pb(px(8.0))
            .children(self.loose.iter().map(|quick| {
                let key = quick.key;
                let active = page == Some(key);
                let title = quick.title(cx);
                let tip = if quick.place == Place::Floating {
                    format!("{title} · floating")
                } else {
                    title.clone()
                };
                let group = SharedString::from(format!("loose-row-{key}"));
                div()
                    .id(("loose-item", key))
                    .role(Role::ListItem)
                    .relative()
                    .group(group.clone())
                    .track_focus(&quick.row)
                    .child(
                        nav_row(
                            ("loose", key),
                            Icon::SquareTerminal,
                            title,
                            if active { Look::Current } else { Look::Plain },
                        )
                        .aria_selected(active)
                        .pr(px(32.0))
                        .focusable()
                        .tab_stop(true)
                        .focus_visible(|style| {
                            style
                                .bg(rgba(theme::HOVER))
                                .text_color(rgb(theme::FOREGROUND))
                        })
                        .tooltip(ui::tooltip(tip, None))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.place_quick(key, Place::Full, false, window, cx)
                            },
                        )),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right(px(4.0))
                            .flex()
                            .items_center()
                            .opacity(if quick.row_focused { 1.0 } else { 0.0 })
                            .group_hover(group, |style| style.opacity(1.0))
                            .child(
                                ui::icon_button(
                                    ("close-loose", key),
                                    Icon::X,
                                    "Close and end the shell",
                                    ButtonSize::Tiny,
                                )
                                .focusable()
                                .tab_stop(true)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| this.close_quick(key, window, cx),
                                )),
                            ),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }
}
