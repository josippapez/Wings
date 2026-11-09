//! Other apps' data for the terminals. Programs in panes run under the terminal server's privacy responsibility, the
//! helper app Wings Terminals (`terminal_server.rs`), and macOS never asks before it refuses them other apps' data, like
//! Chrome's profile, which the Chrome DevTools MCP reads. Turning on that app's data for the helper app in System
//! Settings, Files & Folders lets them, so Wings checks at launch and asks once, with a button that opens the list.

use gpui::{
    AnyElement, Context, FocusHandle, KeyDownEvent, SharedString, Task, Window, div, prelude::*,
    px, rgb,
};

use crate::{
    terminal_server::HELPER,
    theme,
    ui::{self, Variant},
};

use super::Workspace;

/// System Settings, Privacy & Security, Files & Folders.
const FILES_AND_FOLDERS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders";

#[derive(Default)]
pub(super) struct DiskAccess {
    /// What the last check found: `None` before it, or when the server keeps the app's privacy responsibility.
    pub(super) allowed: Option<bool>,
    /// The dialog that asks for it, while open.
    pub(super) asking: Option<FocusHandle>,
    /// System Settings was opened for it: coming back checks again.
    pub(super) granting: bool,
    checking: Option<Task<()>>,
}

impl Workspace {
    /// Checks in the background whether the terminals can read other apps' data, and asks once if they can't.
    pub(super) fn check_disk_access(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            let Some(servers) = self.config.terminal_server.clone() else {
                return;
            };
            if self.disk_access.checking.is_some() || !servers.0.is_on() {
                return;
            }
            let check = cx
                .background_executor()
                .spawn(async move { servers.0.app_data_access() });
            self.disk_access.checking = Some(cx.spawn(async move |this, cx| {
                let allowed = check.await;
                let _ = this.update(cx, |this, cx| this.disk_access_checked(allowed, cx));
            }));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
    }

    /// What a check found: asks for access once if the terminals don't have it.
    pub fn disk_access_checked(&mut self, allowed: Option<bool>, cx: &mut Context<Self>) {
        let access = &mut self.disk_access;
        access.checking = None;
        access.allowed = allowed;
        if allowed == Some(true) {
            access.granting = false;
        }
        if allowed == Some(false) && !self.prefs.app_data_asked {
            let focus = cx.focus_handle();
            self.disk_access.asking = Some(focus.clone());
            self.set_prefs(|prefs| prefs.app_data_asked = true, cx);
            // The dialog takes the keys, so Escape closes it. The check ends with no window at hand.
            let workspace = cx.entity();
            cx.defer(move |cx| {
                cx.with_window(workspace.entity_id(), |window, cx| focus.focus(window, cx));
            });
        }
        cx.notify();
    }

    /// Opens Files & Folders in System Settings.
    pub(super) fn open_files_and_folders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.open_url(FILES_AND_FOLDERS);
        self.disk_access.granting = true;
        self.close_disk_access(window, cx);
    }

    /// Closes the dialog, if open, and gives the keys back to the tab shown.
    pub(super) fn close_disk_access(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.disk_access.asking.take() {
            self.leave("app-data-access", focus, ui::motion::POPUP_IN, cx);
            self.show(self.active, window, cx);
        }
    }

    /// "Let terminals read other apps' data?", once, when the launch's check finds they can't.
    pub(super) fn disk_access_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let focus = self.disk_access.asking.as_ref()?;
        let (backdrop, panel) = ui::dialog(
            "app-data-access",
            "Let terminals read other apps' data?",
            focus,
            480.0,
            None,
        );
        let muted = |text: SharedString| {
            div()
                .text_size(px(13.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(text)
        };
        Some(
            backdrop
                .child(ui::dialog_panel(
                    panel
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                this.close_disk_access(window, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(
                            ui::dialog_body()
                                .child(ui::dialog_header(
                                    "Let terminals read other apps' data?",
                                    muted(
                                        "Programs in Wings' terminals can't read other apps' data, like Chrome's \
                                         profile that the Chrome DevTools MCP needs, and macOS never asks."
                                            .into(),
                                    ),
                                ))
                                .child(muted(
                                    format!(
                                        "In Files & Folders, find {HELPER} and turn on the app whose data they \
                                         need, like Google Chrome. You can also do this later in Settings, General."
                                    )
                                    .into(),
                                ))
                                .child(
                                    ui::dialog_footer()
                                        .child(
                                            ui::button(
                                                "app-data-access-later",
                                                "Not now",
                                                Variant::Ghost,
                                                false,
                                                "Not now",
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.close_disk_access(window, cx)
                                            })),
                                        )
                                        .child(
                                            ui::button(
                                                "app-data-access-open",
                                                "Open Files & Folders",
                                                Variant::Default,
                                                false,
                                                "Open Files & Folders",
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_files_and_folders(window, cx)
                                            })),
                                        ),
                                ),
                        ),
                ))
                .into_any_element(),
        )
    }

    /// The row in Settings, General: whether the terminals have it, and the same button as the dialog's. Only where the
    /// terminal server owns its privacy responsibility.
    pub(super) fn disk_access_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        #[cfg(unix)]
        let owned = self
            .config
            .terminal_server
            .as_ref()
            .is_some_and(|servers| servers.0.owns_responsibility());
        #[cfg(not(unix))]
        let owned = false;
        if !cfg!(target_os = "macos") || !owned || !self.terminal_server_on() {
            return None;
        }
        let control = match self.disk_access.allowed {
            Some(true) => div()
                .text_size(px(13.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child("Allowed")
                .into_any_element(),
            _ => ui::button(
                "app-data-access-settings",
                "Open Files & Folders",
                Variant::Secondary,
                false,
                "Open Files & Folders",
            )
            .on_click(cx.listener(|this, _, window, cx| this.open_files_and_folders(window, cx)))
            .into_any_element(),
        };
        Some(ui::setting_row(
            "Let terminals read other apps' data",
            format!(
                "Programs in terminals, like the Chrome DevTools MCP reading Chrome's profile, need that app's data \
                 turned on for {HELPER} in Files & Folders, and macOS never asks. The button opens the list."
            ),
            None,
            control,
        ))
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::super::tests::open_config;

    /// Asked once: a later launch that finds the same doesn't ask again, and finding access closes nothing it opened.
    #[test]
    fn terminals_refused_other_apps_data_are_asked_once() {
        let (_app, mut window) = open_config(|_| {});
        window.update(|w, _, cx| w.disk_access_checked(Some(false), cx));
        window.read(|w, _| {
            assert!(w.disk_access.asking.is_some(), "the dialog asks");
            assert!(w.prefs.app_data_asked);
        });
        window.update(|w, window, cx| w.close_disk_access(window, cx));
        window.update(|w, _, cx| w.disk_access_checked(Some(false), cx));
        window.read(|w, _| assert!(w.disk_access.asking.is_none(), "not again"));

        let (_app, mut window) = open_config(|_| {});
        window.update(|w, _, cx| {
            w.disk_access.granting = true;
            w.disk_access_checked(Some(true), cx);
        });
        window.read(|w, _| {
            assert!(w.disk_access.asking.is_none());
            assert_eq!(w.disk_access.allowed, Some(true));
            assert!(!w.disk_access.granting, "nothing left to check for");
        });
        // A check that can't tell asks nothing.
        window.update(|w, _, cx| w.disk_access_checked(None, cx));
        window.read(|w, _| assert!(w.disk_access.asking.is_none()));
    }
}
