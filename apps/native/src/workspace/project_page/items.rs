//! The page's sections (Markdown notes, written by you or by an agent, edited in place) and plugin blocks
//! (`sections.tsx`, `blocks.tsx`).

use std::{rc::Rc, time::Duration};

use gpui::{
    AnyElement, ClickEvent, Context, Entity, FontWeight, Role, SharedString, Subscription, Task,
    Window, div, prelude::*, px, rgb, rgba,
};
use wings_spaces::project_info::{Item, MAX_BODY, MAX_TITLE, Op, Who, fingerprint};

use crate::{
    icons::{Icon, icon},
    markdown::{self, Block},
    page::PageView,
    text_field::{FieldEvent, TextField},
    theme,
    ui::{self, ButtonSize, Variant},
};

use super::{
    ProjectPage, mark_style,
    merge::{EditorState, ago, take_stored},
    now_ms,
};

/// A save goes out this long after typing stops.
const SAVE_AFTER: Duration = Duration::from_millis(600);
/// The most a block's page may ask for.
const BLOCK_MAX_HEIGHT: u32 = 1200;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Saving {
    Saved,
    Unsaved,
    Failed(String),
}

/// A section open for writing: its fields, and what it knows of storage.
pub(super) struct SectionEdit {
    title: Entity<TextField>,
    pub(super) body: Entity<TextField>,
    state: EditorState,
    /// The title as typed, which a save sent after the editor closes still needs.
    latest_title: String,
    saving: Saving,
    timer: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

/// An edit of a section's body, from the text the page started from.
fn edit_op(id: &str, body: String, from: &str, title: String) -> Op {
    Op::EditSection {
        id: id.to_owned(),
        title: Some(title),
        body: Some(body),
        base_len: from.encode_utf16().count(),
        base_hash: fingerprint(from),
    }
}

impl ProjectPage {
    /// The Markdown of a section, parsed once per text.
    fn blocks_of(&mut self, id: &str, body: &str) -> Rc<Vec<Block>> {
        match self.markdown.get(id) {
            Some((text, blocks)) if text == body => blocks.clone(),
            _ => {
                let blocks = Rc::new(markdown::parse(body));
                self.markdown
                    .insert(id.to_owned(), (body.to_owned(), blocks.clone()));
                blocks
            }
        }
    }

    fn stored_body(&self, id: &str) -> Option<String> {
        self.data
            .as_ref()?
            .items
            .iter()
            .find_map(|item| match item {
                Item::Section {
                    id: found, body, ..
                } if found == id => Some(body.clone()),
                _ => None,
            })
    }

    /// Storage changed: an open editor takes what an agent added, and one whose section is gone closes.
    pub(super) fn sections_changed(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.editors.keys().cloned().collect();
        for id in ids {
            let Some(stored) = self.stored_body(&id) else {
                self.editors.remove(&id);
                continue;
            };
            let Some(edit) = self.editors.get_mut(&id) else {
                continue;
            };
            if let Some((text, base)) = take_stored(&edit.state, &stored) {
                edit.state.text = text.clone();
                edit.state.base = base;
                edit.body.update(cx, |field, cx| {
                    field.text = text;
                    cx.notify();
                });
            }
        }
        if let Some(data) = &self.data {
            self.markdown
                .retain(|id, _| data.items.iter().any(|item| item.id() == id));
        }
    }

    pub(super) fn edit_section(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let Some((title, body)) = data.items.iter().find_map(|item| match item {
            Item::Section {
                id: found,
                title,
                body,
                ..
            } if found == id => Some((title.clone(), body.clone())),
            _ => None,
        }) else {
            return;
        };
        let title_field = cx.new(|cx| {
            let mut field = TextField::new(
                Role::TextInput,
                "Section title",
                "Title",
                gpui::px(120.0),
                cx,
            );
            field.fill = true;
            field.max_chars = Some(MAX_TITLE);
            field.text = title.clone();
            field
        });
        let body_field = cx.new(|cx| {
            let mut field = TextField::new(
                Role::MultilineTextInput,
                format!("{title}, Markdown"),
                "Markdown: lists, tables, [links](https://example.com).",
                gpui::px(120.0),
                cx,
            );
            field.fill = true;
            field.rows = Some(12);
            field.max_chars = Some(MAX_BODY);
            field.text = body.clone();
            field
        });
        let item = id.to_owned();
        let mut subs = Vec::new();
        for field in [&title_field, &body_field] {
            let item = item.clone();
            subs.push(cx.subscribe_in(
                field,
                window,
                move |this, _, event, window, cx| match event {
                    FieldEvent::Changed => this.touched(&item, cx),
                    FieldEvent::Cancel => this.finish_edit(&item, window, cx),
                    FieldEvent::Submit { .. } => {}
                },
            ));
        }
        body_field.update(cx, |field, cx| {
            gpui::Focusable::focus_handle(field, cx).focus(window, cx)
        });
        self.editors.insert(
            id.to_owned(),
            SectionEdit {
                title: title_field,
                body: body_field,
                state: EditorState {
                    base: body.clone(),
                    text: body,
                    ..EditorState::default()
                },
                latest_title: title,
                saving: Saving::Saved,
                timer: None,
                _subs: subs,
            },
        );
        cx.notify();
    }

    /// Typing costs no write: a save goes out after typing stops.
    pub(super) fn touched(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(edit) = self.editors.get_mut(id) else {
            return;
        };
        edit.state.text = edit.body.read(cx).text.clone();
        edit.latest_title = edit.title.read(cx).text.clone();
        edit.state.dirty = true;
        edit.saving = Saving::Unsaved;
        let id = id.to_owned();
        edit.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            this.update(cx, |this, cx| this.flush(&id, cx)).ok();
        }));
        cx.notify();
    }

    /// Sends what was typed. One save at a time: a second would be merged against a base the first already changed.
    fn flush(&mut self, id: &str, cx: &mut Context<Self>) {
        let stale = self.stored_body(id).unwrap_or_default();
        let Some(edit) = self.editors.get_mut(id) else {
            return;
        };
        edit.timer = None;
        if !edit.state.dirty || edit.state.inflight {
            return;
        }
        edit.state.dirty = false;
        edit.state.inflight = true;
        edit.state.sent = true;
        edit.state.stale = stale;
        let (body, from, title) = (
            edit.state.text.clone(),
            edit.state.base.clone(),
            edit.latest_title.clone(),
        );
        let op = edit_op(id, body.clone(), &from, title);
        let id = id.to_owned();
        self.apply(op, cx, move |this, result, cx| {
            let again = match this.editors.get_mut(&id) {
                Some(edit) => {
                    edit.state.inflight = false;
                    match result {
                        Ok(_) => {
                            edit.state.base = body;
                            if !edit.state.dirty {
                                edit.saving = Saving::Saved;
                            }
                            edit.state.dirty
                        }
                        Err(error) => {
                            edit.state.dirty = true;
                            edit.saving = Saving::Failed(error);
                            false
                        }
                    }
                }
                None => false,
            };
            if again {
                this.flush(&id, cx);
            }
            this.sections_changed(cx);
        });
    }

    pub(super) fn finish_edit(&mut self, id: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.flush(id, cx);
        // A save still out answers to a closed editor, which it finds gone.
        self.editors.remove(id);
        cx.notify();
    }

    pub(super) fn section_card(
        &mut self,
        item: &Item,
        first: bool,
        last: bool,
        mark: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Item::Section {
            id,
            title,
            body,
            by,
            updated_at,
        } = item
        else {
            return div().into_any_element();
        };
        if self.fresh.as_deref() == Some(id.as_str()) {
            self.fresh = None;
            self.edit_section(id, window, cx);
        }
        let card = div()
            .id(SharedString::from(format!("item-{id}")))
            .role(Role::Article)
            .aria_label(title.clone())
            .group(SharedString::from(format!("section-{id}")))
            .rounded(px(12.0))
            .border_1()
            .border_color(rgba(ui::HAIRLINE))
            .bg(rgba(0xffffff05))
            .px(px(20.0))
            .py(px(16.0))
            .hover(|style| style.border_color(rgba(theme::HAIRLINE_STRONG)));
        if self.editors.contains_key(id) {
            return mark_style(card, mark)
                .child(self.section_editor(id, cx))
                .into_any_element();
        }
        let group = SharedString::from(format!("section-{id}"));
        let confirm = self.removing.as_deref() == Some(id.as_str());
        let actions = self.item_actions(id, title, first, last, confirm, "Delete section", cx);
        let edit_id = id.clone();
        let edit = ui::icon_button(
            SharedString::from(format!("edit-{id}")),
            Icon::Pencil,
            format!("Edit {title}"),
            ButtonSize::Xs,
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.edit_section(&edit_id, window, cx)
        }));
        let content = if body.trim().is_empty() {
            let edit_id = id.clone();
            div()
                .id(SharedString::from(format!("empty-{id}")))
                .role(Role::Button)
                .cursor_pointer()
                .text_size(px(13.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .hover(|style| style.text_color(rgb(theme::FOREGROUND)))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.edit_section(&edit_id, window, cx)
                }))
                .child("Empty. Press Edit to write.")
                .into_any_element()
        } else {
            let blocks = self.blocks_of(id, body);
            markdown::render(&format!("md-{id}"), &blocks)
        };
        let now = now_ms();
        let when = ago(*updated_at, now);
        mark_style(card, mark)
            .child(
                div()
                    .mb(px(8.0))
                    .min_h(px(32.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(16.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .when(!confirm, |a| {
                                a.opacity(0.0).group_hover(group, |s| s.opacity(1.0))
                            })
                            .child(actions.0)
                            .child(actions.1)
                            .child(edit)
                            .child(actions.2),
                    ),
            )
            .child(content)
            .child(
                div()
                    .mt(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .text_size(px(11.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(icon(
                        if *by == Who::Agent {
                            Icon::Sparkles
                        } else {
                            Icon::User
                        },
                        12.0,
                        theme::MUTED_FOREGROUND,
                    ))
                    .child(if *by == Who::Agent {
                        "Written by an agent"
                    } else {
                        "Written by you"
                    })
                    .child("\u{b7}")
                    .child(when),
            )
            .into_any_element()
    }

    /// The move up, move down and delete controls of an item, the delete one asking first.
    #[allow(clippy::too_many_arguments)]
    fn item_actions(
        &self,
        id: &str,
        title: &str,
        first: bool,
        last: bool,
        confirm: bool,
        delete_label: &'static str,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement, AnyElement) {
        let mv = |dir: i32, glyph: Icon, label: String, off: bool, cx: &mut Context<Self>| {
            let target = id.to_owned();
            let button = ui::icon_button(
                SharedString::from(format!("move-{dir}-{id}")),
                glyph,
                label,
                ButtonSize::Xs,
            );
            if off {
                button.opacity(0.4).cursor_default().into_any_element()
            } else {
                button
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.act(
                            Op::MoveItem {
                                id: target.clone(),
                                dir,
                            },
                            cx,
                        )
                    }))
                    .into_any_element()
            }
        };
        let up = mv(-1, Icon::ArrowUp, format!("Move {title} up"), first, cx);
        let down = mv(1, Icon::ArrowDown, format!("Move {title} down"), last, cx);
        let target = id.to_owned();
        let remove = if confirm {
            let delete_id = id.to_owned();
            div()
                .flex()
                .items_center()
                .child(
                    ui::button(
                        SharedString::from(format!("delete-{id}")),
                        delete_label,
                        Variant::Destructive,
                        false,
                        delete_label,
                    )
                    .h(px(24.0))
                    .px(px(8.0))
                    .text_size(px(12.0))
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.removing = None;
                            this.act(
                                Op::DeleteItem {
                                    id: delete_id.clone(),
                                },
                                cx,
                            )
                        },
                    )),
                )
                .child(
                    ui::button(
                        SharedString::from(format!("keep-{id}")),
                        "Keep",
                        Variant::Ghost,
                        false,
                        "Keep",
                    )
                    .h(px(24.0))
                    .px(px(8.0))
                    .text_size(px(12.0))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.removing = None;
                        cx.notify();
                    })),
                )
                .into_any_element()
        } else {
            ui::icon_button(
                SharedString::from(format!("remove-{id}")),
                Icon::Trash,
                format!("Delete {title}"),
                ButtonSize::Xs,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.removing = Some(target.clone());
                cx.notify();
            }))
            .into_any_element()
        };
        (up, down, remove)
    }

    fn section_editor(&self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(edit) = self.editors.get(id) else {
            return div().into_any_element();
        };
        let done_id = id.to_owned();
        let (text, failed) = match &edit.saving {
            Saving::Saved => ("Saved".to_owned(), false),
            Saving::Unsaved => ("Saving...".to_owned(), false),
            Saving::Failed(error) => (
                format!("Couldn't save: {error} Your text is still here."),
                true,
            ),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().min_w_0().flex_1().child(edit.title.clone()))
                    .child(
                        ui::button(
                            SharedString::from(format!("done-{id}")),
                            "Done",
                            Variant::Secondary,
                            false,
                            "Done",
                        )
                        .h(px(28.0))
                        .text_size(px(12.5))
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, window, cx| {
                                this.finish_edit(&done_id, window, cx)
                            },
                        )),
                    ),
            )
            .child(edit.body.clone())
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(rgb(if failed {
                        theme::DANGER
                    } else {
                        theme::MUTED_FOREGROUND
                    }))
                    .child(text),
            )
            .into_any_element()
    }

    /// A plugin's block: a title bar with move and delete, then the plugin's page in its own frame.
    pub(super) fn block_item(
        &mut self,
        item: &Item,
        first: bool,
        last: bool,
        mark: Option<bool>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Item::Block {
            id,
            plugin_id,
            block_id,
            height,
            state,
            ..
        } = item
        else {
            return div().into_any_element();
        };
        let source = self
            .blocks
            .iter()
            .find(|b| &b.plugin == plugin_id && &b.id == block_id)
            .cloned();
        let title = source.as_ref().map_or_else(
            || format!("Missing block \u{b7} {plugin_id}/{block_id}"),
            |b| b.title.clone(),
        );
        let group = SharedString::from(format!("block-{id}"));
        let confirm = self.removing.as_deref() == Some(id.as_str());
        let (up, down, remove) =
            self.item_actions(id, &title, first, last, confirm, "Remove block", cx);
        let body: AnyElement = match source {
            Some(source) => {
                let visible = self.visible;
                let store = self.info.store.clone();
                let project = self.id.clone();
                let block = id.clone();
                let plugin = plugin_id.clone();
                let block_kind = block_id.clone();
                let initial = state.clone();
                let view: Entity<PageView> = self
                    .block_views
                    .entry(id.clone())
                    .or_insert_with(|| {
                        let state = crate::page::plugin::FrameState::new(
                            crate::page::plugin::StatePlace::Block,
                            initial,
                            move |value| {
                                let info = store.get(&project)?;
                                if !info.items.iter().any(|item| matches!(item,
                                    Item::Block { id, plugin_id, block_id, .. }
                                    if id == &block && plugin_id == &plugin && block_id == &block_kind)) {
                                    return Err("The plugin's block is no longer on this Project page".into());
                                }
                                store.apply(&project, Op::SetBlockState { id: block.clone(), state: Some(value) })?;
                                Ok(())
                            },
                        );
                        cx.new(|cx| PageView::in_plugin_with_state(&source.host, &source.source, None, state, cx))
                    })
                    .clone();
                view.read(cx).set_visible(visible);
                let px_height = height.unwrap_or(source.min_height).clamp(source.min_height, BLOCK_MAX_HEIGHT);
                div()
                    .h(px(px_height as f32))
                    .w_full()
                    .overflow_hidden()
                    .rounded(px(14.0))
                    .border_1()
                    .border_color(rgba(ui::HAIRLINE))
                    .bg(rgb(theme::SURFACE))
                    .child(view)
                    .into_any_element()
            }
            None => div()
                .rounded(px(12.0))
                .border_1()
                .border_color(rgba(theme::HAIRLINE_STRONG))
                .px(px(20.0))
                .py(px(16.0))
                .text_size(px(13.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(format!(
                    "The plugin for this block ({plugin_id}) is off, has no native build of it, or no longer offers it. Turn it on in the desktop app's Settings, or remove the block."
                ))
                .into_any_element(),
        };
        let known = self
            .blocks
            .iter()
            .any(|b| &b.plugin == plugin_id && &b.id == block_id);
        mark_style(
            div()
                .id(SharedString::from(format!("item-{id}")))
                .role(Role::Article)
                .aria_label(title.clone())
                .group(group.clone())
                .rounded(px(12.0))
                .flex()
                .flex_col()
                .gap(px(6.0)),
            mark,
        )
        .child(
            div()
                .min_h(px(32.0))
                .px(px(4.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(icon(Icon::Puzzle, 14.0, theme::MUTED_FOREGROUND))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(title),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .when(known && !confirm, |a| {
                            a.opacity(0.0).group_hover(group, |s| s.opacity(1.0))
                        })
                        .child(up)
                        .child(down)
                        .child(remove),
                ),
        )
        .child(body)
        .into_any_element()
    }
}
