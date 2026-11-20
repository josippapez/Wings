//! A slow drift of dithered pixels behind a page that asks for it (`backdrop: "pixels"` on its `Page`), over all of the
//! card that holds it. A grid of square cells, each shaded from a few slow waves of where it is and when, cut into a few
//! shades of the page's ground with a hint of the Claude orange and the info blue, so text on it reads as on the plain
//! ground. Cells of one shade side by side in a row are painted as one quad, and the ground's shade not at all.
//!
//! The card leaves a slot for it (`backdrop_slot`) and paints no ground of its own. The window's `BackdropLayer`, drawn
//! before the piece with the card, paints in the slot. The layer is a view of its own that asks to be drawn again at
//! each step, so a step draws the window's root and the layer again while the main column and the page keep their last
//! drawing: GPUI marks a view and the views above it dirty (`Window::mark_view_dirty`), and only the root is above the
//! layer. It steps 15 times a second on the spinners' clock, on every other spinner step, only while the window is in
//! front and a slot shows, and stands still under Reduce motion.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

use gpui::{
    App, Bounds, Context, Corners, EntityId, Hsla, IntoElement, Pixels, Render, SharedString, Window, WindowId, canvas,
    div, fill, point, prelude::*, px, rgba, size,
};

use super::{slots, status_dot};
use crate::theme::{self, Palette};

/// The layer the backdrops draw in (`slots`).
const LAYER: &str = "backdrop";

/// How wide and tall a cell is, in points.
pub(crate) const CELL: f32 = 12.0;

/// How long a step of the drift lasts: 15 a second, every other step of the spinners' 30.
pub(crate) const STEP: Duration = Duration::from_nanos(1_000_000_000 / 15);

/// How many shades a cell can be: two darker than the ground, the ground itself (`BASE`), then the orange and the blue,
/// each faint and fainter.
pub(crate) const SHADES: usize = 7;

/// The ground's shade, which the layer paints once under the cells and no cell paints again.
pub(crate) const BASE: u8 = 2;

/// The 4x4 ordered dither: where in a band's edge a cell falls, so two bands meet in a checker of cells, not a line.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Where the waves' bands change, darkest to brightest, on their 0 to 1.
const EDGES: [f32; 4] = [0.30, 0.42, 0.58, 0.70];

/// Each shade's colour over the ground, in `palette`: the window's darker ground strong and light, the ground, then the
/// Claude orange and the info blue, each at 5% and 9%.
fn colors(palette: &Palette) -> [Hsla; SHADES] {
    let over = |color: u32, alpha: u32| -> Hsla { rgba(color << 8 | alpha).into() };
    [
        over(palette.background, 0xd9),
        over(palette.background, 0x73),
        over(palette.surface, 0xff),
        over(theme::CLAUDE, 0x0d),
        over(theme::CLAUDE, 0x17),
        over(palette.info, 0x0d),
        over(palette.info, 0x17),
    ]
}

/// The shade of the cell `col` across and `row` down, `t` seconds into the drift: a sum of slow waves cut into five
/// bands, dithered at their edges. The two brightest bands are warm or cool by a broader, slower wave.
pub(crate) fn shade(col: u32, row: u32, t: f32) -> u8 {
    let (x, y) = (col as f32, row as f32);
    // Rings out of a centre that wanders over the first 70 by 45 cells or so.
    let (cx, cy) = (34.0 + 22.0 * (t * 0.029).sin(), 22.0 + 14.0 * (t * 0.037).cos());
    let waves = (x * 0.067 + t * 0.07).sin()
        + (y * 0.089 - t * 0.05).sin()
        + ((x - y) * 0.047 + t * 0.04).sin()
        + ((x - cx).hypot(y - cy) * 0.16 - t * 0.12).sin();
    let dither = (f32::from(BAYER[(row % 4) as usize][(col % 4) as usize]) + 0.5) / 16.0 - 0.5;
    let value = waves / 8.0 + 0.5 + dither * 0.06;
    let band = EDGES.iter().filter(|edge| value >= **edge).count() as u8;
    if band <= BASE {
        return band;
    }
    let warm = (x * 0.019 + y * 0.023 + t * 0.017).sin() > 0.0;
    if warm { band } else { band + 2 }
}

/// The runs of one shade in a row of cells' shades, by their first cell and the one past their last, leaving out the
/// ground's.
pub(crate) fn runs_of(shades: &[u8]) -> Vec<(usize, usize, u8)> {
    let mut runs = Vec::new();
    let mut start = 0;
    for at in 1..=shades.len() {
        if at == shades.len() || shades[at] != shades[start] {
            if shades[start] != BASE {
                runs.push((start, at, shades[start]));
            }
            start = at;
        }
    }
    runs
}

/// How far in from each side a row from `top` to `bottom` starts and ends, in a box `height` tall whose corners are
/// `radius` round, so no cell shows past the curve: how deep the curve is at the row's edge nearest a corner.
pub(crate) fn corner_inset(top: f32, bottom: f32, height: f32, radius: f32) -> f32 {
    let depth = |from_edge: f32| {
        if from_edge >= radius {
            return 0.0;
        }
        let dy = radius - from_edge;
        radius - (radius * radius - dy * dy).max(0.0).sqrt()
    };
    depth(top).max(depth(height - bottom))
}

/// Cells of one shade side by side in a row, in points from the slot's top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Run {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub shade: u8,
}

/// What to paint over the ground of a slot `width` by `height` points whose corners are `radius` round, at step `step`
/// of the drift. The cells start at the slot's top left corner, which doesn't scroll, so they stay where they are.
pub(crate) fn runs_in(width: f32, height: f32, radius: f32, step: u64) -> Vec<Run> {
    let t = step as f32 * STEP.as_secs_f32();
    let (cols, rows) = ((width / CELL).ceil().max(0.0) as u32, (height / CELL).ceil().max(0.0) as u32);
    let mut shades = Vec::with_capacity(cols as usize);
    let mut runs = Vec::new();
    for row in 0..rows {
        let top = row as f32 * CELL;
        let bottom = (top + CELL).min(height);
        let inset = corner_inset(top, bottom, height, radius);
        shades.clear();
        shades.extend((0..cols).map(|col| shade(col, row, t)));
        for (start, end, shade) in runs_of(&shades) {
            let (left, right) = ((start as f32 * CELL).max(inset), (end as f32 * CELL).min(width - inset));
            if right > left {
                runs.push(Run { x: left, y: top, width: right - left, height: bottom - top, shade });
            }
        }
    }
    runs
}

/// What a slot's backdrop is painted in: the page's palette, and how round the corners of what holds it are.
#[derive(Clone, Copy)]
struct Look {
    palette: Palette,
    radius: f32,
}

thread_local! {
    /// Each slot's look, by window and slot, as the piece that left the slot last drew it.
    static LOOKS: RefCell<HashMap<(WindowId, SharedString), Look>> = RefCell::default();
    /// The layers that already asked to be drawn again at a step, by the step.
    static WAKING: RefCell<HashSet<(EntityId, u64)>> = RefCell::default();
}

/// Where the backdrop goes, in a piece drawn with `drawing_piece`: all of what holds it, whose corners are `radius`
/// round, painted in `palette`. What holds it paints no ground of its own: the window's `BackdropLayer` paints here.
pub fn backdrop_slot(name: impl Into<SharedString>, palette: Palette, radius: f32) -> impl IntoElement {
    let name = name.into();
    let key = name.clone();
    let look = Look { palette, radius };
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(slots::slot(LAYER, name, None))
        .child(
            canvas(
                move |_, window, _| {
                    let id = window.window_handle().window_id();
                    LOOKS.with_borrow_mut(|looks| looks.insert((id, key), look));
                },
                |_, _, _, _| {},
            )
            .absolute(),
        )
}

/// A slot's runs as last worked out, and what for: the step and the slot's width, height and corners.
struct Painted {
    name: SharedString,
    key: (u64, u32, u32, u32),
    runs: Rc<Vec<Run>>,
}

/// The window's backdrops, in the slots its pieces leave for them (`backdrop_slot`). Made before the window first draws,
/// and drawn in each draw of the window before the pieces with the slots, over the window's own ground.
#[derive(Default)]
pub struct BackdropLayer {
    /// What each slot was last painted with, so a draw of the window between two steps (a spinner's, a terminal's) paints
    /// the same runs again without working them out.
    painted: Rc<RefCell<Vec<Painted>>>,
    /// How many times it drew, for tests of what draws again.
    #[cfg(test)]
    pub(crate) drawn: usize,
}

impl Render for BackdropLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.drawn += 1;
        }
        let painted = self.painted.clone();
        // Painted once every piece has been laid out in this draw, so it finds each slot where it is now.
        canvas(|_, _, _| {}, move |_, _, window, cx| paint_slots(&painted, window, cx))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
    }
}

fn paint_slots(painted: &RefCell<Vec<Painted>>, window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let shown = slots::shown(id, LAYER);
    let showing = |name: &SharedString| shown.iter().any(|(at, _)| at == name);
    LOOKS.with_borrow_mut(|looks| looks.retain(|(window, name), _| *window != id || showing(name)));
    let mut painted = painted.borrow_mut();
    painted.retain(|was| showing(&was.name));
    if shown.is_empty() {
        return;
    }
    let step = step(window, cx);
    for (name, slot) in &shown {
        let Some(look) = LOOKS.with_borrow(|looks| looks.get(&(id, name.clone())).copied()) else {
            continue;
        };
        let (width, height) = (f32::from(slot.bounds.size.width), f32::from(slot.bounds.size.height));
        let key = (step, width.to_bits(), height.to_bits(), look.radius.to_bits());
        let runs = match painted.iter().find(|was| was.name == *name) {
            Some(was) if was.key == key => was.runs.clone(),
            _ => {
                let runs = Rc::new(runs_in(width, height, look.radius, step));
                painted.retain(|was| was.name != *name);
                painted.push(Painted { name: name.clone(), key, runs: runs.clone() });
                runs
            }
        };
        window.with_content_mask(Some(slot.mask), |window| paint(slot.bounds, &look, &runs, window));
    }
}

/// The ground of a slot at `bounds`, at the corners of what holds it, and the runs over it.
fn paint(bounds: Bounds<Pixels>, look: &Look, runs: &[Run], window: &mut Window) {
    let colors = colors(&look.palette);
    window.paint_quad(fill(bounds, colors[BASE as usize]).corner_radii(Corners::all(px(look.radius))));
    for run in runs {
        let origin = point(bounds.origin.x + px(run.x), bounds.origin.y + px(run.y));
        window.paint_quad(fill(Bounds::new(origin, size(px(run.width), px(run.height))), colors[run.shade as usize]));
    }
}

/// The step of the drift to paint, and a draw of the layer at the next while the window is in front. The first under
/// Reduce motion, where it stands still.
fn step(window: &mut Window, cx: &mut App) -> u64 {
    if cx.reduce_motion() {
        return 0;
    }
    let elapsed = status_dot::clock(cx);
    let step = (elapsed.as_nanos() / STEP.as_nanos()) as u64;
    if window.is_window_active() {
        wake_at(step + 1, elapsed, window, cx);
    }
    step
}

/// Draws the layer being drawn again as step `next` starts, `elapsed` into the clock, unless it already asked to.
fn wake_at(next: u64, elapsed: Duration, window: &mut Window, cx: &mut App) {
    let view = window.current_view();
    if !WAKING.with_borrow_mut(|waking| waking.insert((view, next))) {
        return;
    }
    let delay = Duration::from_nanos(next * STEP.as_nanos() as u64).saturating_sub(elapsed);
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(delay).await;
            WAKING.with_borrow_mut(|waking| waking.remove(&(view, next)));
            cx.update(|_, cx| cx.notify(view)).ok();
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shades of a grid of cells `t` seconds in, row by row.
    fn grid(t: f32) -> Vec<u8> {
        (0..60).flat_map(|row| (0..84).map(move |col| shade(col, row, t))).collect()
    }

    #[test]
    fn a_cell_is_one_of_a_few_shades_and_the_same_one_each_time_it_is_asked() {
        let cells = grid(12.5);
        assert_eq!(cells, grid(12.5));
        assert!(cells.iter().all(|shade| usize::from(*shade) < SHADES));
        let seen: HashSet<u8> = cells.iter().copied().collect();
        assert!(seen.len() >= 4, "a pattern, not a flat ground: {seen:?}");
    }

    #[test]
    fn a_step_of_the_drift_changes_few_cells() {
        let (now, next) = (grid(30.0), grid(30.0 + STEP.as_secs_f32()));
        let moved = now.iter().zip(&next).filter(|(a, b)| a != b).count();
        assert!(moved * 10 < now.len(), "{moved} of {} cells changed in one step", now.len());
    }

    #[test]
    fn cells_of_one_shade_side_by_side_are_one_run_and_the_grounds_are_left_out() {
        assert_eq!(runs_of(&[BASE, BASE, 3, 3, 3, BASE, 0, 0, 4]), vec![(2, 5, 3), (6, 8, 0), (8, 9, 4)]);
        assert_eq!(runs_of(&[1, 1]), vec![(0, 2, 1)]);
        assert_eq!(runs_of(&[BASE; 5]), vec![]);
        assert_eq!(runs_of(&[]), vec![]);
        // A slot's runs are fewer than its cells off the ground.
        let (width, height) = (1000.0, 760.0);
        let runs = runs_in(width, height, 13.0, 300);
        let cells: usize = runs.iter().map(|run| (run.width / CELL).ceil() as usize).sum();
        assert!(runs.len() < cells, "{} runs for {cells} cells", runs.len());
    }

    #[test]
    fn no_cell_shows_past_the_rounded_corners_of_what_holds_it() {
        let (width, height, radius) = (300.0, 200.0, 13.0);
        // A point is in the rounded box when it is within `radius` of the box its corners' centres make.
        let inside = |x: f32, y: f32| {
            let (cx, cy) = (x.clamp(radius, width - radius), y.clamp(radius, height - radius));
            (x - cx).hypot(y - cy) <= radius + 0.001
        };
        let runs: Vec<Run> = (0..40).flat_map(|step| runs_in(width, height, radius, step * 50)).collect();
        assert!(runs.iter().any(|run| run.y == 0.0), "the top row has cells");
        for run in &runs {
            let (right, bottom) = (run.x + run.width, run.y + run.height);
            for (x, y) in [(run.x, run.y), (right, run.y), (run.x, bottom), (right, bottom)] {
                assert!(inside(x, y), "{run:?} shows past a corner at ({x}, {y})");
            }
        }
        assert_eq!(corner_inset(96.0, 108.0, height, radius), 0.0, "a row away from the corners runs edge to edge");
        assert_eq!(corner_inset(0.0, 12.0, height, radius), radius);
        assert_eq!(corner_inset(188.0, 200.0, height, radius), radius);
    }
}

#[cfg(all(test, feature = "test-support"))]
mod drawing {
    use std::cell::Cell;

    use gpui::{Entity, StyleRefinement, TestApp, TestAppWindow};

    use super::*;

    /// A piece that keeps its last drawing, with a backdrop's slot over all of it or none, counting its draws.
    struct Piece {
        slot: bool,
        drawn: Rc<Cell<usize>>,
    }

    impl Render for Piece {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.drawn.set(self.drawn.get() + 1);
            let slot = self.slot;
            slots::drawing_piece("piece", window.window_handle().window_id(), || {
                div()
                    .relative()
                    .size_full()
                    .when(slot, |piece| piece.child(backdrop_slot("tab", Palette::APP, 13.0)))
            })
        }
    }

    /// The window, as the workspace's: the layer, then the piece over it, cached.
    struct Frame {
        layer: Entity<BackdropLayer>,
        piece: Entity<Piece>,
    }

    impl Render for Frame {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            slots::start_draw(window.window_handle().window_id());
            div()
                .relative()
                .size_full()
                .child(self.layer.clone())
                .child(self.piece.clone().cached(StyleRefinement::default().size_full()))
        }
    }

    fn open(slot: bool, active: bool) -> (TestApp, TestAppWindow<Frame>, Rc<Cell<usize>>) {
        let mut app = TestApp::new();
        let drawn = Rc::new(Cell::new(0));
        let counter = drawn.clone();
        let mut window = app.open_window(move |_, cx| Frame {
            layer: cx.new(|_| BackdropLayer::default()),
            piece: cx.new(|_| Piece { slot, drawn: counter }),
        });
        if active {
            window.update(|_, window, _| window.activate_window());
        }
        app.run_until_parked();
        (app, window, drawn)
    }

    /// How many times the layer and the piece drew in 30 steps of the drift.
    fn thirty_steps(app: &TestApp, window: &mut TestAppWindow<Frame>, piece: &Rc<Cell<usize>>) -> (usize, usize) {
        window.draw();
        let layer = |window: &TestAppWindow<Frame>| window.read(|frame, cx| frame.layer.read(cx).drawn);
        let (layers, pieces) = (layer(&*window), piece.get());
        for _ in 0..30 {
            app.advance_clock(STEP);
            app.run_until_parked();
        }
        (layer(&*window) - layers, piece.get() - pieces)
    }

    #[test]
    fn a_step_of_the_drift_draws_the_layer_and_not_the_piece_it_is_in() {
        let (app, mut window, piece) = open(true, true);
        let (layers, pieces) = thirty_steps(&app, &mut window, &piece);
        assert_eq!(pieces, 0, "the piece kept its drawing");
        assert!((29..=31).contains(&layers), "the layer drew {layers} times in 30 steps");
    }

    #[test]
    fn the_drift_stands_still_behind_other_apps_under_reduce_motion_and_without_a_slot() {
        let (app, mut window, piece) = open(true, false);
        assert_eq!(thirty_steps(&app, &mut window, &piece).0, 0, "no step while the window is behind another app's");

        let mut app = TestApp::new();
        app.update(|cx| cx.set_reduce_motion(true));
        let drawn = Rc::new(Cell::new(0));
        let counter = drawn.clone();
        let mut window = app.open_window(move |_, cx| Frame {
            layer: cx.new(|_| BackdropLayer::default()),
            piece: cx.new(|_| Piece { slot: true, drawn: counter }),
        });
        window.update(|_, window, _| window.activate_window());
        app.run_until_parked();
        assert_eq!(thirty_steps(&app, &mut window, &drawn).0, 0, "drawn once and still under Reduce motion");

        let (app, mut window, piece) = open(false, true);
        assert_eq!(thirty_steps(&app, &mut window, &piece).0, 0, "nothing to draw while no slot shows, as with the tab hidden");
    }
}
