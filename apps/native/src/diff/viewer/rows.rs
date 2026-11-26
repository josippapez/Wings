//! A diff's code rows, unified and split.
use super::*;

impl Viewer {
    pub(super) fn code_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
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
            .relative()
            .group(SharedString::from(format!("diff-line-{index}")))
            .flex()
            .text_size(px(13.0))
            .font_family(crate::terminal_view::DEFAULT_FONT_FAMILY)
            .when(!self.wrap, |row| row.h(px(ROW)).whitespace_nowrap());
        match self.rows[index] {
            CodeRow::Hunk(h) => {
                let hunk = &file.hunks[h];
                row.items_center()
                    .bg(rgba(theme::info() << 8 | 0x16))
                    .text_color(rgb(theme::info()))
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
                // One column shows a removed line as the old file has it, else as the new one does, as the desktop.
                let spans = self.spans(h, l, line.kind == DiffLineKind::Deleted);
                // In the list of `varied_rows` the text moves sideways on its own, under the line numbers.
                let text = if self.varied_rows() && !self.wrap {
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .overflow_hidden()
                        .child(self.code_text(line, spans, self.split_x))
                } else {
                    self.code_text(line, spans, px(0.0))
                };
                let add = self.add_comment_button(
                    index,
                    (h, l),
                    line.kind == DiffLineKind::Deleted,
                    cx,
                );
                row.bg(rgba(bg))
                    .text_color(rgb(color))
                    .children(add)
                    .child(number(line.old_line))
                    .child(number(line.new_line))
                    .child(div().w(px(22.0)).flex_none().child(mark))
                    .child(text)
                    .into_any_element()
            }
            CodeRow::Split { hunk, left, right } => {
                let lines = &file.hunks[hunk].lines;
                let side =
                    |at: Option<usize>, old| at.map(|l| (&lines[l], self.spans(hunk, l, old)));
                let add = |at: Option<usize>, old, cx: &mut Context<Self>| {
                    at.and_then(|l| self.add_comment_button(index, (hunk, l), old, cx))
                };
                let (add_left, add_right) = (add(left, true, cx), add(right, false, cx));
                row.child(self.split_side(side(left, true), true, add_left))
                    .child(div().w(px(1.0)).flex_none().bg(rgba(ui::HAIRLINE)))
                    .child(self.split_side(side(right, false), false, add_right))
                    .into_any_element()
            }
            CodeRow::Notes(n) => self.notes_row(&self.notes[n], cx),
        }
    }

    /// One column of a side-by-side row: the old file's line on the left, the new one's on the right.
    pub(super) fn split_side(
        &self,
        line: Option<(&DiffLine, &[Span])>,
        old: bool,
        add: Option<AnyElement>,
    ) -> Div {
        let side = div().flex_1().min_w_0().relative().flex().overflow_hidden();
        let Some((line, spans)) = line else {
            return side;
        };
        let (mark, color, bg) = colors(line.kind);
        side.bg(rgba(bg))
            .text_color(rgb(color))
            .children(add)
            .child(number(if old { line.old_line } else { line.new_line }))
            .child(div().w(px(22.0)).flex_none().child(mark))
            .child(self.code_text(line, spans, self.split_x))
    }

    /// A line's text in its syntax colours, moved left by `shift` when unwrapped side by side. As on the desktop, a
    /// changed line keeps its colours on its green or red background.
    pub(super) fn code_text(&self, line: &DiffLine, spans: &[Span], shift: Pixels) -> Div {
        self.styled_code(&line.text, line.missing_newline, spans, shift)
    }

    /// `code_text` for any text: a hunk's line, or one an open fold shows.
    pub(super) fn styled_code(&self, text: &str, missing_newline: bool, spans: &[Span], shift: Pixels) -> Div {
        let mut start = 0;
        let highlights = spans.iter().map(|span| {
            let range = start..span.end;
            start = span.end;
            let style = HighlightStyle {
                color: Some(rgb(span.color).into()),
                font_style: span.italic.then_some(gpui::FontStyle::Italic),
                ..HighlightStyle::default()
            };
            (range, style)
        });
        let text = format!(
            "{}{}",
            text,
            if missing_newline {
                "  ⏎ No newline at end of file"
            } else {
                ""
            }
        );
        let text = StyledText::new(text).with_highlights(highlights.collect::<Vec<_>>());
        let code = div().text_color(rgb(highlight::FOREGROUND));
        if self.wrap {
            code.flex_1().min_w_0().pr(px(12.0)).child(text)
        } else {
            code.flex_none().ml(-shift).child(text)
        }
    }
}
