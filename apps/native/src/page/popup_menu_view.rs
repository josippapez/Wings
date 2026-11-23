//! A page's right-click menu, in two halves. A `Stack` with `onContextMenu` tells the page where the right button went
//! down and what was under the pointer, and a `PopupMenu` is the list of actions the page then opens at that point.
use super::*;
use crate::ui::behaviors::{DismissReason, MenuKey, bind_layer, popup_at};
use gpui::{Bounds, MouseButton, MouseDownEvent, Point};
use serde_json::{Value, json};

/// What a stack's listener for the children's places hears: where each child of the stack was drawn.
pub(super) type ChildrenWatch = Box<dyn Fn(Vec<Bounds<Pixels>>, &mut Window, &mut gpui::App) + 'static>;

/// What turns the places of the elements a stack draws into the places of its children, for a column drawn a window at a
/// time (`windowed_column`), whose elements are a few of its children between spacers.
pub(super) type ChildrenPlaces =
    Box<dyn Fn(Vec<Bounds<Pixels>>, &mut Window, &mut gpui::App) -> Vec<Bounds<Pixels>> + 'static>;

/// The index in `stack.children` of the node behind each element the stack draws, in order: `PageView::items` makes one
/// element of a run of plain strings, and none of an empty run, and `rising` one of each child.
fn element_children(stack: &tree::Stack) -> Vec<usize> {
    if stack.enter {
        return (0..stack.children.len()).collect();
    }
    let mut slots = Vec::new();
    // Where the run of strings under way started, and whether any of it has text.
    let mut run: Option<(usize, bool)> = None;
    for (at, child) in stack.children.iter().enumerate() {
        match child {
            Node::Str { text, .. } => {
                let run = run.get_or_insert((at, false));
                run.1 |= !text.is_empty();
            }
            _ => {
                slots.extend(run.take().filter(|(_, text)| *text).map(|(first, _)| first));
                slots.push(at);
            }
        }
    }
    slots.extend(run.filter(|(_, text)| *text).map(|(first, _)| first));
    slots
}

/// The `Code` node `id`, wherever in the tree it is.
fn find_code(nodes: &[Node], id: u64) -> Option<&tree::Code> {
    nodes.iter().find_map(|node| match node {
        Node::Code(code) if code.id == id => Some(code),
        other => other.parts().into_iter().find_map(|part| find_code(part, id)),
    })
}

impl PageView {
    /// `element`, a `Stack`'s box, with what its `onContextMenu` needs: its children's places, kept as they are drawn,
    /// and a listener for the right button that comes after what is inside it (GPUI hands a press to the innermost
    /// element first), so it finds what they recorded under the pointer. It sends the page where the press was, which
    /// child was under it, and the link or code that was, then stops the press there. `watch` is what else wants the
    /// children's places, which GPUI keeps one listener for. `children` makes the children's places of the elements'.
    pub(super) fn with_context_menu(
        &self,
        element: gpui::Div,
        stack: &tree::Stack,
        watch: Option<ChildrenWatch>,
        children: Option<ChildrenPlaces>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let Some(handler) = stack.on_context_menu.clone() else {
            return match (watch, children) {
                (Some(watch), None) => element.on_children_prepainted(watch),
                (None, None) => element,
                (watch, Some(children)) => element.on_children_prepainted(move |bounds, window, cx| {
                    let bounds = children(bounds, window, cx);
                    if let Some(watch) = &watch {
                        watch(bounds, window, cx);
                    }
                }),
            };
        };
        let places: Rc<RefCell<Vec<Bounds<Pixels>>>> = Rc::default();
        let kept = places.clone();
        let slots = element_children(stack);
        element
            .on_children_prepainted(move |bounds, window, cx| {
                let bounds = match &children {
                    Some(children) => children(bounds, window, cx),
                    None => bounds,
                };
                if let Some(watch) = &watch {
                    watch(bounds.clone(), window, cx);
                }
                *kept.borrow_mut() = bounds;
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    let child = places
                        .borrow()
                        .iter()
                        .position(|place| place.contains(&event.position))
                        .and_then(|at| slots.get(at).copied());
                    let mut payload = json!({ "x": f32::from(event.position.x), "y": f32::from(event.position.y), "child": child });
                    // Only what was recorded by this very press: one left by a press somewhere else is not under it.
                    if let Some((at, Value::Object(hit))) = view.context_hit.take()
                        && at == event.position
                        && let Value::Object(payload) = &mut payload
                    {
                        payload.extend(hit);
                    }
                    if let Some(text) = view.selected_text()
                        && let Value::Object(payload) = &mut payload
                    {
                        payload.insert("selection".to_owned(), json!({ "text": text }));
                    }
                    view.act(&handler, Some(&payload));
                    cx.stop_propagation();
                }),
            )
    }

    /// A `Code` node was pressed with the right button at `at`: remembers its whole text for the `Stack` round it that
    /// asks, without stopping the press.
    pub(super) fn record_code_hit(&mut self, id: u64, at: Point<Pixels>) {
        let Some(code) = self.tree.as_ref().and_then(|tree| find_code(&tree.children, id)) else {
            return;
        };
        let hit = json!({ "code": { "text": code.text, "isBlock": true } });
        *self.context_hit.borrow_mut() = Some((at, hit));
    }
}

/// The desktop's `--accent` under the item you point at, as `ui::menu` draws it.
const ACCENT: u32 = 0xffffff14;

impl PageView {
    /// A `PopupMenu`: its items in the look of a `Menu`'s list, over the page at the point the page gave, moved in to
    /// stay in the window. A click on an item sends its `onClick` and the page then leaves the menu out; Escape and a
    /// press outside send `onClose`. It is drawn only while the page sends it, so the menu has no state of its own
    /// besides the keyboard (`render` gives it that).
    pub(super) fn popup_menu(&self, menu: &tree::PopupMenu, cx: &mut Context<Self>) -> AnyElement {
        let id = menu.id;
        let items = menu.items.iter().enumerate().filter_map(|(at, node)| {
            let Node::Button(item) = node else {
                return None;
            };
            let text = plain_text(&item.children);
            let label = if text.trim().is_empty() {
                item.label.clone().unwrap_or_default()
            } else {
                text
            };
            let destructive = item.variant == tree::Variant::Destructive;
            let row = div()
                .id(ElementId::NamedInteger(format!("page-popup-{id}-item").into(), at as u64))
                .role(Role::MenuItem)
                .aria_label(label.clone())
                .flex()
                .flex_col()
                .gap(px(1.0))
                .px(px(6.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .text_color(rgb(if destructive { theme::palette().danger } else { theme::palette().foreground }))
                .whitespace_nowrap()
                .child(div().min_w_0().truncate().text_size(px(14.0)).child(label))
                .children(item.description.clone().map(|description| {
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.0))
                        .text_color(rgb(theme::palette().muted_foreground))
                        .child(description)
                }));
            let row = ui::menu_highlight(row, self.popup_keys.highlight == Some(at));
            Some(match item.on_click.clone() {
                Some(handler) if !item.disabled => row
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgba(if destructive { theme::palette().danger << 8 | 0x33 } else { ACCENT })))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        cx.stop_propagation();
                        view.act(&handler, None);
                    }))
                    .into_any_element(),
                _ => row
                    .opacity(0.5)
                    .a11y_synthetic_children(|builder| {
                        builder.parent_node().set_disabled();
                    })
                    .into_any_element(),
            })
        });
        let on_close = menu.on_close.clone();
        // What the keys pick and jump by: each item's handler and text, none for one that is off.
        let (labels, picks): (Vec<Option<String>>, Vec<Option<String>>) = menu
            .items
            .iter()
            .map(|node| match node {
                Node::Button(item) if !item.disabled && item.on_click.is_some() => {
                    let text = plain_text(&item.children);
                    let label = if text.trim().is_empty() { item.label.clone().unwrap_or_default() } else { text };
                    (Some(label), item.on_click.clone())
                }
                _ => (None, None),
            })
            .unzip();
        let list = ui::menu(ElementId::NamedInteger("page-popup-menu".into(), id), "Menu")
            .track_focus(&self.popup_focus)
            .children(items)
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _, cx| {
                match view.popup_keys.press(event, &labels) {
                    MenuKey::Moved => {}
                    MenuKey::Pick(at) => {
                        if let Some(handler) = &picks[at] {
                            view.act(handler, None);
                        }
                    }
                    MenuKey::Ignored => return,
                }
                cx.stop_propagation();
                cx.notify();
            }));
        // Escape, or a press outside, which closes it without counting as a click in the page, which could then write or
        // copy. `render` gives the keyboard back as the page leaves the menu out.
        let list = bind_layer(
            list,
            cx.listener(move |view, _: &DismissReason, _, _| {
                if let Some(handler) = &on_close {
                    view.send(handler, None);
                }
            }),
        );
        div()
            .child(popup_at(point(px(menu.x), px(menu.y)), list).layer())
            .into_any_element()
    }
}
