//! "Allow this setup command?": an agent asks, with `worktree_setup` or `wings worktree-setup`, to set the command that
//! runs in every new worktree of a project, and nothing is saved until you allow it. The desktop's
//! `setup-approval-dialog.tsx`: one prompt at a time, the others waiting behind it, and Allow only answers once the
//! prompt has been on screen a moment, so a click meant for one can't land on the next.

use crate::workspace::NotifyWorkspace;
use std::{sync::mpsc, time::Duration};

use gpui::{AnyElement, Context, KeyDownEvent, Task, div, prelude::*, px, rgb, rgba};
use wings_connect::panes::SETUP_APPROVAL_WAIT;

use crate::{
    theme,
    ui::{self, behaviors::{EscapeExt, TrapTabExt}, Variant},
};

use super::Workspace;

/// How long a prompt shows before Allow answers, as the desktop's.
const ARM_AFTER: Duration = Duration::from_secs(1);

pub(super) struct SetupAsk {
    id: u64,
    /// The project's name.
    pub(super) project: String,
    /// Who asks: the agent's name or its pane's title.
    pub(super) pane: String,
    pub(super) current: Option<String>,
    /// Exactly what will run; empty clears it.
    pub(super) proposed: String,
    /// An existing worktree receives this one-time setup, rather than changing project settings.
    existing_folder: Option<String>,
    existing_files: Vec<String>,
    answer: mpsc::Sender<bool>,
}

#[derive(Default)]
pub(super) struct SetupAsks {
    pub(super) queue: Vec<SetupAsk>,
    /// The prompt shown has been up long enough for Allow to count.
    pub(super) armed: bool,
    next: u64,
    arm: Option<Task<()>>,
}

impl Workspace {
    /// Shows the prompt behind any already waiting, and gives the answer to wait for. It goes when the tool call gives
    /// up, `SETUP_APPROVAL_WAIT` from now.
    pub(super) fn ask_setup_approval(
        &mut self,
        project: String,
        pane: String,
        current: Option<String>,
        proposed: String,
        cx: &mut Context<Self>,
    ) -> mpsc::Receiver<bool> {
        let (answer, answered) = mpsc::channel();
        let asks = &mut self.setup_asks;
        let id = asks.next;
        asks.next += 1;
        asks.queue.push(SetupAsk {
            id,
            project,
            pane,
            current,
            proposed,
            existing_folder: None,
            existing_files: Vec::new(),
            answer,
        });
        if asks.queue.len() == 1 {
            self.arm_setup_prompt(cx);
            self.take_review_focus(cx);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETUP_APPROVAL_WAIT).await;
            let _ = this.update(cx, |this, cx| {
                let first = this.setup_asks.queue.first().map(|ask| ask.id);
                this.setup_asks.queue.retain(|ask| ask.id != id);
                if first == Some(id) && !this.setup_asks.queue.is_empty() {
                    this.arm_setup_prompt(cx);
                }
                this.give_back_review_focus(cx);
                cx.notify_workspace();
            });
        })
        .detach();
        cx.notify_workspace();
        answered
    }

    pub(super) fn ask_existing_worktree_setup_approval(
        &mut self,
        project: String,
        pane: String,
        folder: String,
        files: Vec<String>,
        command: Option<String>,
        cx: &mut Context<Self>,
    ) -> mpsc::Receiver<bool> {
        let answered = self.ask_setup_approval(project, pane, None, command.unwrap_or_default(), cx);
        let ask = self.setup_asks.queue.last_mut().expect("the setup approval was queued");
        ask.existing_folder = Some(folder);
        ask.existing_files = files;
        answered
    }

    #[cfg(feature = "test-support")]
    pub fn show_existing_worktree_setup_approval(
        &mut self,
        project: &str,
        folder: &str,
        files: &[&str],
        command: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let _ = self.ask_existing_worktree_setup_approval(
            project.into(),
            "Claude in acme-web".into(),
            folder.into(),
            files.iter().map(|file| (*file).into()).collect(),
            command.map(str::to_string),
            cx,
        );
    }

    /// Allow waits a moment each time the prompt shown changes.
    fn arm_setup_prompt(&mut self, cx: &mut Context<Self>) {
        self.setup_asks.armed = false;
        self.setup_asks.arm = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ARM_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                this.setup_asks.armed = true;
                cx.notify_workspace();
            });
        }));
    }

    /// Answers the prompt shown. Allow before it is armed does nothing.
    pub(super) fn answer_setup(&mut self, allow: bool, cx: &mut Context<Self>) {
        if self.setup_asks.queue.is_empty() || (allow && !self.setup_asks.armed) {
            return;
        }
        let ask = self.setup_asks.queue.remove(0);
        let _ = ask.answer.send(allow);
        if !self.setup_asks.queue.is_empty() {
            self.arm_setup_prompt(cx);
        } else {
            self.leave("setup", ask, crate::ui::motion::POPUP_IN, cx);
            self.give_back_review_focus(cx);
        }
        cx.notify_workspace();
    }

    /// The review or the approval that came up takes the keyboard, so Tab and Escape reach it and not the pane under it.
    /// They come up with no window at hand, so it is taken once the call returns.
    pub(super) fn take_review_focus(&mut self, cx: &mut Context<Self>) {
        let workspace = cx.entity();
        cx.defer(move |cx| {
            cx.with_window(workspace.entity_id(), |window, cx| {
                workspace.update(cx, |this, cx| {
                    let focus = this.review_focus.clone();
                    if !focus.contains_focused(window, cx) && this.review_shown() {
                        this.review_recovery.open(&focus, window, cx);
                    }
                })
            });
        });
    }

    /// Gives the keyboard back to what had it once neither the review nor the approval is left.
    pub(super) fn give_back_review_focus(&mut self, cx: &mut Context<Self>) {
        let workspace = cx.entity();
        cx.defer(move |cx| {
            cx.with_window(workspace.entity_id(), |window, cx| {
                workspace.update(cx, |this, cx| {
                    if !this.review_shown() {
                        let focus = this.review_focus.clone();
                        this.review_recovery.restore_within(&focus, window, cx);
                    }
                })
            });
        });
    }

    fn review_shown(&self) -> bool {
        !self.setup_asks.queue.is_empty() || !self.manage.reviews.is_empty()
    }

    pub(super) fn setup_approval_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let asks = &self.setup_asks;
        let ask = asks.queue.first()?;
        let waiting = asks.queue.len() - 1;
        let code = |text: String, dim: bool| {
            div()
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .bg(rgba(0xffffff0f))
                .font_family("Menlo")
                .text_size(px(12.0))
                .when(dim, |code| code.text_color(rgb(theme::MUTED_FOREGROUND)))
                .child(text)
        };
        let label = |text: &'static str| {
            div()
                .text_size(px(12.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(text)
        };
        let existing = ask.existing_folder.is_some();
        let title = if existing { "Apply this worktree setup?" } else { "Allow this setup command?" };
        let description = if existing {
            format!("{} asks to set up this existing worktree.", ask.pane)
        } else {
            format!("{} asks to set the setup command for {}.", ask.pane, ask.project)
        };
        let details = if let Some(folder) = &ask.existing_folder {
            div()
                .id("setup-approval-existing")
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(div().flex().flex_col().gap(px(4.0)).child(label("Folder")).child(code(folder.clone(), false)))
                .child(div().flex().flex_col().gap(px(4.0)).child(label("Files to copy")).child(code(
                    if ask.existing_files.is_empty() { "none".into() } else { ask.existing_files.join("\n") },
                    false,
                )))
                .child(div().flex().flex_col().gap(px(4.0)).child(label("Command to run")).child(code(
                    if ask.proposed.is_empty() { "none".into() } else { ask.proposed.clone() },
                    false,
                )))
        } else {
            div()
                .id("setup-approval-commands")
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(div().flex().flex_col().gap(px(4.0)).child(label("Now")).child(code(ask.current.clone().unwrap_or_else(|| "none".into()), true)))
                .child(div().flex().flex_col().gap(px(4.0)).child(label("Proposed, exactly as it will run")).child(code(if ask.proposed.is_empty() { "(clear it)".into() } else { ask.proposed.clone() }, false)))
                .child(label("It runs in every new worktree of this project before the agent starts."))
        };
        let (backdrop, panel) = ui::dialog(
            "setup-approval",
            title,
            &self.review_focus,
            512.0,
            None,
        );
        Some(
            backdrop
                .child(ui::dialog_panel(
                    panel
                        .trap_tab(&self.review_focus)
                        .on_escape(cx.listener(|this, _: &KeyDownEvent, _, cx| {
                            this.answer_setup(false, cx);
                        }))
                        .child(
                            ui::dialog_body()
                                .child(ui::dialog_header(
                                    title,
                                    div()
                                        .text_size(px(13.0))
                                        .text_color(rgb(theme::MUTED_FOREGROUND))
                                        .child(format!(
                                            "{}{}",
                                            description,
                                            if waiting > 0 {
                                                format!(" {waiting} more waiting.")
                                            } else {
                                                String::new()
                                            }
                                        )),
                                ))
                                .child(
                                    div()
                                        .id("setup-approval-details")
                                        .max_h(px(240.0))
                                        .overflow_y_scroll()
                                        .child(details),
                                )
                                .child(
                                    ui::dialog_footer()
                                        .child(
                                            ui::button(
                                                "setup-approval-deny",
                                                "Deny",
                                                Variant::Ghost,
                                                false,
                                                "Deny",
                                            )
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.answer_setup(false, cx)
                                                }),
                                            ),
                                        )
                                        .child(
                                            ui::button(
                                                "setup-approval-allow",
                                                "Allow",
                                                Variant::Default,
                                                !asks.armed,
                                                "Allow",
                                            )
                                            .when(
                                                asks.armed,
                                                |button| {
                                                    button.on_click(cx.listener(
                                                        |this, _, _, cx| {
                                                            this.answer_setup(true, cx)
                                                        },
                                                    ))
                                                },
                                            ),
                                        ),
                                ),
                        ),
                ))
                .into_any_element(),
        )
    }
}
