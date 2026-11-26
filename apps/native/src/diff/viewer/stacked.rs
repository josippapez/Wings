//! Every file's diff stacked in one scroll, as a sidebar's widened diff shows it: a header per file that stays at the
//! top while its file scrolls, the unchanged lines around the hunks folded into rows that open, and a picker for what
//! the diff is of. Only the rows on screen become elements: they go in a `list`, each as tall as its kind, and opening a
//! fold splices its lines in without measuring the other rows again.

use super::{Event, ROW, Viewer, colors, no_lines, number, toggle};
use crate::{
    diff::{
        Focus, Scope,
        highlight::{self, Span},
        patch::{Diff, DiffFile, DiffLineKind},
        rows::file_path,
        stack::{self, Fold, RowKey, StackRow},
        worktree,
    },
    icons::{Icon, icon},
    theme, ui,
};
use gpui::{
    AnyElement, Bounds, ClickEvent, ClipboardItem, Context, ElementId, FontWeight, ListAlignment,
    ListOffset, ListState, MouseDownEvent, Pixels, Point, Role, SharedString, Task, canvas, div,
    list, point, prelude::*, px, rgb, rgba,
};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

/// A file header's height, a closed fold's, and the gap between two files.
const HEADER: f32 = 36.0;
const FOLD: f32 = 28.0;
const GAP: f32 = 12.0;

/// What is known of a file's text, by its path.
#[derive(Clone)]
enum Text {
    Reading,
    Read(Arc<Vec<String>>),
    /// Past the limits, outside the repository, or changed since the patch: its folds stay closed.
    Unread,
}

pub(super) struct Stack {
    /// Asked for by the workspace.
    on: bool,
    /// The patch the rows are of.
    diff: Option<Arc<Diff>>,
    rows: Vec<StackRow>,
    folds: Vec<Vec<Fold>>,
    /// Where each file's header is in `rows`.
    headers: Vec<usize>,
    list: ListState,
    texts: HashMap<String, Text>,
    /// The syntax colours of each read file's lines, by path; None while they're worked out.
    text_colors: HashMap<String, Option<Arc<Vec<Vec<Span>>>>>,
    reading: Option<Task<()>>,
    /// The folds opened, by file path and the fold's first old line, which stays put while the working tree changes.
    open: HashMap<String, HashSet<usize>>,
    /// The plugin's focus waits for a patch to scroll to.
    pub(super) focus_pending: bool,
    /// The open file menu: its file, and where it was asked for.
    menu: Option<(usize, Point<Pixels>)>,
    scopes_open: bool,
    /// Where the scope picker was drawn, for its menu to open under it.
    scope_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Stack {
    pub(super) fn new(focus_pending: bool) -> Self {
        Self {
            on: false,
            diff: None,
            rows: Vec::new(),
            folds: Vec::new(),
            headers: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(200.0)),
            texts: HashMap::new(),
            text_colors: HashMap::new(),
            reading: None,
            open: HashMap::new(),
            focus_pending,
            menu: None,
            scopes_open: false,
            scope_bounds: Rc::default(),
        }
    }
}

impl Viewer {
    /// Every file stacked in one scroll, as a sidebar's diff shows them, or one file at a time.
    pub fn set_stacked(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.stack.on != on {
            self.stack.on = on;
            self.rebuild(cx);
            cx.notify();
        }
    }

    /// The editor a file header opens its file in, by its name.
    pub fn set_editor(&mut self, name: Option<SharedString>, cx: &mut Context<Self>) {
        self.editor = name;
        cx.notify();
    }

    /// Stacked, unless the diff has review threads, which sit under their lines in the one-file view.
    pub(super) fn stacked(&self) -> bool {
        self.stack.on && self.comments().is_empty() && !self.allows_comments()
    }

    /// Lays every file out again. The row at the top stays there when it is still in the patch, else its file's header
    /// does; a plugin's focus not yet shown scrolls to its file instead.
    pub(super) fn restack(&mut self, _cx: &mut Context<Self>) {
        let anchor = self.stack_anchor();
        let diff = self.view.update.patch.clone().filter(|_| self.stacked());
        self.stack.diff = diff.clone();
        let Some(diff) = diff else {
            self.stack.rows.clear();
            self.stack.folds.clear();
            self.stack.headers.clear();
            self.stack.list.reset(0);
            return;
        };
        let texts = &self.stack.texts;
        self.stack.folds = diff
            .files
            .iter()
            .map(|file| {
                let lines = match texts.get(file_path(file)) {
                    Some(Text::Read(lines)) => Some(lines.len()),
                    _ => None,
                };
                stack::folds(file, lines)
            })
            .collect();
        let open = self.open_folds(&diff);
        self.stack.rows = stack::rows(&diff.files, &self.stack.folds, &open);
        self.stack.headers = stack::headers(&self.stack.rows, diff.files.len());
        self.widest = self.stack_widest(&diff);
        // Most rows are lines, so the scrollbar is about right before the rest are measured.
        self.stack
            .list
            .reset_with_uniform_height(self.stack.rows.len(), px(ROW));
        let target = if self.stack.focus_pending {
            self.stack.focus_pending = false;
            let focus = self.view.update.focus.clone();
            focus
                .and_then(|focus| self.focus_row(&diff, &focus))
                .map(|at| (at, px(0.0)))
        } else {
            anchor.and_then(|(key, offset)| {
                stack::find(&key, &self.stack.rows, &diff.files, &self.stack.folds)
                    .map(|(at, exact)| (at, if exact { offset } else { px(0.0) }))
            })
        };
        if let Some((item_ix, offset_in_item)) = target {
            self.stack.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item,
            });
        }
    }

    /// The row at the top, by what it shows, and how far into it the stack is scrolled.
    fn stack_anchor(&self) -> Option<(RowKey, Pixels)> {
        let diff = self.stack.diff.as_ref()?;
        let top = self.stack.list.logical_scroll_top();
        let row = *self.stack.rows.get(top.item_ix)?;
        Some((
            stack::key(row, &diff.files, &self.stack.folds),
            top.offset_in_item,
        ))
    }

    /// The folds opened, by file and their place in its folds, of the files whose text is read.
    fn open_folds(&self, diff: &Diff) -> HashSet<(usize, usize)> {
        let mut open = HashSet::new();
        for (f, file) in diff.files.iter().enumerate() {
            let path = file_path(file);
            let (Some(Text::Read(_)), Some(starts)) =
                (self.stack.texts.get(path), self.stack.open.get(path))
            else {
                continue;
            };
            for (k, fold) in self.stack.folds[f].iter().enumerate() {
                if starts.contains(&fold.old_start) {
                    open.insert((f, k));
                }
            }
        }
        open
    }

    /// The longest line the stack shows, in characters, for how far it scrolls sideways.
    fn stack_widest(&self, diff: &Diff) -> usize {
        let hunks = diff
            .files
            .iter()
            .flat_map(|file| &file.hunks)
            .flat_map(|hunk| &hunk.lines)
            .map(|line| line.text.chars().count());
        let open = self.stack.rows.iter().filter_map(|row| match *row {
            StackRow::Unchanged { file, fold, line } => self
                .unchanged_text(file, fold, line)
                .map(|text| text.chars().count()),
            _ => None,
        });
        hunks.chain(open).max().unwrap_or(0)
    }

    fn focus_row(&self, diff: &Diff, focus: &Focus) -> Option<usize> {
        let path = Some(focus.path.as_str());
        let file = diff
            .files
            .iter()
            .position(|file| file.new_path.as_deref() == path || file.old_path.as_deref() == path)?;
        let header = *self.stack.headers.get(file)?;
        let line = focus.line.and_then(|line| {
            stack::line_row(
                &self.stack.rows,
                &diff.files,
                &self.stack.folds,
                file,
                line,
                focus.old,
            )
        });
        Some(line.unwrap_or(header))
    }

    /// Scrolls to the file's header.
    pub(super) fn scroll_to_file(&mut self, file: usize) {
        if let Some(&at) = self.stack.headers.get(file) {
            self.stack.list.scroll_to(ListOffset {
                item_ix: at,
                offset_in_item: px(0.0),
            });
        }
    }

    /// A new patch: the files that didn't change keep their text, its colours and their open folds.
    pub(super) fn keep_texts(&mut self, old: Option<&Diff>, new: &Diff) {
        fn by_path(diff: &Diff) -> HashMap<&str, &DiffFile> {
            diff.files.iter().map(|file| (file_path(file), file)).collect()
        }
        let (before, after) = (old.map(by_path).unwrap_or_default(), by_path(new));
        let same = |path: &str| before.get(path).is_some_and(|was| after.get(path) == Some(was));
        self.stack
            .texts
            .retain(|path, text| !matches!(text, Text::Reading) && same(path));
        self.stack
            .text_colors
            .retain(|path, colors| colors.is_some() && same(path));
        self.stack
            .open
            .retain(|path, _| after.contains_key(path.as_str()));
        // A read under way is of the old patch.
        self.stack.reading = None;
    }

    /// Reads the text of the files whose folds can open and that aren't read yet, off the window's thread, when the
    /// plugin says the patch's new side is its folder's working tree.
    pub(super) fn read_texts(&mut self, cx: &mut Context<Self>) {
        let Some(diff) = self.stack.diff.clone() else {
            return;
        };
        let reading = self
            .stack
            .texts
            .values()
            .any(|text| matches!(text, Text::Reading));
        if self.view.update.worktree != Some(true) || self.view.cwd.is_empty() || reading {
            return;
        }
        let wanted: Vec<usize> = diff
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| {
                stack::foldable(file) && !self.stack.texts.contains_key(file_path(file))
            })
            .map(|(at, _)| at)
            .collect();
        if wanted.is_empty() {
            return;
        }
        for &at in &wanted {
            self.stack
                .texts
                .insert(file_path(&diff.files[at]).to_owned(), Text::Reading);
        }
        let root = PathBuf::from(&self.view.cwd);
        let work = cx.background_executor().spawn({
            let diff = diff.clone();
            async move { worktree::read_all(&root, &diff, &wanted) }
        });
        self.stack.reading = Some(cx.spawn(async move |this, cx| {
            let read = work.await;
            let _ = this.update(cx, |this, cx| {
                if !this
                    .stack
                    .diff
                    .as_ref()
                    .is_some_and(|now| Arc::ptr_eq(now, &diff))
                {
                    return;
                }
                for (path, lines) in read {
                    let text = lines.map_or(Text::Unread, |lines| Text::Read(Arc::new(lines)));
                    this.stack.texts.insert(path, text);
                }
                // The folds after the last hunks show now, and those opened while it was read open.
                this.restack(cx);
                cx.notify();
            });
        }));
    }

    /// A line of an open fold, from the file's text.
    fn unchanged_text(&self, file: usize, fold: usize, line: usize) -> Option<&str> {
        let diff = self.stack.diff.as_ref()?;
        let fold = self.stack.folds.get(file)?.get(fold)?;
        let Some(Text::Read(lines)) = self.stack.texts.get(file_path(diff.files.get(file)?))
        else {
            return None;
        };
        lines.get(fold.new_start + line - 1).map(String::as_str)
    }

    /// Opens a fold in place: its lines go where its row was, and nothing else is measured again. One whose file is
    /// still being read opens when the text comes.
    fn open_fold(&mut self, file: usize, fold: usize, cx: &mut Context<Self>) {
        let found = self.stack.folds.get(file).and_then(|f| f.get(fold)).copied();
        let (Some(diff), Some(found)) = (self.stack.diff.clone(), found) else {
            return;
        };
        let path = file_path(&diff.files[file]).to_owned();
        self.stack
            .open
            .entry(path.clone())
            .or_default()
            .insert(found.old_start);
        if matches!(self.stack.texts.get(&path), Some(Text::Read(_)))
            && let Some(at) = self
                .stack
                .rows
                .iter()
                .position(|row| *row == StackRow::Fold { file, fold })
        {
            self.stack
                .rows
                .splice(at..at + 1, stack::fold_lines(file, fold, found.len));
            self.stack.list.splice(at..at + 1, found.len);
            self.stack.headers = stack::headers(&self.stack.rows, diff.files.len());
            let widest = (0..found.len)
                .filter_map(|line| self.unchanged_text(file, fold, line))
                .map(|text| text.chars().count())
                .max()
                .unwrap_or(0);
            self.widest = self.widest.max(widest);
        }
        cx.notify();
    }

    /// Closes the file menu and the scope picker's, and says whether one was open.
    pub(super) fn close_menus(&mut self) -> bool {
        let open = self.stack.menu.is_some() || self.stack.scopes_open;
        self.stack.menu = None;
        self.stack.scopes_open = false;
        open
    }

    /// Shows another scope: the diff starts loading again, and the plugin is asked for that scope's patch.
    fn pick_scope(&mut self, id: String, cx: &mut Context<Self>) {
        self.stack.scopes_open = false;
        if self.view.update.scope.as_deref() == Some(id.as_str()) {
            cx.notify();
            return;
        }
        self.view.update.scope = Some(id.clone());
        self.view.update.patch = None;
        self.view.update.error = None;
        self.view.update.focus = None;
        self.stack.focus_pending = false;
        self.stack.texts.clear();
        self.stack.text_colors.clear();
        self.stack.open.clear();
        self.stack.reading = None;
        self.colors.clear();
        self.coloring = None;
        self.rebuild(cx);
        self.ask_plugin(Some(id), cx);
        cx.notify();
    }

    /// The arrows, the page keys, Home and End over the stack. False for a key it doesn't take.
    pub(super) fn stack_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if matches!(key, "left" | "right") {
            if self.wrap {
                return false;
            }
            self.scroll_split(px(if key == "left" { 40.0 } else { -40.0 }), cx);
            return true;
        }
        let list = self.stack.list.clone();
        let page = list.viewport_bounds().size.height;
        match key {
            "down" => list.scroll_by(px(ROW)),
            "up" => list.scroll_by(px(-ROW)),
            "pagedown" => list.scroll_by(page),
            "pageup" => list.scroll_by(-page),
            "home" => list.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.0),
            }),
            "end" => list.scroll_to_reveal_item(self.stack.rows.len().saturating_sub(1)),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Works out a file's syntax colours off the window's thread, one file at a time, as its lines come on screen.
    fn color_stacked(&mut self, file: usize, cx: &mut Context<Self>) {
        let Some(diff) = self.stack.diff.clone() else {
            return;
        };
        let busy = self.coloring.as_ref().is_some_and(|(at, of, _)| {
            Arc::ptr_eq(of, &diff) && !self.colors.contains_key(at)
        });
        if busy || self.colors.contains_key(&file) || file >= diff.files.len() {
            return;
        }
        let work = cx.background_executor().spawn({
            let diff = diff.clone();
            async move { highlight::file_colors(&diff.files[file]) }
        });
        let task = cx.spawn({
            let diff = diff.clone();
            async move |this, cx| {
                let colors = work.await;
                let _ = this.update(cx, |this, cx| {
                    if this
                        .stack
                        .diff
                        .as_ref()
                        .is_some_and(|now| Arc::ptr_eq(now, &diff))
                    {
                        this.colors.insert(file, colors);
                        cx.notify();
                    }
                });
            }
        });
        self.coloring = Some((file, diff, task));
    }

    /// Works out the colours of a read file's lines, for its open folds, off the window's thread.
    fn color_text(&mut self, file: usize, cx: &mut Context<Self>) {
        let path = match self.stack.diff.as_ref().and_then(|diff| diff.files.get(file)) {
            Some(file) => file_path(file),
            None => return,
        };
        if self.stack.text_colors.contains_key(path) {
            return;
        }
        let path = path.to_owned();
        let Some(Text::Read(lines)) = self.stack.texts.get(&path).cloned() else {
            return;
        };
        self.stack.text_colors.insert(path.clone(), None);
        let work = cx.background_executor().spawn({
            let (path, lines) = (path.clone(), lines.clone());
            async move { highlight::text_colors(&path, &lines) }
        });
        cx.spawn(async move |this, cx| {
            let colors = work.await;
            let _ = this.update(cx, |this, cx| {
                if matches!(this.stack.texts.get(&path), Some(Text::Read(now)) if Arc::ptr_eq(now, &lines))
                {
                    this.stack.text_colors.insert(path, Some(Arc::new(colors)));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The stack in place of the file list and the open file, with the header of the file at the top over it.
    pub(super) fn stack_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = list(
            self.stack.list.clone(),
            cx.processor(|this, index: usize, _, cx| {
                #[cfg(feature = "test-support")]
                this.rendered_rows.set(this.rendered_rows.get() + 1);
                this.stack_row(index, cx)
            }),
        )
        .size_full();
        let code = if self.wrap {
            rows.into_any_element()
        } else {
            self.sideways(rows, cx)
        };
        // A comment box scrolled under the header over the rows, being typed in, draws its text under it, in place.
        let over = Rc::new(Cell::new(Bounds::default()));
        let sticky = self.sticky_header(over.clone(), cx);
        let code = ui::under(over, code);
        div()
            .id("diff-code-panel")
            .relative()
            .flex_1()
            .min_h_0()
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
            .child(code)
            .children(sticky)
            .into_any_element()
    }

    fn stack_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.stack.rows.get(index).copied() else {
            return div().into_any_element();
        };
        match row {
            StackRow::Header(file) => self.file_header(file, false, cx),
            StackRow::Gap(_) => div().h(px(GAP)).w_full().into_any_element(),
            StackRow::Note(file) => self.stack_note(file),
            StackRow::Fold { file, fold } => self.fold_row(file, fold, cx),
            StackRow::Unchanged { file, fold, line } => {
                self.color_text(file, cx);
                self.unchanged_row(file, fold, line)
            }
            StackRow::Line { file, hunk, line } => {
                self.color_stacked(file, cx);
                self.hunk_row(file, hunk, line)
            }
        }
    }

    /// The header of the file whose rows are at the top, over them, until the next file's header pushes it up.
    fn sticky_header(&self, over: Rc<Cell<Bounds<Pixels>>>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let top = self.stack.list.logical_scroll_top();
        let row = *self.stack.rows.get(top.item_ix)?;
        // The file's own header, whole at the top, needs no copy over it.
        if matches!(row, StackRow::Header(_)) && top.offset_in_item <= px(0.0) {
            return None;
        }
        let file = row.file();
        let viewport = self.stack.list.viewport_bounds();
        let shift = self
            .stack
            .headers
            .get(file + 1)
            .and_then(|&next| self.stack.list.bounds_for_item(next))
            .map_or(px(0.0), |next| {
                (next.top() - viewport.top() - px(HEADER)).min(px(0.0))
            });
        Some(
            div()
                .id("diff-sticky")
                .absolute()
                .top(shift)
                .left_0()
                .right_0()
                .occlude()
                .child(ui::bounds_into(over))
                .children(ui::spot(move || format!("diff-sticky-{file}")))
                .child(self.file_header(file, true, cx))
                .into_any_element(),
        )
    }

    /// A file's header: its folder dimmed and its name, how many lines it adds and removes, and its actions.
    fn file_header(&self, f: usize, sticky: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(file) = self.stack.diff.as_ref().and_then(|diff| diff.files.get(f)) else {
            return div().h(px(HEADER)).into_any_element();
        };
        let path = file_path(file);
        let (folder, name) = match path.rsplit_once('/') {
            Some((folder, name)) => (Some(format!("{folder}/")), name.to_owned()),
            None => (None, path.to_owned()),
        };
        let renamed = file
            .old_path
            .as_ref()
            .filter(|old| file.new_path.as_ref().is_some_and(|new| new != *old))
            .map(|old| format!("from {old}"));
        let (added, removed) = self.counts.get(f).copied().unwrap_or_default();
        let id = |what: &str| {
            let sticky = if sticky { "-sticky" } else { "" };
            ElementId::NamedInteger(format!("diff-file-{what}{sticky}").into(), f as u64)
        };
        let editor = self
            .editor
            .clone()
            .filter(|_| !self.view.cwd.is_empty() && file.new_path.is_some());
        let line = stack::first_change(file).and_then(|line| u32::try_from(line).ok());
        let path = path.to_owned();
        let count = |text: String, color: u32| {
            div()
                .flex_none()
                .text_size(px(12.0))
                .text_color(rgb(color))
                .child(text)
        };
        div()
            .h(px(HEADER))
            .w_full()
            .flex_none()
            .bg(rgb(theme::surface()))
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .pl(px(12.0))
                    .pr(px(6.0))
                    .bg(rgba(0xffffff0a))
                    .border_t_1()
                    .border_b_1()
                    .border_color(rgba(ui::HAIRLINE))
                    .text_size(px(13.0))
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .when_some(folder, |text, folder| {
                                text.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(rgb(theme::muted_foreground()))
                                        .child(folder),
                                )
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name),
                            )
                            .when_some(renamed, |text, renamed| {
                                text.child(
                                    div()
                                        .ml(px(8.0))
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.0))
                                        .text_color(rgb(theme::muted_foreground()))
                                        .child(renamed),
                                )
                            }),
                    )
                    .child(count(format!("+{added}"), theme::done()))
                    .child(count(format!("−{removed}"), theme::blocked()))
                    .when_some(editor, |row, editor| {
                        let path = path.clone();
                        row.child(
                            ui::icon_button(
                                id("open"),
                                Icon::Code,
                                format!("Open in {editor}"),
                                ui::ButtonSize::Xs,
                            )
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(Event::OpenInEditor {
                                    path: path.clone(),
                                    line,
                                })
                            })),
                        )
                    })
                    .child(
                        ui::icon_button(id("more"), Icon::Ellipsis, "More", ui::ButtonSize::Xs)
                            .children((!sticky).then(|| ui::spot(move || format!("diff-file-more-{f}"))).flatten())
                            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                this.stack.menu = Some((f, event.position()));
                                this.stack.scopes_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// Why a file has no lines to show.
    fn stack_note(&self, file: usize) -> AnyElement {
        let text = self
            .stack
            .diff
            .as_ref()
            .and_then(|diff| diff.files.get(file))
            .map_or("", no_lines);
        div()
            .h(px(HEADER))
            .w_full()
            .flex()
            .items_center()
            .pl(px(16.0))
            .text_size(px(12.0))
            .text_color(rgb(theme::muted_foreground()))
            .child(text)
            .into_any_element()
    }

    /// A closed fold: how many unchanged lines it holds, and a click opens it once the file's text can be read.
    fn fold_row(&self, file: usize, fold: usize, cx: &mut Context<Self>) -> AnyElement {
        let found = self.stack.folds.get(file).and_then(|f| f.get(fold)).copied();
        let (Some(diff), Some(found)) = (self.stack.diff.as_ref(), found) else {
            return div().h(px(FOLD)).into_any_element();
        };
        let opens = matches!(
            self.stack.texts.get(file_path(&diff.files[file])),
            Some(Text::Read(_) | Text::Reading)
        );
        let label = format!(
            "{} unmodified line{}",
            found.len,
            if found.len == 1 { "" } else { "s" }
        );
        let glyph = if found.hunk == 0 {
            Icon::ArrowUp
        } else if found.hunk >= diff.files[file].hunks.len() {
            Icon::ArrowDown
        } else {
            Icon::ArrowUpDown
        };
        div()
            .id(ElementId::NamedInteger(
                format!("diff-fold-{file}").into(),
                fold as u64,
            ))
            .h(px(FOLD))
            .w_full()
            .flex()
            .items_center()
            .bg(rgba(0xffffff06))
            .text_size(px(12.0))
            .text_color(rgb(theme::muted_foreground()))
            .children(ui::spot(move || format!("diff-fold-{file}-{fold}")))
            // Under the line numbers, past the changed lines' edge.
            .child(
                div()
                    .w(px(3.0 + 96.0 + 22.0))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(icon(glyph, 12.0, theme::muted_foreground())),
            )
            .child(label.clone())
            .when(opens, |row| {
                row.cursor_pointer()
                    .role(Role::Button)
                    .aria_label(SharedString::from(format!("Show {label}")))
                    .hover(|style| {
                        style
                            .bg(rgba(theme::hover()))
                            .text_color(rgb(theme::foreground()))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.open_fold(file, fold, cx)))
            })
            .into_any_element()
    }

    fn hunk_row(&self, file: usize, hunk: usize, line: usize) -> AnyElement {
        let found = self
            .stack
            .diff
            .as_ref()
            .and_then(|diff| diff.files.get(file)?.hunks.get(hunk)?.lines.get(line));
        let Some(found) = found else {
            return div().h(px(ROW)).into_any_element();
        };
        // A removed line in its old side's colours, any other in its new side's.
        let old = found.kind == DiffLineKind::Deleted;
        let spans = self
            .colors
            .get(&file)
            .and_then(|colors| colors.get(hunk)?.get(line))
            .map_or(&[][..], |colors| {
                if old {
                    colors.old.as_slice()
                } else {
                    colors.new.as_slice()
                }
            });
        self.stack_line(
            found.kind,
            found.old_line,
            found.new_line,
            &found.text,
            found.missing_newline,
            spans,
        )
    }

    fn unchanged_row(&self, file: usize, fold: usize, line: usize) -> AnyElement {
        let found = self.stack.folds.get(file).and_then(|f| f.get(fold)).copied();
        let (Some(found), Some(text), Some(diff)) = (
            found,
            self.unchanged_text(file, fold, line),
            self.stack.diff.as_ref(),
        ) else {
            return div().h(px(ROW)).into_any_element();
        };
        let new = found.new_start + line;
        let spans = self
            .stack
            .text_colors
            .get(file_path(&diff.files[file]))
            .and_then(Option::as_ref)
            .and_then(|colors| colors.get(new - 1))
            .map_or(&[][..], Vec::as_slice);
        self.stack_line(
            DiffLineKind::Context,
            Some(found.old_start + line),
            Some(new),
            text,
            false,
            spans,
        )
    }

    /// A line of code: a coloured edge for a changed line, both line numbers, its mark, and its text in its syntax
    /// colours, which scrolls sideways under the numbers when it doesn't wrap.
    fn stack_line(
        &self,
        kind: DiffLineKind,
        old: Option<usize>,
        new: Option<usize>,
        text: &str,
        missing_newline: bool,
        spans: &[Span],
    ) -> AnyElement {
        let (mark, color, bg) = colors(kind);
        let edge = match kind {
            DiffLineKind::Added => Some(theme::done()),
            DiffLineKind::Deleted => Some(theme::danger()),
            DiffLineKind::Context => None,
        };
        let shift = if self.wrap { px(0.0) } else { self.split_x };
        let code = self.styled_code(text, missing_newline, spans, shift);
        div()
            .min_h(px(ROW))
            .w_full()
            .flex()
            .text_size(px(13.0))
            .font_family(crate::terminal_view::DEFAULT_FONT_FAMILY)
            .when(!self.wrap, |row| row.h(px(ROW)).whitespace_nowrap())
            .bg(rgba(bg))
            .text_color(rgb(color))
            .child(
                div()
                    .w(px(3.0))
                    .flex_none()
                    .when_some(edge, |bar, edge| bar.bg(rgb(edge))),
            )
            .child(number(old))
            .child(number(new))
            .child(div().w(px(22.0)).flex_none().child(mark))
            .child(if self.wrap {
                code
            } else {
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .overflow_hidden()
                    .child(code)
            })
            .into_any_element()
    }

    /// The header over the stack: Back, the scope picker or the title, the lines added and removed, the subtitle and
    /// the number of files, then Refresh and Wrap.
    pub(super) fn stack_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let scopes = self
            .view
            .update
            .scopes
            .clone()
            .filter(|scopes| !scopes.is_empty());
        let files = self.view.update.patch.as_ref().map(|diff| diff.files.len());
        let (added, removed) = self
            .counts
            .iter()
            .fold((0, 0), |(a, r), (fa, fr)| (a + fa, r + fr));
        let files_text = files.map(|n| format!("{n} file{}", if n == 1 { "" } else { "s" }));
        let subtitle = [Some(self.view.subtitle.clone()), files_text]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let count = |text: String, color: u32| {
            div()
                .flex_none()
                .text_size(px(12.0))
                .text_color(rgb(color))
                .child(text)
        };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .py(px(8.0))
            .pr(px(8.0))
            .pl(px(if self.back { 8.0 } else { 16.0 }))
            .border_b_1()
            .border_color(rgba(ui::HAIRLINE))
            .when(self.back, |bar| {
                bar.child(
                    ui::icon_button("diff-back", Icon::ArrowLeft, "Back", ui::ButtonSize::Sm)
                        .track_focus(&self.close_focus)
                        .children(ui::spot(|| "diff-back".into()))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close))),
                )
            })
            .child(match &scopes {
                Some(scopes) => self.scope_picker(scopes, cx),
                None => div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.view.title.clone())
                    .into_any_element(),
            })
            .when(files.is_some(), |bar| {
                bar.child(count(format!("+{added}"), theme::done()))
                    .child(count(format!("−{removed}"), theme::blocked()))
            })
            .child(
                div()
                    .ml(px(4.0))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(rgb(theme::muted_foreground()))
                    .child(subtitle),
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
                        button.on_click(cx.listener(|this, _, _, cx| this.request_refresh(cx)))
                    }),
                )
            })
            .child(
                toggle(
                    "diff-wrap",
                    Icon::TextWrap,
                    "Wrap long lines",
                    None,
                    self.wrap,
                )
                .on_click(cx.listener(|this, _, _, cx| this.set_wrap(!this.wrap, cx))),
            )
            .when(!self.back, |bar| {
                bar.child(
                    ui::icon_button("diff-close", Icon::X, "Close diff", ui::ButtonSize::Sm)
                        .track_focus(&self.close_focus)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close))),
                )
            })
            .into_any_element()
    }

    /// A ghost select of the diff's scopes, showing the one picked.
    pub(super) fn scope_picker(&self, scopes: &[Scope], cx: &mut Context<Self>) -> AnyElement {
        let now = self.view.update.scope.as_deref();
        let label = scopes
            .iter()
            .find(|scope| Some(scope.id.as_str()) == now)
            .unwrap_or(&scopes[0])
            .label
            .clone();
        let bounds = self.stack.scope_bounds.clone();
        div()
            .id("diff-scope")
            .role(Role::ComboBox)
            .aria_label("Changes to show")
            .aria_expanded(self.stack.scopes_open)
            .flex_none()
            .h(px(28.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(8.0))
            .rounded(px(8.0))
            .cursor_pointer()
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .when(self.stack.scopes_open, |picker| picker.bg(rgba(theme::hover())))
            .hover(|style| style.bg(rgba(theme::hover())))
            .child(
                canvas(move |at, _, _| bounds.set(at), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .children(ui::spot(|| "diff-scope".into()))
            .child(label)
            .child(icon(Icon::ChevronDown, 14.0, theme::muted_foreground()))
            .on_click(cx.listener(|this, _, _, cx| {
                this.stack.scopes_open = !this.stack.scopes_open;
                this.stack.menu = None;
                cx.notify();
            }))
            .into_any_element()
    }

    /// The scope picker's menu, or a file header's, when one is open.
    pub(super) fn stack_menus(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        // The scope picker is in the one-file header too, for a diff that takes new comments.
        if self.stack.scopes_open {
            return self.scope_menu(cx);
        }
        if !self.stacked() {
            return None;
        }
        self.file_menu(cx)
    }

    fn scope_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let scopes = self
            .view
            .update
            .scopes
            .clone()
            .filter(|scopes| !scopes.is_empty())?;
        let now = self
            .view
            .update
            .scope
            .clone()
            .unwrap_or_else(|| scopes[0].id.clone());
        let trigger = self.stack.scope_bounds.get();
        let items: Vec<AnyElement> = scopes
            .iter()
            .enumerate()
            .map(|(at, scope)| {
                let id = scope.id.clone();
                ui::select_item(
                    ElementId::NamedInteger("diff-scope-item".into(), at as u64),
                    scope.label.clone(),
                    scope.id == now,
                )
                .children(ui::spot(move || format!("diff-scope-item-{at}")))
                .on_click(cx.listener(move |this, _, _, cx| this.pick_scope(id.clone(), cx)))
                .into_any_element()
            })
            .collect();
        Some(ui::dropdown_menu(
            trigger,
            false,
            ui::menu("diff-scope-menu", "Changes to show")
                .children(items)
                // A click on the picker itself toggles it.
                .on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    if !trigger.contains(&event.position) {
                        this.stack.scopes_open = false;
                        cx.notify();
                    }
                })),
        ))
    }

    fn file_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (file, at) = self.stack.menu?;
        let path = file_path(self.stack.diff.as_ref()?.files.get(file)?).to_owned();
        let full = (!self.view.cwd.is_empty())
            .then(|| Path::new(&self.view.cwd).join(&path).display().to_string());
        let copy = |id: &'static str, label: &'static str, text: String| {
            ui::menu_item(id, Icon::Copy, label, false, false)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    this.stack.menu = None;
                    cx.notify();
                }))
                .into_any_element()
        };
        let items: Vec<AnyElement> = std::iter::once(copy("diff-copy-path", "Copy path", path))
            .chain(full.map(|full| copy("diff-copy-full-path", "Copy full path", full)))
            .collect();
        Some(ui::dropdown_menu(
            // The point under the pointer as an empty trigger, 6pt over the menu's top right corner.
            gpui::Bounds::new(point(at.x, at.y + px(2.0)), gpui::Size::default()),
            true,
            ui::menu("diff-file-menu", "File actions")
                .children(items)
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.stack.menu = None;
                    cx.notify();
                })),
        ))
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use crate::{
        diff::{self, View},
        page::plugin::{PluginConfig, PluginHost},
    };
    use gpui::{MouseButton, TestApp, TestAppWindow};
    use serde_json::{Value, json};

    /// `a.rs` changed in two places with twelve unchanged lines between them, `b.rs` added, and `c.png` binary.
    const PATCH: &str = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -5,3 +5,4 @@\n five\n+new six\n six\n seven\n@@ -20,3 +21,2 @@\n twenty\n-twenty-one\n twenty-two\ndiff --git a/b.rs b/b.rs\nnew file mode 100644\n--- /dev/null\n+++ b/b.rs\n@@ -0,0 +1,2 @@\n+one\n+two\ndiff --git a/c.png b/c.png\nBinary files a/c.png and b/c.png differ\n";

    fn open(payload: Value, cwd: &str) -> (TestApp, TestAppWindow<Viewer>) {
        open_in(payload, cwd, None)
    }

    /// `open`, in a window `height` points tall from its first frame, so a list longer than that scrolls.
    fn open_in(payload: Value, cwd: &str, height: Option<f32>) -> (TestApp, TestAppWindow<Viewer>) {
        let view = View {
            id: "test:5".into(),
            plugin_id: "test".into(),
            sidebar: Some("side".into()),
            title: "Changes".into(),
            subtitle: "repo".into(),
            cwd: cwd.into(),
            refresh: true,
            update: diff::clean_update(&payload).unwrap(),
        };
        let host = Arc::new(PluginHost::new(PluginConfig {
            id: "test".into(),
            root: std::env::temp_dir(),
            scope: "test".into(),
        }));
        let mut app = TestApp::new();
        let build = move |_: &mut gpui::Window, cx: &mut gpui::Context<Viewer>| {
            let mut viewer = Viewer::new(view, host, cx);
            viewer.set_stacked(true, cx);
            viewer
        };
        let window = match height {
            None => app.open_window(build),
            Some(height) => app.open_window_with_options(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                        gpui::point(px(0.0), px(0.0)),
                        gpui::size(px(800.0), px(height)),
                    ))),
                    ..Default::default()
                },
                build,
            ),
        };
        (app, window)
    }

    /// A repository folder of its own with a.rs's 30 lines as `PATCH` has them.
    fn repo(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("wings-stack-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let a: String = (1..=30)
            .map(|n| {
                let line = match n {
                    5 => "five".to_owned(),
                    6 => "new six".to_owned(),
                    7 => "six".to_owned(),
                    8 => "seven".to_owned(),
                    21 => "twenty".to_owned(),
                    22 => "twenty-two".to_owned(),
                    n => format!("line {n}"),
                };
                line + "\n"
            })
            .collect();
        std::fs::write(dir.join("a.rs"), a).unwrap();
        dir.to_string_lossy().into_owned()
    }

    /// The Changes sidebar's diff: every file stacked, opened at the file that was clicked, its folds opening to the
    /// lines read from the repository, and a reload keeping the open folds and the line at the top.
    #[test]
    fn every_file_stacks_and_its_folds_open_to_the_lines_in_the_repository() {
        let dir = repo("folds");
        // Short enough that the stack scrolls.
        let (app, mut window) = open_in(
            json!({ "patch": PATCH, "worktree": true, "focus": { "path": "b.rs" } }),
            &dir,
            Some(220.0),
        );
        app.run_until_parked();
        window.draw();
        window.read(|v, _| {
            assert_eq!(
                v.stack.rows[..3],
                [
                    StackRow::Header(0),
                    StackRow::Fold { file: 0, fold: 0 },
                    StackRow::Line { file: 0, hunk: 0, line: 0 },
                ]
            );
            // Once a.rs was read, the eight lines after its last hunk fold too.
            assert_eq!(v.stack.rows[10], StackRow::Fold { file: 0, fold: 2 });
            assert_eq!(v.stack.folds[0][2].len, 8);
            assert_eq!(v.stack.headers, [0, 12, 16]);
            // Opened from b.rs in the sidebar, it starts there, as far down as a list that ends under it can go: here
            // the whole stack is about two screens of this short window.
            assert!(v.stack.list.logical_scroll_top().item_ix > 0, "it scrolled toward b.rs");
            assert!(v.stack.list.bounds_for_item(12).is_some(), "b.rs's header is on screen");
            assert_eq!(v.counts, [(1, 1), (2, 0), (0, 0)]);
        });

        // The twelve lines between the hunks, with their old and new numbers.
        window.update(|v, _, cx| v.open_fold(0, 1, cx));
        window.read(|v, _| {
            assert_eq!(
                v.stack.rows[6..18],
                stack::fold_lines(0, 1, 12).collect::<Vec<_>>()[..]
            );
            assert_eq!(v.stack.list.item_count(), v.stack.rows.len());
            assert_eq!(v.unchanged_text(0, 1, 0), Some("line 9"));
            assert_eq!(v.unchanged_text(0, 1, 11), Some("line 20"));
            assert_eq!(v.stack.headers[1], 23);
        });

        // A file added above a.rs on the next load: the fold stays open and the line at the top stays there.
        window.update(|v, _, _| {
            v.stack.list.scroll_to(ListOffset {
                item_ix: 9,
                offset_in_item: px(5.0),
            })
        });
        let next = format!(
            "diff --git a/0.rs b/0.rs\nnew file mode 100644\n--- /dev/null\n+++ b/0.rs\n@@ -0,0 +1,2 @@\n+zero\n+two\n{PATCH}"
        );
        window.update(|v, _, cx| v.update(diff::clean_update(&json!({ "patch": next })).unwrap(), cx));
        window.read(|v, _| {
            assert_eq!(v.stack.rows[13], StackRow::Unchanged { file: 1, fold: 1, line: 3 });
            let top = v.stack.list.logical_scroll_top();
            assert_eq!((top.item_ix, top.offset_in_item), (13, px(5.0)));
        });
    }

    /// Only the rows on screen are drawn, and the file whose rows are at the top keeps its header over them.
    #[test]
    fn only_the_rows_on_screen_render_and_the_file_at_the_top_keeps_its_header() {
        let patch: String = (0..20)
            .map(|f| {
                let lines: String = (0..100).map(|l| format!("+line {l}\n")).collect();
                format!(
                    "diff --git a/f{f:02}.rs b/f{f:02}.rs\nnew file mode 100644\n--- /dev/null\n+++ b/f{f:02}.rs\n@@ -0,0 +1,100 @@\n{lines}"
                )
            })
            .collect();
        let (_app, mut window) = open(json!({ "patch": patch }), "");
        window.draw();
        window.read(|v, _| {
            assert_eq!(v.stack.rows.len(), 20 * 101 + 19);
            assert!(
                (1..100).contains(&v.rendered_code_rows()),
                "rendered {} rows",
                v.rendered_code_rows()
            );
        });
        assert!(crate::ui::spot_at("diff-sticky-0").is_none());
        window.update(|v, _, cx| {
            v.select_file(2, cx);
            let at = v.stack.headers[2] + 40;
            v.stack.list.scroll_to(ListOffset {
                item_ix: at,
                offset_in_item: px(0.0),
            });
            cx.notify();
        });
        window.draw();
        let code = crate::ui::spot_at("diff-code").unwrap();
        let sticky = crate::ui::spot_at("diff-sticky-2").unwrap();
        assert_eq!(sticky.top(), code.top());
    }

    /// The picker shows the other scope's patch: it asks the plugin for it, and drops a patch of the scope before that
    /// was still on its way.
    #[test]
    fn picking_a_scope_loads_its_patch_and_drops_one_of_the_scope_before() {
        let scopes = json!([{ "id": "uncommitted", "label": "Uncommitted" }, { "id": "branch", "label": "Branch" }]);
        let (_app, mut window) = open(
            json!({ "patch": PATCH, "scopes": scopes, "scope": "uncommitted" }),
            "",
        );
        window.draw();
        let click = |window: &mut TestAppWindow<Viewer>, name: &str| {
            let spot = crate::ui::spot_at(name).unwrap_or_else(|| panic!("no {name}"));
            window.simulate_click(spot.center(), MouseButton::Left);
            window.draw();
        };
        click(&mut window, "diff-scope");
        window.read(|v, _| assert!(v.stack.scopes_open));
        click(&mut window, "diff-scope-item-1");
        window.read(|v, _| {
            assert_eq!(v.view.update.scope.as_deref(), Some("branch"));
            assert!(v.view.update.patch.is_none() && v.stack.rows.is_empty());
            assert!(!v.stack.scopes_open);
        });
        let shown = |payload: Value, window: &mut TestAppWindow<Viewer>| {
            window.update(|v, _, cx| v.update(diff::clean_update(&payload).unwrap(), cx));
        };
        shown(json!({ "patch": PATCH, "scope": "uncommitted" }), &mut window);
        window.read(|v, _| assert!(v.view.update.patch.is_none()));
        let branch = "diff --git a/d.rs b/d.rs\n--- a/d.rs\n+++ b/d.rs\n@@ -1 +1 @@\n-x\n+y\n";
        shown(json!({ "patch": branch, "scope": "branch" }), &mut window);
        window.read(|v, _| {
            let diff = v.stack.diff.as_ref().unwrap();
            assert_eq!(file_path(&diff.files[0]), "d.rs");
            assert_eq!(v.stack.list.logical_scroll_top().item_ix, 0);
        });
    }

    /// A diff with review threads shows them under their lines one file at a time, as before.
    #[test]
    fn a_diff_with_comments_keeps_the_one_file_view() {
        let comment = json!({ "id": 1, "replyTo": null, "path": "b.rs", "line": 1, "side": "additions", "author": "maria", "body": "Why?",
            "createdAt": "2026-11-01T10:00:00Z", "url": "", "canReply": true });
        let (_app, window) = open(json!({ "patch": PATCH, "comments": [comment] }), "");
        window.read(|v, _| {
            assert!(!v.stacked() && v.stack.rows.is_empty());
            assert!(!v.rows.is_empty());
        });
    }
}
