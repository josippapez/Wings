//! The cue that a hover card is on its way: a thin bar under the pointer that fills over the card's open delay and
//! follows the pointer while it waits. It moves, so it draws in the window's layer for progress bars
//! (`live::Kind::Progress`), never in the page that holds the card: a step of it, or a move of the pointer, draws that
//! layer and neither the page nor the piece under it.

use std::time::Duration;

use gpui::{AnyElement, App, Bounds, IntoElement, Pixels, Point, SharedString, TextStyle, Window, div, point, prelude::*, px, rgba, size};

use super::live;

/// How wide and how tall the bar is, in points.
pub const WIDTH: f32 = 28.0;
pub const HEIGHT: f32 = 3.0;

/// How far under the pointer's hot spot the bar sits: clear of the arrow, which is about 20 pt tall. Over the hot spot by
/// the same distance when there is no room below in the window.
const BELOW: f32 = 28.0;

/// Where the bar goes, in the window, for the pointer at `at` in a window `height` tall: centred under it, or over it near
/// the window's foot.
pub fn bar_at(at: Point<Pixels>, height: Pixels) -> Bounds<Pixels> {
    let above = height - at.y < px(BELOW + HEIGHT + 8.0);
    let y = if above { at.y - px(BELOW + HEIGHT) } else { at.y + px(BELOW) };
    Bounds::new(point(at.x - px(WIDTH / 2.0), y), size(px(WIDTH), px(HEIGHT)))
}

/// How often the bar steps: the spinners' pace.
const STEP: Duration = Duration::from_nanos(1_000_000_000 / 30);

/// Where the bar goes in a view a piece puts on screen (`slots::place_view`): all of the box that holds it, which is
/// `WIDTH` by `HEIGHT`. It fills from `started` (a moment on the clock, `live::clock`) over `delay`; `fill` and `track`
/// are its colours as `0xRRGGBBAA`. Under Reduce motion it stands still, empty.
pub fn hover_cue_slot(
    name: impl Into<SharedString>,
    started: Duration,
    delay: Duration,
    fill: u32,
    track: u32,
) -> impl IntoElement {
    live::slot(live::Kind::Progress, name, Cue { started, delay, fill, track })
}

struct Cue {
    started: Duration,
    delay: Duration,
    fill: u32,
    track: u32,
}

impl Cue {
    /// How much of the bar is full `now` into the clock, from 0 to 1.
    fn filled(&self, now: Duration) -> f32 {
        (now.saturating_sub(self.started).as_secs_f32() / self.delay.as_secs_f32()).clamp(0.0, 1.0)
    }
}

impl live::Draw for Cue {
    fn pace(&self, _: &Window, cx: &App) -> Option<Duration> {
        (!cx.reduce_motion()).then_some(STEP)
    }

    /// Under the pointer where it is now, not where it came onto the card's trigger: each step reads it again.
    fn place(&self, _: Bounds<Pixels>, window: &Window) -> Bounds<Pixels> {
        bar_at(window.mouse_position(), window.viewport_size().height)
    }

    fn draw(&self, now: Duration, _: &TextStyle, _: &Window, cx: &App) -> AnyElement {
        let filled = if cx.reduce_motion() { 0.0 } else { self.filled(now) };
        div()
            .size_full()
            .rounded_full()
            .overflow_hidden()
            .bg(rgba(self.track))
            .child(div().h_full().w(gpui::relative(filled)).rounded_full().bg(rgba(self.fill)))
            .into_any_element()
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;

    #[test]
    fn the_bar_fills_over_the_delay_and_no_further() {
        let cue = Cue { started: Duration::from_secs(2), delay: Duration::from_millis(600), fill: 0, track: 0 };
        assert_eq!(cue.filled(Duration::from_secs(1)), 0.0);
        assert_eq!(cue.filled(Duration::from_secs(2)), 0.0);
        assert!((cue.filled(Duration::from_millis(2300)) - 0.5).abs() < 1e-4);
        assert_eq!(cue.filled(Duration::from_secs(5)), 1.0);
    }

    #[test]
    fn the_bar_sits_centred_under_the_pointer_or_over_it_at_the_windows_foot() {
        let bar = bar_at(point(px(100.0), px(50.0)), px(800.0));
        assert_eq!(bar, Bounds::new(point(px(100.0 - WIDTH / 2.0), px(50.0 + BELOW)), size(px(WIDTH), px(HEIGHT))));
        let near_foot = bar_at(point(px(100.0), px(790.0)), px(800.0));
        assert_eq!(near_foot.origin.y, px(790.0 - BELOW - HEIGHT));
    }
}
