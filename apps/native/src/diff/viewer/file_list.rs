//! The file list beside the code: its header and rows.
use super::*;

impl Viewer {
    /// The desktop's row above the file list: the filter, and the flat list or folder tree switch.
    pub(super) fn list_header(&self, cx: &mut Context<Self>) -> Div {
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
                    .child(icon(Icon::Search, 14.0, theme::muted_foreground()))
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
    pub(super) fn list_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
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
                    .text_color(rgb(theme::muted_foreground()))
                    .hover(|style| {
                        style
                            .bg(rgba(theme::hover()))
                            .text_color(rgb(theme::foreground()))
                    })
                    .child(icon(
                        if *open {
                            Icon::ChevronDown
                        } else {
                            Icon::ChevronRight
                        },
                        12.0,
                        theme::muted_foreground(),
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
                    DiffStatus::Added => ("A", theme::done()),
                    DiffStatus::Deleted => ("D", theme::blocked()),
                    DiffStatus::Modified => ("M", theme::working()),
                    DiffStatus::Renamed => ("R", theme::info()),
                    DiffStatus::Copied => ("C", theme::info()),
                };
                let (added, removed) = self.counts.get(index).copied().unwrap_or_default();
                let comments = self.file_comments.get(index).copied().unwrap_or_default();
                let (folder, name) = match path.rsplit_once('/') {
                    Some((folder, name)) => (Some(folder.to_owned()), name.to_owned()),
                    None => (None, path.clone()),
                };
                let selected = index == self.selected && !self.discussion_open;
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
                        row.hover(|style| style.bg(rgba(theme::hover())))
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
                                    ui::behaviors::cut_text("folder", folder)
                                        .text_color(rgb(theme::muted_foreground())),
                                )
                            }),
                    )
                    .when(comments > 0, |row| {
                        row.child(comment_count(comments, 12.0).flex_none())
                    })
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::done()))
                            .child(format!("+{added}")),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::blocked()))
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
}
