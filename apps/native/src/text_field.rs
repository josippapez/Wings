//! A one-line text field for the find bar and pane names. GPUI has no text input, so this one takes
//! typed and composed text, Backspace, paste and select-all, and leaves the rest to its owner.

use std::ops::Range;

use gpui::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, KeyDownEvent, Pixels, Point, Role, SharedString, UTF16Selection, Window, canvas,
    div, prelude::*, px, rgb, rgba,
};

use crate::{
    actions::Paste,
    theme::{FOREGROUND, MUTED_FOREGROUND},
};

/// The desktop terminal's `selectionBackground`.
const SELECTED: u32 = 0xffffff33;

pub enum FieldEvent {
    Changed,
    /// Enter, or Shift-Enter with `back`.
    Submit {
        back: bool,
    },
    Cancel,
}

pub struct TextField {
    focus: FocusHandle,
    role: Role,
    label: SharedString,
    placeholder: SharedString,
    width: Pixels,
    pub text: String,
    /// Opening the bar selects the term, so typing replaces it, as in the desktop find bar.
    selected: bool,
    marked: String,
    bounds: Option<Bounds<Pixels>>,
}

impl EventEmitter<FieldEvent> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextField {
    pub fn new(
        role: Role,
        label: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus: cx.focus_handle().tab_stop(true),
            role,
            label: label.into(),
            placeholder: placeholder.into(),
            width,
            text: String::new(),
            selected: false,
            marked: String::new(),
            bounds: None,
        }
    }

    /// Focuses the field with its term selected.
    pub fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = !self.text.is_empty();
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn edit(&mut self, edit: impl FnOnce(&mut String), cx: &mut Context<Self>) {
        if std::mem::take(&mut self.selected) {
            self.text.clear();
        }
        edit(&mut self.text);
        cx.emit(FieldEvent::Changed);
        cx.notify();
    }

    /// Paste comes here before the terminal around the field gets it.
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        // The term is one line, so pasted line breaks become spaces.
        let pasted = pasted.replace(['\r', '\n'], " ");
        self.edit(|text| text.push_str(&pasted), cx);
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let mods = key.modifiers;
        let secondary = if cfg!(target_os = "macos") {
            mods.platform
        } else {
            mods.control
        };
        match key.key.as_str() {
            "escape" => cx.emit(FieldEvent::Cancel),
            "enter" => cx.emit(FieldEvent::Submit { back: mods.shift }),
            "backspace" if self.marked.is_empty() => self.edit(
                |text| {
                    text.pop();
                },
                cx,
            ),
            "a" if secondary => self.select_all(window, cx),
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let entity = cx.entity();
        let focus = self.focus.clone();
        let register = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                entity.update(cx, |field, _| field.bounds = Some(bounds));
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
            },
        )
        .absolute()
        .size_full();
        let empty = self.text.is_empty() && self.marked.is_empty();
        div()
            .id("text-field")
            .role(self.role)
            .aria_label(self.label.clone())
            .aria_placeholder(self.placeholder.clone())
            .aria_value(self.text.clone())
            .track_focus(&self.focus)
            .key_context("FindField")
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::paste))
            .relative()
            .h(px(28.0))
            .w(self.width)
            .px(px(8.0))
            .flex()
            .items_center()
            .overflow_hidden()
            .text_size(px(13.0))
            .cursor_text()
            .child(register)
            .when(empty, |field| {
                field.child(
                    div()
                        .text_color(rgb(MUTED_FOREGROUND))
                        .child(self.placeholder.clone()),
                )
            })
            .when(!self.text.is_empty(), |field| {
                field.child(
                    div()
                        .flex_none()
                        .text_color(rgb(FOREGROUND))
                        .when(self.selected, |text| text.bg(rgba(SELECTED)))
                        .child(self.text.clone()),
                )
            })
            .when(!self.marked.is_empty(), |field| {
                field.child(
                    div()
                        .flex_none()
                        .text_color(rgb(FOREGROUND))
                        .underline()
                        .child(self.marked.clone()),
                )
            })
            .when(focused && !self.selected, |field| {
                field.child(div().flex_none().w(px(1.0)).h(px(16.0)).bg(rgb(FOREGROUND)))
            })
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let utf16: Vec<_> = format!("{}{}", self.text, self.marked)
            .encode_utf16()
            .collect();
        let range = range.start.min(utf16.len())..range.end.min(utf16.len());
        *adjusted = Some(range.clone());
        String::from_utf16(&utf16[range]).ok()
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let text = self.text.encode_utf16().count();
        let end = text + self.marked.encode_utf16().count();
        Some(UTF16Selection {
            range: if self.selected { 0..text } else { end..end },
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let start = self.text.encode_utf16().count();
        (!self.marked.is_empty()).then(|| start..start + self.marked.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let marked = std::mem::take(&mut self.marked);
        self.edit(|text| text.push_str(&marked), cx);
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked.clear();
        self.edit(|current| current.push_str(text), cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if std::mem::take(&mut self.selected) {
            self.text.clear();
        }
        self.marked = text.to_string();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.bounds
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}
