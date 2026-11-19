//! `popup`: draws a popup's `content` part over the rest of the window beside its anchor, placed by `position::place`
//! with the size it was measured at in the same frame (UI SDK's `createPopup`, without its look: the caller passes
//! its own content and styling). Close it with `layer::bind_layer` and give the keyboard back with `FocusRecovery`.

use gpui::{
    AnyElement, App, Bounds, Display, Edges, Element, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Point, Position, Size, Style, Window, deferred, point, px,
};

use super::position::{Align, Placement, Side, place};

enum Anchor {
    /// A box in the window, like a trigger's bounds kept from its last frame, or a point as an empty box.
    Bounds(Bounds<Pixels>),
    /// The box of the element the popup is a child of, in the frame it's drawn in.
    Parent,
}

/// `content` beside `anchor` (window coordinates), under it unless set otherwise.
pub fn popup(anchor: Bounds<Pixels>, content: impl IntoElement) -> Popup {
    Popup::new(Anchor::Bounds(anchor), content)
}

/// `popup` at a point, as a context menu at the pointer: its top-left corner there, or its bottom-left when it doesn't
/// fit under it.
pub fn popup_at(at: Point<Pixels>, content: impl IntoElement) -> Popup {
    let anchor = Bounds {
        origin: at,
        size: Size::default(),
    };
    Popup::new(Anchor::Bounds(anchor), content).side_offset(0.0)
}

/// `popup` beside the element it's a child of, measured in the same frame: put it in a `relative` box around the
/// trigger, for a trigger whose bounds the view doesn't keep.
pub fn popup_on_parent(content: impl IntoElement) -> Popup {
    Popup::new(Anchor::Parent, content)
}

/// A popup being built: set its side, edge and offsets, then draw it with `layer`.
pub struct Popup {
    anchor: Anchor,
    content: AnyElement,
    placement: Placement,
    priority: usize,
}

impl Popup {
    fn new(anchor: Anchor, content: impl IntoElement) -> Self {
        Self {
            anchor,
            content: content.into_any_element(),
            placement: Placement::default(),
            priority: 1,
        }
    }

    pub fn side(mut self, side: Side) -> Self {
        self.placement.side = side;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.placement.align = align;
        self
    }

    /// The room between the popup and its anchor, 4pt unless set.
    pub fn side_offset(mut self, offset: f32) -> Self {
        self.placement.side_offset = px(offset);
        self
    }

    /// Which deferred layer it's drawn in, 1 unless set: a higher one is drawn over a lower.
    pub fn priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }

    /// The popup, drawn after the rest of the window, over it.
    pub fn layer(self) -> AnyElement {
        let priority = self.priority;
        deferred(self).with_priority(priority).into_any_element()
    }
}

/// The box that holds a popup's content, sized to it.
pub struct PopupLayout {
    content_box: LayoutId,
}

impl IntoElement for Popup {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Popup {
    type RequestLayoutState = PopupLayout;
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
    ) -> (LayoutId, PopupLayout) {
        let content = self.content.request_layout(window, cx);
        // Out of the flow and as big as its content, as `anchored()`'s box is, so a flex parent doesn't stretch it.
        let content_box = window.request_layout(
            Style {
                position: Position::Absolute,
                display: Display::Flex,
                ..Style::default()
            },
            [content],
            cx,
        );
        let anchor_box = match self.anchor {
            // Over the parent's box, which is the anchor.
            Anchor::Parent => Style {
                position: Position::Absolute,
                inset: Edges {
                    top: px(0.0).into(),
                    right: px(0.0).into(),
                    bottom: px(0.0).into(),
                    left: px(0.0).into(),
                },
                ..Style::default()
            },
            Anchor::Bounds(_) => Style {
                position: Position::Absolute,
                ..Style::default()
            },
        };
        (
            window.request_layout(anchor_box, [content_box], cx),
            PopupLayout { content_box },
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut PopupLayout,
        window: &mut Window,
        cx: &mut App,
    ) {
        let content = window.layout_bounds(layout.content_box);
        let anchor = match self.anchor {
            Anchor::Bounds(anchor) => anchor,
            Anchor::Parent => bounds,
        };
        let mut placement = self.placement;
        placement.collision_padding += window.client_inset().unwrap_or_default();
        let (origin, _) = place(anchor, content.size, window.viewport_size(), placement);
        let offset = origin - content.origin;
        let offset = point(offset.x.round(), offset.y.round());
        window.with_element_offset(offset, |window| {
            self.content.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut PopupLayout,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.content.paint(window, cx);
    }
}
