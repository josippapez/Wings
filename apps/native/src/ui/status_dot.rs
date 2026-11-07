//! The desktop's `StatusDot` (`components/status-dot.tsx`): what an agent is doing, in 10px.

use std::{
    cell::RefCell,
    collections::HashSet,
    f32::consts::TAU,
    sync::OnceLock,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Bounds, Element, EntityId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, PathBuilder, Pixels, Window, canvas, div, point, prelude::*, px, rgb, rgba,
};

use crate::{agents::AgentState, theme};

/// A faint ring with its top quarter in full color, turned by `angle`, like the desktop's
/// `border-working/30 border-t-working` circle.
pub fn spinner(angle: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let center = bounds.center();
            let radius = bounds.size.width / 2.0 - px(0.75);
            let arc = |from: f32, to: f32| {
                let at = |a: f32| point(center.x + radius * a.cos(), center.y + radius * a.sin());
                let mut path = PathBuilder::stroke(px(1.5));
                path.move_to(at(from));
                for step in 1..=24 {
                    path.line_to(at(from + (to - from) * step as f32 / 24.0));
                }
                path.build().ok()
            };
            if let Some(ring) = arc(0.0, TAU) {
                window.paint_path(ring, rgba(theme::WORKING << 8 | 0x4d));
            }
            let top = -TAU / 4.0 + angle;
            if let Some(quarter) = arc(top - TAU / 8.0, top + TAU / 8.0) {
                window.paint_path(quarter, rgb(theme::WORKING));
            }
        },
    )
    .size_full()
}

/// How often a working spinner moves on: 30 times a second.
const SPIN_STEP: Duration = Duration::from_nanos(1_000_000_000 / 30);

/// A ring that turns once a second, as `spinner` draws it. Every one on screen takes its angle from the same clock and
/// asks to be drawn again at the same moments, at most once per view and step, so any number of them cost one redraw
/// per step, where each used to keep a timer of its own. Under Reduce motion it stands still, as GPUI's animations do.
pub fn spinning() -> Spin {
    Spin
}

pub struct Spin;

thread_local! {
    /// The views that already asked to be drawn again at a step, by step.
    static WAKING: RefCell<HashSet<(EntityId, u128)>> = RefCell::default();
}

/// When the spinners' clock started: the first time one was drawn.
static ORIGIN: OnceLock<Instant> = OnceLock::new();

impl IntoElement for Spin {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Spin {
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
        let turn = if cx.reduce_motion() {
            0.0
        } else {
            let now = cx.background_executor().now();
            let elapsed = now.saturating_duration_since(*ORIGIN.get_or_init(|| now));
            wake_at_next_step(elapsed, window, cx);
            elapsed.as_secs_f32().fract()
        };
        let mut ring = div()
            .size_full()
            .child(spinner(turn * TAU))
            .into_any_element();
        (ring.request_layout(window, cx), ring)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        ring: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        ring.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        ring: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        ring.paint(window, cx);
    }
}

/// Draws the view being drawn again at the spinners' next step, `elapsed` into their clock, unless a spinner in it
/// already asked for that step.
fn wake_at_next_step(elapsed: Duration, window: &mut Window, cx: &mut App) {
    let step = elapsed.as_nanos() / SPIN_STEP.as_nanos();
    let view = window.current_view();
    if !WAKING.with_borrow_mut(|waking| waking.insert((view, step))) {
        return;
    }
    let next = SPIN_STEP * (step + 1) as u32;
    let delay = next.saturating_sub(elapsed);
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(delay).await;
            WAKING.with_borrow_mut(|waking| waking.remove(&(view, step)));
            cx.update(|_, cx| cx.notify(view)).ok();
        })
        .detach();
}

/// The desktop's `StatusDot`: a spinning ring while working, a red dot when it needs you, a green
/// dot when done and an outlined ring when idle, in a 10px box.
pub fn status_dot(state: AgentState) -> impl IntoElement {
    let dot = |size: f32| div().size(px(size)).rounded_full();
    let shape = match state {
        AgentState::Working => div().size(px(10.0)).child(spinning()).into_any_element(),
        AgentState::Blocked => dot(10.0).bg(rgb(theme::BLOCKED)).into_any_element(),
        AgentState::Done => dot(8.0).bg(rgb(theme::DONE)).into_any_element(),
        AgentState::Idle => dot(8.0)
            .border(px(1.5))
            .border_color(rgb(theme::IDLE))
            .into_any_element(),
    };
    div()
        .size(px(10.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(shape)
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{Context, Render, TestApp};

    use super::*;

    /// Working agents' dots in one view, counting each time the view draws.
    struct Dots {
        shown: usize,
        drawn: Rc<Cell<usize>>,
    }

    impl Render for Dots {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.drawn.set(self.drawn.get() + 1);
            div()
                .flex()
                .children((0..self.shown).map(|_| status_dot(AgentState::Working)))
        }
    }

    #[test]
    fn spinners_that_start_at_different_moments_still_draw_once_per_step() {
        let mut app = TestApp::new();
        let drawn = Rc::new(Cell::new(0));
        let counter = drawn.clone();
        let mut window = app.open_window(move |_, _| Dots {
            shown: 1,
            drawn: counter,
        });
        window.draw();
        // Two more agents start working a third and two thirds of a step later.
        for _ in 0..2 {
            app.advance_clock(SPIN_STEP / 3);
            window.update(|dots, _, cx| {
                dots.shown += 1;
                cx.notify();
            });
            app.run_until_parked();
        }
        let before = drawn.get();
        // A second's worth of steps draws the view once a step, not once a step for each spinner.
        for _ in 0..30 {
            app.advance_clock(SPIN_STEP);
            app.run_until_parked();
        }
        let steps = drawn.get() - before;
        assert!((29..=31).contains(&steps), "drawn {steps} times in 30 steps");
    }
}
