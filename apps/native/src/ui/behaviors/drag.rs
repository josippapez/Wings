//! `drag`: a press that moves becomes a drag only past a small threshold, so a click still selects, and Escape cancels
//! it while it goes (the desktop's `trackDrag`). Shared by the title bar's tabs and the sidebar's projects.

use gpui::{Context, Pixels, Point, Subscription};

/// How far the pointer moves before a press is a drag.
const THRESHOLD: f64 = 5.0;

/// Whether the pointer, now at `at`, has moved far enough from where it was pressed (`from`) for a drag.
pub fn past_threshold(from: Point<Pixels>, at: Point<Pixels>) -> bool {
    (at - from).magnitude() >= THRESHOLD
}

/// Runs `cancel` on Escape and stops the key there, until the returned subscription is dropped: keep it with the drag.
pub fn cancel_on_escape<T: 'static>(
    cx: &mut Context<T>,
    cancel: impl Fn(&mut T, &mut Context<T>) + 'static,
) -> Subscription {
    let this = cx.weak_entity();
    cx.intercept_keystrokes(move |event, _, cx| {
        if event.keystroke.key == "escape" {
            let _ = this.update(cx, |this, cx| cancel(this, cx));
            cx.stop_propagation();
        }
    })
}

#[cfg(test)]
mod tests {
    use gpui::{point, px};

    use super::*;

    #[test]
    fn a_press_is_a_drag_only_past_the_threshold() {
        let from = point(px(10.0), px(10.0));
        assert!(!past_threshold(from, point(px(13.0), px(13.0))));
        assert!(past_threshold(from, point(px(10.0), px(15.0))));
    }
}
