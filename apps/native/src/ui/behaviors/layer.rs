//! `bind_layer`: the one dismissal rule for popups (UI SDK's `bindLayer`): Escape inside the layer and a press outside
//! it dismiss it, with the reason.

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

/// `content`, a popup's box, dismissed by Escape in it (stopped there, so the page under it doesn't hear it) and by a
/// press outside it. A press on its own trigger is outside too: a trigger that toggles checks `OutsidePress`'s point
/// against its bounds, or records on its own press that the popup was open.
pub fn bind_layer<E: InteractiveElement>(
    content: E,
    on_dismiss: impl Fn(&DismissReason, &mut Window, &mut App) + 'static,
) -> E {
    let on_escape = Rc::new(on_dismiss);
    let on_outside = on_escape.clone();
    content
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                on_escape(&DismissReason::EscapeKey, window, cx);
            }
        })
        .on_mouse_down_out(move |event: &MouseDownEvent, window, cx| {
            on_outside(&DismissReason::OutsidePress(event.position), window, cx)
        })
}
