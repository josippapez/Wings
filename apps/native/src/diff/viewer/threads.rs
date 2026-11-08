//! The desktop's review threads (`diff-viewer.tsx`'s `CommentThread`, `CommentBody` and `ReplyBox`): under their lines,
//! and in the discussion in place of a file.

use super::{FieldEvent, Notes, TextField, Viewer};
use crate::{
    icons::{Icon, icon},
    markdown, theme, ui,
};
use chrono::{DateTime, Utc};
use gpui::{
    AnimationExt, AnyElement, BoxShadow, Context, Entity, Focusable, FontWeight, KeyDownEvent,
    Role, SharedString, Subscription, Task, Window, div, hsla, point, prelude::*, px, rgb, rgba,
};
use std::{rc::Rc, time::Duration};

/// The card's `bg-[#1d1b22]`.
const CARD: u32 = 0x1d1b22;

/// An open reply box: what's typed, and while the plugin posts it, nothing more can be.
pub(super) struct ReplyBox {
    field: Entity<TextField>,
    busy: bool,
    error: Option<String>,
    _events: Subscription,
    task: Option<Task<()>>,
    timeout: Option<Task<()>>,
}

impl Viewer {
    /// The threads under a line: in one column both sides' under it, side by side each under its own column.
    pub(super) fn notes_row(&self, notes: &Notes, cx: &mut Context<Self>) -> AnyElement {
        let cards = |threads: &[usize], cx: &mut Context<Self>| {
            threads
                .iter()
                .map(|&t| self.thread_card(t, true, cx))
                .collect::<Vec<_>>()
        };
        if !self.split() {
            let mut all = cards(&notes.old, cx);
            all.extend(cards(&notes.new, cx));
            return div()
                .w_full()
                .flex()
                .flex_col()
                .children(all)
                .into_any_element();
        }
        let column =
            |cards: Vec<AnyElement>| div().flex_1().min_w_0().flex().flex_col().children(cards);
        div()
            .w_full()
            .flex()
            .child(column(cards(&notes.old, cx)))
            .child(div().w(px(1.0)).flex_none().bg(rgba(ui::HAIRLINE)))
            .child(column(cards(&notes.new, cx)))
            .into_any_element()
    }

    /// One of the discussion's threads, in the desktop's `flex-col gap-3 p-5`.
    pub(super) fn discussion_item(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(&t) = self.discussion.get(index) else {
            return div().into_any_element();
        };
        div()
            .px(px(20.0))
            .pt(px(if index == 0 { 20.0 } else { 12.0 }))
            .when(index + 1 == self.discussion.len(), |item| item.pb(px(20.0)))
            .child(self.thread_card(t, false, cx))
            .into_any_element()
    }

    /// The file list's first row when there are comments with no line to sit on: it opens them in place of a file.
    pub(super) fn discussion_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let count: usize = self
            .discussion
            .iter()
            .map(|&t| 1 + self.threads[t].replies.len())
            .sum();
        let open = self.discussion_open;
        div()
            .flex_none()
            .px(px(8.0))
            .child(
                div()
                    .id("diff-discussion")
                    .h(px(28.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(8.0))
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .role(Role::ListBoxOption)
                    .aria_label(SharedString::from(format!("Discussion, {count} comments")))
                    .aria_selected(open)
                    .bg(rgba(if open { 0xffffff14 } else { 0 }))
                    .when(!open, |row| row.hover(|style| style.bg(rgba(theme::HOVER))))
                    .children(ui::spot(|| "diff-discussion".into()))
                    .child(icon(Icon::MessageSquare, 12.0, theme::MUTED_FOREGROUND))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Discussion"),
                    )
                    .child(
                        div()
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(count.to_string()),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.files_focus.focus(window, cx);
                        this.open_discussion(cx);
                    })),
            )
            // The desktop's `mb-1 border-b pb-1` under it.
            .child(
                div()
                    .mt(px(4.0))
                    .mb(px(4.0))
                    .h(px(1.0))
                    .bg(rgba(ui::HAIRLINE)),
            )
            .into_any_element()
    }

    /// The desktop's `CommentThread`: the first comment, its replies under a line, and the reply box when the plugin
    /// takes replies. Under a line it's indented past the line numbers; in the discussion it fills the width and names
    /// its file.
    fn thread_card(&self, t: usize, inline: bool, cx: &mut Context<Self>) -> AnyElement {
        let comments = self.comments();
        let thread = &self.threads[t];
        let root = &comments[thread.root];
        let id = root.id;
        div()
            .id(SharedString::from(format!("diff-thread-{id}")))
            .children(ui::spot(|| format!("diff-thread-{id}")))
            .when(inline, |card| {
                card.mt(px(6.0)).mb(px(6.0)).mr(px(12.0)).ml(px(48.0))
            })
            .max_w(px(768.0))
            .flex()
            .flex_col()
            .p(px(12.0))
            .rounded(px(12.0))
            .border_1()
            .border_color(rgba(ui::HAIRLINE))
            .bg(rgb(CARD))
            .shadow(vec![BoxShadow {
                color: hsla(0.0, 0.0, 0.0, 0.25),
                offset: point(px(0.0), px(6.0)),
                blur_radius: px(20.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .text_size(px(13.0))
            .text_color(rgb(theme::FOREGROUND))
            .when_some(root.path.clone().filter(|_| !inline), |card, path| {
                card.child(
                    div()
                        .mb(px(10.0))
                        .truncate()
                        .text_size(px(11.0))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(path),
                )
            })
            .child(self.comment_body(thread.root, cx))
            .when(!thread.replies.is_empty(), |card| {
                card.child(
                    div()
                        .mt(px(12.0))
                        .pl(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .border_l_1()
                        .border_color(rgba(theme::HAIRLINE_STRONG))
                        .children(thread.replies.iter().map(|&at| self.comment_body(at, cx))),
                )
            })
            .when(root.can_reply, |card| card.child(self.reply_box(id, cx)))
            .into_any_element()
    }

    /// The desktop's `CommentBody`: the author's initial, name and when, a link to the comment, and what it says.
    fn comment_body(&self, at: usize, cx: &mut Context<Self>) -> AnyElement {
        let comment = &self.comments()[at];
        let initial: String = comment
            .author
            .chars()
            .next()
            .into_iter()
            .flat_map(char::to_uppercase)
            .collect();
        let blocks = self
            .bodies
            .borrow_mut()
            .entry(at)
            .or_insert_with(|| Rc::new(markdown::parse(&comment.body)))
            .clone();
        let url = comment.url.clone();
        div()
            .flex()
            .gap(px(10.0))
            .child(
                div()
                    .mt(px(1.0))
                    .size(px(24.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(rgba(theme::BORDER))
                    .bg(rgba(theme::HOVER))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(initial),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .text_size(px(12.0))
                            // Who wrote it keeps its room; when gives way first in a narrow dock.
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(comment.author.clone()),
                            )
                            .children(ago(&comment.created_at, Utc::now()).map(|when| {
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .child(when)
                            }))
                            .when(!url.is_empty(), |line| {
                                line.child(
                                    div().ml_auto().child(
                                        ui::icon_button(
                                            SharedString::from(format!(
                                                "diff-comment-link-{}",
                                                comment.id
                                            )),
                                            Icon::ExternalLink,
                                            "Open comment in the browser",
                                            ui::ButtonSize::Xs,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.open_link(url.clone(), cx)
                                            }),
                                        ),
                                    ),
                                )
                            }),
                    )
                    .child(div().mt(px(4.0)).child(markdown::render_with(
                        &format!("diff-comment-{}", comment.id),
                        &blocks,
                        Some(&self.opener),
                    ))),
            )
            .into_any_element()
    }

    /// The desktop's `ReplyBox`: a Reply button, which opens a box that sends with ⌘↩ or its button. A failed send
    /// keeps the text and shows the plugin's error.
    fn reply_box(&self, root: i64, cx: &mut Context<Self>) -> AnyElement {
        let Some(reply) = self.replies.get(&root) else {
            return div()
                .mt(px(10.0))
                .ml(px(-8.0))
                .flex()
                .child(
                    div()
                        .id(SharedString::from(format!("diff-reply-{root}")))
                        .children(ui::spot(|| format!("diff-reply-{root}")))
                        .role(Role::Button)
                        .aria_label("Reply")
                        .h(px(24.0))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(rgba(0))
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .cursor_pointer()
                        .focusable()
                        .tab_stop(true)
                        .hover(|style| {
                            style
                                .bg(rgba(theme::HOVER))
                                .text_color(rgb(theme::FOREGROUND))
                        })
                        .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
                        .child(icon(Icon::Reply, 12.0, theme::MUTED_FOREGROUND))
                        .child("Reply")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_reply(root, window, cx)
                        })),
                )
                .into_any_element();
        };
        let empty = reply.field.read(cx).text.trim().is_empty();
        let busy = reply.busy;
        div()
            .mt(px(12.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .capture_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let key = &event.keystroke;
                if key.key == "enter" && (key.modifiers.platform || key.modifiers.control) {
                    this.send_reply(root, cx);
                    cx.stop_propagation();
                }
            }))
            .child(reply.field.clone())
            .when_some(reply.error.clone(), |reply, error| {
                reply.child(
                    div()
                        .id(SharedString::from(format!("diff-reply-error-{root}")))
                        .role(Role::Alert)
                        .text_size(px(12.0))
                        .text_color(rgb(theme::BLOCKED))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.0))
                    .child(
                        small(ui::button(
                            SharedString::from(format!("diff-reply-cancel-{root}")),
                            "Cancel",
                            ui::Variant::Ghost,
                            busy,
                            "Cancel",
                        ))
                        .when(!busy, |button| {
                            button.on_click(
                                cx.listener(move |this, _, _, cx| this.close_reply(root, cx)),
                            )
                        }),
                    )
                    .child(
                        small(ui::button(
                            SharedString::from(format!("diff-reply-send-{root}")),
                            "Reply",
                            ui::Variant::Default,
                            busy || empty,
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .when(busy, |label| {
                                    label.child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "diff-reply-busy-{root}"
                                            )))
                                            .size(px(14.0))
                                            .with_animation(
                                                SharedString::from(format!(
                                                    "diff-reply-turn-{root}"
                                                )),
                                                gpui::Animation::new(Duration::from_secs(1))
                                                    .repeat()
                                                    .with_max_fps(30.0),
                                                |ring, turn| {
                                                    ring.child(ui::working_spinner(
                                                        turn * std::f32::consts::TAU,
                                                    ))
                                                },
                                            ),
                                    )
                                })
                                .child("Reply"),
                        ))
                        .children(ui::spot(|| format!("diff-reply-send-{root}")))
                        .when(!busy && !empty, |button| {
                            button.on_click(
                                cx.listener(move |this, _, _, cx| this.send_reply(root, cx)),
                            )
                        }),
                    ),
            )
            .into_any_element()
    }

    fn open_reply(&mut self, root: i64, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut field = TextField::new(
                Role::MultilineTextInput,
                "Reply",
                "Write a reply",
                px(200.0),
                cx,
            );
            field.fill = true;
            field.rows = Some(3);
            field
        });
        // Escape closes the reply, not the diff; and the Reply button follows what's typed.
        let events = cx.subscribe(&field, move |this, _, event: &FieldEvent, cx| match event {
            FieldEvent::Cancel => this.close_reply(root, cx),
            _ => cx.notify(),
        });
        field.focus_handle(cx).focus(window, cx);
        self.replies.insert(
            root,
            ReplyBox {
                field,
                busy: false,
                error: None,
                _events: events,
                task: None,
                timeout: None,
            },
        );
        cx.notify();
    }

    fn close_reply(&mut self, root: i64, cx: &mut Context<Self>) {
        if self.replies.get(&root).is_some_and(|reply| !reply.busy) {
            self.replies.remove(&root);
            cx.notify();
        }
    }

    /// Hands what's typed to the plugin's `onReply`. The box waits for it, as long as the desktop's 310 s, and closes
    /// once it's posted; the plugin adds the reply to the thread with `updateDiff`.
    pub(super) fn send_reply(&mut self, root: i64, cx: &mut Context<Self>) {
        let Some(reply) = self.replies.get_mut(&root) else {
            return;
        };
        let body = reply.field.read(cx).text.trim().to_owned();
        if body.is_empty() || reply.busy {
            return;
        }
        reply.busy = true;
        reply.error = None;
        reply.field.update(cx, |field, _| field.disabled = true);
        let (sent, received) = async_channel::bounded(1);
        self.host
            .send_reply(&self.view.id, root, &body, move |result| {
                let _ = sent.try_send(result);
            });
        let settle = move |this: &mut Self, result: Result<(), String>, cx: &mut Context<Self>| {
            let Some(reply) = this.replies.get_mut(&root) else {
                return;
            };
            match result {
                Ok(()) => {
                    this.replies.remove(&root);
                }
                Err(error) => {
                    reply.busy = false;
                    reply.error = Some(error);
                    reply.field.update(cx, |field, _| field.disabled = false);
                }
            }
            cx.notify();
        };
        // Each settles the box and stops the other, as the Refresh button's do.
        reply.task = Some(cx.spawn(async move |this, cx| {
            if let Ok(result) = received.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(reply) = this.replies.get_mut(&root) {
                        reply.timeout = None;
                    }
                    settle(this, result, cx)
                });
            }
        }));
        reply.timeout = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(310))
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(reply) = this.replies.get_mut(&root) {
                    reply.task = None;
                }
                settle(this, Err("The plugin didn't respond".into()), cx)
            });
        }));
        cx.notify();
    }

    /// A comment's link, or one in what it says: opened if the plugin's `openUrl` grant covers it, as the desktop's
    /// `plugin_open_url`.
    pub(super) fn open_link(&mut self, url: String, cx: &mut Context<Self>) {
        let host = self.host.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { host.open_url(&url) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.link_error = result
                    .err()
                    .map(|error| format!("Couldn't open the link: {error}"));
                cx.notify();
            });
        })
        .detach();
    }

    #[cfg(feature = "test-support")]
    pub fn reply_error(&self, root: i64) -> Option<String> {
        self.replies
            .get(&root)
            .and_then(|reply| reply.error.clone())
    }

    #[cfg(feature = "test-support")]
    pub fn reply_open(&self, root: i64) -> bool {
        self.replies.contains_key(&root)
    }
}

/// shadcn's `Button` at size `sm`: 28px tall with 12.8px text.
fn small(button: gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div> {
    button
        .h(px(28.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .text_size(px(12.8))
}

/// When a comment was written, as the desktop's `ago`: in days once it's a day or more, else in hours, else in
/// minutes, in `Intl.RelativeTimeFormat`'s English with `numeric: "auto"`. Nothing when it isn't a time.
pub(super) fn ago(created: &str, now: DateTime<Utc>) -> Option<String> {
    let then = DateTime::parse_from_rfc3339(created).ok()?;
    let ms = (then.with_timezone(&Utc) - now).num_milliseconds() as f64;
    // JavaScript's `Math.round`, which rounds halves up.
    let round = |n: f64| (n + 0.5).floor() as i64;
    let (n, unit) = match (round(ms / 86_400_000.0), round(ms / 3_600_000.0)) {
        (days, _) if days.abs() >= 1 => (days, "day"),
        (_, hours) if hours.abs() >= 1 => (hours, "hour"),
        _ => (round(ms / 60_000.0), "minute"),
    };
    Some(match (n, unit) {
        (0, _) => format!("this {unit}"),
        (-1, "day") => "yesterday".into(),
        (1, "day") => "tomorrow".into(),
        (n, _) if n < 0 => format!("{} {unit}{} ago", -n, if n == -1 { "" } else { "s" }),
        (n, _) => format!("in {n} {unit}{}", if n == 1 { "" } else { "s" }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_times_read_as_the_desktop_writes_them() {
        let now = DateTime::parse_from_rfc3339("2026-11-08T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let ago = |at: &str| ago(at, now);
        assert_eq!(ago("2026-11-08T11:59:40Z").as_deref(), Some("this minute"));
        assert_eq!(
            ago("2026-11-08T11:55:00Z").as_deref(),
            Some("5 minutes ago")
        );
        assert_eq!(ago("2026-11-08T11:00:00Z").as_deref(), Some("1 hour ago"));
        assert_eq!(
            ago("2026-11-08T02:00:00+02:00").as_deref(),
            Some("12 hours ago")
        );
        assert_eq!(
            ago("2026-11-07T10:00:00.123Z").as_deref(),
            Some("yesterday")
        );
        assert_eq!(ago("2026-11-01T12:00:00Z").as_deref(), Some("7 days ago"));
        assert_eq!(ago("2026-11-08T14:00:00Z").as_deref(), Some("in 2 hours"));
        assert_eq!(ago("last week"), None);
    }
}
