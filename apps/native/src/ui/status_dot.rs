//! The desktop's `StatusDot` (`components/status-dot.tsx`): what an agent is doing, in 10px.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    f32::consts::TAU,
    sync::OnceLock,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Bounds, Element, EntityId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, PathBuilder, Pixels, SharedString, Window, canvas, div, point, prelude::*, px, rgb,
    rgba,
};

use super::{slots, spinner_host};
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
        window.paint_path(ring, rgba(theme::palette().working << 8 | 0x4d));
    }
    let top = -TAU / 4.0 + angle;
    if let Some(quarter) = arc(top - TAU / 8.0, top + TAU / 8.0) {
        window.paint_path(quarter, rgb(theme::palette().working));
    }
}

/// How often a working spinner moves on: 30 times a second.
pub(crate) const SPIN_STEP: Duration = Duration::from_nanos(1_000_000_000 / 30);

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
    /// The views that already asked to be drawn again at a moment on the clock, by that moment and the pace of what
    /// asked, in nanoseconds.
    static WAKING: RefCell<HashSet<(EntityId, u128, u128)>> = RefCell::default();
}

/// When the spinners' clock started: the first time one was drawn.
static ORIGIN: OnceLock<Instant> = OnceLock::new();

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
    let elapsed = clock(cx);
    wake_at_next(elapsed, SPIN_STEP, window, cx);
    elapsed.as_secs_f32().fract()
}

/// How far into the spinners' clock it is now, for something that steps on it at a pace of its own (`backdrop`), so its
/// steps fall on theirs.
pub(super) fn clock(cx: &App) -> Duration {
    let now = cx.background_executor().now();
    now.saturating_duration_since(*ORIGIN.get_or_init(|| now))
}

/// Seconds on the spinners' clock, and a redraw of the view being drawn at the next multiple of `every` on it, for
/// something that moves slower than the spinners, so each of its steps falls on one of theirs. Whether it should move at
/// all (Reduce motion, a window behind others) is the caller's to say.
pub fn ticking_every(every: Duration, window: &mut Window, cx: &mut App) -> f32 {
    let elapsed = clock(cx);
    wake_at_next(elapsed, every, window, cx);
    elapsed.as_secs_f32()
}

/// Where the spinner named `name` goes, in a piece drawn with `drawing_piece`: `spinner_layer` turns it there, so a step
/// redraws the layer, not the piece.
pub fn spinner_slot(name: impl Into<SharedString>) -> impl IntoElement {
    slots::slot(SPINNERS, name, None)
}

/// The layer the spinners in slots turn in.
const SPINNERS: &str = "spinners";

/// The window's spinners in slots, drawn over everything before it in the window. Draw it once in each draw of the
/// window, after the pieces with slots; it also starts the window's draw for the other slot layers (`slots`).
pub fn spinner_layer(window: &Window) -> impl IntoElement {
    let id = window.window_handle().window_id();
    slots::start_draw(id);
    rings(id, SPINNERS)
}

/// Where the spinner named `name` goes in a view a piece puts on screen (`slots::place_view`), a page say:
/// `view_spinner_layer` turns it there, so a step redraws the layer, not the view, which may be a whole chat.
pub fn view_spinner_slot(name: impl Into<SharedString>) -> impl IntoElement {
    slots::slot(VIEW_SPINNERS, name, None)
}

/// `status_dot` in a view a piece puts on screen: working, its ring is the spinner in the slot `name`
/// (`view_spinner_slot`).
pub fn view_status_dot_slot(state: AgentState, name: impl Into<SharedString>) -> AnyElement {
    match state {
        AgentState::Working => div()
            .size(px(10.0))
            .flex_none()
            .child(view_spinner_slot(name))
            .into_any_element(),
        state => status_dot(state).into_any_element(),
    }
}

/// The layer the spinners in views' slots turn in.
const VIEW_SPINNERS: &str = "view-spinners";

/// What moves in the views a piece puts on screen (`slots::place_view`), so a step of it draws this layer and not the
/// views, which may be whole chats: the spinners and working dots in their slots (`view_spinner_slot`,
/// `view_status_dot_slot`) and the shimmering lines in theirs (`shimmer::shimmer_slot`), all on one clock. Drawn right
/// after the piece that puts the views on screen, so what the window draws over that piece later (a sidebar sliding over
/// it, a floating terminal, a menu) stays over them too. In the same draw of the window as `spinner_layer`, which starts
/// it.
pub fn view_spinner_layer(window: &Window) -> impl IntoElement {
    view_layer(window.window_handle().window_id())
}

fn view_layer(id: gpui::WindowId) -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(rings(id, VIEW_SPINNERS))
        .child(
            canvas(
                move |_, window, cx| super::shimmer::lay_out_lines(id, window, cx),
                |_, lines, window, cx| super::shimmer::paint_lines(lines, window, cx),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
}

/// `spinner_layer` (`Spinners::Window`) or `view_spinner_layer` (`Spinners::Views`) as a view of its own, so a step of
/// its spinners redraws it and not the view it is in. A step tells the view drawing the rings to draw again, and the
/// workspace's pieces draw again whenever the workspace view is told so (`workspace::parts`): in the workspace itself,
/// each step drew the sidebar, title bar and main column again. Make it before the window's first draw: GPUI only redraws
/// a view that is older than the frame it is first drawn in.
pub struct SpinnerLayer(Spinners);

#[derive(Clone, Copy)]
pub enum Spinners {
    Window,
    Views,
}

impl SpinnerLayer {
    pub fn new(spinners: Spinners) -> Self {
        Self(spinners)
    }

    /// The layer to draw in the window's draw, where `spinner_layer` or `view_spinner_layer` would go. Unlike
    /// `spinner_layer` it doesn't start the window's draw for the slot layers: call `slots::start_draw` first in the draw.
    pub fn in_draw(layer: &gpui::Entity<Self>) -> AnyElement {
        layer.clone().into_any_element()
    }
}

impl Render for SpinnerLayer {
    fn render(&mut self, window: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        super::readout::render(match self.0 {
            Spinners::Window => "spinners",
            Spinners::Views => "view-spinners",
        });
        let id = window.window_handle().window_id();
        match self.0 {
            Spinners::Window => div().absolute().top_0().left_0().size_full().child(rings(id, SPINNERS)),
            Spinners::Views => view_layer(id),
        }
    }
}

/// The rings in the slots of `layer` in the window `id`, all at one angle, and a redraw of the view drawing them at the
/// spinners' next step while there are any. In a window whose spinners turn outside its drawing (`spinner_host`), only
/// those under an overlay of this draw (`slots::cover`): the others are handed to the host, and ask for no redraw.
fn rings(id: gpui::WindowId, layer: &'static str) -> impl IntoElement {
    canvas(
        move |_, window, cx| {
            let shown = slots::shown(id, layer);
            let turn = if shown.is_empty() || spinner_host::hosted(id) {
                0.0
            } else {
                turn(window, cx)
            };
            (shown, turn)
        },
        move |_, (shown, turn), window, cx| {
            if !spinner_host::hosted(id) {
                for (_, slot) in shown {
                    window.with_content_mask(Some(slot.mask), |window| {
                        paint_ring(slot.bounds, turn * TAU, window)
                    });
                }
                return;
            }
            // Painted, every overlay has its place in this draw.
            let covers = slots::covers(id);
            let (scale, still) = (window.scale_factor(), cx.reduce_motion());
            let mut under = Vec::new();
            for (name, slot) in shown {
                let clip = slot.bounds.intersect(&slot.mask.bounds);
                if clip.is_empty() {
                    continue;
                }
                if covers.iter().any(|cover| cover.intersects(&clip)) {
                    under.push(slot);
                } else {
                    let placed = spinner_host::Placed {
                        bounds: slot.bounds,
                        clip: slot.mask.bounds,
                        color: theme::palette().working,
                        scale,
                        still,
                    };
                    spinner_host::hand(id, (slot.piece, layer, name), placed);
                }
            }
            if !under.is_empty() {
                let turn = self::turn(window, cx);
                for slot in under {
                    window.with_content_mask(Some(slot.mask), |window| {
                        paint_ring(slot.bounds, turn * TAU, window)
                    });
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Draws the view being drawn again at the next multiple of `every` on the spinners' clock, `elapsed` into it, unless
/// something in it already asked for that moment.
fn wake_at_next(elapsed: Duration, every: Duration, window: &mut Window, cx: &mut App) {
    let view = window.current_view();
    DRAWN.with_borrow_mut(|drawn| drawn.insert(view, elapsed));
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
            // A view that drew its spinners again since this was asked, within the last step, for a terminal's output say,
            // turned them then: it waits for the step after, so a busy terminal and a spinner make one stream of frames,
            // not two.
            let recent = DRAWN.with_borrow(|drawn| {
                drawn
                    .get(&view)
                    .is_some_and(|&at| at > elapsed && at + every >= next)
            });
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
        AgentState::Done => dot(8.0).bg(rgb(theme::palette().done)).into_any_element(),
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
            slots::drawing_piece("piece", window.window_handle().window_id(), || {
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

    /// The window as the workspace draws it: the piece, cached, and the spinner layer, a view of its own, over it.
    struct Layered {
        piece: gpui::Entity<Piece>,
        spinners: gpui::Entity<SpinnerLayer>,
        drawn: Rc<Cell<usize>>,
    }

    impl Render for Layered {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.drawn.set(self.drawn.get() + 1);
            div()
                .size_full()
                .child(
                    self.piece
                        .clone()
                        .cached(gpui::StyleRefinement::default().size(px(20.0))),
                )
                .child({
                    slots::start_draw(window.window_handle().window_id());
                    SpinnerLayer::in_draw(&self.spinners)
                })
        }
    }

    /// The workspace's pieces draw again whenever the workspace changes (`workspace::parts`): a step of the spinners isn't
    /// such a change. It was: each step told the window's root view, so the sidebar, title bar and main column all drew
    /// again 30 times a second while an agent worked anywhere.
    #[test]
    fn a_spinner_step_leaves_alone_a_piece_that_follows_the_window() {
        let mut app = TestApp::new();
        let piece_drawn = Rc::new(Cell::new(0));
        let frame_drawn = Rc::new(Cell::new(0));
        let (piece_counter, frame_counter) = (piece_drawn.clone(), frame_drawn.clone());
        let mut window = app.open_window(move |_, cx| Layered {
            piece: cx.new(|_| Piece(piece_counter)),
            spinners: cx.new(|_| SpinnerLayer::new(Spinners::Window)),
            drawn: frame_counter,
        });
        window.update(|frame, _, cx| {
            let root = cx.entity();
            frame.piece.update(cx, |_, cx| cx.observe(&root, |_, _, cx| cx.notify()).detach());
        });
        window.draw();
        let (pieces, frames) = (piece_drawn.get(), frame_drawn.get());
        for _ in 0..30 {
            app.advance_clock(SPIN_STEP);
            app.run_until_parked();
        }
        assert_eq!(piece_drawn.get(), pieces, "the piece kept its drawing");
        let steps = frame_drawn.get() - frames;
        assert!((29..=31).contains(&steps), "the layer turned {steps} times in 30 steps");
    }

    /// A view a piece puts on screen, as a page in a tab, with a working agent's dot in it.
    struct Page;

    impl Render for Page {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let id = window.window_handle().window_id();
            slots::drawing_piece(slots::Piece::View(cx.entity_id()), id, || {
                div().child(view_status_dot_slot(AgentState::Working, "agent"))
            })
        }
    }

    /// The window: the page while its tab is `shown`, kept as last drawn, then the layers.
    struct Tabs {
        page: gpui::Entity<Page>,
        shown: bool,
    }

    impl Render for Tabs {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let layer = spinner_layer(window);
            if self.shown {
                slots::place_view(self.page.entity_id(), window.window_handle().window_id());
            }
            div()
                .size_full()
                .when(self.shown, |tabs| {
                    tabs.child(
                        self.page
                            .clone()
                            .cached(gpui::StyleRefinement::default().size(px(20.0))),
                    )
                })
                .child(view_spinner_layer(window))
                .child(layer)
                .child(spinner_host::spinner_sync(window))
        }
    }

    #[test]
    fn a_page_switched_away_from_leaves_no_ring_behind_and_has_it_back_when_shown() {
        let mut app = TestApp::new();
        let mut window = app.open_window(|_, cx| Tabs {
            page: cx.new(|_| Page),
            shown: true,
        });
        window.draw();
        let rings = |window: &mut gpui::TestAppWindow<Tabs>| {
            window.update(|_, window, _| slots::shown(window.window_handle().window_id(), VIEW_SPINNERS).len())
        };
        assert_eq!(rings(&mut window), 1, "the page's ring turns in the layer");
        let show = |window: &mut gpui::TestAppWindow<Tabs>, shown: bool| {
            window.update(|tabs, _, cx| {
                tabs.shown = shown;
                cx.notify();
            });
            window.draw();
        };
        show(&mut window, false);
        assert_eq!(rings(&mut window), 0, "no ring where the page was");
        show(&mut window, true);
        assert_eq!(rings(&mut window), 1, "the ring is back with the page");
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

    /// A stand-in for what turns the spinners over the window's drawing (`native_spinners::Host`): where it shows each,
    /// and how many times it was asked to put one.
    #[derive(Clone, Default)]
    struct Over {
        shown: Rc<RefCell<HashMap<spinner_host::Key, spinner_host::Placed>>>,
        placings: Rc<Cell<usize>>,
    }

    impl spinner_host::Present for Over {
        fn place(&mut self, key: &spinner_host::Key, placed: &spinner_host::Placed, _: f64) {
            self.shown.borrow_mut().insert(key.clone(), *placed);
            self.placings.set(self.placings.get() + 1);
        }

        fn remove(&mut self, key: &spinner_host::Key) {
            self.shown.borrow_mut().remove(key);
        }
    }

    /// Has `over` turn the window's spinners, as `turn_spinners_natively` has the layer over GPUI's drawing.
    fn turn_over<V: Render + 'static>(window: &mut gpui::TestAppWindow<V>, over: &Over) {
        let present = over.clone();
        window.update(move |_, window, _| {
            spinner_host::present_with(window.window_handle().window_id(), Box::new(present))
        });
        window.draw();
    }

    /// A piece that keeps its last drawing, with an agent's dot in it: a spinner while it works.
    struct Agent {
        working: bool,
    }

    impl Render for Agent {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let state = if self.working { AgentState::Working } else { AgentState::Idle };
            slots::drawing_piece("piece", window.window_handle().window_id(), || {
                div().child(status_dot_slot(state, "agent"))
            })
        }
    }

    /// The window as the workspace draws it with its spinners turned over its drawing: the piece, cached, the spinner
    /// layer, an overlay where `cover` says, and the hand-over, last; counting its own draws.
    struct Hosted {
        piece: gpui::Entity<Agent>,
        spinners: gpui::Entity<SpinnerLayer>,
        cover: Option<Bounds<Pixels>>,
        drawn: Rc<Cell<usize>>,
    }

    impl Render for Hosted {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.drawn.set(self.drawn.get() + 1);
            slots::start_draw(window.window_handle().window_id());
            div()
                .size_full()
                .child(self.piece.clone().cached(gpui::StyleRefinement::default().size(px(20.0))))
                .child(SpinnerLayer::in_draw(&self.spinners))
                .children(self.cover.map(|at| {
                    slots::cover(div().absolute().left(at.origin.x).top(at.origin.y).w(at.size.width).h(at.size.height))
                }))
                .child(spinner_host::spinner_sync(window))
        }
    }

    /// A window with a working agent's spinner, turned by `over` from the start, and an overlay at `cover`, once the step
    /// its first draw asked for, before `over` turned its spinners, has passed. Counts the window's draws.
    fn hosted(cover: Option<Bounds<Pixels>>) -> (TestApp, gpui::TestAppWindow<Hosted>, Over, Rc<Cell<usize>>) {
        let mut app = TestApp::new();
        let drawn = Rc::new(Cell::new(0));
        let counter = drawn.clone();
        let mut window = app.open_window(move |_, cx| Hosted {
            piece: cx.new(|_| Agent { working: true }),
            spinners: cx.new(|_| SpinnerLayer::new(Spinners::Window)),
            cover,
            drawn: counter,
        });
        let over = Over::default();
        turn_over(&mut window, &over);
        app.advance_clock(SPIN_STEP);
        app.run_until_parked();
        (app, window, over, drawn)
    }

    /// The window's draws in 30 of the spinners' steps.
    fn thirty_steps(app: &TestApp, drawn: &Rc<Cell<usize>>) -> usize {
        let before = drawn.get();
        for _ in 0..30 {
            app.advance_clock(SPIN_STEP);
            app.run_until_parked();
        }
        drawn.get() - before
    }

    /// It drew the window 30 times a second while an agent worked, only to turn the spinners: about a quarter of a core
    /// while Claude worked.
    #[test]
    fn a_spinner_turned_over_the_window_draws_no_frame_of_it() {
        let (app, mut window, over, drawn) = hosted(None);
        let frames = thirty_steps(&app, &drawn);
        eprintln!("[measure] 30 steps of a spinner turned over the window: {frames} draws of the window");
        assert_eq!(frames, 0, "no draw of the window");
        let slot = window.update(|_, window, _| {
            slots::shown(window.window_handle().window_id(), SPINNERS).pop().expect("the spinner's slot").1
        });
        let shown = over.shown.borrow().clone();
        let key: spinner_host::Key = (slots::Piece::Named("piece"), SPINNERS, "agent".into());
        assert_eq!(shown.len(), 1, "one spinner over the window");
        assert_eq!(shown[&key].bounds, slot.bounds, "at its slot");
        assert_eq!(shown[&key].bounds.size, gpui::size(px(10.0), px(10.0)));
        // The window drawn again for something else: the spinner stays as it was, turning, not put there again.
        for _ in 0..3 {
            window.update(|_, _, cx| cx.notify());
            window.draw();
        }
        assert_eq!(over.placings.get(), 1, "put there once");
    }

    #[test]
    fn an_overlay_over_a_spinner_has_the_window_turn_it_under_the_overlay_until_it_goes() {
        let over_it = Bounds::new(point(px(5.0), px(5.0)), gpui::size(px(50.0), px(50.0)));
        let (app, mut window, over, drawn) = hosted(Some(over_it));
        assert!(over.shown.borrow().is_empty(), "not over the overlay");
        let frames = thirty_steps(&app, &drawn);
        assert!((29..=31).contains(&frames), "the window turns it, under the overlay: {frames} draws");
        window.update(|hosted, _, cx| {
            hosted.cover = None;
            cx.notify();
        });
        window.draw();
        app.run_until_parked();
        assert_eq!(over.shown.borrow().len(), 1, "over the window again once the overlay is gone");
        // The step the window's last turn asked for.
        app.advance_clock(SPIN_STEP);
        app.run_until_parked();
        assert_eq!(thirty_steps(&app, &drawn), 0, "and no draw of the window");
    }

    #[test]
    fn an_overlay_elsewhere_leaves_a_spinner_over_the_window() {
        let elsewhere = Bounds::new(point(px(100.0), px(100.0)), gpui::size(px(20.0), px(20.0)));
        let (app, _window, over, drawn) = hosted(Some(elsewhere));
        assert_eq!(over.shown.borrow().len(), 1);
        assert_eq!(thirty_steps(&app, &drawn), 0);
    }

    #[test]
    fn a_spinner_whose_piece_draws_without_it_goes_from_over_the_window() {
        let (app, mut window, over, _) = hosted(None);
        assert_eq!(over.shown.borrow().len(), 1);
        window.update(|hosted, _, cx| {
            hosted.piece.update(cx, |agent, cx| {
                agent.working = false;
                cx.notify();
            })
        });
        app.run_until_parked();
        window.draw();
        assert!(over.shown.borrow().is_empty(), "the agent stopped: no ring over the window");
    }

    #[test]
    fn a_page_switched_away_from_leaves_no_ring_over_the_window_and_has_it_back_when_shown() {
        let mut app = TestApp::new();
        let mut window = app.open_window(|_, cx| Tabs {
            page: cx.new(|_| Page),
            shown: true,
        });
        let over = Over::default();
        turn_over(&mut window, &over);
        let page = window.read(|tabs, _| tabs.page.entity_id());
        let key: spinner_host::Key = (slots::Piece::View(page), VIEW_SPINNERS, "agent".into());
        assert!(over.shown.borrow().contains_key(&key), "the page's ring over the window");
        let show = |window: &mut gpui::TestAppWindow<Tabs>, shown: bool| {
            window.update(|tabs, _, cx| {
                tabs.shown = shown;
                cx.notify();
            });
            window.draw();
        };
        show(&mut window, false);
        assert!(over.shown.borrow().is_empty(), "no ring where the page was");
        show(&mut window, true);
        assert!(over.shown.borrow().contains_key(&key), "the ring is back with the page");
        app.run_until_parked();
    }
}
