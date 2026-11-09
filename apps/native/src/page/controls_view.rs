//! The page's form controls: checkboxes, chips, list buttons, search fields, text inputs, disclosures, selects and
//! comboboxes, and the menus they open.

use std::{collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, Context, ElementId, FontWeight, KeyDownEvent, Pixels, Role, SharedString, Toggled,
    anchored, canvas, deferred, div, prelude::*, px, rgb, rgba,
};

use super::{
    InputState, Node, PageView, SearchState, line_height, plain_text, send_shortcut, tree,
};
use crate::{theme, ui};

/// The padding over and under a `ListButton`'s parts, inside its 1pt border.
pub(super) const LIST_BUTTON_PADDING: f32 = 2.0;

impl PageView {
    pub(super) fn checkbox(&self, checkbox: &tree::Checkbox, cx: &mut Context<Self>) -> AnyElement {
        let label = plain_text(&checkbox.children);
        let checked = checkbox.checked;
        let element = div()
            .id(ElementId::NamedInteger("page-checkbox".into(), checkbox.id))
            .role(Role::Button)
            .aria_label(label)
            .aria_toggled(if checkbox.checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_pointer()
            .opacity(if checkbox.disabled { 0.5 } else { 1.0 })
            .child(
                div()
                    .size(px(16.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(rgba(ui::HAIRLINE))
                    .bg(rgba(if checkbox.checked {
                        theme::INFO << 8 | 0xff
                    } else {
                        0
                    }))
                    .text_size(px(12.0))
                    .text_color(rgb(theme::BACKGROUND))
                    .child(if checkbox.checked {
                        "✓".into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            )
            .child(self.inline(&checkbox.children, cx));
        match checkbox.on_change.clone() {
            Some(handler) => {
                let keyboard_handler = handler.clone();
                element
                    .focusable()
                    .tab_stop(true)
                    .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _, _| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "space" | "enter")
                        {
                            view.act(
                                &keyboard_handler,
                                Some(&serde_json::json!({ "checked": !checked })),
                            );
                        }
                    }))
                    .on_click(cx.listener(move |view, _, _, _| {
                        view.act(&handler, Some(&serde_json::json!({ "checked": !checked })))
                    }))
                    .into_any_element()
            }
            None => element
                .cursor_default()
                .a11y_synthetic_children(|builder| {
                    builder.parent_node().set_disabled();
                })
                .into_any_element(),
        }
    }

    pub(super) fn chips(&self, select: &tree::Select, cx: &mut Context<Self>) -> AnyElement {
        // Every chip's keys move along the same chips, so they share one list of them and their focus.
        let focus = Rc::new(
            select
                .options
                .iter()
                .filter(|o| !o.disabled)
                .map(|option| {
                    let handle = self
                        .chip_focus
                        .borrow_mut()
                        .entry((select.id, option.value.clone()))
                        .or_insert_with(|| cx.focus_handle().tab_stop(true))
                        .clone();
                    (option.value.clone(), handle)
                })
                .collect::<HashMap<_, _>>(),
        );
        let next = select
            .options
            .iter()
            .filter(|o| !o.disabled)
            .map(|o| o.value.clone())
            .collect::<Rc<[_]>>();
        let options = select
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let handler = select.on_change.clone();
                let value = option.value.clone();
                let keyboard_handler = handler.clone();
                let keyboard_value = value.clone();
                let next = next.clone();
                let selected = select.current() == Some(value.as_str());
                let disabled = option.disabled;
                let keyboard_focus = focus.clone();
                let click_focus = focus.get(&value).cloned();
                let id = ElementId::NamedInteger(
                    format!("page-chips-{}", select.id).into(),
                    index as u64,
                );
                let label = SharedString::from(option.label.clone());
                if select.line {
                    line_tab(id, label, selected, disabled)
                } else {
                    ui::chip(id, label.clone(), selected, disabled, label)
                }
                .when_some(focus.get(&value), |button, focus| button.track_focus(focus))
                .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                    let at = next
                        .iter()
                        .position(|v| v == &keyboard_value)
                        .unwrap_or_default();
                    let target = match event.keystroke.key.as_str() {
                        "left" => {
                            next.get(at.checked_sub(1).unwrap_or(next.len().saturating_sub(1)))
                        }
                        "right" => next.get((at + 1) % next.len().max(1)),
                        "home" => next.first(),
                        "end" => next.last(),
                        "space" | "enter" => next.get(at),
                        _ => return,
                    };
                    if let (Some(handler), Some(value)) = (&keyboard_handler, target) {
                        view.act(handler, Some(&serde_json::json!({ "value": value })));
                        if let Some(focus) = keyboard_focus.get(value) {
                            focus.focus(window, cx);
                        }
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                }))
                .on_click(cx.listener(move |view, _, window, cx| {
                    if !disabled {
                        if let Some(focus) = &click_focus {
                            focus.focus(window, cx);
                        }
                        if let Some(handler) = &handler {
                            view.act(handler, Some(&serde_json::json!({ "value": value })));
                        }
                    }
                }))
                .into_any_element()
            })
            .collect::<Vec<_>>();
        div()
            .id(ElementId::NamedInteger("page-chips".into(), select.id))
            .role(Role::Group)
            .aria_label(select.label.clone())
            .flex()
            .flex_wrap()
            // plugin-ui.css's `.w-chips`, and `.w-tabs[data-variant="line"]`, which starts its first label at
            // the content's edge, not its tab's padding and edge in from it.
            .gap(px(if select.line { 4.0 } else { 8.0 }))
            .when(select.line, |tabs| tabs.ml(px(-7.0)))
            .children(options)
            .into_any_element()
    }

    /// A `ListButton`. In a baseline row it starts `drop` points down, as a text there does, and takes the room the
    /// row's texts leave: at the row's whole width it squeezed a log line's number beside it onto two lines.
    pub(super) fn list_button(
        &self,
        item: &tree::ListButton,
        drop: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let button = &item.button;
        let handler = button.on_click.clone();
        let keyboard = handler.clone();
        let key_handler = button.on_key_down.clone();
        let mut element = div()
            .id(ElementId::NamedInteger(
                "page-list-button".into(),
                button.id,
            ))
            .role(Role::Button)
            .aria_label(plain_text(&button.children))
            .w_full()
            .min_w_0()
            .flex()
            .gap(px(8.0))
            .items_center()
            .px(px(6.0))
            .pl(px(6.0 + item.indent))
            .py(px(LIST_BUTTON_PADDING))
            .rounded(px(6.0))
            .border_1()
            .border_color(rgba(0))
            .when_some(drop, |el, drop| {
                el.flex_1().when(drop > 0.0, |el| el.mt(px(drop)))
            })
            // Its lines 2pt apart, as Changes' `.commit` stacks a commit's subject over its hash and author:
            // 8pt between a commit's own lines put them further apart than the commits were.
            .when(item.vertical, |el| {
                el.flex_col().items_stretch().gap(px(2.0))
            })
            // Lined up on the first lines of its texts, as `.w-list-button[data-align="baseline"]`: Session
            // detail's "You" and the time sat halfway down a prompt that wraps to three lines.
            .when_some(
                line_height(&button.children).filter(|_| item.baseline),
                |el, height| el.items_start().line_height(px(height)),
            )
            // The twisty is a 12pt chevron centred on the row, as the desktop's and a disclosure's are: the
            // "⌄" glyph it was sits at the bottom of its line, below the row's text.
            .when_some(item.expanded, |el, expanded| {
                let chevron = crate::icons::icon(
                    if expanded {
                        crate::icons::Icon::ChevronDown
                    } else {
                        crate::icons::Icon::ChevronRight
                    },
                    12.0,
                    theme::MUTED_FOREGROUND,
                );
                // On a row lined up on its first lines, the chevron is centred on the first.
                let first = line_height(&button.children).filter(|_| item.baseline);
                el.aria_expanded(expanded).child(match first {
                    Some(height) => div()
                        .flex_none()
                        .mt(px((height - 12.0) / 2.0))
                        .child(chevron)
                        .into_any_element(),
                    None => chevron.into_any_element(),
                })
            })
            .children(self.nodes(&button.children, cx));
        if !button.disabled {
            element = element
                .focusable()
                .tab_stop(true)
                .cursor_pointer()
                .hover(|s| s.bg(rgba(theme::HOVER)))
                .focus_visible(|s| s.border_color(rgb(theme::FOREGROUND)))
                .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                    if let Some(handler) = &key_handler
                        && send_shortcut(view, handler, event, window, cx)
                    {
                        return;
                    }
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        if let Some(handler) = &keyboard {
                            view.act(handler, None);
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }))
                .on_click(cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(handler) = &handler {
                        view.act(handler, None);
                    }
                }));
        }
        element.into_any_element()
    }

    pub(super) fn search_field(
        &self,
        field: &tree::SearchField,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut fields = self.fields.borrow_mut();
        let state = fields.entry(field.id).or_insert_with(|| {
            let value = field.value.clone().unwrap_or_default();
            let entity = cx.new(|cx| {
                let mut input = crate::text_field::TextField::new(
                    Role::SearchInput,
                    field.label.clone(),
                    field.placeholder.clone(),
                    px(280.0),
                    cx,
                );
                input.text = value;
                input
            });
            let id = field.id;
            cx.subscribe(&entity, move |view, entity, event, cx| {
                if matches!(event, crate::text_field::FieldEvent::Changed)
                    && let Some(handler) = view
                        .fields
                        .borrow()
                        .get(&id)
                        .and_then(|state| state.handler.as_deref())
                {
                    view.send(
                        handler,
                        Some(&serde_json::json!({ "value": entity.read(cx).text })),
                    );
                }
            })
            .detach();
            SearchState {
                field: entity,
                handler: field.on_input.clone(),
            }
        });
        state.handler = field.on_input.clone();
        let entity = state.field.clone();
        div()
            .min_w_0()
            .border_1()
            .border_color(rgba(ui::HAIRLINE))
            .rounded(px(6.0))
            .child(entity)
            .into_any_element()
    }

    pub(super) fn input(&self, input: &tree::Input, cx: &mut Context<Self>) -> AnyElement {
        let mut inputs = self.inputs.borrow_mut();
        let state = inputs.entry(input.id).or_insert_with(|| {
            let entity = cx.new(|cx| {
                let role = if input.rows.is_some() {
                    Role::MultilineTextInput
                } else {
                    Role::TextInput
                };
                // A browser's empty time field shows --:--.
                let placeholder = if input.time && input.placeholder.is_empty() {
                    "--:--".to_string()
                } else {
                    input.placeholder.clone()
                };
                let mut field = crate::text_field::TextField::new(
                    role,
                    input.label.clone(),
                    placeholder,
                    px(0.0),
                    cx,
                );
                field.fill = true;
                field.rows = input.rows;
                field.time = input.time;
                field
            });
            let id = input.id;
            cx.subscribe(&entity, move |view, entity, event, cx| {
                if let crate::text_field::FieldEvent::Submit { .. }
                | crate::text_field::FieldEvent::Cancel = event
                {
                    let handler = view
                        .inputs
                        .borrow()
                        .get(&id)
                        .and_then(|state| state.key_handler.clone());
                    if let Some(handler) = handler {
                        let key = if matches!(event, crate::text_field::FieldEvent::Cancel) {
                            "Escape"
                        } else {
                            "Enter"
                        };
                        let payload = serde_json::json!({"type":"keydown", "key":key});
                        // Escape gives no user activation in a browser either.
                        if key == "Enter" {
                            view.act(&handler, Some(&payload));
                        } else {
                            view.send(&handler, Some(&payload));
                        }
                    }
                    return;
                }
                if !matches!(event, crate::text_field::FieldEvent::Changed) {
                    return;
                }
                // Typing brings a combobox's options back, with none of them picked out.
                view.suggestions_closed = false;
                view.highlight = None;
                cx.notify();
                let text = entity.read(cx).value().to_string();
                let handler = view.inputs.borrow_mut().get_mut(&id).and_then(|state| {
                    let handler = state.handler.clone()?;
                    state.sent.push(text.clone());
                    Some(handler)
                });
                if let Some(handler) = handler {
                    view.send(&handler, Some(&serde_json::json!({ "value": text })));
                }
            })
            .detach();
            InputState {
                field: entity,
                handler: None,
                key_handler: None,
                blur_handler: None,
                initialized: false,
                auto_focus: input.auto_focus,
                select_on_focus: input.select_on_focus,
                blur_subscription: None,
                given: String::new(),
                sent: Vec::new(),
            }
        });
        state.handler = input.on_change.clone();
        state.key_handler = input.on_key_down.clone();
        state.blur_handler = input.on_blur.clone();
        let (disabled, max_chars) = (input.disabled, input.max_length);
        let echo = state.sent.iter().position(|sent| *sent == input.value);
        if let Some(echo) = echo {
            state.sent.drain(..=echo);
        }
        state.field.update(cx, |field, cx| {
            if echo.is_none() && state.given != input.value && field.text != input.value {
                field.set_text(input.value.clone());
                state.sent.clear();
                cx.notify();
            }
            if field.disabled != disabled
                || field.max_chars != max_chars
                || field.secret != input.password
            {
                (field.disabled, field.max_chars) = (disabled, max_chars);
                field.secret = input.password;
                cx.notify();
            }
        });
        state.given = input.value.clone();
        state.field.clone().into_any_element()
    }

    pub(super) fn disclosure(
        &self,
        disclosure: &tree::Disclosure,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = disclosure.id;
        let open = disclosure.open.unwrap_or_else(|| self.open.contains(&id));
        let handler = disclosure.on_open_change.clone();
        let trigger = ui::disclosure_trigger(
            ElementId::NamedInteger("page-disclosure".into(), id),
            plain_text(&disclosure.summary),
            open,
            // A summary longer than the line wraps under itself, as in the desktop, instead of running past the card.
            div().min_w_0().child(self.inline(&disclosure.summary, cx)),
        )
        .on_click(cx.listener(move |view, _, _, cx| {
            cx.stop_propagation();
            if let Some(handler) = &handler {
                view.act(handler, Some(&serde_json::json!({"open":!open})));
            } else {
                view.toggle_disclosure(id, cx);
            }
        }));
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .child(trigger)
            .when(open, |el| el.children(self.nodes(&disclosure.body, cx)))
            .into_any_element()
    }

    /// Fills a select or combobox and keeps where it was drawn, and how tall the window was, for its menu.
    fn measure_menu(id: u64, cx: &mut Context<Self>) -> gpui::Canvas<()> {
        let entity = cx.entity();
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let height = window.viewport_size().height;
                entity.update(cx, |view, _| {
                    view.select_bounds.insert(id, (bounds, height));
                });
            },
        )
        .absolute()
        .size_full()
    }

    pub(super) fn select(&self, select: &tree::Select, cx: &mut Context<Self>) -> AnyElement {
        let id = select.id;
        let open = self.menu == Some(id);
        let name = if select.label.is_empty() {
            "Select"
        } else {
            &select.label
        };
        // Pressed before the menu's own press-outside closes it, so the click knows the menu was open.
        let trigger = ui::select_trigger(
            ElementId::NamedInteger("page-select".into(), id),
            name.to_owned(),
            select.shown().to_owned(),
        )
        .w_full()
        .capture_any_mouse_down(cx.listener(move |view, _, _, _| {
            view.pressed_open = view.menu == Some(id);
        }))
        .on_click(cx.listener(move |view, _, _, cx| view.toggle_menu(id, cx)));
        let measure = Self::measure_menu(id, cx);
        let menu = open.then(|| {
            let (bounds, window_height) = self.select_bounds.get(&id).copied().unwrap_or_default();
            // As wide as the select, and no narrower than the desktop's 144px.
            let width = bounds.size.width.max(px(144.0));
            let room = menu_room(bounds, window_height);
            let current = select.current();
            let items = select.options.iter().enumerate().map(|(at, option)| {
                let item = ui::select_item(
                    ElementId::NamedInteger(format!("page-select-{id}-item").into(), at as u64),
                    option.label.clone(),
                    current == Some(option.value.as_str()),
                );
                if option.disabled {
                    return item.opacity(0.5).cursor_default().into_any_element();
                }
                let (on_change, value) = (select.on_change.clone(), option.value.clone());
                item.on_click(
                    cx.listener(move |view, _, _, cx| view.pick(on_change.as_deref(), &value, cx)),
                )
                .into_any_element()
            });
            ui::popover(
                ui::select_menu(items)
                    .w(width)
                    .max_h(room)
                    .id(ElementId::NamedInteger("page-select-menu".into(), id))
                    .overflow_y_scroll()
                    .on_mouse_down_out(cx.listener(|view, _, _, cx| {
                        view.menu = None;
                        cx.notify();
                    })),
            )
        });
        div()
            .relative()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(measure)
            .child(trigger)
            .children(menu)
            .into_any_element()
    }

    /// A combobox's field, and while it has the keyboard the options that match it in a list under it, over the page,
    /// as the desktop's: arrows move through them, Enter or a click picks one, and Escape, a pick or a press outside
    /// put the list away.
    pub(super) fn combobox(&self, combobox: &tree::Combobox, cx: &mut Context<Self>) -> AnyElement {
        let id = combobox.id;
        let field = combobox.field.iter().find_map(|node| match node {
            Node::Input(input) => Some((input.id, input.disabled)),
            _ => None,
        });
        let options = combobox
            .options
            .iter()
            .filter_map(|node| match node {
                Node::Button(button) => {
                    Some((plain_text(&button.children), button.on_click.clone()?))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let listed = !options.is_empty() || !combobox.empty.is_empty();
        let shows = move |view: &Self| {
            field.is_some_and(|(input, disabled)| {
                view.focused == Some(input) && !view.suggestions_closed && !disabled && listed
            })
        };
        let highlight = self.highlight.filter(|at| *at < options.len());
        let handlers = options
            .iter()
            .map(|(_, handler)| handler.clone())
            .collect::<Vec<_>>();
        let measure = Self::measure_menu(id, cx);
        let list = shows(self).then(|| {
            let (bounds, window_height) = self.select_bounds.get(&id).copied().unwrap_or_default();
            let room = menu_room(bounds, window_height);
            let rows = options
                .into_iter()
                .enumerate()
                .map(|(at, (label, handler))| {
                    ui::select_item(
                        ElementId::NamedInteger(
                            format!("page-combobox-{id}-option").into(),
                            at as u64,
                        ),
                        label.clone(),
                        false,
                    )
                    .children(ui::spot(|| format!("combobox-option:{label}")))
                    .when(highlight == Some(at), |row| {
                        row.bg(rgba(theme::HOVER_STRONG))
                    })
                    .on_click(cx.listener(move |view, _, _, cx| view.pick_suggestion(&handler, cx)))
                    .into_any_element()
                })
                .collect::<Vec<_>>();
            // plugin-ui.css's `.w-option-empty`.
            let empty = (!combobox.empty.is_empty()).then(|| {
                div()
                    .px(px(6.0))
                    .py(px(8.0))
                    .text_center()
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(self.inline(&combobox.empty, cx))
                    .into_any_element()
            });
            // Over the page as `ui::popover` draws a select's menu, without its motion: the desktop's
            // `.w-combobox-popup` has `transition: none`, as a list that changes with each key has nothing to animate.
            deferred(
                anchored().child(
                    div().child(
                        ui::select_menu(rows.into_iter().chain(empty))
                            // As wide as the field, as the desktop's `.w-combobox-popup`.
                            .w(bounds.size.width)
                            .max_h(room)
                            .id(ElementId::NamedInteger("page-combobox-list".into(), id))
                            .overflow_y_scroll()
                            // A press on the list is the list's: the button under it doesn't hear it, and a
                            // focusable frame around the page doesn't take the keyboard from the field.
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .on_mouse_down_out(cx.listener(
                                move |view, event: &gpui::MouseDownEvent, _, cx| {
                                    let in_field =
                                        view.select_bounds.get(&id).is_some_and(|(bounds, _)| {
                                            bounds.contains(&event.position)
                                        });
                                    if !in_field {
                                        view.suggestions_closed = true;
                                        cx.notify();
                                    }
                                },
                            )),
                    ),
                ),
            )
        });
        if list.is_some() {
            self.listing.set(true);
        }
        div()
            .relative()
            .min_w_0()
            .flex()
            .flex_col()
            // Before the field's own keys, which move its caret on up and down and send Enter and Escape to the page.
            .capture_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                if !shows(view) || event.keystroke.modifiers.modified() {
                    return;
                }
                let count = handlers.len();
                match event.keystroke.key.as_str() {
                    "down" if count > 0 => {
                        view.highlight = Some(view.highlight.map_or(0, |at| (at + 1) % count));
                    }
                    "up" if count > 0 => {
                        view.highlight = Some(
                            view.highlight
                                .map_or(count - 1, |at| (at + count - 1) % count),
                        );
                    }
                    "enter" => match view.highlight.and_then(|at| handlers.get(at)) {
                        Some(handler) => view.pick_suggestion(handler, cx),
                        None => return,
                    },
                    "escape" => {
                        view.suggestions_closed = true;
                        view.highlight = None;
                    }
                    _ => return,
                }
                window.prevent_default();
                cx.stop_propagation();
                cx.notify();
            }))
            // A press in the field brings the options back, as focusing it does: one outside put them away, and left
            // the field the keyboard.
            .capture_any_mouse_down(cx.listener(|view, _, _, cx| {
                if view.suggestions_closed {
                    view.suggestions_closed = false;
                    cx.notify();
                }
            }))
            .child(measure)
            .children(self.nodes(&combobox.field, cx))
            .children(list)
            .into_any_element()
    }
}

/// How tall a select's or combobox's menu may be: as the desktop's, no taller than the room under it, or over it when
/// there is more there, and it scrolls. Cyclops' hundred projects ran off the window, and once taller than the window
/// it was moved up to cover it from the top. Its box starts 36pt down, under the select, or ends at the select's top.
fn menu_room(bounds: gpui::Bounds<Pixels>, window_height: Pixels) -> Pixels {
    (window_height - bounds.top()).max(bounds.top()) - px(36.0 + 8.0)
}

/// plugin-ui.css's tab of `.w-tabs[data-variant="line"]`: its label at 14px, muted until it's picked, with a 2px line
/// in the text colour under the picked one, and no fill.
fn line_tab(
    id: ElementId,
    label: SharedString,
    selected: bool,
    disabled: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .aria_toggled(if selected {
            Toggled::True
        } else {
            Toggled::False
        })
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(rgba(0))
        .text_size(px(14.0))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .text_color(rgb(if selected {
            theme::FOREGROUND
        } else {
            theme::MUTED_FOREGROUND
        }))
        .child(div().px(px(6.0)).pt(px(2.0)).child(label))
        .child(
            div()
                .h(px(2.0))
                .when(selected, |line| line.bg(rgb(theme::FOREGROUND))),
        )
        .focus_visible(|style| style.border_color(rgb(theme::FOREGROUND)))
        .when(disabled, |tab| {
            tab.opacity(0.5).a11y_synthetic_children(|builder| {
                builder.parent_node().set_disabled();
            })
        })
        .when(!disabled, |tab| {
            tab.focusable()
                .tab_stop(true)
                .cursor_pointer()
                .hover(|style| style.text_color(rgb(theme::FOREGROUND)))
        })
}
