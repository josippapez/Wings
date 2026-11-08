//! Moving a tab along the title bar, as the desktop's `dragTab` (`title-bar.tsx`): pressed and moved 5px, the tab
//! follows the pointer and the others make room as it passes their middles. The order is saved once, when it is let
//! go; Escape puts it back. A tab moves among its own kind, as Move Tab Left and Right do: terminals among terminals,
//! plugin pages among plugin pages. The project page and Issues stay where they are.

use gpui::{
    AnyElement, Axis, Context, DispatchPhase, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Subscription, canvas, prelude::*,
};

use super::{
    Active, Workspace,
    drag_slot::{DRAG_THRESHOLD, Laid, Slot},
};

/// The title bar's drag: where each tab was laid out, the press on one and the drag it becomes.
#[derive(Default)]
pub(super) struct TabDrags {
    laid: Laid<Active>,
    drag: Option<Drag>,
    /// The click a drag's release makes, which doesn't select the tab.
    swallow_click: bool,
}

struct Drag {
    tab: Active,
    /// Where the pointer went down, and how far right of the tab's left edge.
    from: Point<Pixels>,
    grab: Pixels,
    /// Where the pointer is.
    x: Pixels,
    /// Past the threshold: a drag, not a click.
    moving: bool,
    /// The tabs of its kind, as they were when it was pressed and as they are now.
    before: Vec<Active>,
    order: Vec<Active>,
    /// Takes Escape while it moves, so the key ends the drag and reaches no terminal.
    _escape: Option<Subscription>,
}

impl Workspace {
    /// The tabs `tab` moves among, in strip order, when it moves at all.
    fn movable_with(&self, tab: Active) -> Option<Vec<Active>> {
        match tab {
            Active::Terminal(_) => Some(
                self.terminals
                    .iter()
                    .map(|tab| Active::Terminal(tab.id))
                    .collect(),
            ),
            Active::Plugin(_) => Some(
                self.plugin_tabs
                    .iter()
                    .map(|tab| Active::Plugin(tab.id))
                    .collect(),
            ),
            Active::Project | Active::Issues => None,
        }
    }

    /// The left button went down on `tab` at `at`.
    pub(super) fn press_tab(&mut self, tab: Active, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.tab_drags.swallow_click = false;
        let Some(order) = self.movable_with(tab) else {
            return;
        };
        let Some(laid) = self.tab_drags.laid.borrow().get(&tab).copied() else {
            return;
        };
        self.tab_drags.drag = Some(Drag {
            tab,
            from: at,
            grab: at.x - laid.left(),
            x: at.x,
            moving: false,
            before: order.clone(),
            order,
            _escape: None,
        });
        // Drawn once more, with the listeners that follow the pointer anywhere in the window.
        self.redraw_title_bar(cx);
    }

    /// The pointer moved with the button down: past the threshold the tab follows it, and passing the middle of another
    /// tab of its kind takes that tab's place.
    fn drag_tab_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.tab_drags.drag else {
            return;
        };
        if !drag.moving {
            if (at - drag.from).magnitude() < DRAG_THRESHOLD {
                return;
            }
            drag.moving = true;
            let this = cx.weak_entity();
            drag._escape = Some(cx.intercept_keystrokes(move |event, _, cx| {
                if event.keystroke.key == "escape" {
                    let _ = this.update(cx, |this, cx| this.cancel_tab_drag(cx));
                    cx.stop_propagation();
                }
            }));
        }
        drag.x = at.x;
        let laid = self.tab_drags.laid.borrow();
        if let Some(own) = laid.get(&drag.tab) {
            let middle = drag.x - drag.grab + own.size.width / 2.0;
            let left_of = drag
                .order
                .iter()
                .filter(|tab| **tab != drag.tab)
                .filter(|tab| {
                    laid.get(*tab)
                        .is_some_and(|other| other.center().x < middle)
                })
                .count();
            if let Some(from) = drag.order.iter().position(|tab| *tab == drag.tab)
                && from != left_of
            {
                let tab = drag.order.remove(from);
                drag.order.insert(left_of, tab);
            }
        }
        drop(laid);
        self.redraw_title_bar(cx);
    }

    /// The button came up: a drag puts the tab where it was let go and saves the order, once.
    fn drop_tab(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.tab_drags.drag.take() else {
            return;
        };
        if !drag.moving {
            return self.redraw_title_bar(cx);
        }
        // The release makes a click on the tab it ends on, which isn't one.
        self.tab_drags.swallow_click = true;
        let to = drag
            .order
            .iter()
            .position(|tab| *tab == drag.tab)
            .filter(|at| drag.before.get(*at) != Some(&drag.tab));
        match to {
            Some(to) => self.move_tab_to(drag.tab, to, cx),
            None => self.redraw_title_bar(cx),
        }
    }

    /// Escape: the tab goes back where it was and nothing is saved. The release still to come is no click.
    fn cancel_tab_drag(&mut self, cx: &mut Context<Self>) {
        if self.tab_drags.drag.take().is_some() {
            self.tab_drags.swallow_click = true;
            self.redraw_title_bar(cx);
        }
    }

    /// The click a drag's release makes, which doesn't select the tab. Enter on a focused tab always does.
    pub(super) fn tab_drag_click(&mut self, event: &gpui::ClickEvent) -> bool {
        !event.is_keyboard()
            && (std::mem::take(&mut self.tab_drags.swallow_click)
                || self.tab_drags.drag.as_ref().is_some_and(|drag| drag.moving))
    }

    /// Where `tab` is drawn while it is dragged, its left edge in the window.
    pub(super) fn dragged_left(&self, tab: Active) -> Option<Pixels> {
        self.tab_drags
            .drag
            .as_ref()
            .filter(|drag| drag.moving && drag.tab == tab)
            .map(|drag| drag.x - drag.grab)
    }

    /// The strip's tabs, the dragged one's kind in the order the drag has reached, each noting where it was laid out and
    /// the dragged one drawn under the pointer.
    pub(super) fn tab_strip(&self, tabs: Vec<(Active, AnyElement)>) -> Vec<AnyElement> {
        let mut tabs: Vec<Option<(Active, AnyElement)>> = tabs.into_iter().map(Some).collect();
        if let Some(drag) = self.tab_drags.drag.as_ref().filter(|drag| drag.moving) {
            let moving: Vec<(Active, AnyElement)> = drag
                .order
                .iter()
                .filter_map(|want| {
                    tabs.iter_mut()
                        .find(|tab| tab.as_ref().is_some_and(|(tab, _)| tab == want))
                        .and_then(Option::take)
                })
                .collect();
            // The places its kind had, emptied above, filled again in its order.
            let mut moving = moving.into_iter();
            for place in tabs.iter_mut().filter(|tab| tab.is_none()) {
                *place = moving.next();
            }
        }
        tabs.into_iter()
            .flatten()
            .map(|(tab, element)| {
                let laid = self.tab_drags.laid.clone();
                let at = self.dragged_left(tab);
                Slot::new(element, tab, laid, Axis::Horizontal, at).into_any_element()
            })
            .collect()
    }

    /// While a tab is pressed: the window's listeners that follow the pointer wherever it goes and end the drag where the
    /// button comes up, as the sidebar's project drag does.
    pub(super) fn tab_drag_listeners(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.tab_drags.drag.as_ref()?;
        let entity = cx.entity();
        Some(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let moved = entity.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            moved.update(cx, |this, cx| {
                                // Let go outside the window, it ends where it is.
                                if event.dragging() {
                                    this.drag_tab_to(event.position, cx);
                                } else {
                                    this.drop_tab(cx);
                                }
                            });
                        }
                    });
                    let released = entity.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                            released.update(cx, |this, cx| this.drop_tab(cx));
                        }
                    });
                },
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }

    /// Draws the title bar again and leaves the sidebar and panes as they were drawn: a drag changes nothing else.
    fn redraw_title_bar(&self, cx: &mut Context<Self>) {
        match &self.parts {
            Some(parts) => parts.title_bar.update(cx, |_, cx| cx.notify()),
            None => cx.notify(),
        }
    }
}
