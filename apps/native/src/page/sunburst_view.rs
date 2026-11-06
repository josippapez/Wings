//! The disk map: wedges in rings round a centre that says what the pointer is on. The page lays the wedges out; this
//! draws them, lights the one under the pointer and tells the page when the pointer moves to another or clicks.
use super::*;
use gpui::{
    App, Bounds, ClickEvent, CursorStyle, MouseMoveEvent, PathBuilder, ShapedLine, TextAlign, point,
};
use std::{f32::consts::FRAC_PI_2, rc::Rc};
use tree::{Geometry, Hit, Sunburst, Wedge};

/// Half the width of the line between wedges, in points.
const GAP: f32 = 0.5;

/// What the view keeps of a map between renders: what the pointer is on, and where it was drawn.
#[derive(Default)]
pub(super) struct SunburstState {
    hover: Option<Hit>,
    bounds: Bounds<Pixels>,
}

impl PageView {
    pub(super) fn sunburst(&self, map: &Sunburst, cx: &mut Context<Self>) -> AnyElement {
        let id = map.id;
        let hover = self.sunbursts.borrow().get(&id).and_then(|s| s.hover);
        let map = Rc::new(map.clone());
        let entity = cx.entity();
        let drawn = map.clone();
        let surface = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                entity.update(cx, |view, _| {
                    view.sunbursts.borrow_mut().entry(id).or_default().bounds = bounds;
                });
                paint(&drawn, hover, bounds, window, cx);
            },
        )
        .absolute()
        .size_full();
        let moved = map.clone();
        let clicked = map.clone();
        div()
            .id(ElementId::NamedInteger("page-sunburst".into(), id))
            .role(Role::Image)
            .aria_label(format!("{}, {}", map.title, map.detail))
            .relative()
            .flex_1()
            .w_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .when(
                matches!(hover, Some(Hit::Wedge(_))) || (hover == Some(Hit::Center) && map.up),
                |el| el.cursor(CursorStyle::PointingHand),
            )
            .child(surface)
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _, cx| {
                let found = view.sunburst_hit(&moved, event.position);
                view.sunburst_point(&moved, found, cx);
            }))
            .on_hover(cx.listener({
                let map = map.clone();
                move |view, hovered: &bool, _, cx| {
                    if !hovered {
                        view.sunburst_point(&map, None, cx);
                    }
                }
            }))
            .on_click(cx.listener(move |view, event: &ClickEvent, _, _| {
                let Some(handler) = &clicked.on_select else {
                    return;
                };
                let found = event
                    .mouse_position()
                    .and_then(|position| view.sunburst_hit(&clicked, position));
                if let Some(hit) = found {
                    view.send(
                        handler,
                        Some(&serde_json::json!({
                            "index": Hit::index(Some(hit)),
                            "meta": event.modifiers().platform,
                        })),
                    );
                }
            }))
            .into_any_element()
    }

    /// What is under `position` (in the window) in the map as it was last drawn.
    fn sunburst_hit(&self, map: &Sunburst, position: gpui::Point<Pixels>) -> Option<Hit> {
        let bounds = self.sunbursts.borrow().get(&map.id)?.bounds;
        let local = position - bounds.origin;
        let geometry = Geometry::of(
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            map.rings,
        )?;
        geometry.hit(
            map.rings,
            &map.wedges,
            f32::from(local.x),
            f32::from(local.y),
        )
    }

    /// The pointer is on `hit` now: the page hears of it when that is another wedge.
    fn sunburst_point(&mut self, map: &Sunburst, hit: Option<Hit>, cx: &mut Context<Self>) {
        let changed = {
            let mut states = self.sunbursts.borrow_mut();
            let state = states.entry(map.id).or_default();
            let changed = state.hover != hit;
            state.hover = hit;
            changed
        };
        if !changed {
            return;
        }
        if let Some(handler) = &map.on_point {
            self.send(
                handler,
                Some(&serde_json::json!({ "index": Hit::index(hit) })),
            );
        }
        cx.notify();
    }
}

fn paint(
    map: &Sunburst,
    hover: Option<Hit>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(geometry) = Geometry::of(
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
        map.rings,
    ) else {
        return;
    };
    let centre = point(
        bounds.origin.x + px(geometry.cx),
        bounds.origin.y + px(geometry.cy),
    );
    for wedge in &map.wedges {
        if let Some(path) = wedge_path(centre, &geometry, wedge, GAP, PathBuilder::fill()) {
            window.paint_path(
                path,
                rgba(wedge.color << 8 | (wedge.alpha * 255.0).round() as u32),
            );
        }
    }
    if let Some(selected) = map.selected.and_then(|index| map.wedges.get(index)) {
        let path = wedge_path(
            centre,
            &geometry,
            selected,
            1.0,
            PathBuilder::stroke(px(2.0)),
        );
        if let Some(path) = path {
            window.paint_path(path, rgb(theme::FOREGROUND));
        }
    }
    if let Some(Hit::Wedge(index)) = hover
        && let Some(wedge) = map.wedges.get(index)
        && let Some(path) = wedge_path(centre, &geometry, wedge, GAP, PathBuilder::fill())
    {
        window.paint_path(path, rgba(theme::FOREGROUND << 8 | 0x52));
    }

    // The middle.
    let disc = |radius: f32, mut path: PathBuilder| {
        let start = point(centre.x + px(radius), centre.y);
        path.move_to(start);
        for to in [centre.x - px(radius), centre.x + px(radius)] {
            path.arc_to(
                point(px(radius), px(radius)),
                px(0.0),
                false,
                true,
                point(to, centre.y),
            );
        }
        path.close();
        path.build().ok()
    };
    if let Some(path) = disc(geometry.inner - 1.0, PathBuilder::fill()) {
        window.paint_path(path, rgb(theme::SURFACE));
    }
    if hover == Some(Hit::Center)
        && map.up
        && let Some(path) = disc(geometry.inner - 1.0, PathBuilder::fill())
    {
        window.paint_path(path, rgba(theme::FOREGROUND << 8 | 0x1a));
    }
    if let Some(path) = disc(geometry.inner - 1.0, PathBuilder::stroke(px(1.0))) {
        window.paint_path(path, rgba(ui::HAIRLINE));
    }
    words(map, centre, geometry.inner, window, cx);
}

/// The outline of a wedge pulled in by `inset` points on every side, unless that would leave nothing of it, in
/// `path`'s style. `None` when it is too small to see.
fn wedge_path(
    centre: gpui::Point<Pixels>,
    geometry: &Geometry,
    wedge: &Wedge,
    inset: f32,
    mut path: PathBuilder,
) -> Option<gpui::Path<Pixels>> {
    let r0 = geometry.inner + wedge.ring as f32 * geometry.ring_width;
    let r1 = r0 + geometry.ring_width;
    if r1 - r0 < 0.5 || (wedge.to - wedge.from) * r1 < 0.5 {
        return None;
    }
    let d = if r1 - r0 > 2.0 * inset && (wedge.to - wedge.from) * r0 > 2.0 * inset {
        inset
    } else {
        0.0
    };
    let (start, end) = (wedge.from - FRAC_PI_2, wedge.to - FRAC_PI_2);
    let (near, far) = (d / r0, d / r1);
    let at = |radius: f32, angle: f32| {
        point(
            centre.x + px(radius * angle.cos()),
            centre.y + px(radius * angle.sin()),
        )
    };
    // An arc is one stroke of at most half a circle: a wedge round the whole circle starts and ends at one point.
    let parts = if end - start > std::f32::consts::PI {
        2
    } else {
        1
    };
    let (a, b) = (start + far, end - far);
    let (c, e) = (start + near, end - near);
    path.move_to(at(r1 - d, a));
    for step in 1..=parts {
        let angle = a + (b - a) * step as f32 / parts as f32;
        path.arc_to(
            point(px(r1 - d), px(r1 - d)),
            px(0.0),
            false,
            true,
            at(r1 - d, angle),
        );
    }
    path.line_to(at(r0 + d, e));
    for step in 1..=parts {
        let angle = e + (c - e) * step as f32 / parts as f32;
        path.arc_to(
            point(px(r0 + d), px(r0 + d)),
            px(0.0),
            false,
            false,
            at(r0 + d, angle),
        );
    }
    path.close();
    path.build().ok()
}

/// The title and detail in the middle, each cut short with an ellipsis until it fits the circle.
fn words(
    map: &Sunburst,
    centre: gpui::Point<Pixels>,
    inner: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let size = (inner * 0.16).round().clamp(13.0, 19.0);
    let small = (size * 0.82).round().max(11.0);
    let gap = size * 0.78;
    // What a line can use: the circle's chord at the line's edge farthest from the middle, less some room.
    let room =
        |offset: f32| (2.0 * (inner * inner - offset * offset).max(0.0).sqrt() - 12.0).max(0.0);
    let style = window.text_style();
    let mut line =
        |text: &str, size: f32, bold: bool, color: u32, max: f32, y: f32, window: &mut Window| {
            if text.is_empty() {
                return;
            }
            let shape = |text: String, window: &mut Window| -> ShapedLine {
                let mut run = style.to_run(text.len());
                run.color = rgb(color).into();
                if bold {
                    run.font.weight = FontWeight::SEMIBOLD;
                }
                window
                    .text_system()
                    .shape_line(text.into(), px(size), &[run], None)
            };
            let mut shaped = shape(text.to_owned(), window);
            if f32::from(shaped.width) > max {
                // The longest start of the text that fits with an ellipsis.
                let chars: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
                let (mut low, mut high) = (0, chars.len().saturating_sub(1));
                while low < high {
                    let mid = (low + high).div_ceil(2);
                    let fit = shape(format!("{}…", &text[..chars[mid]]), window);
                    if f32::from(fit.width) <= max {
                        low = mid;
                    } else {
                        high = mid - 1;
                    }
                }
                shaped = shape(format!("{}…", text[..chars[low]].trim_end()), window);
            }
            let origin = point(
                centre.x - shaped.width / 2.0,
                centre.y + px(y) - px(size * 1.45 / 2.0),
            );
            let _ = shaped.paint(origin, px(size * 1.45), TextAlign::Left, None, window, cx);
        };
    line(
        &map.title,
        size,
        true,
        theme::FOREGROUND,
        room(gap + size / 2.0),
        -gap,
        window,
    );
    line(
        &map.detail,
        small,
        false,
        theme::MUTED_FOREGROUND,
        room(gap + small / 2.0),
        gap,
        window,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wedge_in_the_first_ring_has_an_outline_and_a_sliver_has_none() {
        let geometry = Geometry::of(400.0, 400.0, 6).unwrap();
        let centre = point(px(200.0), px(200.0));
        let wedge = |from, to| Wedge {
            from,
            to,
            ring: 0,
            color: 0,
            alpha: 1.0,
        };
        assert!(
            wedge_path(
                centre,
                &geometry,
                &wedge(0.0, 1.0),
                GAP,
                PathBuilder::fill()
            )
            .is_some()
        );
        assert!(
            wedge_path(
                centre,
                &geometry,
                &wedge(0.0, 0.001),
                GAP,
                PathBuilder::fill()
            )
            .is_none()
        );
        // A wedge over half the circle is a large arc, and a whole circle still has a path.
        assert!(
            wedge_path(
                centre,
                &geometry,
                &wedge(0.0, 5.0),
                GAP,
                PathBuilder::fill()
            )
            .is_some()
        );
    }
}
