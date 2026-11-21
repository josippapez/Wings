//! What moves while the tree it sits in stays the same (a spinner, a working dot, a pulsing line, a line that counts the
//! seconds) draws in a layer of the window, never in the piece or the page it sits in. The piece leaves a live slot
//! (`slot`): a name, the kind of layer, and what it draws, which says how often it changes (`Draw`). The window's layer
//! for that kind (`Layer`) draws it there, over the pieces, and asks for its next step on one clock. A step redraws the
//! layer, never the piece or the page under it: GPUI draws a notified view again with every view it is in, and a cached
//! piece that draws again rebuilds all of it. A page that turned its own spinners rebuilt a whole chat 30 times a second.
//!
//! Each kind of update has a layer of its own, as a layer's redraw draws everything it holds: the spinners step 30 times
//! a second, the pulsing lines 10, the counting lines once. What macOS animates by itself stays there: the spinners a
//! window's host turns in Core Animation (`spinner_host`) and the backdrops under the window (`native_backdrop`).

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::OnceLock,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Context, EntityId, IntoElement, Pixels, Render, SharedString, TextStyle, TextStyleRefinement, Window,
    WindowId, canvas, div, prelude::*, px,
};

use super::{
    readout, slots,
    slots::Piece,
    spinner_host,
};
use crate::theme;

#[cfg(all(test, feature = "test-support"))]
mod tests;

/// A kind of live slot: the layer of the window that draws it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    /// Spinners and working dots in the window's pieces (the sidebar, the tabs, the pane cards), drawn over all of them.
    Spinners,
    /// Spinners and working dots in the views a piece puts on screen (`slots::place_view`): the pages.
    ViewSpinners,
    /// Lines in those views that pulse while something works.
    Pulses,
    /// Lines in those views that count the seconds since something started, and don't pulse.
    Counts,
    /// Bars in those views that fill over a wait, a hover card's cue, at the spinners' pace.
    Progress,
}

impl Kind {
    /// The layer of slots it draws in (`slots`), which is also its name in the performance readout.
    pub(crate) const fn layer(self) -> &'static str {
        match self {
            Kind::Spinners => "spinners",
            Kind::ViewSpinners => "view-spinners",
            Kind::Pulses => "pulses",
            Kind::Counts => "counts",
            Kind::Progress => "progress",
        }
    }

    /// Its slots are spinners, which what turns a window's spinners outside its drawing turns (`spinner_host`).
    fn handed(self) -> bool {
        matches!(self, Kind::Spinners | Kind::ViewSpinners)
    }
}

/// What a live slot draws, each time its layer draws.
pub trait Draw: 'static {
    /// How often it looks different now: its layer draws again at the next multiple of this on the clock (`clock`).
    /// None while it stands still (Reduce motion, a window behind others).
    fn pace(&self, window: &Window, cx: &App) -> Option<Duration>;
    /// What it shows `now` into the clock, in `style`, the text style where its slot is. Laid out at the slot's place and
    /// size, clipped as the slot is, in that text style.
    fn draw(&self, now: Duration, style: &TextStyle, window: &Window, cx: &App) -> AnyElement;
    /// The width of what it shows `now` on one line, for what changes in width as it moves (a count of seconds): when
    /// it is wider than its slot, the view that left the slot draws again, so the slot takes the room it needs now.
    fn width(&self, _now: Duration, _style: &TextStyle, _window: &Window, _cx: &App) -> Option<Pixels> {
        None
    }
}

/// What a slot's piece last left it to draw, and the text style where it is.
struct Drawn {
    draw: Rc<dyn Draw>,
    style: TextStyle,
}

thread_local! {
    /// What each live slot draws, by window, kind and name.
    static DRAWS: RefCell<HashMap<(WindowId, Kind, SharedString), Drawn>> = RefCell::default();
    /// When each view last asked for its next step, on the clock.
    static ASKED: RefCell<HashMap<EntityId, Duration>> = RefCell::default();
    /// The views that already asked to be drawn again at a moment on the clock, by that moment and the pace of what
    /// asked, in nanoseconds.
    static WAKING: RefCell<HashSet<(EntityId, u128, u128)>> = RefCell::default();
    /// The width a slot's view was last asked to make room for, by window, kind and name, in tenths of a pixel: a view
    /// is asked once per width, not at each step.
    static GREW: RefCell<HashMap<(WindowId, Kind, SharedString), i64>> = RefCell::default();
}

/// When the clock started: the first time something on it was drawn.
static ORIGIN: OnceLock<Instant> = OnceLock::new();

/// Where `draw` goes, filling what holds it, in a piece drawn with `slots::drawing_piece` or a view a piece puts on
/// screen (`slots::place_view`): the window's layer for `kind` draws it there and steps it, so a step draws neither the
/// piece nor the view. Names are the window's own within a kind: a page puts its own in them.
pub fn slot(kind: Kind, name: impl Into<SharedString>, draw: impl Draw) -> impl IntoElement {
    let name = name.into();
    let key = name.clone();
    let draw: Rc<dyn Draw> = Rc::new(draw);
    div().size_full().child(slots::slot(kind.layer(), name, None)).child(
        canvas(
            move |_, window, _| {
                let id = window.window_handle().window_id();
                let drawn = Drawn { draw, style: window.text_style() };
                DRAWS.with_borrow_mut(|draws| draws.insert((id, kind, key), drawn));
            },
            |_, _, _, _| {},
        )
        .absolute(),
    )
}

/// The window's layer for one kind of live slot, as a view of its own, so a step of what it draws redraws it and none of
/// the pieces it is drawn over. Make it before the window's first draw: GPUI only redraws a view that is older than the
/// frame it is first drawn in. Draw it in each draw of the window, after the pieces with its slots and once
/// `slots::start_draw` has started the draw.
pub struct Layer(Kind);

impl Layer {
    pub fn new(kind: Kind) -> Self {
        Self(kind)
    }
}

impl Render for Layer {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        readout::render(self.0.layer());
        layer(self.0, window)
    }
}

/// The layer for `kind` drawn in the view that draws it, which its steps then draw again: for a window of a single view.
pub fn layer(kind: Kind, window: &Window) -> impl IntoElement {
    let id = window.window_handle().window_id();
    canvas(
        move |_, window, cx| lay_out(id, kind, window, cx),
        move |_, (now, laid), window, cx| paint(id, kind, now, laid, window, cx),
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The layers of what moves in the views a piece puts on screen (spinners, pulsing lines, counting lines), in the view
/// that draws them, for a window that draws them in its own view.
pub fn view_spinner_layer(window: &Window) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(layer(Kind::ViewSpinners, window))
        .child(layer(Kind::Pulses, window))
        .child(layer(Kind::Counts, window))
        .child(layer(Kind::Progress, window))
}

/// A slot laid out for its layer's paint.
struct Laid {
    name: SharedString,
    slot: slots::Slot,
    element: AnyElement,
    style: TextStyleRefinement,
    pace: Option<Duration>,
}

/// The slots of `kind` in the window `id` that show, each laid out where it is with what its piece left it to draw.
fn lay_out(id: WindowId, kind: Kind, window: &mut Window, cx: &mut App) -> (Duration, Vec<Laid>) {
    let shown = slots::shown(id, kind.layer());
    DRAWS.with_borrow_mut(|draws| {
        draws.retain(|(window, of, name), _| *window != id || *of != kind || shown.iter().any(|(at, _)| at == name))
    });
    GREW.with_borrow_mut(|grew| {
        grew.retain(|(window, of, name), _| *window != id || *of != kind || shown.iter().any(|(at, _)| at == name))
    });
    let now = clock(cx);
    let laid = shown
        .into_iter()
        .filter_map(|(name, slot)| {
            let (draw, style) =
                DRAWS.with_borrow(|draws| draws.get(&(id, kind, name.clone())).map(|it| (it.draw.clone(), it.style.clone())))?;
            let mut element = draw.draw(now, &style, window, cx);
            // Wider than its slot, which the view measured when it last drew: it draws again with the words as they
            // are now, once for each width.
            if let (Some(width), Piece::View(view)) = (draw.width(now, &style, window, cx), slot.piece) {
                if width > slot.bounds.size.width + px(0.5) {
                    let tenths = (f32::from(width) * 10.0) as i64;
                    let asked = GREW.with_borrow_mut(|grew| grew.insert((id, kind, name.clone()), tenths) != Some(tenths));
                    if asked {
                        cx.notify(view);
                    }
                }
            }
            let style = refinement(&style);
            window.with_text_style(Some(style.clone()), |window| {
                window.with_content_mask(Some(slot.mask), |window| {
                    element.prepaint_as_root(slot.bounds.origin, slot.bounds.size.into(), window, cx)
                })
            });
            let pace = draw.pace(window, cx);
            Some(Laid { name, slot, element, style, pace })
        })
        .collect();
    (now, laid)
}

/// Paints what `lay_out` laid out, and asks for the layer's next step while any of it moves. In a window whose spinners
/// turn outside its drawing (`spinner_host`), a spinner is handed to that instead, unless an overlay of this draw is over
/// it (`slots::cover`): only those it keeps ask for a step.
fn paint(id: WindowId, kind: Kind, now: Duration, laid: Vec<Laid>, window: &mut Window, cx: &mut App) {
    let hosted = kind.handed() && spinner_host::hosted(id);
    // Painted, every overlay has its place in this draw.
    let covers = if hosted { slots::covers(id) } else { Vec::new() };
    let (scale, still) = (window.scale_factor(), cx.reduce_motion());
    let mut pace: Option<Duration> = None;
    for Laid { name, slot, mut element, style, pace: its } in laid {
        if hosted {
            let clip = slot.bounds.intersect(&slot.mask.bounds);
            if clip.is_empty() {
                continue;
            }
            if !covers.iter().any(|cover| cover.intersects(&clip)) {
                let placed = spinner_host::Placed {
                    bounds: slot.bounds,
                    clip: slot.mask.bounds,
                    color: theme::palette().working,
                    scale,
                    still,
                };
                spinner_host::hand(id, (slot.piece, kind.layer(), name), placed);
                continue;
            }
        }
        window.with_text_style(Some(style), |window| {
            window.with_content_mask(Some(slot.mask), |window| element.paint(window, cx))
        });
        pace = match (pace, its) {
            (Some(pace), Some(its)) => Some(pace.min(its)),
            (pace, its) => pace.or(its),
        };
    }
    if let Some(pace) = pace {
        wake_at_next(now, pace, window, cx);
    }
}

/// All of `style`: what draws something in another part of the window as it would be drawn where its slot is.
fn refinement(style: &TextStyle) -> TextStyleRefinement {
    TextStyleRefinement {
        color: Some(style.color),
        font_family: Some(style.font_family.clone()),
        font_features: Some(style.font_features.clone()),
        font_fallbacks: style.font_fallbacks.clone(),
        font_size: Some(style.font_size),
        line_height: Some(style.line_height),
        font_weight: Some(style.font_weight),
        font_style: Some(style.font_style),
        background_color: style.background_color,
        underline: style.underline,
        strikethrough: style.strikethrough,
        white_space: Some(style.white_space),
        text_overflow: style.text_overflow.clone(),
        text_align: Some(style.text_align),
        line_clamp: style.line_clamp,
    }
}

/// How far into the clock it is now. Everything that moves steps on this one clock, so any number of them at any pace
/// make one stream of frames: a slower pace's steps fall on a faster one's.
pub(crate) fn clock(cx: &App) -> Duration {
    let now = cx.background_executor().now();
    now.saturating_duration_since(*ORIGIN.get_or_init(|| now))
}

/// Seconds on the clock, and a draw again of the view being drawn at the next multiple of `every` on it: for something
/// that moves in that view. In a page or a piece, use a live slot instead. Whether it should move at all (Reduce motion,
/// a window behind others) is the caller's to say.
pub fn ticking_every(every: Duration, window: &mut Window, cx: &mut App) -> f32 {
    let elapsed = clock(cx);
    wake_at_next(elapsed, every, window, cx);
    elapsed.as_secs_f32()
}

/// Draws the view being drawn again at the next multiple of `every` on the clock, `elapsed` into it, unless something in
/// it already asked for that moment at that pace.
fn wake_at_next(elapsed: Duration, every: Duration, window: &mut Window, cx: &mut App) {
    let view = window.current_view();
    ASKED.with_borrow_mut(|asked| asked.insert(view, elapsed));
    wake_after(view, elapsed, every, window, cx);
}

fn wake_after(view: EntityId, elapsed: Duration, every: Duration, window: &mut Window, cx: &mut App) {
    let step = elapsed.as_nanos() / every.as_nanos();
    let next = Duration::from_nanos(((step + 1) * every.as_nanos()) as u64);
    // By pace too: a slower pace's wait skips a moment a faster one has to draw in (`recent` below).
    let key = (view, next.as_nanos(), every.as_nanos());
    if !WAKING.with_borrow_mut(|waking| waking.insert(key)) {
        return;
    }
    let delay = next.saturating_sub(elapsed);
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(delay).await;
            WAKING.with_borrow_mut(|waking| waking.remove(&key));
            // A view that drew again since this was asked, within the last step, for a terminal's output say, moved on
            // then: it waits for the step after, so a busy terminal and a spinner make one stream of frames, not two.
            let recent = ASKED.with_borrow(|asked| asked.get(&view).is_some_and(|&at| at > elapsed && at + every >= next));
            cx.update(|window, cx| {
                if recent {
                    wake_after(view, next, every, window, cx);
                } else {
                    cx.notify(view);
                }
            })
            .ok();
        })
        .detach();
}
