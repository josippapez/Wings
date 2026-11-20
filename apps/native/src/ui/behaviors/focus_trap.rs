//! `trap_tab`: Tab and Shift-Tab cycle through a dialog's own controls and never leave it (UI SDK's focus trap). The
//! controls are whatever inside the dialog is a tab stop, in the window's tab order; from the last one Tab goes to the
//! first, and from the first Shift-Tab goes to the last. Built on gpui's window-wide `focus_next` and `focus_prev`,
//! stepped until the keyboard is inside the dialog again: the steps in between are never drawn or heard, as the focus
//! events compare only where it started and where it ends.

use gpui::{App, FocusHandle, InteractiveElement, KeyDownEvent, Window};

/// A step per tab stop in the window, at most: a window never has more, and a dialog with none ends where it began.
const MAX_STEPS: usize = 1000;

/// Moves the keyboard to the next tab stop inside `within` (the dialog's focus handle, which its panel tracks), or the
/// previous one when `back`, wrapping at its ends. Leaves it where it was when `within` has no tab stop.
pub fn cycle(within: &FocusHandle, back: bool, window: &mut Window, cx: &mut App) {
    let from = window.focused(cx);
    for _ in 0..MAX_STEPS {
        if back {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        if within.contains_focused(window, cx) {
            return;
        }
        if window.focused(cx) == from {
            break;
        }
    }
    if let Some(from) = from {
        window.focus(&from, cx);
    }
}

/// `content`, a dialog's panel, with Tab and Shift-Tab trapped in `within`, the focus handle the panel tracks. The key
/// is stopped there, so the page under it doesn't move its own focus. Other Tab chords (with Cmd, Ctrl or Alt) are
/// not the dialog's.
pub fn trap_tab<E: InteractiveElement>(content: E, within: &FocusHandle) -> E {
    let within = within.clone();
    content.on_key_down(move |event: &KeyDownEvent, window, cx| {
        let mods = event.keystroke.modifiers;
        if event.keystroke.key != "tab" || mods.number_of_modifiers() != u8::from(mods.shift) {
            return;
        }
        cycle(&within, mods.shift, window, cx);
        window.prevent_default();
        cx.stop_propagation();
    })
}

/// `trap_tab` as a method, to chain onto a dialog's panel.
pub trait TrapTabExt: InteractiveElement + Sized {
    fn trap_tab(self, within: &FocusHandle) -> Self {
        trap_tab(self, within)
    }
}

impl<E: InteractiveElement> TrapTabExt for E {}

#[cfg(test)]
mod tests {
    use gpui::{Context, FocusHandle, Render, TestApp, Window, div, prelude::*, px};

    use super::*;

    /// A control before the dialog, the dialog with three, and a control after it, all tab stops in that order.
    struct Page {
        before: FocusHandle,
        dialog: FocusHandle,
        inside: [FocusHandle; 3],
        after: FocusHandle,
        trapped: bool,
    }

    impl Render for Page {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let stop = |id: &'static str, handle: &FocusHandle| {
                div().id(id).size(px(20.0)).track_focus(handle)
            };
            let panel = div()
                .id("dialog")
                .size(px(100.0))
                .track_focus(&self.dialog)
                .children(self.inside.iter().enumerate().map(|(at, handle)| {
                    stop(["one", "two", "three"][at], handle)
                }));
            div()
                .size_full()
                .child(stop("before", &self.before))
                .child(if self.trapped {
                    trap_tab(panel, &self.dialog).into_any_element()
                } else {
                    panel.into_any_element()
                })
                .child(stop("after", &self.after))
        }
    }

    fn open(trapped: bool) -> (TestApp, gpui::TestAppWindow<Page>) {
        let mut app = TestApp::new();
        let mut window = app.open_window(move |_, cx| Page {
            before: cx.focus_handle().tab_stop(true),
            dialog: cx.focus_handle(),
            inside: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            after: cx.focus_handle().tab_stop(true),
            trapped,
        });
        window.draw();
        (app, window)
    }

    fn focus_inside(window: &mut gpui::TestAppWindow<Page>, at: usize) {
        window.update(|page, window, cx| window.focus(&page.inside[at], cx));
        window.draw();
    }

    fn is_inside(window: &mut gpui::TestAppWindow<Page>) -> [bool; 3] {
        window.update(|page, window, _| std::array::from_fn(|at| page.inside[at].is_focused(window)))
    }

    #[test]
    fn tab_from_the_last_control_goes_to_the_first_and_shift_tab_from_the_first_to_the_last() {
        let (_app, mut window) = open(true);
        focus_inside(&mut window, 1);
        window.simulate_keystroke("tab");
        assert_eq!(is_inside(&mut window), [false, false, true]);
        window.simulate_keystroke("tab");
        assert_eq!(is_inside(&mut window), [true, false, false]);
        window.simulate_keystroke("shift-tab");
        assert_eq!(is_inside(&mut window), [false, false, true]);
        window.simulate_keystroke("shift-tab");
        assert_eq!(is_inside(&mut window), [false, true, false]);
    }

    #[test]
    fn tab_from_the_dialog_itself_enters_its_first_control() {
        let (_app, mut window) = open(true);
        window.update(|page, window, cx| window.focus(&page.dialog, cx));
        window.draw();
        window.simulate_keystroke("tab");
        assert_eq!(is_inside(&mut window), [true, false, false]);
    }

    /// The contract's other side: with no trap, the same Tab does nothing here (the window moves it), so this fails on
    /// a dialog that doesn't bind it, and the keyboard never ends up outside a trapped one.
    #[test]
    fn a_trapped_dialog_never_lets_the_keyboard_out() {
        let (_app, mut window) = open(true);
        focus_inside(&mut window, 2);
        for _ in 0..7 {
            window.simulate_keystroke("tab");
            assert!(is_inside(&mut window).contains(&true));
        }
        let (_app, mut window) = open(false);
        focus_inside(&mut window, 2);
        window.simulate_keystroke("tab");
        assert_eq!(is_inside(&mut window), [false, false, true], "untrapped, Tab isn't handled by the page itself");
    }
}
