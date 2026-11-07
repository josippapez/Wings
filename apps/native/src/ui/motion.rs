//! The desktop app's motion: its easing curves and durations, in one place, so the native app moves the way the
//! desktop does. Each curve maps the linear progress of a GPUI `Animation` (0 to 1) to how far along it looks.

use std::{
    cell::Cell,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Bounds, ContentMask, Corners, Div, Element, ElementId, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, Pixels, Refineable, Style, StyleRefinement, Window,
    div, point, prelude::*, px, size,
};

/// `--ease-out-soft` in apps/desktop/src/index.css.
pub const EASE_OUT_SOFT: (f32, f32, f32, f32) = (0.22, 1.0, 0.36, 1.0);
/// The sidebars' slide: `[data-sliding] > [data-panel]` in apps/desktop/src/index.css.
pub const EASE_SLIDE: (f32, f32, f32, f32) = (0.32, 0.72, 0.0, 1.0);
/// CSS's `ease`, which tw-animate-css's `animate-in` uses when no `ease-*` class is set (tooltips, menus, dialogs).
pub const EASE: (f32, f32, f32, f32) = (0.25, 0.1, 0.25, 1.0);
/// Tailwind's `--default-transition-timing-function` and `ease-in-out`, for `transition-*` classes such as the
/// sidebar's `transition-opacity`.
pub const EASE_IN_OUT: (f32, f32, f32, f32) = (0.4, 0.0, 0.2, 1.0);

/// A docked sidebar opening or closing: `flex-grow 320ms`.
pub const SLIDE: Duration = Duration::from_millis(320);
/// The sidebar's contents fading in or out as it opens or closes: `transition-opacity duration-200`.
pub const SIDEBAR_FADE: Duration = Duration::from_millis(200);
/// The peek of a closed sidebar sliding out: `SidebarPeek`'s `{ type: "spring", duration: 0.25, bounce: 0 }`.
pub const PEEK: Duration = Duration::from_millis(250);
/// A tooltip coming in: tw-animate-css's `animate-in` default of 150ms.
pub const TOOLTIP_IN: Duration = Duration::from_millis(150);
/// A menu, popover or dialog coming in: shadcn's `duration-100`.
pub const POPUP_IN: Duration = Duration::from_millis(100);
/// How far a tooltip or menu slides in from: `slide-in-from-*-2`, two of Tailwind's 4px spacing steps.
pub const SLIDE_IN: f32 = 8.0;
/// The history sheet's dimmed backdrop fading in: `transition-opacity duration-150`.
pub const SHEET_BACKDROP: Duration = Duration::from_millis(150);
/// The history sheet coming in from the right: `transition duration-200 ease-in-out` from `translate-x-[2.5rem]`.
pub const SHEET_IN: Duration = Duration::from_millis(200);
pub const SHEET_SLIDE: f32 = 40.0;

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function.
pub fn cubic_bezier((x1, y1, x2, y2): (f32, f32, f32, f32)) -> impl Fn(f32) -> f32 + Copy {
    // The curve's polynomial coefficients, as browsers solve it (WebKit's UnitBezier).
    let (cx, cy) = (3.0 * x1 as f64, 3.0 * y1 as f64);
    let (bx, by) = (
        3.0 * (x2 as f64 - x1 as f64) - cx,
        3.0 * (y2 as f64 - y1 as f64) - cy,
    );
    let (ax, ay) = (1.0 - cx - bx, 1.0 - cy - by);
    let x = move |t: f64| ((ax * t + bx) * t + cx) * t;
    let y = move |t: f64| ((ay * t + by) * t + cy) * t;
    let dx = move |t: f64| (3.0 * ax * t + 2.0 * bx) * t + cx;
    move |progress: f32| {
        if progress <= 0.0 {
            return 0.0;
        }
        if progress >= 1.0 {
            return 1.0;
        }
        let target = progress as f64;
        // Newton's method first, then bisection where the slope is too flat for it.
        let mut t = target;
        for _ in 0..8 {
            let error = x(t) - target;
            if error.abs() < 1e-7 {
                return y(t) as f32;
            }
            let slope = dx(t);
            if slope.abs() < 1e-6 {
                break;
            }
            t -= error / slope;
        }
        let (mut low, mut high) = (0.0, 1.0);
        t = target;
        for _ in 0..40 {
            let at = x(t);
            if (at - target).abs() < 1e-7 {
                break;
            }
            if at < target {
                low = t;
            } else {
                high = t;
            }
            t = (low + high) / 2.0;
        }
        y(t) as f32
    }
}

/// motion's time-defined spring with no bounce, as `SidebarPeek` uses: critically damped, with its stiffness chosen
/// so that it is within 0.001 of the end when the duration is up, where it lands (motion-dom's `findSpring`).
pub fn spring() -> impl Fn(f32) -> f32 + Copy {
    let k = spring_rate();
    move |progress: f32| {
        if progress >= 1.0 {
            return 1.0;
        }
        let x = k * progress.max(0.0) as f64;
        (1.0 - (-x).exp() * (1.0 + x)) as f32
    }
}

/// The critically damped spring's rate times its duration: the root of `e^-x (1 + x) = 0.001`, found as motion does,
/// by Newton's method from 5.
fn spring_rate() -> f64 {
    let mut x: f64 = 5.0;
    for _ in 1..12 {
        let envelope = (-x).exp() * (x + 1.0) - 0.001;
        let derivative = -(-x).exp() * x;
        x -= envelope / derivative;
    }
    x
}

/// A CSS transition of one number, as a browser runs it: from where it is to `to` over `duration` along `curve`. A
/// change of course before it ends starts a new one from the value it has reached, and one that turns back to where it
/// came from is shortened by how far it got, as the CSS Transitions spec's "reversing shortening factor" does, so a
/// sidebar toggled mid-slide turns around where it is rather than jumping to an edge.
#[derive(Clone, Copy, Debug)]
pub struct Transition {
    from: f32,
    to: f32,
    /// When it began, as `BackgroundExecutor::now` gives it, so tests' fake time moves it.
    start: Instant,
    duration: Duration,
    curve: (f32, f32, f32, f32),
    /// Where a transition reversing this one would have to go to count as reversing it, and how much of its full
    /// duration this one runs.
    reversing_from: f32,
    shortening: f32,
}

impl Transition {
    /// Resting at `value`.
    pub fn at(value: f32, now: Instant) -> Self {
        Self {
            from: value,
            to: value,
            start: now,
            duration: Duration::ZERO,
            curve: EASE,
            reversing_from: value,
            shortening: 1.0,
        }
    }

    /// How far it is through its time at `now`, 0 to 1.
    fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32())
            .clamp(0.0, 1.0)
    }

    /// Its value at `now`.
    pub fn value(&self, now: Instant) -> f32 {
        self.value_after(now.saturating_duration_since(self.start))
    }

    /// Its value `elapsed` after it began.
    pub fn value_after(&self, elapsed: Duration) -> f32 {
        let t = if self.duration.is_zero() {
            1.0
        } else {
            (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
        };
        self.from + (self.to - self.from) * cubic_bezier(self.curve)(t)
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Heading for `to` from `now` on, over `duration` (or less, turning back), along `curve`.
    pub fn retarget(
        &self,
        to: f32,
        duration: Duration,
        curve: (f32, f32, f32, f32),
        now: Instant,
    ) -> Self {
        let current = self.value(now);
        let running = self.progress(now) < 1.0;
        if running && (self.reversing_from - to).abs() < 1e-6 {
            // CSS Transitions §3: the new one runs for the share of the old one's way it had covered.
            let eased = cubic_bezier(self.curve)(self.progress(now));
            let shortening = (eased * self.shortening + 1.0 - self.shortening)
                .abs()
                .clamp(0.0, 1.0);
            return Self {
                from: current,
                to,
                start: now,
                duration: duration.mul_f32(shortening),
                curve,
                reversing_from: self.to,
                shortening,
            };
        }
        Self {
            from: current,
            to,
            start: now,
            duration,
            curve,
            reversing_from: current,
            shortening: 1.0,
        }
    }
}

/// The docked sidebar's slide: the share of its width it takes, a `flex-grow 320ms` transition along the slide's
/// curve, and the opacity of its contents, the `aside`'s `transition-opacity duration-200`. Reduce motion drops the
/// slide, as the desktop's `[data-sliding]` rule does under `prefers-reduced-motion`, and keeps the fade, which the
/// desktop doesn't turn off.
#[derive(Clone, Copy, Debug)]
pub struct SidebarSlide {
    pub share: Transition,
    pub opacity: Transition,
}

impl SidebarSlide {
    pub fn at(open: bool, now: Instant) -> Self {
        let value = if open { 1.0 } else { 0.0 };
        Self {
            share: Transition::at(value, now),
            opacity: Transition::at(value, now),
        }
    }

    /// Toggled at `now`: both head for the other end from wherever they are.
    pub fn toggle(&self, open: bool, reduced: bool, now: Instant) -> Self {
        let to = if open { 1.0 } else { 0.0 };
        Self {
            share: self.share.retarget(
                to,
                if reduced { Duration::ZERO } else { SLIDE },
                EASE_SLIDE,
                now,
            ),
            opacity: self.opacity.retarget(to, SIDEBAR_FADE, EASE_IN_OUT, now),
        }
    }

    /// How long until both are done.
    pub fn duration(&self) -> Duration {
        self.share.duration().max(self.opacity.duration())
    }

    /// The share and the opacity `elapsed` after the toggle.
    pub fn after(&self, elapsed: Duration) -> (f32, f32) {
        (
            self.share.value_after(elapsed),
            self.opacity.value_after(elapsed),
        )
    }

    pub fn now(&self, now: Instant) -> (f32, f32) {
        (self.share.value(now), self.opacity.value(now))
    }
}

/// The Inbox count's beat: `inbox-pulse 700ms var(--ease-out-soft) 3` in apps/desktop/src/index.css.
pub const PULSE: Duration = Duration::from_millis(700);
pub const PULSE_BEATS: u32 = 3;

/// The Inbox count's scale `t` of the way through its three beats: `inbox-pulse`'s keyframes, 1 at 0% and 100% and
/// 1.35 at 40%, each stretch eased with `--ease-out-soft`, as a CSS animation eases between keyframes.
pub fn pulse_scale(t: f32) -> f32 {
    if t >= 1.0 {
        return 1.0;
    }
    let beat = (t * PULSE_BEATS as f32).fract();
    let ease = cubic_bezier(EASE_OUT_SOFT);
    if beat < 0.4 {
        1.0 + 0.35 * ease(beat / 0.4)
    } else {
        1.35 - 0.35 * ease((beat - 0.4) / 0.6)
    }
}

/// Whether to keep to fades, as macOS's Reduce motion asks: GPUI's own flag, which `follow_reduce_motion` sets. The
/// app's transitions draw with `tween`, which doesn't snap on it, so each picks what to keep.
pub fn reduced(cx: &App) -> bool {
    cx.reduce_motion()
}

/// One frame of a popup coming in or going out: its opacity, how far it is from its place along the side it slides
/// from, and how big its surface is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub opacity: f32,
    pub offset: f32,
    pub scale: f32,
}

/// tw-animate-css's `zoom-in-95` and `zoom-out-95`.
pub const ZOOM_FROM: f32 = 0.95;

/// Something coming in, as tw-animate-css's `animate-in fade-in-0 zoom-in-95 slide-in-from-*-2` (its `enter`
/// keyframes, eased with CSS's `ease`), `t` of the way through in time. Reduce motion keeps the fade alone.
pub fn enter_frame(t: f32, reduced: bool) -> Frame {
    let eased = cubic_bezier(EASE)(t);
    Frame {
        opacity: eased,
        offset: if reduced {
            0.0
        } else {
            SLIDE_IN * (1.0 - eased)
        },
        scale: if reduced {
            1.0
        } else {
            ZOOM_FROM + (1.0 - ZOOM_FROM) * eased
        },
    }
}

/// Something going out, as `animate-out fade-out-0 zoom-out-95` (the `exit` keyframes): it fades and shrinks where it
/// is, with no slide. Reduce motion keeps the fade alone.
pub fn exit_frame(t: f32, reduced: bool) -> Frame {
    let eased = cubic_bezier(EASE)(t);
    Frame {
        opacity: 1.0 - eased,
        offset: 0.0,
        scale: if reduced {
            1.0
        } else {
            1.0 - (1.0 - ZOOM_FROM) * eased
        },
    }
}

/// Whether a popup or layer is drawn coming in, or on its way out after it closed (with a number telling one exit
/// from the next, so each starts over).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    In,
    Out(u64),
}

thread_local! {
    /// The phase popups built right now are in: `Out` while a closed layer is drawn on its way out.
    static PHASE: Cell<Phase> = const { Cell::new(Phase::In) };
    /// The scale of the surfaces being laid out, from the nearest `tween` that zooms.
    static ZOOM: Cell<f32> = const { Cell::new(1.0) };
}

/// Builds with `phase` as the phase of the popups `build` makes, so a layer drawn on its way out takes its menus and
/// popovers with it.
pub fn with_phase<R>(phase: Phase, build: impl FnOnce() -> R) -> R {
    let outer = PHASE.replace(phase);
    let result = build();
    PHASE.set(outer);
    result
}

/// The phase popups built now are in.
pub fn phase() -> Phase {
    PHASE.get()
}

/// The id of a popup's animation in `phase`: `name` coming in, and one per exit going out.
fn phase_id(name: &'static str, phase: Phase) -> ElementId {
    match phase {
        Phase::In => ElementId::Name(name.into()),
        Phase::Out(exit) => ElementId::NamedInteger(format!("{name}-out").into(), exit),
    }
}

/// `element` animated over `duration`, as GPUI's `with_animation` does, but without GPUI's Reduce motion snap, so a
/// transition can keep its fade under Reduce motion as the desktop does. `animate` gets the element, how far along it
/// is in time (0 to 1) and whether Reduce motion is on, each frame.
pub fn tween<E: IntoElement + 'static>(
    element: E,
    id: impl Into<ElementId>,
    duration: Duration,
    animate: impl FnOnce(E, f32, bool) -> E + 'static,
) -> Tween<E> {
    Tween {
        id: id.into(),
        element: Some(element),
        duration,
        animate: Some(Box::new(move |element, t, reduced| {
            (animate(element, t, reduced), None)
        })),
    }
}

/// A `tween` that also zooms the surfaces (`surface`) under it: `animate` returns their scale with the element.
pub fn tween_zoom<E: IntoElement + 'static>(
    element: E,
    id: impl Into<ElementId>,
    duration: Duration,
    animate: impl FnOnce(E, f32, bool) -> (E, f32) + 'static,
) -> Tween<E> {
    Tween {
        id: id.into(),
        element: Some(element),
        duration,
        animate: Some(Box::new(move |element, t, reduced| {
            let (element, scale) = animate(element, t, reduced);
            (element, Some(scale))
        })),
    }
}

type Animate<E> = Box<dyn FnOnce(E, f32, bool) -> (E, Option<f32>)>;

pub struct Tween<E> {
    id: ElementId,
    element: Option<E>,
    duration: Duration,
    animate: Option<Animate<E>>,
}

impl<E: IntoElement + 'static> IntoElement for Tween<E> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: IntoElement + 'static> Element for Tween<E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let duration = self.duration;
        let element = self.element.take().expect("laid out once");
        let animate = self.animate.take().expect("laid out once");
        window.with_element_state(
            global_id.expect("a tween has an id"),
            |start: Option<Instant>, window| {
                let now = cx.background_executor().now();
                let start = start.unwrap_or(now);
                let t = if duration.is_zero() {
                    1.0
                } else {
                    (now.saturating_duration_since(start).as_secs_f32() / duration.as_secs_f32())
                        .min(1.0)
                };
                let (element, zoom) = animate(element, t, reduced(cx));
                let mut element = element.into_any_element();
                if t < 1.0 {
                    window.request_animation_frame();
                }
                let outer = ZOOM.get();
                if let Some(zoom) = zoom {
                    ZOOM.set(zoom);
                }
                let layout = element.request_layout(window, cx);
                ZOOM.set(outer);
                ((layout, element), start)
            },
        )
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

/// `element`'s box, what it paints of itself (its background, border and shadow), drawn at the scale of the `tween`
/// around it, about `origin` (a share of its width and height), with its contents clipped to it, as close as GPUI
/// comes to tw-animate-css's `zoom-in-95`: GPUI can't scale an element and its contents. Its quads and shadows
/// (`Quad`, `Shadow` in gpui's scene.rs) carry no transformation, glyphs are always drawn with
/// `TransformationMatrix::unit()` (`Window::paint_glyph`), and the one way to draw a whole subtree bigger or smaller,
/// `Window::set_scale_factor`, is for tests only. So the surface zooms and the contents fade in at their size.
pub fn surface<E: Styled + IntoElement>(mut element: E, origin: (f32, f32)) -> Surface {
    let style = element.style();
    let chrome = StyleRefinement {
        background: style.background.take(),
        border_color: style.border_color.take(),
        box_shadow: style.box_shadow.take(),
        corner_radii: style.corner_radii.clone(),
        border_widths: style.border_widths.clone(),
        ..StyleRefinement::default()
    };
    Surface {
        chrome,
        origin,
        scale: None,
        child: element.into_any_element(),
    }
}

/// A `surface` at `scale`, whatever the tweens around it say.
pub fn surface_scaled<E: Styled + IntoElement>(
    element: E,
    origin: (f32, f32),
    scale: f32,
) -> Surface {
    Surface {
        scale: Some(scale),
        ..surface(element, origin)
    }
}

pub struct Surface {
    chrome: StyleRefinement,
    origin: (f32, f32),
    scale: Option<f32>,
    child: AnyElement,
}

impl IntoElement for Surface {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Surface {
    /// The scale it was laid out at.
    type RequestLayoutState = f32;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
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
    ) -> (LayoutId, f32) {
        // The box is the contents' own, so the surface takes their place and size in the layout.
        (
            self.child.request_layout(window, cx),
            self.scale.unwrap_or(ZOOM.get()),
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut f32,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        scale: &mut f32,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let scale = *scale;
        let mut style = Style::default();
        style.refine(&self.chrome);
        let child = &mut self.child;
        if scale == 1.0 {
            style.paint(bounds, window, cx, |window, cx| child.paint(window, cx));
            return;
        }
        let rem = window.rem_size();
        let about = point(
            bounds.origin.x + bounds.size.width * self.origin.0,
            bounds.origin.y + bounds.size.height * self.origin.1,
        );
        let scaled = Bounds {
            origin: point(
                about.x + (bounds.origin.x - about.x) * scale,
                about.y + (bounds.origin.y - about.y) * scale,
            ),
            size: size(bounds.size.width * scale, bounds.size.height * scale),
        };
        let radii = style.corner_radii.to_pixels(rem);
        style.corner_radii = Corners {
            top_left: (radii.top_left * scale).into(),
            top_right: (radii.top_right * scale).into(),
            bottom_right: (radii.bottom_right * scale).into(),
            bottom_left: (radii.bottom_left * scale).into(),
        };
        for shadow in &mut style.box_shadow {
            shadow.offset = point(shadow.offset.x * scale, shadow.offset.y * scale);
            shadow.blur_radius *= scale;
            shadow.spread_radius *= scale;
        }
        style.paint(scaled, window, cx, |window, cx| {
            if scale < 1.0 {
                window.with_content_mask(Some(ContentMask { bounds: scaled }), |window| {
                    child.paint(window, cx)
                })
            } else {
                child.paint(window, cx)
            }
        });
    }
}

/// Where a popup slides in from, as shadcn picks `slide-in-from-*` by the side it opens on, and the corner its
/// surface zooms about, as Base UI's `--transform-origin` puts it at the trigger.
#[derive(Clone, Copy)]
pub enum From {
    /// It opens below its trigger, so it comes down, and zooms about its top edge's middle.
    Above,
    /// It opens below and to the right of the pointer, so it comes down, and zooms about its top left corner.
    Corner,
    /// It opens to the right of its trigger, so it comes in from the left, and zooms about its top left corner.
    Left,
}

impl From {
    fn origin(self) -> (f32, f32) {
        match self {
            From::Above => (0.5, 0.0),
            From::Corner | From::Left => (0.0, 0.0),
        }
    }
}

fn slide<E: Styled>(element: E, from: From, offset: f32) -> E {
    match from {
        From::Above | From::Corner => element.top(px(-offset)),
        From::Left => element.left(px(-offset)),
    }
}

/// A popup (a menu, a popover, a tooltip) coming in, as shadcn's `animate-in fade-in-0 zoom-in-95 slide-in-from-*-2`,
/// or on its way out, as `animate-out fade-out-0 zoom-out-95`, over `duration`, in the phase popups are being built in.
/// Its surface zooms (see `surface`); the wrapper takes its place in the layout, and on its way out the pointer passes
/// through it.
pub fn popup<E: Styled + IntoElement + 'static>(
    name: &'static str,
    from: From,
    duration: Duration,
    content: E,
) -> AnyElement {
    let phase = phase();
    let wrapper = div().relative().child(surface(content, from.origin()));
    let popup = tween_zoom(
        wrapper,
        phase_id(name, phase),
        duration,
        move |popup, t, reduced| {
            let frame = match phase {
                Phase::In => enter_frame(t, reduced),
                Phase::Out(_) => exit_frame(t, reduced),
            };
            (
                slide(popup.opacity(frame.opacity), from, frame.offset),
                frame.scale,
            )
        },
    );
    inert_out(phase, popup)
}

/// `element` as it is in `phase`: on its way out it is drawn but out of the pointer's reach, so a click or a hover
/// goes to what is under it, as though it had already gone.
fn inert_out(phase: Phase, element: impl IntoElement) -> AnyElement {
    match phase {
        Phase::In => element.into_any_element(),
        Phase::Out(_) => Inert(element.into_any_element()).into_any_element(),
    }
}

/// Lays out and paints its child, but makes its hitboxes under an empty content mask, so none of them is ever under
/// the pointer: the child's clicks, hovers and occlusion pass through to what is under it.
struct Inert(AnyElement);

impl IntoElement for Inert {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Inert {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
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
    ) -> (LayoutId, ()) {
        (self.0.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        // Hitboxes keep the content mask they were made under, and the hit test only counts the part inside it.
        let nowhere = ContentMask {
            bounds: Bounds::default(),
        };
        window.with_content_mask(Some(nowhere), |window| self.0.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0.paint(window, cx);
    }
}

/// A layer over the whole window, a dialog and its backdrop, fading in as shadcn's `Dialog` (`duration-100
/// animate-in fade-in-0`, its panel `zoom-in-95` too: build the panel with `surface`), or out on its way out, in
/// `phase`. On its way out the pointer passes through it.
pub fn dialog_layer(name: &'static str, phase: Phase, layer: impl IntoElement) -> AnyElement {
    let wrapper = full(div()).child(layer);
    let layer = tween_zoom(
        wrapper,
        phase_id(name, phase),
        POPUP_IN,
        move |layer, t, reduced| {
            let frame = match phase {
                Phase::In => enter_frame(t, reduced),
                Phase::Out(_) => exit_frame(t, reduced),
            };
            (layer.opacity(frame.opacity), frame.scale)
        },
    );
    inert_out(phase, layer)
}

/// A popup layer over the whole window, the Inbox and the clear layer that closes it: `popup`'s motion for a layer.
pub fn popup_layer(
    name: &'static str,
    from: From,
    phase: Phase,
    layer: impl IntoElement,
) -> AnyElement {
    let wrapper = full(div()).child(layer);
    let layer = tween_zoom(
        wrapper,
        phase_id(name, phase),
        POPUP_IN,
        move |layer, t, reduced| {
            let frame = match phase {
                Phase::In => enter_frame(t, reduced),
                Phase::Out(_) => exit_frame(t, reduced),
            };
            (
                slide(layer.opacity(frame.opacity), from, frame.offset),
                frame.scale,
            )
        },
    );
    inert_out(phase, layer)
}

fn full(layer: Div) -> Div {
    layer.absolute().top_0().left_0().size_full()
}

/// The history sheet's layer, which the pointer passes through on its way out, and nothing else, since the backdrop and the sheet each fade on
/// their own (`sheet_backdrop`, `sheet_panel`).
pub fn sheet_layer(phase: Phase, layer: impl IntoElement) -> AnyElement {
    inert_out(phase, full(div()).child(layer))
}

/// The sheet's dimmed backdrop, `bg` at full strength, fading in or out on its own, as the desktop's `SheetOverlay`'s
/// `transition-opacity duration-150` (Tailwind's default `ease-in-out` curve) does, separately from the sheet.
pub fn sheet_backdrop<E: Styled + IntoElement + 'static>(
    backdrop: E,
    bg: Hsla,
    phase: Phase,
) -> AnyElement {
    let ease = cubic_bezier(EASE_IN_OUT);
    tween(
        backdrop,
        phase_id("sheet-backdrop", phase),
        SHEET_BACKDROP,
        move |backdrop, t, _| {
            let shown = match phase {
                Phase::In => ease(t),
                Phase::Out(_) => 1.0 - ease(t),
            };
            backdrop.bg(bg.opacity(shown))
        },
    )
    .into_any_element()
}

/// The sheet coming in from the right or going back out, as the desktop's `SheetContent`'s `transition duration-200
/// ease-in-out` between `opacity-0 translate-x-[2.5rem]` and its place. Reduce motion keeps the fade alone.
pub fn sheet_panel<E: Styled + IntoElement + 'static>(sheet: E, phase: Phase) -> AnyElement {
    let ease = cubic_bezier(EASE_IN_OUT);
    tween(
        sheet,
        phase_id("sheet", phase),
        SHEET_IN,
        move |sheet, t, reduced| {
            let shown = match phase {
                Phase::In => ease(t),
                Phase::Out(_) => 1.0 - ease(t),
            };
            let offset = if reduced {
                0.0
            } else {
                SHEET_SLIDE * (1.0 - shown)
            };
            sheet.opacity(shown).left(px(offset))
        },
    )
    .into_any_element()
}

/// Follows macOS's Reduce motion, as the desktop follows `prefers-reduced-motion`: GPUI then shows every animation at
/// its end at once.
pub fn follow_reduce_motion(cx: &mut App) {
    cx.set_reduce_motion(system_reduces_motion());
}

#[cfg(all(target_os = "macos", not(test)))]
fn system_reduces_motion() -> bool {
    use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
    // SAFETY: `sharedWorkspace` and `accessibilityDisplayShouldReduceMotion` (macOS 10.12) take no arguments and
    // return an object and a BOOL.
    unsafe {
        let workspace: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        msg_send![&*workspace, accessibilityDisplayShouldReduceMotion]
    }
}

/// Tests and other platforms animate.
#[cfg(not(all(target_os = "macos", not(test))))]
fn system_reduces_motion() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_curve_starts_at_zero_and_ends_at_one() {
        for curve in [EASE_OUT_SOFT, EASE_SLIDE, EASE, EASE_IN_OUT] {
            let ease = cubic_bezier(curve);
            assert_eq!(ease(0.0), 0.0, "{curve:?}");
            assert_eq!(ease(1.0), 1.0, "{curve:?}");
            assert!((ease(1e-4)).abs() < 0.01, "{curve:?}");
            assert!((ease(1.0 - 1e-4) - 1.0).abs() < 0.01, "{curve:?}");
        }
        let spring = spring();
        assert_eq!(spring(0.0), 0.0);
        assert_eq!(spring(1.0), 1.0);
        assert!((spring(0.9999) - 0.999).abs() < 1e-3);
    }

    /// Points on each curve, from the parametric cubic-bezier itself: at parameter `s` the curve is at
    /// `(bx(s), by(s))`, so easing `bx(s)` must give `by(s)`.
    #[test]
    fn the_curves_follow_their_cubic_bezier() {
        let point = |(x1, y1, x2, y2): (f32, f32, f32, f32), s: f32| {
            let bez = |p1: f32, p2: f32| {
                3.0 * (1.0 - s).powi(2) * s * p1 + 3.0 * (1.0 - s) * s * s * p2 + s.powi(3)
            };
            (bez(x1, x2), bez(y1, y2))
        };
        for curve in [EASE_OUT_SOFT, EASE_SLIDE, EASE, EASE_IN_OUT] {
            let ease = cubic_bezier(curve);
            for s in [0.1, 0.25, 0.5, 0.75, 0.9] {
                let (x, y) = point(curve, s);
                assert!(
                    (ease(x) - y).abs() < 1e-4,
                    "{curve:?} at {x}: {} vs {y}",
                    ease(x)
                );
            }
        }
        // Known values: CSS's `ease` is at about 0.8024 halfway, and the slide curve is well past halfway by then.
        assert!((cubic_bezier(EASE)(0.5) - 0.8024).abs() < 1e-3);
        assert!(cubic_bezier(EASE_SLIDE)(0.5) > 0.85);
    }

    #[test]
    fn the_spring_is_motions_critically_damped_spring() {
        let k = spring_rate();
        assert!(((-k).exp() * (1.0 + k) - 0.001).abs() < 1e-9);
        // Critically damped, it never overshoots and only ever moves on.
        let spring = spring();
        let mut last = 0.0;
        for i in 1..=100 {
            let at = spring(i as f32 / 100.0);
            assert!(at >= last && at <= 1.0);
            last = at;
        }
    }

    #[test]
    fn the_sidebar_fades_in_its_first_200ms_and_takes_its_width_by_320ms() {
        let now = Instant::now();
        let opening = SidebarSlide::at(false, now).toggle(true, false, now);
        assert_eq!(opening.after(Duration::ZERO), (0.0, 0.0));
        assert_eq!(opening.after(SIDEBAR_FADE).1, 1.0);
        assert!(opening.after(SIDEBAR_FADE).0 < 1.0);
        assert_eq!(opening.after(SLIDE), (1.0, 1.0));
        let closing = SidebarSlide::at(true, now).toggle(false, false, now);
        assert_eq!(closing.after(Duration::ZERO), (1.0, 1.0));
        assert_eq!(closing.after(SLIDE), (0.0, 0.0));
    }

    /// Turned back mid-way, a transition starts from the value it reached and runs for the share of its time it had
    /// covered, as the CSS Transitions spec reverses one; a new one in the same direction runs its full time.
    #[test]
    fn a_transition_turned_back_reverses_from_where_it_is() {
        let start = Instant::now();
        let opening = Transition::at(0.0, start).retarget(1.0, SLIDE, EASE_SLIDE, start);
        let mid = start + SLIDE / 4;
        let reached = opening.value(mid);
        assert!(reached > 0.3 && reached < 0.9, "{reached}");
        let back = opening.retarget(0.0, SLIDE, EASE_SLIDE, mid);
        assert_eq!(back.value(mid), reached, "it turns where it is");
        assert!((back.duration().as_secs_f32() - SLIDE.as_secs_f32() * reached).abs() < 1e-3);
        assert_eq!(back.value(mid + back.duration()), 0.0);
        // Turned again, it runs back up to 1 for the share of the way back it covered.
        let again = back.retarget(1.0, SLIDE, EASE_SLIDE, mid + back.duration() / 2);
        assert!(again.duration() < SLIDE);
        // Settled, a new one runs its full time.
        let settled = back.retarget(1.0, SLIDE, EASE_SLIDE, mid + SLIDE);
        assert_eq!(settled.duration(), SLIDE);
    }

    /// Reduce motion drops the sidebar's slide, as the desktop's `[data-sliding]` rule does, but keeps its fade.
    #[test]
    fn reduce_motion_keeps_the_sidebar_fade_and_drops_the_slide() {
        let now = Instant::now();
        let opening = SidebarSlide::at(false, now).toggle(true, true, now);
        assert_eq!(opening.share.duration(), Duration::ZERO);
        assert_eq!(opening.after(Duration::ZERO).0, 1.0);
        assert_eq!(opening.after(Duration::ZERO).1, 0.0);
        assert_eq!(opening.duration(), SIDEBAR_FADE);
    }

    /// The Inbox count beats three times, up to 1.35 at 40% of each 700ms beat and back, and ends still.
    #[test]
    fn the_inbox_count_beats_three_times_and_ends_at_its_size() {
        let beats = PULSE_BEATS as f32;
        for beat in 0..PULSE_BEATS {
            let start = beat as f32 / beats;
            assert!((pulse_scale(start) - 1.0).abs() < 1e-4);
            assert!((pulse_scale(start + 0.4 / beats) - 1.35).abs() < 1e-3);
        }
        assert_eq!(pulse_scale(1.0), 1.0);
        assert!(
            pulse_scale(0.1 / beats) > 1.2,
            "ease-out-soft gets most of the way early"
        );
    }

    /// Popups come in faded, 8px off and at 95%, and go out fading and shrinking in place; Reduce motion keeps the
    /// fade alone, as macOS asks.
    #[test]
    fn popups_fade_zoom_and_slide_and_reduce_motion_keeps_the_fade() {
        let at = |opacity, offset, scale| Frame {
            opacity,
            offset,
            scale,
        };
        assert_eq!(enter_frame(0.0, false), at(0.0, SLIDE_IN, ZOOM_FROM));
        assert_eq!(enter_frame(1.0, false), at(1.0, 0.0, 1.0));
        assert_eq!(exit_frame(0.0, false), at(1.0, 0.0, 1.0));
        assert_eq!(exit_frame(1.0, false), at(0.0, 0.0, ZOOM_FROM));
        assert_eq!(enter_frame(0.0, true), at(0.0, 0.0, 1.0));
        assert_eq!(exit_frame(0.5, true).scale, 1.0);
        assert!(exit_frame(0.5, true).opacity < 1.0);
    }
}
