//! The app updater's dialog, as the desktop's `UpdateDialog` (`apps/desktop/src/components/update-dialog.tsx`) with the
//! states its toasts show (`apps/desktop/src/lib/updater.ts`), and its retained, infrequent background work.
use super::Workspace;
use crate::{
    icons::{Icon, icon},
    theme,
    ui::{self, ButtonSize, Variant, motion},
    updater::{self, Available, Prepared},
};
use gpui::{
    AnimationExt, AnyElement, Context, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton,
    Render, SharedString, Task, Window, div, prelude::*, px, relative, rgb, rgba,
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

/// How often the download's bar moves on at most. A download arrives in thousands of small pieces; each step draws the
/// bar again, and ten a second reads as smooth for a bar that takes seconds to fill.
const PROGRESS_STEP: Duration = Duration::from_millis(100);
/// The desktop's toast titles for a failed check and a failed install.
const CHECK_FAILED: &str = "Couldn't check for updates";
const INSTALL_FAILED: &str = "Couldn't install the update";

enum State {
    Idle,
    Checking,
    Current,
    Available(Available),
    /// Downloading the update, then verifying it. How far it is, `Updates::busy` draws.
    Downloading(Available),
    /// The installer is starting; Wings quits once it has.
    Installing(Available),
    /// `retry` is the update whose download or install failed, to try again without checking first: GitHub can refuse
    /// a release's files for a few minutes after it is published.
    Error {
        title: &'static str,
        message: String,
        retry: Option<Available>,
    },
}

/// What the dialog says in a state.
#[derive(Debug, PartialEq)]
struct Content {
    title: String,
    description: String,
    /// The footer's outline button, which closes the dialog, and its main one. No footer while Wings works.
    dismiss: Option<&'static str>,
    action: Option<&'static str>,
}

/// What the dialog shows while Wings works on the update: a turning ring while it checks or restarts, and the bytes
/// of the update received so far while it downloads. A view of its own, so a step of either draws only it again, not
/// the window around the dialog.
pub(super) struct Busy {
    activity: Activity,
    /// How many times the download moved the bar on, for tests of how often it draws.
    #[cfg(test)]
    steps: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Activity {
    Spinning(&'static str),
    Downloading { received: u64, total: u64 },
}

/// `4.2 MB of 13.5 MB`, as Finder counts megabytes.
fn progress_value(received: u64, total: u64) -> String {
    let mb = |bytes: u64| format!("{:.1} MB", bytes as f64 / 1_000_000.0);
    if received >= total {
        mb(total)
    } else {
        format!("{} of {}", mb(received), mb(total))
    }
}

impl Render for Busy {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match self.activity {
            Activity::Spinning(label) => div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(
                    div()
                        .id("update-busy")
                        .role(gpui::Role::Status)
                        .aria_label(label)
                        .size(px(16.0))
                        .flex_none()
                        .with_animation(
                            "update-busy-turn",
                            gpui::Animation::new(Duration::from_secs(1))
                                .repeat()
                                .with_max_fps(30.0),
                            |ring, turn| {
                                ring.child(ui::working_spinner(turn * std::f32::consts::TAU))
                            },
                        ),
                )
                .child(label)
                .into_any_element(),
            // The desktop's `Progress` with a `ProgressLabel` and `ProgressValue`: the words above a 4px bar.
            Activity::Downloading { received, total } => {
                let done = received >= total;
                div()
                    .id("update-progress")
                    .role(gpui::Role::ProgressIndicator)
                    .aria_label(if done { "Verifying" } else { "Downloading" })
                    .flex()
                    .flex_wrap()
                    .gap(px(12.0))
                    .child(div().font_weight(FontWeight::MEDIUM).child(if done {
                        "Verifying"
                    } else {
                        "Downloading"
                    }))
                    .child(
                        div()
                            .ml_auto()
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(progress_value(received, total)),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(4.0))
                            .rounded_full()
                            .overflow_hidden()
                            .bg(rgba(MUTED))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(if total == 0 {
                                        0.0
                                    } else {
                                        received.min(total) as f32 / total as f32
                                    }))
                                    .bg(rgb(theme::FOREGROUND)),
                            ),
                    )
                    .into_any_element()
            }
        }
    }
}

/// The desktop's `--muted`, the bar's track, and at half that the dialog footer's fill.
const MUTED: u32 = 0xffffff0f;
const FOOTER: u32 = 0xffffff08;

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use crate::workspace::tests::open_config;

    fn sample() -> Available {
        Available::sample("9.0.0", "- Faster checks\n- A progress bar")
    }

    #[test]
    fn duplicate_install_clicks_do_not_launch_another_installer() {
        let (_app, mut window) = open_config(|_| {});
        window.update(|w, window, cx| {
            w.updates.state = State::Installing(sample());
            w.update_action(window, cx);
            w.update_action(window, cx);
            assert!(matches!(w.updates.state, State::Installing(_)));
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
            w.updates.state = State::Downloading(sample());
            w.check_updates(true, window, cx);
            assert!(w.updates.open);
            assert!(w.updates.focus.is_focused(window));
            assert!(matches!(w.updates.state, State::Downloading(_)));
            assert!(w.updates.task.is_none());
            w.close_updates(window, cx);
            assert!(focus.is_focused(window));
        });
    }

    #[test]
    fn tab_stays_on_update_controls_and_escape_closes_the_dialog() {
        let (_app, mut window) = open_config(|_| {});
        window.update(|w, window, cx| {
            w.updates.state = State::Available(sample());
            w.show_updates(window, cx);
        });
        for focused in [
            |w: &Workspace, window: &Window| w.updates.dismiss_focus.is_focused(window),
            |w: &Workspace, window: &Window| w.updates.action_focus.is_focused(window),
            |w: &Workspace, window: &Window| w.updates.close_focus.is_focused(window),
            |w: &Workspace, window: &Window| w.updates.dismiss_focus.is_focused(window),
        ] {
            window.draw();
            window.simulate_keystroke("tab");
            window.update(|w, window, _| assert!(focused(w, window)));
        }
        window.simulate_keystroke("escape");
        assert!(!window.read(|w, _| w.updates.open));
    }

    /// The desktop's dialog for an update, its toasts' words for the rest, and a footer only where there's a choice.
    #[test]
    fn each_state_shows_the_desktops_words_and_draws() {
        let (_app, mut window) = open_config(|_| {});
        let app = crate::data_dir::APP_NAME;
        let running = updater::VERSION;
        let cases = [
            (
                State::Checking,
                Content {
                    title: "Checking for updates".into(),
                    description: format!("You're running {app} {running}."),
                    dismiss: None,
                    action: None,
                },
            ),
            (
                State::Current,
                Content {
                    title: format!("{app} is up to date"),
                    description: format!("You're running {app} {running}."),
                    dismiss: Some("Close"),
                    action: None,
                },
            ),
            (
                State::Available(sample()),
                Content {
                    title: format!("{app} 9.0.0"),
                    description: format!("{running} → 9.0.0"),
                    dismiss: Some("Later"),
                    action: Some("Install and restart"),
                },
            ),
            (
                State::Downloading(sample()),
                Content {
                    title: format!("Installing {app} 9.0.0"),
                    description: format!("{running} → 9.0.0"),
                    dismiss: None,
                    action: None,
                },
            ),
            (
                State::Installing(sample()),
                Content {
                    title: format!("Installing {app} 9.0.0"),
                    description: format!("{running} → 9.0.0"),
                    dismiss: None,
                    action: None,
                },
            ),
            (
                State::Error {
                    title: INSTALL_FAILED,
                    message: "GitHub couldn't send the update just now (HTTP 504).".into(),
                    retry: Some(sample()),
                },
                Content {
                    title: "Couldn't install the update".into(),
                    description: "GitHub couldn't send the update just now (HTTP 504).".into(),
                    dismiss: Some("Close"),
                    action: Some("Retry"),
                },
            ),
        ];
        for (state, content) in cases {
            window.update(|w, window, cx| {
                w.updates.state = state;
                w.show_updates(window, cx);
                assert_eq!(w.updates.content(), content);
            });
            window.draw();
        }
    }

    #[test]
    fn the_download_shows_bytes_received_of_the_total_then_verifying() {
        assert_eq!(progress_value(4_200_000, 13_529_763), "4.2 MB of 13.5 MB");
        assert_eq!(progress_value(13_529_763, 13_529_763), "13.5 MB");
    }

    /// The bar moves on at most once a step however many pieces arrive, and draws only itself, not the window's pieces.
    #[test]
    fn download_progress_redraws_only_the_dialog_and_at_most_once_a_step() {
        let (app, mut window) = open_config(|_| {});
        let (send, receive) = async_channel::bounded(1);
        window.update(|w, window, cx| {
            w.updates.state = State::Downloading(sample());
            w.set_activity(
                Activity::Downloading {
                    received: 0,
                    total: 13_529_763,
                },
                cx,
            );
            w.show_updates(window, cx);
            w.report_progress(receive, cx);
        });
        window.draw();
        let drawn = |window: &mut gpui::TestAppWindow<Workspace>| {
            window.read(|w, cx| {
                let parts = w.parts.as_ref().expect("the pieces");
                (
                    parts.main.read(cx).drawn,
                    parts.sidebar.read(cx).drawn,
                    w.updates.busy.read(cx).steps,
                    w.updates.busy.read(cx).activity,
                )
            })
        };
        let (main, sidebar, steps, _) = drawn(&mut window);
        for piece in 1..=1000u64 {
            send.force_send(piece * 8192).unwrap();
        }
        app.run_until_parked();
        window.draw();
        let after = drawn(&mut window);
        assert_eq!(
            (after.0, after.1),
            (main, sidebar),
            "the window kept its drawing"
        );
        assert_eq!(after.2, steps + 1, "one step for a thousand pieces");
        assert_eq!(
            after.3,
            Activity::Downloading {
                received: 1000 * 8192,
                total: 13_529_763
            }
        );
        send.force_send(13_529_763).unwrap();
        app.run_until_parked();
        assert_eq!(drawn(&mut window).2, steps + 1, "waits out the step");
        app.advance_clock(PROGRESS_STEP);
        app.run_until_parked();
        window.draw();
        let last = drawn(&mut window);
        assert_eq!(last.2, steps + 2);
        assert_eq!((last.0, last.1), (main, sidebar));
    }

    /// As the desktop's `checkForUpdate`: an automatic check's find sounds, shows its toast and the sidebar's row, and
    /// leaves the dialog closed; the row opens it.
    #[test]
    fn an_automatic_find_shows_the_toast_and_the_sidebar_row_and_not_the_dialog() {
        use crate::workspace::sound::PLAYED;
        let (_app, mut window) = open_config(|_| {});
        PLAYED.with(|played| played.borrow_mut().clear());
        window.update(|w, window, cx| w.checked(false, Ok(Some(sample())), window, cx));
        window.draw();
        assert!(!window.read(|w, _| w.updates.open));
        assert_eq!(PLAYED.with(|played| played.borrow().len()), 1);
        assert!(
            crate::ui::spot_at("update-whats-new").is_some(),
            "the toast"
        );
        let row = crate::ui::spot_at("update-app").expect("the sidebar's row");
        window.simulate_click(row.center(), gpui::MouseButton::Left);
        assert!(window.read(|w, _| w.updates.open));
        assert!(window.read(|w, _| matches!(w.updates.state, State::Available(_))));
    }

    #[test]
    fn a_manual_find_opens_the_dialog_without_a_toast() {
        let (_app, mut window) = open_config(|_| {});
        window.update(|w, window, cx| w.checked(true, Ok(Some(sample())), window, cx));
        window.draw();
        window.read(|w, _| {
            assert!(w.updates.open);
            assert!(!w.updates.toast);
        });
    }

    /// The toast's What's new opens the dialog, and its Install and restart starts the download there.
    #[test]
    fn the_toast_opens_the_dialog_or_installs() {
        let (app, mut window) = open_config(|_| {});
        window.update(|w, window, cx| w.checked(false, Ok(Some(sample())), window, cx));
        window.draw();
        let whats_new = crate::ui::spot_at("update-whats-new").unwrap();
        window.simulate_click(whats_new.center(), gpui::MouseButton::Left);
        window.read(|w, _| assert!(w.updates.open && !w.updates.toast));
        window.update(|w, window, cx| {
            w.close_updates(window, cx);
            w.updates.toast = true;
            cx.notify();
        });
        // The closing dialog covers the toast until it has gone.
        app.advance_clock(Duration::from_secs(1));
        app.run_until_parked();
        window.draw();
        let install = crate::ui::spot_at("update-install").unwrap();
        window.simulate_click(install.center(), gpui::MouseButton::Left);
        window.read(|w, _| {
            assert!(w.updates.open && !w.updates.toast);
            // A test build is no app bundle, so the download it started already failed.
            assert!(matches!(
                w.updates.state,
                State::Downloading(_)
                    | State::Error {
                        title: INSTALL_FAILED,
                        ..
                    }
            ));
        });
    }

    /// A download GitHub refused (a 504 right after a release) is tried again from Retry without a new check.
    #[test]
    fn retry_after_a_failed_download_downloads_again() {
        let (_app, mut window) = open_config(|_| {});
        window.update(|w, window, cx| {
            w.updates.state = State::Error {
                title: INSTALL_FAILED,
                message: "504".into(),
                retry: Some(sample()),
            };
            w.update_action(window, cx);
            assert!(matches!(&w.updates.state, State::Downloading(update) if update.release.version == "9.0.0"));
            assert_eq!(
                w.updates.busy.read(cx).activity,
                Activity::Downloading {
                    received: 0,
                    total: 13_529_763
                }
            );
        });
    }
}

pub(super) struct Updates {
    state: State,
    open: bool,
    focus: FocusHandle,
    /// The corner's ×, the footer's outline button and its main one.
    close_focus: FocusHandle,
    dismiss_focus: FocusHandle,
    action_focus: FocusHandle,
    previous: Option<FocusHandle>,
    task: Option<Task<()>>,
    progress: Option<Task<()>>,
    timer: Option<Task<()>>,
    busy: Entity<Busy>,
    /// The desktop's "Wings X is available" toast, shown when an automatic check finds an update.
    toast: bool,
}
impl Updates {
    /// Made with the workspace, before its first draw: GPUI only draws a view again on its own if it's older than the
    /// frame it was first drawn in (`parts`).
    pub(super) fn new(cx: &mut Context<Workspace>) -> Self {
        Self {
            state: State::Idle,
            open: false,
            focus: cx.focus_handle(),
            close_focus: cx.focus_handle().tab_stop(true),
            dismiss_focus: cx.focus_handle().tab_stop(true),
            action_focus: cx.focus_handle().tab_stop(true),
            previous: None,
            task: None,
            progress: None,
            timer: None,
            busy: cx.new(|_| Busy {
                activity: Activity::Spinning(""),
                #[cfg(test)]
                steps: 0,
            }),
            toast: false,
        }
    }

    /// The update a check found and that isn't installed yet, as the desktop's `availableUpdate`: the sidebar offers it.
    fn pending(&self) -> Option<&Available> {
        match &self.state {
            State::Available(update)
            | State::Downloading(update)
            | State::Installing(update)
            | State::Error {
                retry: Some(update),
                ..
            } => Some(update),
            State::Idle | State::Checking | State::Current | State::Error { retry: None, .. } => {
                None
            }
        }
    }

    fn busy(&self) -> bool {
        matches!(
            self.state,
            State::Checking | State::Downloading(_) | State::Installing(_)
        )
    }

    fn content(&self) -> Content {
        let app = crate::data_dir::APP_NAME;
        let running = format!("You're running {app} {}.", updater::VERSION);
        let change =
            |update: &Available| format!("{} → {}", updater::VERSION, update.release.version);
        let (title, description, dismiss, action) = match &self.state {
            State::Idle | State::Checking => ("Checking for updates".into(), running, None, None),
            State::Current => (format!("{app} is up to date"), running, Some("Close"), None),
            State::Available(update) => (
                format!("{app} {}", update.release.version),
                change(update),
                Some("Later"),
                Some("Install and restart"),
            ),
            State::Downloading(update) | State::Installing(update) => (
                format!("Installing {app} {}", update.release.version),
                change(update),
                None,
                None,
            ),
            State::Error { title, message, .. } => (
                (*title).into(),
                message.clone(),
                Some("Close"),
                Some("Retry"),
            ),
        };
        Content {
            title,
            description,
            dismiss,
            action,
        }
    }

    /// The dialog's controls in the order Tab moves through them: the footer's, then the corner's ×.
    fn controls(&self) -> Vec<FocusHandle> {
        let content = self.content();
        content
            .dismiss
            .map(|_| self.dismiss_focus.clone())
            .into_iter()
            .chain(content.action.map(|_| self.action_focus.clone()))
            .chain([self.close_focus.clone()])
            .collect()
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

    fn set_activity(&mut self, activity: Activity, cx: &mut Context<Self>) {
        self.updates.busy.update(cx, |busy, cx| {
            busy.activity = activity;
            cx.notify();
        });
    }

    pub(super) fn check_updates(
        &mut self,
        manual: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if manual {
            // The desktop's manual check replaces the toast with its own.
            self.updates.toast = false;
            self.show_updates(window, cx);
        }
        if self.updates.busy() || matches!(self.updates.state, State::Available(_)) {
            return;
        }
        self.updates.state = State::Checking;
        self.set_activity(Activity::Spinning("Looking for a newer release…"), cx);
        cx.notify();
        let work = cx.background_executor().spawn(async {
            updater::destination()?;
            updater::check()
        });
        self.updates.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.checked(manual, result, window, cx)
            });
        }));
    }

    /// What a check found, as the desktop's `checkForUpdate`: the update you asked for opens its dialog, and one an
    /// automatic check found sounds and shows its toast, with the sidebar's row to come back to it.
    fn checked(
        &mut self,
        manual: bool,
        result: updater::Result<Option<Available>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.updates.state = match result {
            Ok(Some(update)) => {
                if manual {
                    self.show_updates(window, cx);
                } else {
                    self.notification_sound(false);
                    self.updates.toast = true;
                }
                State::Available(update)
            }
            Ok(None) => State::Current,
            Err(message) => State::Error {
                title: CHECK_FAILED,
                message,
                retry: None,
            },
        };
        cx.notify();
    }

    /// The desktop's "Update to Wings X" row above Settings, while an update waits: it opens the dialog.
    pub(in crate::workspace) fn update_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let update = self.updates.pending()?;
        Some(
            super::sidebar::nav_row_with(
                "update-app",
                icon(Icon::CircleArrowUp, 16.0, theme::DONE),
                format!(
                    "Update to {} {}",
                    crate::data_dir::APP_NAME,
                    update.release.version
                ),
                super::sidebar::Look::Plain,
            )
            .text_color(rgb(theme::FOREGROUND))
            .tooltip(ui::tooltip("See what's new and install it", None))
            .children(ui::spot(|| "update-app".into()))
            .on_click(cx.listener(|this, _, window, cx| this.show_updates(window, cx)))
            .into_any_element(),
        )
    }

    /// The desktop's update toast, at the bottom of the notices: the release's first line of notes, What's new to open
    /// the dialog and Install and restart to install it there.
    pub(super) fn update_toast(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let update = self.updates.pending().filter(|_| self.updates.toast)?;
        let notes = update
            .release
            .notes
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned();
        let button = |id: &'static str, label: &'static str, variant: Variant| {
            ui::button(id, label, variant, false, label).children(ui::spot(move || id.into()))
        };
        Some(
            div()
                .id("update-toast")
                .role(gpui::Role::Alert)
                .aria_label(format!(
                    "{} {} is available",
                    crate::data_dir::APP_NAME,
                    update.release.version
                ))
                .relative()
                .p(px(12.0))
                .rounded(px(12.0))
                .border_1()
                .border_color(rgba(ui::HAIRLINE))
                .bg(rgba(theme::POPOVER))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .pr(px(20.0))
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!(
                            "{} {} is available",
                            crate::data_dir::APP_NAME,
                            update.release.version
                        )),
                )
                .when(!notes.is_empty(), |card| {
                    card.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(notes),
                    )
                })
                .child(
                    ui::dialog_footer()
                        .child(
                            button("update-whats-new", "What's new", Variant::Ghost).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.updates.toast = false;
                                    this.show_updates(window, cx);
                                }),
                            ),
                        )
                        .child(
                            button("update-install", "Install and restart", Variant::Default)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.updates.toast = false;
                                    this.show_updates(window, cx);
                                    this.update_action(window, cx);
                                })),
                        ),
                )
                // Sonner's close button.
                .child(
                    div().absolute().top(px(6.0)).right(px(6.0)).child(
                        ui::icon_button("update-toast-close", Icon::X, "Close", ButtonSize::Tiny)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.updates.toast = false;
                                cx.notify();
                            })),
                    ),
                )
                .into_any_element(),
        )
    }

    /// The footer's main button: installs an update, tries a failed one again, or checks again.
    fn update_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.updates.state {
            State::Available(update)
            | State::Error {
                retry: Some(update),
                ..
            } => {
                let update = update.clone();
                self.install_update(update, window, cx);
            }
            State::Current | State::Error { retry: None, .. } => {
                self.check_updates(true, window, cx)
            }
            State::Idle | State::Checking | State::Downloading(_) | State::Installing(_) => {}
        }
    }

    /// The desktop's Install and restart: downloads, verifies and installs the update, then restarts.
    fn install_update(&mut self, update: Available, window: &mut Window, cx: &mut Context<Self>) {
        let total = update.release.size;
        self.updates.state = State::Downloading(update.clone());
        self.set_activity(Activity::Downloading { received: 0, total }, cx);
        cx.notify();
        let (send, receive) = async_channel::bounded(1);
        let download = update.clone();
        let work = cx.background_executor().spawn(async move {
            updater::prepare(&download, &updater::destination()?, |received| {
                // Only the newest count waits for the bar; the bar takes it at its own pace.
                let _ = send.force_send(received);
            })
        });
        self.report_progress(receive, cx);
        self.updates.task = Some(cx.spawn_in(window, async move |this, cx| {
            let prepared = work.await;
            let _ = this.update_in(cx, |this, window, cx| match prepared {
                Ok(prepared) => this.launch_installer(prepared, update, window, cx),
                Err(message) => {
                    this.updates.state = State::Error {
                        title: INSTALL_FAILED,
                        message,
                        retry: Some(update),
                    };
                    cx.notify();
                }
            });
        }));
    }

    /// Moves the bar on as the download's counts arrive, at most once each `PROGRESS_STEP`, drawing only the bar.
    fn report_progress(&mut self, receive: async_channel::Receiver<u64>, cx: &mut Context<Self>) {
        let busy = self.updates.busy.downgrade();
        self.updates.progress = Some(cx.spawn(async move |_, cx| {
            while let Ok(received) = receive.recv().await {
                let moved = busy.update(cx, |busy, cx| {
                    if let Activity::Downloading { total, .. } = busy.activity {
                        busy.activity = Activity::Downloading { received, total };
                        #[cfg(test)]
                        {
                            busy.steps += 1;
                        }
                        cx.notify();
                    }
                });
                if moved.is_err() {
                    break;
                }
                cx.background_executor().timer(PROGRESS_STEP).await;
            }
        }));
    }

    fn launch_installer(
        &mut self,
        prepared: Prepared,
        update: Available,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.updates.state = State::Installing(update.clone());
        self.set_activity(
            Activity::Spinning("Restarting. Wings saves your workspace first."),
            cx,
        );
        // Wings is about to quit, so the dialog says so even if it was closed while the update downloaded.
        self.show_updates(window, cx);
        let work = cx
            .background_executor()
            .spawn(async move { prepared.launch_installer() });
        self.updates.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, _, cx| match result {
                // GPUI's normal quit runs the existing workspace and history save hooks.
                Ok(()) => cx.quit(),
                Err(message) => {
                    this.updates.state = State::Error {
                        title: INSTALL_FAILED,
                        message,
                        retry: Some(update),
                    };
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

    /// The release's notes in Markdown, or a link to its page when it has none, as the desktop's.
    fn update_notes(update: &Available) -> AnyElement {
        let notes = update.release.notes.trim();
        let body = if notes.is_empty() {
            let page = update.release.page();
            div()
                .flex()
                .flex_wrap()
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child("No release notes for this version.\u{a0}")
                .child(
                    div()
                        .id("update-release-page")
                        .role(gpui::Role::Link)
                        .text_color(rgb(theme::FOREGROUND))
                        .underline()
                        .cursor_pointer()
                        .child("See it on GitHub")
                        .on_click(move |_, _, cx| cx.open_url(&page)),
                )
                .into_any_element()
        } else {
            crate::markdown::render("update-notes", &crate::markdown::parse(notes))
        };
        // The desktop's `max-h-[50vh] overflow-y-auto`, at half the shortest window.
        div()
            .id("update-notes")
            .max_h(px(320.0))
            .overflow_y_scroll()
            .child(body)
            .into_any_element()
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
        let content = self.updates.content();
        let body: Option<AnyElement> = match &self.updates.state {
            State::Available(update) => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(Self::update_notes(update))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(restart_warning(server_configured)),
                    )
                    .into_any_element(),
            ),
            State::Idle | State::Checking | State::Downloading(_) | State::Installing(_) => {
                Some(self.updates.busy.clone().into_any_element())
            }
            State::Current | State::Error { .. } => None,
        };
        let (backdrop, panel) = ui::dialog(
            "native-update",
            SharedString::from(content.title.clone()),
            &self.updates.focus,
            448.0,
            None,
        );
        let footer = (content.dismiss.is_some() || content.action.is_some()).then(|| {
            // The desktop's `DialogFooter`: a tinted strip under a hairline, its buttons on the right.
            ui::dialog_footer()
                .p(px(16.0))
                .border_t_1()
                .border_color(rgba(theme::BORDER))
                .bg(rgba(FOOTER))
                // GPUI clips the panel square, so the strip rounds itself inside the panel's 14px corners.
                .rounded_b(px(13.0))
                .children(content.dismiss.map(|label| {
                    ui::button("update-dismiss", label, Variant::Outline, false, label)
                        .track_focus(&self.updates.dismiss_focus)
                        .on_click(cx.listener(|this, _, window, cx| this.close_updates(window, cx)))
                }))
                .children(content.action.map(|label| {
                    ui::button("update-action", label, Variant::Default, false, label)
                        .track_focus(&self.updates.action_focus)
                        .children(ui::spot(|| "update-action".into()))
                        .on_click(cx.listener(|this, _, window, cx| this.update_action(window, cx)))
                }))
        });
        Some(
            backdrop
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_updates(window, cx)),
                )
                .child(ui::dialog_panel(
                    panel
                        .flex_col()
                        .text_size(px(14.0))
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "tab" {
                                let controls = this.updates.controls();
                                let next = controls
                                    .iter()
                                    .position(|focus| focus.is_focused(window))
                                    .map_or(0, |at| (at + 1) % controls.len());
                                window.focus(&controls[next], cx);
                                cx.stop_propagation();
                            } else if event.keystroke.key == "escape" {
                                this.close_updates(window, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .children(ui::spot(|| "update-dialog".into()))
                        .child(
                            div()
                                .p(px(16.0))
                                .flex()
                                .flex_col()
                                .gap(px(16.0))
                                // The desktop's `DialogHeader`, clear of the corner's ×.
                                .child(
                                    div()
                                        .pr(px(28.0))
                                        .flex()
                                        .flex_col()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .text_size(px(16.0))
                                                .line_height(px(16.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(content.title),
                                        )
                                        .child(
                                            div()
                                                .id("update-description")
                                                .max_h(px(180.0))
                                                .overflow_y_scroll()
                                                .text_color(rgb(theme::MUTED_FOREGROUND))
                                                .child(content.description),
                                        ),
                                )
                                .children(body),
                        )
                        .children(footer)
                        .child(
                            div().absolute().top(px(8.0)).right(px(8.0)).child(
                                ui::icon_button("update-close", Icon::X, "Close", ButtonSize::Sm)
                                    .track_focus(&self.updates.close_focus)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_updates(window, cx)
                                    })),
                            ),
                        ),
                ))
                .into_any_element(),
        )
    }

    /// The dialog in `state`, with a made-up release, for the screenshots.
    #[cfg(feature = "test-support")]
    pub fn sample_update(&mut self, state: &str, window: &mut Window, cx: &mut Context<Self>) {
        let update = Available::sample(
            "0.3.0",
            "Updates show their progress as they download, and checking for app and plugin updates takes a moment \
             again.\n\n- **Install and restart** downloads, verifies and installs in one go.\n- Settings, Plugins shows \
             a spinner while it checks.",
        );
        let total = update.release.size;
        if state == "found" {
            // An automatic check's find, without its sound.
            self.updates.state = State::Available(update);
            self.updates.toast = true;
            cx.notify();
            return;
        }
        let (state, activity) = match state {
            "checking" => (
                State::Checking,
                Some(Activity::Spinning("Looking for a newer release…")),
            ),
            "current" => (State::Current, None),
            "available" => (State::Available(update), None),
            "no-notes" => (State::Available(Available::sample("0.3.0", "")), None),
            "downloading" => (
                State::Downloading(update),
                Some(Activity::Downloading {
                    received: total * 37 / 100,
                    total,
                }),
            ),
            "verifying" => (
                State::Downloading(update),
                Some(Activity::Downloading {
                    received: total,
                    total,
                }),
            ),
            "installing" => (
                State::Installing(update),
                Some(Activity::Spinning(
                    "Restarting. Wings saves your workspace first.",
                )),
            ),
            _ => (
                State::Error {
                    title: INSTALL_FAILED,
                    message: "GitHub couldn't send the update just now (HTTP 504). A new release can take a few \
                              minutes to become available. Try again shortly."
                        .into(),
                    retry: Some(update),
                },
                None,
            ),
        };
        self.updates.state = state;
        if let Some(activity) = activity {
            self.set_activity(activity, cx);
        }
        self.show_updates(window, cx);
    }
}
