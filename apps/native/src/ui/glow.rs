//! A soft glow that drifts behind a page's empty state: a few blurred discs of the Claude orange and the info blue,
//! moving slowly on the spinners' clock. Each disc is one blurred shadow, which the GPU draws in a pass, so a frame
//! costs a handful of primitives. It stands still under Reduce motion and while the window is behind another.

use std::f32::consts::TAU;

use gpui::{
    App, Bounds, BoxShadow, Corners, Hsla, IntoElement, Pixels, Styled, Window, canvas, point, px,
    rgba,
};

use crate::theme;

/// The discs: colour, how strong, size against the smaller side of the bounds, where they sit around the centre as a
/// share of the bounds, how far they wander, and how many seconds a loop takes.
struct Disc {
    color: u32,
    alpha: f32,
    size: f32,
    at: (f32, f32),
    reach: (f32, f32),
    period: f32,
    phase: f32,
}

const DISCS: [Disc; 4] = [
    Disc { color: theme::CLAUDE, alpha: 0.34, size: 0.30, at: (-0.07, -0.03), reach: (0.07, 0.05), period: 23.0, phase: 0.0 },
    Disc { color: theme::CLAUDE, alpha: 0.22, size: 0.22, at: (0.10, 0.05), reach: (0.06, 0.05), period: 31.0, phase: 0.4 },
    Disc { color: theme::INFO, alpha: 0.20, size: 0.24, at: (0.07, -0.07), reach: (0.07, 0.04), period: 37.0, phase: 0.7 },
    Disc { color: 0xc06a8a, alpha: 0.16, size: 0.20, at: (-0.11, 0.07), reach: (0.05, 0.04), period: 29.0, phase: 0.2 },
];

/// How often the glow steps. Its discs take 23 to 37 seconds to go round, so 10 steps a second look as smooth as the
/// spinners' 30, and each step draws the page it is in again.
const GLOW_STEP: std::time::Duration = std::time::Duration::from_millis(100);

/// The glow, filling the element it is drawn in.
pub fn glow() -> impl IntoElement {
    canvas(
        |_, window, cx| {
            if cx.reduce_motion() {
                0.0
            } else if window.is_window_active() {
                super::status_dot::ticking_every(GLOW_STEP, window, cx)
            } else {
                super::status_dot::clock(cx).as_secs_f32()
            }
        },
        paint,
    )
    .size_full()
}

fn paint(bounds: Bounds<Pixels>, seconds: f32, window: &mut Window, _: &mut App) {
    let center = bounds.center();
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    // Sized by the smaller side, so a short pane keeps the glow inside it rather than cut flat at its edges.
    let side = width.min(height * 1.2);
    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
        for disc in &DISCS {
            let turn = (seconds / disc.period + disc.phase) * TAU;
            // A Lissajous loop, so no two discs line up the same way twice.
            let x = disc.at.0 + disc.reach.0 * turn.sin();
            let y = disc.at.1 + disc.reach.1 * (turn * 1.3).cos();
            let radius = side * disc.size / 2.0;
            let origin = point(center.x + px(x * width - radius), center.y + px(y * height - radius));
            let shape = Bounds::new(origin, gpui::size(px(radius * 2.0), px(radius * 2.0)));
            let color: Hsla = rgba(disc.color << 8 | (disc.alpha * 255.0) as u32).into();
            window.paint_drop_shadows(
                shape,
                Corners::all(px(radius)),
                &[BoxShadow { color, offset: point(px(0.0), px(0.0)), blur_radius: px(radius * 0.55), spread_radius: px(0.0), inset: false }],
            );
        }
    });
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{Context, Render, TestApp, div, prelude::*};

    use super::*;

    struct Glowing(Rc<Cell<usize>>);

    impl Render for Glowing {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            div().size_full().child(glow())
        }
    }

    /// A second of the glow behind an empty chat draws its page about 10 times, not the spinners' 30.
    #[test]
    fn a_second_of_the_glow_draws_its_view_ten_times() {
        let mut app = TestApp::new();
        let drawn = Rc::new(Cell::new(0));
        let counter = drawn.clone();
        let mut window = app.open_window(move |_, _| Glowing(counter));
        window.update(|_, window, _| window.activate_window());
        window.draw();
        let before = drawn.get();
        for _ in 0..100 {
            app.advance_clock(std::time::Duration::from_millis(10));
            app.run_until_parked();
        }
        let steps = drawn.get() - before;
        assert!((9..=11).contains(&steps), "drawn {steps} times in a second");
    }
}
