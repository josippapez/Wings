//! What a viewer holds of its diff: updates from the plugin, comments placed on rows, the rows rebuilt and coloured.
use super::*;

impl Viewer {
    /// What the plugin sent: a new patch starts the file again, at its focus or its top. Comments alone, or with the patch
    /// already shown, as after a reply and on each of a live diff's loads, leave the code where it's scrolled to.
    pub fn update(&mut self, update: Update, cx: &mut Context<Self>) {
        // One for a scope other than the one picked was on its way when another was picked.
        if let (Some(scope), Some(now)) = (&update.scope, &self.view.update.scope)
            && scope != now
        {
            return;
        }
        let stacked = self.stacked();
        let mut rebuild = update.focus.is_some();
        if let Some(patch) = update.patch {
            rebuild |= self.view.update.patch.as_ref() != Some(&patch);
            // A file that didn't change keeps its colours, so a live diff's reload doesn't draw it plain again.
            let old = self.view.update.patch.clone();
            self.colors.retain(|at, _| {
                old.as_ref().and_then(|old| old.files.get(*at)) == patch.files.get(*at)
            });
            self.keep_texts(old.as_deref(), &patch);
            self.view.update.patch = Some(patch);
            self.view.update.error = None;
        }
        if let Some(error) = update.error {
            self.view.update.error = Some(error);
        }
        if let Some(focus) = update.focus {
            self.view.update.focus = Some(focus);
            self.stack.focus_pending = true;
        }
        if let Some(scope) = update.scope {
            self.view.update.scope = Some(scope);
        }
        if let Some(scopes) = update.scopes {
            self.view.update.scopes = Some(scopes);
        }
        if let Some(worktree) = update.worktree {
            self.view.update.worktree = Some(worktree);
        }
        if let Some(allow) = update.allow_new_comments {
            self.view.update.allow_new_comments = Some(allow);
            if !allow {
                self.compose = None;
            }
        }
        let comments = update.comments.is_some() || update.allow_new_comments.is_some();
        if let Some(list) = update.comments {
            self.view.update.comments = Some(list);
            self.keep_drafts();
        }
        // Review threads show one file at a time, so comments coming or going change the layout.
        rebuild |= self.stacked() != stacked;
        if rebuild {
            self.rebuild(cx);
        } else if comments && self.stacked() {
            self.group_comments();
        } else if comments {
            self.rethread();
        }
        cx.notify();
    }

    /// The comments, or none.
    pub(super) fn comments(&self) -> &[Comment] {
        self.view
            .update
            .comments
            .as_deref()
            .map_or(&[], Vec::as_slice)
    }

    /// Groups the comments into threads, and counts each file's.
    pub(super) fn group_comments(&mut self) {
        let comments = self
            .view
            .update
            .comments
            .as_deref()
            .map_or(&[][..], Vec::as_slice);
        self.threads = comments::threads(comments);
        self.discussion = comments::discussion(comments, &self.threads);
        // A reply posted in the discussion leaves it where it's scrolled to.
        if self.discussion_list.item_count() != self.discussion.len() {
            self.discussion_list.reset(self.discussion.len());
        }
        self.file_comments = self
            .view
            .update
            .patch
            .as_ref()
            .map_or_else(Vec::new, |diff| {
                diff.files
                    .iter()
                    .map(|file| comments::on_file(comments, &self.threads, file_path(file)))
                    .collect()
            });
        self.bodies.borrow_mut().clear();
        // A reply box whose thread is gone has nothing to answer.
        let roots: HashSet<i64> = self.threads.iter().map(|t| comments[t.root].id).collect();
        self.replies.retain(|id, _| roots.contains(id));
        if self.discussion.is_empty() {
            self.discussion_open = false;
        }
    }

    /// The open file's rows with its threads under their lines.
    pub(super) fn place_notes(&mut self, rows: Vec<CodeRow>) {
        let Some(file) = self
            .view
            .update
            .patch
            .as_ref()
            .and_then(|diff| diff.files.get(self.selected))
        else {
            self.rows = rows;
            self.notes.clear();
            return;
        };
        let comments = self
            .view
            .update
            .comments
            .as_deref()
            .map_or(&[][..], Vec::as_slice);
        let mut on_lines = comments::on_lines(comments, &self.threads, file_path(file));
        if let Some(compose) = self.compose.as_ref().filter(|c| c.path == file_path(file)) {
            on_lines
                .entry((compose.old, compose.line))
                .or_default()
                .push(compose::COMPOSE);
        }
        (self.rows, self.notes) = with_notes(rows, &file.hunks, &on_lines);
    }

    /// The comments changed and the patch didn't: the threads go under their lines again, and the code stays where it's
    /// scrolled to, the thread at the top too.
    pub(super) fn rethread(&mut self) {
        let (code, in_notes, offset) = self.top_row();
        self.group_comments();
        let rows = self
            .rows
            .iter()
            .copied()
            .filter(|row| !matches!(row, CodeRow::Notes(_)))
            .collect();
        self.place_notes(rows);
        let at = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| !matches!(row, CodeRow::Notes(_)))
            .nth(code)
            .map_or(0, |(at, _)| at);
        let at = if in_notes && matches!(self.rows.get(at + 1), Some(CodeRow::Notes(_))) {
            at + 1
        } else {
            at
        };
        self.wrapped.reset(self.rows.len());
        if self.varied_rows() {
            self.wrapped.scroll_to(ListOffset {
                item_ix: at,
                offset_in_item: offset,
            });
        } else {
            let handle = self.scroll.0.borrow().base_handle.clone();
            let x = handle.offset().x;
            handle.set_offset(point(x, -(px(at as f32 * ROW) + offset)));
        }
    }

    /// The row at the top of the code, as the how-manyth row of code it is or follows, whether it's that row's threads,
    /// and how far into it the code is scrolled.
    pub(super) fn top_row(&self) -> (usize, bool, Pixels) {
        let (at, offset) = if self.varied_rows() {
            let top = self.wrapped.logical_scroll_top();
            (top.item_ix, top.offset_in_item)
        } else {
            let y = -self.scroll.0.borrow().base_handle.offset().y;
            let at = (y / px(ROW)).floor().max(0.0) as usize;
            (at, y - px(at as f32 * ROW))
        };
        let at = at.min(self.rows.len().saturating_sub(1));
        let in_notes = matches!(self.rows.get(at), Some(CodeRow::Notes(_)));
        let code = self.rows[..at]
            .iter()
            .filter(|row| !matches!(row, CodeRow::Notes(_)))
            .count();
        (code - usize::from(in_notes && code > 0), in_notes, offset)
    }

    /// Rows of different heights, in a list that measures each: wrapped lines, or threads under the lines.
    pub(super) fn varied_rows(&self) -> bool {
        self.wrap || !self.notes.is_empty()
    }

    pub(super) fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.rows.clear();
        self.notes.clear();
        self.relist(cx);
        self.group_comments();
        let Some(diff) = &self.view.update.patch else {
            self.restack(cx);
            return;
        };
        self.counts = diff.files.iter().map(super::super::stack::counts).collect();
        if self.stacked() {
            self.restack(cx);
            self.read_texts(cx);
            return;
        }
        if let Some(focus) = &self.view.update.focus
            && let Some(index) = diff.files.iter().position(|file| {
                file.new_path.as_deref() == Some(&focus.path)
                    || file.old_path.as_deref() == Some(&focus.path)
            })
        {
            self.selected = index;
            self.discussion_open = false;
        }
        self.selected = self.selected.min(diff.files.len().saturating_sub(1));
        let Some(file) = diff.files.get(self.selected) else {
            return;
        };
        let rows = code_rows(&file.hunks, self.split());
        self.place_notes(rows);
        self.color_file(cx);
        let diff = self.view.update.patch.as_ref().unwrap();
        let file = &diff.files[self.selected];
        let focus = self.view.update.focus.as_ref().filter(|f| f.line.is_some());
        let target = focus.and_then(|focus| {
            self.rows.iter().position(|row| {
                let (h, l) = match *row {
                    CodeRow::Line(h, l) => (h, Some(l)),
                    CodeRow::Split {
                        hunk, left, right, ..
                    } => (hunk, if focus.old { left } else { right }),
                    CodeRow::Hunk(_) | CodeRow::Notes(_) => return false,
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
        self.scroll.reveal(target, Reveal::Top);
        self.wrapped.scroll_to(ListOffset {
            item_ix: target,
            offset_in_item: px(0.0),
        });
    }

    /// Works out the open file's syntax colours off the window's thread, unless they're known or under way.
    pub(super) fn color_file(&mut self, cx: &mut Context<Self>) {
        let (index, Some(diff)) = (self.selected, self.view.update.patch.clone()) else {
            return;
        };
        let under_way = self
            .coloring
            .as_ref()
            .is_some_and(|(at, of, _)| *at == index && Arc::ptr_eq(of, &diff));
        if self.colors.contains_key(&index) || under_way || index >= diff.files.len() {
            return;
        }
        let work = cx.background_executor().spawn({
            let diff = diff.clone();
            async move { highlight::file_colors(&diff.files[index]) }
        });
        let task = cx.spawn({
            let diff = diff.clone();
            async move |this, cx| {
                let colors = work.await;
                let _ = this.update(cx, |this, cx| {
                    if this
                        .view
                        .update
                        .patch
                        .as_ref()
                        .is_some_and(|now| Arc::ptr_eq(now, &diff))
                    {
                        this.colors.insert(index, colors);
                        cx.notify();
                    }
                });
            }
        });
        self.coloring = Some((index, diff, task));
    }

    /// A line's colours on its old side or its new one, or none until they come.
    pub(super) fn spans(&self, hunk: usize, line: usize, old: bool) -> &[Span] {
        self.colors
            .get(&self.selected)
            .and_then(|file| file.get(hunk)?.get(line))
            .map_or(&[], |colors| if old { &colors.old } else { &colors.new })
    }
}
