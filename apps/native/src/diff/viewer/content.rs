//! What fills a viewer: the file's rows, the wide-code scroller and the messages.
use super::*;

impl Viewer {
    pub(super) fn content(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(diff) = &self.view.update.patch else {
            return self.message("Loading diff…", false);
        };
        if diff.files.is_empty() && self.discussion.is_empty() {
            return self.message("No changed files", false);
        }
        if self.stacked() {
            return self.stack_content(cx);
        }
        let files = uniform_list(
            "diff-files",
            self.list_rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| this.list_row(index, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.files_scroll)
        .flex_1()
        .min_h_0();
        let file = diff.files.get(self.selected);
        let code = if self.discussion_open {
            list(
                self.discussion_list.clone(),
                cx.processor(|this, index: usize, _, cx| this.discussion_item(index, cx)),
            )
            .flex_1()
            .min_h_0()
            .into_any_element()
        } else if let Some(file) = file.filter(|_| self.rows.is_empty()) {
            self.message(no_lines(file), false)
        } else if file.is_none() {
            self.message("No changed files", false)
        } else if self.varied_rows() {
            let rows = list(
                self.wrapped.clone(),
                cx.processor(|this, index: usize, _, cx| this.code_row(index, cx)),
            );
            if self.wrap {
                rows.flex_1().min_h_0().into_any_element()
            } else {
                self.sideways(rows.size_full(), cx)
            }
        } else {
            let rows = uniform_list(
                "diff-code",
                self.rows.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    #[cfg(feature = "test-support")]
                    this.rendered_rows
                        .set(this.rendered_rows.get() + range.len());
                    range
                        .map(|index| this.code_row(index, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.scroll)
            .h_full();
            if self.split() {
                self.sideways(rows.w_full(), cx)
            } else {
                div()
                    .id("diff-code-scroll")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_x_scroll()
                    // Else GPUI turns a vertical wheel into sideways movement here, as nothing scrolls vertically.
                    .restrict_scroll_to_axis()
                    .track_scroll(&self.horizontal)
                    .child(rows.w(px(self.code_width)).min_w(gpui::relative(1.0)))
                    .children(crate::ui::cutoff::clips_on_purpose())
                    .into_any_element()
            }
        };
        let renamed = match file {
            _ if self.discussion_open => "Discussion".to_owned(),
            None => String::new(),
            Some(file) => {
                let path = file_path(file).to_owned();
                file.old_path
                    .as_ref()
                    .filter(|old| file.new_path.as_ref().is_some_and(|new| new != *old))
                    .map(|old| format!("{old} → {path}"))
                    .unwrap_or(path)
            }
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .when(!self.docked || self.dock_files, |body| {
                body.child(
                    div()
                        .id("diff-file-panel")
                        // The desktop's `w-56` in the dock and `w-72` when the diff fills the window.
                        .w(px(if self.docked { 224.0 } else { 288.0 }))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .border_r_1()
                        .border_color(rgba(theme::border()))
                        .child(self.list_header(cx))
                        .child(
                            div()
                                .id("diff-file-list")
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .border_1()
                                .border_color(rgba(0))
                                .track_focus(&self.files_focus)
                                .role(Role::ListBox)
                                .aria_label("Changed files")
                                .focus_visible(|style| style.border_color(rgb(theme::info())))
                                .when(!self.discussion.is_empty(), |list| {
                                    list.child(self.discussion_row(cx))
                                })
                                .child(files),
                        ),
                )
            })
            .child(
                div()
                    .id("diff-code-panel")
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .track_focus(&self.code_focus)
                    .role(Role::Region)
                    .aria_label("Diff code")
                    .focus_visible(|style| style.border_1().border_color(rgb(theme::info())))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.code_focus.focus(window, cx)),
                    )
                    .children(ui::spot(|| "diff-code".into()))
                    .child(
                        div()
                            .h(px(32.0))
                            .flex_none()
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .text_size(px(12.0))
                            .bg(rgb(theme::surface()))
                            .child(renamed),
                    )
                    .child(code),
            )
            .into_any_element()
    }

    /// Code that scrolls sideways a line at a time rather than as one wide block: each column, or each line's text,
    /// clips its own, and Shift and the wheel move them together.
    pub(super) fn sideways(&self, rows: impl IntoElement, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("diff-code-scroll")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(ROW));
                let by = if event.modifiers.shift {
                    delta.y
                } else {
                    delta.x
                };
                if by != px(0.0) {
                    this.scroll_split(by, cx);
                }
            }))
            .child(rows)
            .children(crate::ui::cutoff::clips_on_purpose())
            .into_any_element()
    }

    pub(super) fn message(&self, text: &str, error: bool) -> AnyElement {
        div()
            .id("diff-message")
            .flex_1()
            .min_h_0()
            .p(px(24.0))
            .role(if error { Role::Alert } else { Role::Status })
            .text_color(rgb(if error {
                theme::danger()
            } else {
                theme::muted_foreground()
            }))
            .child(text.to_owned())
            .into_any_element()
    }
}
