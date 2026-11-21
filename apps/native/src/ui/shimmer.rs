//! A line that pulses while something works, as the desktop's `shimmer`: its opacity follows the clock (`live::clock`),
//! at a third of the spinners' pace. In a view a piece puts on screen (`slots::place_view`), the window's layer for them
//! draws it (`shimmer_slot`, `live::Kind::Pulses`), so a step of it doesn't draw the view again; elsewhere it pulses in
//! place (`shimmer`).

use std::{f32::consts::TAU, rc::Rc, time::Duration};

use gpui::{
    AnyElement, App, Bounds, Div, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels,
    SharedString, TextStyle, Window, div, prelude::*,
};

use super::live;

/// Where a shimmering line goes in a view a piece puts on screen (`slots::place_view`): all of the box that holds it,
/// whose text style it is drawn in. The window's layer for pulsing lines draws `words()` there, pulsing; what holds the
/// slot draws the same words in no colour under it, for their room, selection and screen readers. `counts`: the words
/// count the time since something started, so the layer asks for them again each second while it doesn't pulse.
pub fn shimmer_slot(
    name: impl Into<SharedString>,
    counts: bool,
    words: impl Fn() -> SharedString + 'static,
) -> impl IntoElement {
    line_slot(live::Kind::Pulses, name, Line { words: Rc::new(words), counts, pulses: true })
}

/// Where a line that counts the time since something started, and doesn't pulse, goes in a view a piece puts on screen:
/// the window's layer for counting lines draws it again each second, so the view doesn't. A background task's
/// "Running · 12s" drew its whole page each second.
pub fn count_slot(name: impl Into<SharedString>, words: impl Fn() -> SharedString + 'static) -> impl IntoElement {
    line_slot(live::Kind::Counts, name, Line { words: Rc::new(words), counts: true, pulses: false })
}

fn line_slot(kind: live::Kind, name: impl Into<SharedString>, line: Line) -> impl IntoElement {
    div().absolute().top_0().left_0().size_full().child(live::slot(kind, name, line))
}

/// What a shimmering or counting line in a slot says, and how.
struct Line {
    /// Its words now: a `since` in them counts up.
    words: Rc<dyn Fn() -> SharedString>,
    /// Its words count the time since something started, and change each second.
    counts: bool,
    /// It pulses; a line that only counts stands still at full opacity.
    pulses: bool,
}

impl Line {
    /// It pulses now: not under Reduce motion, nor while the window is behind others.
    fn pulsing(&self, window: &Window, cx: &App) -> bool {
        self.pulses && !cx.reduce_motion() && window.is_window_active()
    }
}

impl live::Draw for Line {
    fn pace(&self, window: &Window, cx: &App) -> Option<Duration> {
        if self.pulsing(window, cx) {
            Some(SHIMMER_STEP)
        } else {
            self.counts.then_some(COUNT_STEP)
        }
    }

    fn draw(&self, now: Duration, style: &TextStyle, window: &Window, cx: &App) -> AnyElement {
        let opacity = if self.pulsing(window, cx) { opacity_at(now.as_secs_f32()) } else { 1.0 };
        div()
            .size_full()
            .whitespace_nowrap()
            .text_color(style.color.opacity(opacity))
            .child((self.words)())
            .into_any_element()
    }

    fn width(&self, _: Duration, style: &TextStyle, window: &Window, _: &App) -> Option<Pixels> {
        let words = (self.words)();
        let run = style.to_run(words.len());
        Some(window.text_system().shape_line(words, style.font_size.to_pixels(window.rem_size()), &[run], None).width)
    }
}

/// One pulse, from full to faint and back, in seconds.
const PERIOD: f32 = 1.6;

/// How often a shimmer steps: every third step of the spinners, 10 times a second. In the window's layer a step draws
/// only the layer; in place it draws again the view it is in, which at the spinners' 30 kept a whole chat page drawing
/// 30 times a second while an agent worked.
const SHIMMER_STEP: Duration = Duration::from_nanos(3 * (1_000_000_000 / 30));

/// How often a shimmer that stands still draws its view again when it counts the time since something started.
const COUNT_STEP: Duration = Duration::from_secs(1);

/// `element`, pulsing while the window is in front. It stands still at full opacity under Reduce motion and while the
/// window is behind others. `counts`: the element counts the time since something started (`since`), which a draw of its
/// view moves on, so it still draws its view once a second while it stands still.
pub fn shimmer(element: Div, counts: bool) -> Shimmer {
    Shimmer {
        element: Some(element),
        counts,
    }
}

pub struct Shimmer {
    element: Option<Div>,
    counts: bool,
}

/// The shimmer's opacity `seconds` into the spinners' clock: full at the start of each pulse, 0.45 halfway.
fn opacity_at(seconds: f32) -> f32 {
    let phase = (seconds / PERIOD).fract();
    0.45 + 0.55 * (0.5 + 0.5 * (phase * TAU).cos())
}

impl Element for Shimmer {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
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
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let element = self.element.take().expect("a shimmer is laid out once");
        let element = if !cx.reduce_motion() && window.is_window_active() {
            let seconds = live::ticking_every(SHIMMER_STEP, window, cx);
            element.opacity(opacity_at(seconds))
        } else {
            if self.counts {
                live::ticking_every(COUNT_STEP, window, cx);
            }
            element
        };
        let mut element = element.into_any_element();
        (element.request_layout(window, cx), element)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}
