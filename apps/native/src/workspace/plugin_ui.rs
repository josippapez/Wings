//! What plugins draw in the window: badges in pane headers, the status strip under a pane's terminal, their sidebar pages
//! in a panel on the right, and the notifications the system couldn't show. These follow the desktop's
//! `pane-badge.tsx`, `pane-status.tsx` and `right-sidebar.tsx`.

use gpui::{
    Anchor, AnyElement, Bounds, BoxShadow, Context, Entity, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, Pixels, Render, Role, SharedString, Task, Window, anchored, canvas, deferred,
    div, hsla, point, prelude::*, px, relative, rgb, rgba, svg,
};

use crate::{
    icons::{Icon, icon},
    page::{
        PageView,
        plugin::ui::{Badge, CountIcon, PaneStatus, Row, StatusItem, Tone},
    },
    theme,
    ui::{self, ButtonSize, HAIRLINE, Variant},
};

use super::{Workspace, plugin_env::ACTION_TIMEOUT};

/// The right-hand sidebar's width: the desktop's starting width. It isn't resizable here yet.
pub(super) const RIGHT_WIDTH: f32 = 360.0;
/// The desktop's `--popover`, the card's colour.
const CARD: u32 = 0x201d23;
/// A pane's status strip: fixed, so an update never changes the terminal's rows. Only the strip appearing or going away
/// does (`STATUS_STRIP_HEIGHT`).
pub(super) const STRIP_HEIGHT: f32 = 22.0;

/// The pill, text, ring and dot colours of a tone: the desktop's `toneClass`.
pub(super) struct Palette {
    pill: u32,
    pub(super) text: u32,
    ring: u32,
    dot: u32,
}

pub(super) fn palette(tone: Tone) -> Palette {
    let (pill, text, ring, dot) = match tone {
        Tone::Neutral => (0xffffff0f, theme::MUTED_FOREGROUND, 0xffffff1a, theme::IDLE),
        Tone::Info => (0x7aa7f21f, 0x9cc0ff, 0x7aa7f240, 0x7aa7f2),
        Tone::Success => (0x7dd3a71f, theme::DONE, 0x7dd3a740, theme::DONE),
        Tone::Warning => (0xe9b65f1f, theme::WORKING, 0xe9b65f40, theme::WORKING),
        Tone::Danger => (0xf2786a1f, theme::BLOCKED, 0xf2786a4d, theme::BLOCKED),
        Tone::Merged => (0xb392f024, 0xc9b2f7, 0xb392f04d, 0xb392f0),
    };
    Palette {
        pill,
        text,
        ring,
        dot,
    }
}

/// A badge's card, open: which badge, and its buttons while one of them waits for the plugin.
pub(super) struct BadgeCard {
    pub(super) pane: usize,
    pub(super) plugin: String,
    /// The button that was clicked and hasn't been answered.
    pub(super) pending: Option<String>,
    /// What the plugin threw in answer to the last click.
    pub(super) error: Option<SharedString>,
    /// Where the pill is, so a click on it isn't also a click outside the card.
    pill: Option<Bounds<Pixels>>,
    /// Gives up on the plugin after `ACTION_TIMEOUT`.
    _timeout: Option<Task<()>>,
}

impl Workspace {
    /// A pane header's badges, by plugin id.
    pub(super) fn badge_pills(&self, pane: usize, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(badges) = self.plugin_ui.badges.get(&pane) else {
            return Vec::new();
        };
        badges
            .iter()
            .map(|(plugin, badge)| {
                let open = self
                    .badge_card
                    .as_ref()
                    .is_some_and(|card| card.pane == pane && card.plugin == *plugin);
                self.badge(pane, plugin, badge, open, cx)
            })
            .collect()
    }

    fn badge(
        &self,
        pane: usize,
        plugin: &str,
        badge: &Badge,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(badge.tone);
        let id = SharedString::from(format!("pane-badge-{pane}-{plugin}"));
        let owner = plugin.to_owned();
        let pill = div()
            .id(id)
            .role(Role::Button)
            .aria_label(badge.label.clone())
            .h(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(10.0))
            .rounded_full()
            .border_1()
            .border_color(rgba(colors.ring))
            .bg(rgba(colors.pill))
            .text_color(rgb(colors.text))
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .whitespace_nowrap()
            .cursor_pointer()
            .focusable()
            .tab_stop(true)
            .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
            .hover(|style| style.bg(rgba(colors.pill + 0x10)))
            .when(open, |pill| pill.bg(rgba(colors.pill + 0x10)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(
                cx.listener(move |this, _, _, cx| this.toggle_badge_card(pane, owner.clone(), cx)),
            )
            // The spinner: a ring, while the plugin refreshes.
            .when(badge.loading, |pill| {
                pill.child(
                    div()
                        .size(px(10.0))
                        .flex_none()
                        .rounded_full()
                        .border_2()
                        .border_color(rgba(colors.ring)),
                )
            })
            .child(
                div()
                    .max_w(px(176.0))
                    .min_w_0()
                    .truncate()
                    .child(badge.label.clone()),
            )
            .children(badge.counts.iter().map(|(kind, value)| {
                let glyph = match kind {
                    CountIcon::Changed => "±",
                    CountIcon::Push => "↑",
                    CountIcon::Pull => "↓",
                };
                div().flex_none().child(format!("{glyph}{value}"))
            }));
        let card = open.then(|| self.badge_card_element(pane, plugin, badge, cx));
        let measure = open.then(|| {
            let entity = cx.entity();
            canvas(
                move |bounds, _, cx| {
                    entity.update(cx, |this, _| {
                        if let Some(card) = &mut this.badge_card {
                            card.pill = Some(bounds);
                        }
                    })
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        });
        div()
            .relative()
            .flex_none()
            .child(pill)
            .children(measure)
            .children(card)
            .into_any_element()
    }

    /// The card under an open badge: its title and rows and its buttons, over everything else and right-aligned to the pill.
    fn badge_card_element(
        &self,
        pane: usize,
        plugin: &str,
        badge: &Badge,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.badge_card.as_ref();
        let pending = state.and_then(|card| card.pending.clone());
        let error = state.and_then(|card| card.error.clone());
        let busy = pending.is_some();
        let actions = badge.actions.iter().map(|action| {
            let (owner, action_id) = (plugin.to_owned(), action.id.clone());
            let waiting = pending.as_deref() == Some(action.id.as_str());
            let label = SharedString::from(action.label.clone());
            ui::button(
                SharedString::from(format!("badge-action-{pane}-{plugin}-{}", action.id)),
                label.clone(),
                if action.primary {
                    Variant::Default
                } else {
                    Variant::Secondary
                },
                busy,
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .text_size(px(12.0))
                    .when(waiting, |content| content.child("…"))
                    .child(label),
            )
            .when(!busy, |button| {
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.run_badge_action(pane, owner.clone(), action_id.clone(), cx)
                }))
            })
        });
        let card = div()
            .id(SharedString::from(format!("badge-card-{pane}-{plugin}")))
            .role(Role::Dialog)
            .aria_label(badge.title.clone().unwrap_or_else(|| badge.label.clone()))
            .w(px(320.0))
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .rounded(px(12.0))
            // The desktop's card is 95% over a blur; without the blur the text behind would show through, so it's solid.
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgba(HAIRLINE))
            .shadow(vec![BoxShadow {
                color: hsla(0.0, 0.0, 0.0, 0.4),
                offset: point(px(0.0), px(8.0)),
                blur_radius: px(24.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .text_size(px(12.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let on_pill = this
                    .badge_card
                    .as_ref()
                    .and_then(|card| card.pill)
                    .is_some_and(|pill| pill.contains(&event.position));
                if !on_pill {
                    this.badge_card = None;
                    cx.notify();
                }
            }))
            .when(badge.title.is_some() || badge.subtitle.is_some(), |card| {
                card.child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .children(badge.title.clone().map(|title| {
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(theme::FOREGROUND))
                                .child(title)
                        }))
                        .children(badge.subtitle.clone().map(|subtitle| {
                            div()
                                .truncate()
                                .text_color(rgb(theme::MUTED_FOREGROUND))
                                .child(subtitle)
                        })),
                )
            })
            .when(!badge.rows.is_empty(), |card| {
                card.child(rows_grid(&badge.rows, true))
            })
            .children(error.map(|error| {
                div()
                    .id(("badge-error", pane))
                    .role(Role::Alert)
                    .text_color(rgb(theme::BLOCKED))
                    .child(error)
            }))
            .when(!badge.actions.is_empty(), |card| {
                card.child(div().flex().flex_wrap().gap(px(6.0)).children(actions))
            });
        // Zero-size, at the pill's bottom right: the card hangs from it, right edge to right edge.
        div()
            .absolute()
            .top(px(32.0))
            .right_0()
            .w(px(0.0))
            .h(px(0.0))
            .child(deferred(
                anchored()
                    .anchor(Anchor::TopRight)
                    .snap_to_window_with_margin(px(8.0))
                    .child(card),
            ))
            .into_any_element()
    }

    pub(super) fn toggle_badge_card(
        &mut self,
        pane: usize,
        plugin: String,
        cx: &mut Context<Self>,
    ) {
        let same = self
            .badge_card
            .as_ref()
            .is_some_and(|card| card.pane == pane && card.plugin == plugin);
        self.badge_card = (!same).then_some(BadgeCard {
            pane,
            plugin,
            pending: None,
            error: None,
            pill: None,
            _timeout: None,
        });
        cx.notify();
    }

    /// A click on a badge's button: the plugin's main hears it with `onAction`, and the card shows the button busy until
    /// every listener has settled, or the error one threw.
    pub(super) fn run_badge_action(
        &mut self,
        pane: usize,
        plugin: String,
        action: String,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self
            .plugins
            .iter()
            .find(|installed| installed.id == plugin)
            .map(|installed| installed.host.clone())
        else {
            return;
        };
        let (answer, answered) = async_channel::bounded(1);
        let give_up = answer.clone();
        let timeout = cx.spawn(async move |_, cx| {
            cx.background_executor().timer(ACTION_TIMEOUT).await;
            let _ = give_up.try_send(Err("The plugin didn't respond".to_owned()));
        });
        if let Some(card) = &mut self.badge_card {
            card.pending = Some(action.clone());
            card.error = None;
            card._timeout = Some(timeout);
        }
        cx.notify();
        host.send_action(&pane.to_string(), &action, move |result| {
            let _ = answer.try_send(result);
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = answered.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(card) = this
                    .badge_card
                    .as_mut()
                    .filter(|card| card.pane == pane && card.plugin == plugin)
                {
                    card.pending = None;
                    card._timeout = None;
                    card.error = result.err().map(Into::into);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The strip under a pane's terminal, if a plugin shows something there.
    pub(super) fn status_strip(&self, pane: usize, cx: &mut Context<Self>) -> Option<AnyElement> {
        let statuses = self.plugin_ui.statuses.get(&pane)?;
        let groups: Vec<AnyElement> = statuses
            .iter()
            .enumerate()
            .flat_map(|(index, (plugin, status))| {
                let dot = (index > 0).then(|| {
                    div()
                        .size(px(3.0))
                        .flex_none()
                        .rounded_full()
                        .bg(rgba(0xffffff33))
                        .into_any_element()
                });
                dot.into_iter()
                    .chain([self.status_group(pane, plugin, status, cx)])
            })
            .collect();
        Some(
            div()
                .id(("pane-status", pane))
                .role(Role::Group)
                .aria_label("Status")
                .h(px(STRIP_HEIGHT))
                .flex_none()
                .flex()
                .flex_wrap()
                .content_start()
                .items_center()
                .gap_x(px(12.0))
                .overflow_hidden()
                .border_t_1()
                .border_color(rgba(HAIRLINE))
                .pl(px(14.0))
                .pr(px(12.0))
                .text_size(px(11.0))
                .children(groups)
                .into_any_element(),
        )
    }

    /// One plugin's items: a button when the status names a page to `open`. Pointing at it shows its `rows`.
    fn status_group(
        &self,
        pane: usize,
        plugin: &str,
        status: &PaneStatus,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (owner, page) = (plugin.to_owned(), status.open.clone());
        let label = status
            .items
            .iter()
            .map(|item| match &item.label {
                Some(label) => format!("{label} {}", item.value),
                None => item.value.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let rows = status.rows.clone();
        div()
            .id(SharedString::from(format!("pane-status-{pane}-{plugin}")))
            .role(if page.is_some() {
                Role::Button
            } else {
                Role::Group
            })
            .aria_label(label)
            .h(px(STRIP_HEIGHT))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(12.0))
            .overflow_hidden()
            .whitespace_nowrap()
            .when(page.is_some(), |group| {
                group
                    .cursor_pointer()
                    .focusable()
                    .tab_stop(true)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(page) = &page {
                            this.open_plugin_page(&owner, page, window, cx);
                        }
                    }))
            })
            .when(!rows.is_empty(), |group| {
                group.tooltip(move |_, cx| cx.new(|_| RowsTip(rows.clone())).into())
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(status.items.iter().map(status_item))
            .into_any_element()
    }

    /// Shows a page of a plugin: its sidebar in the panel on the right, or its view as a tab, as a status strip's click
    /// does on the desktop (`openStatusTarget`).
    pub(super) fn open_plugin_page(
        &mut self,
        plugin: &str,
        page: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_sidebar = self
            .plugins
            .iter()
            .filter(|installed| installed.id == plugin)
            .any(|installed| installed.sidebars.iter().any(|sidebar| sidebar.id == page));
        if is_sidebar {
            self.show_sidebar(plugin, page, cx);
        } else {
            self.open_plugin_view(plugin, page, None, window, cx);
        }
    }

    /// Opens the plugin's sidebar in the panel on the right, starting its page the first time. The page keeps running
    /// while the panel is closed or shows another, as the desktop's does.
    pub(super) fn show_sidebar(&mut self, plugin: &str, sidebar: &str, cx: &mut Context<Self>) {
        let key = (plugin.to_owned(), sidebar.to_owned());
        if !self.sidebar_pages.contains_key(&key) {
            let Some(installed) = self.plugins.iter().find(|installed| installed.id == plugin)
            else {
                return;
            };
            let Some(page) = installed.sidebars.iter().find(|page| page.id == sidebar) else {
                return;
            };
            let (host, source) = (installed.host.clone(), page.source.clone());
            let page = cx.new(|cx| PageView::in_plugin(&host, &source, cx));
            self.sidebar_pages.insert(key.clone(), page);
        }
        self.right = Some(key);
        self.sync_plugin_visibility(cx);
        cx.notify();
    }

    /// The title bar button of a sidebar: open, it closes the panel.
    fn toggle_sidebar(&mut self, plugin: &str, sidebar: &str, cx: &mut Context<Self>) {
        let shown = self
            .right
            .as_ref()
            .is_some_and(|(open, page)| open == plugin && page == sidebar);
        if shown {
            self.right = None;
            self.sync_plugin_visibility(cx);
            cx.notify();
        } else {
            self.show_sidebar(plugin, sidebar, cx);
        }
    }

    /// The plugins' sidebar buttons, at the project bar's right end: each opens its page in the panel on the right.
    pub(super) fn sidebar_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.plugins
            .iter()
            .flat_map(|installed| {
                installed
                    .sidebars
                    .iter()
                    .map(move |sidebar| (installed, sidebar))
            })
            .map(|(installed, sidebar)| {
                let (plugin, page) = (installed.id.clone(), sidebar.id.clone());
                let open = self
                    .right
                    .as_ref()
                    .is_some_and(|(open, shown)| *open == plugin && *shown == page);
                let mark = match &installed.plugin.icon_svg {
                    Some(svg_text) => svg()
                        .data(svg_text.as_bytes())
                        .size(px(16.0))
                        .flex_none()
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .into_any_element(),
                    None => icon(Icon::Puzzle, 16.0, theme::MUTED_FOREGROUND).into_any_element(),
                };
                ui::ghost_button(
                    SharedString::from(format!("plugin-sidebar-{plugin}-{page}")),
                    sidebar.title.clone(),
                    None,
                    ButtonSize::Sm,
                    Some(open),
                    mark,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.toggle_sidebar(&plugin, &page, cx)),
                )
                .into_any_element()
            })
            .collect()
    }

    /// The panel on the right, with the open sidebar's page, or nothing while none is open.
    pub(super) fn right_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (plugin, sidebar) = self.right.as_ref()?;
        let page: Entity<PageView> = self
            .sidebar_pages
            .get(&(plugin.clone(), sidebar.clone()))?
            .clone();
        let installed = self
            .plugins
            .iter()
            .find(|installed| installed.id == *plugin)?;
        let title = installed
            .sidebars
            .iter()
            .find(|page| page.id == *sidebar)
            .map(|page| page.title.clone())?;
        Some(
            div()
                .id("right-sidebar")
                .role(Role::Complementary)
                .aria_label(title.clone())
                // 360 as the desktop starts it, but never more than 40% of the window, so a narrow one keeps room for the panes.
                .w(px(RIGHT_WIDTH))
                .min_w(px(250.0))
                .max_w(relative(0.4))
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                .pb(px(8.0))
                .pl(px(4.0))
                .pr(px(8.0))
                .child(
                    div()
                        .h(px(32.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .pl(px(4.0))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .pl(px(8.0))
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(theme::MUTED_FOREGROUND))
                                .child(title),
                        )
                        .child(
                            ui::icon_button(
                                "close-right-sidebar",
                                Icon::X,
                                "Close sidebar",
                                ButtonSize::Xs,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.right = None;
                                this.sync_plugin_visibility(cx);
                                cx.notify();
                            })),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .rounded(px(14.0))
                        .border_1()
                        .border_color(rgba(HAIRLINE))
                        .bg(rgb(theme::SURFACE))
                        .child(page),
                )
                .into_any_element(),
        )
    }

    /// Notifications the system couldn't show, at the bottom right. A click puts one away.
    pub(super) fn notices_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.plugin_ui.notices.is_empty() {
            return None;
        }
        Some(
            div()
                .absolute()
                .right(px(16.0))
                .bottom(px(16.0))
                .w(px(300.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .children(self.plugin_ui.notices.iter().map(|notice| {
                    let id = notice.id;
                    div()
                        .id(("notice", id))
                        .role(Role::Alert)
                        .aria_label(notice.title.clone())
                        .p(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .rounded(px(12.0))
                        .border_1()
                        .border_color(rgba(theme::HAIRLINE_STRONG))
                        .bg(rgba(theme::POPOVER))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| this.dismiss_notice(id, cx)))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(notice.title.clone()),
                        )
                        .when(!notice.body.is_empty(), |card| {
                            card.child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .child(notice.body.clone()),
                            )
                        })
                }))
                .into_any_element(),
        )
    }
}

/// One figure of a status strip: its label, its meter and its value.
fn status_item(item: &StatusItem) -> AnyElement {
    let tone = item.tone.unwrap_or(Tone::Neutral);
    let colors = palette(tone);
    div()
        .h(px(STRIP_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.0))
        .whitespace_nowrap()
        .children(
            item.label
                .clone()
                .map(|label| div().text_color(rgb(theme::MUTED_FOREGROUND)).child(label)),
        )
        .children(item.percent.map(|percent| {
            div()
                .w(px(40.0))
                .h(px(3.0))
                .flex_none()
                .overflow_hidden()
                .rounded_full()
                .bg(rgba(0xffffff1a))
                .child(
                    div()
                        .h_full()
                        .w(relative((percent / 100.0) as f32))
                        .rounded_full()
                        .bg(rgb(colors.dot)),
                )
        }))
        .child(
            div()
                .text_color(rgb(match item.tone {
                    Some(tone) if tone != Tone::Neutral => colors.text,
                    _ => theme::FOREGROUND,
                }))
                .child(item.value.clone()),
        )
        .into_any_element()
}

/// Label and value pairs, a column each: a badge card's rows, and the card a status strip shows on hover.
fn rows_grid(rows: &[Row], tinted: bool) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .when(tinted, |grid| {
            grid.p(px(10.0)).rounded(px(8.0)).bg(rgba(0xffffff08))
        })
        .children(rows.iter().map(|row| {
            let colors = row.tone.map(palette);
            div()
                .flex()
                .gap(px(12.0))
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(row.label.clone()),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .children(colors.as_ref().map(|colors| {
                            div()
                                .size(px(6.0))
                                .flex_none()
                                .rounded_full()
                                .bg(rgb(colors.dot))
                        }))
                        .text_color(rgb(colors.as_ref().map_or(theme::FOREGROUND, |c| c.text)))
                        .child(div().truncate().child(row.value.clone())),
                )
        }))
        .into_any_element()
}

/// The card a status strip shows when you point at it.
struct RowsTip(Vec<Row>);

impl Render for RowsTip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(288.0))
            .p(px(12.0))
            .rounded(px(12.0))
            .border_1()
            .border_color(rgba(HAIRLINE))
            .bg(rgb(CARD))
            .text_size(px(12.0))
            .text_color(rgb(theme::FOREGROUND))
            .child(rows_grid(&self.0, false))
    }
}
