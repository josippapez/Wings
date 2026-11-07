//! The app updater's dialog and retained, infrequent background work.
use super::Workspace;
use crate::{
    theme,
    ui::{self, Variant, motion},
    updater::{self, Available, Prepared},
};
use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, KeyDownEvent, Task, Window, div, prelude::*, px,
    rgb,
};
use std::time::Duration;

fn restart_warning(server_configured: bool) -> &'static str {
    if server_configured {
        "Terminals running in the background keep going. Others, and their agents, may stop. Save your work before \
         installing."
    } else {
        "Restarting may stop running terminals and agents. Save your work before installing."
    }
}

enum State {
    Idle,
    Checking,
    Current,
    Available(Available),
    Downloading,
    Ready(Prepared),
    Installing,
    Error(String),
    #[cfg(feature = "test-support")]
    SampleAvailable,
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use crate::workspace::tests::open_config;

    #[test]
    fn duplicate_install_clicks_do_not_launch_another_installer() {
        let (_app, mut window) = open_config(|_| {});
        window.update(|w, window, cx| {
            w.updates.state = State::Installing;
            w.install_update(window, cx);
            w.install_update(window, cx);
            assert!(matches!(w.updates.state, State::Installing));
            assert!(w.updates.task.is_none());
        });
    }

    #[test]
    fn a_configured_server_does_not_promise_that_fallback_panes_survive() {
        assert!(restart_warning(true).contains("Others, and their agents, may stop"));
        assert!(restart_warning(true).contains("may stop"));
        assert!(restart_warning(false).contains("may stop running terminals"));
    }

    #[test]
    fn update_dialog_restores_focus_and_busy_checks_do_not_replace_work() {
        let (_app, mut window) = open_config(|_| {});
        window.update(|w, window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            w.updates.state = State::Downloading;
            w.check_updates(true, window, cx);
            assert!(w.updates.open);
            assert!(w.updates.focus.is_focused(window));
            assert!(matches!(w.updates.state, State::Downloading));
            assert!(w.updates.task.is_none());
            w.close_updates(window, cx);
            assert!(focus.is_focused(window));
        });
    }

    #[test]
    fn tab_stays_on_update_controls_and_escape_closes_the_dialog() {
        let (_app, mut window) = open_config(|_| {});
        window.update(|w, window, cx| {
            w.updates.state = State::Current;
            w.show_updates(window, cx);
        });
        window.draw();
        window.simulate_keystroke("tab");
        window.update(|w, window, _| assert!(w.updates.close_focus.is_focused(window)));
        window.draw();
        window.simulate_keystroke("tab");
        window.update(|w, window, _| assert!(w.updates.action_focus.is_focused(window)));
        window.draw();
        window.simulate_keystroke("tab");
        window.update(|w, window, _| assert!(w.updates.close_focus.is_focused(window)));
        window.simulate_keystroke("escape");
        assert!(!window.read(|w, _| w.updates.open));
    }
}

pub(super) struct Updates {
    state: State,
    open: bool,
    focus: FocusHandle,
    close_focus: FocusHandle,
    action_focus: FocusHandle,
    previous: Option<FocusHandle>,
    task: Option<Task<()>>,
    timer: Option<Task<()>>,
}
impl Updates {
    pub(super) fn new(cx: &mut Context<Workspace>) -> Self {
        Self {
            state: State::Idle,
            open: false,
            focus: cx.focus_handle(),
            close_focus: cx.focus_handle().tab_stop(true),
            action_focus: cx.focus_handle().tab_stop(true),
            previous: None,
            task: None,
            timer: None,
        }
    }
    fn busy(&self) -> bool {
        matches!(
            self.state,
            State::Checking | State::Downloading | State::Installing
        )
    }
}

impl Workspace {
    pub(super) fn start_update_checks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cfg!(debug_assertions) || !cfg!(target_os = "macos") {
            return;
        }
        // Checking bundle eligibility and its plist is disk work too.
        self.updates.timer = Some(cx.spawn_in(window, async move |this, cx| {
            let bundled = cx
                .background_executor()
                .spawn(async { updater::destination().is_ok() })
                .await;
            if !bundled {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| this.check_updates(false, window, cx));
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(6 * 60 * 60))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| {
                        if this.window_active {
                            this.check_updates(false, window, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn show_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.updates.open {
            self.updates.previous = window.focused(cx);
            self.updates.open = true;
            window.focus(&self.updates.focus, cx);
        }
        cx.notify();
    }

    fn close_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.updates.open {
            self.leave("update", (), motion::POPUP_IN, cx);
        }
        self.updates.open = false;
        if let Some(focus) = self.updates.previous.take() {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    pub(super) fn check_updates(
        &mut self,
        manual: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if manual {
            self.show_updates(window, cx);
        }
        if self.updates.busy()
            || matches!(self.updates.state, State::Available(_) | State::Ready(_))
        {
            return;
        }
        self.updates.state = State::Checking;
        cx.notify();
        let work = cx.background_executor().spawn(async {
            updater::destination()?;
            updater::check()
        });
        self.updates.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.updates.state = match result {
                    Ok(Some(update)) => {
                        this.show_updates(window, cx);
                        State::Available(update)
                    }
                    Ok(None) => State::Current,
                    Err(error) => State::Error(error),
                };
                cx.notify();
            });
        }));
    }

    fn download_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Available(update) = &self.updates.state else {
            return;
        };
        let update = update.clone();
        self.updates.state = State::Downloading;
        cx.notify();
        let work = cx
            .background_executor()
            .spawn(async move { updater::prepare(&update, &updater::destination()?) });
        self.updates.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, _, cx| {
                this.updates.state = match result {
                    Ok(prepared) => State::Ready(prepared),
                    Err(error) => State::Error(error),
                };
                cx.notify();
            });
        }));
    }

    fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.updates.state, State::Ready(_)) {
            return;
        }
        let State::Ready(prepared) = std::mem::replace(&mut self.updates.state, State::Installing)
        else {
            unreachable!()
        };
        cx.notify();
        let work = cx
            .background_executor()
            .spawn(async move { prepared.launch_installer() });
        self.updates.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, _, cx| match result {
                // GPUI's normal quit runs the existing workspace and history save hooks.
                Ok(()) => cx.quit(),
                Err(error) => {
                    this.updates.state = State::Error(error);
                    cx.notify();
                }
            });
        }));
    }

    /// The updates dialog, open or on its way out after it closed.
    pub(super) fn update_layer(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, motion::Phase)> {
        if self.updates.open {
            return self
                .update_dialog(cx)
                .map(|dialog| (dialog, motion::Phase::In));
        }
        self.leaving(
            "update",
            false,
            |this, ()| this.updates.open = true,
            |this| std::mem::take(&mut this.updates.open).then_some(()),
            Self::update_dialog,
            cx,
        )
    }

    pub(super) fn update_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.updates.open {
            return None;
        }
        #[cfg(unix)]
        let server_configured = self
            .config
            .terminal_server
            .as_ref()
            .is_some_and(|servers| servers.0.is_on());
        #[cfg(not(unix))]
        let server_configured = false;
        let (title, message, action) = match &self.updates.state {
            State::Idle => ("Wings updates".to_owned(), "Check for a newer native release.".to_owned(), Some("Check again")),
            State::Checking => ("Checking for updates".into(), "Contacting the native release feed…".into(), None),
            State::Current => ("Wings is up to date".into(), format!("You're running {} {}.", crate::data_dir::APP_NAME, updater::VERSION), Some("Check again")),
            State::Available(update) => (format!("{} {} is available", crate::data_dir::APP_NAME, update.release.version), if update.release.notes.is_empty() { "A newer native release is ready to download.".into() } else { update.release.notes.clone() }, Some("Download update")),
            State::Downloading => ("Downloading and verifying".into(), "Checking the signed archive and native app bundle. You can keep working.".into(), None),
            State::Ready(prepared) => (format!("{} {} is ready", crate::data_dir::APP_NAME, prepared.version), "The update is authenticated. Install it when you're ready to restart Wings.".into(), Some("Install and restart")),
            State::Installing => ("Preparing to restart".into(), "Starting the installer. Wings will save your workspace before it quits.".into(), None),
            State::Error(error) => ("Couldn't update Wings".into(), error.clone(), Some("Retry")),
            #[cfg(feature = "test-support")]
            State::SampleAvailable => ("Wings Native 0.2.0 is available".into(), "Native app updates now download and verify in the background.\nYour saved projects and workspace stay in place.".into(), Some("Download update")),
        };
        let (backdrop, panel) = ui::dialog(
            "native-update",
            title.clone(),
            &self.updates.focus,
            480.0,
            360.0,
        );
        Some(
            backdrop
                .child(ui::dialog_panel(
                    panel
                        .h_auto()
                        .flex_col()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "tab" {
                                let to_action = !this.updates.busy()
                                    && this.updates.close_focus.is_focused(window);
                                window.focus(
                                    if to_action {
                                        &this.updates.action_focus
                                    } else {
                                        &this.updates.close_focus
                                    },
                                    cx,
                                );
                                cx.stop_propagation();
                            } else if event.keystroke.key == "escape" {
                                this.close_updates(window, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(
                            div()
                                .p(px(24.0))
                                .flex()
                                .flex_col()
                                .gap(px(16.0))
                                .child(
                                    div()
                                        .text_size(px(18.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .id("update-message")
                                        .max_h(px(180.0))
                                        .overflow_y_scroll()
                                        .text_size(px(14.0))
                                        .child(message),
                                )
                                .child(
                                    div()
                                        .text_color(rgb(theme::MUTED_FOREGROUND))
                                        .child(restart_warning(server_configured)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap(px(8.0))
                                        .child(
                                            ui::button(
                                                "update-close",
                                                "Not now",
                                                Variant::Ghost,
                                                false,
                                                "Not now",
                                            )
                                            .track_focus(&self.updates.close_focus)
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    this.close_updates(window, cx)
                                                }),
                                            ),
                                        )
                                        .children(action.map(|action| {
                                            ui::button(
                                                "update-action",
                                                action,
                                                Variant::Default,
                                                false,
                                                action,
                                            )
                                            .track_focus(&self.updates.action_focus)
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    match this.updates.state {
                                                        State::Available(_) => {
                                                            this.download_update(window, cx)
                                                        }
                                                        State::Ready(_) => {
                                                            this.install_update(window, cx)
                                                        }
                                                        _ => this.check_updates(true, window, cx),
                                                    }
                                                }),
                                            )
                                        })),
                                ),
                        ),
                ))
                .into_any_element(),
        )
    }

    #[cfg(feature = "test-support")]
    pub fn sample_update(&mut self, state: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.updates.state = match state {
            "available" => State::SampleAvailable,
            "error" => State::Error("Update authentication failed. The downloaded metadata does not match the trusted updater key. Check again or contact the release publisher.".into()),
            _ => State::Downloading,
        };
        self.show_updates(window, cx);
    }
}
