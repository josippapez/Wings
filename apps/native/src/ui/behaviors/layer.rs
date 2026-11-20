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

/// `bind_escape` as a method, to chain onto a dialog's panel.
pub trait EscapeExt: InteractiveElement + Sized {
    fn on_escape(self, on_escape: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static) -> Self {
        bind_escape(self, on_escape)
    }
}

impl<E: InteractiveElement> EscapeExt for E {}

/// `content`, a popup's box, dismissed by Escape in it (stopped there, so the page under it doesn't hear it) and by a
/// press outside it. A press on its own trigger is outside too: a trigger that toggles checks `OutsidePress`'s point
/// against its bounds, or records on its own press that the popup was open.
pub fn bind_layer<E: InteractiveElement>(
    content: E,
    on_dismiss: impl Fn(&DismissReason, &mut Window, &mut App) + 'static,
) -> E {
    let on_escape = Rc::new(on_dismiss);
    let on_outside = on_escape.clone();
    bind_escape(content, move |_, window, cx| {
        on_escape(&DismissReason::EscapeKey, window, cx)
    })
    .on_mouse_down_out(move |event: &MouseDownEvent, window, cx| {
        on_outside(&DismissReason::OutsidePress(event.position), window, cx)
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{Context, FocusHandle, Render, TestApp, Window, div, prelude::*, px};

    use super::*;

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
