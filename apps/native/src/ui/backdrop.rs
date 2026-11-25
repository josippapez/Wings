//! A slow pattern of dithered pixels behind a page that asks for it (`backdrop: "pixels"` or `"pixels:<pattern>"` on its
//! `Page`), over all of the card that holds it. A grid of square cells, each one of a few shades of the page's ground
//! with a hint of the Claude orange and the info blue by where it is and when (`pixels`), so text on it reads as on the
//! plain ground. Cells of one shade side by side in a row are painted as one quad, and the ground's shade not at all.
//!
//! The card leaves a slot for it (`backdrop_slot`) and paints no ground of its own. The window's `BackdropLayer`, drawn
//! before the piece with the card, paints in the slot. The layer is a view of its own that asks to be drawn again at
//! each step, so a step draws the window's root and the layer again while the main column and the page keep their last
//! drawing: GPUI marks a view and the views above it dirty (`Window::mark_view_dirty`), and only the root is above the
//! layer. It steps 15 times a second on the spinners' clock, on every other spinner step, only while the window is in
//! front and a slot shows, and stands still under Reduce motion.
//!
//! A page can ask for a gradient instead (`backdrop: "gradient:<preset>"`, `gradients.rs`): a Metal shader draws each
//! step into a frame of its own (`shader`), which the layer draws as a GPUI surface on the same steps. Where Metal
//! can't be set up, the slot keeps the plain ground.
//!
//! That is how a window draws them on its own. The app's window draws them in a layer of Wings' own under GPUI's
//! drawing of the window instead (`native_backdrop`, `draw_backdrops_under`), so a step is one Metal draw into that
//! layer and no draw of the window at all. The window's glass (`window_glass`) leaves a hole where the slot is, and the
//! window paints nothing of the backdrop. The layer moves only in a draw of the window, which is the only time the slot
//! can move. A slot that appears shows the plain ground for a draw while its layer gets its first drawing, a slot that
//! goes keeps its layer a frame longer, and a slot that moves has a layer over where it was and where it is for a frame,
//! so the window's drawing and the layer, which reach the screen apart, never leave the desktop showing through the
//! hole. Windows without the layer (offscreen ones, tests, or where it can't be set up) paint the backdrops as above.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, Corners, EntityId, Hsla, IntoElement, PaintQuad, Pixels, Render,
    SharedString, Window, WindowId, canvas, div, fill, point, prelude::*, px, quad, rgba, size, transparent_black,
};

use super::{live, slots};
use crate::{
    gradients::{self, Preset, Uniforms},
    page::tree::Backdrop,
    pixels::{BASE, Pattern, RAMP, SHADES, Tone},
    theme::{self, Palette},
};

/// The layer the backdrops draw in (`slots`).
const LAYER: &str = "backdrop";

/// How long a step of the drift lasts: 15 a second, every other step of the spinners' 30.
pub(crate) const STEP: Duration = Duration::from_nanos(1_000_000_000 / 15);

/// Halfway between two 0xRRGGBB colours.
fn halfway(a: u32, b: u32) -> u32 {
    let channel = |shift: u32| (((a >> shift) & 0xff) + ((b >> shift) & 0xff)) / 2 << shift;
    channel(16) | channel(8) | channel(0)
}

/// The colours of the warm and the cool shades of a `tone` in `palette`.
fn accents(palette: &Palette, tone: Tone) -> (u32, u32) {
    match tone {
        Tone::Claude => (theme::CLAUDE, palette.info),
        Tone::Mono => (palette.foreground, palette.foreground),
        Tone::Ocean => (palette.info, palette.done),
        Tone::Meadow => (palette.done, palette.working),
        Tone::Dusk => (palette.danger, halfway(palette.danger, palette.info)),
        // A ramp has its own colours (`ramp`), not the palette's.
        Tone::Aurora | Tone::Lagoon => (palette.foreground, palette.foreground),
    }
}

/// The six steps of a ramp's tone as 0xRRGGBB and 0 to 255, deepest to brightest, the colours the tone's pattern lays over
/// the ground (`pixels::RAMP`): up to about 18% of colour at the brightest, with the muted text still 4.5:1 on each.
fn ramp(tone: Tone) -> Option<[(u32, u32); 6]> {
    match tone {
        Tone::Aurora => Some([
            (0x2a3cff, 0x30),
            (0x5b52ff, 0x34),
            (0x9a58f0, 0x34),
            (0xd35ccb, 0x30),
            (0xff6f91, 0x2c),
            (0xff9d5c, 0x2c),
        ]),
        Tone::Lagoon => Some([
            (0x0e9a8f, 0x30),
            (0x2dd4bf, 0x28),
            (0x38bdf8, 0x2c),
            (0x6366f1, 0x34),
            (0x8b5cf6, 0x34),
            (0xc084fc, 0x30),
        ]),
        _ => None,
    }
}

/// Each shade's colour over the ground and how strong, as 0xRRGGBB and 0 to 255, in `palette`: the window's darker
/// ground strong and light, the ground, then the `tone`'s warm and cool colours, each at 5% and 9%.
fn tints(palette: &Palette, tone: Tone) -> [(u32, u32); SHADES] {
    if let Some(steps) = ramp(tone) {
        let mut tints = [(palette.surface, 0xff); SHADES];
        for (step, shade) in steps.into_iter().zip(RAMP) {
            tints[shade as usize] = step;
        }
        return tints;
    }
    let (warm, cool) = accents(palette, tone);
    [
        (palette.background, 0xd9),
        (palette.background, 0x73),
        (palette.surface, 0xff),
        (warm, 0x0d),
        (warm, 0x17),
        (cool, 0x0d),
        (cool, 0x17),
    ]
}

fn colors(palette: &Palette, tone: Tone) -> [Hsla; SHADES] {
    tints(palette, tone).map(|(color, alpha)| rgba(color << 8 | alpha).into())
}

/// Each shade's colour laid over the ground, opaque, as sRGB 0 to 1: what the window makes of a cell it paints over the
/// ground (`colors`), as GPUI blends sRGB values as they are, for a layer that draws the cells itself.
fn opaque_colors(palette: &Palette, tone: Tone) -> [[f32; 4]; SHADES] {
    let channels = |color: u32| [(color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff].map(|c| c as f32 / 255.0);
    let ground = channels(palette.surface);
    tints(palette, tone).map(|(color, alpha)| {
        let (color, alpha) = (channels(color), alpha as f32 / 255.0);
        let [r, g, b] = [0, 1, 2].map(|i| ground[i] + (color[i] - ground[i]) * alpha);
        [r, g, b, 1.0]
    })
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
/// of `pattern`. The cells start at the slot's top left corner, which doesn't scroll, so they stay where they are.
pub(crate) fn runs_in(pattern: Pattern, width: f32, height: f32, radius: f32, step: u64) -> Vec<Run> {
    let t = step as f32 * STEP.as_secs_f32();
    let cell = pattern.cell();
    let (cols, rows) = ((width / cell).ceil().max(0.0) as u32, (height / cell).ceil().max(0.0) as u32);
    let mut shades = Vec::with_capacity(cols as usize);
    let mut runs = Vec::new();
    for row in 0..rows {
        let top = row as f32 * cell;
        let bottom = (top + cell).min(height);
        let inset = corner_inset(top, bottom, height, radius);
        shades.clear();
        shades.extend((0..cols).map(|col| pattern.shade(col, row, t)));
        for (start, end, shade) in runs_of(&shades) {
            let (left, right) = ((start as f32 * cell).max(inset), (end as f32 * cell).min(width - inset));
            if right > left {
                runs.push(Run { x: left, y: top, width: right - left, height: bottom - top, shade });
            }
        }
    }
    runs
}

/// What a slot's backdrop is: a pattern of pixels or a gradient, the page's palette, and how round the corners of what holds it
/// are.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    kind: Backdrop,
    palette: Palette,
    radius: f32,
}

thread_local! {
    /// Each slot's look, by window and slot, as the piece that left the slot last drew it.
    static LOOKS: RefCell<HashMap<(WindowId, SharedString), Look>> = RefCell::default();
    /// The layers that already asked to be drawn again at a step, by the step.
    static WAKING: RefCell<HashSet<(EntityId, u64)>> = RefCell::default();
}

/// Where the backdrop `kind` goes, in a piece drawn with `drawing_piece`: all of what holds it, whose corners are `radius`
/// round, painted in `palette`. What holds it paints no ground of its own: the window's `BackdropLayer` paints here.
pub fn backdrop_slot(name: impl Into<SharedString>, kind: Backdrop, palette: Palette, radius: f32) -> impl IntoElement {
    let name = name.into();
    let key = name.clone();
    let look = Look { kind, palette, radius };
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

/// A slot's runs as last worked out, and what for: the step, the slot's width and height, and its look.
struct Painted {
    name: SharedString,
    key: (u64, u32, u32, Look),
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

/// The slots of the window `id` that show, with the looks the pieces that left them drew.
fn looks_of(id: WindowId, shown: &[(SharedString, slots::Slot)]) -> Vec<(&SharedString, &slots::Slot, Look)> {
    shown
        .iter()
        .filter_map(|(name, slot)| Some((name, slot, LOOKS.with_borrow(|looks| looks.get(&(id, name.clone())).copied())?)))
        .collect()
}

/// Which of the slots that show a window's native layer shows: the first by name. The window paints any other.
fn pick(looks: &[(&SharedString, &slots::Slot, Look)]) -> Option<usize> {
    (0..looks.len()).min_by(|a, b| looks[*a].0.cmp(looks[*b].0))
}

fn paint_slots(painted: &RefCell<Vec<Painted>>, window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let shown = slots::shown(id, LAYER);
    let showing = |name: &SharedString| shown.iter().any(|(at, _)| at == name);
    LOOKS.with_borrow_mut(|looks| looks.retain(|(window, name), _| *window != id || showing(name)));
    let mut painted = painted.borrow_mut();
    painted.retain(|was| showing(&was.name));
    SHADER.with_borrow_mut(|shader| shader.forget(&|(window, name): &SlotKey| *window != id || showing(name)));
    let mut looks = looks_of(id, &shown);
    let native = pick(&looks);
    match (present_native(id, native.map(|at| looks[at]), window, cx), native) {
        (InDraw::Ground, Some(at)) => {
            let (_, slot, look) = looks.remove(at);
            window.with_content_mask(Some(slot.mask), |window| paint(slot.bounds, &look, &[], window));
        }
        (InDraw::Layer, Some(at)) => {
            looks.remove(at);
        }
        _ => {}
    }
    // Pixels always move; a gradient only once its shader is set up, and never after it failed.
    let moving = looks.iter().any(|(_, _, look)| match look.kind {
        Backdrop::Pixels(_) => true,
        Backdrop::Gradient(_) => SHADER.with_borrow_mut(|shader| shader.usable()),
    });
    if !moving {
        for (_, slot, look) in &looks {
            window.with_content_mask(Some(slot.mask), |window| paint(slot.bounds, look, &[], window));
        }
        return;
    }
    let step = step(window, cx);
    for (name, slot, look) in looks {
        let pattern = match look.kind {
            Backdrop::Pixels(pattern) => pattern,
            Backdrop::Gradient(preset) => {
                let key = (id, name.clone());
                window.with_content_mask(Some(slot.mask), |window| paint_gradient(&key, slot, &look, preset, step, window));
                continue;
            }
        };
        let (width, height) = (f32::from(slot.bounds.size.width), f32::from(slot.bounds.size.height));
        let key = (step, width.to_bits(), height.to_bits(), look);
        let runs = match painted.iter().find(|was| was.name == *name) {
            Some(was) if was.key == key => was.runs.clone(),
            _ => {
                let runs = Rc::new(runs_in(pattern, width, height, look.radius, step));
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
    let colors = colors(&look.palette, tone_of(look.kind));
    window.paint_quad(fill(bounds, colors[BASE as usize]).corner_radii(Corners::all(px(look.radius))));
    for run in runs {
        let origin = point(bounds.origin.x + px(run.x), bounds.origin.y + px(run.y));
        window.paint_quad(fill(Bounds::new(origin, size(px(run.width), px(run.height))), colors[run.shade as usize]));
    }
}

/// The colours a backdrop's cells take; a gradient has no cells, so the plain ones.
fn tone_of(kind: Backdrop) -> Tone {
    match kind {
        Backdrop::Pixels(pattern) => pattern.tone(),
        Backdrop::Gradient(_) => Tone::Claude,
    }
}

/// A slot, by its window and name.
pub(crate) type SlotKey = (WindowId, SharedString);

#[cfg(target_os = "macos")]
pub(crate) type Frame = super::shader::Frame;
#[cfg(not(target_os = "macos"))]
pub(crate) type Frame = std::convert::Infallible;

/// What draws the gradients' frames: Metal (`shader::Metal`), or a stand-in in tests.
pub(crate) trait Shade {
    /// Starts drawing a frame of `uniforms` for `slot`, without waiting for it.
    fn render(&mut self, slot: &SlotKey, uniforms: &Uniforms) -> Result<(), String>;
    /// The newest frame of `slot` that is finished.
    fn latest(&mut self, slot: &SlotKey) -> Option<Frame>;
    /// Lets go of the frames of slots `keep` turns down.
    fn forget(&mut self, keep: &dyn Fn(&SlotKey) -> bool);
}

/// What a slot's newest frame was drawn for: the step, its size in the frame's pixels, and its look.
type Drawn = (u64, (u32, u32), Look);

enum Shader {
    /// Not needed yet: Metal is set up the first time a gradient shows.
    Untried,
    Ready(Box<dyn Shade>, HashMap<SlotKey, Drawn>),
    /// Metal couldn't be set up or draw, so gradients keep the plain ground. Said once, as it happened.
    Failed,
}

impl Shader {
    fn usable(&mut self) -> bool {
        if let Shader::Untried = self {
            *self = match make_shade() {
                Ok(shade) => Shader::Ready(shade, HashMap::new()),
                Err(why) => fail(&why),
            };
        }
        matches!(self, Shader::Ready(..))
    }

    fn forget(&mut self, keep: &dyn Fn(&SlotKey) -> bool) {
        if let Shader::Ready(shade, drawn) = self {
            shade.forget(keep);
            drawn.retain(|slot, _| keep(slot));
        }
    }

    /// The frame to paint in `slot`, drawn first if what it shows changed since its last.
    fn frame(&mut self, slot: &SlotKey, now: Drawn, uniforms: impl FnOnce() -> Uniforms) -> Option<Frame> {
        if !self.usable() {
            return None;
        }
        let Shader::Ready(shade, drawn) = self else { return None };
        if drawn.get(slot) != Some(&now) {
            if let Err(why) = shade.render(slot, &uniforms()) {
                *self = fail(&why);
                return None;
            }
            drawn.insert(slot.clone(), now);
        }
        shade.latest(slot)
    }
}

fn fail(why: &str) -> Shader {
    eprintln!("Gradient backgrounds are off, showing the plain ground: {why}");
    Shader::Failed
}

#[cfg(target_os = "macos")]
fn make_shade() -> Result<Box<dyn Shade>, String> {
    super::shader::Metal::new().map(|metal| Box::new(metal) as Box<dyn Shade>)
}

#[cfg(not(target_os = "macos"))]
fn make_shade() -> Result<Box<dyn Shade>, String> {
    Err("needs Metal".to_owned())
}

thread_local! {
    static SHADER: RefCell<Shader> = const { RefCell::new(Shader::Untried) };
}

/// Draws the gradients with `shade` in place of Metal, on this thread.
#[cfg(test)]
pub(crate) fn use_shade(shade: Box<dyn Shade>) {
    SHADER.set(Shader::Ready(shade, HashMap::new()));
}

/// The frame's pixels for each device pixel: half, as the mesh is smooth and the window draws it scaled up smoothly.
const RESOLUTION: f32 = 0.5;

/// The size of a frame for a slot `width` by `height` points at `scale` device pixels a point: even, as 4:2:0 halves
/// it, and at most 1600 wide or tall.
pub(crate) fn frame_size(width: f32, height: f32, scale: f32) -> (u32, u32) {
    let side = |points: f32| ((points * scale * RESOLUTION / 2.0).round() as u32 * 2).clamp(2, 1600);
    (side(width), side(height))
}

/// How tall a band of a frame is where it meets a rounded corner: the window clips a surface only to a rectangle, so
/// each band is cut to how far in the curve is over it, as the drift's rows are (`corner_inset`).
const BAND: f32 = 2.0;

/// The bands a frame is drawn in down a slot `height` tall whose corners are `radius` round: from and to where, and
/// how far in from each side. One for the middle, a few thin ones by the corners.
pub(crate) fn bands(height: f32, radius: f32) -> Vec<(f32, f32, f32)> {
    let corner = radius.min(height / 2.0).max(0.0);
    let mut edges: Vec<f32> = (0..).map(|at| at as f32 * BAND).take_while(|y| *y < corner).collect();
    edges.extend((0..).map(|at| height - corner + at as f32 * BAND).take_while(|y| *y < height));
    edges.push(height);
    edges.windows(2).filter(|pair| pair[1] > pair[0]).map(|pair| (pair[0], pair[1], corner_inset(pair[0], pair[1], height, radius))).collect()
}

/// A gradient in `slot`: the plain ground, then the newest frame over it, cut to the corners of what holds it.
fn paint_gradient(key: &SlotKey, slot: &slots::Slot, look: &Look, preset: Preset, step: u64, window: &mut Window) {
    let bounds = slot.bounds;
    paint(bounds, look, &[], window);
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let scale = window.scale_factor();
    let size = frame_size(width, height, scale);
    let uniforms = || {
        let seconds = step as f64 * STEP.as_secs_f64();
        gradients::uniforms(preset, &look.palette, seconds, size, look.radius * scale * RESOLUTION)
    };
    let Some(frame) = SHADER.with_borrow_mut(|shader| shader.frame(key, (step, size, *look), uniforms)) else {
        return;
    };
    for (top, bottom, inset) in bands(height, look.radius) {
        let band = Bounds::new(
            point(bounds.origin.x + px(inset), bounds.origin.y + px(top)),
            size_of(width - inset * 2.0, bottom - top),
        );
        window.with_content_mask(Some(ContentMask { bounds: band }), |window| paint_frame(bounds, &frame, window));
    }
}

fn size_of(width: f32, height: f32) -> gpui::Size<Pixels> {
    size(px(width.max(0.0)), px(height.max(0.0)))
}

#[cfg(target_os = "macos")]
fn paint_frame(bounds: Bounds<Pixels>, frame: &Frame, window: &mut Window) {
    window.paint_surface(bounds, frame.clone());
}

#[cfg(not(target_os = "macos"))]
fn paint_frame(_: Bounds<Pixels>, frame: &Frame, _: &mut Window) {
    match *frame {}
}

/// The step of the backdrops now, and how far into the spinners' clock it is. The first under Reduce motion, where they
/// stand still.
fn now(cx: &App) -> (u64, Duration) {
    let elapsed = live::clock(cx);
    let step = if cx.reduce_motion() { 0 } else { (elapsed.as_nanos() / STEP.as_nanos()) as u64 };
    (step, elapsed)
}

/// The step of the drift to paint, and a draw of the layer at the next while the window is in front.
fn step(window: &mut Window, cx: &mut App) -> u64 {
    let (step, elapsed) = now(cx);
    if !cx.reduce_motion() && window.is_window_active() {
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

/// What a native layer draws in a slot.
pub(crate) enum Drawing<'a> {
    /// `cols` by `rows` cells `cell` points wide from the slot's top left corner, row by row, one pixel a cell, each the
    /// colour of its shade.
    Cells { cell: f32, cols: u32, rows: u32, shades: &'a [u8], colors: [[f32; 4]; SHADES] },
    /// A gradient's frame over the whole slot, `uniforms.size` pixels big.
    Gradient(&'a Uniforms),
}

/// What draws a window's backdrops in a layer under the window's drawing (`native_backdrop::Host`), or a stand-in in
/// tests.
pub(crate) trait Present {
    /// Puts the slot's layer as `placed` and draws `drawing` in it if there is one, as one change of the screen, without
    /// waiting for the GPU.
    fn show(&mut self, slot: &SharedString, placed: &Placed, drawing: Option<&Drawing>) -> Result<(), String>;
    /// Takes the slot's layer away.
    fn remove(&mut self, slot: &SharedString);
}

/// Where a slot's layer is and how: at `frame`, in the window's points from its top left corner, with its corners
/// `radius` round, for a window of `scale` device pixels a point, showing or hidden.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Placed {
    pub frame: Bounds<Pixels>,
    pub radius: f32,
    pub scale: f32,
    pub visible: bool,
}

/// The slot a window's native layer shows.
struct Shown {
    name: SharedString,
    look: Look,
    /// Where the slot is in the window's last draw.
    bounds: Bounds<Pixels>,
    placed: Option<Placed>,
    /// What the layer last drew: the step, its size in cells or pixels, and the look.
    drawn: Option<(u64, (u32, u32), Look)>,
}

/// A window's backdrops drawn under its drawing.
struct NativeLayer {
    present: Box<dyn Present>,
    shown: Option<Shown>,
}

/// What a draw of the window does in the slot its native layer shows.
#[derive(Clone, Copy, PartialEq, Debug)]
enum InDraw {
    /// Paints it, as a window without the layer does.
    Window,
    /// Paints the plain ground: the layer has its first drawing and shows from the next draw.
    Ground,
    /// Nothing: the layer shows it through the glass's hole.
    Layer,
}

/// What a draw of the window did to its native layer.
struct Outcome {
    in_draw: InDraw,
    /// The slot it stopped showing, whose layer goes once the window's drawing without it is on screen.
    leaving: Option<SharedString>,
    /// The layer covers where the slot was as well as where it is, until the window's drawing is on screen.
    moved: bool,
}

impl NativeLayer {
    /// Puts the shown slot's layer as `placed` and draws step `step` in it at the size of its frame, unless it is there
    /// and shows that already.
    fn show(&mut self, placed: Placed, step: u64) -> Result<(), String> {
        let Some(shown) = self.shown.as_mut() else { return Ok(()) };
        let (width, height) = (f32::from(placed.frame.size.width), f32::from(placed.frame.size.height));
        let look = shown.look;
        let shades: Vec<u8>;
        let uniforms: Uniforms;
        let mut drawing = None;
        let key = match look.kind {
            Backdrop::Pixels(pattern) => {
                let cell = pattern.cell();
                let (cols, rows) = ((width / cell).ceil().max(1.0) as u32, (height / cell).ceil().max(1.0) as u32);
                let key = (step, (cols, rows), look);
                if shown.drawn != Some(key) {
                    let t = step as f32 * STEP.as_secs_f32();
                    shades = pattern.shades(cols, rows, t);
                    let colors = opaque_colors(&look.palette, pattern.tone());
                    drawing = Some(Drawing::Cells { cell, cols, rows, shades: &shades, colors });
                }
                key
            }
            Backdrop::Gradient(preset) => {
                let size = frame_size(width, height, placed.scale);
                let key = (step, size, look);
                if shown.drawn != Some(key) {
                    let seconds = step as f64 * STEP.as_secs_f64();
                    uniforms = gradients::uniforms(preset, &look.palette, seconds, size, look.radius * placed.scale * RESOLUTION);
                    drawing = Some(Drawing::Gradient(&uniforms));
                }
                key
            }
        };
        if shown.placed == Some(placed) && drawing.is_none() {
            return Ok(());
        }
        self.present.show(&shown.name, &placed, drawing.as_ref())?;
        shown.placed = Some(placed);
        if drawing.is_some() {
            shown.drawn = Some(key);
        }
        Ok(())
    }

    /// A draw of the window, with `chosen` the slot the layer shows in it, if any.
    fn in_draw(&mut self, chosen: Option<(&SharedString, &slots::Slot, Look)>, step: u64, scale: f32) -> Result<Outcome, String> {
        let mut outcome = Outcome { in_draw: InDraw::Window, leaving: None, moved: false };
        if self.shown.as_ref().is_some_and(|shown| chosen.is_none_or(|(name, ..)| *name != shown.name)) {
            outcome.leaving = self.shown.take().map(|shown| shown.name);
        }
        let Some((name, slot, look)) = chosen else { return Ok(outcome) };
        let bounds = slot.bounds;
        let shown =
            self.shown.get_or_insert_with(|| Shown { name: name.clone(), look, bounds, placed: None, drawn: None });
        // Shown from the draw after its first drawing, as the glass leaves it a hole from then (`hole`).
        let ready = shown.drawn.is_some();
        // The window's last drawing may still be on screen when the layer moves, or its next one already: over both
        // places, neither shows the desktop through its hole. `settle` takes it back to the slot.
        let frame = if ready && shown.bounds != bounds { shown.bounds.union(&bounds) } else { bounds };
        shown.bounds = bounds;
        shown.look = look;
        outcome.moved = frame != bounds;
        self.show(Placed { frame, radius: look.radius, scale, visible: ready }, step)?;
        outcome.in_draw = if ready { InDraw::Layer } else { InDraw::Ground };
        Ok(outcome)
    }
}

thread_local! {
    /// Each window's native layer, for the windows that have one.
    static NATIVE: RefCell<HashMap<WindowId, NativeLayer>> = RefCell::default();
    /// The windows whose native layer already waits for a step, by the step.
    static NATIVE_WAKING: RefCell<HashSet<(WindowId, u64)>> = RefCell::default();
}

/// Has `present` draw the backdrops of the window `id` under its drawing, from its next draw.
pub(crate) fn present_with(id: WindowId, present: Box<dyn Present>) {
    NATIVE.with_borrow_mut(|natives| natives.insert(id, NativeLayer { present, shown: None }));
}

/// Draws the window's backdrops in a layer under its drawing from now on, so a step of one never draws the window. Where
/// Metal or the layer can't be set up the window paints them itself, as an offscreen window does.
#[cfg(target_os = "macos")]
pub fn draw_backdrops_under(window: &mut Window) {
    match super::native_backdrop::Host::new(window) {
        Ok(host) => {
            present_with(window.window_handle().window_id(), Box::new(host));
            window.refresh();
        }
        Err(why) => eprintln!("Backdrops are painted in the window: {why}"),
    }
}

#[cfg(feature = "test-support")]
thread_local! {
    static SCREENSHOT_GROUND: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// For screenshots: `rgb` under the glass, where the running app shows the desktop through the window's vibrancy. An
/// offscreen window has none, so its glass lay on black and a screenshot came out darker than the app, hiding lines a
/// lighter ground shows.
#[cfg(feature = "test-support")]
pub fn set_screenshot_ground(rgb: u32) {
    SCREENSHOT_GROUND.set(Some(rgb));
}

/// The window's glass, which everything in the window lies on, in `color`: all over the window but for a hole where a
/// backdrop shows in the layer under the window. First in the window's root, which paints no ground of its own.
pub fn window_glass(color: impl Into<Hsla>) -> impl IntoElement {
    let color = color.into();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            #[cfg(feature = "test-support")]
            if let Some(ground) = SCREENSHOT_GROUND.get() {
                window.paint_quad(gpui::fill(bounds, gpui::rgb(ground)));
            }
            // Painted once every piece has been laid out in this draw, so the hole is where the slot is now.
            let id = window.window_handle().window_id();
            window.paint_quad(match hole(id) {
                Some((slot, radius)) => holed(bounds, slot, radius, color),
                None => fill(bounds, color),
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Where the glass of the window `id` leaves a hole for the slot its native layer shows, and how round: once the layer
/// has a drawing, while the slot still shows in this draw.
fn hole(id: WindowId) -> Option<(Bounds<Pixels>, f32)> {
    let name = NATIVE.with_borrow(|natives| {
        let shown = natives.get(&id)?.shown.as_ref()?;
        shown.drawn.is_some().then(|| shown.name.clone())
    })?;
    let shown = slots::shown(id, LAYER);
    let looks = looks_of(id, &shown);
    let (chosen, slot, look) = looks[pick(&looks)?];
    (*chosen == name).then_some((slot.bounds, look.radius))
}

/// A quad in `color` over all of `window` but `hole`, whose corners are `radius` round: a border so wide around the hole
/// that it reaches past the window, with nothing inside it. GPUI rounds a border's inner corners to the outer ones'
/// radius less the border (`quad_fragment`), so outer corners as much rounder as the border is wide leave the hole
/// `radius` round.
fn holed(window: Bounds<Pixels>, hole: Bounds<Pixels>, radius: f32, color: Hsla) -> PaintQuad {
    let reach = (f32::from(window.size.width) + f32::from(window.size.height)).ceil() + 1.0;
    let radius = radius.min(f32::from(hole.size.width).min(f32::from(hole.size.height)) / 2.0);
    let around = Bounds::new(
        point(hole.origin.x - px(reach), hole.origin.y - px(reach)),
        size(hole.size.width + px(reach * 2.0), hole.size.height + px(reach * 2.0)),
    );
    quad(around, Corners::all(px(radius + reach)), transparent_black(), px(reach), color, BorderStyle::Solid)
}

/// A draw of the window `id`, as its native layer sees it: places and draws the layer of `chosen`, the slot it shows,
/// and says what the window paints there. Paints all of them itself without the layer.
fn present_native(
    id: WindowId,
    chosen: Option<(&SharedString, &slots::Slot, Look)>,
    window: &mut Window,
    cx: &mut App,
) -> InDraw {
    let (step, elapsed) = now(cx);
    let scale = window.scale_factor();
    let Some(outcome) = NATIVE.with_borrow_mut(|natives| Some(natives.get_mut(&id)?.in_draw(chosen, step, scale))) else {
        return InDraw::Window;
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(why) => {
            fail_native(id, &why, window);
            return InDraw::Window;
        }
    };
    if let Some(name) = outcome.leaving {
        window.on_next_frame(move |window, _| take_away(window, &name));
    }
    match outcome.in_draw {
        // The next draw leaves the hole.
        InDraw::Ground => window.request_animation_frame(),
        InDraw::Layer => {
            if outcome.moved {
                window.on_next_frame(settle);
            }
            if !cx.reduce_motion() && window.is_window_active() {
                native_wake(step + 1, elapsed, window, cx);
            }
        }
        InDraw::Window => {}
    }
    outcome.in_draw
}

/// Takes away the layer of the slot `name`, unless it shows again.
fn take_away(window: &mut Window, name: &SharedString) {
    let id = window.window_handle().window_id();
    NATIVE.with_borrow_mut(|natives| {
        if let Some(native) = natives.get_mut(&id)
            && native.shown.as_ref().is_none_or(|shown| shown.name != *name)
        {
            native.present.remove(name);
        }
    });
}

/// Brings the layer that covered where its slot was and is back to the slot, now the window's drawing of it is on
/// screen. No draw of the window.
fn settle(window: &mut Window, _: &mut App) {
    let id = window.window_handle().window_id();
    let settled = NATIVE.with_borrow_mut(|natives| {
        let native = natives.get_mut(&id)?;
        let shown = native.shown.as_ref()?;
        let placed = shown.placed.filter(|placed| placed.frame != shown.bounds)?;
        let (bounds, step) = (shown.bounds, shown.drawn.map_or(0, |drawn| drawn.0));
        Some(native.show(Placed { frame: bounds, ..placed }, step))
    });
    if let Some(Err(why)) = settled {
        fail_native(id, &why, window);
    }
}

/// Draws a step of the window's native layer as step `next` starts, `elapsed` into the clock, unless it already waits
/// for it.
fn native_wake(next: u64, elapsed: Duration, window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    if !NATIVE_WAKING.with_borrow_mut(|waking| waking.insert((id, next))) {
        return;
    }
    let delay = Duration::from_nanos(next * STEP.as_nanos() as u64).saturating_sub(elapsed);
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(delay).await;
            NATIVE_WAKING.with_borrow_mut(|waking| waking.remove(&(id, next)));
            cx.update(native_step).ok();
        })
        .detach();
}

/// A step of the window's native layer: its next drawing, and a wake for the step after while the window is in front.
/// The window doesn't draw.
fn native_step(window: &mut Window, cx: &mut App) {
    if cx.reduce_motion() || !window.is_window_active() {
        return;
    }
    let id = window.window_handle().window_id();
    let (step, elapsed) = now(cx);
    let drawn = NATIVE.with_borrow_mut(|natives| {
        let native = natives.get_mut(&id)?;
        let placed = native.shown.as_ref()?.placed.filter(|placed| placed.visible)?;
        Some(native.show(placed, step))
    });
    match drawn {
        Some(Ok(())) => native_wake(step + 1, elapsed, window, cx),
        Some(Err(why)) => fail_native(id, &why, window),
        None => {}
    }
}

/// The native layer of the window `id` couldn't place or draw: said once, and the window paints its backdrops itself
/// from its next draw, with no hole in its glass.
fn fail_native(id: WindowId, why: &str, window: &mut Window) {
    eprintln!("Backdrops are painted in the window from now on: {why}");
    let gone = NATIVE.with_borrow_mut(|natives| natives.remove(&id));
    drop(gone);
    window.on_next_frame(|window, _| window.refresh());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_of_one_shade_side_by_side_are_one_run_and_the_grounds_are_left_out() {
        assert_eq!(runs_of(&[BASE, BASE, 3, 3, 3, BASE, 0, 0, 4]), vec![(2, 5, 3), (6, 8, 0), (8, 9, 4)]);
        assert_eq!(runs_of(&[1, 1]), vec![(0, 2, 1)]);
        assert_eq!(runs_of(&[BASE; 5]), vec![]);
        assert_eq!(runs_of(&[]), vec![]);
        // A slot's runs are fewer than its cells off the ground.
        let (width, height) = (1000.0, 760.0);
        let runs = runs_in(Pattern::Drift, width, height, 13.0, 300);
        let cells: usize = runs.iter().map(|run| (run.width / Pattern::Drift.cell()).ceil() as usize).sum();
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
        let runs: Vec<Run> =
            Pattern::ALL.into_iter().flat_map(|pattern| (0..40).flat_map(move |step| runs_in(pattern, width, height, radius, step * 50))).collect();
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

    #[test]
    fn a_gradients_bands_cover_the_slot_top_to_bottom_and_none_shows_past_a_corner() {
        let (width, height, radius) = (1000.0, 760.0, 13.0);
        let inside = |x: f32, y: f32| {
            let (cx, cy) = (x.clamp(radius, width - radius), y.clamp(radius, height - radius));
            (x - cx).hypot(y - cy) <= radius + 0.001
        };
        let bands = bands(height, radius);
        assert_eq!(bands.first().map(|band| band.0), Some(0.0));
        assert_eq!(bands.last().map(|band| band.1), Some(height));
        assert!(bands.windows(2).all(|pair| pair[0].1 == pair[1].0), "no gap between bands: {bands:?}");
        assert!(bands.len() <= 16, "a few surfaces a draw, not a row each: {}", bands.len());
        for (top, bottom, inset) in &bands {
            for (x, y) in [(*inset, *top), (width - inset, *top), (*inset, *bottom), (width - inset, *bottom)] {
                assert!(inside(x, y), "the band {top}..{bottom} shows past a corner at ({x}, {y})");
            }
        }
        // A slot shorter than its two corners still has bands, all of it.
        let short = super::bands(20.0, radius);
        assert_eq!((short.first().unwrap().0, short.last().unwrap().1), (0.0, 20.0));
    }

    #[test]
    fn the_glass_covers_the_window_but_for_a_hole_as_round_as_the_card() {
        let window = Bounds::new(point(px(0.0), px(0.0)), size(px(1000.0), px(640.0)));
        let hole = Bounds::new(point(px(240.0), px(38.0)), size(px(752.0), px(594.0)));
        let glass = holed(window, hole, 13.0, rgba(theme::GLASS).into());
        let reach = f32::from(glass.border_widths.left);
        assert!(
            [glass.border_widths.top, glass.border_widths.right, glass.border_widths.bottom].iter().all(|side| f32::from(*side) == reach),
            "one width all round, so GPUI rounds the inner corners as circles"
        );
        // The border's inner edge is the hole, rounded to the card's corners: the outer radius less the border.
        let inner = Bounds::new(
            point(glass.bounds.origin.x + px(reach), glass.bounds.origin.y + px(reach)),
            size(glass.bounds.size.width - px(reach * 2.0), glass.bounds.size.height - px(reach * 2.0)),
        );
        assert_eq!(inner, hole);
        assert_eq!(f32::from(glass.corner_radii.top_left) - reach, 13.0);
        // The border reaches past every corner of the window from anywhere in the hole.
        assert!(reach >= f32::from(window.size.width).hypot(f32::from(window.size.height)));
        assert_eq!(glass.background, gpui::Background::from(transparent_black()), "nothing in the hole");
        assert_eq!(glass.border_color, Hsla::from(rgba(theme::GLASS)));
        // A hole narrower than its corners is as round as it can be.
        let thin = holed(window, Bounds::new(point(px(10.0), px(10.0)), size(px(20.0), px(400.0))), 13.0, glass.border_color);
        assert_eq!(f32::from(thin.corner_radii.top_left) - f32::from(thin.border_widths.left), 10.0);
    }

    #[test]
    fn a_layers_cells_are_the_colours_the_window_blends_over_the_ground() {
        // What GPUI makes of each shade's colour (`colors`) painted over the ground: source over, on sRGB values as they
        // are (`SourceAlpha`, `OneMinusSourceAlpha` in gpui-pre-apple's pipelines).
        let ground = Hsla::from(rgba(Palette::APP.surface << 8 | 0xff)).to_rgb();
        for (tone, (shade, (opaque, painted))) in [Tone::Claude, Tone::Mono, Tone::Ocean, Tone::Meadow, Tone::Dusk, Tone::Aurora, Tone::Lagoon].into_iter().flat_map(|tone| {
            opaque_colors(&Palette::APP, tone).into_iter().zip(colors(&Palette::APP, tone)).enumerate().map(move |pair| (tone, pair))
        }) {
            let opaque = &opaque;
            let painted = painted.to_rgb();
            let blended = [ground.r, ground.g, ground.b]
                .into_iter()
                .zip([painted.r, painted.g, painted.b])
                .map(|(ground, color)| ground + (color - ground) * painted.a);
            for (channel, (opaque, blended)) in opaque.iter().zip(blended).enumerate() {
                assert!((opaque - blended).abs() < 2e-3, "{tone:?} shade {shade} channel {channel}: {opaque} for {blended}");
            }
            assert_eq!(opaque[3], 1.0);
        }
    }

    #[test]
    fn muted_text_keeps_its_contrast_on_every_step_of_a_colour_ramp() {
        let rgb = |color: [f32; 4]| [0, 1, 2].map(|i| (color[i] * 255.0).round() as u32);
        for tone in [Tone::Aurora, Tone::Lagoon] {
            let muted = gradients::luminance(Palette::APP.muted_foreground);
            let steps = opaque_colors(&Palette::APP, tone);
            for shade in RAMP {
                let [r, g, b] = rgb(steps[shade as usize]);
                let ratio = gradients::contrast(muted, gradients::luminance(r << 16 | g << 8 | b));
                assert!(ratio >= 4.5, "{tone:?} step {shade}: muted text {ratio:.2}:1");
            }
            // Every step is a colour of its own, and none is the ground's.
            let colors: HashSet<[u32; 3]> = RAMP.iter().map(|shade| rgb(steps[*shade as usize])).collect();
            assert_eq!(colors.len(), RAMP.len(), "{tone:?}");
            assert!(!colors.contains(&rgb(steps[BASE as usize])), "{tone:?}");
        }
    }

    #[test]
    fn a_frame_is_half_the_slots_device_pixels_even_and_capped() {
        assert_eq!(frame_size(1000.0, 760.0, 2.0), (1000, 760));
        assert_eq!(frame_size(640.0, 761.0, 1.0), (320, 380));
        assert_eq!(frame_size(5000.0, 0.0, 2.0), (1600, 2));
        let (width, height) = frame_size(333.0, 517.0, 2.0);
        assert!(width % 2 == 0 && height % 2 == 0);
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
        kind: Backdrop,
        drawn: Rc<Cell<usize>>,
    }

    impl Render for Piece {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.drawn.set(self.drawn.get() + 1);
            let (slot, kind) = (self.slot, self.kind);
            slots::drawing_piece("piece", window.window_handle().window_id(), || {
                div()
                    .relative()
                    .size_full()
                    .when(slot, |piece| piece.child(backdrop_slot("tab", kind, Palette::APP, 13.0)))
            })
        }
    }

    thread_local! {
        /// The hole the window's glass left in each draw, in order.
        static GLASS: RefCell<Vec<Option<(Bounds<Pixels>, f32)>>> = RefCell::default();
    }

    /// The hole the glass left in the window's last draw.
    fn last_hole() -> Option<(Bounds<Pixels>, f32)> {
        GLASS.with_borrow(|glass| glass.last().copied().expect("the glass was painted"))
    }

    /// The window, as the workspace's: the glass first, which asks where to leave a hole as `window_glass` does, then the
    /// layer, then the piece over it, cached.
    struct Frame {
        layer: Entity<BackdropLayer>,
        piece: Entity<Piece>,
    }

    impl Render for Frame {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            slots::start_draw(window.window_handle().window_id());
            let glass = canvas(
                |_, _, _| {},
                |_, _, window, _| {
                    let hole = hole(window.window_handle().window_id());
                    GLASS.with_borrow_mut(|glass| glass.push(hole));
                },
            )
            .absolute()
            .size_full();
            div()
                .relative()
                .size_full()
                .child(glass)
                .child(self.layer.clone())
                .child(self.piece.clone().cached(StyleRefinement::default().size_full()))
        }
    }

    const PIXELS: Backdrop = Backdrop::Pixels(Pattern::Drift);

    fn open(slot: bool, active: bool) -> (TestApp, TestAppWindow<Frame>, Rc<Cell<usize>>) {
        open_with(slot, PIXELS, active)
    }

    fn open_with(slot: bool, kind: Backdrop, active: bool) -> (TestApp, TestAppWindow<Frame>, Rc<Cell<usize>>) {
        let mut app = TestApp::new();
        let drawn = Rc::new(Cell::new(0));
        let counter = drawn.clone();
        let mut window = app.open_window(move |_, cx| Frame {
            layer: cx.new(|_| BackdropLayer::default()),
            piece: cx.new(|_| Piece { slot, kind, drawn: counter }),
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
            piece: cx.new(|_| Piece { slot: true, kind: PIXELS, drawn: counter }),
        });
        window.update(|_, window, _| window.activate_window());
        app.run_until_parked();
        assert_eq!(thirty_steps(&app, &mut window, &drawn).0, 0, "drawn once and still under Reduce motion");

        let (app, mut window, piece) = open(false, true);
        assert_eq!(thirty_steps(&app, &mut window, &piece).0, 0, "nothing to draw while no slot shows, as with the tab hidden");
    }

    /// A stand-in for Metal: counts the frames asked for and keeps which slots have frames, and fails if `broken`.
    #[derive(Clone, Default)]
    struct Fake {
        rendered: Rc<Cell<usize>>,
        live: Rc<RefCell<HashSet<SlotKey>>>,
        broken: bool,
    }

    impl Shade for Fake {
        fn render(&mut self, slot: &SlotKey, uniforms: &Uniforms) -> Result<(), String> {
            self.rendered.set(self.rendered.get() + 1);
            assert!(uniforms.size[0] >= 2.0 && uniforms.size[1] >= 2.0);
            if self.broken {
                return Err("no Metal device".to_owned());
            }
            self.live.borrow_mut().insert(slot.clone());
            Ok(())
        }

        // `super::Frame`: this module's `Frame` is the test window.
        fn latest(&mut self, _: &SlotKey) -> Option<super::Frame> {
            None
        }

        fn forget(&mut self, keep: &dyn Fn(&SlotKey) -> bool) {
            self.live.borrow_mut().retain(|slot| keep(slot));
        }
    }

    const HALO: Backdrop = Backdrop::Gradient(Preset::Halo);

    #[test]
    fn a_step_of_a_gradient_draws_one_frame_and_the_layer_and_not_the_piece() {
        let fake = Fake::default();
        use_shade(Box::new(fake.clone()));
        let (app, mut window, piece) = open_with(true, HALO, true);
        let before = fake.rendered.get();
        let (layers, pieces) = thirty_steps(&app, &mut window, &piece);
        let frames = fake.rendered.get() - before;
        eprintln!("[measure] 30 steps of a gradient: {frames} frames drawn, the layer drew {layers} times, the piece {pieces}");
        assert_eq!(pieces, 0, "the piece kept its drawing");
        assert!((29..=31).contains(&layers), "the layer drew {layers} times in 30 steps");
        assert!((29..=31).contains(&frames), "one frame a step, not one a draw: {frames}");
    }

    #[test]
    fn a_gradient_stands_still_behind_other_apps_and_under_reduce_motion() {
        let fake = Fake::default();
        use_shade(Box::new(fake.clone()));
        let (app, mut window, piece) = open_with(true, HALO, false);
        let (layers, _) = thirty_steps(&app, &mut window, &piece);
        assert_eq!((layers, fake.rendered.get()), (0, 1), "one still frame behind another app's window");

        let fake = Fake::default();
        use_shade(Box::new(fake.clone()));
        let mut app = TestApp::new();
        app.update(|cx| cx.set_reduce_motion(true));
        let drawn = Rc::new(Cell::new(0));
        let counter = drawn.clone();
        let mut window = app.open_window(move |_, cx| Frame {
            layer: cx.new(|_| BackdropLayer::default()),
            piece: cx.new(|_| Piece { slot: true, kind: HALO, drawn: counter }),
        });
        window.update(|_, window, _| window.activate_window());
        app.run_until_parked();
        let (layers, _) = thirty_steps(&app, &mut window, &drawn);
        assert_eq!((layers, fake.rendered.get()), (0, 1), "one still frame under Reduce motion");
    }

    #[test]
    fn a_gradient_whose_shader_fails_keeps_the_plain_ground_and_stops_stepping() {
        let fake = Fake { broken: true, ..Fake::default() };
        use_shade(Box::new(fake.clone()));
        let (app, mut window, piece) = open_with(true, HALO, true);
        let (layers, _) = thirty_steps(&app, &mut window, &piece);
        assert_eq!(fake.rendered.get(), 1, "tried once, then the plain ground");
        assert!(layers <= 1, "no more steps once it failed: the layer drew {layers} times");
        assert!(SHADER.with_borrow(|shader| matches!(shader, Shader::Failed)));
    }

    #[test]
    fn a_hidden_gradient_lets_go_of_its_frames() {
        let fake = Fake::default();
        use_shade(Box::new(fake.clone()));
        let (app, mut window, piece) = open_with(true, HALO, true);
        thirty_steps(&app, &mut window, &piece);
        assert_eq!(fake.live.borrow().len(), 1, "one slot's frames, however many steps");
        window.update(|frame, _, cx| frame.piece.update(cx, |piece, cx| {
            piece.slot = false;
            cx.notify();
        }));
        app.run_until_parked();
        window.draw();
        assert!(fake.live.borrow().is_empty(), "the hidden slot's frames are let go");
        let rendered = fake.rendered.get();
        thirty_steps(&app, &mut window, &piece);
        assert_eq!(fake.rendered.get(), rendered, "nothing drawn while it is hidden");
    }

    /// A stand-in for the layer under the window: keeps where it put each slot's layer and what it drew, and fails to draw
    /// if `broken`.
    #[derive(Clone, Default)]
    struct Under {
        log: Rc<RefCell<UnderLog>>,
        broken: bool,
    }

    #[derive(Default)]
    struct UnderLog {
        /// Each place: the slot, its frame, and whether it shows.
        placed: Vec<(SharedString, Bounds<Pixels>, bool)>,
        /// Each drawing: the slot, and its size in cells or pixels.
        drawn: Vec<(SharedString, (u32, u32))>,
        removed: Vec<SharedString>,
    }

    impl Present for Under {
        fn show(&mut self, slot: &SharedString, placed: &Placed, drawing: Option<&Drawing>) -> Result<(), String> {
            self.log.borrow_mut().placed.push((slot.clone(), placed.frame, placed.visible));
            let Some(drawing) = drawing else { return Ok(()) };
            if self.broken {
                return Err("no layer".to_owned());
            }
            let size = match drawing {
                Drawing::Cells { cols, rows, shades, .. } => {
                    assert_eq!(shades.len(), (cols * rows) as usize);
                    (*cols, *rows)
                }
                Drawing::Gradient(uniforms) => (uniforms.size[0] as u32, uniforms.size[1] as u32),
            };
            self.log.borrow_mut().drawn.push((slot.clone(), size));
            Ok(())
        }

        fn remove(&mut self, slot: &SharedString) {
            self.log.borrow_mut().removed.push(slot.clone());
        }
    }

    impl Under {
        fn drawn(&self) -> usize {
            self.log.borrow().drawn.len()
        }

        fn last_placed(&self) -> (Bounds<Pixels>, bool) {
            self.log.borrow().placed.last().map(|(_, frame, visible)| (*frame, *visible)).expect("placed")
        }
    }

    fn window_id(window: &mut TestAppWindow<Frame>) -> WindowId {
        window.update(|_, window, _| window.window_handle().window_id())
    }

    /// Lets the window's next frame come, as the screen would.
    fn next_frame(app: &TestApp, window: &mut TestAppWindow<Frame>) {
        window.update(|_, window, cx| {
            window.simulate_next_frame(cx);
        });
        app.run_until_parked();
    }

    /// Hands the window's backdrops to `under` and lets a frame pass, so its layer shows.
    fn draw_under(app: &TestApp, window: &mut TestAppWindow<Frame>, under: &Under) {
        let id = window_id(window);
        let present = under.clone();
        window.update(move |_, window, _| {
            present_with(id, Box::new(present));
            window.refresh();
        });
        app.run_until_parked();
        next_frame(app, window);
        // The draw of the window the layer asked for at the next step before there was a layer under it.
        app.advance_clock(STEP);
        app.run_until_parked();
    }

    #[test]
    fn under_the_window_a_step_draws_the_layer_and_never_the_window() {
        let (app, mut window, piece) = open(true, true);
        let under = Under::default();
        draw_under(&app, &mut window, &under);
        assert!(under.last_placed().1, "the layer shows");
        let drawn = under.drawn();
        let (layers, pieces) = thirty_steps(&app, &mut window, &piece);
        let steps = under.drawn() - drawn;
        eprintln!("[measure] 30 steps under the window: the layer under it drew {steps} times, the window's layer {layers}, the piece {pieces}");
        assert_eq!((layers, pieces), (0, 0), "no draw of the window");
        assert!((29..=31).contains(&steps), "one drawing a step: {steps}");
    }

    #[test]
    fn a_slot_shows_its_layer_and_the_glass_its_hole_from_the_draw_after_its_first_drawing() {
        let (app, mut window, _) = open(true, true);
        let under = Under::default();
        let id = window_id(&mut window);
        let present = under.clone();
        window.update(move |_, window, _| {
            present_with(id, Box::new(present));
            window.refresh();
        });
        app.run_until_parked();
        assert_eq!(under.log.borrow().placed.iter().map(|placed| placed.2).collect::<Vec<_>>(), [false], "placed hidden");
        assert_eq!(under.drawn(), 1, "its first drawing");
        assert_eq!(last_hole(), None, "the window paints the plain ground there, under the glass");
        next_frame(&app, &mut window);
        let (frame, visible) = under.last_placed();
        assert!(visible);
        assert_eq!(last_hole(), Some((frame, 13.0)), "the glass leaves the slot to the layer");
        assert_eq!(hole(id), Some((frame, 13.0)));
    }

    #[test]
    fn under_the_window_it_stands_still_behind_other_apps_and_under_reduce_motion() {
        let (app, mut window, piece) = open(true, false);
        let under = Under::default();
        draw_under(&app, &mut window, &under);
        // A draw of the window behind another app's still shows the step it is at, once.
        window.draw();
        let drawn = under.drawn();
        let (layers, _) = thirty_steps(&app, &mut window, &piece);
        assert_eq!((layers, under.drawn() - drawn), (0, 0), "no step while the window is behind another app's");

        let mut app = TestApp::new();
        app.update(|cx| cx.set_reduce_motion(true));
        let pieces = Rc::new(Cell::new(0));
        let counter = pieces.clone();
        let mut window = app.open_window(move |_, cx| Frame {
            layer: cx.new(|_| BackdropLayer::default()),
            piece: cx.new(|_| Piece { slot: true, kind: PIXELS, drawn: counter }),
        });
        window.update(|_, window, _| window.activate_window());
        app.run_until_parked();
        let under = Under::default();
        draw_under(&app, &mut window, &under);
        let drawn = under.drawn();
        let (layers, _) = thirty_steps(&app, &mut window, &pieces);
        assert_eq!((layers, under.drawn() - drawn), (0, 0), "drawn once and still under Reduce motion");
        assert!(drawn >= 1);
    }

    #[test]
    fn a_slot_that_goes_keeps_its_layer_a_frame_longer_then_it_goes_and_nothing_steps() {
        let (app, mut window, piece) = open(true, true);
        let under = Under::default();
        draw_under(&app, &mut window, &under);
        let id = window_id(&mut window);
        window.update(|frame, _, cx| {
            frame.piece.update(cx, |piece, cx| {
                piece.slot = false;
                cx.notify();
            })
        });
        app.run_until_parked();
        assert!(under.log.borrow().removed.is_empty(), "the window's drawing without the hole may not be on screen yet");
        assert_eq!(hole(id), None, "the glass covers where it was");
        next_frame(&app, &mut window);
        assert_eq!(under.log.borrow().removed.iter().map(|name| name.to_string()).collect::<Vec<_>>(), ["tab"]);
        let drawn = under.drawn();
        thirty_steps(&app, &mut window, &piece);
        assert_eq!(under.drawn(), drawn, "nothing drawn while it is gone");
    }

    #[test]
    fn a_slot_that_moves_has_its_layer_over_where_it_was_and_is_until_the_next_frame() {
        let (app, mut window, _) = open(true, true);
        let under = Under::default();
        draw_under(&app, &mut window, &under);
        let id = window_id(&mut window);
        let (before, _) = under.last_placed();
        // Narrower and taller, so where it was and where it is together are neither.
        let now = Bounds::new(before.origin, size(before.size.width - px(200.0), before.size.height + px(100.0)));
        window.simulate_resize(now.size);
        // The test window's resize reaches the window only so (as in `workspace::plugin_tests`): `simulate_resize` calls
        // the window back while it holds the app, and GPUI drops that call.
        window.update(|_, window, cx| window.bounds_changed(cx));
        app.run_until_parked();
        assert_eq!(under.last_placed(), (before.union(&now), true), "over where it was and where it is");
        assert_ne!(before.union(&now), now);
        assert_eq!(last_hole(), Some((now, 13.0)), "the glass's hole is where the slot is now");
        assert_eq!(hole(id), Some((now, 13.0)));
        next_frame(&app, &mut window);
        assert_eq!(under.last_placed(), (now, true), "back to the slot once that drawing is on screen");
        let cell = Pattern::Drift.cell();
        let cells = ((f32::from(now.size.width) / cell).ceil() as u32, (f32::from(now.size.height) / cell).ceil() as u32);
        assert_eq!(under.log.borrow().drawn.last().map(|drawn| drawn.1), Some(cells), "drawn at the slot's size");
    }

    #[test]
    fn where_the_layer_cant_draw_the_window_paints_the_backdrop_again() {
        let (app, mut window, piece) = open(true, true);
        let under = Under { broken: true, ..Under::default() };
        draw_under(&app, &mut window, &under);
        let id = window_id(&mut window);
        assert!(NATIVE.with_borrow(|natives| !natives.contains_key(&id)), "the layer is let go");
        assert_eq!(hole(id), None);
        let (layers, pieces) = thirty_steps(&app, &mut window, &piece);
        assert_eq!(pieces, 0);
        assert!((29..=31).contains(&layers), "the window's layer steps again: {layers}");
    }

    #[test]
    fn a_gradient_under_the_window_draws_one_frame_a_step_and_the_window_nothing() {
        let fake = Fake::default();
        use_shade(Box::new(fake.clone()));
        let (app, mut window, piece) = open_with(true, HALO, true);
        let under = Under::default();
        draw_under(&app, &mut window, &under);
        let (rendered, drawn) = (fake.rendered.get(), under.drawn());
        let (layers, pieces) = thirty_steps(&app, &mut window, &piece);
        let steps = under.drawn() - drawn;
        assert_eq!((layers, pieces), (0, 0), "no draw of the window");
        assert_eq!(fake.rendered.get(), rendered, "the window's own frames aren't drawn");
        assert!((29..=31).contains(&steps), "one frame a step: {steps}");
        let (_, (width, height)) = under.log.borrow().drawn.last().cloned().unwrap();
        assert!(width % 2 == 0 && height % 2 == 0 && width >= 2, "a frame as the window's own: {width}x{height}");
    }
}
