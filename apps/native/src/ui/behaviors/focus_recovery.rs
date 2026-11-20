//! `FocusRecovery`: a popup takes the keyboard as it opens and gives it back to what had it, its trigger, as it closes
//! from the keyboard or a pick (UI SDK's `createFocusRecovery`). It never takes the keyboard back from something that
//! took it since, like a field you pressed in.

use gpui::{App, FocusHandle, Window};

#[derive(Default)]
pub struct FocusRecovery(Option<FocusHandle>);

impl FocusRecovery {
    /// Moves the keyboard to `popup`, remembering what had it.
    pub fn open(&mut self, popup: &FocusHandle, window: &mut Window, cx: &mut App) {
        self.0 = window.focused(cx).filter(|focused| focused != popup);
        window.focus(popup, cx);
    }

    /// Gives the keyboard back to what had it before `open`, if `popup` still has it.
    pub fn restore(&mut self, popup: &FocusHandle, window: &mut Window, cx: &mut App) {
        if let Some(before) = self.0.take()
            && popup.is_focused(window)
        {
            window.focus(&before, cx);
        }
    }

    /// `restore` for a dialog, whose keyboard sits on one of its controls rather than on `popup` itself. True when it
    /// gave the keyboard back, false when there was nothing to give it to or something outside the dialog had it.
    pub fn restore_within(&mut self, popup: &FocusHandle, window: &mut Window, cx: &mut App) -> bool {
        match self.0.take() {
            Some(before) if popup.contains_focused(window, cx) => {
                window.focus(&before, cx);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Context, Render, TestApp, div, prelude::*, px};

    use super::*;

    struct Page {
        trigger: FocusHandle,
        dialog: FocusHandle,
        control: FocusHandle,
    }

    impl Render for Page {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(div().id("trigger").size(px(10.0)).track_focus(&self.trigger))
                .child(
                    div()
                        .id("dialog")
                        .size(px(50.0))
                        .track_focus(&self.dialog)
                        .child(div().id("control").size(px(10.0)).track_focus(&self.control)),
                )
        }
    }

    #[test]
    fn a_dialog_gives_the_keyboard_back_from_one_of_its_controls() {
        let mut app = TestApp::new();
        let mut window = app.open_window(|_, cx| Page {
            trigger: cx.focus_handle(),
            dialog: cx.focus_handle(),
            control: cx.focus_handle(),
        });
        window.draw();
        let mut recovery = FocusRecovery::default();
        window.update(|page, window, cx| {
            window.focus(&page.trigger, cx);
            recovery.open(&page.dialog, window, cx);
            assert!(page.dialog.is_focused(window));
            window.focus(&page.control, cx);
        });
        window.draw();
        window.update(|page, window, cx| {
            assert!(recovery.restore_within(&page.dialog, window, cx));
            assert!(page.trigger.is_focused(window));
            // Once is all: nothing remembered now.
            assert!(!recovery.restore_within(&page.dialog, window, cx));
        });
    }

    #[test]
    fn a_dialog_never_takes_the_keyboard_from_what_took_it_since() {
        let mut app = TestApp::new();
        let mut window = app.open_window(|_, cx| Page {
            trigger: cx.focus_handle(),
            dialog: cx.focus_handle(),
            control: cx.focus_handle(),
        });
        window.draw();
        let mut recovery = FocusRecovery::default();
        window.update(|page, window, cx| {
            window.focus(&page.trigger, cx);
            recovery.open(&page.dialog, window, cx);
            // Something outside the dialog took it.
            window.focus(&page.trigger, cx);
            assert!(!recovery.restore_within(&page.dialog, window, cx));
            assert!(page.trigger.is_focused(window));
        });
    }
}
