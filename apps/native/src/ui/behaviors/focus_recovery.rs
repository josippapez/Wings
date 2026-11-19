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
}
