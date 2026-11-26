//! What plugins draw in the window: badges in pane headers, the status strip under a pane's terminal, their sidebar pages
//! in a panel on the right, and the notifications the system couldn't show. These follow the desktop's
//! `pane-badge.tsx`, `pane-status.tsx` and `right-sidebar.tsx`.

use crate::workspace::NotifyWorkspace;
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

use gpui::{
    AnyElement, Bounds, BoxShadow, Context, ElementId, Entity, FocusHandle, FontWeight,
    ClickEvent, HoverListenerMode, IntoElement, MouseButton, MouseDownEvent, Pixels, Render, Role,
    SharedString, Subscription, Task, Window, canvas, div,
    hsla, point, prelude::*, px, relative, rgb, rgba,
};

use crate::{
    icons::{Icon, plugin_glyph},
    installed::Installed,
    page::{
        PageView,
        plugin::ui::{Badge, CountIcon, PaneStatus, Row, StatusItem, Tone},
    },
    theme,
    ui::{
        self, ButtonSize, HAIRLINE, Variant,
        behaviors::{Align, DismissReason, bind_layer, popup_on_parent},
        motion,
    },
};

use super::{Workspace, plugin_env::ACTION_TIMEOUT};

/// The desktop's `INLINE_BUTTONS`: plugin buttons that show before the rest move into a menu. Pinned ones always show,
/// and take these places first.
const INLINE_BUTTONS: usize = 3;

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
        Tone::Neutral => (0xffffff0f, theme::muted_foreground(), 0xffffff1a, theme::idle()),
        Tone::Info => (0x7aa7f21f, 0x9cc0ff, 0x7aa7f240, 0x7aa7f2),
        Tone::Success => (0x7dd3a71f, theme::done(), 0x7dd3a740, theme::done()),
        Tone::Warning => (0xe9b65f1f, theme::working(), 0xe9b65f40, theme::working()),
        Tone::Danger => (0xf2786a1f, theme::blocked(), 0xf2786a4d, theme::blocked()),
        Tone::Merged => (0xb392f024, 0xc9b2f7, 0xb392f04d, 0xb392f0),
    };
    Palette {
        pill,
        text,
        ring,
        dot,
    }
}

/// The icon of `installed`'s sidebar `sidebar`, as the desktop's `PluginGlyph`: the one its manifest names, else the
/// plugin's own SVG.
pub(super) fn sidebar_glyph(installed: &Installed, sidebar: &str, size: f32) -> AnyElement {
    let named = installed
        .plugin
        .manifest
        .contributes
        .sidebars
        .iter()
        .find(|declared| declared.id == sidebar)
        .map_or("", |declared| declared.icon.as_str());
    plugin_glyph(
        named,
        installed.plugin.icon_svg.as_deref(),
        size,
        theme::muted_foreground(),
    )
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
            .focus_visible(|style| style.border_color(rgb(theme::foreground())))
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
                                .text_color(rgb(theme::foreground()))
                                .child(title)
                        }))
                        .children(badge.subtitle.clone().map(|subtitle| {
                            div()
                                .truncate()
                                .text_color(rgb(theme::muted_foreground()))
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
                    .text_color(rgb(theme::blocked()))
                    .child(error)
            }))
            .when(!badge.actions.is_empty(), |card| {
                card.child(div().flex().flex_wrap().gap(px(6.0)).children(actions))
            });
        // A press on the pill is its own: it toggles the card.
        let card = bind_layer(
            card,
            cx.listener(|this, reason: &DismissReason, _, cx| {
                let on_pill = this
                    .badge_card
                    .as_ref()
                    .and_then(|card| card.pill)
                    .zip(match reason {
                        DismissReason::OutsidePress(at) => Some(*at),
                        DismissReason::EscapeKey => None,
                    })
                    .is_some_and(|(pill, at)| pill.contains(&at));
                if !on_pill {
                    this.badge_card = None;
                    cx.notify_workspace();
                }
            }),
        );
        // 8pt under the pill, right edge to right edge: the pill's box is the card's parent.
        popup_on_parent(card)
            .align(Align::End)
            .side_offset(8.0)
            .priority(0)
            .layer()
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
        cx.notify_workspace();
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
        cx.notify_workspace();
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
                cx.notify_workspace();
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

    /// The plugins' bar buttons, as the desktop's `pagesOf`: each plugin's sidebars, then its pages with a button that
    /// open as a tab or a window (the places a native page can open from here).
    fn bar_buttons(&self) -> Vec<BarButton<'_>> {
        self
            .plugins
            .iter()
            .flat_map(|installed| {
                let contributes = &installed.plugin.manifest.contributes;
                let sidebars = installed
                    .sidebars
                    .iter()
                    .filter(|sidebar| {
                        !contributes
                            .sidebars
                            .iter()
                            .any(|declared| declared.id == sidebar.id && !declared.button)
                    })
                    .map(move |sidebar| BarButton {
                    installed,
                    id: sidebar.id.clone(),
                    title: sidebar.title.clone(),
                    kind: Kind::Sidebar(sidebar),
                });
                let views = installed
                    .plugin
                    .manifest
                    .contributes
                    .views
                    .iter()
                    .filter(|view| view.button)
                    .filter(|view| installed.pages.iter().any(|page| page.id == view.id))
                    .filter_map(move |view| {
                        let kind = match view.opens.first()?.as_str() {
                            "tab" => Kind::Tab,
                            "window" => Kind::Window,
                            _ => return None,
                        };
                        Some(BarButton {
                            installed,
                            id: view.id.clone(),
                            title: view.title.clone(),
                            kind,
                        })
                    });
                sidebars.chain(views)
            })
            .collect()
    }

    /// The plugin buttons the bar wants, for the room they take: the letters in each one's title, in the order the bar
    /// draws them (every pinned one, then the free places), and whether its menu shows even with all of them in the bar
    /// (there are more buttons, or one is pinned and the menu is where it's let go).
    pub(super) fn bar_fit(&self) -> (Vec<usize>, bool) {
        let all = self.bar_buttons();
        let keys: Vec<String> = all.iter().map(BarButton::key).collect();
        let (inline, overflow) = split_buttons(&keys, &self.plugin_ui.pinned, usize::MAX);
        let pinned_any = inline.iter().any(|at| self.plugin_ui.pinned.contains(&keys[*at]));
        let letters = inline.iter().map(|at| all[*at].title.chars().count()).collect();
        (letters, !overflow.is_empty() || pinned_any)
    }

    /// The plugins' sidebar buttons, at the project bar's right end: each opens its page in the panel on the right. A
    /// label the plugin set (`setSidebarLabel`) follows the icon behind a tone dot, as the desktop's `ProjectBar` draws
    /// it, and its rows show when you point at the button. A sidebar with a popover page shows that instead, under the
    /// button, while you point at it (the desktop's `WithPopover`).
    /// The project bar's plugin buttons, as the desktop's: the pinned ones in the order they were pinned, then others up
    /// to `INLINE_BUTTONS`, and a menu of the rest that also chooses which stay in the bar. A narrow bar shows `inline`
    /// of them at most.
    pub(super) fn sidebar_buttons(&self, inline: usize, labels: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let all = self.bar_buttons();
        let keys: Vec<String> = all.iter().map(BarButton::key).collect();
        let (inline, overflow) = split_buttons(&keys, &self.plugin_ui.pinned, inline);
        let pinned_any = inline
            .iter()
            .any(|at| self.plugin_ui.pinned.contains(&keys[*at]));
        let mut buttons: Vec<AnyElement> = inline
            .iter()
            .map(|at| self.bar_button(&all[*at], labels, cx))
            .collect();
        if !overflow.is_empty() || pinned_any {
            let overflow: Vec<&BarButton> = overflow.iter().map(|at| &all[*at]).collect();
            let keys: Vec<_> = all
                .iter()
                .map(|button| (button.key(), button.title.clone()))
                .collect();
            buttons.push(self.buttons_menu(&overflow, keys, cx));
        }
        buttons
    }

    fn bar_button(&self, button: &BarButton, labels: bool, cx: &mut Context<Self>) -> AnyElement {
        let installed = button.installed;
        let (plugin, id) = (installed.id.clone(), button.id.clone());
        let opens = match button.kind {
            Kind::Sidebar(sidebar) => return self.sidebar_button(installed, sidebar, labels, cx),
            Kind::Tab => Kind::Tab,
            Kind::Window => Kind::Window,
        };
        let shown = matches!(opens, Kind::Tab)
            && matches!(self.active, super::Active::Plugin(tab)
                if self.plugin_tabs.iter().any(|t| t.id == tab && t.plugin == plugin && t.view == id));
        ui::untipped_ghost_button(
            SharedString::from(format!("plugin-view-{plugin}-{id}")),
            button.title.clone(),
            ButtonSize::Sm,
            Some(shown),
            if labels {
                titled(button.glyph(14.0), &button.title)
            } else {
                button.glyph(16.0)
            },
        )
        .when(labels, |button| button_with_title(button, shown))
        .children(ui::spot(|| format!("bar-button:{plugin}:{id}")))
        .tooltip(ui::tooltip(button.title.clone(), None))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.open_bar_view(&plugin, &id, &opens, window, cx)
        }))
        .into_any_element()
    }

    /// Opens a page from its bar button, in the first place its manifest lists.
    fn open_bar_view(
        &mut self,
        plugin: &str,
        id: &str,
        opens: &Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.plugin_ui.buttons_menu = false;
        match opens {
            Kind::Window => {
                if let Err(error) = self.open_plugin_window(plugin, id, None, None, cx) {
                    eprintln!("Couldn't open {plugin}'s {id}: {error}");
                }
            }
            _ => {
                self.open_plugin_view(plugin, id, None, window, cx);
            }
        }
        cx.notify_workspace();
    }

    /// The "…" after the plugin buttons: the ones that didn't fit, then "Keep in bar" with a check by each pinned one.
    fn buttons_menu(
        &self,
        overflow: &[&BarButton],
        all: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut items: Vec<AnyElement> = overflow
            .iter()
            .map(|button| {
                let (plugin, page) = (button.installed.id.clone(), button.id.clone());
                // A live label, like a running timer, after the title as in the bar.
                let label = self
                    .plugin_ui
                    .labels
                    .get(&(plugin.clone(), page.clone()))
                    .map(|label| {
                        div()
                            .ml(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .max_w(px(200.0))
                            .text_size(px(12.0))
                            .text_color(rgb(theme::muted_foreground()))
                            .child(ui::dot(tone_dot(label.tone)))
                            .child(div().min_w_0().truncate().child(label.text.clone()))
                    });
                let opens = match button.kind {
                    Kind::Sidebar(_) => None,
                    Kind::Tab => Some(Kind::Tab),
                    Kind::Window => Some(Kind::Window),
                };
                ui::select_item_with(
                    SharedString::from(format!("plugin-more-{plugin}-{page}")),
                    button.glyph(16.0),
                    button.title.clone(),
                )
                .children(label)
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| match &opens {
                    None => {
                        this.plugin_ui.buttons_menu = false;
                        this.toggle_sidebar(&plugin, &page, event.modifiers().secondary(cx), cx);
                    }
                    Some(opens) => this.open_bar_view(&plugin, &page, opens, window, cx),
                }))
                .into_any_element()
            })
            .collect();
        if !overflow.is_empty() {
            items.push(ui::menu_separator());
        }
        items.push(
            div()
                .px(px(8.0))
                .py(px(6.0))
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(theme::muted_foreground()))
                .child("Keep in bar")
                .into_any_element(),
        );
        items.extend(all.into_iter().map(|(key, title)| {
            let checked = self.plugin_ui.pinned.contains(&key);
            ui::select_item(SharedString::from(format!("keep-{key}")), title, checked)
                .role(Role::MenuItemCheckBox)
                .aria_toggled(if checked {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_pin(&key, cx)))
                .into_any_element()
        }));
        let label = if overflow.is_empty() {
            "Plugin buttons".to_string()
        } else {
            format!("{} more from plugins", overflow.len())
        };
        let open = self.plugin_ui.buttons_menu;
        div()
            .flex()
            .flex_col()
            .items_end()
            .child(
                ui::behaviors::note_press(
                    ui::icon_button("plugin-buttons", Icon::Ellipsis, label, ButtonSize::Sm),
                    "plugin-buttons",
                    open,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.plugin_ui.buttons_menu = !ui::behaviors::was_open("plugin-buttons", open, cx);
                    cx.notify_workspace();
                })),
            )
            .children(
                self.popover_phase("plugin-buttons-menu", open, cx)
                    .map(|phase| {
                        motion::with_phase(phase, || {
                            ui::popover(ui::select_menu(items).w(px(224.0)).on_mouse_down_out(
                                cx.listener(|this, _, _, cx| {
                                    this.plugin_ui.buttons_menu = false;
                                    cx.notify_workspace();
                                }),
                            ))
                        })
                    }),
            )
            .into_any_element()
    }

    /// Keeps a plugin button in the bar, or lets it go back to its place among the others.
    pub(super) fn toggle_pin(&mut self, key: &str, cx: &mut Context<Self>) {
        let pinned = &mut self.plugin_ui.pinned;
        match pinned.iter().position(|kept| kept == key) {
            Some(at) => {
                pinned.remove(at);
            }
            None => pinned.push(key.to_owned()),
        }
        self.save(cx);
        cx.notify_workspace();
    }

    fn sidebar_button(
        &self,
        installed: &Installed,
        sidebar: &crate::installed::Sidebar,
        labels: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        {
            {
                let (plugin, page) = (installed.id.clone(), sidebar.id.clone());
                let key = (plugin.clone(), page.clone());
                let open = self.right.contains(&key);
                let mark = sidebar_glyph(installed, &page, if labels { 14.0 } else { 16.0 });
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
                    .when(labels, |content| content.child(title_text(&sidebar.title)))
                    .when_some(label, |content, label| match count_of(&label.text) {
                        // A count is a pill, as the redesign draws a plugin's.
                        Some(count) => content.child(count_badge(count)),
                        None => content.child(ui::dot(tone_dot(label.tone))).child(
                            div()
                                .max_w(px(200.0))
                                .truncate()
                                .text_size(px(12.0))
                                .child(label.text.clone()),
                        ),
                    });
                let button = ui::untipped_ghost_button(
                    SharedString::from(format!("plugin-sidebar-{plugin}-{page}")),
                    name.clone(),
                    ButtonSize::Sm,
                    Some(open),
                    content,
                )
                .when(label.is_some(), |button| button.w_auto().px(px(8.0)))
                .when(labels, |button| button_with_title(button, open))
                .children(ui::spot(|| format!("bar-button:{plugin}:{page}")));
                // The popover shows more than a tooltip would. Typing in it doesn't take the pointer off its button.
                let button = if has_popover {
                    let key = key.clone();
                    button
                        .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
                        .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                            this.hover_popover(key.clone(), false, *hovered, window, cx)
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
                let menu_key = key.clone();
                let button = button.on_click(cx.listener(
                    move |this, event: &ClickEvent, _, cx| {
                        let split = event.modifiers().secondary(cx);
                        this.toggle_sidebar(&plugin, &page, split, cx)
                    },
                ));
                let card = has_popover.then(|| self.popover_card(&key, cx)).flatten();
                div()
                    .relative()
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.open_sidebar_menu(menu_key.clone(), event, cx)
                        }),
                    )
                    .flex_none()
                    .child(button)
                    .children(card)
                    .into_any_element()
            }
        }
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
            .bg(rgb(theme::dialog()))
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
            .when_some(self.plugin_ui.popover.focus.as_ref(), |card, focus| {
                card.track_focus(focus)
            })
            // gpui reads a key press as the pointer leaving until it moves again: typing in the popover would close it.
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
            .on_hover(cx.listener(move |this, over: &bool, window, cx| {
                this.hover_popover(hovered.clone(), true, *over, window, cx)
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // A press anywhere else closes it, and takes the keyboard from a field in it, as one outside a desktop
            // popover does; one on a menu its page opened over the window, which can reach past the card, doesn't.
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                let popover = &this.plugin_ui.popover;
                let menu = popover.open.as_ref().and_then(|key| popover.pages.get(key));
                if menu.is_some_and(|(page, _)| page.read(cx).shows_menu()) {
                    return;
                }
                if this.popover_has_focus(window, cx) {
                    window.blur(cx);
                }
                this.hide_popover(cx);
            }))
            .child(page.clone())
            .children(ui::spot(|| "plugin-popover".to_owned()));
        Some(motion::with_phase(phase, || {
            // 6pt under the button, right edge to right edge, as the desktop's `align="end" sideOffset={6}`: the
            // button's box is the popover's parent.
            let card = motion::popup("plugin-popover", motion::From::Above, motion::POPUP_IN, card);
            popup_on_parent(card)
                .align(Align::End)
                .side_offset(6.0)
                .priority(0)
                .layer()
        }))
    }

    /// The pointer came onto (`hovered`) or left the button of the sidebar `key`, or its popover (`card`). It shows
    /// after `POPOVER_OPEN_DELAY` over either and goes `POPOVER_CLOSE_DELAY` after leaving both, as Base UI's
    /// `HoverCardTrigger delay={300} closeDelay={200}`; never over its own open sidebar, which shows the same and more.
    /// While a field in it has the keyboard or a menu of its page is open it stays, and goes once that ends with the
    /// pointer away (`popover_focus_left`).
    pub(super) fn hover_popover(
        &mut self,
        key: (String, String),
        card: bool,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.plugin_ui.popover.focus.is_none() {
            let focus = cx.focus_handle();
            self.plugin_ui.popover.focus_out =
                Some(cx.on_focus_out(&focus, window, |this, _, window, cx| {
                    this.popover_focus_left(window, cx)
                }));
            self.plugin_ui.popover.focus = Some(focus);
        }
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
        if over && (shown || self.right.contains(&key)) {
            popover.timer = None;
        } else if over {
            popover.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(POPOVER_OPEN_DELAY).await;
                let _ = this.update(cx, |this, cx| this.show_popover(key, cx));
            }));
        } else if shown {
            self.close_popover_later(window, cx);
        } else {
            popover.timer = None;
        }
    }

    /// Closes the popover after `POPOVER_CLOSE_DELAY`, unless by then a field in it has the keyboard or a menu of its
    /// page is open.
    fn close_popover_later(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.popover_holds_keyboard(window, cx) {
            self.plugin_ui.popover.timer = None;
            return;
        }
        self.plugin_ui.popover.timer = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(POPOVER_CLOSE_DELAY).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.popover_holds_keyboard(window, cx) {
                    this.hide_popover(cx);
                }
            });
        }));
    }

    /// The keyboard left the popover: it goes as it would have when the pointer left, unless the pointer is on it.
    fn popover_focus_left(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let popover = &self.plugin_ui.popover;
        let Some(open) = popover.open.as_ref() else {
            return;
        };
        let over = popover.hovered.as_ref() == Some(open) && popover.over.contains(&true);
        if !over {
            self.close_popover_later(window, cx);
        }
    }

    /// Whether the keyboard is in the shown popover.
    fn popover_has_focus(&self, window: &Window, cx: &gpui::App) -> bool {
        let popover = &self.plugin_ui.popover;
        popover.open.is_some()
            && popover
                .focus
                .as_ref()
                .is_some_and(|focus| focus.contains_focused(window, cx))
    }

    /// Whether the shown popover is in use beyond the pointer: a field in it has the keyboard, or its page opened a
    /// menu, which is drawn over the window and can reach past the card.
    fn popover_holds_keyboard(&self, window: &Window, cx: &gpui::App) -> bool {
        let popover = &self.plugin_ui.popover;
        let menu = popover.open.as_ref().and_then(|key| popover.pages.get(key));
        self.popover_has_focus(window, cx)
            || menu.is_some_and(|(page, _)| page.read(cx).shows_menu())
    }

    /// Shows the popover page of the sidebar `key`, loading it the first time or after it was unloaded.
    pub(super) fn show_popover(&mut self, key: (String, String), cx: &mut Context<Self>) {
        if self.right.contains(&key) {
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
                cx.notify_workspace();
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
        cx.notify_workspace();
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
        cx.notify_workspace();
    }

    /// The panel on the right, with the open sidebar's page, or nothing while none is open. Opening or closing it
    /// slides it, as the desktop's right panel does, its page narrowing with it down to its 250px and clipped below;
    /// moving from one sidebar to another doesn't.
    pub(super) fn right_panel(
        &mut self,
        viewport: Pixels,
        window: gpui::WindowId,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let now = cx.background_executor().now();
        let open = !self.right.is_empty();
        let reduced = motion::reduced(cx);
        let slide = &mut self.plugin_ui.right_slide;
        if open && slide.shown != self.right {
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
                            cx.notify_workspace();
                        }
                    });
                }));
            }
            Some(_) => {}
        }
        let share = slide.share;
        if !open && share.is_none() {
            slide.shown.clear();
        }
        let count = slide.count;
        let shown = slide.shown.clone();
        let pages = self.right_pages(&shown, window, cx)?;
        let title = match shown.only() {
            Some((plugin, sidebar)) => self
                .sidebar_pages
                .get(&(plugin.clone(), sidebar.clone()))
                .and_then(|page| page.read(cx).header_title())
                .or_else(|| {
                    self.plugins
                        .iter()
                        .find(|installed| installed.id == *plugin)
                        .and_then(|installed| installed.sidebars.iter().find(|page| page.id == *sidebar))
                        .map(|page| page.title.clone())
                })
                .unwrap_or_else(|| "Sidebar".to_owned()),
            None => "Sidebars".to_owned(),
        };
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
            // Flush with the main column, with one line between them.
            .border_l_1()
            .border_color(rgba(HAIRLINE))
            .child(pages);
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
        let update = self.update_toast(cx);
        if update.is_none()
            && self.plugin_ui.notices.is_empty()
            && self.manage.announcements.is_empty()
        {
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
                .children(update)
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
                        .border_color(rgba(theme::hairline_strong()))
                        .bg(rgba(theme::popover()))
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
                                    .text_color(rgb(theme::muted_foreground()))
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
                .map(|label| div().text_color(rgb(theme::muted_foreground())).child(label)),
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
                    _ => theme::foreground(),
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
                        .text_color(rgb(theme::muted_foreground()))
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
                        .text_color(rgb(colors.as_ref().map_or(theme::foreground(), |c| c.text)))
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
    /// The sidebars it shows, kept while it slides shut.
    shown: super::right_stack::Stack,
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
    pub(super) hovered: Option<(String, String)>,
    over: [bool; 2],
    /// Shows or hides it after its delay.
    timer: Option<Task<()>>,
    unload: HashMap<(String, String), Task<()>>,
    /// Held by the shown card, so it knows when a field in its page has the keyboard, and hears when it leaves.
    focus: Option<FocusHandle>,
    focus_out: Option<Subscription>,
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
/// A plugin button's title after its icon, as the redesign's `.btn` draws it.
fn title_text(title: &str) -> AnyElement {
    div().text_size(px(12.5)).whitespace_nowrap().child(title.to_owned()).into_any_element()
}

fn titled(glyph: AnyElement, title: &str) -> AnyElement {
    div().flex().items_center().gap(px(6.0)).child(glyph).child(title_text(title)).into_any_element()
}

/// A bar button that grows to its title: 26 tall with 10 points at each side, its text a step under the foreground.
fn button_with_title(button: gpui::Stateful<gpui::Div>, on: bool) -> gpui::Stateful<gpui::Div> {
    button
        .w_auto()
        .h(px(26.0))
        .px(px(10.0))
        .rounded(px(6.0))
        .when(!on, |button| button.text_color(rgb(theme::secondary_foreground())))
}

/// The label a plugin set when it is a count: digits only.
pub(super) fn count_of(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty() && text.chars().all(|c| c.is_ascii_digit())).then_some(text)
}

/// The redesign's `.badge`: 11 point semibold digits in an 18 tall pill tinted with the accent.
fn count_badge(count: &str) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(18.0))
        .h(px(18.0))
        .px(px(5.0))
        .rounded(px(9.0))
        .bg(rgba(theme::info() << 8 | 0x24))
        .text_color(rgb(theme::info()))
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(count.to_owned())
        .into_any_element()
}

pub(super) fn tone_dot(tone: Option<Tone>) -> u32 {
    match tone {
        Some(Tone::Warning) => theme::working(),
        Some(Tone::Danger) => theme::blocked(),
        _ => theme::done(),
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
                                .text_color(rgba(theme::background() << 8 | 0x99))
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
                    .bg(rgb(theme::foreground()))
                    .text_size(px(12.0))
                    .text_color(rgb(theme::background()))
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
                    .text_color(rgb(theme::foreground()))
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

    /// Pins a plugin button to the project bar, as "Keep in bar" does.
    pub fn shot_pin(&mut self, key: &str, cx: &mut Context<Self>) {
        self.toggle_pin(key, cx);
    }

    /// Adds the plugin's sidebar as a split below the open ones, as a Cmd+click on its button does.
    pub fn shot_split_sidebar(&mut self, plugin: &str, sidebar: &str, cx: &mut Context<Self>) {
        self.split_sidebar(plugin, sidebar, cx);
    }

    /// Opens a diff from the plugin's open sidebar, as its `openDiff` with `sidebar` does.
    pub fn shot_sidebar_diff(
        &mut self,
        plugin: &str,
        sidebar: &str,
        patch: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let update = crate::diff::clean_update(&serde_json::json!({ "patch": patch }))
            .expect("a patch to show");
        let view = crate::diff::View {
            id: format!("{plugin}:shot"),
            plugin_id: plugin.to_owned(),
            sidebar: Some(sidebar.to_owned()),
            title: "Uncommitted changes".into(),
            subtitle: "acme-web \u{b7} Working tree against HEAD".into(),
            cwd: String::new(),
            refresh: false,
            update,
        };
        self.open_diff_view(view, window, cx);
    }

    pub fn shot_popover(&mut self, plugin: &str, sidebar: &str, cx: &mut Context<Self>) {
        self.right.clear();
        self.sync_plugin_visibility(cx);
        self.show_popover((plugin.to_owned(), sidebar.to_owned()), cx);
    }

    /// The page of the plugin tab on screen.
    pub fn tab_page(&self) -> Option<Entity<PageView>> {
        let super::Active::Plugin(id) = self.active else {
            return None;
        };
        self.plugin_tabs.iter().find(|tab| tab.id == id).map(|tab| tab.page.clone())
    }

    pub fn sidebar_page(&self, plugin: &str, sidebar: &str) -> Option<Entity<PageView>> {
        self.sidebar_pages
            .get(&(plugin.to_owned(), sidebar.to_owned()))
            .cloned()
    }

    /// A running plugin's host, to call its tools as Claude does.
    pub fn plugin_host(&self, plugin: &str) -> Option<std::sync::Arc<crate::page::plugin::PluginHost>> {
        self.plugins
            .iter()
            .find(|installed| installed.id == plugin)
            .map(|installed| installed.host.clone())
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

/// What a plugin button in the project bar opens.
enum Kind<'a> {
    Sidebar(&'a crate::installed::Sidebar),
    Tab,
    Window,
}

/// One of the project bar's plugin buttons: a sidebar, or a page that opens as a tab or a window.
struct BarButton<'a> {
    installed: &'a Installed,
    id: String,
    title: String,
    kind: Kind<'a>,
}

impl BarButton<'_> {
    /// As the desktop's button keys, which pins are saved by.
    fn key(&self) -> String {
        format!("{}:{}", self.installed.id, self.id)
    }

    fn glyph(&self, size: f32) -> AnyElement {
        match self.kind {
            Kind::Sidebar(_) => sidebar_glyph(self.installed, &self.id, size),
            _ => {
                let named = self
                    .installed
                    .plugin
                    .manifest
                    .contributes
                    .views
                    .iter()
                    .find(|view| view.id == self.id)
                    .map_or("", |view| view.icon.as_str());
                plugin_glyph(
                    named,
                    self.installed.plugin.icon_svg.as_deref(),
                    size,
                    theme::muted_foreground(),
                )
            }
        }
    }
}

/// Which plugin buttons show in the bar and which go in its menu, by index into `keys`, as the desktop's `ProjectBar`:
/// the pinned ones in the order they were pinned, then the others in order up to `INLINE_BUTTONS`.
///
/// A narrow bar shows no more than `limit`: the pinned ones that don't fit go in the menu too, after those that were there.
fn split_buttons(keys: &[String], pinned: &[String], limit: usize) -> (Vec<usize>, Vec<usize>) {
    let mut inline: Vec<usize> = pinned
        .iter()
        .filter_map(|key| keys.iter().position(|k| k == key))
        .collect();
    let rest: Vec<usize> = (0..keys.len())
        .filter(|at| !pinned.contains(&keys[*at]))
        .collect();
    let free = INLINE_BUTTONS.saturating_sub(inline.len()).min(rest.len());
    inline.extend(&rest[..free]);
    let mut menu = inline.split_off(limit.min(inline.len()));
    menu.extend(&rest[free..]);
    (inline, menu)
}

#[cfg(test)]
mod pin_tests {
    use super::split_buttons;

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn three_buttons_show_and_the_rest_go_in_the_menu() {
        let all = keys(&["a:1", "b:1", "c:1", "d:1", "e:1"]);
        assert_eq!(split_buttons(&all, &[], usize::MAX), (vec![0, 1, 2], vec![3, 4]));
    }

    #[test]
    fn pinned_buttons_show_first_in_the_order_they_were_pinned_and_take_the_places() {
        let all = keys(&["a:1", "b:1", "c:1", "d:1", "e:1"]);
        assert_eq!(
            split_buttons(&all, &keys(&["e:1", "c:1"]), usize::MAX),
            (vec![4, 2, 0], vec![1, 3])
        );
        // More pinned than places: all of them show, and nothing else.
        let pins = keys(&["e:1", "d:1", "c:1", "b:1"]);
        assert_eq!(split_buttons(&all, &pins, usize::MAX), (vec![4, 3, 2, 1], vec![0]));
        // A pin of a plugin that's gone counts for nothing.
        assert_eq!(
            split_buttons(&all, &keys(&["gone:1"]), usize::MAX),
            (vec![0, 1, 2], vec![3, 4])
        );
    }
}

#[cfg(test)]
mod bar_tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::super::plugin_tests::open_plugins;
    use super::Kind;
    use crate::{installed::fixture::Data, page::runtime::fake::idle};

    /// As the desktop's bar: a plugin's pages that open as a tab or a window get a button beside its sidebars, unless
    /// the manifest says `button: false`, and a tab page's button opens it as a tab.
    #[test]
    fn a_plugins_tab_pages_get_bar_buttons_beside_its_sidebars() {
        let data = Data::temp();
        data.install_script(
            "disk",
            json!({ "contributes": {
                "sidebars": [{ "id": "side", "title": "Side", "page": "dist/side.html" }],
                "views": [
                    { "id": "map", "title": "Disk usage", "page": "dist/map.html", "opens": ["tab"] },
                    { "id": "one", "title": "One item", "page": "dist/one.html", "opens": ["tab"], "button": false },
                ],
            } }),
            &idle("main"),
            &[("side", &idle("page")), ("map", &idle("page")), ("one", &idle("page"))],
            "disk-scope",
        );
        let (_app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
        window.update(|w, window, cx| {
            assert_eq!(
                w.sidebar_buttons(usize::MAX, false, cx).len(),
                2,
                "the sidebar and Disk usage, not One item"
            );
            w.open_bar_view("disk", "map", &Kind::Tab, window, cx);
            assert!(
                w.plugin_tabs
                    .iter()
                    .any(|tab| tab.plugin == "disk" && tab.view == "map")
            );
        });
    }

    /// A sidebar the manifest says `button: false` for gets no bar button, and isn't offered in "Keep in bar", but
    /// the plugin still opens it.
    #[test]
    fn a_sidebar_without_a_button_stays_out_of_the_bar_but_still_opens() {
        let data = Data::temp();
        data.install_script(
            "chat",
            json!({ "contributes": {
                "sidebars": [
                    { "id": "side", "title": "Side", "page": "dist/side.html" },
                    { "id": "helper", "title": "Helper", "page": "dist/helper.html", "button": false },
                ],
            } }),
            &idle("main"),
            &[("side", &idle("page")), ("helper", &idle("page"))],
            "chat-scope",
        );
        let (_app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
        window.update(|w, _, cx| {
            assert_eq!(w.sidebar_buttons(usize::MAX, false, cx).len(), 1, "Side only, not Helper");
            assert!(w.open_sidebar_from_plugin("chat", "helper", cx));
        });
    }
}
