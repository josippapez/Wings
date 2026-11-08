//! A row or tab that can be dragged into another place in its list, as the sidebar's projects and the title bar's tabs:
//! notes where it was laid out, which a drag measures the others by, and draws the dragged one under the pointer
//! instead, over the others as the desktop's `z-index` puts it, with its hitbox, so it stays the one pointed at and the
//! ones it passes over aren't.

use std::{cell::RefCell, collections::HashMap, hash::Hash, rc::Rc};

use gpui::{
    AnyElement, App, Axis, Bounds, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window, point, px,
};

/// How far the pointer moves before a press is a drag, so a click still selects: the desktop's `trackDrag` threshold.
pub(super) const DRAG_THRESHOLD: f64 = 5.0;

/// Where each item was laid out when last drawn, in the window, by key.
pub(super) type Laid<K> = Rc<RefCell<HashMap<K, Bounds<Pixels>>>>;

pub(super) struct Slot<K> {
    item: Option<AnyElement>,
    key: K,
    laid: Laid<K>,
    axis: Axis,
    /// Where the dragged one is drawn instead along `axis`, its top or left edge in the window.
    at: Option<Pixels>,
}

impl<K> Slot<K> {
    pub(super) fn new(
        item: AnyElement,
        key: K,
        laid: Laid<K>,
        axis: Axis,
        at: Option<Pixels>,
    ) -> Self {
        Self {
            item: Some(item),
            key,
            laid,
            axis,
            at,
        }
    }
}

impl<K: Hash + Eq + Clone + 'static> IntoElement for Slot<K> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<K: Hash + Eq + Clone + 'static> Element for Slot<K> {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let item = self.item.as_mut().expect("laid out once");
        (item.request_layout(window, cx), ())
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
        self.laid.borrow_mut().insert(self.key.clone(), bounds);
        match (self.at, self.item.take()) {
            // Drawn after everything else, still clipped by the list.
            (Some(at), Some(item)) => {
                let moved = match self.axis {
                    Axis::Vertical => point(px(0.0), at - bounds.top()),
                    Axis::Horizontal => point(at - bounds.left(), px(0.0)),
                };
                window.defer_draw(
                    item,
                    window.element_offset() + moved,
                    1,
                    Some(window.content_mask()),
                );
            }
            (None, Some(mut item)) => {
                item.prepaint(window, cx);
                self.item = Some(item);
            }
            (_, None) => {}
        }
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
        if let Some(item) = &mut self.item {
            item.paint(window, cx);
        }
    }
}
