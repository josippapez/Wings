//! A line that pulses while something works, as the desktop's `shimmer`: its opacity follows the spinners' clock
//! (`status_dot::ticking_every`), at a third of their pace. In a view a piece puts on screen (`slots::place_view`), the
//! window's `view_spinner_layer` draws it (`shimmer_slot`), so a step of it doesn't draw the view again; elsewhere it
//! pulses in place (`shimmer`).

use std::{cell::RefCell, collections::HashMap, f32::consts::TAU, rc::Rc, time::Duration};

use gpui::{
    AnyElement, App, Bounds, ContentMask, Div, Element, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, SharedString, TextStyle, TextStyleRefinement, Window, WindowId,
    canvas, div, prelude::*,
};

use super::{slots, status_dot};

/// What a shimmering line in a slot says, and how.
struct Line {
    /// The text style where its slot is, as the view that left it last drew it.
    style: TextStyle,
    /// Its words now: a `since` in them counts up.
    words: Rc<dyn Fn() -> SharedString>,
    /// Its words count the time since something started, and change each second.
    counts: bool,
}

thread_local! {
    /// Each shimmer slot's line, by window and slot.
    static LINES: RefCell<HashMap<(WindowId, SharedString), Line>> = RefCell::default();
}

/// The layer the shimmering lines in slots are drawn in.
const SHIMMERS: &str = "shimmers";

/// Where a shimmering line goes in a view a piece puts on screen (`slots::place_view`): all of the box that holds it,
/// whose text style it is drawn in. The window's `view_spinner_layer` draws `words()` there, pulsing; what holds the
/// slot draws the same words in no colour under it, for their room, selection and screen readers. `counts`: the words
/// count the time since something started, so the layer asks for them again each second.
pub fn shimmer_slot(
    name: impl Into<SharedString>,
    counts: bool,
    words: impl Fn() -> SharedString + 'static,
) -> impl IntoElement {
    let name = name.into();
    let key = name.clone();
    let words: Rc<dyn Fn() -> SharedString> = Rc::new(words);
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(slots::slot(SHIMMERS, name, None))
        .child(
            canvas(
                move |_, window, _| {
                    let id = window.window_handle().window_id();
                    let line = Line {
                        style: window.text_style(),
                        words,
                        counts,
                    };
                    LINES.with_borrow_mut(|lines| lines.insert((id, key), line));
                },
                |_, _, _, _| {},
            )
            .absolute(),
        )
}

/// The shimmering lines in the slots of the window `id`, laid out where their slots are, at the pulse's opacity now, and
/// a redraw of the view drawing them at the pulse's next step while the window is in front (each second while it isn't,
/// for lines that count). Drawn with `paint_lines`, in the same draw.
pub(super) fn lay_out_lines(id: WindowId, window: &mut Window, cx: &mut App) -> Vec<(AnyElement, ContentMask<Pixels>, TextStyleRefinement)> {
    let shown = slots::shown(id, SHIMMERS);
    LINES.with_borrow_mut(|lines| {
        lines.retain(|(window, name), _| *window != id || shown.iter().any(|(at, _)| at == name))
    });
    let lines: Vec<(slots::Slot, TextStyle, Rc<dyn Fn() -> SharedString>, bool)> = shown
        .into_iter()
        .filter_map(|(name, slot)| {
            LINES.with_borrow(|lines| {
                let line = lines.get(&(id, name))?;
                Some((slot, line.style.clone(), line.words.clone(), line.counts))
            })
        })
        .collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let opacity = if !cx.reduce_motion() && window.is_window_active() {
        opacity_at(status_dot::ticking_every(SHIMMER_STEP, window, cx))
    } else {
        if lines.iter().any(|(.., counts)| *counts) {
            status_dot::ticking_every(COUNT_STEP, window, cx);
        }
        1.0
    };
    lines
        .into_iter()
        .map(|(slot, style, words, _)| {
            let style = refinement(&style, opacity);
            let mut line = div().size_full().child(words()).into_any_element();
            window.with_text_style(Some(style.clone()), |window| {
                window.with_content_mask(Some(slot.mask), |window| {
                    line.prepaint_as_root(slot.bounds.origin, slot.bounds.size.into(), window, cx)
                })
            });
            (line, slot.mask, style)
        })
        .collect()
}

/// Draws the lines `lay_out_lines` laid out.
pub(super) fn paint_lines(lines: Vec<(AnyElement, ContentMask<Pixels>, TextStyleRefinement)>, window: &mut Window, cx: &mut App) {
    for (mut line, mask, style) in lines {
        window.with_text_style(Some(style), |window| {
            window.with_content_mask(Some(mask), |window| line.paint(window, cx))
        });
    }
}

/// All of `style`, its colour at `opacity`: what draws a line in another part of the window as it would be drawn where
/// its slot is.
fn refinement(style: &TextStyle, opacity: f32) -> TextStyleRefinement {
    TextStyleRefinement {
        color: Some(style.color.opacity(opacity)),
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

impl IntoElement for Shimmer {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
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
            let seconds = status_dot::ticking_every(SHIMMER_STEP, window, cx);
            element.opacity(opacity_at(seconds))
        } else {
            if self.counts {
                status_dot::ticking_every(COUNT_STEP, window, cx);
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
