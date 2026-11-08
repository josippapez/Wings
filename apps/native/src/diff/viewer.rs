//! A plugin's diff, docked beside a tab or filling the window. Only viewport rows become GPUI elements.
use super::{
    Update, View,
    patch::{DiffLine, DiffLineKind, DiffStatus},
    rows::{CodeRow, ListRow, code_rows, file_path, list_rows},
};
use crate::{
    icons::{Icon, icon},
    page::plugin::PluginHost,
    text_field::{FieldEvent, TextField},
    theme, ui,
};
use gpui::{
    AnyElement, App, Context, Div, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    KeyDownEvent, ListAlignment, ListOffset, ListState, Pixels, Role, ScrollEvent, ScrollStrategy,
    SharedString, Stateful, Subscription, Task, UniformListScrollHandle, Window, div, list,
    prelude::*, px, rgb, rgba, uniform_list,
};
use std::{collections::HashSet, sync::Arc, time::Duration};

pub enum Event {
    Close,
    /// Wrap or the folder tree was switched, for the app to keep, as the desktop keeps them in `localStorage`.
    Layout {
        wrap: bool,
        tree: bool,
    },
    /// Expand (true) or Dock beside the tab (false) was pressed.
    Expand(bool),
    /// The docked file list was shown or hidden, for the app to keep, as the desktop's `wings.diff.dockFiles`.
    DockFiles(bool),
}

/// A code row's height, unwrapped.
const ROW: f32 = 22.0;
/// About one character of the 13px code font, to size unwrapped code before it's drawn.
const CHAR: f32 = 8.0;

pub struct Viewer {
    pub view: View,
    host: Arc<PluginHost>,
    selected: usize,
    rows: Vec<CodeRow>,
    /// Side by side, else one column, once chosen (`split`).
    split_choice: Option<bool>,
    /// Drawn beside a tab rather than filling the window.
    docked: bool,
    /// Filling the window, it has a tab to dock beside.
    can_dock: bool,
    /// Docked, the file list shows.
    dock_files: bool,
    /// Long lines wrap; else they scroll sideways.
    pub(crate) wrap: bool,
    /// The file list is a folder tree, else a flat list.
    pub(crate) tree: bool,
    /// Folders closed in the tree, by path.
    collapsed: HashSet<String>,
    list_rows: Vec<ListRow>,
    filter: Entity<TextField>,
    _filter_events: Subscription,
    /// Wrapped rows differ in height, so they go in a list that measures each.
    wrapped: ListState,
    /// How far unwrapped side-by-side code is scrolled sideways. Both columns move together.
    split_x: Pixels,
    /// The longest line of the open file, in characters.
    widest: usize,
    /// Each file's added and removed lines, for the file list and the header's totals.
    counts: Vec<(usize, usize)>,
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
        let filter = cx.new(|cx| {
            // Unframed: the row around it draws the frame and the search icon, and clips it.
            TextField::new(
                Role::SearchInput,
                "Filter files",
                "Filter files",
                px(400.0),
                cx,
            )
        });
        let filter_events = cx.subscribe(&filter, |this, _, event: &FieldEvent, cx| {
            if matches!(event, FieldEvent::Changed) {
                this.relist(cx);
                cx.notify();
            }
        });
        let mut this = Self {
            view,
            host,
            selected: 0,
            rows: Vec::new(),
            split_choice: None,
            docked: false,
            can_dock: false,
            dock_files: true,
            wrap: false,
            tree: false,
            collapsed: HashSet::new(),
            list_rows: Vec::new(),
            filter,
            _filter_events: filter_events,
            wrapped: ListState::new(0, ListAlignment::Top, px(200.0)),
            split_x: px(0.0),
            widest: 0,
            counts: Vec::new(),
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
        this.rebuild(cx);
        this
    }

    /// Wrap and the folder tree as last chosen.
    pub fn set_layout(&mut self, wrap: bool, tree: bool, cx: &mut Context<Self>) {
        self.wrap = wrap;
        self.tree = tree;
        self.rebuild(cx);
    }

    pub fn set_split(&mut self, split: bool, cx: &mut Context<Self>) {
        self.split_choice = Some(split);
        self.rebuild(cx);
        cx.notify();
    }

    /// Side by side until you pick, except docked: a dock is usually too narrow for two columns, as the desktop's
    /// `splitChoice ?? !dock`.
    pub fn split(&self) -> bool {
        self.split_choice.unwrap_or(!self.docked)
    }

    /// Beside its tab (`docked`), or filling the window, where `can_dock` offers to go back beside the tab.
    pub fn set_dock(&mut self, docked: bool, can_dock: bool, cx: &mut Context<Self>) {
        let split = self.split();
        self.docked = docked;
        self.can_dock = can_dock;
        if self.split() != split {
            self.rebuild(cx);
        }
        cx.notify();
    }

    /// Whether the docked file list shows, as last chosen.
    pub fn set_dock_files(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.dock_files = shown;
        cx.notify();
    }

    fn choose_dock_files(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.set_dock_files(shown, cx);
        cx.emit(Event::DockFiles(shown));
    }

    fn set_wrap(&mut self, wrap: bool, cx: &mut Context<Self>) {
        self.wrap = wrap;
        self.rebuild(cx);
        cx.emit(Event::Layout {
            wrap,
            tree: self.tree,
        });
        cx.notify();
    }

    fn set_tree(&mut self, tree: bool, cx: &mut Context<Self>) {
        self.tree = tree;
        self.relist(cx);
        cx.emit(Event::Layout {
            wrap: self.wrap,
            tree,
        });
        cx.notify();
    }

    fn toggle_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(path) {
            self.collapsed.insert(path.to_owned());
        }
        self.relist(cx);
        cx.notify();
    }

    /// The file list's rows for the filter and the layout.
    fn relist(&mut self, cx: &App) {
        let filter = self.filter.read(cx).text.clone();
        self.list_rows = self
            .view
            .update
            .patch
            .as_ref()
            .map_or_else(Vec::new, |diff| {
                list_rows(&diff.files, &filter, self.tree, &self.collapsed)
            });
    }

    /// The listed files, in the order the list shows them.
    fn listed_files(&self) -> Vec<usize> {
        self.list_rows
            .iter()
            .filter_map(|row| match row {
                ListRow::File { index, .. } => Some(*index),
                ListRow::Folder { .. } => None,
            })
            .collect()
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
        self.rebuild(cx);
        cx.notify();
    }

    fn rebuild(&mut self, cx: &App) {
        self.rows.clear();
        self.relist(cx);
        let Some(diff) = &self.view.update.patch else {
            return;
        };
        self.counts = diff
            .files
            .iter()
            .map(|file| {
                let lines = file.hunks.iter().flat_map(|hunk| &hunk.lines);
                lines.fold((0, 0), |(added, removed), line| match line.kind {
                    DiffLineKind::Added => (added + 1, removed),
                    DiffLineKind::Deleted => (added, removed + 1),
                    DiffLineKind::Context => (added, removed),
                })
            })
            .collect();
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
        self.rows = code_rows(&file.hunks, self.split());
        let focus = self.view.update.focus.as_ref().filter(|f| f.line.is_some());
        let target = focus.and_then(|focus| {
            self.rows.iter().position(|row| {
                let (h, l) = match *row {
                    CodeRow::Line(h, l) => (h, Some(l)),
                    CodeRow::Split {
                        hunk, left, right, ..
                    } => (hunk, if focus.old { left } else { right }),
                    CodeRow::Hunk(_) => return false,
                };
                l.is_some_and(|l| {
                    let line = &file.hunks[h].lines[l];
                    let number = if focus.old {
                        line.old_line
                    } else {
                        line.new_line
                    };
                    number == focus.line
                })
            })
        });
        let lines = file.hunks.iter().flat_map(|hunk| &hunk.lines);
        self.widest = lines
            .map(|line| line.text.chars().count())
            .max()
            .unwrap_or(0);
        self.code_width = (self.widest as f32 * CHAR + 140.0).max(400.0);
        self.split_x = px(0.0);
        self.wrapped.reset(self.rows.len());
        let target = target.unwrap_or(0);
        self.scroll.scroll_to_item(target, ScrollStrategy::Top);
        self.wrapped.scroll_to(ListOffset {
            item_ix: target,
            offset_in_item: px(0.0),
        });
    }

    /// Moves unwrapped side-by-side code sideways, as far as its longest line goes.
    fn scroll_split(&mut self, by: Pixels, cx: &mut Context<Self>) {
        let most = px((self.widest as f32 * CHAR - 200.0).max(0.0));
        self.split_x = (self.split_x - by).clamp(px(0.0), most);
        cx.notify();
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
            self.rebuild(cx);
            self.horizontal.set_offset(gpui::point(px(0.0), px(0.0)));
            let row = self
                .list_rows
                .iter()
                .position(|row| matches!(row, ListRow::File { index: file, .. } if *file == index));
            if let Some(row) = row {
                self.files_scroll
                    .scroll_to_item(row, ScrollStrategy::Center);
            }
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
            // Up and down go through the files the list shows, in its order.
            let listed = self.listed_files();
            let Some(last) = listed.len().checked_sub(1) else {
                return;
            };
            let at = listed.iter().position(|index| *index == self.selected);
            let next = match (key.key.as_str(), at) {
                ("down", Some(at)) => (at + 1).min(last),
                ("up", Some(at)) => at.saturating_sub(1),
                ("down" | "home", _) => 0,
                ("up" | "end", _) => last,
                _ => return,
            };
            self.select_file(listed[next], cx);
        } else if self.code_focus.is_focused(window) {
            if matches!(key.key.as_str(), "left" | "right") && self.split() && !self.wrap {
                self.scroll_split(px(if key.key == "left" { 40.0 } else { -40.0 }), cx);
                window.prevent_default();
                cx.stop_propagation();
                return;
            }
            if self.wrap {
                let height = self.wrapped.viewport_bounds().size.height;
                match key.key.as_str() {
                    "down" => self.wrapped.scroll_by(px(ROW)),
                    "up" => self.wrapped.scroll_by(px(-ROW)),
                    "pagedown" => self.wrapped.scroll_by(height),
                    "pageup" => self.wrapped.scroll_by(-height),
                    "home" => self.wrapped.scroll_to(ListOffset {
                        item_ix: 0,
                        offset_in_item: px(0.0),
                    }),
                    "end" => self
                        .wrapped
                        .scroll_to_reveal_item(self.rows.len().saturating_sub(1)),
                    _ => return,
                }
                cx.notify();
                cx.stop_propagation();
                window.prevent_default();
                return;
            }
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
            .min_h(px(ROW))
            .w_full()
            .flex()
            .text_size(px(13.0))
            .font_family(crate::terminal_view::DEFAULT_FONT_FAMILY)
            .when(!self.wrap, |row| row.h(px(ROW)).whitespace_nowrap());
        match self.rows[index] {
            CodeRow::Hunk(h) => {
                let hunk = &file.hunks[h];
                row.items_center()
                    .bg(rgba(theme::INFO << 8 | 0x16))
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
            CodeRow::Line(h, l) => {
                let line = &file.hunks[h].lines[l];
                let (mark, color, bg) = colors(line.kind);
                row.bg(rgba(bg))
                    .text_color(rgb(color))
                    .child(number(line.old_line))
                    .child(number(line.new_line))
                    .child(div().w(px(22.0)).flex_none().child(mark))
                    .child(self.code_text(line, px(0.0)))
                    .into_any_element()
            }
            CodeRow::Split { hunk, left, right } => {
                let lines = &file.hunks[hunk].lines;
                row.child(self.split_side(left.map(|l| &lines[l]), true))
                    .child(div().w(px(1.0)).flex_none().bg(rgba(ui::HAIRLINE)))
                    .child(self.split_side(right.map(|l| &lines[l]), false))
                    .into_any_element()
            }
        }
    }

    /// One column of a side-by-side row: the old file's line on the left, the new one's on the right.
    fn split_side(&self, line: Option<&DiffLine>, old: bool) -> Div {
        let side = div().flex_1().min_w_0().flex().overflow_hidden();
        let Some(line) = line else {
            return side;
        };
        let (mark, color, bg) = colors(line.kind);
        side.bg(rgba(bg))
            .text_color(rgb(color))
            .child(number(if old { line.old_line } else { line.new_line }))
            .child(div().w(px(22.0)).flex_none().child(mark))
            .child(self.code_text(line, self.split_x))
    }

    /// A line's text, moved left by `shift` when unwrapped side by side.
    fn code_text(&self, line: &DiffLine, shift: Pixels) -> Div {
        let text = format!(
            "{}{}",
            line.text,
            if line.missing_newline {
                "  ⏎ No newline at end of file"
            } else {
                ""
            }
        );
        if self.wrap {
            div().flex_1().min_w_0().pr(px(12.0)).child(text)
        } else {
            div().flex_none().ml(-shift).child(text)
        }
    }

    /// The desktop's row above the file list: the filter, and the flat list or folder tree switch.
    fn list_header(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .p(px(8.0))
            .child(
                div()
                    .h(px(32.0))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .pl(px(10.0))
                    .rounded(px(theme::RADIUS))
                    .border_1()
                    .border_color(rgba(0xffffff1a))
                    .bg(rgba(0xffffff08))
                    .text_size(px(12.0))
                    .child(icon(Icon::Search, 14.0, theme::MUTED_FOREGROUND))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(self.filter.clone()),
                    ),
            )
            .child(toggle_group([
                toggle("diff-flat", Icon::List, "Flat list", None, !self.tree)
                    .on_click(cx.listener(|this, _, _, cx| this.set_tree(false, cx))),
                toggle("diff-tree", Icon::ListTree, "Folder tree", None, self.tree)
                    .on_click(cx.listener(|this, _, _, cx| this.set_tree(true, cx))),
            ]))
    }

    /// A row of the file list: a folder in the tree, or a file with how it changed and its line counts. In the tree a
    /// file shows only its name; the flat list adds its folder.
    fn list_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(diff) = &self.view.update.patch else {
            return div().into_any_element();
        };
        let item = |id: ElementId| {
            div()
                .id(id)
                .h(px(28.0))
                .w_full()
                .pr(px(8.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .rounded(px(8.0))
                .text_size(px(12.0))
                .cursor_pointer()
        };
        let row = match &self.list_rows[index] {
            ListRow::Folder {
                path,
                label,
                depth,
                open,
            } => {
                let path = path.clone();
                item(SharedString::from(format!("dir:{path}")).into())
                    .pl(px(8.0 + *depth as f32 * 12.0))
                    .gap(px(6.0))
                    .role(Role::Button)
                    .aria_label(SharedString::from(path.clone()))
                    .aria_expanded(*open)
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .hover(|style| {
                        style
                            .bg(rgba(theme::HOVER))
                            .text_color(rgb(theme::FOREGROUND))
                    })
                    .child(icon(
                        if *open {
                            Icon::ChevronDown
                        } else {
                            Icon::ChevronRight
                        },
                        12.0,
                        theme::MUTED_FOREGROUND,
                    ))
                    .child(div().min_w_0().truncate().child(label.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_folder(&path, cx)))
            }
            ListRow::File { index, depth } => {
                let index = *index;
                let file = &diff.files[index];
                let path = file_path(file).to_owned();
                // The desktop's `changeMark`: a coloured letter for how the file changed.
                let (mark, color) = match file.status {
                    DiffStatus::Added => ("A", theme::DONE),
                    DiffStatus::Deleted => ("D", theme::BLOCKED),
                    DiffStatus::Modified => ("M", theme::WORKING),
                    DiffStatus::Renamed => ("R", theme::INFO),
                    DiffStatus::Copied => ("C", theme::INFO),
                };
                let (added, removed) = self.counts.get(index).copied().unwrap_or_default();
                let (folder, name) = match path.rsplit_once('/') {
                    Some((folder, name)) => (Some(folder.to_owned()), name.to_owned()),
                    None => (None, path.clone()),
                };
                let selected = index == self.selected;
                item(index.into())
                    .pl(px(match depth {
                        Some(depth) => 8.0 + *depth as f32 * 12.0 + 18.0,
                        None => 8.0,
                    }))
                    .bg(rgba(if selected { 0xffffff14 } else { 0 }))
                    .role(Role::ListBoxOption)
                    .aria_label(SharedString::from(path))
                    .aria_selected(selected)
                    .when(!selected, |row| {
                        row.hover(|style| style.bg(rgba(theme::HOVER)))
                    })
                    .child(
                        div()
                            .w(px(12.0))
                            .flex_none()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(color))
                            .child(mark),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .gap(px(6.0))
                            .overflow_hidden()
                            .child(div().flex_none().child(name))
                            .when_some(folder.filter(|_| depth.is_none()), |text, folder| {
                                text.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(rgb(theme::MUTED_FOREGROUND))
                                        .child(folder),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::DONE))
                            .child(format!("+{added}")),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::BLOCKED))
                            .child(format!("−{removed}")),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.files_focus.focus(window, cx);
                        this.select_file(index, cx);
                    }))
            }
        };
        // The desktop's list has 8px sides.
        div().w_full().px(px(8.0)).child(row).into_any_element()
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
        } else if self.wrap {
            list(
                self.wrapped.clone(),
                cx.processor(|this, index: usize, _, _| this.code_row(index)),
            )
            .flex_1()
            .min_h_0()
            .into_any_element()
        } else {
            let rows = uniform_list(
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
            .h_full();
            if self.split() {
                // Each column clips its own lines, and Shift and the wheel move both sideways together.
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
                    .child(rows.w_full())
                    .into_any_element()
            } else {
                div()
                    .id("diff-code-scroll")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_x_scroll()
                    .track_scroll(&self.horizontal)
                    .child(rows.w(px(self.code_width)).min_w(gpui::relative(1.0)))
                    .into_any_element()
            }
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
                        .border_color(rgba(theme::BORDER))
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
                                .focus_visible(|style| style.border_color(rgb(theme::INFO)))
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
                    .focus_visible(|style| style.border_1().border_color(rgb(theme::INFO)))
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

/// A line's mark, text colour and background.
fn colors(kind: DiffLineKind) -> (&'static str, u32, u32) {
    match kind {
        DiffLineKind::Added => ("+", theme::DONE, 0x7dd3a712),
        DiffLineKind::Deleted => ("-", theme::DANGER, 0xf2786a12),
        DiffLineKind::Context => (" ", theme::FOREGROUND, 0),
    }
}

/// A line number in the gutter.
fn number(line: Option<usize>) -> Div {
    div()
        .w(px(48.0))
        .flex_none()
        .text_right()
        .pr(px(8.0))
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .child(line.map(|n| n.to_string()).unwrap_or_default())
}

/// The desktop's `Toggle` and `ToggleGroupItem` at size `sm`: an icon, with a label when there's room, on a fill while
/// pressed.
fn toggle(
    id: impl Into<ElementId>,
    which: Icon,
    label: &'static str,
    text: Option<&'static str>,
    pressed: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(SharedString::from(label))
        .aria_toggled(if pressed {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .h(px(28.0))
        .min_w(px(28.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .px(px(if text.is_some() { 8.0 } else { 6.0 }))
        .rounded(px(6.0))
        .text_size(px(12.0))
        .cursor_pointer()
        .bg(rgba(if pressed { 0xffffff17 } else { 0 }))
        .when(!pressed, |item| {
            item.hover(|style| style.bg(rgba(theme::HOVER)))
        })
        .child(icon(which, 16.0, theme::FOREGROUND))
        .children(text)
}

/// The desktop's `ToggleGroup`: its items on a faint fill.
fn toggle_group(items: impl IntoIterator<Item = Stateful<Div>>) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .p(px(2.0))
        .rounded(px(8.0))
        .bg(rgba(0xffffff0d))
        .children(items)
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
        let (docked, split) = (self.docked, self.split());
        // Docked, the toggles show only their icons, as the desktop's.
        let text = |label| (!docked).then_some(label);
        div()
            .id("native-diff")
            .size_full()
            .flex()
            .flex_col()
            // Docked, the dock's card is the background.
            .when(!docked, |viewer| viewer.bg(rgb(theme::BACKGROUND)))
            .text_color(rgb(theme::FOREGROUND))
            .text_size(px(13.0))
            // Only filling the window is it a dialog; in the dock it's a part of the window beside the tab.
            .role(if docked { Role::Region } else { Role::Dialog })
            .aria_label(self.view.title.clone())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .children(ui::spot(|| "diff".into()))
            // The desktop's header: `py-3 pr-3 pl-5` and a 15px title when the diff fills the window, `py-2 pr-2 pl-4`
            // and 13px docked. Under the title, the subtitle, the number of files and the lines added and removed.
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .py(px(if docked { 8.0 } else { 12.0 }))
                    .pr(px(if docked { 8.0 } else { 12.0 }))
                    .pl(px(if docked { 16.0 } else { 20.0 }))
                    .border_b_1()
                    .border_color(rgba(ui::HAIRLINE))
                    .child(
                        div()
                            .mr(px(12.0))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(if docked { 13.0 } else { 15.0 }))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(self.view.title.clone()),
                            )
                            .child(
                                div()
                                    .mt(px(2.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .min_w_0()
                                    .whitespace_nowrap()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .when(!self.view.subtitle.is_empty(), |line| {
                                        line.child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .child(self.view.subtitle.clone()),
                                        )
                                    })
                                    .when_some(self.view.update.patch.as_ref(), |line, patch| {
                                        let (added, removed) = self
                                            .counts
                                            .iter()
                                            .fold((0, 0), |(a, r), (fa, fr)| (a + fa, r + fr));
                                        line.child(format!("{} files", patch.files.len()))
                                            .child(
                                                div()
                                                    .text_color(rgb(theme::DONE))
                                                    .child(format!("+{added}")),
                                            )
                                            .child(
                                                div()
                                                    .text_color(rgb(theme::BLOCKED))
                                                    .child(format!("−{removed}")),
                                            )
                                    }),
                            ),
                    )
                    .when(self.view.refresh, |bar| {
                        bar.child(
                            ui::icon_button(
                                "diff-refresh",
                                Icon::RefreshCw,
                                if self.refreshing {
                                    "Refreshing…"
                                } else {
                                    "Refresh"
                                },
                                ui::ButtonSize::Sm,
                            )
                            .track_focus(&self.refresh_focus)
                            .when(self.refreshing, |button| button.opacity(0.5))
                            .when(!self.refreshing, |button| {
                                button.on_click(
                                    cx.listener(|this, _, _, cx| this.request_refresh(cx)),
                                )
                            }),
                        )
                    })
                    .child(toggle_group([
                        toggle("diff-split", Icon::Columns, "Split", text("Split"), split)
                            .on_click(cx.listener(|this, _, _, cx| this.set_split(true, cx))),
                        toggle(
                            "diff-unified",
                            Icon::Rows3,
                            "Unified",
                            text("Unified"),
                            !split,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.set_split(false, cx))),
                    ]))
                    .child(
                        toggle(
                            "diff-wrap",
                            Icon::TextWrap,
                            "Wrap long lines",
                            text("Wrap"),
                            self.wrap,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.set_wrap(!this.wrap, cx))),
                    )
                    .when(docked, |bar| {
                        bar.child(
                            toggle(
                                "diff-files-toggle",
                                Icon::PanelLeft,
                                "File list",
                                None,
                                self.dock_files,
                            )
                            .children(ui::spot(|| "diff-files-toggle".into()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choose_dock_files(!this.dock_files, cx)
                            })),
                        )
                    })
                    .child(
                        div()
                            .mx(px(4.0))
                            .h(px(16.0))
                            .w(px(1.0))
                            .bg(rgba(ui::HAIRLINE)),
                    )
                    .when(docked, |bar| {
                        bar.child(
                            ui::untipped_ghost_button(
                                "diff-expand",
                                "Expand",
                                ui::ButtonSize::Sm,
                                None,
                                icon(Icon::Maximize, 16.0, theme::MUTED_FOREGROUND),
                            )
                            .tooltip(ui::tooltip("Fill the window", None))
                            .children(ui::spot(|| "diff-expand".into()))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Expand(true)))),
                        )
                    })
                    .when(!docked && self.can_dock, |bar| {
                        bar.child(
                            ui::icon_button(
                                "diff-dock",
                                Icon::Minimize,
                                "Dock beside the tab",
                                ui::ButtonSize::Sm,
                            )
                            .children(ui::spot(|| "diff-dock".into()))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Expand(false)))),
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
        window.simulate_click(point(px(80.0), px(130.0)), MouseButton::Left);
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

#[cfg(all(test, feature = "test-support"))]
mod layout_tests {
    use super::*;
    use crate::{diff, page::plugin::PluginConfig};
    use gpui::{TestApp, TestAppWindow};
    use std::{cell::RefCell, rc::Rc};

    /// Each `Event::Layout` the viewer sent: wrap, then tree.
    type Layouts = Rc<RefCell<Vec<(bool, bool)>>>;

    fn open(patch: &str) -> (TestApp, TestAppWindow<Viewer>, Layouts) {
        let view = View {
            id: "test:2".into(),
            plugin_id: "test".into(),
            title: "Changes".into(),
            subtitle: String::new(),
            cwd: String::new(),
            refresh: false,
            update: diff::clean_update(&serde_json::json!({ "patch": patch })).unwrap(),
        };
        let host = Arc::new(PluginHost::new(PluginConfig {
            id: "test".into(),
            root: std::env::temp_dir(),
            scope: "test".into(),
        }));
        let mut app = TestApp::new();
        let window = app.open_window(move |_, cx| Viewer::new(view, host, cx));
        let layouts = Rc::new(RefCell::new(Vec::new()));
        let seen = layouts.clone();
        let viewer = window.root();
        let subscription = app.update(|cx| {
            cx.subscribe(&viewer, move |_, event: &Event, _| {
                if let Event::Layout { wrap, tree } = event {
                    seen.borrow_mut().push((*wrap, *tree));
                }
            })
        });
        std::mem::forget(subscription);
        (app, window, layouts)
    }

    #[test]
    fn the_filter_tree_split_and_wrap_choices_change_what_shows() {
        let patch = "diff --git a/src/app.rs b/src/app.rs\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,2 +1,2 @@\n same\n-old\n+new\ndiff --git a/src/lib/api.rs b/src/lib/api.rs\n--- a/src/lib/api.rs\n+++ b/src/lib/api.rs\n@@ -1 +1 @@\n-a\n+b\ndiff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-x\n+y\n";
        let (_app, mut window, layouts) = open(patch);
        window.draw();
        // Side by side first, as the desktop's modal: the removed and added line share a row.
        window.read(|v, _| {
            assert_eq!(
                v.rows,
                [
                    CodeRow::Hunk(0),
                    CodeRow::Split {
                        hunk: 0,
                        left: Some(0),
                        right: Some(0)
                    },
                    CodeRow::Split {
                        hunk: 0,
                        left: Some(1),
                        right: Some(2)
                    },
                ]
            )
        });

        // Typing in the filter narrows the list, and the arrows go through what it shows.
        window.update(|v, window, cx| v.filter.focus_handle(cx).focus(window, cx));
        window.simulate_input("API");
        window.read(|v, _| assert_eq!(v.listed_files(), [1]));
        window.update(|v, window, cx| {
            v.files_focus.focus(window, cx);
            v.select_file(0, cx);
        });
        window.simulate_keystroke("down");
        assert_eq!(window.read(|v, _| v.selected_file()), 1);

        window.update(|v, _, cx| {
            v.filter.update(cx, |field, cx| {
                field.text.clear();
                cx.emit(FieldEvent::Changed);
            });
            v.set_tree(true, cx);
        });
        window.read(|v, _| {
            assert_eq!(
                v.list_rows.first(),
                Some(&ListRow::Folder {
                    path: "src".into(),
                    label: "src".into(),
                    depth: 0,
                    open: true
                })
            );
            assert_eq!(v.listed_files(), [1, 0, 2], "folders first, then files");
        });

        window.update(|v, _, cx| {
            v.set_split(false, cx);
            v.select_file(0, cx);
        });
        window.read(|v, _| {
            assert_eq!(
                v.rows[1..],
                [
                    CodeRow::Line(0, 0),
                    CodeRow::Line(0, 1),
                    CodeRow::Line(0, 2)
                ]
            )
        });

        window.update(|v, _, cx| v.set_wrap(true, cx));
        window.draw();
        window.read(|v, _| assert_eq!(v.wrapped.item_count(), v.rows.len()));
        assert_eq!(*layouts.borrow(), [(false, true), (true, true)]);
    }

    /// The desktop's docked diff: one column until you pick, and a file list its toggle hides, which the app keeps, and
    /// Expand. Filling the window it's side by side again, with Dock beside the tab when it has a tab to go back to.
    #[test]
    fn docked_it_starts_in_one_column_with_a_file_list_toggle_and_offers_to_fill_the_window() {
        let patch = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n";
        let (mut app, mut window, _) = open(patch);
        let events = Rc::new(RefCell::new(Vec::new()));
        let seen = events.clone();
        let viewer = window.root();
        let _subscription = app.update(|cx| {
            cx.subscribe(&viewer, move |_, event: &Event, _| match event {
                Event::Expand(full) => seen.borrow_mut().push(format!("expand {full}")),
                Event::DockFiles(shown) => seen.borrow_mut().push(format!("files {shown}")),
                _ => {}
            })
        });
        let unified = [CodeRow::Line(0, 0), CodeRow::Line(0, 1)];
        let spot = |name: &str| crate::ui::spot_at(name).unwrap_or_else(|| panic!("no {name}"));
        let click = |window: &mut TestAppWindow<Viewer>, name: &str| {
            window.simulate_click(spot(name).center(), gpui::MouseButton::Left);
            window.draw();
        };

        window.update(|v, _, cx| v.set_dock(true, true, cx));
        window.draw();
        window.read(|v, _| assert_eq!(v.rows[1..], unified));
        // The code starts after the dock's 224 file list until the toggle hides the list.
        assert_eq!(spot("diff-code").left(), px(224.0));
        click(&mut window, "diff-files-toggle");
        assert_eq!(spot("diff-code").left(), px(0.0));
        click(&mut window, "diff-expand");
        assert_eq!(*events.borrow(), ["files false", "expand true"]);

        // Filling the window it shows the list and goes side by side, and offers to dock again.
        window.update(|v, _, cx| v.set_dock(false, true, cx));
        window.draw();
        window.read(|v, _| assert!(matches!(v.rows[1], CodeRow::Split { .. })));
        assert_eq!(spot("diff-code").left(), px(288.0));
        click(&mut window, "diff-dock");
        assert_eq!(events.borrow().last().unwrap(), "expand false");

        // A layout you chose stays wherever it shows.
        window.update(|v, _, cx| {
            v.set_split(false, cx);
            v.set_dock(true, true, cx);
            v.set_dock(false, true, cx);
        });
        window.read(|v, _| assert_eq!(v.rows[1..], unified));
    }
}
