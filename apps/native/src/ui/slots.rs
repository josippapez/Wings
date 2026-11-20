//! Places that a piece of the window that keeps its last drawing (`workspace::parts`) leaves for what is drawn over it
//! later in the window, so that can change without the piece drawing itself again: the spinners in it, which
//! `spinner_layer` turns, and its terminals, which a `slot_layer` draws.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, App, Bounds, ContentMask, Element, EntityId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Position, SharedString, Style, Window, WindowId, canvas,
    prelude::*,
};

/// A piece of the window that keeps its last drawing: one of the workspace's, by name, or a view that one of them puts
/// on screen, a page say, by its entity. A view's slots count only while the piece that put it there still does
/// (`place_view`): a page left behind by a tab switch keeps its last drawing, and its spinners must not turn on over
/// whatever took its place.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Piece {
    Named(&'static str),
    View(EntityId),
}

impl From<&'static str> for Piece {
    fn from(name: &'static str) -> Self {
        Piece::Named(name)
    }
}

/// What draws over a slot after the layer has drawn in it, clipped to the slot.
pub type Over = Rc<dyn Fn(&mut Window)>;

/// A slot's place, what clips it there, and what draws over it.
#[derive(Clone)]
pub struct Slot {
    pub bounds: Bounds<Pixels>,
    pub mask: ContentMask<Pixels>,
    over: Option<Over>,
    /// The draw that left it there.
    drawn: u64,
}

#[derive(Default)]
struct Slots {
    /// The window's draws so far, counted by `start_draw`.
    frame: HashMap<WindowId, u64>,
    /// The draw each piece of a window last drew itself in.
    drawn: HashMap<(WindowId, Piece), u64>,
    /// Each slot by window, piece, the layer that draws in it, and name.
    slots: HashMap<(WindowId, Piece, &'static str, SharedString), Slot>,
    /// The piece being drawn, whose the slots made now are.
    current: Option<Piece>,
    /// Which piece last put each view on screen in a window, and in which of its draws (`place_view`).
    placed: HashMap<(WindowId, EntityId), (Piece, u64)>,
}

impl Slots {
    /// Whether the slots `piece` left in the window `id` still count: a named piece's always do, a view's while the
    /// piece that put it on screen last did so in the draw it last drew itself in.
    fn on_screen(&self, id: WindowId, piece: Piece) -> bool {
        match piece {
            Piece::Named(_) => true,
            Piece::View(view) => self.placed.get(&(id, view)).is_some_and(|&(by, at)| {
                by != piece && self.drawn.get(&(id, by)) == Some(&at) && self.on_screen(id, by)
            }),
        }
    }
}

/// The piece the window's own drawing is, outside any `drawing_piece`.
const WINDOW: Piece = Piece::Named("window");

thread_local! {
    static SLOTS: RefCell<Slots> = RefCell::default();
}

/// Runs `draw`, the drawing of the piece `piece` of the window `id`, so the slots it leaves are its own.
pub fn drawing_piece<R>(piece: impl Into<Piece>, id: WindowId, draw: impl FnOnce() -> R) -> R {
    let piece = piece.into();
    let outer = SLOTS.with_borrow_mut(|slots| {
        let frame = slots.frame.get(&id).copied().unwrap_or_default();
        slots.drawn.insert((id, piece), frame);
        slots.current.replace(piece)
    });
    let drawn = draw();
    SLOTS.with_borrow_mut(|slots| slots.current = outer);
    drawn
}

/// Says that the piece being drawn puts `view` on screen in the window `id`, in this draw of it: the slots `view` leaves
/// as the piece `Piece::View` count until that piece draws itself again without saying so. A view that keeps its last
/// drawing (`.cached`) and is put there again finds its slots still there.
pub fn place_view(view: EntityId, id: WindowId) {
    SLOTS.with_borrow_mut(|slots| {
        let by = slots.current.unwrap_or(WINDOW);
        let frame = slots.frame.get(&id).copied().unwrap_or_default();
        slots.placed.insert((id, view), (by, frame));
    });
}

/// Whether a piece now puts `view` on screen in the window `id` (`place_view`), so the slots it leaves will be drawn in.
pub fn view_placed(view: EntityId, id: WindowId) -> bool {
    SLOTS.with_borrow(|slots| slots.on_screen(id, Piece::View(view)))
}

/// Lets go of what `view` left in every window: its slots and where it was put. For a view that is gone.
pub fn forget_view(view: EntityId) {
    SLOTS.with_borrow_mut(|slots| {
        let piece = Piece::View(view);
        slots.slots.retain(|(_, of, _, _), _| *of != piece);
        slots.drawn.retain(|(_, of), _| *of != piece);
        slots.placed.retain(|(_, placed), _| *placed != view);
    });
}

/// Starts a draw of the window `id`: the slots its pieces leave from now on are this draw's. Once in each draw of the
/// window, before its pieces with slots draw themselves, and before anything says it puts a view on screen
/// (`place_view`): the workspace does it first in its draw, `spinner_layer` elsewhere.
pub fn start_draw(id: WindowId) {
    SLOTS.with_borrow_mut(|slots| {
        let frame = slots.frame.entry(id).or_default();
        *frame += 1;
        let frame = *frame;
        slots.drawn.insert((id, WINDOW), frame);
    });
}

/// The place, in a piece drawn with `drawing_piece`, where `layer` draws what it has for `name`, then `over` over it.
pub fn slot(
    layer: &'static str,
    name: impl Into<SharedString>,
    over: Option<Over>,
) -> impl IntoElement {
    let piece = SLOTS.with_borrow(|slots| slots.current).unwrap_or(WINDOW);
    let name = name.into();
    canvas(
        move |bounds, window, _| {
            let id = window.window_handle().window_id();
            let mask = window.content_mask();
            SLOTS.with_borrow_mut(|slots| {
                let drawn = slots.frame.get(&id).copied().unwrap_or_default();
                let slot = Slot {
                    bounds,
                    mask,
                    over,
                    drawn,
                };
                slots.slots.insert((id, piece, layer, name), slot);
            });
        },
        |_, _, _, _| {},
    )
    .size_full()
}

/// The slots of `layer` in the window `id` still there: left in the draw their piece last drew itself in, by a piece on
/// screen. A view's slots wait while nothing puts it on screen, for a tab switched back to that is drawn from its cache.
pub(super) fn shown(id: WindowId, layer: &'static str) -> Vec<(SharedString, Slot)> {
    SLOTS.with_borrow_mut(|state| {
        let Slots { drawn, slots, .. } = &mut *state;
        slots.retain(|(window, piece, _, _), slot| {
            *window != id || drawn.get(&(*window, *piece)) == Some(&slot.drawn)
        });
        let state = &*state;
        state
            .slots
            .iter()
            .filter(|((window, piece, of, _), _)| *window == id && *of == layer && state.on_screen(id, *piece))
            .map(|((.., name), slot)| (name.clone(), slot.clone()))
            .collect()
    })
}

/// Draws each of `children` at the slot of `layer` named for it, over everything drawn before it in the window: laid
/// out at the slot's place and size, clipped as the slot is, then what the slot draws over it. A child without a slot
/// isn't drawn. Draw it after the pieces with the slots, in each draw of the window.
pub fn slot_layer(layer: &'static str, children: Vec<(SharedString, AnyElement)>) -> SlotLayer {
    SlotLayer { layer, children }
}

pub struct SlotLayer {
    layer: &'static str,
    children: Vec<(SharedString, AnyElement)>,
}

impl Element for SlotLayer {
    type RequestLayoutState = ();
    type PrepaintState = Vec<(AnyElement, Slot)>;

    fn id(&self) -> Option<gpui::ElementId> {
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
        // Out of its parent's flow: the children take their places from their slots.
        let style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<(AnyElement, Slot)> {
        let shown = shown(window.window_handle().window_id(), self.layer);
        let mut drawn = Vec::new();
        for (name, mut child) in self.children.drain(..) {
            let Some((_, slot)) = shown.iter().find(|(at, _)| *at == name) else {
                continue;
            };
            // Laid out in the same draw as the piece left the slot, so a resize or a split never lags a frame.
            window.with_content_mask(Some(slot.mask), |window| {
                child.prepaint_as_root(slot.bounds.origin, slot.bounds.size.into(), window, cx)
            });
            drawn.push((child, slot.clone()));
        }
        drawn
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        drawn: &mut Vec<(AnyElement, Slot)>,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (child, slot) in drawn {
            window.with_content_mask(Some(slot.mask), |window| {
                child.paint(window, cx);
                if let Some(over) = &slot.over {
                    window.with_content_mask(
                        Some(ContentMask {
                            bounds: slot.bounds,
                        }),
                        |window| over(window),
                    );
                }
            });
        }
    }
}
