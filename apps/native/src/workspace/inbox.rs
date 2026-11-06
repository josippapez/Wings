//! The desktop's Inbox (`notification-bell.tsx`, `lib/notifications.ts`): the sidebar row with the unread count, and the
//! list beside it of what agents and plugins said, newest first. An agent that comes to need you or finishes is kept,
//! as is every notification a plugin shows. Opening the list marks everything read.

use std::{collections::HashSet, path::PathBuf};

use gpui::{
    AnyElement, BoxShadow, Context, Div, ElementId, FontWeight, Role, SharedString, Stateful,
    Window, div, hsla, point, prelude::*, px, rgb, rgba,
};

use crate::{
    agents::{self, Agent, AgentState},
    icons::{Icon, icon},
    page::plugin::{Env, Notification, NotifyButton},
    theme,
    ui::{self, HAIRLINE},
};

use super::{
    Workspace,
    sidebar::{NAV_GAP, NAV_ROW},
    sound,
    title_bar::TITLE_BAR_HEIGHT,
};

/// The most the list keeps, as the desktop's `MAX_NOTIFICATIONS`.
const MAX: usize = 100;
/// The desktop's `--chart-1`, the count's colour while a plugin's notification is unread.
const ACCENT: u32 = 0x7aa7f2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Blocked,
    Done,
    Plugin,
}

/// What the list keeps about one thing that happened.
pub(super) struct Event {
    id: usize,
    /// ms since 1970.
    at: u64,
    pub(super) kind: Kind,
    pub(super) title: SharedString,
    /// The project for an agent, the plugin's name for a plugin.
    project: String,
    /// What the agent was waiting for, or the plugin's body.
    pub(super) detail: String,
    /// The agent's pane, for Go to pane.
    pub(super) pane: Option<usize>,
    pub(super) read: bool,
    pub(super) plugin: Option<Notification>,
}

#[derive(Default)]
pub(super) struct Inbox {
    pub(super) events: Vec<Event>,
    /// While the list is open: what was unread when it opened, so those rows stay marked while you look at them.
    pub(super) open: Option<HashSet<usize>>,
    next: usize,
    /// Notifications close together share one sound.
    sound: sound::Burst,
}

impl Inbox {
    /// Adds an unread event at the top.
    fn add(
        &mut self,
        kind: Kind,
        title: String,
        project: String,
        detail: String,
        pane: Option<usize>,
        plugin: Option<Notification>,
    ) {
        let id = self.next;
        self.next += 1;
        self.events.insert(
            0,
            Event {
                id,
                at: now_ms(),
                kind,
                title: title.into(),
                project,
                detail,
                pane,
                read: false,
                plugin,
            },
        );
        self.events.truncate(MAX);
    }

    pub(super) fn unread(&self) -> usize {
        self.events.iter().filter(|event| !event.read).count()
    }
}

impl Workspace {
    /// Plays the chosen sound for something new, unless it's off or one just played, as the desktop's
    /// `notificationSound`. `watching`: it's about the pane on screen.
    fn notification_sound(&mut self, watching: bool) {
        let settings = self.prefs.notify_sound;
        if watching && !settings.watching {
            return;
        }
        if settings.enabled && self.inbox.sound.due(now_ms()) {
            sound::play(settings.sound);
        }
    }

    /// A plugin's notification, unread.
    pub(super) fn inbox_plugin(&mut self, notification: Notification) {
        self.notification_sound(false);
        let title = if notification.title.is_empty() {
            notification.name.clone()
        } else {
            notification.title.clone()
        };
        let (project, detail) = (notification.name.clone(), notification.body.clone());
        self.inbox.add(
            Kind::Plugin,
            title,
            project,
            detail,
            None,
            Some(notification),
        );
    }

    /// Keeps each agent that came to need you or finished since the last check. One you are looking at, in the window
    /// in front, counts as read.
    pub(super) fn inbox_agents(&mut self, before: &std::collections::HashMap<usize, Agent>) {
        let seen = self.window_active.then(|| self.focused_pane()).flatten();
        let mut changed: Vec<(usize, Agent)> = self
            .agents
            .iter()
            .filter(|(pane, agent)| {
                before.get(pane).is_some_and(|was| was.state != agent.state)
                    && matches!(agent.state, AgentState::Blocked | AgentState::Done)
            })
            .map(|(pane, agent)| (*pane, agent.clone()))
            .collect();
        changed.sort_by_key(|(pane, _)| *pane);
        for (pane, agent) in changed {
            let name = agent
                .name
                .clone()
                .unwrap_or_else(|| agents::label(agent.kind).to_owned());
            let project = self
                .projects
                .iter()
                .find(|project| project.path.to_string_lossy() == agent.space_id)
                .map_or_else(String::new, |project| project.name.clone());
            let (kind, title, detail) = if agent.state == AgentState::Blocked {
                let waiting = agent.waiting_for.clone();
                (
                    Kind::Blocked,
                    format!("{name} needs you"),
                    waiting.unwrap_or_else(|| "waiting for input".into()),
                )
            } else {
                (Kind::Done, format!("{name} finished"), String::new())
            };
            self.inbox
                .add(kind, title, project, detail, Some(pane), None);
            self.inbox.events[0].read = seen == Some(pane);
            self.notification_sound(seen == Some(pane));
        }
    }

    pub(super) fn open_inbox(&mut self, cx: &mut Context<Self>) {
        let unread = self.inbox.events.iter().filter(|event| !event.read);
        self.inbox.open = Some(unread.map(|event| event.id).collect());
        for event in &mut self.inbox.events {
            event.read = true;
        }
        cx.notify();
    }

    pub(super) fn close_inbox(&mut self, cx: &mut Context<Self>) {
        if self.inbox.open.take().is_some() {
            cx.notify();
        }
    }

    fn inbox_go_to_pane(&mut self, pane: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.inbox.open = None;
        let project = self
            .agents
            .get(&pane)
            .map(|agent| PathBuf::from(&agent.space_id));
        match project {
            Some(project) => self.go_to_pane(project, pane, window, cx),
            None => self.show_notice("That pane has closed".into(), String::new(), cx),
        }
    }

    /// A plugin notification's button: its view as a new tab of the notification's project, else the one on screen, or
    /// its link in the browser if the plugin may open it.
    pub(super) fn inbox_button(
        &mut self,
        notification: &Notification,
        button: &NotifyButton,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inbox.open = None;
        match button {
            NotifyButton::Url { url, .. } => {
                let installed = self
                    .plugins
                    .iter()
                    .find(|installed| installed.id == notification.plugin);
                let allowed = installed.is_some_and(|installed| {
                    wings_plugins::fetch::may_open_url(&installed.plugin, url)
                });
                let opened = if allowed {
                    self.plugin_env.open_url(url)
                } else {
                    Err(format!("{} may not open {url}", notification.plugin))
                };
                if let Err(error) = opened {
                    self.show_notice("Couldn't open the link".into(), error, cx);
                }
            }
            NotifyButton::Open { view, args, .. } => {
                let project = notification
                    .project
                    .as_ref()
                    .map_or_else(|| self.config.project.clone(), PathBuf::from);
                self.open_section_view(
                    project,
                    &notification.plugin,
                    view,
                    args.clone(),
                    true,
                    window,
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// The sidebar's Inbox row, with the unread count in the accent colour while a plugin's notification is unread.
    pub(super) fn inbox_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let unread = self.inbox.unread();
        let accent = self
            .inbox
            .events
            .iter()
            .any(|event| !event.read && event.kind == Kind::Plugin);
        let open = self.inbox.open.is_some();
        super::sidebar::nav_row("inbox", Icon::Inbox, "Inbox", super::sidebar::Look::Plain)
            .aria_label(if unread > 0 {
                format!("Inbox, {unread} unread")
            } else {
                "Inbox".into()
            })
            .aria_expanded(open)
            .when(open, |row| {
                row.bg(rgba(theme::HOVER))
                    .text_color(rgb(theme::FOREGROUND))
            })
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // While the list is open, a click here lands on the layer that closes it.
            .on_click(cx.listener(|this, _, _, cx| this.open_inbox(cx)))
            .when(unread > 0, |row| {
                row.child(
                    div()
                        .h(px(16.0))
                        .min_w(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .px(px(4.0))
                        .rounded_full()
                        .bg(rgba(if accent {
                            ACCENT << 8 | 0xff
                        } else {
                            0xffffff1a
                        }))
                        .text_size(px(10.0))
                        .text_color(rgb(if accent { 0x17151a } else { theme::FOREGROUND }))
                        .child(if unread > 99 {
                            "99+".to_string()
                        } else {
                            unread.to_string()
                        }),
                )
            })
            .into_any_element()
    }

    /// The open list, beside the sidebar at the Inbox row's height.
    pub(super) fn inbox_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let fresh = self.inbox.open.as_ref()?;
        let left = if self.sidebar {
            self.side.width + 4.0
        } else {
            8.0
        };
        let events = &self.inbox.events;
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(rgba(HAIRLINE))
            .py(px(8.0))
            .pr(px(8.0))
            .pl(px(14.0))
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Notifications"),
            )
            .child(xs_button("inbox-clear", "Clear", events.is_empty()).when(
                !events.is_empty(),
                |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.inbox.events.clear();
                        cx.notify();
                    }))
                },
            ));
        let body = if events.is_empty() {
            div()
                .py(px(40.0))
                .px(px(24.0))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.0))
                .text_center()
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child("Nothing yet"),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child("When an agent needs you or finishes, it shows up here."),
                )
                .into_any_element()
        } else {
            let now = now_ms();
            div()
                .id("inbox-list")
                .role(Role::List)
                .max_h(px(384.0))
                .overflow_y_scroll()
                .p(px(6.0))
                .flex()
                .flex_col()
                .children(
                    events
                        .iter()
                        .map(|event| self.inbox_entry(event, fresh.contains(&event.id), now, cx)),
                )
                .into_any_element()
        };
        // A click anywhere else lands on this clear layer and only closes the list, so a click on the Inbox row closes it
        // rather than closing it and opening it again.
        let outside = div()
            .id("inbox-outside")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_any_mouse_down(cx.listener(|this, _, _, cx| this.close_inbox(cx)));
        Some(
            outside
                .child(
                    div()
                        .id("inbox-popover")
                        .role(Role::Dialog)
                        .aria_label("Notifications")
                        .absolute()
                        // Beside the Inbox row, the third in the sidebar's nav.
                        .top(px(TITLE_BAR_HEIGHT + 8.0 + 2.0 * (NAV_ROW + NAV_GAP)))
                        .left(px(left))
                        .w(px(384.0))
                        .flex()
                        .flex_col()
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(rgba(0xece9e41a))
                        .bg(rgb(0x201d23))
                        .text_color(rgb(theme::FOREGROUND))
                        .shadow(vec![BoxShadow {
                            color: hsla(0.0, 0.0, 0.0, 0.3),
                            offset: point(px(0.0), px(4.0)),
                            blur_radius: px(12.0),
                            spread_radius: px(0.0),
                            inset: false,
                        }])
                        .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                        .child(header)
                        .child(body),
                )
                .into_any_element(),
        )
    }

    fn inbox_entry(
        &self,
        event: &Event,
        fresh: bool,
        now: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = event.id;
        let glyph = match event.kind {
            Kind::Plugin => icon(Icon::Puzzle, 14.0, theme::MUTED_FOREGROUND).into_any_element(),
            Kind::Blocked => {
                ui::status_dot(("inbox-dot", id), AgentState::Blocked).into_any_element()
            }
            Kind::Done => ui::status_dot(("inbox-dot", id), AgentState::Done).into_any_element(),
        };
        let detail = [event.project.as_str(), event.detail.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(": ");
        let mut buttons: Vec<AnyElement> = Vec::new();
        if let Some(notification) = &event.plugin {
            for (index, button) in notification.buttons.iter().enumerate() {
                let (notification, button) = (notification.clone(), button.clone());
                let id = ElementId::NamedInteger(format!("inbox-{id}-button").into(), index as u64);
                buttons.push(
                    xs_button(id, button.label().to_owned(), false)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.inbox_button(&notification, &button, window, cx)
                        }))
                        .into_any_element(),
                );
            }
        }
        div()
            .flex()
            .items_start()
            .gap(px(10.0))
            .px(px(10.0))
            .py(px(8.0))
            .rounded(px(8.0))
            .when(fresh, |row| row.bg(rgba(0xffffff0a)))
            .child(
                div()
                    .mt(px(4.0))
                    .w(px(12.0))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(glyph),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(8.0))
                            .text_size(px(13.0))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(event.title.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.0))
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .child(ago(now, event.at)),
                            ),
                    )
                    .child(
                        div()
                            .line_clamp(2)
                            .text_size(px(12.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(detail),
                    )
                    .when_some(event.pane, |entry, pane| {
                        entry.child(div().mt(px(4.0)).ml(px(-8.0)).flex().child(
                            xs_button(("inbox-go", id), "Go to pane", false).on_click(cx.listener(
                                move |this, _, window, cx| this.inbox_go_to_pane(pane, window, cx),
                            )),
                        ))
                    })
                    .when(!buttons.is_empty(), |entry| {
                        entry.child(
                            div()
                                .mt(px(4.0))
                                .ml(px(-8.0))
                                .flex()
                                .flex_wrap()
                                .gap_x(px(4.0))
                                .children(buttons),
                        )
                    }),
            )
            .into_any_element()
    }
}

#[cfg(feature = "test-support")]
impl Workspace {
    /// Made-up notifications for screenshots, the newest unread: an agent that finished an hour ago, a plugin's with a
    /// button five minutes ago, and an agent that needs you now.
    pub fn sample_inbox(&mut self, cx: &mut Context<Self>) {
        let project = self
            .projects
            .first()
            .map_or_else(String::new, |p| p.name.clone());
        let plugin = Notification {
            plugin: "ci".into(),
            name: "Build bot".into(),
            title: "Build passed".into(),
            body: "main is green after 4 minutes".into(),
            project: None,
            buttons: vec![NotifyButton::Url {
                label: "Open run".into(),
                url: "https://ci.example/run/1".into(),
            }],
        };
        let inbox = &mut self.inbox;
        inbox.add(
            Kind::Done,
            "Claude finished".into(),
            project.clone(),
            String::new(),
            Some(0),
            None,
        );
        inbox.events[0].at -= 60 * 60 * 1000;
        inbox.events[0].read = true;
        inbox.add(
            Kind::Plugin,
            plugin.title.clone(),
            plugin.name.clone(),
            plugin.body.clone(),
            None,
            Some(plugin),
        );
        inbox.events[0].at -= 5 * 60 * 1000;
        inbox.add(
            Kind::Blocked,
            "Claude needs you".into(),
            project,
            "Allow Bash: cargo test".into(),
            Some(0),
            None,
        );
        cx.notify();
    }

    pub fn show_inbox(&mut self, cx: &mut Context<Self>) {
        self.open_inbox(cx);
    }
}

/// shadcn's ghost `Button` at size `xs`, in muted type.
fn xs_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .h(px(24.0))
        .flex_none()
        .flex()
        .items_center()
        .px(px(8.0))
        .rounded(px(6.0))
        .text_size(px(12.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .when(disabled, |button| {
            button.opacity(0.5).a11y_synthetic_children(|builder| {
                builder.parent_node().set_disabled();
            })
        })
        .when(!disabled, |button| {
            button.cursor_pointer().hover(|style| {
                style
                    .bg(rgba(0xffffff0f))
                    .text_color(rgb(theme::FOREGROUND))
            })
        })
        .child(label)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// How long ago, as the desktop's list says it (`Intl.RelativeTimeFormat` in English, short, `numeric: "auto"`).
fn ago(now: u64, at: u64) -> String {
    let s = now.saturating_sub(at) / 1000;
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min. ago", s / 60),
        3600..86_400 => format!("{} hr. ago", s / 3600),
        _ => match s / 86_400 {
            1 => "yesterday".into(),
            days => format!("{days} days ago"),
        },
    }
}
