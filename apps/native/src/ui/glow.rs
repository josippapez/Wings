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

/// The glow, filling the element it is drawn in.
pub fn glow() -> impl IntoElement {
    canvas(|_, window, cx| crate::ui::status_dot::ticking(window, cx).unwrap_or(0.0), paint).size_full()
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
