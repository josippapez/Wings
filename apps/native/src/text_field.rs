//! A text field for the find bar, pane names, Settings and the plugin pages' inputs. GPUI has no text input, so
//! this one shapes and paints its own text: a caret and a selection, set by click, drag, double and triple click or the
//! arrow keys (Shift extends, Alt and Cmd move by word and line), Backspace and Delete, copy, cut, paste,
//! select-all and an input method's composing text. It leaves the keys it doesn't use to its owner.

use std::{borrow::Cow, ops::Range};

use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, ContentMask, Context, DispatchPhase, Element,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, GlobalElementId, InspectorElementId, IntoElement, KeyDownEvent, LayoutId, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Role, SharedString,
    Style, TextAlign, TextRun, TextStyle, UTF16Selection, UnderlineStyle, Window, WrappedLine, div,
    fill, point, prelude::*, px, relative, rgb, rgba, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    actions::{self, Paste},
    theme::{FOREGROUND, HOVER, MUTED_FOREGROUND},
};

/// The border of a field that draws its own frame: the desktop's `--input`.
const FRAME: u32 = 0xffffff1a;
/// The desktop terminal's `selectionBackground`.
const SELECTED: u32 = 0xffffff33;

pub enum FieldEvent {
    Changed,
    /// Enter, or Shift-Enter with `back`.
    Submit {
        back: bool,
    },
    Cancel,
}

pub struct TextField {
    focus: FocusHandle,
    role: Role,
    label: SharedString,
    placeholder: SharedString,
    width: Pixels,
    /// Set it freely: the caret and the selection are clamped to whatever it holds when they are read.
    pub text: String,
    /// The lines a multi-line field shows at least. Enter then starts a new line, and a one-line field (`None`) submits.
    pub rows: Option<usize>,
    /// Takes the width of its place, not `width`.
    pub fill: bool,
    /// Dimmed, and takes no typing.
    pub disabled: bool,
    /// The most characters it holds: what is typed or pasted past that is dropped.
    pub max_chars: Option<usize>,
    /// A password: each character shows as a dot, and copy and cut take nothing, as in a browser's
    /// `<input type="password">`.
    pub secret: bool,
    /// A browser's `<input type="time">`: it takes digits, and makes HH:MM of them as they are typed.
    pub time: bool,
    /// Byte offsets into `text`, read through `anchor()` and `head()`. The caret is the head, and the selection runs
    /// from the anchor to it. Both start past the end, so a field with a text set from outside has its caret after it.
    anchor: usize,
    head: usize,
    /// What an input method is composing, shown at the caret and not yet part of `text`.
    marked: String,
    /// How far a one-line text is scrolled left, to keep the caret in view.
    scroll: Pixels,
    /// The mouse went down in the field and is still down: moving it extends the selection.
    dragging: bool,
    /// Where the last paint put the text, which clicks and the input method ask about.
    bounds: Bounds<Pixels>,
    layout: Option<Layout>,
}

impl EventEmitter<FieldEvent> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextField {
    pub fn new(
        role: Role,
        label: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus: cx.focus_handle().tab_stop(true),
            role,
            label: label.into(),
            placeholder: placeholder.into(),
            width,
            text: String::new(),
            rows: None,
            fill: false,
            disabled: false,
            max_chars: None,
            secret: false,
            time: false,
            anchor: usize::MAX,
            head: usize::MAX,
            marked: String::new(),
            scroll: px(0.0),
            dragging: false,
            bounds: Bounds::default(),
            layout: None,
        }
    }

    /// Takes `text` from outside with the caret after it, as a browser's input does when a page sets its value: a
    /// suggestion picked after typing two letters left the caret after its second, where the next key went.
    pub fn set_text(&mut self, text: String) {
        self.text = text;
        (self.anchor, self.head) = (usize::MAX, usize::MAX);
    }

    /// What a page hears the field holds: a time field's text once it is a whole HH:MM, and "" before, as from a
    /// browser, so an hour typed without its minutes isn't taken for a time.
    pub fn value(&self) -> &str {
        if self.time && self.text.len() != 5 {
            ""
        } else {
            &self.text
        }
    }

    /// Where the field's text was last drawn, in the window.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// Focuses the field with its term selected, so typing replaces it, as in the desktop find bar.
    pub fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        self.select(0, self.text.len(), window, cx);
    }

    /// `offset` as a place in the text: the end if it is past it, and the start of a character if it is inside one.
    fn at(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len());
        (0..=offset)
            .rev()
            .find(|&at| self.text.is_char_boundary(at))
            .unwrap_or(0)
    }

    fn anchor(&self) -> usize {
        self.at(self.anchor)
    }

    fn head(&self) -> usize {
        self.at(self.head)
    }

    fn selection(&self) -> Range<usize> {
        let (anchor, head) = (self.anchor(), self.head());
        anchor.min(head)..anchor.max(head)
    }

    /// What the field shows: the text, with the composing text at the caret.
    fn shown(&self) -> Cow<'_, str> {
        if self.marked.is_empty() {
            return Cow::Borrowed(&self.text);
        }
        let head = self.head();
        Cow::Owned(format!(
            "{}{}{}",
            &self.text[..head],
            self.marked,
            &self.text[head..]
        ))
    }

    /// Where the caret is in `shown()`: after the composing text.
    fn caret(&self) -> usize {
        self.head() + self.marked.len()
    }

    fn select(&mut self, anchor: usize, head: usize, window: &mut Window, cx: &mut Context<Self>) {
        (self.anchor, self.head) = (self.at(anchor), self.at(head));
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    /// Moves the caret, and keeps the anchor where it was when `extend`.
    fn go(&mut self, to: usize, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let anchor = if extend { self.anchor() } else { to };
        self.select(anchor, to, window, cx);
    }

    /// Replaces `range` of the text and puts the caret after what replaces it.
    fn replace(
        &mut self,
        range: Range<usize>,
        with: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        // A one-line field takes its text as one line.
        let with = if self.rows.is_none() && with.contains(['\r', '\n']) {
            Cow::Owned(with.replace(['\r', '\n'], " "))
        } else {
            Cow::Borrowed(with)
        };
        let with = match self.max_chars {
            Some(max) => {
                let kept = self.text.chars().count() - self.text[range.clone()].chars().count();
                let room = max.saturating_sub(kept);
                with.char_indices()
                    .nth(room)
                    .map_or(&*with, |(end, _)| &with[..end])
            }
            None => &*with,
        };
        if range.is_empty() && with.is_empty() {
            return;
        }
        self.text.replace_range(range.clone(), with);
        let mut caret = range.start + with.len();
        if self.time {
            // The caret stays after as many digits as were before it, or at the end if it was there.
            let at_end = caret == self.text.len();
            let digits = self.text[..caret].chars().filter(char::is_ascii_digit).count();
            self.text = time_of(&self.text);
            caret = if at_end {
                self.text.len()
            } else {
                self.text
                    .char_indices()
                    .filter(|(_, c)| c.is_ascii_digit())
                    .nth(digits)
                    .map_or(self.text.len(), |(at, _)| at)
            };
        }
        (self.anchor, self.head) = (caret, caret);
        window.notify_text_input_changed(&self.focus);
        cx.emit(FieldEvent::Changed);
        cx.notify();
    }

    /// The composing text becomes text.
    fn commit(&mut self) {
        if !self.marked.is_empty() {
            let head = self.head();
            let marked = std::mem::take(&mut self.marked);
            self.text.insert_str(head, &marked);
            (self.anchor, self.head) = (head + marked.len(), head + marked.len());
        }
    }

    fn copy(&self, cx: &mut Context<Self>) {
        let selection = self.selection();
        if !selection.is_empty() && !self.secret {
            cx.write_to_clipboard(ClipboardItem::new_string(self.text[selection].to_string()));
        }
    }

    fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let pasted = if self.rows.is_some() {
            pasted.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            pasted
        };
        self.replace(self.selection(), &pasted, window, cx);
    }

    /// Paste comes here before the terminal around the field gets it.
    fn on_paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        self.paste(window, cx);
    }

    /// Copy comes here before the terminal around the field gets it, but leaves it the terminal's selection when
    /// nothing in the field is selected.
    fn on_copy(&mut self, _: &actions::Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection().is_empty() {
            cx.propagate();
        } else {
            self.copy(cx);
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let mods = key.modifiers;
        let secondary = if cfg!(target_os = "macos") {
            mods.platform
        } else {
            mods.control
        };
        match key.key.as_str() {
            "escape" => cx.emit(FieldEvent::Cancel),
            "enter" if self.rows.is_some() => {
                self.replace(self.selection(), "\n", window, cx);
            }
            "enter" => cx.emit(FieldEvent::Submit { back: mods.shift }),
            // The input method has the rest while it composes.
            _ if !self.marked.is_empty() => return,
            "a" if secondary => self.select(0, self.text.len(), window, cx),
            "c" if secondary => self.copy(cx),
            "x" if secondary && !self.secret => {
                self.copy(cx);
                self.replace(self.selection(), "", window, cx);
            }
            "x" if secondary => {}
            "v" if secondary => self.paste(window, cx),
            "left" | "right" | "up" | "down" | "home" | "end" => {
                self.travel(key.key.as_str(), mods, window, cx)
            }
            "backspace" => self.erase(false, mods, window, cx),
            "delete" => self.erase(true, mods, window, cx),
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    /// Where Alt or Cmd on macOS, and Ctrl elsewhere, send a move or a delete: by word, or to the line's end.
    fn reach(mods: Modifiers) -> (bool, bool) {
        if cfg!(target_os = "macos") {
            (mods.alt, mods.platform)
        } else {
            (mods.control, false)
        }
    }

    fn travel(&mut self, key: &str, mods: Modifiers, window: &mut Window, cx: &mut Context<Self>) {
        let (word, line) = Self::reach(mods);
        let (head, selection) = (self.head(), self.selection());
        let text = &self.text;
        let to = match key {
            "left" | "right" if !mods.shift && !selection.is_empty() && !word && !line => {
                if key == "left" {
                    selection.start
                } else {
                    selection.end
                }
            }
            "left" if line => line_start(text, head),
            "left" if word => word_before(text, head),
            "left" => before(text, head),
            "right" if line => line_end(text, head),
            "right" if word => word_after(text, head),
            "right" => after(text, head),
            "home" => line_start(text, head),
            "end" => line_end(text, head),
            // Up and down are the start and end of a one-line text, and of a text with Cmd.
            "up" if self.rows.is_none() || line => 0,
            "down" if self.rows.is_none() || line => text.len(),
            _ => self.row_from(head, key == "up"),
        };
        self.go(to, mods.shift, window, cx);
    }

    /// The place a row above or below `from`, in the same column, and the start or end of the text past the first
    /// and last row.
    fn row_from(&self, from: usize, up: bool) -> usize {
        let Some(layout) = &self.layout else {
            return from;
        };
        let at = layout.point(from);
        if up && at.y < layout.line_height {
            0
        } else if !up && at.y + layout.line_height >= layout.height {
            self.text.len()
        } else {
            let middle = at.y + layout.line_height / 2.0;
            let y = if up {
                middle - layout.line_height
            } else {
                middle + layout.line_height
            };
            layout.index_at(point(at.x, y))
        }
    }

    /// Backspace or Delete: the selection, or what the caret would move over.
    fn erase(
        &mut self,
        forward: bool,
        mods: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut range = self.selection();
        if range.is_empty() {
            let (word, line) = Self::reach(mods);
            let (head, text) = (range.start, &self.text);
            range = if forward {
                let to = match () {
                    () if line => line_end(text, head),
                    () if word => word_after(text, head),
                    () => after(text, head),
                };
                head..to
            } else {
                let to = match () {
                    () if line => line_start(text, head),
                    () if word => word_before(text, head),
                    () => before(text, head),
                };
                to..head
            };
        }
        self.replace(range, "", window, cx);
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = &self.layout else {
            return;
        };
        let clicked = layout.index_at(event.position - layout.origin);
        // What was being composed is text now, and the clicked place counts in it.
        self.commit();
        let at = self.at(clicked);
        match event.click_count {
            1 => self.go(at, event.modifiers.shift, window, cx),
            2 => {
                let word = word_at(&self.text, at);
                self.select(word.start, word.end, window, cx);
            }
            _ => {
                let (start, end) = (line_start(&self.text, at), line_end(&self.text, at));
                self.select(start, end, window, cx);
            }
        }
        self.dragging = true;
    }

    fn drag_to(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = &self.layout else {
            return;
        };
        let at = self.at(layout.index_at(position - layout.origin));
        self.go(at, true, window, cx);
    }
}

impl Render for TextField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(ElementId::NamedInteger(
                "text-field".into(),
                cx.entity_id().as_u64(),
            ))
            .role(self.role)
            .aria_label(self.label.clone())
            .aria_placeholder(self.placeholder.clone())
            .aria_value(if self.secret {
                MASK.repeat(self.text.chars().count())
            } else {
                self.text.clone()
            })
            .track_focus(&self.focus)
            .key_context("FindField")
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .relative()
            .when(self.fill, |field| {
                // A form input draws its own frame, shadcn's `Input`, so focus shows on it.
                field
                    .w_full()
                    .min_w_0()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(rgba(FRAME))
                    .bg(rgba(HOVER))
                    .focus(|style| style.border_color(rgb(FOREGROUND)))
            })
            .when(!self.fill, |field| field.w(self.width))
            .px(px(8.0))
            .flex()
            .overflow_hidden()
            .text_size(px(13.0))
            .cursor_text()
            .opacity(if self.disabled { 0.5 } else { 1.0 })
            .when_some(self.rows, |field, rows| {
                field
                    .min_h(px(rows as f32 * 19.0 + 14.0))
                    .py(px(7.0))
                    .flex_col()
            })
            .when(self.rows.is_none(), |field| {
                field
                    .h(px(if self.fill { 32.0 } else { 28.0 }))
                    .items_center()
            })
            .child(FieldText(cx.entity()))
    }
}

/// The text, its selection and its caret: the part of the field that is drawn and hit.
struct FieldText(Entity<TextField>);

impl TextField {
    /// The shown text, or the placeholder when there is none, with the selection or the composing text marked.
    fn shape(
        &self,
        style: &TextStyle,
        wrap: Option<Pixels>,
        focused: bool,
        window: &Window,
    ) -> Layout {
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.pixel_snap(
            style
                .line_height
                .to_pixels(font_size.into(), window.rem_size()),
        );
        let doc: SharedString = self.shown().into_owned().into();
        let placeholder = doc.is_empty();
        let secret = self.secret && !placeholder;
        let text = if placeholder {
            self.placeholder.clone()
        } else if secret {
            MASK.repeat(doc.chars().count()).into()
        } else {
            doc.clone()
        };
        let color = rgb(if placeholder {
            MUTED_FOREGROUND
        } else {
            FOREGROUND
        });
        let (range, selecting) = if !self.marked.is_empty() {
            (self.head()..self.head() + self.marked.len(), false)
        } else if focused && !placeholder {
            (self.selection(), true)
        } else {
            (0..0, false)
        };
        let range = if secret {
            masked(&doc, range.start)..masked(&doc, range.end)
        } else {
            range
        };
        let plain = |len| TextRun {
            color: color.into(),
            ..style.to_run(len)
        };
        let special = TextRun {
            background_color: selecting.then(|| rgba(SELECTED).into()),
            underline: (!selecting && !range.is_empty()).then(|| UnderlineStyle {
                color: Some(color.into()),
                thickness: px(1.0),
                wavy: false,
            }),
            ..plain(range.len())
        };
        let runs: Vec<_> = [plain(range.start), special, plain(text.len() - range.end)]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect();
        let lines = window
            .text_system()
            .shape_text(text, font_size, &runs, wrap, None)
            .unwrap_or_default();
        let mut layout = Layout::new(doc, lines.into_vec(), font_size, line_height);
        layout.secret = secret;
        layout
    }
}

/// The digits of `typed` as much of an HH:MM as they make, filled as a browser's time field fills its hours and then its
/// minutes: a first digit of the hour above 2, or of the minutes above 5, gets a 0 before it, a digit no time can
/// have there is dropped, and the colon comes with the first minute digit.
fn time_of(typed: &str) -> String {
    let mut digits = String::new();
    for c in typed.chars().filter(char::is_ascii_digit) {
        match digits.len() {
            0 if c > '2' => digits.extend(['0', c]),
            2 if c > '5' => digits.extend(['0', c]),
            1 if digits == "2" && c > '3' => {}
            0..=3 => digits.push(c),
            _ => break,
        }
    }
    if digits.len() <= 2 {
        digits
    } else {
        format!("{}:{}", &digits[..2], &digits[2..])
    }
}

/// What a password shows for each character.
const MASK: &str = "•";

/// Where offset `at` of `doc` falls in its masked text, one `MASK` a character.
fn masked(doc: &str, at: usize) -> usize {
    doc[..at.min(doc.len())].chars().count() * MASK.len()
}

/// The offset of `doc` that offset `at` of its masked text falls at.
fn unmasked(doc: &str, at: usize) -> usize {
    doc.char_indices()
        .nth(at / MASK.len())
        .map_or(doc.len(), |(at, _)| at)
}

struct Layout {
    /// The text this was made from, without the placeholder.
    doc: SharedString,
    lines: Vec<WrappedLine>,
    /// Where each line starts in the text, and its top.
    starts: Vec<usize>,
    tops: Vec<Pixels>,
    height: Pixels,
    font_size: Pixels,
    line_height: Pixels,
    /// The window's place of the text's top left corner, scrolled.
    origin: Point<Pixels>,
    /// The lines are of the text masked (`TextField::secret`), and an offset into `doc` is mapped to them and back.
    secret: bool,
}

impl Layout {
    fn new(
        doc: SharedString,
        lines: Vec<WrappedLine>,
        font_size: Pixels,
        line_height: Pixels,
    ) -> Self {
        let (mut starts, mut tops) = (Vec::new(), Vec::new());
        let (mut start, mut top) = (0, px(0.0));
        for line in &lines {
            starts.push(start);
            tops.push(top);
            start += line.len() + 1;
            top += line_height * (line.wrap_boundaries.len() + 1);
        }
        Self {
            doc,
            lines,
            starts,
            tops,
            height: top,
            font_size,
            line_height,
            origin: Point::default(),
            secret: false,
        }
    }

    fn width(&self) -> Pixels {
        self.lines
            .iter()
            .map(|line| line.unwrapped_layout.width)
            .fold(px(0.0), Pixels::max)
    }

    fn line_of(&self, index: usize) -> usize {
        self.starts
            .partition_point(|&start| start <= index)
            .saturating_sub(1)
    }

    /// Where `index` is, from the text's top left corner.
    fn point(&self, index: usize) -> Point<Pixels> {
        let index = if self.secret {
            masked(&self.doc, index)
        } else {
            index
        };
        let i = self.line_of(index);
        let Some(line) = self.lines.get(i) else {
            return Point::default();
        };
        let at = line
            .position_for_index(index - self.starts[i], self.line_height)
            .unwrap_or_default();
        point(at.x, at.y + self.tops[i])
    }

    /// The place between two characters nearest to `at`, from the text's top left corner.
    fn index_at(&self, at: Point<Pixels>) -> usize {
        let index = self.line_index_at(at);
        if self.secret {
            unmasked(&self.doc, index)
        } else {
            index
        }
    }

    /// `index_at`, as an offset into the lines.
    fn line_index_at(&self, at: Point<Pixels>) -> usize {
        let i = self
            .tops
            .partition_point(|&top| top <= at.y)
            .saturating_sub(1);
        let Some(line) = self.lines.get(i) else {
            return 0;
        };
        let rows = line.wrap_boundaries.len() + 1;
        let row = (((at.y - self.tops[i]).max(px(0.0)) / self.line_height) as usize).min(rows - 1);
        let row_start = |row: usize| match row {
            0 => 0,
            row => {
                let wrap = line.wrap_boundaries[row - 1];
                line.unwrapped_layout.runs[wrap.run_ix].glyphs[wrap.glyph_ix].index
            }
        };
        let (start, end) = (
            row_start(row),
            if row + 1 == rows {
                line.len()
            } else {
                row_start(row + 1)
            },
        );
        let x = |index: usize| line.unwrapped_layout.x_for_index(index);
        let target = at.x + x(start);
        let (mut nearest, mut gap) = (start, (x(start) - target).abs());
        for (offset, grapheme) in line.text[start..end].grapheme_indices(true) {
            let index = start + offset + grapheme.len();
            // The end of a wrapped row is where the next one starts.
            if index == end && row + 1 < rows {
                break;
            }
            let away = (x(index) - target).abs();
            if away < gap {
                (nearest, gap) = (index, away);
            }
        }
        self.starts[i] + nearest
    }
}

impl IntoElement for FieldText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FieldText {
    type RequestLayoutState = ();
    type PrepaintState = Option<Layout>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        let field = self.0.clone();
        // The text's style is the field's here, and not where the measuring is done.
        let text_style = window.text_style();
        let id = window.request_measured_layout(style, move |known, available, window, cx| {
            let field = field.read(cx);
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let wrap = width.filter(|_| field.rows.is_some());
            let layout = field.shape(&text_style, wrap, false, window);
            size(width.unwrap_or_else(|| layout.width()), layout.height)
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let field = self.0.read(cx);
        let one_line = field.rows.is_none();
        let wrap = (!one_line).then_some(bounds.size.width);
        let style = window.text_style();
        let mut layout = field.shape(&style, wrap, field.focus.is_focused(window), window);
        let mut scroll = px(0.0);
        if one_line {
            // Scrolls as little as keeps the caret in view, and no further than the text's end.
            let caret = layout.point(field.caret()).x;
            let furthest = (layout.width() + px(1.0) - bounds.size.width).max(px(0.0));
            scroll = field
                .scroll
                .max(px(0.0))
                .min(furthest)
                .min(caret)
                .max(caret + px(1.0) - bounds.size.width);
        }
        layout.origin = bounds.origin - point(scroll, px(0.0));
        self.0.update(cx, |field, _| field.scroll = scroll);
        Some(layout)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        layout: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(layout) = layout.take() else {
            return;
        };
        let field = self.0.read(cx);
        let focus = field.focus.clone();
        let caret = (focus.is_focused(window) && !field.disabled && field.anchor() == field.head())
            .then(|| layout.point(field.caret()));
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.0.clone()), cx);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (line, top) in layout.lines.iter().zip(&layout.tops) {
                let origin = layout.origin + point(px(0.0), *top);
                line.paint_background(
                    origin,
                    layout.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
                line.paint(
                    origin,
                    layout.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
            }
            if let Some(at) = caret {
                let height = layout.font_size + px(3.0);
                let top = (layout.line_height - height) / 2.0;
                window.paint_quad(fill(
                    Bounds::new(
                        layout.origin + at + point(px(0.0), top),
                        size(px(1.0), height),
                    ),
                    rgb(FOREGROUND),
                ));
            }
        });
        // A drag goes on past the field's edge, so these listen to the whole window.
        let field = self.0.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Capture {
                field.update(cx, |field, cx| {
                    if event.pressed_button != Some(MouseButton::Left) {
                        field.dragging = false;
                    } else if field.dragging {
                        field.drag_to(event.position, window, cx);
                    }
                });
            }
        });
        let field = self.0.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                field.update(cx, |field, _| field.dragging = false);
            }
        });
        self.0.update(cx, |field, _| {
            field.bounds = bounds;
            field.layout = Some(layout);
        });
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let shown = self.shown();
        let start = from_utf16(&shown, range.start);
        let end = from_utf16(&shown, range.end).max(start);
        *adjusted = Some(to_utf16(&shown, start)..to_utf16(&shown, end));
        Some(shown[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let shown = self.shown();
        let (range, reversed) = if self.marked.is_empty() {
            (self.selection(), self.anchor() > self.head())
        } else {
            (self.caret()..self.caret(), false)
        };
        Some(UTF16Selection {
            range: to_utf16(&shown, range.start)..to_utf16(&shown, range.end),
            reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Where the input method puts the caret inside its composing text isn't followed.
        if self.marked.is_empty() {
            let start = from_utf16(&self.text, range.start);
            let end = from_utf16(&self.text, range.end);
            self.select(start, end, window, cx);
        }
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.shown().encode_utf16().count())
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let start = to_utf16(&self.text, self.head());
        (!self.marked.is_empty()).then(|| start..start + self.marked.encode_utf16().count())
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let marked = std::mem::take(&mut self.marked);
        self.replace(self.selection(), &marked, window, cx);
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.shown_range(range);
        self.commit();
        self.replace(range, text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = self.shown_range(range);
        self.commit();
        // The text the composition replaces goes now, and the composing text takes its place.
        let removed = !range.is_empty();
        self.text.replace_range(range.clone(), "");
        (self.anchor, self.head) = (range.start, range.start);
        self.marked = text.to_string();
        window.notify_text_input_changed(&self.focus);
        if removed {
            cx.emit(FieldEvent::Changed);
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let shown = self.shown();
        let layout = self.layout.as_ref().filter(|layout| layout.doc == *shown)?;
        let start = from_utf16(&shown, range.start);
        let end = from_utf16(&shown, range.end).max(start);
        let (from, to) = (layout.point(start), layout.point(end));
        Some(Bounds::from_corners(
            layout.origin + point(from.x.min(to.x), from.y),
            layout.origin + point(from.x.max(to.x), to.y + layout.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let shown = self.shown();
        let layout = self.layout.as_ref().filter(|layout| layout.doc == *shown)?;
        if !self.bounds.contains(&position) {
            return None;
        }
        let index = layout.index_at(position - layout.origin).min(shown.len());
        Some(to_utf16(&shown, index))
    }
}

impl TextField {
    /// An input method's range of the shown text, in bytes: the one it gave, else what it is composing, else the
    /// selection.
    fn shown_range(&self, utf16: Option<Range<usize>>) -> Range<usize> {
        match utf16 {
            Some(range) => {
                let shown = self.shown();
                let start = from_utf16(&shown, range.start);
                start..from_utf16(&shown, range.end).max(start)
            }
            None if !self.marked.is_empty() => self.head()..self.head() + self.marked.len(),
            None => self.selection(),
        }
    }
}

/// The byte offset of a UTF-16 one, or the end of the text past it.
fn from_utf16(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (at, char) in text.char_indices() {
        if units >= offset {
            return at;
        }
        units += char.len_utf16();
    }
    text.len()
}

fn to_utf16(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].encode_utf16().count()
}

/// The start of the character before `at`: one grapheme, so an emoji or an accented letter goes whole.
fn before(text: &str, at: usize) -> usize {
    text.grapheme_indices(true)
        .rev()
        .map(|(start, _)| start)
        .find(|&start| start < at)
        .unwrap_or(0)
}

fn after(text: &str, at: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(start, grapheme)| start + grapheme.len())
        .find(|&end| end > at)
        .unwrap_or(text.len())
}

fn word_before(text: &str, at: usize) -> usize {
    text.unicode_word_indices()
        .map(|(start, _)| start)
        .take_while(|&start| start < at)
        .last()
        .unwrap_or(0)
}

fn word_after(text: &str, at: usize) -> usize {
    text.unicode_word_indices()
        .map(|(start, word)| start + word.len())
        .find(|&end| end > at)
        .unwrap_or(text.len())
}

/// The word, or the spaces or the punctuation, around `at`.
fn word_at(text: &str, at: usize) -> Range<usize> {
    let mut last = 0..0;
    for (start, part) in text.split_word_bound_indices() {
        last = start..start + part.len();
        if at < last.end {
            break;
        }
    }
    last
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |newline| newline + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..]
        .find('\n')
        .map_or(text.len(), |newline| at + newline)
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use gpui::{Keystroke, TestApp, TestAppWindow};

    use super::*;

    /// A window with a field at its top left corner, whose text is drawn from x 8.
    struct Host(Entity<TextField>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.0.clone())
        }
    }

    fn open(text: &str) -> (TestApp, TestAppWindow<Host>, Entity<TextField>) {
        let mut app = TestApp::new();
        let mut field = None;
        let mut window = app.open_window(|_, cx| {
            let entity = cx.new(|cx| {
                let mut field = TextField::new(Role::TextInput, "Name", "Name", px(240.0), cx);
                field.text = text.to_string();
                field
            });
            field = Some(entity.clone());
            Host(entity)
        });
        let field = field.unwrap();
        window.update(|_, window, cx| {
            let focus = field.read(cx).focus.clone();
            focus.focus(window, cx)
        });
        window.draw();
        (app, window, field)
    }

    /// The window's place of the gap before the character at `index`, on the field's line.
    fn gap(window: &TestAppWindow<Host>, field: &Entity<TextField>, index: usize) -> Point<Pixels> {
        window.read(|_, cx| {
            let field = field.read(cx);
            let layout = field.layout.as_ref().expect("the field was drawn");
            let at = layout.origin + layout.point(index);
            point(at.x, field.bounds.center().y)
        })
    }

    fn press(window: &mut TestAppWindow<Host>, keys: &'static str) {
        for key in keys.split(' ') {
            let keystroke = Keystroke::parse(key).unwrap();
            window.simulate_event(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            window.simulate_event(gpui::KeyUpEvent { keystroke });
        }
    }

    fn text(window: &TestAppWindow<Host>, field: &Entity<TextField>) -> String {
        window.read(|_, cx| field.read(cx).text.clone())
    }

    fn selected(window: &TestAppWindow<Host>, field: &Entity<TextField>) -> Range<usize> {
        window.read(|_, cx| field.read(cx).selection())
    }

    #[test]
    fn a_click_between_letters_puts_the_caret_there_and_typing_inserts_at_it() {
        let (_app, mut window, field) = open("flaoting");
        window.simulate_click(gap(&window, &field, 3), MouseButton::Left);
        assert_eq!(selected(&window, &field), 3..3);
        window.simulate_input("t");
        window.draw();
        assert_eq!(text(&window, &field), "flatoting");
        assert_eq!(selected(&window, &field), 4..4);
    }

    #[test]
    fn shift_arrows_select_and_typing_replaces_the_selection() {
        let (_app, mut window, field) = open("hello world");
        window.simulate_click(gap(&window, &field, 5), MouseButton::Left);
        press(
            &mut window,
            "shift-right shift-right shift-right shift-right shift-right shift-right",
        );
        assert_eq!(selected(&window, &field), 5..11);
        window.simulate_input("!");
        window.draw();
        assert_eq!(text(&window, &field), "hello!");
        assert_eq!(selected(&window, &field), 6..6);
    }

    #[test]
    fn a_double_click_selects_the_word_and_backspace_deletes_the_selection() {
        let (_app, mut window, field) = open("one two three");
        let inside = gap(&window, &field, 5);
        for click_count in [1, 2] {
            window.simulate_event(MouseDownEvent {
                position: inside,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count,
                first_mouse: false,
            });
        }
        assert_eq!(selected(&window, &field), 4..7);
        press(&mut window, "backspace");
        assert_eq!(text(&window, &field), "one  three");
        assert_eq!(selected(&window, &field), 4..4);
        // With nothing selected, Backspace takes one character.
        press(&mut window, "backspace");
        assert_eq!(text(&window, &field), "one three");
    }

    /// Cyclops' start and end: typed as digits, with no colon, as in a browser's `<input type="time">`.
    #[test]
    fn a_time_takes_digits_and_puts_the_colon_in() {
        let (_app, mut window, field) = open("");
        window.update(|_, _, cx| field.update(cx, |field, _| field.time = true));
        let value = |window: &TestAppWindow<Host>| window.read(|_, cx| field.read(cx).value().to_string());
        for (typed, shown) in [("0", "0"), ("9", "09"), ("3", "09:3"), ("0", "09:30"), ("5", "09:30")] {
            window.simulate_input(typed);
            assert_eq!(text(&window, &field), shown);
        }
        assert_eq!(value(&window), "09:30");
        press(&mut window, "backspace");
        assert_eq!(text(&window, &field), "09:3");
        assert_eq!(value(&window), "", "an unfinished time is no time");
        press(&mut window, "backspace backspace");
        assert_eq!(text(&window, &field), "0");
        // A 9 can only be 09, a 24th hour isn't one, and a colon or a letter is let go.
        press(&mut window, "cmd-a backspace");
        for typed in ["9", ":", "a", "7"] {
            window.simulate_input(typed);
        }
        assert_eq!(text(&window, &field), "09:07");
        press(&mut window, "cmd-a backspace");
        for typed in ["2", "4", "3", "1", "5"] {
            window.simulate_input(typed);
        }
        assert_eq!(text(&window, &field), "23:15");
        // A pasted 9:45 is 09:45.
        window.update(|_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string("9:45".into())));
        press(&mut window, "cmd-a cmd-v");
        assert_eq!(text(&window, &field), "09:45");
    }

    /// Cyclops' API token: a browser's `<input type="password">` shows a dot a character, and copy and cut take nothing.
    #[test]
    fn a_password_shows_dots_edits_by_character_and_is_never_copied() {
        let (_app, mut window, field) = open("cyc_é1");
        window.update(|_, _, cx| field.update(cx, |field, _| field.secret = true));
        window.draw();
        window.read(|_, cx| {
            let layout = field.read(cx).layout.as_ref().unwrap();
            let shown: String = layout
                .lines
                .iter()
                .map(|line| line.text.to_string())
                .collect();
            assert_eq!(shown, "••••••");
        });
        // A click between the third and fourth dots is between the third and fourth characters.
        window.simulate_click(gap(&window, &field, 3), MouseButton::Left);
        assert_eq!(selected(&window, &field), 3..3);
        window.simulate_input("x");
        window.draw();
        assert_eq!(text(&window, &field), "cycx_é1");
        // Past the two-byte é, the caret moves a character at a time.
        press(&mut window, "end left");
        assert_eq!(selected(&window, &field), 7..7);
        window.update(|_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
        press(&mut window, "cmd-a cmd-c cmd-x");
        assert_eq!(text(&window, &field), "cycx_é1", "cut leaves the text");
        let clipboard =
            window.update(|_, _, cx| cx.read_from_clipboard().and_then(|item| item.text()));
        assert_eq!(clipboard.as_deref(), Some("before"));
    }
}
