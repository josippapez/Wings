//! What a frame cuts off, for the screenshot examples (`examples/shot.rs`, `examples/shot_plugins.rs`): text, a button or
//! a box that runs past the edge of the window, the page or a box that clips it, text that runs past its card or its own
//! box, and text squeezed to a few letters a line. Pages cut off at 640 points wide shipped though their screenshots
//! showed it, because nobody saw it in the PNG.
//!
//! Built only with `test-support`, and nothing is noted until `check` asks, so the app, the tests and the benchmarks
//! draw as they did. `check` draws the window again from scratch, so every piece paints, and while it does:
//! - each line of a page's text notes its words, how they were laid out, its box and the clip it was painted under
//!   (`note_text`, from the page's selectable lines), each button its name, box and clip (`named`), and a page
//!   button's label of plain words where it is drawn (`label`);
//! - each card notes its edges and, by the order pieces paint in, which pieces it holds (`card`);
//! - each box that clips on purpose notes its clip (`clips_on_purpose`): one that scrolls sideways, whose content is
//!   scrolled to, or the sidebar as it slides shut.
//!
//! Then it reads every box the frame painted, a background or a border, each with the clip it was painted under
//! (`Window::painted_quads`). A piece is cut off when it is in view top to bottom (not scrolled out of a list or below the
//! fold) but runs past its clip's left or right edge. Text cut short with an ellipsis (`truncate`) is laid out to fit its
//! box, so it never counts, nor does anything in a box that clips on purpose.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
    sync::Mutex,
};

use gpui::{AnyElement, App, Bounds, Pixels, ScaledPixels, Size, TextLayout, Window, canvas, prelude::*};

/// How far past an edge a piece may run before it counts: a border's width, under which a box clipped inside its
/// border keeps its edge.
const SLACK: f32 = 1.5;

/// The widest a box's edge is painted: GPUI paints a border as a piece of its own this wide.
const EDGE: f32 = 3.0;

/// A row broken inside a word that holds this many letters or fewer is squeezed: Issues' settings shipped with its name
/// a letter a line at 640 points.
const SQUEEZED: usize = 3;

/// The longest a name is quoted.
const NAME: usize = 60;

thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static NOTED: RefCell<Noted> = RefCell::default();
}

/// What a screenshot run's frames cut off, said again at its end (`end_run`).
static RUN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Whether a check is drawing the frame, so the pieces it reads note themselves.
pub fn watching() -> bool {
    WATCHING.get()
}

/// Something a frame cut off.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    /// The window's width, in points.
    pub width: f32,
    /// What is cut off: a text's words, a button's name, or a box and the words in it.
    pub what: String,
    /// How: "runs 38 pt past the window's right edge".
    pub how: String,
}

impl std::fmt::Display for Cut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.what, self.how)
    }
}

/// Draws `window` again from scratch while its pieces note themselves, and says what that frame cuts off.
pub fn check(window: &mut Window, cx: &mut App) -> Vec<Cut> {
    NOTED.set(Noted::default());
    WATCHING.set(true);
    window.refresh();
    window.draw(cx).clear(cx);
    WATCHING.set(false);
    NOTED.take().cuts(&window.painted_quads(), window.scale_factor(), window.viewport_size())
}

/// Prints what the screenshot `path` cut off, a line each, and keeps the lines for `end_run`.
pub fn report(path: &Path, cuts: &[Cut]) {
    let mut run = RUN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    for cut in cuts {
        let line = format!("CUT OFF at {:.0} in {}: {cut}", cut.width, path.display());
        println!("{line}");
        run.push(line);
    }
}

/// The end of a screenshot run: says again all that its screenshots cut off and exits with 1 when anything was, so a
/// page cut off at 640 points can't pass unseen. Says so when nothing was.
pub fn end_run() {
    let run = std::mem::take(&mut *RUN.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    if run.is_empty() {
        println!("Nothing cut off.");
        return;
    }
    println!("\n{} cut off in these screenshots (open each PNG at the place named, see `ui::cutoff`):", run.len());
    for line in &run {
        println!("{line}");
    }
    std::process::exit(1);
}

/// A line of a page's text, `words`, laid out as `layout` in the box `bounds`, as a check's frame paints it.
pub(crate) fn note_text(words: &str, layout: &TextLayout, bounds: Bounds<Pixels>, window: &Window) {
    let clip = Rect::of(window.content_mask().bounds);
    let own = Rect::of(bounds);
    let (widest, squeezed) = measure(layout);
    let drawn = Rect { right: own.left + widest, ..own };
    note(|noted| {
        noted.piece(Piece { name: Some(words.to_owned()), drawn, clip, own: Some(own), squeezed })
    });
}

/// In a check's frame, a canvas over its parent, a button, that notes it as the piece `name`. None otherwise.
pub(crate) fn named(name: &str) -> Option<AnyElement> {
    let name = watching().then(|| name.to_owned())?;
    let note_it = move |bounds: Bounds<Pixels>, _: (), window: &mut Window, _: &mut App| {
        let clip = Rect::of(window.content_mask().bounds);
        note(|noted| noted.piece(Piece { name: Some(name), drawn: Rect::of(bounds), clip, own: None, squeezed: None }));
    };
    Some(canvas(|_, _, _| {}, note_it).absolute().top_0().left_0().size_full().into_any_element())
}

/// In a check's frame, a canvas over a button whose content is one line of words, centred, which `words` says: it notes
/// the words where they are drawn, wider than the button when it is squeezed. None otherwise, and `words` isn't asked.
pub(crate) fn label(words: impl FnOnce() -> Option<String>) -> Option<AnyElement> {
    let words = watching().then(words).flatten().filter(|words| !words.is_empty() && !words.contains('\n'))?;
    let note_it = move |bounds: Bounds<Pixels>, _: (), window: &mut Window, _: &mut App| {
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let run = style.to_run(words.len());
        let wide = window.text_system().shape_line(words.clone().into(), size, &[run], None).width.as_f32();
        let own = Rect::of(bounds);
        let middle = (own.left + own.right) / 2.0;
        let drawn = Rect { left: middle - wide / 2.0, right: middle + wide / 2.0, ..own };
        let clip = Rect::of(window.content_mask().bounds);
        note(|noted| noted.piece(Piece { name: Some(words), drawn, clip, own: Some(own), squeezed: None }));
    };
    Some(canvas(|_, _, _| {}, note_it).absolute().top_0().left_0().size_full().into_any_element())
}

/// In a check's frame, a canvas over a box whose content runs past its edges on purpose, noting its clip: one that
/// scrolls sideways, or the sidebar clipped as it slides shut. None otherwise.
pub(crate) fn clips_on_purpose() -> Option<AnyElement> {
    watching().then(|| {
        canvas(|_, _, _| {}, |_, _, window, _| {
            let clip = Rect::of(window.content_mask().bounds);
            note(|noted| noted.on_purpose.push(clip));
        })
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    })
}

/// In a check's frame, the first and last children of a card: the first notes its edges as it starts to paint, the
/// last that it has painted all it holds. None otherwise.
pub(crate) fn card() -> (Option<AnyElement>, Option<AnyElement>) {
    if !watching() {
        return (None, None);
    }
    let at = Rc::new(Cell::new(None));
    let opened = at.clone();
    let opens = canvas(|_, _, _| {}, move |bounds, _, _, _| opened.set(Some(note(|noted| noted.open_card(bounds)))));
    let closes = canvas(|_, _, _| {}, move |_, _, _, _| {
        if let Some(card) = at.get() {
            note(|noted| noted.close_card(card));
        }
    });
    (
        Some(opens.absolute().top_0().left_0().size_full().into_any_element()),
        Some(closes.absolute().top_0().left_0().size_0().into_any_element()),
    )
}

fn note<R>(f: impl FnOnce(&mut Noted) -> R) -> R {
    NOTED.with_borrow_mut(f)
}

/// The widest row `layout` was drawn in, and the fewest letters a row broken inside a word holds, if one is.
fn measure(layout: &TextLayout) -> (f32, Option<usize>) {
    let text = layout.text();
    let (mut widest, mut fewest) = (0.0f32, None::<usize>);
    for (line, shaped) in text.split('\n').zip(layout.line_layouts()) {
        let whole = &shaped.unwrapped_layout;
        // Where each row after the first starts: its first glyph's byte and place along the unwrapped line.
        let starts: Vec<(usize, f32)> = shaped
            .wrap_boundaries
            .iter()
            .filter_map(|boundary| {
                let glyph = whole.runs.get(boundary.run_ix)?.glyphs.get(boundary.glyph_ix)?;
                Some((glyph.index, glyph.position.x.as_f32()))
            })
            .collect();
        let (mut from, mut from_x) = (0usize, 0.0f32);
        for &(at, x) in &starts {
            widest = widest.max(x - from_x);
            let inside_word = line.get(..at).and_then(|row| row.chars().last()).is_some_and(char::is_alphanumeric)
                && line.get(at..).and_then(|rest| rest.chars().next()).is_some_and(char::is_alphanumeric);
            if inside_word {
                let letters = line.get(from..at).map_or(0, |row| row.trim().chars().count());
                fewest = Some(fewest.map_or(letters, |fewest| fewest.min(letters)));
            }
            (from, from_x) = (at, x);
        }
        widest = widest.max(whole.width.as_f32() - from_x);
    }
    (widest, fewest)
}

/// A rectangle in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Rect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Rect {
    fn of(bounds: Bounds<Pixels>) -> Self {
        let (origin, size) = (bounds.origin, bounds.size);
        let (left, top) = (origin.x.as_f32(), origin.y.as_f32());
        Rect { left, top, right: left + size.width.as_f32(), bottom: top + size.height.as_f32() }
    }

    fn scaled(bounds: Bounds<ScaledPixels>, scale: f32) -> Self {
        let (left, top) = (bounds.origin.x.0 / scale, bounds.origin.y.0 / scale);
        Rect { left, top, right: left + bounds.size.width.0 / scale, bottom: top + bounds.size.height.0 / scale }
    }

    fn width(&self) -> f32 {
        self.right - self.left
    }

    fn height(&self) -> f32 {
        self.bottom - self.top
    }

    /// Inside `outer`, give or take the slack.
    fn within(&self, outer: &Rect) -> bool {
        self.left >= outer.left - SLACK
            && self.top >= outer.top - SLACK
            && self.right <= outer.right + SLACK
            && self.bottom <= outer.bottom + SLACK
    }

    fn overlaps(&self, other: &Rect) -> bool {
        self.left < other.right && other.left < self.right && self.top < other.bottom && other.top < self.bottom
    }
}

/// A piece of text or a named box, as a check's frame painted it.
struct Piece {
    /// Its words or name.
    name: Option<String>,
    /// Where it was drawn: a text's glyphs, a box's edges.
    drawn: Rect,
    /// The clip it was painted under.
    clip: Rect,
    /// A text's own box.
    own: Option<Rect>,
    /// The fewest letters a row of a text broken inside a word holds.
    squeezed: Option<usize>,
}

/// A card: its edges and the pieces it holds, from the `from`th noted to the one before `to`.
struct Card {
    edges: Rect,
    from: usize,
    to: Option<usize>,
}

#[derive(Default)]
struct Noted {
    pieces: Vec<Piece>,
    cards: Vec<Card>,
    on_purpose: Vec<Rect>,
}

/// Which way a piece runs past its edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Side {
    Left,
    Right,
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

/// A piece or a painted box that runs past its clip's edge `edge` on `side`, by `by` points.
struct Over {
    side: Side,
    edge: f32,
    by: f32,
    drawn: Rect,
    clip: Rect,
    piece: Option<usize>,
}

impl Noted {
    fn piece(&mut self, piece: Piece) {
        self.pieces.push(piece);
    }

    fn open_card(&mut self, bounds: Bounds<Pixels>) -> usize {
        self.cards.push(Card { edges: Rect::of(bounds), from: self.pieces.len(), to: None });
        self.cards.len() - 1
    }

    fn close_card(&mut self, card: usize) {
        let to = self.pieces.len();
        if let Some(card) = self.cards.get_mut(card) {
            card.to = Some(to);
        }
    }

    /// Whether `clip` is that of a box that clips on purpose, or inside one.
    fn scrolls(&self, clip: &Rect) -> bool {
        self.on_purpose.iter().any(|outer| clip.within(outer))
    }

    /// How a piece drawn at `drawn` under `clip` runs past the clip's edges, unless it is out of view top to bottom or
    /// in a box that clips on purpose. A clip with no room is a box clipped away whole, a column of a board scrolled
    /// out of view: nothing in it shows.
    fn overs(&self, drawn: Rect, clip: Rect, piece: Option<usize>) -> Vec<Over> {
        let shows = clip.width() > 0.0 && clip.height() > 0.0 && drawn.width() > 0.0 && drawn.height() > 0.0;
        let in_view = drawn.top < clip.bottom && clip.top < drawn.bottom;
        if !shows || !in_view || self.scrolls(&clip) {
            return Vec::new();
        }
        [(Side::Right, clip.right, drawn.right - clip.right), (Side::Left, clip.left, clip.left - drawn.left)]
            .into_iter()
            .filter(|(_, _, by)| *by > SLACK)
            .map(|(side, edge, by)| Over { side, edge, by, drawn, clip, piece })
            .collect()
    }

    fn cuts(&self, quads: &[gpui::Quad], scale: f32, viewport: Size<Pixels>) -> Vec<Cut> {
        let width = viewport.width.as_f32();
        let mut overs: Vec<Over> = self
            .pieces
            .iter()
            .enumerate()
            .flat_map(|(at, piece)| self.overs(piece.drawn, piece.clip, Some(at)))
            .collect();
        overs.extend(painted_boxes(quads, scale).into_iter().flat_map(|(drawn, clip)| self.overs(drawn, clip, None)));
        let mut cuts = Vec::new();
        let mut reported = HashSet::new();
        for group in groups(overs) {
            reported.extend(group.iter().filter_map(|over| over.piece));
            cuts.push(self.cut_of(&group, width));
        }
        // Text and buttons that run past their card, in the page.
        for (at, piece) in self.pieces.iter().enumerate() {
            if reported.contains(&at) || self.scrolls(&piece.clip) {
                continue;
            }
            let Some(card) = self.cards.iter().rev().find(|card| card.from <= at && card.to.is_none_or(|to| at < to)) else {
                continue;
            };
            let past = [(Side::Right, piece.drawn.right - card.edges.right), (Side::Left, card.edges.left - piece.drawn.left)]
                .into_iter()
                .find(|(_, by)| *by > SLACK);
            if let Some((side, by)) = past {
                reported.insert(at);
                cuts.push(Cut { width, what: quoted(piece.name.as_deref()), how: format!("runs {by:.0} pt past its card's {} edge", side.word()) });
            }
        }
        for (at, piece) in self.pieces.iter().enumerate() {
            let what = || quoted(piece.name.as_deref());
            if let Some(own) = piece.own.filter(|_| !reported.contains(&at))
                && piece.drawn.right - own.right > SLACK
                && !self.scrolls(&piece.clip)
            {
                let by = piece.drawn.right - own.right;
                cuts.push(Cut { width, what: what(), how: format!("runs {by:.0} pt past its own box, over what is beside it") });
            }
            if let Some(letters) = piece.squeezed.filter(|letters| *letters <= SQUEEZED) {
                let wide = piece.own.map_or(0.0, |own| own.width());
                cuts.push(Cut { width, what: what(), how: format!("is squeezed to {letters} letters a line, in a box {wide:.0} pt wide") });
            }
        }
        cuts
    }

    /// One cut for a group of pieces and boxes past the same edge: named by the text and buttons in it that are cut,
    /// else by the words inside its box, else by its size and place.
    fn cut_of(&self, group: &[Over], width: f32) -> Cut {
        let first = &group[0];
        let by = group.iter().map(|over| over.by).fold(0.0, f32::max);
        let outer = group
            .iter()
            .max_by(|a, b| (a.drawn.width() * a.drawn.height()).total_cmp(&(b.drawn.width() * b.drawn.height())))
            .map_or(first.drawn, |over| over.drawn);
        let mut names: Vec<String> = Vec::new();
        for name in group.iter().filter_map(|over| self.pieces.get(over.piece?)?.name.as_deref()) {
            let name = quoted(Some(name));
            if !names.contains(&name) {
                names.push(name);
            }
        }
        let what = if !names.is_empty() {
            let more = names.len().saturating_sub(3);
            names.truncate(3);
            match more {
                0 => names.join(", "),
                more => format!("{} and {more} more", names.join(", ")),
            }
        } else {
            let shown = Rect { left: outer.left.max(first.clip.left), right: outer.right.min(first.clip.right), ..outer };
            let inside: Vec<String> = self
                .pieces
                .iter()
                .filter(|piece| piece.drawn.overlaps(&shown))
                .filter_map(|piece| piece.name.as_deref())
                .take(2)
                .map(|name| quoted(Some(name)))
                .collect();
            let size = format!("a {:.0}×{:.0} box at ({:.0}, {:.0})", outer.width(), outer.height(), outer.left, outer.top);
            match inside.is_empty() {
                true => size,
                false => format!("{size} holding {}", inside.join(", ")),
            }
        };
        let side = first.side.word();
        let edge = match first.side {
            Side::Right if (first.edge - width).abs() < 1.0 => "the window's right edge".to_owned(),
            Side::Left if first.edge.abs() < 1.0 => "the window's left edge".to_owned(),
            _ => format!("the {side} edge of the box that clips it, at x {:.0}", first.edge),
        };
        let wholly = group.iter().all(|over| match over.side {
            Side::Right => over.drawn.left >= over.clip.right,
            Side::Left => over.drawn.right <= over.clip.left,
        });
        let how = match wholly {
            true => format!("is wholly past {edge}"),
            false => format!("runs {by:.0} pt past {edge}"),
        };
        Cut { width, what, how }
    }
}

/// The boxes `quads` paint, each with its clip. GPUI paints a box with a border or round corners as pieces, its inside
/// and its four edges, each with the whole box's bounds and a clip cut down to that piece: the box's clip is its pieces'
/// together. A left or right edge's piece is left out: a box with only a right border, a split's divider, is that one
/// piece, and its clip a sliver at the box's right edge, as if the rest of the box were cut off.
fn painted_boxes(quads: &[gpui::Quad], scale: f32) -> Vec<(Rect, Rect)> {
    let mut boxes: Vec<(Rect, Rect)> = Vec::new();
    let mut seen: HashMap<[u32; 4], usize> = HashMap::new();
    for quad in quads {
        let (drawn, clip) = (Rect::scaled(quad.bounds, scale), Rect::scaled(quad.content_mask.bounds, scale));
        if clip.width() <= EDGE && drawn.width() > EDGE {
            continue;
        }
        let key = [drawn.left, drawn.top, drawn.right, drawn.bottom].map(f32::to_bits);
        match seen.get(&key) {
            Some(&at) => {
                let joined = &mut boxes[at].1;
                *joined = Rect {
                    left: joined.left.min(clip.left),
                    top: joined.top.min(clip.top),
                    right: joined.right.max(clip.right),
                    bottom: joined.bottom.max(clip.bottom),
                };
            }
            None => {
                seen.insert(key, boxes.len());
                boxes.push((drawn, clip));
            }
        }
    }
    boxes
}

/// `overs` in groups past the same edge whose pieces overlap top to bottom: a box, the row in it and the words in the
/// row cut by one edge are one thing cut off.
fn groups(mut overs: Vec<Over>) -> Vec<Vec<Over>> {
    overs.sort_by(|a, b| {
        (a.side == Side::Left)
            .cmp(&(b.side == Side::Left))
            .then(a.edge.total_cmp(&b.edge))
            .then(a.drawn.top.total_cmp(&b.drawn.top))
    });
    let mut groups: Vec<Vec<Over>> = Vec::new();
    let mut reach = f32::MIN;
    for over in overs {
        let joins = groups.last().and_then(|group| group.first()).is_some_and(|first| {
            first.side == over.side && (first.edge - over.edge).abs() < 1.0 && over.drawn.top < reach
        });
        if joins {
            reach = reach.max(over.drawn.bottom);
            groups.last_mut().expect("a group to join").push(over);
        } else {
            reach = over.drawn.bottom;
            groups.push(vec![over]);
        }
    }
    groups.sort_by(|a, b| a[0].drawn.top.total_cmp(&b[0].drawn.top));
    groups
}

/// `name` in quotes, cut to its first words when long.
fn quoted(name: Option<&str>) -> String {
    let Some(name) = name else {
        return "a box".to_owned();
    };
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    match name.chars().count() > NAME {
        true => format!("\"{}…\"", name.chars().take(NAME).collect::<String>()),
        false => format!("\"{name}\""),
    }
}
