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
    IntoElement, LayoutId, Pixels, Position, SharedString, Style, Window, WindowId, canvas, div,
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
    /// How round an overlay's corners are, when it says (`Cover::rounded`).
    pub round: Option<f32>,
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
    /// What clips the slots left now, when not the content mask (`clipped_as`).
    static CLIP: RefCell<Option<ContentMask<Pixels>>> = const { RefCell::new(None) };
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

/// Runs `run`, a prepaint under a content mask that keeps the pointer off what it holds (a dialog on its way out), with
/// the slots it leaves clipped by `mask`, as what is painted there is: the dialog's backdrop, gone from the slots, left a
/// web page under it black while it faded.
pub(crate) fn clipped_as<R>(mask: ContentMask<Pixels>, run: impl FnOnce() -> R) -> R {
    let outer = CLIP.replace(Some(mask));
    let done = run();
    CLIP.set(outer);
    done
}

/// The view being painted, when it is one a piece puts on screen, whether or not it draws something of its own over all
/// of it (a page's dialog, which says where it is with `cover` and `backdrop_mark`).
pub(crate) fn view_piece() -> Option<Piece> {
    Some(current_piece()).filter(|piece| matches!(piece, Piece::View(_)))
}

/// The piece being painted, when it is one that keeps its last drawing and draws nothing of its own over all of what is
/// painted now (`in_piece`): that may draw in a layer over the window's pieces instead, where nothing the piece draws
/// over part of it is (`over`).
pub(crate) fn uncovered_piece() -> Option<Piece> {
    let piece = current_piece();
    (piece != WINDOW && !COVERED.get()).then_some(piece)
}

/// Where the piece being painted draws something of its own over part of `bounds`, later in this draw (`under`).
pub(crate) fn over(bounds: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
    OVER.with_borrow(|over| over.iter().map(|at| at.get()).filter(|at| at.intersects(&bounds)).collect())
}

/// `element`, over part of which the piece it is in draws something of its own later in the same draw (a page's pane
/// floating over its content), at the bounds `over` holds once the draw has laid it out: what is painted in `element`
/// where that is draws in place, under it (`over`).
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
    leave(window, piece, layer, name, bounds, None, None);
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

/// Lets go of the slots the piece `piece` left in the window `id`, for a piece that isn't drawn now (the closed
/// sidebar): a named piece's slots otherwise count until it draws again, and a layer would go on drawing in them.
pub fn forget_piece(id: WindowId, piece: impl Into<Piece>) {
    let piece = piece.into();
    SLOTS.with_borrow_mut(|slots| slots.slots.retain(|(window, of, _, _), _| *window != id || *of != piece));
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
        move |bounds, window, _| leave(window, piece, layer, name, bounds, over, None),
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
    round: Option<f32>,
) {
    let id = window.window_handle().window_id();
    let mask = CLIP.with_borrow(Clone::clone).unwrap_or_else(|| window.content_mask());
    SLOTS.with_borrow_mut(|slots| {
        let drawn = slots.frame.get(&id).copied().unwrap_or_default();
        let slot = Slot {
            bounds,
            mask,
            piece,
            over,
            round,
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
    cover_in(COVERS, element)
}

/// The layer the places of overlays that take no clicks are kept in (`passive_cover`).
const PASSIVE_COVERS: &str = "passive-covers";

/// `cover`, for an overlay that takes no clicks (a tooltip): what turns over the window gives way to it as to any, but a
/// web page a page shows (`crate::web`) stays where it is under it, since hiding the page for a tooltip made it blink.
pub fn passive_cover(element: impl IntoElement) -> Cover {
    cover_in(PASSIVE_COVERS, element)
}

/// The layer the places of dialogs' backdrops are kept in (`backdrop_mark`).
const BACKDROPS: &str = "backdrops";

/// Notes that what it fills is a dialog's dimmed backdrop, which takes the clicks over all of it: a web page under it
/// stays in view, dimmed as the rest of the window is, rather than hiding. Put it in the backdrop.
pub fn backdrop_mark() -> Cover {
    cover_in(BACKDROPS, div().absolute().top_0().left_0().size_full())
}

/// Where the backdrops of the window `id` are in its last draw (`backdrop_mark`).
pub(crate) fn backdrops(id: WindowId) -> Vec<Bounds<Pixels>> {
    shown_bounds(id, BACKDROPS)
}

/// `covers`, each with how round its corners are when it says.
pub(crate) fn overlays(id: WindowId) -> Vec<(Bounds<Pixels>, Option<f32>)> {
    [COVERS, PASSIVE_COVERS]
        .into_iter()
        .flat_map(|layer| shown(id, layer))
        .map(|(_, slot)| (slot.bounds.intersect(&slot.mask.bounds), slot.round))
        .filter(|(bounds, _)| !bounds.is_empty())
        .collect()
}

fn cover_in(layer: &'static str, element: impl IntoElement) -> Cover {
    let name = NEXT_COVER.with(|next| {
        let name = next.get();
        next.set(name + 1);
        name
    });
    Cover {
        layer,
        piece: current_piece(),
        name: name.to_string().into(),
        element: element.into_any_element(),
        round: None,
    }
}

pub struct Cover {
    layer: &'static str,
    piece: Piece,
    name: SharedString,
    element: AnyElement,
    round: Option<f32>,
}

impl Cover {
    /// Its corners are `radius` round, as what it covers draws them: a web page under it shows around them.
    pub fn rounded(mut self, radius: f32) -> Self {
        self.round = Some(radius);
        self
    }
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
        leave(window, self.piece, self.layer, self.name.clone(), bounds, None, self.round);
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
pub(crate) fn covers(id: WindowId) -> Vec<Bounds<Pixels>> {
    let mut covers = blocking_covers(id);
    covers.extend(shown_bounds(id, PASSIVE_COVERS));
    covers
}

/// `covers` but those that take no clicks (`passive_cover`): where a native view over the window has to give way, since
/// a click there is the overlay's.
pub(crate) fn blocking_covers(id: WindowId) -> Vec<Bounds<Pixels>> {
    shown_bounds(id, COVERS)
}

fn shown_bounds(id: WindowId, layer: &'static str) -> Vec<Bounds<Pixels>> {
    shown(id, layer)
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

#[cfg(all(test, feature = "test-support"))]
mod dialog_tests {
    use gpui::{Context, FocusHandle, Render, TestApp};

    use super::*;
    use crate::ui::motion::{self, Phase};

    /// A dialog over the window, coming in or on its way out.
    struct Dialog(Phase, FocusHandle);

    impl Render for Dialog {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            start_draw(window.window_handle().window_id());
            let (backdrop, panel) = crate::ui::dialog("test", "Test", &self.1, 300.0, 200.0);
            div().size_full().child(cover(motion::dialog_layer("test", self.0, backdrop.child(panel))))
        }
    }

    /// A dialog's backdrop and panel say where they are while it fades out too: a web page under it went black then.
    #[test]
    fn a_dialog_on_its_way_out_still_says_where_its_backdrop_and_panel_are() {
        for phase in [Phase::In, Phase::Out(1)] {
            let mut app = TestApp::new();
            let mut window = app.open_window(move |_, cx| Dialog(phase, cx.focus_handle()));
            let id = window.update(|_, window, _| window.window_handle().window_id());
            window.draw();
            assert_eq!(backdrops(id).len(), 1, "{phase:?}");
            assert!(overlays(id).iter().any(|(_, round)| *round == Some(14.0)), "the panel, {phase:?}");
        }
    }
}
