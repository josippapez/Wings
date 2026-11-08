//! Drags the window follows wherever the pointer goes, and the edge you drag to resize a panel.

use gpui::{
    Context, DispatchPhase, Div, Entity, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Orientation, Pixels, Role, Stateful, Window, canvas, div, prelude::*, px, rgba,
};

use crate::theme;

/// The width of the strip at a panel's edge that takes the drag.
pub const RESIZE_EDGE: f32 = 8.0;

/// Which side of a resize edge its line is drawn on: where the edge meets its panel's border.
#[derive(Clone, Copy)]
pub enum Side {
    Left,
    Right,
}

/// Listens to the whole window, so a drag goes on wherever the pointer is: each move and each button coming up, in the
/// bubble phase, go to `entity`. Call it while painting; the listeners last that frame.
pub fn window_drag<T: 'static>(
    window: &mut Window,
    entity: &Entity<T>,
    on_move: impl Fn(&mut T, &MouseMoveEvent, &mut Window, &mut Context<T>) + 'static,
    on_up: impl Fn(&mut T, &MouseUpEvent, &mut Context<T>) + 'static,
) {
    let moved = entity.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            moved.update(cx, |this, cx| on_move(this, event, window, cx));
        }
    });
    let released = entity.clone();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            released.update(cx, |this, cx| on_up(this, event, cx));
        }
    });
}

/// The edge you drag to resize a panel, `id` naming it and its hover group, for the caller to place. Invisible until you
/// point at it; its line, on `line`'s side, shows then and while it is dragged. `on_down` gets where it was pressed.
/// While `dragging`, the window follows the pointer wherever it is: `on_move` gets where it moved to, and `on_end` runs
/// when the button comes up, or when a move comes without it because it was let go outside the window.
#[allow(clippy::too_many_arguments)]
pub fn resize_edge<T: 'static>(
    id: &'static str,
    label: &'static str,
    line: Side,
    dragging: bool,
    cx: &mut Context<T>,
    on_down: impl Fn(&mut T, Pixels, &mut Window, &mut Context<T>) + 'static,
    on_move: impl Fn(&mut T, Pixels, &mut Window, &mut Context<T>) + 'static,
    on_end: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
) -> Stateful<Div> {
    let entity = cx.entity();
    div()
        .id(id)
        .group(id)
        .role(Role::Splitter)
        .aria_label(label)
        .aria_orientation(Orientation::Vertical)
        .absolute()
        .top_0()
        .bottom_0()
        .w(px(RESIZE_EDGE))
        .cursor_col_resize()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                on_down(this, event.position.x, window, cx)
            }),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .map(|div| match line {
                    Side::Left => div.left_0(),
                    Side::Right => div.right_0(),
                })
                .w(px(1.0))
                .when(dragging, |line| line.bg(rgba(theme::HAIRLINE_STRONG)))
                .group_hover(id, |style| style.bg(rgba(theme::HAIRLINE_STRONG))),
        )
        .when(dragging, |edge| {
            edge.child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        window_drag(
                            window,
                            &entity,
                            move |this, event, window, cx| {
                                if event.dragging() {
                                    on_move(this, event.position.x, window, cx);
                                } else {
                                    on_end(this, cx);
                                }
                            },
                            move |this, _, cx| on_end(this, cx),
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
        })
}
