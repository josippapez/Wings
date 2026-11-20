//! Comments the user starts on a line, for a plugin whose diff was opened with `allowNewComments`: a "+" in a line's
//! gutter on hover opens a box under the line, Enter (or Cmd-Enter) saves it to the plugin's `onComment`, and Escape
//! cancels. A saved comment shows at once as a draft thread; the plugin may add it to its own `comments` with the
//! draft's id, which replaces the draft.

use super::{
    Viewer,
    threads::{CARD, small},
};
use crate::{
    diff::{comments::Comment, rows::file_path},
    icons::Icon,
    text_field::{FieldEvent, TextField},
    theme, ui,
    ui::behaviors::EscapeExt,
};
use chrono::Utc;
use gpui::{
    AnyElement, BoxShadow, Context, Entity, Focusable, KeyDownEvent, Role, SharedString,
    Subscription, Window, div, hsla, point, prelude::*, px, rgb, rgba,
};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

/// What stands in `Notes` for the compose box, where the others are indexes of threads.
pub(super) const COMPOSE: usize = usize::MAX;

/// The box open under a line.
pub(super) struct Compose {
    /// The file's path in the patch.
    pub path: String,
    /// On the removed side's line number, else the added side's.
    pub old: bool,
    pub line: usize,
    /// The line's text.
    pub quote: String,
    field: Entity<TextField>,
    _events: Subscription,
}

impl Viewer {
    /// The plugin takes comments started here.
    pub(super) fn allows_comments(&self) -> bool {
        self.view.update.allow_new_comments == Some(true)
    }

    /// The "+" in the gutter of a line, hidden until the row is hovered. `row` is the code row's index (it names the
    /// hover group the row sets) and `at` the line's hunk and place in it.
    pub(super) fn add_comment_button(
        &self,
        row: usize,
        at: (usize, usize),
        old: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.allows_comments() {
            return None;
        }
        let name = format!("diff-add-{row}-{}", if old { "old" } else { "new" });
        let (hunk, line) = at;
        Some(
            div()
                .absolute()
                .top(px(1.0))
                .left(px(2.0))
                .opacity(0.0)
                .group_hover(SharedString::from(format!("diff-line-{row}")), |style| {
                    style.opacity(1.0)
                })
                .child(
                    ui::icon_button(
                        SharedString::from(name.clone()),
                        Icon::Plus,
                        "Comment on this line",
                        ui::ButtonSize::Tiny,
                    )
                    .children(ui::spot(|| name))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_compose(hunk, line, old, window, cx)
                    })),
                )
                .into_any_element(),
        )
    }

    /// Opens the box under the line `line` of hunk `hunk` of the open file, on its removed side or its added one.
    pub(super) fn open_compose(
        &mut self,
        hunk: usize,
        line: usize,
        old: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.allows_comments() {
            return;
        }
        let Some(file) = self
            .view
            .update
            .patch
            .as_ref()
            .and_then(|diff| diff.files.get(self.selected))
        else {
            return;
        };
        let Some(text) = file.hunks.get(hunk).and_then(|h| h.lines.get(line)) else {
            return;
        };
        let number = if old { text.old_line } else { text.new_line };
        let Some(number) = number else {
            return;
        };
        let (path, quote) = (file_path(file).to_owned(), text.text.clone());
        let field = cx.new(|cx| {
            let mut field = TextField::new(
                Role::MultilineTextInput,
                "Comment",
                "Leave a comment",
                px(200.0),
                cx,
            );
            field.fill = true;
            field.rows = Some(3);
            // Enter saves, Shift-Enter starts a new line.
            field.enter_submits = true;
            field
        });
        let events = cx.subscribe_in(
            &field,
            window,
            move |this, _, event: &FieldEvent, window, cx| match event {
                FieldEvent::Submit { back: false } => this.save_compose(window, cx),
                FieldEvent::Cancel => this.close_compose(window, cx),
                _ => cx.notify(),
            },
        );
        field.focus_handle(cx).focus(window, cx);
        self.compose = Some(Compose {
            path,
            old,
            line: number,
            quote,
            field,
            _events: events,
        });
        self.rethread();
        cx.notify();
    }

    pub(super) fn close_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.compose.take().is_some() {
            self.code_focus.focus(window, cx);
            self.rethread();
            cx.notify();
        }
    }

    /// Saves what's typed: it shows as a draft under its line and goes to the plugin's `onComment`.
    pub(super) fn save_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(compose) = &self.compose else {
            return;
        };
        let body = compose.field.read(cx).text.trim().to_owned();
        if body.is_empty() {
            return;
        }
        let compose = self.compose.take().unwrap();
        self.code_focus.focus(window, cx);
        self.add_draft(
            &compose.path,
            compose.old,
            compose.line,
            &compose.quote,
            &body,
            cx,
        );
    }

    /// Adds a draft on a line and hands it to the plugin.
    fn add_draft(
        &mut self,
        path: &str,
        old: bool,
        line: usize,
        quote: &str,
        body: &str,
        cx: &mut Context<Self>,
    ) -> i64 {
        let id = self.next_draft;
        self.next_draft -= 1;
        let draft = Comment {
            id,
            reply_to: None,
            path: Some(path.to_owned()),
            line: Some(line),
            old,
            author: "You".into(),
            body: body.to_owned(),
            created_at: Utc::now().to_rfc3339(),
            url: String::new(),
            can_reply: false,
            draft: true,
        };
        self.drafts.push(draft.clone());
        let list = self
            .view
            .update
            .comments
            .get_or_insert_with(|| Arc::new(Vec::new()));
        Arc::make_mut(list).push(draft);
        let fields = json!({
            "path": path,
            "side": if old { "old" } else { "new" },
            "line": line,
            "quote": quote,
            "body": body,
            "id": id,
        });
        self.send_comment(fields, Some(id), cx);
        self.rethread();
        cx.notify();
        id
    }

    /// Takes a draft back: gone from the viewer, and the plugin hears `{ delete: true, id }`.
    pub(super) fn delete_draft(&mut self, id: i64, cx: &mut Context<Self>) {
        if !self.drafts.iter().any(|draft| draft.id == id) {
            return;
        }
        self.drafts.retain(|draft| draft.id != id);
        self.draft_errors.remove(&id);
        if let Some(list) = self.view.update.comments.as_mut() {
            Arc::make_mut(list).retain(|comment| comment.id != id);
        }
        self.send_comment(json!({ "delete": true, "id": id }), None, cx);
        self.rethread();
        cx.notify();
    }

    /// Hands `fields` to the plugin. Its error shows under the draft `draft` names.
    fn send_comment(&mut self, fields: Value, draft: Option<i64>, cx: &mut Context<Self>) {
        #[cfg(feature = "test-support")]
        self.sent_comments.push(fields.clone());
        let (sent, received) = async_channel::bounded(1);
        self.host.send_comment(&self.view.id, fields, move |result| {
            let _ = sent.try_send(result);
        });
        cx.spawn(async move |this, cx| {
            if let (Ok(Err(error)), Some(id)) = (received.recv().await, draft) {
                let _ = this.update(cx, |this, cx| {
                    if this.drafts.iter().any(|draft| draft.id == id) {
                        this.draft_errors.insert(id, error);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// The plugin sent its comments: a draft it echoed back (same id) is its own now, and the rest stay.
    pub(super) fn keep_drafts(&mut self) {
        let Some(list) = self.view.update.comments.as_mut() else {
            return;
        };
        let ids: HashSet<i64> = list.iter().map(|comment| comment.id).collect();
        self.drafts.retain(|draft| !ids.contains(&draft.id));
        let kept: HashSet<i64> = self.drafts.iter().map(|draft| draft.id).collect();
        self.draft_errors.retain(|id, _| kept.contains(id));
        if !self.drafts.is_empty() {
            Arc::make_mut(list).extend(self.drafts.iter().cloned());
        }
    }

    /// The box under a line: where it's for, what's typed, Cancel and Comment. It looks like a thread's card.
    pub(super) fn compose_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(compose) = &self.compose else {
            return div().into_any_element();
        };
        let empty = compose.field.read(cx).text.trim().is_empty();
        let weak = cx.entity().downgrade();
        div()
            .id("diff-compose")
            .children(ui::spot(|| "diff-compose".into()))
            .mt(px(6.0))
            .mb(px(6.0))
            .mr(px(12.0))
            .ml(px(48.0))
            .max_w(px(768.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
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
            .capture_escape(move |_, window, cx| {
                weak.update(cx, |this, cx| this.close_compose(window, cx))
                    .is_ok()
            })
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = &event.keystroke;
                if key.key == "enter" && (key.modifiers.platform || key.modifiers.control) {
                    this.save_compose(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(format!(
                        "Comment on {} line {}",
                        if compose.old { "removed" } else { "added or unchanged" },
                        compose.line
                    )),
            )
            .child(compose.field.clone())
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.0))
                    .child(
                        small(ui::button(
                            "diff-compose-cancel",
                            "Cancel",
                            ui::Variant::Ghost,
                            false,
                            "Cancel",
                        ))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.close_compose(window, cx)),
                        ),
                    )
                    .child(
                        small(ui::button(
                            "diff-compose-save",
                            "Comment",
                            ui::Variant::Default,
                            empty,
                            "Comment",
                        ))
                        .when(!empty, |button| {
                            button.on_click(
                                cx.listener(|this, _, window, cx| this.save_compose(window, cx)),
                            )
                        }),
                    ),
            )
            .into_any_element()
    }

    /// Under a draft's text: why the plugin refused it, and Delete.
    pub(super) fn draft_footer(&self, id: i64, cx: &mut Context<Self>) -> AnyElement {
        div()
            .mt(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .when_some(self.draft_errors.get(&id).cloned(), |footer, error| {
                footer.child(
                    div()
                        .id(SharedString::from(format!("diff-draft-error-{id}")))
                        .role(Role::Alert)
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.0))
                        .text_color(rgb(theme::BLOCKED))
                        .child(error),
                )
            })
            .child(div().flex_1())
            .child(small(ui::button(
                SharedString::from(format!("diff-draft-delete-{id}")),
                "Delete",
                ui::Variant::Ghost,
                false,
                "Delete",
            ))
            .on_click(cx.listener(move |this, _, _, cx| this.delete_draft(id, cx))))
            .into_any_element()
    }

    /// For the screenshots: a box open under a line, as the "+" opens it.
    #[cfg(feature = "test-support")]
    pub fn shot_compose(
        &mut self,
        hunk: usize,
        line: usize,
        old: bool,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_compose(hunk, line, old, window, cx);
        if let Some(compose) = &self.compose {
            compose
                .field
                .update(cx, |field, _| field.text = text.to_owned());
        }
        cx.notify();
    }

    /// For the screenshots: a draft saved on a line.
    #[cfg(feature = "test-support")]
    pub fn shot_draft(
        &mut self,
        path: &str,
        old: bool,
        line: usize,
        quote: &str,
        body: &str,
        cx: &mut Context<Self>,
    ) {
        self.add_draft(path, old, line, quote, body, cx);
    }
}
