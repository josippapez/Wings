//! What to do first, shown when no project is open, as the desktop's `welcome.tsx`. The native app always opens the
//! folder it was started in, so this shows once the last project has been removed from the sidebar.

use gpui::{AnyElement, Context, FontWeight, div, prelude::*, px, rgb, rgba};

use crate::{
    actions::TogglePalette,
    icons::{Icon, icon},
    theme,
    ui::{self, Variant},
};

use super::{Workspace, palette::shortcut_label};

impl Workspace {
    /// The last project is gone: its tabs close, shells and all, and the window shows what to do first.
    pub(super) fn show_welcome(&mut self, cx: &mut Context<Self>) {
        for tab in std::mem::take(&mut self.terminals) {
            for pane in tab.layout.panes() {
                self.panes.remove(&pane);
            }
        }
        self.plugin_tabs.clear();
        self.project_page = None;
        self.issues_open = false;
        self.welcome = true;
        self.active = super::Active::Issues;
        self.sync_plugin_visibility(cx);
        cx.notify();
    }

    pub(super) fn welcome(&self, cx: &mut Context<Self>) -> AnyElement {
        let steps: [(&str, Option<String>); 3] = [
            (
                "Add a project: pick a folder, and Wings opens a terminal in it.",
                None,
            ),
            (
                "Run claude or opencode in a pane. Wings shows the agent's state in the sidebar and on its tab.",
                None,
            ),
            (
                "Press the command palette key to find any command, project, tab or past session. It lists every \
                 shortcut.",
                shortcut_label(&TogglePalette, cx),
            ),
        ];
        let steps = steps.into_iter().enumerate().map(|(index, (text, keys))| {
            div()
                .flex()
                .items_start()
                .gap(px(12.0))
                .child(
                    div()
                        .mt(px(1.0))
                        .size(px(20.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(rgba(theme::HOVER_STRONG))
                        .text_size(px(11.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::FOREGROUND))
                        .child((index + 1).to_string()),
                )
                .child(div().flex_1().min_w_0().child(text))
                .children(keys.map(kbd))
        });
        div()
            .id("welcome")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(24.0))
            .p(px(24.0))
            .child(
                div()
                    .max_w(px(448.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Welcome to Wings"),
                    )
                    .child(
                        div()
                            .text_size(px(14.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child("A terminal workspace for coding agents."),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .max_w(px(448.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .text_size(px(13.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .children(steps),
            )
            .children(self.prefs.global_hotkey.enabled.then(|| {
                let hotkey = crate::hotkey::label(
                    &self.prefs.global_hotkey.accelerator,
                    cfg!(target_os = "macos"),
                );
                div()
                    .max_w(px(448.0))
                    .text_center()
                    .text_size(px(12.0))
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child(format!(
                        "From any app, {hotkey} brings Wings forward and goes to that agent. Change it in \
                         Settings, Keyboard."
                    ))
            }))
            .child(
                ui::button(
                    "welcome-add-project",
                    "Add a project",
                    Variant::Default,
                    false,
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(icon(Icon::FolderPlus, 16.0, 0x17151a))
                        .child("Add a project"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.choose_project(window, cx))),
            )
            .into_any_element()
    }
}

/// shadcn's `Kbd`: a key in a small pill.
fn kbd(keys: String) -> AnyElement {
    div()
        .flex_none()
        .h(px(20.0))
        .px(px(6.0))
        .flex()
        .items_center()
        .rounded(px(4.0))
        .bg(rgba(theme::HOVER_STRONG))
        .text_size(px(12.0))
        .text_color(rgb(theme::FOREGROUND))
        .child(keys)
        .into_any_element()
}
