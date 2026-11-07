//! What plugins draw in the window: badges in pane headers, the status strip under a pane's terminal, their sidebar pages
//! in a panel on the right, and the notifications the system couldn't show. These follow the desktop's
//! `pane-badge.tsx`, `pane-status.tsx` and `right-sidebar.tsx`.

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

use gpui::{
    Anchor, AnyElement, Bounds, BoxShadow, Context, ElementId, Entity, FontWeight, IntoElement,
    MouseButton, MouseDownEvent, Pixels, Render, Role, SharedString, Task, Window, anchored,
    canvas, deferred, div, hsla, point, prelude::*, px, relative, rgb, rgba, svg,
};

use crate::{
    icons::{Icon, icon},
    page::{
        PageView,
        plugin::ui::{Badge, CountIcon, PaneStatus, Row, StatusItem, Tone},
    },
    theme,
    ui::{self, ButtonSize, HAIRLINE, Variant, motion},
};

use super::{Workspace, plugin_env::ACTION_TIMEOUT};

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
            // Not over its own open sidebar: closed rather than hidden, so it doesn't come back when the sidebar closes.
            let key = (plugin.to_owned(), sidebar.to_owned());
            if self.plugin_ui.popover.open.as_ref() == Some(&key) {
                self.hide_popover(cx);
            }
            self.plugin_ui.popover.hovered = None;
        }
    }

    /// The plugins' sidebar buttons, at the project bar's right end: each opens its page in the panel on the right. A
    /// label the plugin set (`setSidebarLabel`) follows the icon behind a tone dot, as the desktop's `ProjectBar` draws
    /// it, and its rows show when you point at the button. A sidebar with a popover page shows that instead, under the
    /// button, while you point at it (the desktop's `WithPopover`).
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
                let key = (plugin.clone(), page.clone());
                let open = self.right.as_ref() == Some(&key);
                let mark = match &installed.plugin.icon_svg {
                    Some(svg_text) => svg()
                        .data(svg_text.as_bytes())
                        .size(px(16.0))
                        .flex_none()
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .into_any_element(),
                    None => icon(Icon::Puzzle, 16.0, theme::MUTED_FOREGROUND).into_any_element(),
                };
                let label = self.plugin_ui.labels.get(&key);
                let name = match label {
                    Some(label) => format!("{}, {}", sidebar.title, label.text),
                    None => sidebar.title.clone(),
                };
                let has_popover = installed
                    .plugin
                    .manifest
                    .contributes
                    .sidebars
                    .iter()
                    .any(|declared| declared.id == page && declared.popover.is_some())
                    && !self.plugin_ui.popover.missing.contains(&key);
                let content = div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(mark)
                    .when_some(label, |content, label| {
                        content.child(ui::dot(tone_dot(label.tone))).child(
                            div()
                                .max_w(px(200.0))
                                .truncate()
                                .text_size(px(12.0))
                                .child(label.text.clone()),
                        )
                    });
                let button = ui::untipped_ghost_button(
                    SharedString::from(format!("plugin-sidebar-{plugin}-{page}")),
                    name.clone(),
                    ButtonSize::Sm,
                    Some(open),
                    content,
                )
                .when(label.is_some(), |button| button.w_auto().px(px(8.0)));
                // The popover shows more than a tooltip would.
                let button = if has_popover {
                    let key = key.clone();
                    button.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        this.hover_popover(key.clone(), false, *hovered, cx)
                    }))
                } else if let Some(label) = label.filter(|label| !label.rows.is_empty()) {
                    let tip = LabelTip {
                        title: sidebar.title.clone(),
                        rows: label.rows.clone(),
                    };
                    button.tooltip(move |_, cx| cx.new(|_| tip.clone()).into())
                } else {
                    button.tooltip(ui::tooltip(name, None))
                };
                let button = button.on_click(
                    cx.listener(move |this, _, _, cx| this.toggle_sidebar(&plugin, &page, cx)),
                );
                let card = has_popover.then(|| self.popover_card(&key, cx)).flatten();
                div()
                    .relative()
                    .flex_none()
                    .child(button)
                    .children(card)
                    .into_any_element()
            })
            .collect()
    }

    /// The popover page of the sidebar `key` under its button, while it is shown or on its way out.
    fn popover_card(&self, key: &(String, String), cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.plugin_ui.popover.open.as_ref() == Some(key);
        let phase = self.popover_phase(format!("plugin-popover-{}-{}", key.0, key.1), shown, cx)?;
        let (page, (width, height)) = self.plugin_ui.popover.pages.get(key)?;
        let hovered = key.clone();
        let card = div()
            .id(SharedString::from(format!(
                "plugin-popover-{}-{}",
                key.0, key.1
            )))
            .role(Role::Dialog)
            .w(px(*width as f32))
            .h(px(*height as f32))
            .overflow_hidden()
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(rgba(0xece9e41a))
            .bg(rgb(ui::DIALOG))
            // shadcn's `shadow-md`.
            .shadow(vec![
                BoxShadow {
                    color: hsla(0.0, 0.0, 0.0, 0.1),
                    offset: point(px(0.0), px(4.0)),
                    blur_radius: px(6.0),
                    spread_radius: px(-1.0),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0.0, 0.0, 0.0, 0.1),
                    offset: point(px(0.0), px(2.0)),
                    blur_radius: px(4.0),
                    spread_radius: px(-2.0),
                    inset: false,
                },
            ])
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                this.hover_popover(hovered.clone(), true, *over, cx)
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(page.clone());
        Some(motion::with_phase(phase, || {
            // Zero-size, at the button's bottom right: the popover hangs 6pt under it, right edge to right edge, as the
            // desktop's `align="end" sideOffset={6}`.
            div()
                .absolute()
                .top(px(28.0 + 6.0))
                .right_0()
                .w(px(0.0))
                .h(px(0.0))
                .child(deferred(
                    anchored()
                        .anchor(Anchor::TopRight)
                        .snap_to_window_with_margin(px(8.0))
                        .child(motion::popup(
                            "plugin-popover",
                            motion::From::Above,
                            motion::POPUP_IN,
                            card,
                        )),
                ))
                .into_any_element()
        }))
    }

    /// The pointer came onto (`hovered`) or left the button of the sidebar `key`, or its popover (`card`). It shows
    /// after `POPOVER_OPEN_DELAY` over either and goes `POPOVER_CLOSE_DELAY` after leaving both, as Base UI's
    /// `HoverCardTrigger delay={300} closeDelay={200}`; never over its own open sidebar, which shows the same and more.
    pub(super) fn hover_popover(
        &mut self,
        key: (String, String),
        card: bool,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let popover = &mut self.plugin_ui.popover;
        if hovered && popover.hovered.as_ref() != Some(&key) {
            popover.hovered = Some(key.clone());
            popover.over = [false; 2];
        }
        if popover.hovered.as_ref() == Some(&key) {
            popover.over[card as usize] = hovered;
        }
        let over = popover.hovered.as_ref() == Some(&key) && popover.over.contains(&true);
        let shown = popover.open.as_ref() == Some(&key);
        if over && (shown || self.right.as_ref() == Some(&key)) {
            popover.timer = None;
        } else if over {
            popover.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(POPOVER_OPEN_DELAY).await;
                let _ = this.update(cx, |this, cx| this.show_popover(key, cx));
            }));
        } else if shown {
            popover.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(POPOVER_CLOSE_DELAY).await;
                let _ = this.update(cx, |this, cx| this.hide_popover(cx));
            }));
        } else {
            popover.timer = None;
        }
    }

    /// Shows the popover page of the sidebar `key`, loading it the first time or after it was unloaded.
    pub(super) fn show_popover(&mut self, key: (String, String), cx: &mut Context<Self>) {
        if self.right.as_ref() == Some(&key) {
            return;
        }
        if !self.plugin_ui.popover.pages.contains_key(&key) {
            let Some(installed) = self.plugins.iter().find(|installed| installed.id == key.0)
            else {
                return;
            };
            let Some((source, width, height)) = crate::installed::popover(installed, &key.1) else {
                // Declared, with no native build: the button keeps its tooltip.
                self.plugin_ui.popover.missing.insert(key);
                cx.notify();
                return;
            };
            let host = installed.host.clone();
            let page = cx.new(|cx| PageView::in_plugin_popover(&host, &source, cx));
            self.plugin_ui
                .popover
                .pages
                .insert(key.clone(), (page, (width, height)));
        }
        if self.plugin_ui.popover.open.as_ref() != Some(&key) {
            self.hide_popover(cx);
        }
        let popover = &mut self.plugin_ui.popover;
        popover.unload.remove(&key);
        if let Some((page, _)) = popover.pages.get(&key) {
            page.read(cx).set_visible(true);
        }
        popover.open = Some(key);
        cx.notify();
    }

    /// Hides the popover that is shown. Its page stays, out of view, for `POPOVER_UNLOAD` so it shows again at once, and
    /// is then dropped: the next show loads it again.
    pub(super) fn hide_popover(&mut self, cx: &mut Context<Self>) {
        let popover = &mut self.plugin_ui.popover;
        popover.timer = None;
        let Some(key) = popover.open.take() else {
            return;
        };
        if let Some((page, _)) = popover.pages.get(&key) {
            page.read(cx).set_visible(false);
        }
        let unloaded = key.clone();
        popover.unload.insert(
            key,
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(POPOVER_UNLOAD).await;
                let _ = this.update(cx, |this, _| {
                    let popover = &mut this.plugin_ui.popover;
                    if popover.open.as_ref() != Some(&unloaded) {
                        popover.pages.remove(&unloaded);
                    }
                    popover.unload.remove(&unloaded);
                });
            }),
        );
        cx.notify();
    }

    /// The panel on the right, with the open sidebar's page, or nothing while none is open. Opening or closing it
    /// slides it, as the desktop's right panel does, its page narrowing with it down to its 250px and clipped below;
    /// moving from one sidebar to another doesn't.
    pub(super) fn right_panel(
        &mut self,
        viewport: Pixels,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let now = cx.background_executor().now();
        let open = self.right.is_some();
        let reduced = motion::reduced(cx);
        let slide = &mut self.plugin_ui.right_slide;
        if self.right.is_some() {
            slide.shown = self.right.clone();
        }
        match slide.open {
            // The first placement, as the window opens, doesn't slide.
            None => slide.open = Some(open),
            Some(was) if was != open => {
                slide.open = Some(open);
                let duration = if reduced {
                    Duration::ZERO
                } else {
                    motion::SLIDE
                };
                let from = slide
                    .share
                    .unwrap_or(motion::Transition::at(if was { 1.0 } else { 0.0 }, now));
                let share = from.retarget(
                    if open { 1.0 } else { 0.0 },
                    duration,
                    motion::EASE_SLIDE,
                    now,
                );
                slide.share = Some(share);
                slide.count += 1;
                let count = slide.count;
                slide.timer = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(share.duration()).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.plugin_ui.right_slide.count == count {
                            this.plugin_ui.right_slide.share = None;
                            cx.notify();
                        }
                    });
                }));
            }
            Some(_) => {}
        }
        let share = slide.share;
        if !open && share.is_none() {
            slide.shown = None;
        }
        let count = slide.count;
        let (plugin, sidebar) = slide.shown.as_ref()?;
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
        let width = self.right_width(viewport);
        let panel = div()
            .id("right-sidebar")
            .role(Role::Complementary)
            .aria_label(title.clone())
            .relative()
            .min_w(px(super::right_edge::MIN_WIDTH))
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
                    // The page pads itself, as the desktop's `.w-shell` does, 14px in from the card's edge.
                    .child(page),
            );
        let panel = panel.child(self.right_resize_edge(cx));
        let Some(share) = share else {
            return Some(panel.w(px(width)).into_any_element());
        };
        let clip = div().flex_none().h_full().overflow_hidden();
        Some(
            motion::tween(
                clip,
                ElementId::NamedInteger("right-slide".into(), count),
                share.duration(),
                move |clip, t, _| {
                    let share = share.value_after(share.duration().mul_f32(t));
                    clip.w(px(width * share)).child(panel.w_full())
                },
            )
            .into_any_element(),
        )
    }

    /// Notifications the system couldn't show, at the bottom right. A click puts one away.
    pub(super) fn notices_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.plugin_ui.notices.is_empty() && self.manage.announcements.is_empty() {
            return None;
        }
        Some(
            div()
                .absolute()
                .right(px(16.0))
                .bottom(px(16.0))
                .w(px(320.0))
                .max_h(gpui::relative(0.9))
                .id("notification-stack")
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .children(self.plugin_update_notices(cx))
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

/// The panel on the right sliding open or shut.
#[derive(Default)]
pub(super) struct RightSlide {
    /// Whether it was open in the last render, from the first one on.
    open: Option<bool>,
    /// The sidebar it shows, kept while it slides shut.
    shown: Option<(String, String)>,
    /// The share of its width it takes, while it slides.
    share: Option<motion::Transition>,
    /// Tells one slide from the next, so each starts over.
    count: u64,
    timer: Option<Task<()>>,
}

/// How long the pointer rests on a sidebar's button before its popover shows, and how long after it leaves the button
/// and the popover before it goes: the desktop's `HoverCardTrigger delay={300} closeDelay={200}`.
const POPOVER_OPEN_DELAY: Duration = Duration::from_millis(300);
const POPOVER_CLOSE_DELAY: Duration = Duration::from_millis(200);
/// How long a hidden popover's page is kept before it is dropped: the desktop's `POPOVER_UNLOAD_MS`.
const POPOVER_UNLOAD: Duration = Duration::from_secs(120);

/// A popover's page and its size in points.
pub(super) type PopoverPage = (Entity<PageView>, (u32, u32));

/// The sidebars' popover pages (`contributes.sidebars[].popover`), by plugin and sidebar id.
#[derive(Default)]
pub(super) struct HoverPopover {
    /// The one shown.
    pub(super) open: Option<(String, String)>,
    /// The pages loaded, shown or kept hidden until they unload, with their size in points.
    pub(super) pages: HashMap<(String, String), PopoverPage>,
    /// Popovers a manifest declares that have no native build.
    missing: HashSet<(String, String)>,
    /// The sidebar the pointer was last on, and whether it is over its button and over its popover.
    hovered: Option<(String, String)>,
    over: [bool; 2],
    /// Shows or hides it after its delay.
    timer: Option<Task<()>>,
    unload: HashMap<(String, String), Task<()>>,
}

impl HoverPopover {
    /// Drops what a plugin that stopped showed.
    pub(super) fn forget(&mut self, plugin: &str) {
        if self.open.as_ref().is_some_and(|(open, _)| open == plugin) {
            self.open = None;
            self.timer = None;
        }
        self.pages.retain(|(owner, _), _| owner != plugin);
        self.unload.retain(|(owner, _), _| owner != plugin);
        self.missing.retain(|(owner, _)| owner != plugin);
    }
}

/// The dot before a sidebar's label: green with no tone, amber or red when the plugin needs attention (the desktop's
/// `ToneDot`).
pub(super) fn tone_dot(tone: Option<Tone>) -> u32 {
    match tone {
        Some(Tone::Warning) => theme::WORKING,
        Some(Tone::Danger) => theme::BLOCKED,
        _ => theme::DONE,
    }
}

/// What a sidebar's button shows when you point at it while its plugin set a label with rows: the title over them.
#[derive(Clone)]
struct LabelTip {
    title: String,
    rows: Vec<(String, String)>,
}

impl Render for LabelTip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (title, rows) = (self.title.clone(), self.rows.clone());
        ui::tip(
            motion::TOOLTIP_IN,
            Rc::new(move || {
                // The desktop's `<dl>` in the tooltip: labels at 60%, values cut to the width.
                let grid = rows.iter().map(|(label, value)| {
                    div()
                        .flex()
                        .gap(px(12.0))
                        .child(
                            div()
                                .flex_none()
                                .text_color(rgba(theme::BACKGROUND << 8 | 0x99))
                                .child(label.clone()),
                        )
                        .child(div().min_w_0().truncate().child(value.clone()))
                });
                div()
                    .max_w(px(320.0))
                    .px(px(12.0))
                    .py(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .rounded(px(6.0))
                    .bg(rgb(theme::FOREGROUND))
                    .text_size(px(12.0))
                    .text_color(rgb(theme::BACKGROUND))
                    .child(div().font_weight(FontWeight::MEDIUM).child(title.clone()))
                    .child(div().flex().flex_col().gap(px(2.0)).children(grid))
            }),
        )
    }
}

/// The card a status strip shows when you point at it.
struct RowsTip(Vec<Row>);

impl Render for RowsTip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let rows = self.0.clone();
        // The desktop's `HoverCard`: in and out over `duration-100`, as tooltips move.
        ui::tip(
            motion::POPUP_IN,
            Rc::new(move || {
                div()
                    .w(px(288.0))
                    .p(px(12.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(rgba(HAIRLINE))
                    .bg(rgb(CARD))
                    .text_size(px(12.0))
                    .text_color(rgb(theme::FOREGROUND))
                    .child(rows_grid(&rows, false))
            }),
        )
    }
}

/// For the screenshots: a plugin's sidebar and popover shown, and their pages.
#[cfg(feature = "test-support")]
impl Workspace {
    pub fn shot_sidebar(&mut self, plugin: &str, sidebar: &str, cx: &mut Context<Self>) {
        self.show_sidebar(plugin, sidebar, cx);
    }

    pub fn shot_popover(&mut self, plugin: &str, sidebar: &str, cx: &mut Context<Self>) {
        self.right = None;
        self.sync_plugin_visibility(cx);
        self.show_popover((plugin.to_owned(), sidebar.to_owned()), cx);
    }

    pub fn sidebar_page(&self, plugin: &str, sidebar: &str) -> Option<Entity<PageView>> {
        self.sidebar_pages
            .get(&(plugin.to_owned(), sidebar.to_owned()))
            .cloned()
    }

    pub fn popover_page(&self, plugin: &str, sidebar: &str) -> Option<Entity<PageView>> {
        let key = (plugin.to_owned(), sidebar.to_owned());
        self.plugin_ui
            .popover
            .pages
            .get(&key)
            .map(|(page, _)| page.clone())
    }
}
