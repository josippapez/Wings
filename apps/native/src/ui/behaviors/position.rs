//! `position`: where a popup goes beside its anchor (UI SDK's `position(anchor, element, { side, align, sideOffset,
//! collisionPadding })`). A pure function of the anchor's bounds, the popup's measured size and the window's size.

use gpui::{Bounds, Pixels, Point, Size, point, px};

/// Which side of its anchor a popup opens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Bottom,
    Top,
}

impl Side {
    fn other(self) -> Self {
        match self {
            Side::Bottom => Side::Top,
            Side::Top => Side::Bottom,
        }
    }
}

/// Which of the anchor's edges a popup lines up with: its left with the anchor's left (`Start`), or its right with the
/// anchor's right (`End`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    End,
}

/// Where a popup wants to be: the side and edge it prefers, the room between it and its anchor (`side_offset`), and
/// the room it keeps from the window's edges (`collision_padding`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub side: Side,
    pub align: Align,
    pub side_offset: Pixels,
    pub collision_padding: Pixels,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            side: Side::Bottom,
            align: Align::Start,
            side_offset: px(4.0),
            collision_padding: px(8.0),
        }
    }
}

/// How tall a popup may be under its anchor and over it, inside the collision padding. A list that scrolls caps its
/// height at the larger of the two, and `place` then opens it on that side.
pub fn room(anchor: Bounds<Pixels>, window: Size<Pixels>, placement: Placement) -> (Pixels, Pixels) {
    let below = window.height - placement.collision_padding - (anchor.bottom() + placement.side_offset);
    let above = anchor.top() - placement.side_offset - placement.collision_padding;
    (below.max(Pixels::ZERO), above.max(Pixels::ZERO))
}

/// Where a popup of `size` goes for `anchor` in a window of `window` (window coordinates), and the side it went on:
/// - its preferred side when it fits there, else the other side when it fits there, else the side with more room;
/// - its top kept inside the padding, shifted up to end inside it too, so only one taller than the window runs off its foot;
/// - lined up with the anchor's start or end edge, then moved left or right to stay inside the padding, and to the
///   left padding when it's wider than the window.
pub fn place(
    anchor: Bounds<Pixels>,
    size: Size<Pixels>,
    window: Size<Pixels>,
    placement: Placement,
) -> (Point<Pixels>, Side) {
    let (below, above) = room(anchor, window, placement);
    let room_on = |side| match side {
        Side::Bottom => below,
        Side::Top => above,
    };
    let preferred = placement.side;
    let other = preferred.other();
    let side = if size.height <= room_on(preferred) {
        preferred
    } else if size.height <= room_on(other) || room_on(other) > room_on(preferred) {
        other
    } else {
        preferred
    };
    let y = match side {
        Side::Bottom => anchor.bottom() + placement.side_offset,
        Side::Top => anchor.top() - placement.side_offset - size.height,
    }
    // Too tall for its side: moved up (or down) to stay whole inside the padding, and only a popup taller than the
    // window runs off its foot.
    .min(window.height - placement.collision_padding - size.height)
    .max(placement.collision_padding);
    let x = match placement.align {
        Align::Start => anchor.left(),
        Align::End => anchor.right() - size.width,
    }
    .min(window.width - placement.collision_padding - size.width)
    .max(placement.collision_padding);
    (point(x, y), side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    fn anchor(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    fn window() -> Size<Pixels> {
        size(px(800.0), px(600.0))
    }

    fn popup(w: f32, h: f32) -> Size<Pixels> {
        size(px(w), px(h))
    }

    #[test]
    fn fits_below() {
        let (at, side) = place(anchor(100.0, 100.0, 80.0, 32.0), popup(200.0, 150.0), window(), Placement::default());
        assert_eq!(side, Side::Bottom);
        assert_eq!(at, point(px(100.0), px(136.0)));
    }

    #[test]
    fn flips_above_without_room_below() {
        // A trigger at the window's foot, as a chat's message box: 600 - 8 - (560 + 4) = 28pt under it.
        let (at, side) = place(anchor(100.0, 528.0, 80.0, 32.0), popup(200.0, 150.0), window(), Placement::default());
        assert_eq!(side, Side::Top);
        assert_eq!(at, point(px(100.0), px(528.0 - 4.0 - 150.0)));
    }

    #[test]
    fn prefers_top_when_asked_and_flips_below() {
        let placement = Placement {
            side: Side::Top,
            ..Placement::default()
        };
        let (_, side) = place(anchor(100.0, 400.0, 80.0, 32.0), popup(200.0, 150.0), window(), placement);
        assert_eq!(side, Side::Top);
        let (at, side) = place(anchor(100.0, 40.0, 80.0, 32.0), popup(200.0, 150.0), window(), placement);
        assert_eq!(side, Side::Bottom);
        assert_eq!(at.y, px(76.0));
    }

    #[test]
    fn clamps_at_the_right_edge() {
        let (at, _) = place(anchor(700.0, 100.0, 80.0, 32.0), popup(200.0, 100.0), window(), Placement::default());
        assert_eq!(at.x, px(800.0 - 8.0 - 200.0));
    }

    #[test]
    fn clamps_at_the_left_edge_when_end_aligned() {
        let placement = Placement {
            align: Align::End,
            ..Placement::default()
        };
        let (at, _) = place(anchor(20.0, 100.0, 40.0, 32.0), popup(200.0, 100.0), window(), placement);
        assert_eq!(at.x, px(8.0));
        let (at, _) = place(anchor(400.0, 100.0, 80.0, 32.0), popup(200.0, 100.0), window(), placement);
        assert_eq!(at.x, px(280.0));
    }

    #[test]
    fn wider_than_the_window_sits_at_the_left_padding() {
        let (at, _) = place(anchor(300.0, 100.0, 80.0, 32.0), popup(900.0, 100.0), window(), Placement::default());
        assert_eq!(at.x, px(8.0));
    }

    #[test]
    fn too_tall_for_both_takes_the_side_with_more_room() {
        // 400pt over the anchor, 144pt under it: over it, its top inside the padding.
        let (at, side) = place(anchor(100.0, 412.0, 80.0, 32.0), popup(200.0, 500.0), window(), Placement::default());
        assert_eq!(side, Side::Top);
        assert_eq!(at.y, px(8.0));
        // 56pt over it, 488pt under it: stays under it, shifted up so its foot is inside the padding.
        let (at, side) = place(anchor(100.0, 68.0, 80.0, 32.0), popup(200.0, 560.0), window(), Placement::default());
        assert_eq!(side, Side::Bottom);
        assert_eq!(at.y, px(32.0));
        // Taller than the window: its top at the padding.
        let (at, _) = place(anchor(100.0, 68.0, 80.0, 32.0), popup(200.0, 700.0), window(), Placement::default());
        assert_eq!(at.y, px(8.0));
    }

    #[test]
    fn a_point_opens_at_it_or_over_it() {
        let placement = Placement {
            side_offset: px(0.0),
            ..Placement::default()
        };
        let (at, _) = place(anchor(300.0, 200.0, 0.0, 0.0), popup(160.0, 120.0), window(), placement);
        assert_eq!(at, point(px(300.0), px(200.0)));
        let (at, side) = place(anchor(300.0, 550.0, 0.0, 0.0), popup(160.0, 120.0), window(), placement);
        assert_eq!(side, Side::Top);
        assert_eq!(at, point(px(300.0), px(430.0)));
    }

    #[test]
    fn room_on_each_side() {
        let (below, above) = room(anchor(100.0, 100.0, 80.0, 32.0), window(), Placement::default());
        assert_eq!(below, px(600.0 - 8.0 - 136.0));
        assert_eq!(above, px(88.0));
    }
}
