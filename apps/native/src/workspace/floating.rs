//! A terminal panel floating over an area, as the desktop's `floating-panel.tsx`: drag its header to move it, any edge
//! or corner to resize it, and it shrinks to a bar when collapsed. The quick terminal and the terminals an agent
//! opened float in one. A gesture moves only the panel; the terminal in it refits its shell once the size holds still
//! (`TerminalView`'s resize delay), as the desktop's refits once the gesture ends.

use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, BoxShadow, Context, MouseButton, MouseDownEvent, Pixels, Point, Role,
    SharedString, Window, div, hsla, point, prelude::*, px, rgb, rgba,
};

use crate::{theme, ui};

use super::{HAIRLINE, Workspace};

/// The desktop's `PANEL_EDGE`, and the panel's smallest size.
pub(super) const EDGE: f32 = 12.0;
const MIN_W: f32 = 280.0;
const MIN_H: f32 = 120.0;

/// Where the panel sits, as distances from the bottom-right corner of the area it floats over, so a smaller window
/// never leaves it out of view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
    pub(super) right: f32,
    pub(super) bottom: f32,
    pub(super) w: f32,
    pub(super) h: f32,
}

/// The sides of the panel a drag takes with it: an edge, two for a corner, none to move the whole panel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Sides {
    pub(super) left: bool,
    pub(super) top: bool,
    pub(super) right: bool,
    pub(super) bottom: bool,
}

impl Sides {
    /// The header's drag, which moves the panel.
    pub(super) const MOVE: Sides = Sides {
        left: false,
        top: false,
        right: false,
        bottom: false,
    };
}

/// A move of the header or a resize from an edge or corner: where the pointer and the panel were when it started.
#[derive(Clone, Copy)]
pub(super) struct Gesture {
    sides: Sides,
    from: Point<Pixels>,
    rect: Rect,
}

impl Gesture {
    pub(super) fn new(sides: Sides, from: Point<Pixels>, rect: Rect) -> Self {
        Self { sides, from, rect }
    }

    /// Where the panel is with the pointer at `position`, kept inside `area` as the desktop's.
    pub(super) fn rect_at(&self, position: Point<Pixels>, area: Bounds<Pixels>) -> Rect {
        let (dx, dy) = (
            (position.x - self.from.x).as_f32(),
            (position.y - self.from.y).as_f32(),
        );
        let (width, height) = (area.size.width.as_f32(), area.size.height.as_f32());
        let (r, sides) = (self.rect, self.sides);
        if sides == Sides::MOVE {
            return Rect {
                right: (r.right - dx).clamp(0.0, (width - r.w).max(0.0)),
                bottom: (r.bottom - dy).clamp(0.0, (height - r.h).max(0.0)),
                ..r
            };
        }
        let mut next = r;
        // The panel is placed from the area's bottom-right corner, so the left and top edges change only its size, and
        // the right and bottom edges move that corner too, keeping the opposite edge where it was.
        if sides.left {
            next.w = (r.w - dx).clamp(MIN_W, (width - r.right).max(MIN_W));
        }
        if sides.right {
            next.right = (r.right - dx).clamp(0.0, (r.right + r.w - MIN_W).max(0.0));
            next.w = r.w + r.right - next.right;
        }
        if sides.top {
            next.h = (r.h - dy).clamp(MIN_H, (height - r.bottom).max(MIN_H));
        }
        if sides.bottom {
            next.bottom = (r.bottom - dy).clamp(0.0, (r.bottom + r.h - MIN_H).max(0.0));
            next.h = r.h + r.bottom - next.bottom;
        }
        next
    }
}

/// How far in from its edge an edge's grip reaches, and the side of a corner's square.
const GRIP: f32 = 6.0;
const CORNER: f32 = 14.0;

/// Starts a move (`Sides::MOVE`) or a resize of those sides of the panel from the pointer's place.
pub(super) type Start = Rc<dyn Fn(&mut Workspace, Sides, Point<Pixels>)>;
/// A click on the collapsed bar.
pub(super) type Expand = Rc<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// One panel, the desktop's `FloatingPanel`.
pub(super) struct Panel {
    /// Names its elements.
    pub(super) id: SharedString,
    /// What a screen reader calls it.
    pub(super) label: SharedString,
    pub(super) rect: Rect,
    /// The area it floats over, as last measured.
    pub(super) area: Option<Bounds<Pixels>>,
    pub(super) collapsed: bool,
    /// Fills the area in the frame a tab has, instead of floating: no moving or resizing.
    pub(super) full: bool,
}

impl Panel {
    /// The frame around `header`'s contents and `body`, which shows only while it isn't collapsed.
    pub(super) fn render(
        self,
        header: Vec<AnyElement>,
        body: AnyElement,
        start: Start,
        expand: Expand,
        cx: &mut Context<Workspace>,
    ) -> AnyElement {
        let Panel {
            id,
            label,
            rect,
            area,
            collapsed,
            full,
        } = self;
        let resize = start.clone();
        let header = div()
            .id(SharedString::from(format!("{id}-header")))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .h(px(if collapsed { 28.0 } else { 32.0 }))
            .pl(px(12.0))
            .pr(px(4.0))
            .text_size(px(12.0))
            .when(!full, |header| {
                header.cursor_grab().on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        start(this, Sides::MOVE, event.position);
                        cx.stop_propagation();
                    }),
                )
            })
            .when(collapsed, |header| {
                header.on_click(cx.listener(move |this, _, window, cx| expand(this, window, cx)))
            })
            .children(header);
        let spot = id.to_string();
        let panel = div()
            .id(id.clone())
            .role(Role::Group)
            .aria_label(label)
            .children(ui::spot(|| spot))
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_1()
            .bg(rgb(theme::surface()))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .when(!collapsed, |panel| {
                panel.child(div().flex_1().min_h_0().child(body))
            })
            .map(|panel| {
                if full {
                    return panel
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .rounded(px(14.0))
                        .border_color(rgba(HAIRLINE));
                }
                // Never wider or taller than the area it floats over, like the desktop's `max-w-full`, and clear of
                // its far edges, so in a narrow pane it doesn't sit on the pane's border.
                let (w, h) = area.map_or((rect.w, rect.h), |area| {
                    (
                        rect.w.min(area.size.width.as_f32() - rect.right - EDGE),
                        rect.h.min(area.size.height.as_f32() - rect.bottom - EDGE),
                    )
                });
                panel
                    .absolute()
                    .right(px(rect.right))
                    .bottom(px(rect.bottom))
                    .border_color(rgba(theme::hairline_strong()))
                    .shadow(vec![BoxShadow {
                        color: hsla(0.0, 0.0, 0.0, 0.35),
                        offset: point(px(0.0), px(10.0)),
                        blur_radius: px(15.0),
                        spread_radius: px(-3.0),
                        inset: false,
                    }])
                    .map(|panel| {
                        if collapsed {
                            // As wide as its header, but never past the area's left edge, like the desktop's
                            // `max-w-full`.
                            panel
                                .h(px(28.0))
                                .when_some(area, |panel, area| {
                                    panel.max_w(px(area.size.width.as_f32() - rect.right))
                                })
                                .rounded_full()
                                .bg(rgba(theme::popover()))
                        } else {
                            panel.w(px(w)).h(px(h)).rounded(px(12.0))
                        }
                    })
            })
            .when(!collapsed && !full, |panel| {
                // The corners after the edges, so they take the press where the two meet.
                panel.children(GRIPS.iter().map(|&(name, sides)| {
                    let resize = resize.clone();
                    let (horizontal, vertical) =
                        (sides.left || sides.right, sides.top || sides.bottom);
                    div()
                        .id(SharedString::from(format!("{id}-resize-{name}")))
                        .absolute()
                        .map(|grip| match (horizontal, vertical) {
                            (true, false) => grip.top_0().bottom_0().w(px(GRIP)).cursor_ew_resize(),
                            (false, _) => grip.left_0().right_0().h(px(GRIP)).cursor_ns_resize(),
                            _ if sides.left == sides.top => {
                                grip.size(px(CORNER)).cursor_nwse_resize()
                            }
                            _ => grip.size(px(CORNER)).cursor_nesw_resize(),
                        })
                        .when(sides.left, |grip| grip.left_0())
                        .when(sides.right, |grip| grip.right_0())
                        .when(sides.top, |grip| grip.top_0())
                        .when(sides.bottom, |grip| grip.bottom_0())
                        .children(ui::spot(|| format!("{id}-resize-{name}")))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                resize(this, sides, event.position);
                                cx.stop_propagation();
                            }),
                        )
                }))
            });
        panel.into_any_element()
    }
}

/// The edges and corners that resize the panel, each with the sides it moves.
const GRIPS: [(&str, Sides); 8] = {
    const fn sides(left: bool, top: bool, right: bool, bottom: bool) -> Sides {
        Sides {
            left,
            top,
            right,
            bottom,
        }
    }
    [
        ("left", sides(true, false, false, false)),
        ("right", sides(false, false, true, false)),
        ("top", sides(false, true, false, false)),
        ("bottom", sides(false, false, false, true)),
        ("top-left", sides(true, true, false, false)),
        ("top-right", sides(false, true, true, false)),
        ("bottom-left", sides(true, false, false, true)),
        ("bottom-right", sides(false, false, true, true)),
    ]
};
