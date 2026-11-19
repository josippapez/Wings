//! Text a person selects on a page by dragging the mouse over it: the selection is kept on the page, drawn as a
//! background over the words, copied with Cmd-C, and sent to the page with a right-click in a `Stack` that has an
//! `onContextMenu`. A selection lives in one line of text, the one the press started in.

use std::ops::Range;

use gpui::{
    AnyElement, ClipboardItem, Context, HighlightStyle, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    StyledText, TextLayout, Window, div, prelude::*, rgba,
};

use super::{PageView, tree::Node};

/// The desktop terminal's `selectionBackground`, as a text field draws its selection.
const SELECTED: u32 = 0xffffff33;

/// The words a person has dragged over in the line of text `id`, from where the press was (`anchor`) to where the
/// pointer is (`head`), either of them the first. `text` is the whole line, to know the line has not changed since.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Selection {
    id: u64,
    anchor: usize,
    head: usize,
    text: String,
}

impl Selection {
    fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    /// The selected words, None while the press has not moved.
    fn chosen(&self) -> Option<&str> {
        self.text.get(self.range()).filter(|words| !words.is_empty())
    }
}

/// The word `at` is in, or where it is when it is between words.
fn word_at(text: &str, at: usize) -> Range<usize> {
    if !text.is_char_boundary(at) {
        return at..at;
    }
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let start = text[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| word(*c))
        .last()
        .map_or(at, |(start, _)| start);
    let end = text[at..]
        .char_indices()
        .find(|(_, c)| !word(*c))
        .map_or(text.len(), |(end, _)| at + end);
    start..end
}

/// `highlights` with `selected` laid over them: runs cut where the selection starts and ends, in order and none
/// overlapping another, as `StyledText` takes them.
pub(super) fn with_selection(
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    selected: Option<Range<usize>>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(selected) = selected else {
        return highlights;
    };
    let mut cuts: Vec<usize> = highlights
        .iter()
        .flat_map(|(range, _)| [range.start, range.end])
        .chain([selected.start, selected.end])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter_map(|cut| {
            let (start, end) = (cut[0], cut[1]);
            let base = highlights
                .iter()
                .find(|(range, _)| range.start <= start && end <= range.end)
                .map(|(_, style)| *style);
            let inside = selected.start <= start && end <= selected.end;
            if base.is_none() && !inside {
                return None;
            }
            let mut style = base.unwrap_or_default();
            if inside {
                style.background_color = Some(rgba(SELECTED).into());
            }
            Some((start..end, style))
        })
        .collect()
}

/// Whether a node `id` is anywhere in `nodes`.
fn has_node(nodes: &[Node], id: u64) -> bool {
    nodes
        .iter()
        .any(|node| node.id() == Some(id) || node.parts().into_iter().any(|part| has_node(part, id)))
}

impl PageView {
    /// The words selected on the page, None when there are none.
    pub(crate) fn selected_text(&self) -> Option<String> {
        self.selection.borrow().as_ref()?.chosen().map(str::to_owned)
    }

    /// The byte range of `text`, the line `id`, that is selected. A selection of a line that has since changed is
    /// dropped: its range would be of other words.
    pub(super) fn selection_in(&self, id: u64, text: &str) -> Option<Range<usize>> {
        let mut selection = self.selection.borrow_mut();
        let current = selection.as_ref().filter(|selection| selection.id == id)?;
        if current.text != text {
            *selection = None;
            return None;
        }
        current.chosen().map(|_| current.range())
    }

    /// Drops the selection when the line it was in has left the page.
    pub(super) fn keep_selection_in(&self, nodes: &[Node]) {
        self.selection
            .borrow_mut()
            .take_if(|selection| !has_node(nodes, selection.id));
    }

    /// `text`, a plain string of the node `id`, as a line a person can select words of.
    pub(super) fn selectable_string(&self, id: u64, text: String, cx: &mut Context<Self>) -> AnyElement {
        let marks = with_selection(Vec::new(), self.selection_in(id, &text));
        let styled = StyledText::new(text.clone()).with_highlights(marks);
        let layout = styled.layout().clone();
        self.selectable(id, text, layout, styled.into_any_element(), cx)
    }

    /// `element`, the line of `text` that `layout` draws, with a press and a drag over it selecting words. A press
    /// without a drag selects none, so a click on a link or a clickable span is still a click; a double press takes the
    /// word. `id` names the line. A press anywhere on the page first drops what was selected (`render` does that).
    pub(super) fn selectable(
        &self,
        id: u64,
        text: String,
        layout: TextLayout,
        element: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (press_layout, drag_layout) = (layout.clone(), layout);
        let whole = text.clone();
        div()
            .child(element)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    let at = press_layout
                        .index_for_position(event.position)
                        .unwrap_or_else(|edge| edge);
                    let (anchor, head) = if event.click_count == 2 {
                        let word = word_at(&text, at);
                        (word.start, word.end)
                    } else {
                        (at, at)
                    };
                    *view.selection.borrow_mut() = Some(Selection { id, anchor, head, text: text.clone() });
                    if anchor != head {
                        window.focus(&view.selection_focus, cx);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, window, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let at = drag_layout
                    .index_for_position(event.position)
                    .unwrap_or_else(|edge| edge);
                let mut selection = view.selection.borrow_mut();
                if let Some(selection) = selection.as_mut().filter(|selection| selection.id == id && selection.text == whole)
                    && selection.head != at
                {
                    selection.head = at;
                    window.focus(&view.selection_focus, cx);
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    /// A left press on the page, anywhere: what was selected is let go before the line pressed on selects anew.
    pub(super) fn release_selection(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if event.button == MouseButton::Left && self.selection.borrow_mut().take().is_some() {
            cx.notify();
        }
    }

    /// Cmd-C (Ctrl-C elsewhere) with words selected puts them on the clipboard. A field that has the keyboard copies its
    /// own selection first and stops the key, so this never takes it from one.
    pub(super) fn copy_selection(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let mods = event.keystroke.modifiers;
        let secondary = if cfg!(target_os = "macos") { mods.platform } else { mods.control };
        if event.keystroke.key != "c" || !secondary || mods.shift || mods.alt {
            return;
        }
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            cx.stop_propagation();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold() -> HighlightStyle {
        HighlightStyle { font_weight: Some(gpui::FontWeight::BOLD), ..Default::default() }
    }

    #[test]
    fn a_selection_is_laid_over_the_runs_of_a_line_without_overlapping_them() {
        let runs: Vec<_> = with_selection(vec![(2..6, bold())], Some(4..9))
            .into_iter()
            .map(|(range, style)| (range, style.font_weight.is_some(), style.background_color.is_some()))
            .collect();
        assert_eq!(runs, [(2..4, true, false), (4..6, true, true), (6..9, false, true)]);
        assert_eq!(with_selection(vec![(2..6, bold())], None).len(), 1);
    }

    #[test]
    fn a_double_press_takes_the_word_under_it() {
        assert_eq!(word_at("one two_x three", 5), 4..9);
        assert_eq!(word_at("one two", 3), 0..3);
        assert_eq!(word_at("a  b", 2), 2..2);
    }
}
