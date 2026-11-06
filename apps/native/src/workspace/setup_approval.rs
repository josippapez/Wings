//! "Allow this setup command?": an agent asks, with `worktree_setup` or `wings worktree-setup`, to set the command that
//! runs in every new worktree of a project, and nothing is saved until you allow it. The desktop's
//! `setup-approval-dialog.tsx`: one prompt at a time, the others waiting behind it, and Allow only answers once the
//! prompt has been on screen a moment, so a click meant for one can't land on the next.

use std::{sync::mpsc, time::Duration};

use gpui::{AnyElement, Context, FontWeight, KeyDownEvent, Task, div, prelude::*, px, rgb, rgba};
use wings_connect::panes::SETUP_APPROVAL_WAIT;

use crate::{
    theme,
    ui::{self, Variant},
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
            answer,
        });
        if asks.queue.len() == 1 {
            self.arm_setup_prompt(cx);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETUP_APPROVAL_WAIT).await;
            let _ = this.update(cx, |this, cx| {
                let first = this.setup_asks.queue.first().map(|ask| ask.id);
                this.setup_asks.queue.retain(|ask| ask.id != id);
                if first == Some(id) && !this.setup_asks.queue.is_empty() {
                    this.arm_setup_prompt(cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
        answered
    }

    /// Allow waits a moment each time the prompt shown changes.
    fn arm_setup_prompt(&mut self, cx: &mut Context<Self>) {
        self.setup_asks.armed = false;
        self.setup_asks.arm = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ARM_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                this.setup_asks.armed = true;
                cx.notify();
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
        }
        cx.notify();
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
        let (backdrop, panel) = ui::dialog(
            "setup-approval",
            "Allow this setup command?",
            &self.review_focus,
            512.0,
            400.0,
        );
        Some(
            backdrop
                .child(
                    panel.h_auto()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" {
                                this.answer_setup(false, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(px(16.0))
                                .p(px(24.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.0))
                                        .child(div().text_size(px(16.0)).font_weight(FontWeight::SEMIBOLD).child("Allow this setup command?"))
                                        .child(
                                            div().text_size(px(13.0)).text_color(rgb(theme::MUTED_FOREGROUND)).child(format!(
                                                "{} asks to set the setup command for {}.{}",
                                                ask.pane,
                                                ask.project,
                                                if waiting > 0 { format!(" {waiting} more waiting.") } else { String::new() }
                                            )),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("setup-approval-commands")
                                        .max_h(px(240.0))
                                        .overflow_y_scroll()
                                        .flex()
                                        .flex_col()
                                        .gap(px(12.0))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(4.0))
                                                .child(label("Now"))
                                                .child(code(ask.current.clone().unwrap_or_else(|| "none".into()), true)),
                                        )
                                        .child(
                                            div().flex().flex_col().gap(px(4.0)).child(label("Proposed, exactly as it will run")).child(code(
                                                if ask.proposed.is_empty() { "(clear it)".into() } else { ask.proposed.clone() },
                                                false,
                                            )),
                                        )
                                        .child(label("It runs in every new worktree of this project before the agent starts.")),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap(px(8.0))
                                        .child(
                                            ui::button("setup-approval-deny", "Deny", Variant::Ghost, false, "Deny")
                                                .on_click(cx.listener(|this, _, _, cx| this.answer_setup(false, cx))),
                                        )
                                        .child(
                                            ui::button("setup-approval-allow", "Allow", Variant::Default, !asks.armed, "Allow")
                                                .when(asks.armed, |button| {
                                                    button.on_click(cx.listener(|this, _, _, cx| this.answer_setup(true, cx)))
                                                }),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}
