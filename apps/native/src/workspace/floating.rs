//! A terminal panel floating over an area, as the desktop's `floating-panel.tsx`: drag its header to move it, its
//! top-left corner to resize it, and it shrinks to a bar when collapsed. The quick terminal and the terminals an agent
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

/// A move of the header or a resize from the corner: where the pointer and the panel were when it started.
#[derive(Clone, Copy)]
pub(super) struct Gesture {
    resize: bool,
    from: Point<Pixels>,
    rect: Rect,
}

impl Gesture {
    pub(super) fn new(resize: bool, from: Point<Pixels>, rect: Rect) -> Self {
        Self { resize, from, rect }
    }

    /// Where the panel is with the pointer at `position`, kept inside `area` as the desktop's.
    pub(super) fn rect_at(&self, position: Point<Pixels>, area: Bounds<Pixels>) -> Rect {
        let (dx, dy) = (
            (position.x - self.from.x).as_f32(),
            (position.y - self.from.y).as_f32(),
        );
        let (width, height) = (area.size.width.as_f32(), area.size.height.as_f32());
        let r = self.rect;
        if self.resize {
            Rect {
                w: (r.w - dx).clamp(MIN_W, (width - r.right).max(MIN_W)),
                h: (r.h - dy).clamp(MIN_H, (height - r.bottom).max(MIN_H)),
                ..r
            }
        } else {
            Rect {
                right: (r.right - dx).clamp(0.0, (width - r.w).max(0.0)),
                bottom: (r.bottom - dy).clamp(0.0, (height - r.h).max(0.0)),
                ..r
            }
        }
    }
}

/// Starts a move (`false`) or a resize (`true`) of the panel from the pointer's place.
pub(super) type Start = Rc<dyn Fn(&mut Workspace, bool, Point<Pixels>)>;
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
                        start(this, false, event.position);
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
            .bg(rgb(theme::SURFACE))
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
                    .border_color(rgba(theme::HAIRLINE_STRONG))
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
                                .bg(rgba(theme::POPOVER))
                        } else {
                            panel.w(px(w)).h(px(h)).rounded(px(12.0))
                        }
                    })
            })
            .when(!collapsed && !full, |panel| {
                panel.child(
                    div()
                        .id(SharedString::from(format!("{id}-resize")))
                        .absolute()
                        .top_0()
                        .left_0()
                        .size(px(16.0))
                        .cursor_nwse_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                resize(this, true, event.position);
                                cx.stop_propagation();
                            }),
                        ),
                )
            });
        panel.into_any_element()
    }
}
