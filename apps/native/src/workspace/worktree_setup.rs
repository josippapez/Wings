//! "Worktree setup": what a new worktree of a project gets before its agent starts, the local files copied into it and
//! its setup command. The desktop's `worktree-setup-dialog.tsx`, opened from a project's menu.

use std::path::PathBuf;

use gpui::{
    AnyElement, AppContext, Context, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent,
    MouseButton, Role, Subscription, Window, div, prelude::*, px, rgb, rgba,
};

use crate::{
    text_field::{FieldEvent, TextField},
    theme,
    ui::{self, Variant, motion},
};

use super::Workspace;

pub(super) struct WorktreeSetupDialog {
    project: PathBuf,
    name: String,
    focus: FocusHandle,
    include: Entity<TextField>,
    setup: Entity<TextField>,
    /// The repo has a `.worktreeinclude`, which is used instead of the list.
    has_file: bool,
    error: Option<String>,
    _fields: [Subscription; 2],
}

impl Workspace {
    pub(super) fn open_worktree_setup(
        &mut self,
        project: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self
            .projects
            .iter()
            .find(|entry| entry.path == project)
            .map_or_else(
                || project.to_string_lossy().into_owned(),
                |p| p.name.clone(),
            );
        let (include, setup, has_file, error) =
            match self.spaces.worktree_setup(&project.to_string_lossy()) {
                Ok(got) => (
                    got.include.unwrap_or(got.defaults).join("\n"),
                    got.setup.unwrap_or_default(),
                    got.has_include_file,
                    None,
                ),
                Err(error) => (String::new(), String::new(), false, Some(error)),
            };
        let failed = error.is_some();
        let include = cx.new(|cx| {
            let mut field = TextField::new(
                Role::MultilineTextInput,
                "Local files to copy",
                "",
                px(120.0),
                cx,
            );
            field.fill = true;
            field.rows = Some(8);
            field.text = include;
            field.disabled = failed;
            field
        });
        let setup = cx.new(|cx| {
            let mut field = TextField::new(
                Role::TextInput,
                "Setup command",
                "pnpm install",
                px(120.0),
                cx,
            );
            field.fill = true;
            field.text = setup;
            field.disabled = failed;
            field
        });
        let events = |field: &Entity<TextField>, window: &mut Window, cx: &mut Context<Self>| {
            cx.subscribe_in(field, window, |this, _, event, window, cx| match event {
                FieldEvent::Submit { .. } => this.save_worktree_setup(window, cx),
                FieldEvent::Cancel => this.close_worktree_setup(window, cx),
                FieldEvent::Changed => {}
            })
        };
        let subscriptions = [events(&include, window, cx), events(&setup, window, cx)];
        include.focus_handle(cx).focus(window, cx);
        self.worktree_setup = Some(WorktreeSetupDialog {
            project,
            name,
            focus: cx.focus_handle(),
            include,
            setup,
            has_file,
            error,
            _fields: subscriptions,
        });
        cx.notify();
    }

    /// Saves both fields. The default list left as it is stays unset, so a later change to the default reaches the
    /// project (`spaces::include_text`).
    fn save_worktree_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = &self.worktree_setup else {
            return;
        };
        let include = dialog
            .include
            .read(cx)
            .text
            .lines()
            .map(str::to_owned)
            .collect();
        let setup = dialog.setup.read(cx).text.clone();
        let project = dialog.project.to_string_lossy().into_owned();
        match self
            .spaces
            .set_worktree_setup(&project, Some(include), Some(setup))
        {
            Ok(()) => self.close_worktree_setup(window, cx),
            Err(error) => {
                if let Some(dialog) = &mut self.worktree_setup {
                    dialog.error = Some(error);
                }
                cx.notify();
            }
        }
    }

    fn close_worktree_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = self.worktree_setup.take() {
            self.leave("worktree-setup", dialog, motion::POPUP_IN, cx);
        }
        if let Some(pane) = self.focused_pane() {
            self.focus_pane(pane, window, cx);
        }
        cx.notify();
    }

    pub(super) fn worktree_setup_dialog(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.worktree_setup.as_ref()?;
        // The window less the dialog's margins, its heading and its buttons.
        let max_fields = (window.viewport_size().height.as_f32() - 260.0).max(120.0);
        let failed = dialog.include.read(cx).disabled;
        let label = |text: &'static str| {
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .child(text)
        };
        let hint = |text: String| {
            div()
                .text_size(px(12.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(text)
        };
        let file = if dialog.has_file {
            " This repo has a .worktreeinclude, so that file is used and this list is ignored."
        } else {
            " A .worktreeinclude in the repo would take precedence over this list."
        };
        let (backdrop, panel) = ui::dialog(
            "worktree-setup",
            "Worktree setup",
            &dialog.focus,
            512.0,
            0.0,
        );
        Some(
            backdrop
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_worktree_setup(window, cx)),
                )
                .child(ui::dialog_panel(
                    panel
                        .h_auto()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                this.close_worktree_setup(window, cx);
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
                                        .child(
                                            div()
                                                .text_size(px(16.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Worktree setup"),
                                        )
                                        .child(hint(format!(
                                            "What a new worktree of {} gets before its agent starts.",
                                            dialog.name
                                        ))),
                                )
                                .child(
                                    div()
                                        .id("worktree-setup-fields")
                                        // In a window too short for the form, the fields scroll and the buttons stay.
                                        .max_h(px(max_fields))
                                        .overflow_y_scroll()
                                        .flex()
                                        .flex_col()
                                        .gap(px(16.0))
                                        .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.0))
                                        .child(label("Local files to copy"))
                                        .child(dialog.include.clone())
                                        .child(hint(format!(
                                            "One pattern per line, gitignore syntax. Only files your repo ignores are \
                                             copied, never tracked ones. Start a line with ! to leave files out, like \
                                             !.claude/hooks/slow.sh. Lines starting with # are notes.{}",
                                            if failed { "" } else { file }
                                        ))),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.0))
                                        .child(label("Setup command"))
                                        .child(dialog.setup.clone())
                                        .child(hint(
                                            "Any shell line, like ./scripts/setup-worktree.sh or make bootstrap && \
                                             pnpm install. It runs in the new worktree's folder, in the first \
                                             terminal opened there, and the agent starts once it succeeds."
                                                .into(),
                                        )),
                                ),
                                )
                                .when_some(dialog.error.clone(), |form, error| {
                                    form.child(
                                        div()
                                            .px(px(12.0))
                                            .py(px(8.0))
                                            .rounded(px(8.0))
                                            .bg(rgba(theme::DANGER << 8 | 0x1a))
                                            .text_size(px(12.0))
                                            .text_color(rgb(theme::DANGER))
                                            .child(error),
                                    )
                                })
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap(px(8.0))
                                        .child(
                                            ui::button(
                                                "worktree-setup-cancel",
                                                "Cancel",
                                                Variant::Ghost,
                                                false,
                                                "Cancel",
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.close_worktree_setup(window, cx)
                                            })),
                                        )
                                        .child(
                                            ui::button(
                                                "worktree-setup-save",
                                                "Save",
                                                Variant::Default,
                                                failed,
                                                "Save",
                                            )
                                            .when(!failed, |button| {
                                                button.on_click(cx.listener(|this, _, window, cx| {
                                                    this.save_worktree_setup(window, cx)
                                                }))
                                            }),
                                        ),
                                ),
                        ),
                ))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use gpui::Focusable;

    use super::super::tests::open_config;

    /// A project's menu opens the dialog on its setup; Save keeps the command, and the default list left as it is stays
    /// unset.
    #[test]
    fn the_worktree_setup_dialog_saves_a_projects_setup_command() {
        let dir = std::env::temp_dir().join(format!(
            "wings-native-worktree-setup-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let alpha = dir.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        let list = dir.join("spaces.json");
        let spaces = serde_json::json!([{ "id": "a", "name": "alpha", "path": alpha }]);
        std::fs::write(&list, serde_json::to_vec(&spaces).unwrap()).unwrap();
        let (_app, mut window) = open_config(|config| {
            config.project = alpha.clone();
            config.projects = Some(list.clone());
        });
        let project = alpha.to_string_lossy().into_owned();
        window.update(|w, window, cx| w.open_worktree_setup(alpha.clone(), window, cx));
        window.draw();
        window.update(|w, window, cx| {
            let dialog = w.worktree_setup.as_ref().unwrap();
            assert_eq!(dialog.name, "alpha");
            assert!(dialog.include.focus_handle(cx).is_focused(window));
            dialog
                .setup
                .update(cx, |field, _| field.text = "pnpm install".into());
            w.save_worktree_setup(window, cx);
            assert!(w.worktree_setup.is_none(), "Save closes it");
            let saved = w.spaces.worktree_setup(&project).unwrap();
            assert_eq!(saved.setup.as_deref(), Some("pnpm install"));
            assert_eq!(saved.include, None);
        });
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&list).unwrap()).unwrap();
        assert_eq!(stored[0]["worktreeSetup"], "pnpm install");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
