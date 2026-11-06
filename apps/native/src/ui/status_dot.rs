//! The desktop's `StatusDot` (`components/status-dot.tsx`): what an agent is doing, in 10px.

use std::{f32::consts::TAU, time::Duration};

use gpui::{
    Animation, AnimationExt, ElementId, IntoElement, PathBuilder, canvas, div, point, prelude::*,
    px, rgb, rgba,
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

/// The desktop's `StatusDot`: a spinning ring while working, a red dot when it needs you, a green
/// dot when done and an outlined ring when idle, in a 10px box.
pub fn status_dot(id: impl Into<ElementId>, state: AgentState) -> impl IntoElement {
    let dot = |size: f32| div().size(px(size)).rounded_full();
    let shape = match state {
        AgentState::Working => div()
            .size(px(10.0))
            .with_animation(
                id,
                Animation::new(Duration::from_secs(1))
                    .repeat()
                    .with_max_fps(30.0),
                |ring, turn| ring.child(spinner(turn * TAU)),
            )
            .into_any_element(),
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
