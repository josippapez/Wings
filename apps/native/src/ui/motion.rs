//! The desktop app's motion: its easing curves and durations, in one place, so the native app moves the way the
//! desktop does. Each curve maps the linear progress of a GPUI `Animation` (0 to 1) to how far along it looks.

use std::time::Duration;

use gpui::App;

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

/// A docked sidebar `t` of the way through its slide (linear time, 0 to 1): the share of its width it takes, and the
/// opacity of its contents. The width follows the slide's curve over `SLIDE`; the contents fade over the shorter
/// `SIDEBAR_FADE`, as the desktop's panel and its `aside` do.
pub fn sidebar_frame(t: f32, opening: bool) -> (f32, f32) {
    let width = cubic_bezier(EASE_SLIDE)(t);
    let fade_t = (t * SLIDE.as_secs_f32() / SIDEBAR_FADE.as_secs_f32()).min(1.0);
    let opacity = cubic_bezier(EASE_IN_OUT)(fade_t);
    if opening {
        (width, opacity)
    } else {
        (1.0 - width, 1.0 - opacity)
    }
}

/// Something coming in, as `animate-in fade-in-0 slide-in-from-*-2`: its opacity and how far it still is from its
/// place, `t` of the way through (linear time).
pub fn enter_frame(t: f32) -> (f32, f32) {
    let eased = cubic_bezier(EASE)(t);
    (eased, SLIDE_IN * (1.0 - eased))
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
        let fade_end = SIDEBAR_FADE.as_secs_f32() / SLIDE.as_secs_f32();
        assert_eq!(sidebar_frame(0.0, true), (0.0, 0.0));
        assert_eq!(sidebar_frame(fade_end, true).1, 1.0);
        assert!(sidebar_frame(fade_end, true).0 < 1.0);
        assert_eq!(sidebar_frame(1.0, true), (1.0, 1.0));
        assert_eq!(sidebar_frame(0.0, false), (1.0, 1.0));
        assert_eq!(sidebar_frame(1.0, false), (0.0, 0.0));
    }
}
