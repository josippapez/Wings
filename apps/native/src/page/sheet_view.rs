//! A page's `Sheet`: Wings' own sheet from the right (`ui::sheet`), over what holds it, as Past sessions is over the
//! window. Its header's search and chips are the sheet's own search box and toggle, and it takes the keyboard while it
//! shows, giving it back when it goes.

use std::cell::Cell;

use gpui::{
    AnyElement, Context, ElementId, Focusable, MouseButton, SharedString, div, prelude::*,
};

use super::{Node, PageView, plain_text, tree};
use crate::{
    icons::Icon,
    ui::{self, ButtonSize, behaviors::EscapeExt, motion},
};

thread_local! {
    /// A sheet's header is being drawn: a `SearchField` or `Chips` in it is drawn as the sheet's.
    static IN_HEADER: Cell<bool> = const { Cell::new(false) };
}

/// Whether what is drawn now is in a sheet's header.
pub(super) fn in_header() -> bool {
    IN_HEADER.with(Cell::get)
}

/// The first `SearchField` under `nodes`, by id.
fn first_search(nodes: &[Node]) -> Option<u64> {
    nodes.iter().find_map(|node| match node {
        Node::SearchField(field) => Some(field.id),
        other => other.parts().into_iter().find_map(first_search),
    })
}

impl PageView {
    pub(super) fn sheet(&self, sheet: &tree::Sheet, cx: &mut Context<Self>) -> AnyElement {
        let id = sheet.id;
        let focus = self
            .sheet_focus
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let close = sheet.on_close.clone();
        let before = IN_HEADER.with(|flag| flag.replace(true));
        let controls: Vec<AnyElement> = self.nodes(&sheet.header, cx);
        IN_HEADER.with(|flag| flag.set(before));
        // It takes the keyboard to its search when it has one, so you can type at once, as in Past sessions.
        let search = first_search(&sheet.header)
            .and_then(|field| self.fields.borrow().get(&field).map(|state| state.field.clone()));
        let target = match search {
            Some(field) => field.read(cx).focus_handle(cx),
            None => focus.clone(),
        };
        self.sheets_drawn.borrow_mut().push((id, target));
        let header = ui::sheet_header(
            self.inline(&sheet.title, cx),
            self.inline(&sheet.description, cx),
        )
        .children(controls);
        let label = SharedString::from(plain_text(&sheet.title));
        let (backdrop, panel) = ui::sheet(ElementId::NamedInteger("page-sheet".into(), id), label, &focus);
        let (on_press, on_key, on_button) = (close.clone(), close.clone(), close);
        let phase = motion::phase();
        motion::sheet_backdrop(
            backdrop
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _, _, _| {
                        if let Some(handler) = &on_press {
                            view.act(handler, None);
                        }
                    }),
                )
                .child(motion::sheet_panel(
                    panel
                        // Before its search field, which keeps Escape, as a `Dialog` does. A select's menu open in it
                        // closes first, on its own.
                        .capture_escape({
                            let page = cx.entity();
                            move |_, window, cx| {
                                page.update(cx, |view, _| {
                                    if view.shows_menu() {
                                        return false;
                                    }
                                    if let Some(handler) = &on_key {
                                        view.send(handler, None);
                                    }
                                    window.prevent_default();
                                    true
                                })
                            }
                        })
                        .child(header)
                        .child(
                            div()
                                .id(ElementId::NamedInteger("page-sheet-list".into(), id))
                                .min_h_0()
                                .flex_1()
                                .overflow_y_scroll()
                                .child(ui::sheet_list().children(self.nodes(&sheet.body, cx))),
                        )
                        .child(ui::sheet_corner(
                            ui::icon_button(
                                ElementId::NamedInteger("page-sheet-close".into(), id),
                                Icon::X,
                                "Close",
                                ButtonSize::Sm,
                            )
                            .on_click(cx.listener(move |view, _, _, _| {
                                if let Some(handler) = &on_button {
                                    view.act(handler, None);
                                }
                            })),
                        )),
                    phase,
                )),
            ui::backdrop_color(),
            phase,
        )
    }

    /// `Chips` in a sheet's header, drawn as its toggle: Past sessions' Any time, Today, 7 days and 30 days.
    pub(super) fn sheet_chips(&self, select: &tree::Select, cx: &mut Context<Self>) -> AnyElement {
        let current = select.current().map(str::to_owned);
        ui::segmented(select.options.iter().enumerate().map(|(at, option)| {
            let (handler, value) = (select.on_change.clone(), option.value.clone());
            let on = current.as_deref() == Some(option.value.as_str());
            ui::segment(
                ElementId::NamedInteger(format!("page-segment-{}", select.id).into(), at as u64),
                option.label.clone(),
                on,
            )
            .on_click(cx.listener(move |view, _, _, _| {
                if let Some(handler) = &handler {
                    view.act(handler, Some(&serde_json::json!({ "value": value })));
                }
            }))
        }))
        .into_any_element()
    }

    /// After a render: a sheet that has just come gets the keyboard, if the page may take it, and when the last one goes,
    /// what had the keyboard before it gets it back.
    pub(super) fn settle_sheets(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let drawn = std::mem::take(&mut *self.sheets_drawn.borrow_mut());
        let opened = drawn.iter().find(|(id, _)| !self.sheets_shown.contains(id));
        if let Some((_, target)) = opened.filter(|_| self.may_take_keyboard(window, cx)) {
            if self.sheet_return.is_none() {
                self.sheet_return = window.focused(cx);
            }
            target.focus(window, cx);
        }
        if drawn.is_empty()
            && !self.sheets_shown.is_empty()
            && let Some(back) = self.sheet_return.take()
        {
            back.focus(window, cx);
        }
        self.sheets_shown = drawn.into_iter().map(|(id, _)| id).collect();
    }
}
