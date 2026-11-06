//! The facts: a table of key and value, each row editable in place, with a row under it to add one
//! (`facts.tsx`). Agents write facts too; a fact an agent wrote can be changed by you, and an agent can't change yours.

use std::collections::HashMap;

use gpui::{
    AnyElement, ClickEvent, Context, Entity, FontWeight, Role, SharedString, Subscription, Window,
    div, prelude::*, px, rgb, rgba,
};
use wings_spaces::project_info::{Fact, Info, MAX_FACTS, MAX_KEY, MAX_VALUE, Op, Who};

use crate::{
    icons::Icon,
    markdown,
    text_field::{FieldEvent, TextField},
    theme,
    ui::{self, ButtonSize, Variant},
};

use super::{
    ProjectPage,
    merge::{Match, find_fact},
};

struct FactEdit {
    /// The key being edited, as stored.
    key: String,
    name: Entity<TextField>,
    value: Entity<TextField>,
    _subs: Vec<Subscription>,
}

/// The fact forms and what they last said.
pub(super) struct FactState {
    /// The row under the table that adds a fact.
    pub(super) name: Entity<TextField>,
    pub(super) value: Entity<TextField>,
    edit: Option<FactEdit>,
    removing: Option<String>,
    error: Option<SharedString>,
}

fn field(
    label: &str,
    placeholder: &str,
    max: usize,
    cx: &mut Context<ProjectPage>,
) -> Entity<TextField> {
    cx.new(|cx| {
        let mut field = TextField::new(
            Role::TextInput,
            label.to_owned(),
            placeholder.to_owned(),
            px(120.0),
            cx,
        );
        field.fill = true;
        field.max_chars = Some(max);
        field
    })
}

impl FactState {
    pub(super) fn new(cx: &mut Context<ProjectPage>) -> Self {
        Self {
            name: field("Fact name", "Name", MAX_KEY, cx),
            value: field("Fact value", "Value", MAX_VALUE, cx),
            edit: None,
            removing: None,
            error: None,
        }
    }

    /// Enter in either field of the add row adds the fact.
    pub(super) fn listen(&self, subs: &mut Vec<Subscription>, cx: &mut Context<ProjectPage>) {
        for field in [&self.name, &self.value] {
            subs.push(cx.subscribe(field, |this, _, event, cx| {
                if matches!(event, FieldEvent::Submit { .. }) {
                    this.submit_fact(cx);
                }
            }));
        }
    }
}

impl ProjectPage {
    pub(super) fn facts_has_room(&self) -> bool {
        self.data
            .as_ref()
            .is_some_and(|info| info.facts.len() < MAX_FACTS)
    }

    /// Scrolls to the add row and puts the caret in its name field.
    pub(super) fn focus_fact_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scroll_to = Some(Match::Fact(String::new()));
        self.facts.name.update(cx, |field, cx| {
            gpui::Focusable::focus_handle(field, cx).focus(window, cx)
        });
        cx.notify();
    }

    pub(super) fn submit_fact(&mut self, cx: &mut Context<Self>) {
        let (key, value) = (
            self.facts.name.read(cx).text.clone(),
            self.facts.value.read(cx).text.clone(),
        );
        if key.trim().is_empty() {
            return;
        }
        // `setFact` replaces a fact with the same key, so adding over one would silently overwrite it.
        if let Some(existing) = self.data.as_ref().and_then(|d| find_fact(&d.facts, &key)) {
            let existing = existing.key.clone();
            self.facts.error = Some(format!("{existing} already exists, edit it instead.").into());
            self.start_edit(&existing, cx);
            cx.notify();
            return;
        }
        let op = Op::SetFact {
            key,
            value,
            by: Who::You,
            old_key: None,
        };
        self.apply(op, cx, |this, result, cx| match result {
            Ok(_) => {
                this.facts.error = None;
                for field in [&this.facts.name, &this.facts.value] {
                    field.update(cx, |field, cx| {
                        field.text.clear();
                        cx.notify();
                    });
                }
            }
            Err(error) => this.facts.error = Some(error.into()),
        });
    }

    fn start_edit(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(fact) = self
            .data
            .as_ref()
            .and_then(|d| d.facts.iter().find(|f| f.key == key))
        else {
            return;
        };
        let (name, value) = (
            field("Fact name", "Dev server", MAX_KEY, cx),
            field("Fact value", "pnpm dev, port 5173", MAX_VALUE, cx),
        );
        name.update(cx, |f, _| f.text = fact.key.clone());
        value.update(cx, |f, _| f.text = fact.value.clone());
        let subs = [&name, &value]
            .into_iter()
            .map(|field| {
                cx.subscribe(field, |this, _, event, cx| match event {
                    FieldEvent::Submit { .. } => this.save_edit(cx),
                    FieldEvent::Cancel => this.cancel_edit(cx),
                    FieldEvent::Changed => {}
                })
            })
            .collect();
        self.facts.edit = Some(FactEdit {
            key: key.to_owned(),
            name,
            value,
            _subs: subs,
        });
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.facts.edit = None;
        self.facts.error = None;
        cx.notify();
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &self.facts.edit else { return };
        let (key, value) = (
            edit.name.read(cx).text.clone(),
            edit.value.read(cx).text.clone(),
        );
        if key.trim().is_empty() {
            return;
        }
        let op = Op::SetFact {
            key,
            value,
            by: Who::You,
            old_key: Some(edit.key.clone()),
        };
        self.apply(op, cx, |this, result, _| match result {
            Ok(_) => {
                this.facts.edit = None;
                this.facts.error = None;
            }
            Err(error) => this.facts.error = Some(error.into()),
        });
    }

    fn remove_fact(&mut self, key: String, cx: &mut Context<Self>) {
        let op = Op::RemoveFact { key, by: Who::You };
        self.apply(op, cx, |this, result, _| {
            this.facts.removing = None;
            this.facts.error = result.err().map(Into::into);
        });
    }

    pub(super) fn facts_section(
        &self,
        data: &Info,
        marks: &HashMap<String, bool>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let heading = div()
            .px(px(4.0))
            .flex()
            .items_baseline()
            .gap(px(8.0))
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(theme::MUTED_FOREGROUND))
            .child("Facts")
            .when(!data.facts.is_empty(), |h| {
                h.child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::NORMAL)
                        .opacity(0.7)
                        .child(data.facts.len().to_string()),
                )
            });
        let rows: Vec<AnyElement> = data
            .facts
            .iter()
            .enumerate()
            .map(|(i, fact)| match &self.facts.edit {
                Some(edit) if edit.key == fact.key => self.fact_form(edit, cx),
                _ => self.fact_row(i, fact, marks.get(&fact.key).copied(), cx),
            })
            .collect();
        let add = if data.facts.len() < MAX_FACTS {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(160.0))
                        .flex_none()
                        .child(self.facts.name.clone()),
                )
                .child(
                    div()
                        .min_w(px(192.0))
                        .flex_1()
                        .child(self.facts.value.clone()),
                )
                .child(
                    ui::button("add-fact", "Add fact", Variant::Ghost, false, "Add fact")
                        .h(px(28.0))
                        .text_size(px(12.5))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.submit_fact(cx))),
                )
                .into_any_element()
        } else {
            div()
                .text_size(px(12.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(format!("A project holds at most {MAX_FACTS} facts."))
                .into_any_element()
        };
        div()
            .id("project-facts")
            .role(Role::Group)
            .aria_label("Facts")
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(heading)
            .children(rows)
            .child(add)
            .children(self.facts.error.clone().map(|error| {
                div()
                    .text_size(px(12.0))
                    .text_color(rgb(theme::DANGER))
                    .child(error)
            }))
            .into_any_element()
    }

    fn fact_form(&self, edit: &FactEdit, cx: &mut Context<Self>) -> AnyElement {
        div()
            .py(px(4.0))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.0))
            .child(div().w(px(160.0)).flex_none().child(edit.name.clone()))
            .child(div().min_w(px(192.0)).flex_1().child(edit.value.clone()))
            .child(
                ui::button("save-fact", "Save", Variant::Secondary, false, "Save")
                    .h(px(28.0))
                    .text_size(px(12.5))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save_edit(cx))),
            )
            .child(
                ui::button("cancel-fact", "Cancel", Variant::Ghost, false, "Cancel")
                    .h(px(28.0))
                    .text_size(px(12.5))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_edit(cx))),
            )
            .into_any_element()
    }

    fn fact_row(
        &self,
        index: usize,
        fact: &Fact,
        mark: Option<bool>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group = SharedString::from(format!("fact-{index}"));
        let removing = self.facts.removing.as_deref() == Some(fact.key.as_str());
        let by = if fact.by == Who::Agent {
            "agent"
        } else {
            "you"
        };
        let (edit_key, remove_key, confirm_key) =
            (fact.key.clone(), fact.key.clone(), fact.key.clone());
        let actions = div()
            .w(px(104.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .when(!removing, |a| {
                a.opacity(0.0)
                    .group_hover(group.clone(), |s| s.opacity(1.0))
            })
            .child(
                div()
                    .mr(px(4.0))
                    .text_size(px(11.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(by),
            )
            .child(
                ui::icon_button(
                    SharedString::from(format!("fact-{index}-edit")),
                    Icon::Pencil,
                    format!("Edit {}", fact.key),
                    ButtonSize::Xs,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.start_edit(&edit_key, cx);
                    cx.notify();
                })),
            )
            .children(if removing {
                vec![
                    ui::button(
                        SharedString::from(format!("fact-{index}-remove")),
                        "Remove",
                        Variant::Destructive,
                        false,
                        "Remove",
                    )
                    .h(px(24.0))
                    .px(px(8.0))
                    .text_size(px(12.0))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.remove_fact(confirm_key.clone(), cx)
                    }))
                    .into_any_element(),
                    ui::button(
                        SharedString::from(format!("fact-{index}-keep")),
                        "Keep",
                        Variant::Ghost,
                        false,
                        "Keep",
                    )
                    .h(px(24.0))
                    .px(px(8.0))
                    .text_size(px(12.0))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.facts.removing = None;
                        cx.notify();
                    }))
                    .into_any_element(),
                ]
            } else {
                vec![
                    ui::icon_button(
                        SharedString::from(format!("fact-{index}-delete")),
                        Icon::Trash,
                        format!("Remove {}", fact.key),
                        ButtonSize::Xs,
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.facts.removing = Some(remove_key.clone());
                        cx.notify();
                    }))
                    .into_any_element(),
                ]
            });
        let row = div()
            .id(SharedString::from(format!("fact-row-{index}")))
            .group(group)
            .flex()
            .items_start()
            .when(index > 0, |r| {
                r.border_t_1().border_color(rgba(ui::HAIRLINE))
            })
            .rounded(px(8.0))
            .hover(|style| style.bg(rgba(0xffffff09)))
            .child(
                div()
                    .w(px(160.0))
                    .flex_none()
                    .px(px(8.0))
                    .py(px(6.0))
                    .text_size(px(12.5))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(fact.key.clone()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .px(px(8.0))
                    .py(px(6.0))
                    .text_size(px(13.0))
                    .child(markdown::inline_element(
                        format!("fact-value-{index}"),
                        &markdown::inline(&fact.value),
                    )),
            )
            .child(div().py(px(4.0)).pr(px(4.0)).child(actions));
        super::mark_style(row, mark).into_any_element()
    }
}
