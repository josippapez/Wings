//! A central diff overlay. Only viewport rows become GPUI elements.
use super::{
    Update, View,
    patch::{DiffLineKind, DiffStatus},
};
use crate::{icons::Icon, page::plugin::PluginHost, theme, ui};
use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Role,
    ScrollStrategy, Task, UniformListScrollHandle, Window, div, prelude::*, px, rgb, rgba,
    uniform_list,
};
use std::{sync::Arc, time::Duration};

pub enum Event {
    Close,
}

#[derive(Clone, Copy)]
enum Row {
    Hunk(usize),
    Line(usize, usize),
}

pub struct Viewer {
    pub view: View,
    host: Arc<PluginHost>,
    selected: usize,
    rows: Vec<Row>,
    code_width: f32,
    scroll: UniformListScrollHandle,
    files_scroll: UniformListScrollHandle,
    horizontal: gpui::ScrollHandle,
    focus: FocusHandle,
    files_focus: FocusHandle,
    code_focus: FocusHandle,
    refresh_focus: FocusHandle,
    close_focus: FocusHandle,
    refreshing: bool,
    refresh_error: Option<String>,
    refresh_task: Option<Task<()>>,
    timeout: Option<Task<()>>,
    #[cfg(feature = "test-support")]
    rendered_rows: std::cell::Cell<usize>,
}

impl EventEmitter<Event> for Viewer {}
impl Focusable for Viewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.host.diff_closed(&self.view.id);
    }
}

impl Viewer {
    pub fn new(view: View, host: Arc<PluginHost>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            view,
            host,
            selected: 0,
            rows: Vec::new(),
            code_width: 0.0,
            scroll: UniformListScrollHandle::new(),
            files_scroll: UniformListScrollHandle::new(),
            horizontal: gpui::ScrollHandle::new(),
            focus: cx.focus_handle(),
            files_focus: cx.focus_handle().tab_stop(true),
            code_focus: cx.focus_handle().tab_stop(true),
            refresh_focus: cx.focus_handle().tab_stop(true),
            close_focus: cx.focus_handle().tab_stop(true),
            refreshing: false,
            refresh_error: None,
            refresh_task: None,
            timeout: None,
            #[cfg(feature = "test-support")]
            rendered_rows: std::cell::Cell::new(0),
        };
        this.rebuild();
        this
    }

    pub fn update(&mut self, update: Update, cx: &mut Context<Self>) {
        if let Some(patch) = update.patch {
            self.view.update.patch = Some(patch);
            self.view.update.error = None;
        }
        if let Some(error) = update.error {
            self.view.update.error = Some(error);
        }
        if let Some(focus) = update.focus {
            self.view.update.focus = Some(focus);
        }
        self.rebuild();
        cx.notify();
    }

    fn rebuild(&mut self) {
        self.rows.clear();
        let Some(diff) = &self.view.update.patch else {
            return;
        };
        if let Some(focus) = &self.view.update.focus
            && let Some(index) = diff.files.iter().position(|file| {
                file.new_path.as_deref() == Some(&focus.path)
                    || file.old_path.as_deref() == Some(&focus.path)
            })
        {
            self.selected = index;
        }
        self.selected = self.selected.min(diff.files.len().saturating_sub(1));
        let Some(file) = diff.files.get(self.selected) else {
            return;
        };
        let mut width = 0;
        let mut target = None;
        for (h, hunk) in file.hunks.iter().enumerate() {
            self.rows.push(Row::Hunk(h));
            for (l, line) in hunk.lines.iter().enumerate() {
                if let Some(focus) = &self.view.update.focus {
                    let number = if focus.old {
                        line.old_line
                    } else {
                        line.new_line
                    };
                    if focus.line.is_some() && number == focus.line {
                        target = Some(self.rows.len());
                    }
                }
                width = width.max(line.text.chars().count());
                self.rows.push(Row::Line(h, l));
            }
        }
        self.code_width = (width as f32 * 8.0 + 140.0).max(400.0);
        self.scroll
            .scroll_to_item(target.unwrap_or(0), ScrollStrategy::Top);
    }

    pub fn select_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self
            .view
            .update
            .patch
            .as_ref()
            .is_some_and(|diff| index < diff.files.len())
        {
            self.selected = index;
            self.view.update.focus = None;
            self.rebuild();
            self.horizontal.set_offset(gpui::point(px(0.0), px(0.0)));
            self.files_scroll
                .scroll_to_item(index, ScrollStrategy::Center);
            cx.notify();
        }
    }

    pub fn selected_file(&self) -> usize {
        self.selected
    }
    #[cfg(feature = "test-support")]
    pub fn rendered_code_rows(&self) -> usize {
        self.rendered_rows.get()
    }

    pub fn request_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.view.refresh || self.refreshing {
            return;
        }
        self.refreshing = true;
        self.refresh_error = None;
        let (sent, received) = async_channel::bounded(1);
        self.host.send_diff_refresh(&self.view.id, move |result| {
            let _ = sent.try_send(result);
        });
        self.refresh_task = Some(cx.spawn(async move |this, cx| {
            if let Ok(result) = received.recv().await {
                let _ = this.update(cx, |this, cx| {
                    this.refreshing = false;
                    this.refresh_error = result.err();
                    this.timeout = None;
                    cx.notify();
                });
            }
        }));
        self.timeout = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(310))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.refreshing = false;
                this.refresh_error = Some("The plugin didn't respond".into());
                this.refresh_task = None;
                this.host.cancel_diff_refresh(&this.view.id);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let pressed = matches!(key.key.as_str(), "enter" | "space");
        if key.key == "escape"
            || (key.modifiers.platform && key.key == "w")
            || (pressed && self.close_focus.is_focused(window))
        {
            cx.emit(Event::Close);
        } else if (pressed && self.refresh_focus.is_focused(window))
            || (key.modifiers.platform && key.key == "r")
        {
            self.request_refresh(cx);
        } else if key.key == "tab" {
            if key.modifiers.shift {
                window.focus_prev(cx);
            } else {
                window.focus_next(cx);
            }
        } else if self.files_focus.is_focused(window) {
            let count = self
                .view
                .update
                .patch
                .as_ref()
                .map_or(0, |diff| diff.files.len());
            let index = match key.key.as_str() {
                "down" => self.selected.saturating_add(1).min(count.saturating_sub(1)),
                "up" => self.selected.saturating_sub(1),
                "home" => 0,
                "end" => count.saturating_sub(1),
                _ => return,
            };
            self.select_file(index, cx);
        } else if self.code_focus.is_focused(window) {
            if matches!(key.key.as_str(), "left" | "right") {
                let mut offset = self.horizontal.offset();
                offset.x += px(if key.key == "left" { 40.0 } else { -40.0 });
                self.horizontal.set_offset(offset);
                window.prevent_default();
                cx.stop_propagation();
                cx.notify();
                return;
            }
            let handle = self.scroll.0.borrow().base_handle.clone();
            let mut offset = handle.offset();
            offset.y = match key.key.as_str() {
                "down" => offset.y - px(22.0),
                "up" => offset.y + px(22.0),
                "pagedown" => offset.y - handle.bounds().size.height,
                "pageup" => offset.y + handle.bounds().size.height,
                "home" => px(0.0),
                "end" => -handle.max_offset().y,
                _ => return,
            };
            handle.set_offset(offset);
            cx.notify();
        } else {
            return;
        }
        cx.stop_propagation();
        window.prevent_default();
    }

    fn code_row(&self, index: usize) -> AnyElement {
        let Some(file) = self
            .view
            .update
            .patch
            .as_ref()
            .and_then(|diff| diff.files.get(self.selected))
        else {
            return div().into_any_element();
        };
        let row = div()
            .h(px(22.0))
            .w_full()
            .flex()
            .items_center()
            .text_size(px(13.0))
            .font_family(crate::terminal_view::DEFAULT_FONT_FAMILY)
            .whitespace_nowrap();
        match self.rows[index] {
            Row::Hunk(h) => {
                let hunk = &file.hunks[h];
                row.bg(rgba(theme::INFO << 8 | 0x16))
                    .text_color(rgb(theme::INFO))
                    .px(px(12.0))
                    .child(format!(
                        "@@ -{},{} +{},{} @@ {}",
                        hunk.old_range.start,
                        hunk.old_range.len,
                        hunk.new_range.start,
                        hunk.new_range.len,
                        hunk.function_context.as_deref().unwrap_or("")
                    ))
                    .into_any_element()
            }
            Row::Line(h, l) => {
                let line = &file.hunks[h].lines[l];
                let (mark, color, bg) = match line.kind {
                    DiffLineKind::Added => ("+", theme::DONE, 0x7dd3a712),
                    DiffLineKind::Deleted => ("-", theme::DANGER, 0xf2786a12),
                    DiffLineKind::Context => (" ", theme::FOREGROUND, 0),
                };
                row.bg(rgba(bg))
                    .text_color(rgb(color))
                    .child(
                        div()
                            .w(px(48.0))
                            .flex_none()
                            .text_right()
                            .pr(px(8.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(line.old_line.map(|n| n.to_string()).unwrap_or_default()),
                    )
                    .child(
                        div()
                            .w(px(48.0))
                            .flex_none()
                            .text_right()
                            .pr(px(8.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(line.new_line.map(|n| n.to_string()).unwrap_or_default()),
                    )
                    .child(div().w(px(22.0)).flex_none().child(mark))
                    .child(format!(
                        "{}{}",
                        line.text,
                        if line.missing_newline {
                            "  ⏎ No newline at end of file"
                        } else {
                            ""
                        }
                    ))
                    .into_any_element()
            }
        }
    }

    fn content(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(diff) = &self.view.update.patch else {
            return self.message("Loading diff…", false);
        };
        if diff.files.is_empty() {
            return self.message("No changed files", false);
        }
        let files = uniform_list(
            "diff-files",
            diff.files.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                let diff = this.view.update.patch.as_ref().unwrap();
                range
                    .map(|index| {
                        let file = &diff.files[index];
                        let path = file
                            .new_path
                            .as_ref()
                            .or(file.old_path.as_ref())
                            .cloned()
                            .unwrap_or_default();
                        let mark = match file.status {
                            DiffStatus::Added => "A",
                            DiffStatus::Deleted => "D",
                            DiffStatus::Modified => "M",
                            DiffStatus::Renamed => "R",
                            DiffStatus::Copied => "C",
                        };
                        div()
                            .id(index)
                            .h(px(32.0))
                            .w_full()
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .bg(rgba(if index == this.selected {
                                theme::HOVER_STRONG
                            } else {
                                0
                            }))
                            .cursor_pointer()
                            .role(Role::ListBoxOption)
                            .aria_label(path.clone())
                            .aria_selected(index == this.selected)
                            .hover(|style| style.bg(rgba(theme::HOVER)))
                            .child(mark)
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(path),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.files_focus.focus(window, cx);
                                this.select_file(index, cx);
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.files_scroll)
        .flex_1()
        .min_h_0();
        let file = &diff.files[self.selected];
        let code = if self.rows.is_empty() {
            let detail = if file.is_binary {
                "Binary file changed. No text diff."
            } else if file.is_placeholder {
                "This file is too large to show."
            } else {
                match file.status {
                    DiffStatus::Added => "Empty file added.",
                    DiffStatus::Deleted => "Empty file deleted.",
                    DiffStatus::Renamed => "File renamed. No changed text lines.",
                    DiffStatus::Copied => "File copied. No changed text lines.",
                    DiffStatus::Modified => "File metadata changed. No changed text lines.",
                }
            };
            self.message(detail, false)
        } else {
            div()
                .id("diff-code-scroll")
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_x_scroll()
                .track_scroll(&self.horizontal)
                .child(
                    uniform_list(
                        "diff-code",
                        self.rows.len(),
                        cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                            #[cfg(feature = "test-support")]
                            this.rendered_rows
                                .set(this.rendered_rows.get() + range.len());
                            range.map(|index| this.code_row(index)).collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.scroll)
                    .w(px(self.code_width))
                    .min_w(gpui::relative(1.0))
                    .h_full(),
                )
                .into_any_element()
        };
        let path = file
            .new_path
            .as_ref()
            .or(file.old_path.as_ref())
            .cloned()
            .unwrap_or_default();
        let renamed = file
            .old_path
            .as_ref()
            .filter(|old| file.new_path.as_ref().is_some_and(|new| new != *old))
            .map(|old| format!("{old} → {path}"))
            .unwrap_or(path);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(
                div()
                    .id("diff-file-panel")
                    .w(px(200.0))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(rgba(theme::BORDER))
                    .track_focus(&self.files_focus)
                    .role(Role::ListBox)
                    .aria_label("Changed files")
                    .focus_visible(|style| style.border_color(rgb(theme::INFO)))
                    .child(files),
            )
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
                    .focus_visible(|style| style.border_1().border_color(rgb(theme::INFO)))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.code_focus.focus(window, cx)),
                    )
                    .child(
                        div()
                            .h(px(32.0))
                            .flex_none()
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .text_size(px(12.0))
                            .bg(rgb(theme::SURFACE))
                            .child(renamed),
                    )
                    .child(code),
            )
            .into_any_element()
    }

    fn message(&self, text: &str, error: bool) -> AnyElement {
        div()
            .id("diff-message")
            .flex_1()
            .min_h_0()
            .p(px(24.0))
            .role(if error { Role::Alert } else { Role::Status })
            .text_color(rgb(if error {
                theme::DANGER
            } else {
                theme::MUTED_FOREGROUND
            }))
            .child(text.to_owned())
            .into_any_element()
    }
}

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_focus = self.refresh_focus.clone().tab_stop(!self.refreshing);
        #[cfg(feature = "test-support")]
        self.rendered_rows.set(0);
        let content = if let Some(error) = &self.view.update.error {
            self.message(error, true)
        } else {
            self.content(cx)
        };
        div()
            .id("native-diff")
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::BACKGROUND))
            .text_color(rgb(theme::FOREGROUND))
            .text_size(px(13.0))
            .role(Role::Dialog)
            .aria_label(self.view.title.clone())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .child(
                div()
                    .h(px(66.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(16.0))
                    .border_b_1()
                    .border_color(rgba(theme::BORDER))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(self.view.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(self.view.subtitle.clone()),
                            ),
                    )
                    .when(self.view.refresh, |bar| {
                        bar.child(
                            ui::button(
                                "diff-refresh",
                                "Refresh diff",
                                ui::Variant::Ghost,
                                self.refreshing,
                                if self.refreshing {
                                    "Refreshing…"
                                } else {
                                    "Refresh"
                                },
                            )
                            .track_focus(&self.refresh_focus)
                            .on_click(cx.listener(|this, _, _, cx| this.request_refresh(cx))),
                        )
                    })
                    .child(
                        ui::icon_button("diff-close", Icon::X, "Close diff", ui::ButtonSize::Sm)
                            .track_focus(&self.close_focus)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close))),
                    ),
            )
            .when_some(self.refresh_error.clone(), |body, error| {
                body.child(
                    div()
                        .id("diff-refresh-error")
                        .px(px(16.0))
                        .py(px(8.0))
                        .role(Role::Alert)
                        .text_color(rgb(theme::DANGER))
                        .child(error),
                )
            })
            .child(content)
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use crate::{diff, page::plugin::PluginConfig};
    use gpui::{Bounds, MouseButton, TestApp, WindowBounds, WindowOptions, point, size};

    #[test]
    fn large_patches_render_only_viewport_rows_and_file_selection_works_by_keyboard() {
        let patch = format!(
            "diff --git a/large b/large\nnew file mode 100644\n--- /dev/null\n+++ b/large\n@@ -0,0 +1,10000 @@\n{}diff --git a/empty b/empty\nnew file mode 100644\n",
            "+line\n".repeat(10_000)
        );
        let view = View {
            id: "test:1".into(),
            plugin_id: "test".into(),
            title: "Large diff".into(),
            subtitle: String::new(),
            cwd: String::new(),
            refresh: false,
            update: diff::clean_update(&serde_json::json!({ "patch":patch })).unwrap(),
        };
        let host = Arc::new(PluginHost::new(PluginConfig {
            id: "test".into(),
            root: std::env::temp_dir(),
            scope: "test".into(),
        }));
        let mut app = TestApp::new();
        app.background_executor().allow_parking();
        let mut window = app.open_window_with_options(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(1000.0), px(640.0)),
                ))),
                ..Default::default()
            },
            move |window, cx| {
                let viewer = Viewer::new(view, host, cx);
                viewer.focus_handle(cx).focus(window, cx);
                viewer
            },
        );
        window.draw();
        window.read(|viewer, _| {
            assert_eq!(viewer.rows.len(), 10_001);
            assert!(
                (1..100).contains(&viewer.rendered_code_rows()),
                "eagerly rendered {} rows",
                viewer.rendered_code_rows()
            );
        });
        window.simulate_click(point(px(80.0), px(82.0)), MouseButton::Left);
        window.simulate_keystroke("down");
        assert_eq!(window.read(|viewer, _| viewer.selected_file()), 1);
        window.simulate_keystroke("up");
        assert_eq!(window.read(|viewer, _| viewer.selected_file()), 0);
        window.simulate_keystroke("tab");
        window.simulate_keystroke("end");
        window.draw();
        assert!(
            window.read(|viewer, _| viewer.scroll.0.borrow().base_handle.offset().y)
                < px(-100_000.0)
        );
    }
}
