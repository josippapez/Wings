//! Scrolling, choosing a file, asking the plugin to refresh, and the keys.
use super::*;

impl Viewer {
    /// Moves unwrapped side-by-side code sideways, as far as its longest line goes.
    pub(super) fn scroll_split(&mut self, by: Pixels, cx: &mut Context<Self>) {
        let most = px((self.widest as f32 * CHAR - 200.0).max(0.0));
        self.split_x = (self.split_x - by).clamp(px(0.0), most);
        cx.notify();
    }

    pub fn select_file(&mut self, index: usize, cx: &mut Context<Self>) {
        // Stacked, every file is open: it scrolls to the file.
        if self.stacked() {
            self.selected = index;
            self.scroll_to_file(index);
            cx.notify();
            return;
        }
        if self
            .view
            .update
            .patch
            .as_ref()
            .is_some_and(|diff| index < diff.files.len())
        {
            self.selected = index;
            self.discussion_open = false;
            self.view.update.focus = None;
            self.rebuild(cx);
            self.horizontal.set_offset(gpui::point(px(0.0), px(0.0)));
            let row = self
                .list_rows
                .iter()
                .position(|row| matches!(row, ListRow::File { index: file, .. } if *file == index));
            if let Some(row) = row {
                self.files_scroll.reveal(row, Reveal::Center);
            }
            cx.notify();
        }
    }

    pub fn selected_file(&self) -> usize {
        self.selected
    }

    /// Shows the discussion in place of a file: the comments on the whole change and on code that changed since.
    pub fn open_discussion(&mut self, cx: &mut Context<Self>) {
        if self.discussion.is_empty() {
            return;
        }
        self.discussion_open = true;
        // Chosen here, the plugin's focus no longer picks the file, as when a file is chosen.
        self.view.update.focus = None;
        self.discussion_list.reset(self.discussion.len());
        cx.notify();
    }

    /// What the file list shows, in its order: the discussion (None) when there is one, then the files.
    pub(super) fn listed(&self) -> Vec<Option<usize>> {
        (!self.discussion.is_empty())
            .then_some(None)
            .into_iter()
            .chain(self.listed_files().into_iter().map(Some))
            .collect()
    }

    #[cfg(feature = "test-support")]
    pub fn rendered_code_rows(&self) -> usize {
        self.rendered_rows.get()
    }

    pub fn request_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.view.refresh || self.refreshing {
            return;
        }
        self.ask_plugin(None, cx);
    }

    /// Asks the plugin to load the diff again, of `scope` when one was picked, and spins until it's done.
    pub(super) fn ask_plugin(&mut self, scope: Option<String>, cx: &mut Context<Self>) {
        self.refreshing = true;
        self.refresh_error = None;
        let (sent, received) = async_channel::bounded(1);
        let done = move |result: Result<(), String>| {
            let _ = sent.try_send(result);
        };
        match scope {
            Some(scope) => self.host.send_diff_scope(&self.view.id, &scope, done),
            None => self.host.send_diff_refresh(&self.view.id, done),
        }
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

    /// How many rows the file list shows at once, for Page Up and Page Down.
    pub(super) fn files_page(&self) -> usize {
        let size = self.files_scroll.0.borrow().last_item_size;
        match size {
            Some(size) if !self.list_rows.is_empty() && size.contents.height > px(0.0) => {
                let row = size.contents.height / self.list_rows.len() as f32;
                ((size.item.height / row) as usize).max(1)
            }
            _ => 1,
        }
    }

    pub(super) fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let pressed = matches!(key.key.as_str(), "enter" | "space");
        if (key.modifiers.platform && key.key == "w")
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
            // Up and down go through the discussion and the files the list shows, in its order.
            let listed = self.listed();
            let open = (!self.discussion_open).then_some(self.selected);
            let at = listed.iter().position(|entry| *entry == open);
            let next = match listbox::clamp_after(key.key.as_str(), at, listed.len(), self.files_page()) {
                Some(next) => next,
                // Typing a file's name goes to it.
                None => {
                    let files = self.view.update.patch.as_ref().map(|diff| &diff.files);
                    let name = |at: usize| {
                        let file = files?.get((*listed.get(at)?)?)?;
                        Some(file_path(file).rsplit('/').next().unwrap_or_default().to_owned())
                    };
                    let Some(next) = listbox::typed_text(event).and_then(|text| {
                        self.files_typeahead.press(text, std::time::Instant::now(), at, listed.len(), name)
                    }) else {
                        return;
                    };
                    next
                }
            };
            match listed[next] {
                Some(index) => self.select_file(index, cx),
                None => self.open_discussion(cx),
            }
        } else if self.code_focus.is_focused(window) && self.stacked() {
            if !self.stack_key(key.key.as_str(), cx) {
                return;
            }
        } else if self.code_focus.is_focused(window) {
            if matches!(key.key.as_str(), "left" | "right")
                && !self.wrap
                && (self.split() || self.varied_rows())
            {
                self.scroll_split(px(if key.key == "left" { 40.0 } else { -40.0 }), cx);
                window.prevent_default();
                cx.stop_propagation();
                return;
            }
            if self.varied_rows() {
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
}
