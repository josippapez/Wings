//! Text a person selects on a page by dragging the mouse over it: the selection is kept on the page, drawn as a
//! background over the words, copied with Cmd-C, and sent to the page with a right-click in a `Stack` that has an
//! `onContextMenu`. A selection starts in the line of text the press was in and runs, in the order the page draws its
//! lines, to the one the pointer is over: across paragraphs, code blocks and messages in one column.

use std::{
    cell::Cell,
    collections::HashMap,
    ops::Range,
    rc::Rc,
};

use gpui::{
    AnyElement, Bounds, ClipboardItem, Context, HighlightStyle, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Point, StyledText, TextLayout, Window, canvas, div, prelude::*, rgba,
};

use super::{PageView, tree::Node};

/// The desktop terminal's `selectionBackground`, as a text field draws its selection.
const SELECTED: u32 = 0xffffff33;

/// A place in the line of text `id`: the byte `at`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    id: u64,
    at: usize,
}

/// What a person has dragged over: from where the press was (`anchor`) to where the pointer is (`head`), either of them
/// the first. `text` is the line the press was in, to know it has not changed since.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Selection {
    anchor: Spot,
    head: Spot,
    text: String,
}

impl Selection {
    /// The two ends in the order the lines are drawn, by `order` (a line's place on the page).
    fn ordered(&self, order: &HashMap<u64, usize>) -> Option<(Spot, Spot)> {
        let (anchor, head) = (self.anchor, self.head);
        let anchor_first = if anchor.id == head.id { anchor.at <= head.at } else { order.get(&anchor.id)? < order.get(&head.id)? };
        Some(if anchor_first { (anchor, head) } else { (head, anchor) })
    }

    /// The part of the line `id`, `len` bytes long, that is selected: None when none of it is.
    fn range_in(&self, id: u64, len: usize, order: &HashMap<u64, usize>) -> Option<Range<usize>> {
        let (start, end) = self.ordered(order)?;
        let range = if start.id == end.id {
            (id == start.id).then_some(start.at..end.at)?
        } else if id == start.id {
            start.at..len
        } else if id == end.id {
            0..end.at
        } else {
            let at = order.get(&id)?;
            (order.get(&start.id)? < at && at < order.get(&end.id)?).then_some(0..len)?
        };
        let range = range.start.min(len)..range.end.min(len);
        (!range.is_empty()).then_some(range)
    }
}

/// A line of text a person can select, as the page drew it: its text, how it was laid out, and where it was drawn,
/// known once it has been (a line scrolled out of a list may not be).
pub(super) struct Line {
    id: u64,
    text: String,
    layout: TextLayout,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

/// The lines of text the page drew, in the order it drew them: the last render's, which a render reads to know which
/// lines lie between a selection's ends, and the ones the render under way draws, which the mouse then goes by.
#[derive(Default)]
pub(super) struct Lines {
    drawn: HashMap<u64, usize>,
    drawing: Vec<Line>,
}

impl Lines {
    /// A render starts: what the last one drew becomes the order it goes by.
    pub(super) fn start(&mut self) {
        self.drawn = order_of(&std::mem::take(&mut self.drawing));
    }
}

fn order_of(lines: &[Line]) -> HashMap<u64, usize> {
    lines.iter().enumerate().map(|(at, line)| (line.id, at)).collect()
}

/// Where the pointer `at` is among `lines`, keeping to those in the `column` the selection started in: in the line it is
/// over, else at the end of the last line above it, else at the start of the first.
fn spot_at(lines: &[Line], at: Point<Pixels>, column: Bounds<Pixels>) -> Option<Spot> {
    let shown: Vec<(&Line, Bounds<Pixels>)> = lines
        .iter()
        .filter_map(|line| Some((line, line.bounds.get()?)))
        .filter(|(_, bounds)| bounds.left() < column.right() && column.left() < bounds.right())
        .collect();
    if let Some((line, _)) = shown.iter().find(|(_, bounds)| bounds.contains(&at)) {
        return Some(Spot { id: line.id, at: line.layout.index_for_position(at).unwrap_or_else(|edge| edge) });
    }
    if let Some((line, _)) = shown.iter().filter(|(_, bounds)| bounds.bottom() <= at.y).max_by(|a, b| a.1.bottom().partial_cmp(&b.1.bottom()).unwrap_or(std::cmp::Ordering::Equal)) {
        return Some(Spot { id: line.id, at: line.text.len() });
    }
    shown.iter().min_by(|a, b| a.1.top().partial_cmp(&b.1.top()).unwrap_or(std::cmp::Ordering::Equal)).map(|(line, _)| Spot { id: line.id, at: 0 })
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
    /// The words selected on the page, lines apart on lines of their own, None when there are none.
    pub(crate) fn selected_text(&self) -> Option<String> {
        let selection = self.selection.borrow();
        let selection = selection.as_ref()?;
        if selection.anchor.id == selection.head.id {
            let range = selection.range_in(selection.anchor.id, selection.text.len(), &HashMap::new())?;
            return selection.text.get(range).map(str::to_owned);
        }
        let lines = self.lines.borrow();
        let order = order_of(&lines.drawing);
        let parts: Vec<&str> = lines
            .drawing
            .iter()
            .filter_map(|line| line.text.get(selection.range_in(line.id, line.text.len(), &order)?))
            .collect();
        (!parts.is_empty()).then(|| parts.join("\n"))
    }

    /// The byte range of `text`, the line `id`, that is selected. A selection whose first line has since changed is
    /// dropped: its range would be of other words.
    pub(super) fn selection_in(&self, id: u64, text: &str) -> Option<Range<usize>> {
        let mut selection = self.selection.borrow_mut();
        let current = selection.as_ref()?;
        if current.anchor.id == id && current.text != text {
            *selection = None;
            return None;
        }
        current.range_in(id, text.len(), &self.lines.borrow().drawn)
    }

    /// Drops the selection when a line it starts or ends in has left the page.
    pub(super) fn keep_selection_in(&self, nodes: &[Node]) {
        self.selection
            .borrow_mut()
            .take_if(|selection| !has_node(nodes, selection.anchor.id) || !has_node(nodes, selection.head.id));
    }

    /// `text`, a plain string of the node `id`, as a line a person can select words of.
    pub(super) fn selectable_string(&self, id: u64, text: String, cx: &mut Context<Self>) -> AnyElement {
        let marks = with_selection(Vec::new(), self.selection_in(id, &text));
        let styled = StyledText::new(text.clone()).with_highlights(marks);
        let layout = styled.layout().clone();
        self.selectable(id, text, layout, styled.into_any_element(), cx)
    }

    /// `element`, the line of `text` that `layout` draws, with a press on it starting a selection. A press without a
    /// drag selects none, so a click on a link or a clickable span is still a click; a double press takes the word. `id`
    /// names the line. A press anywhere on the page first drops what was selected, and the page follows the drag
    /// (`drag_selection`), over this line and the ones after or before it.
    pub(super) fn selectable(
        &self,
        id: u64,
        text: String,
        layout: TextLayout,
        element: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bounds = Rc::new(Cell::new(None));
        self.lines.borrow_mut().drawing.push(Line { id, text: text.clone(), layout: layout.clone(), bounds: bounds.clone() });
        div()
            .relative()
            .child(element)
            // Says where the line was drawn, for a drag that goes past it.
            .child(canvas(move |drawn, _, _| bounds.set(Some(drawn)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    let at = layout.index_for_position(event.position).unwrap_or_else(|edge| edge);
                    let (anchor, head) = if event.click_count == 2 {
                        let word = word_at(&text, at);
                        (word.start, word.end)
                    } else {
                        (at, at)
                    };
                    *view.selection.borrow_mut() =
                        Some(Selection { anchor: Spot { id, at: anchor }, head: Spot { id, at: head }, text: text.clone() });
                    if anchor != head {
                        window.focus(&view.selection_focus, cx);
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
    }

    /// The pointer moved with the left button down: a selection started on a line runs to where it is now.
    pub(super) fn drag_selection(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let head = {
            let selection = self.selection.borrow();
            let Some(anchor) = selection.as_ref().map(|selection| selection.anchor) else {
                return;
            };
            let lines = self.lines.borrow();
            let Some(column) = lines.drawing.iter().find(|line| line.id == anchor.id).and_then(|line| line.bounds.get()) else {
                return;
            };
            spot_at(&lines.drawing, event.position, column)
        };
        let mut selection = self.selection.borrow_mut();
        if let (Some(selection), Some(head)) = (selection.as_mut(), head)
            && selection.head != head
        {
            selection.head = head;
            window.focus(&self.selection_focus, cx);
            cx.notify();
        }
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
