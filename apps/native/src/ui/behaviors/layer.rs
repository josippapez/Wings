//! `bind_layer`: the one dismissal rule for popups (UI SDK's `bindLayer`): Escape inside the layer and a press outside
//! it dismiss it, with the reason. `bind_escape` is its Escape half alone, for a modal dialog whose backdrop already
//! handles the press outside.

use std::rc::Rc;

use gpui::{App, InteractiveElement, KeyDownEvent, MouseDownEvent, Pixels, Point, Window};

/// Why a layer was dismissed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DismissReason {
    /// Escape while the keyboard is in the layer. Give the keyboard back to the trigger (`FocusRecovery::restore`).
    EscapeKey,
    /// A press outside it, at this point in the window. `FocusRecovery::restore` leaves the keyboard where the press
    /// put it.
    OutsidePress(Point<Pixels>),
}

/// `content` with Escape while the keyboard is inside it handled by `on_escape`, and stopped there so the page or the
/// terminal under it doesn't hear it too. For a modal dialog, whose backdrop closes it on a press outside.
pub fn bind_escape<E: InteractiveElement>(
    content: E,
    on_escape: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
) -> E {
    content.on_key_down(move |event: &KeyDownEvent, window, cx| {
        if event.keystroke.key == "escape" {
            cx.stop_propagation();
            on_escape(event, window, cx);
        }
    })
}

/// `bind_escape` in the capture phase, before a field inside `content` hears the key (a field's Escape cancels its own
/// editing and would otherwise run alongside). `on_escape` says whether it took the key: when it did, the key is
/// stopped there; when not (a select's menu open in the dialog closes first, on its own), it goes on to the field.
pub fn capture_escape<E: InteractiveElement>(
    content: E,
    on_escape: impl Fn(&KeyDownEvent, &mut Window, &mut App) -> bool + 'static,
) -> E {
    content.capture_key_down(move |event: &KeyDownEvent, window, cx| {
        if event.keystroke.key == "escape" && on_escape(event, window, cx) {
            cx.stop_propagation();
        }
    })
}

/// `bind_escape` and `capture_escape` as methods, to chain onto a dialog's panel.
pub trait EscapeExt: InteractiveElement + Sized {
    fn on_escape(self, on_escape: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static) -> Self {
        bind_escape(self, on_escape)
    }

    fn capture_escape(
        self,
        on_escape: impl Fn(&KeyDownEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        capture_escape(self, on_escape)
    }
}

impl<E: InteractiveElement> EscapeExt for E {}

/// `content`, a popup's box, dismissed by Escape in it (stopped there, so the page under it doesn't hear it) and by a
/// press outside it. A press on its own trigger is outside too: a trigger that toggles checks `OutsidePress`'s point
/// against its bounds, or records on its own press that the popup was open.
///
/// A press inside it is its own, as in a browser: nothing under it hears it. A menu that hangs past a dialog's panel
/// lies over the dialog's backdrop, whose press closes the dialog.
pub fn bind_layer<E: InteractiveElement>(
    content: E,
    on_dismiss: impl Fn(&DismissReason, &mut Window, &mut App) + 'static,
) -> E {
    let on_escape = Rc::new(on_dismiss);
    let on_outside = on_escape.clone();
    bind_escape(content.occlude(), move |_, window, cx| {
        on_escape(&DismissReason::EscapeKey, window, cx)
    })
    .on_mouse_down_out(move |event: &MouseDownEvent, window, cx| {
        on_outside(&DismissReason::OutsidePress(event.position), window, cx)
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{
        Bounds, Context, FocusHandle, MouseButton, Render, TestApp, Window, WindowBounds, WindowOptions, div, point,
        prelude::*, px, size,
    };

    use super::*;
    use crate::ui::behaviors::popup_on_parent;

    /// A page that hears every key, around a focused box whose Escape is bound with `bind_escape`.
    struct Dialog {
        focus: FocusHandle,
        escapes: Rc<Cell<usize>>,
        page_heard: Rc<Cell<usize>>,
    }

    impl Render for Dialog {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let escapes = self.escapes.clone();
            let page_heard = self.page_heard.clone();
            div()
                .size_full()
                .on_key_down(move |_: &KeyDownEvent, _, _| page_heard.set(page_heard.get() + 1))
                .child(bind_escape(
                    div().id("dialog").size(px(100.0)).track_focus(&self.focus),
                    move |_, _, _| escapes.set(escapes.get() + 1),
                ))
        }
    }

    /// A focused field that stops its own Escape (as `TextField` does), inside a dialog panel.
    struct Field {
        focus: FocusHandle,
        heard: Rc<Cell<usize>>,
        field_heard: Rc<Cell<usize>>,
        takes: bool,
    }

    impl Render for Field {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let (heard, field_heard, takes) = (self.heard.clone(), self.field_heard.clone(), self.takes);
            div().size_full().child(
                capture_escape(
                    div().id("panel").size(px(100.0)).child(
                        div()
                            .id("field")
                            .size(px(50.0))
                            .track_focus(&self.focus)
                            .on_key_down(move |_: &KeyDownEvent, _, cx| {
                                field_heard.set(field_heard.get() + 1);
                                cx.stop_propagation();
                            }),
                    ),
                    move |_, _, _| {
                        heard.set(heard.get() + 1);
                        takes
                    },
                ),
            )
        }
    }

    /// A modal: a backdrop that closes it on a press, and a 100pt panel, in which two menus hang past the panel's foot over
    /// the backdrop: a `bind_layer` layer beside its parent, as a page's select menu is, and a `ui::popover`, as Wings'
    /// own selects' menus are.
    struct Modal {
        picked: Rc<Cell<usize>>,
        closed: Rc<Cell<usize>>,
    }

    impl Render for Modal {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let item = |id: &'static str, picked: Rc<Cell<usize>>| {
                div()
                    .id(id)
                    .w(px(30.0))
                    .h(px(200.0))
                    .on_click(move |_, _, _| picked.set(picked.get() + 1))
            };
            let layer = bind_layer(div().id("layer").child(item("layer-item", self.picked.clone())), |_, _, _| {});
            let popover = crate::ui::popover(div().id("popover").mt(px(24.0)).child(item("popover-item", self.picked.clone())));
            let closed = self.closed.clone();
            div().size_full().child(
                div()
                    .id("backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, move |_, _, _| closed.set(closed.get() + 1))
                    .child(
                        div()
                            .id("panel")
                            .absolute()
                            .top(px(20.0))
                            .left(px(20.0))
                            .size(px(100.0))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(div().absolute().top_0().left_0().size(px(20.0)).child(popup_on_parent(layer).layer()))
                            .child(div().absolute().top_0().left(px(50.0)).size(px(20.0)).child(popover)),
                    ),
            )
        }
    }

    #[test]
    fn a_press_on_a_menu_past_a_modals_panel_is_the_menus_and_never_reaches_the_backdrop() {
        let mut app = TestApp::new();
        let (picked, closed) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let (p, c) = (picked.clone(), closed.clone());
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(400.0), px(400.0)));
        let mut window = app.open_window_with_options(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_, _| Modal { picked: p, closed: c },
        );
        // 200pt down: past the panel's foot at 120pt, on the layer's item (from 20pt across) and the popover's (from 70pt).
        for (x, picks) in [(35.0, 1), (85.0, 2)] {
            window.draw();
            window.simulate_click(point(px(x), px(200.0)), MouseButton::Left);
            assert_eq!((picked.get(), closed.get()), (picks, 0), "a press at {x}pt across");
        }
    }

    #[test]
    fn capture_escape_runs_before_a_field_and_hands_the_key_on_when_it_declines() {
        for takes in [true, false] {
            let mut app = TestApp::new();
            let (heard, field_heard) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
            let (h, f) = (heard.clone(), field_heard.clone());
            let mut window = app.open_window(move |_, cx| Field {
                focus: cx.focus_handle(),
                heard: h,
                field_heard: f,
                takes,
            });
            window.draw();
            window.update(|view, window, cx| window.focus(&view.focus, cx));
            window.draw();
            window.simulate_keystroke("escape");
            assert_eq!((heard.get(), field_heard.get()), (1, usize::from(!takes)), "takes: {takes}");
        }
    }

    #[test]
    fn escape_in_a_dialog_is_handled_there_and_not_heard_by_the_page_under_it() {
        let mut app = TestApp::new();
        let escapes = Rc::new(Cell::new(0));
        let page_heard = Rc::new(Cell::new(0));
        let (e, h) = (escapes.clone(), page_heard.clone());
        let mut window = app.open_window(move |_, cx| Dialog {
            focus: cx.focus_handle(),
            escapes: e,
            page_heard: h,
        });
        window.draw();
        window.update(|view, window, cx| window.focus(&view.focus, cx));
        window.draw();

        window.simulate_keystroke("escape");
        assert_eq!((escapes.get(), page_heard.get()), (1, 0));

        // Any other key is the page's, not the dialog's.
        window.simulate_keystroke("a");
        assert_eq!((escapes.get(), page_heard.get()), (1, 1));
    }
}
