//! Places that a piece of the window that keeps its last drawing (`workspace::parts`) leaves for what is drawn over it
//! later in the window, so that can change without the piece drawing itself again: the spinners in it, which
//! `spinner_layer` turns, its terminals, which a `slot_layer` draws, and the text of the field being typed in, which the
//! field layer draws (`text_field::FieldLayer`). And where the overlays of a draw are (`cover`), which what turns over
//! GPUI's drawing (`spinner_host`) keeps clear of.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

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
    /// The piece that left it, whose slots' names are its own.
    pub piece: Piece,
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
    /// The name the next cover in a draw takes (`cover`).
    static NEXT_COVER: Cell<u64> = const { Cell::new(0) };
    /// What is being laid out, prepainted or painted is under something its own piece draws over it (`in_piece`).
    static COVERED: Cell<bool> = const { Cell::new(false) };
    /// Where its own piece draws something over part of what is being painted (`under`).
    static OVER: RefCell<Vec<Rc<Cell<Bounds<Pixels>>>>> = RefCell::default();
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

/// `element`, what a piece's render returns, laid out, prepainted and painted as that piece's drawing: `drawing_piece`
/// only covers building it, and GPUI renders the views inside it (a text field) later, as it lays them out, so those
/// learn their piece from this. `piece` None keeps the piece around it. `covered`: the piece draws something of its own
/// over what is in it (a page's dialog or sheet), so nothing in it may draw in a layer over the window's pieces.
pub fn in_piece(piece: Option<Piece>, covered: bool, element: impl IntoElement) -> InPiece {
    InPiece {
        piece,
        covered,
        element: element.into_any_element(),
    }
}

pub struct InPiece {
    piece: Option<Piece>,
    covered: bool,
    element: AnyElement,
}

/// Runs `run` inside the piece `piece` (the one around it for None), `covered` or under a piece that is.
fn inside<R>(piece: Option<Piece>, covered: bool, run: impl FnOnce() -> R) -> R {
    let outer = SLOTS.with_borrow_mut(|slots| {
        let outer = slots.current;
        if piece.is_some() {
            slots.current = piece;
        }
        outer
    });
    let was = COVERED.replace(COVERED.get() || covered);
    let done = run();
    COVERED.set(was);
    SLOTS.with_borrow_mut(|slots| slots.current = outer);
    done
}

impl Element for InPiece {
    type RequestLayoutState = ();
    type PrepaintState = ();

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
        (inside(self.piece, self.covered, || self.element.request_layout(window, cx)), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        inside(self.piece, self.covered, || self.element.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        inside(self.piece, self.covered, || self.element.paint(window, cx));
    }
}

/// The piece being painted, when it is one that keeps its last drawing and draws nothing of its own over what is
/// painted now at `bounds` (`in_piece`, `under`): that may draw in a layer over the window's pieces instead.
pub(crate) fn uncovered_piece(bounds: Bounds<Pixels>) -> Option<Piece> {
    let piece = current_piece();
    let over = OVER.with_borrow(|over| over.iter().any(|at| at.get().intersects(&bounds)));
    (piece != WINDOW && !COVERED.get() && !over).then_some(piece)
}

/// `element`, over part of which the piece it is in draws something of its own later in the same draw (a page's pane
/// floating over its content), at the bounds `over` holds once the draw has laid it out: what is painted in `element`
/// where that is draws in place, under it (`uncovered_piece`).
pub fn under(over: Rc<Cell<Bounds<Pixels>>>, element: impl IntoElement) -> Under {
    Under {
        over,
        element: element.into_any_element(),
    }
}

pub struct Under {
    over: Rc<Cell<Bounds<Pixels>>>,
    element: AnyElement,
}

impl Element for Under {
    type RequestLayoutState = ();
    type PrepaintState = ();

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
        (self.element.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        OVER.with_borrow_mut(|over| over.push(self.over.clone()));
        self.element.paint(window, cx);
        OVER.with_borrow_mut(|over| over.pop());
    }
}

/// Where it is drawn, kept in `at` in each draw for `under`: put it in what is drawn over the content, as the child
/// that fills it.
pub fn bounds_into(at: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |bounds, _, _| at.set(bounds), |_, _, _, _| {}).absolute().top_0().left_0().size_full()
}

/// The window's draws so far (`start_draw`).
pub(crate) fn draws(id: WindowId) -> u64 {
    SLOTS.with_borrow(|slots| slots.frame.get(&id).copied().unwrap_or_default())
}

/// Leaves the slot `name` of `layer` at `bounds`, as the piece `piece` does in this draw (`uncovered_piece`), for what
/// draws in that layer later in the draw.
pub(crate) fn leave_slot(window: &Window, piece: Piece, layer: &'static str, name: SharedString, bounds: Bounds<Pixels>) {
    leave(window, piece, layer, name, bounds, None);
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
    // A cover's name needs to be its own only in its piece's draw: names start again, so a piece's covers drawn again
    // take the places of its last ones.
    NEXT_COVER.with(|next| next.set(0));
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
    let piece = current_piece();
    let name = name.into();
    canvas(
        move |bounds, window, _| leave(window, piece, layer, name, bounds, over),
        |_, _, _, _| {},
    )
    .size_full()
}

/// The piece being drawn, whose the slots made now are.
fn current_piece() -> Piece {
    SLOTS.with_borrow(|slots| slots.current).unwrap_or(WINDOW)
}

/// Leaves the slot `name` of `layer` at `bounds` in the window being drawn, as the piece `piece` does in this draw,
/// clipped as what is drawn there now.
fn leave(
    window: &Window,
    piece: Piece,
    layer: &'static str,
    name: SharedString,
    bounds: Bounds<Pixels>,
    over: Option<Over>,
) {
    let id = window.window_handle().window_id();
    let mask = window.content_mask();
    SLOTS.with_borrow_mut(|slots| {
        let drawn = slots.frame.get(&id).copied().unwrap_or_default();
        let slot = Slot {
            bounds,
            mask,
            piece,
            over,
            drawn,
        };
        slots.slots.insert((id, piece, layer, name), slot);
    });
}

/// The layer the overlays' places are kept in (`cover`).
const COVERS: &str = "covers";

/// `element`, an overlay (a menu, a popover, a dialog, a tooltip), and a note of where it is in each draw of the window:
/// what turns over GPUI's drawing of the window (`spinner_host`) gives way where one is. Kept as a slot of the piece
/// being drawn, so an overlay in a piece that keeps its last drawing counts for as long as the piece keeps it. Laid out,
/// drawn and placed as `element` alone.
pub fn cover(element: impl IntoElement) -> Cover {
    let name = NEXT_COVER.with(|next| {
        let name = next.get();
        next.set(name + 1);
        name
    });
    Cover {
        piece: current_piece(),
        name: name.to_string().into(),
        element: element.into_any_element(),
    }
}

pub struct Cover {
    piece: Piece,
    name: SharedString,
    element: AnyElement,
}

impl Element for Cover {
    type RequestLayoutState = ();
    type PrepaintState = ();

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
        // The element's own box, so the cover's bounds are the element's, wherever its style puts it.
        (self.element.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        leave(window, self.piece, COVERS, self.name.clone(), bounds, None);
        self.element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element.paint(window, cx);
    }
}

/// Where the overlays of the window `id` are in its last draw (`cover`), as far as each shows.
pub(super) fn covers(id: WindowId) -> Vec<Bounds<Pixels>> {
    shown(id, COVERS)
        .into_iter()
        .map(|(_, slot)| slot.bounds.intersect(&slot.mask.bounds))
        .filter(|bounds| !bounds.is_empty())
        .collect()
}

/// The slots of `layer` in the window `id` still there: left in the draw their piece last drew itself in, by a piece on
/// screen. A view's slots wait while nothing puts it on screen, for a tab switched back to that is drawn from its cache.
pub(crate) fn shown(id: WindowId, layer: &'static str) -> Vec<(SharedString, Slot)> {
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
