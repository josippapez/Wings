//! The desktop's `StatusDot` (`components/status-dot.tsx`): what an agent is doing, in 10px.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    f32::consts::TAU,
    sync::OnceLock,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Bounds, ContentMask, Element, EntityId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, PathBuilder, Pixels, SharedString, Window, WindowId, canvas, div, point,
    prelude::*, px, rgb, rgba,
};

use crate::{agents::AgentState, theme};

/// A faint ring with its top quarter in full color, turned by `angle`, like the desktop's
/// `border-working/30 border-t-working` circle.
pub fn spinner(angle: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint_ring(bounds, angle, window),
    )
    .size_full()
}

/// `spinner`'s ring in `bounds`, turned by `angle`.
fn paint_ring(bounds: Bounds<Pixels>, angle: f32, window: &mut Window) {
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
    /// When each view last drew its spinners, on their clock.
    static DRAWN: RefCell<HashMap<EntityId, Duration>> = RefCell::default();
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
        let turn = turn(window, cx);
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

/// How far round the spinners are, and a redraw of the view being drawn asked for at their next step. Still under
/// Reduce motion.
fn turn(window: &mut Window, cx: &mut App) -> f32 {
    if cx.reduce_motion() {
        return 0.0;
    }
    let now = cx.background_executor().now();
    let elapsed = now.saturating_duration_since(*ORIGIN.get_or_init(|| now));
    wake_at_next_step(elapsed, window, cx);
    elapsed.as_secs_f32().fract()
}

/// Spinners over a piece of the window that keeps its last drawing (`workspace::parts`). The piece leaves a slot where
/// each goes (`spinner_slot`) and the window's `spinner_layer` turns them, so a step redraws the layer, not the piece.
#[derive(Default)]
struct Slots {
    /// The window's draws so far, counted by its `spinner_layer`.
    frame: HashMap<WindowId, u64>,
    /// The draw each piece of a window last drew itself in.
    drawn: HashMap<(WindowId, &'static str), u64>,
    /// Each slot's place and what clips it, by window, piece and name, with the draw that left it there.
    slots: HashMap<(WindowId, &'static str, SharedString), Slot>,
    /// The piece being drawn, whose the slots made now are.
    current: Option<&'static str>,
}

/// A slot's place, what clips it there, and the draw that left it.
type Slot = (Bounds<Pixels>, ContentMask<Pixels>, u64);

/// The piece the window's own drawing is, outside any `drawing_piece`.
const WINDOW: &str = "window";

thread_local! {
    static SLOTS: RefCell<Slots> = RefCell::default();
}

/// Runs `draw`, the drawing of the piece `piece` of the window `id`, so the slots it leaves are its own.
pub fn drawing_piece<R>(piece: &'static str, id: WindowId, draw: impl FnOnce() -> R) -> R {
    let outer = SLOTS.with_borrow_mut(|slots| {
        let frame = slots.frame.get(&id).copied().unwrap_or_default();
        slots.drawn.insert((id, piece), frame);
        slots.current.replace(piece)
    });
    let drawn = draw();
    SLOTS.with_borrow_mut(|slots| slots.current = outer);
    drawn
}

/// Where the spinner named `name` goes, in a piece drawn with `drawing_piece`: `spinner_layer` turns it there.
pub fn spinner_slot(name: impl Into<SharedString>) -> impl IntoElement {
    let piece = SLOTS.with_borrow(|slots| slots.current).unwrap_or(WINDOW);
    let name = name.into();
    canvas(
        move |bounds, window, _| {
            let id = window.window_handle().window_id();
            let mask = window.content_mask();
            SLOTS.with_borrow_mut(|slots| {
                let frame = slots.frame.get(&id).copied().unwrap_or_default();
                slots.slots.insert((id, piece, name), (bounds, mask, frame));
            });
        },
        |_, _, _, _| {},
    )
    .size_full()
}

/// The window's spinners in slots, drawn over everything before it in the window. Draw it once in each draw of the
/// window, after the pieces with slots.
pub fn spinner_layer(window: &Window) -> impl IntoElement {
    let id = window.window_handle().window_id();
    SLOTS.with_borrow_mut(|slots| {
        let frame = slots.frame.entry(id).or_default();
        *frame += 1;
        let frame = *frame;
        slots.drawn.insert((id, WINDOW), frame);
    });
    // The slots still there: left in the draw their piece last drew itself in.
    let shown = move || {
        SLOTS.with_borrow_mut(|slots| {
            let Slots { drawn, slots, .. } = slots;
            slots.retain(|(window, piece, _), (_, _, frame)| {
                *window != id || drawn.get(&(*window, *piece)) == Some(frame)
            });
            slots
                .iter()
                .filter(|((window, _, _), _)| *window == id)
                .map(|(_, (bounds, mask, _))| (*bounds, *mask))
                .collect::<Vec<_>>()
        })
    };
    canvas(
        move |_, window, cx| {
            let shown = shown();
            let turn = if shown.is_empty() {
                0.0
            } else {
                turn(window, cx)
            };
            (shown, turn)
        },
        |_, (shown, turn), window, _| {
            for (bounds, mask) in shown {
                window
                    .with_content_mask(Some(mask), |window| paint_ring(bounds, turn * TAU, window));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Draws the view being drawn again at the spinners' next step, `elapsed` into their clock, unless a spinner in it
/// already asked for that step.
fn wake_at_next_step(elapsed: Duration, window: &mut Window, cx: &mut App) {
    let view = window.current_view();
    DRAWN.with_borrow_mut(|drawn| drawn.insert(view, elapsed));
    wake_after(view, elapsed, window, cx);
}

fn wake_after(view: EntityId, elapsed: Duration, window: &mut Window, cx: &mut App) {
    let step = elapsed.as_nanos() / SPIN_STEP.as_nanos();
    if !WAKING.with_borrow_mut(|waking| waking.insert((view, step))) {
        return;
    }
    let next = SPIN_STEP * (step + 1) as u32;
    let delay = next.saturating_sub(elapsed);
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(delay).await;
            WAKING.with_borrow_mut(|waking| waking.remove(&(view, step)));
            // A view that drew its spinners again since this was asked, within the last step, for a terminal's output say,
            // turned them
            // then: it waits for the step after, so a busy terminal and a spinner make one stream of frames, not two.
            let recent = DRAWN.with_borrow(|drawn| {
                drawn
                    .get(&view)
                    .is_some_and(|&at| at > elapsed && at + SPIN_STEP >= next)
            });
            cx.update(|window, cx| {
                if recent {
                    wake_after(view, next, window, cx);
                } else {
                    cx.notify(view);
                }
            })
            .ok();
        })
        .detach();
}

/// `status_dot` in a piece drawn with `drawing_piece`: working, its ring is the spinner in the slot `name`.
pub fn status_dot_slot(state: AgentState, name: impl Into<SharedString>) -> AnyElement {
    match state {
        AgentState::Working => div()
            .size(px(10.0))
            .flex_none()
            .child(spinner_slot(name))
            .into_any_element(),
        state => status_dot(state).into_any_element(),
    }
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

    /// A piece that keeps its last drawing, with a working agent's dot in it, counting its draws.
    struct Piece(Rc<Cell<usize>>);

    impl Render for Piece {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            drawing_piece("piece", window.window_handle().window_id(), || {
                div().child(status_dot_slot(AgentState::Working, "agent"))
            })
        }
    }

    /// The window: the piece, cached, and the spinner layer over it, counting its own draws.
    struct Frame {
        piece: gpui::Entity<Piece>,
        drawn: Rc<Cell<usize>>,
    }

    impl Render for Frame {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.drawn.set(self.drawn.get() + 1);
            div()
                .size_full()
                .child(
                    self.piece
                        .clone()
                        .cached(gpui::StyleRefinement::default().size(px(20.0))),
                )
                .child(spinner_layer(window))
        }
    }

    #[test]
    fn a_spinner_in_a_slot_turns_without_drawing_its_piece_again() {
        let mut app = TestApp::new();
        let (piece_drawn, frame_drawn) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let (piece_counter, frame_counter) = (piece_drawn.clone(), frame_drawn.clone());
        let mut window = app.open_window(move |_, cx| Frame {
            piece: cx.new(|_| Piece(piece_counter)),
            drawn: frame_counter,
        });
        window.draw();
        let (pieces, frames) = (piece_drawn.get(), frame_drawn.get());
        for _ in 0..30 {
            app.advance_clock(SPIN_STEP);
            app.run_until_parked();
        }
        assert_eq!(piece_drawn.get(), pieces, "the piece kept its drawing");
        let steps = frame_drawn.get() - frames;
        assert!(
            (29..=31).contains(&steps),
            "the layer turned {steps} times in 30 steps"
        );
    }

    #[test]
    fn a_view_redrawn_by_its_terminal_turns_its_spinners_in_those_frames() {
        let mut app = TestApp::new();
        let drawn = Rc::new(Cell::new(0));
        let counter = drawn.clone();
        let mut window = app.open_window(move |_, _| Dots {
            shown: 1,
            drawn: counter,
        });
        window.draw();
        let before = drawn.get();
        // A second of a terminal's output redrawing the view 20 times, every 50 ms, in 10 ms ticks.
        for tick in 1..=100 {
            app.advance_clock(Duration::from_millis(10));
            if tick % 5 == 0 {
                window.update(|_, _, cx| cx.notify());
            }
            app.run_until_parked();
        }
        let frames = drawn.get() - before;
        assert!(
            frames <= 35,
            "{frames} frames, where the output's 20 and the spinners' 30 used to make 50"
        );
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
        assert!(
            (29..=31).contains(&steps),
            "drawn {steps} times in 30 steps"
        );
    }
}
