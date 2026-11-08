//! Moving a project up or down the sidebar, as the desktop's `dragSpace` and its menu's Move up and Move down
//! (`sidebar.tsx`). Only in the Manual sort and only a listed project, not a worktree or one under "Older": pressed
//! and moved 5px, its row follows the pointer and the others make room as it passes them. The list is saved once,
//! when it is let go; Escape puts it back. Pinned projects always lead, so a project moves among those of its kind.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Subscription, Window, canvas, point, prelude::*, px,
};

use crate::workspace::Workspace;

use super::order::Sort;

/// How far the pointer moves before a press is a drag, so a click still selects: `trackDrag`'s threshold.
const THRESHOLD: f64 = 5.0;

/// Where each listed project's row was laid out when last drawn, by folder, which a drag measures the others by.
pub(super) type Rows = Rc<RefCell<HashMap<PathBuf, Bounds<Pixels>>>>;

/// A press on a project's row, and the drag it becomes.
pub(super) struct Drag {
    project: PathBuf,
    /// Where the pointer went down, and how far below the row's top.
    from: Point<Pixels>,
    grab: Pixels,
    /// Where the pointer is.
    y: Pixels,
    /// Past the threshold: a drag, not a click.
    moving: bool,
    /// The listed projects of its kind, as they were when it was pressed and as they are now.
    before: Vec<PathBuf>,
    order: Vec<PathBuf>,
    /// Takes Escape while it moves, so the key ends the drag and reaches no terminal.
    _escape: Option<Subscription>,
}

impl Workspace {
    /// The listed projects that move like `project`, pinned or not, in the order shown, when they can move at all.
    fn movable_like(&self, project: &Path) -> Option<Vec<PathBuf>> {
        if self.side.sort != Sort::Manual {
            return None;
        }
        let pinned = self
            .projects
            .iter()
            .find(|entry| entry.path == project && !entry.child)?
            .pinned;
        let tops: Vec<PathBuf> = self
            .project_order()
            .shown
            .into_iter()
            .map(|row| &self.projects[row])
            .filter(|entry| !entry.child && entry.pinned == pinned)
            .map(|entry| entry.path.clone())
            .collect();
        tops.contains(&project.to_path_buf()).then_some(tops)
    }

    /// The projects Move up and Move down put `project` in the place of, or `None` when it doesn't move in this sort or
    /// where it is listed. Either is `None` at that end.
    pub(in crate::workspace) fn project_steps(
        &self,
        project: &Path,
    ) -> Option<(Option<PathBuf>, Option<PathBuf>)> {
        let tops = self.movable_like(project)?;
        let at = tops.iter().position(|top| top == project)?;
        Some((
            at.checked_sub(1).map(|up| tops[up].clone()),
            tops.get(at + 1).cloned(),
        ))
    }

    /// Puts `project` in the place of `slot` and saves the list, as the desktop's `onMoveSpace`.
    pub(in crate::workspace) fn move_project(
        &mut self,
        project: &Path,
        slot: &Path,
        cx: &mut Context<Self>,
    ) {
        if let (Some(project), Some(slot)) = (project.to_str(), slot.to_str()) {
            self.spaces.move_project(project, slot);
        }
        self.projects = self.spaces.entries();
        // The rows are drawn by their place in the list, which changed.
        self.side.hovered = None;
        cx.notify();
    }

    /// The left button went down on the row of `project` at `at`.
    pub(super) fn press_project(
        &mut self,
        project: &Path,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.side.swallow_click = false;
        let Some(order) = self.movable_like(project) else {
            return;
        };
        let Some(row) = self.side.rows.borrow().get(project).copied() else {
            return;
        };
        self.side.drag = Some(Drag {
            project: project.to_path_buf(),
            from: at,
            grab: at.y - row.top(),
            y: at.y,
            moving: false,
            before: order.clone(),
            order,
            _escape: None,
        });
        // Drawn once more, with the listeners that follow the pointer anywhere in the window.
        self.redraw_sidebar(cx);
    }

    /// The pointer moved with the button down: past the threshold the row follows it, and passing the middle of another
    /// row of its kind takes that row's place.
    fn drag_project_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.side.drag else {
            return;
        };
        if !drag.moving {
            if (at - drag.from).magnitude() < THRESHOLD {
                return;
            }
            drag.moving = true;
            let this = cx.weak_entity();
            drag._escape = Some(cx.intercept_keystrokes(move |event, _, cx| {
                if event.keystroke.key == "escape" {
                    let _ = this.update(cx, |this, cx| this.cancel_drag(cx));
                    cx.stop_propagation();
                }
            }));
        }
        drag.y = at.y;
        let rows = self.side.rows.borrow();
        if let Some(own) = rows.get(&drag.project) {
            let middle = drag.y - drag.grab + own.size.height / 2.0;
            let above = drag
                .order
                .iter()
                .filter(|top| **top != drag.project)
                .filter(|top| rows.get(*top).is_some_and(|row| row.center().y < middle))
                .count();
            if let Some(from) = drag.order.iter().position(|top| *top == drag.project)
                && from != above
            {
                let project = drag.order.remove(from);
                drag.order.insert(above, project);
            }
        }
        drop(rows);
        self.redraw_sidebar(cx);
    }

    /// The button came up: a drag puts the project where it was let go and saves the list, once.
    fn drop_project(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.side.drag.take() else {
            return;
        };
        if !drag.moving {
            return self.redraw_sidebar(cx);
        }
        // The release makes a click on the row it ends on, which isn't one.
        self.side.swallow_click = true;
        let slot = drag
            .order
            .iter()
            .position(|top| *top == drag.project)
            .and_then(|at| drag.before.get(at))
            .filter(|slot| **slot != drag.project);
        match slot {
            Some(slot) => {
                self.move_project(&drag.project, slot, cx);
                // Still under the pointer.
                self.side.hovered = self
                    .projects
                    .iter()
                    .position(|entry| entry.path == drag.project);
            }
            None => self.redraw_sidebar(cx),
        }
    }

    /// Escape: the project goes back where it was and nothing is saved. The release still to come is no click.
    fn cancel_drag(&mut self, cx: &mut Context<Self>) {
        if self.side.drag.take().is_some() {
            self.side.swallow_click = true;
            self.redraw_sidebar(cx);
        }
    }

    /// The click a drag's release makes, which doesn't select the project.
    pub(super) fn drag_click(&mut self) -> bool {
        std::mem::take(&mut self.side.swallow_click)
            || self.side.drag.as_ref().is_some_and(|drag| drag.moving)
    }

    /// `shown` with the dragged project's rows, and its worktrees, in the place the drag has reached.
    pub(super) fn dragged_order(&self, shown: Vec<usize>) -> Vec<usize> {
        let Some(drag) = self.side.drag.as_ref().filter(|drag| drag.moving) else {
            return shown;
        };
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for row in shown {
            match groups.last_mut() {
                Some(group) if self.projects[row].child => group.push(row),
                _ => groups.push(vec![row]),
            }
        }
        let top = |group: &[usize]| group.first().map(|&row| &self.projects[row].path);
        let mut moving: Vec<Vec<usize>> = Vec::new();
        for path in &drag.order {
            if let Some(at) = groups.iter().position(|group| top(group) == Some(path)) {
                moving.push(std::mem::take(&mut groups[at]));
            }
        }
        // The places the drag's projects had, emptied above, filled again in its order.
        let mut moving = moving.into_iter();
        groups
            .into_iter()
            .flat_map(|group| {
                if group.is_empty() {
                    moving.next().unwrap_or_default()
                } else {
                    group
                }
            })
            .collect()
    }

    /// Where the dragged project's row is drawn, its top in the window, while it moves.
    pub(super) fn dragged_top(&self, project: &Path) -> Option<Pixels> {
        self.side
            .drag
            .as_ref()
            .filter(|drag| drag.moving && drag.project == project)
            .map(|drag| drag.y - drag.grab)
    }

    /// While a row is pressed: the window's listeners that follow the pointer wherever it goes and end the drag where
    /// the button comes up, as the edge's resize does.
    pub(super) fn drag_listeners(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.side.drag.as_ref()?;
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
                                    this.drag_project_to(event.position, cx);
                                } else {
                                    this.drop_project(cx);
                                }
                            });
                        }
                    });
                    let released = entity.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                            released.update(cx, |this, cx| this.drop_project(cx));
                        }
                    });
                },
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }

    /// Draws the sidebar again and leaves the panes and title bar as they were drawn: a drag changes nothing else.
    fn redraw_sidebar(&self, cx: &mut Context<Self>) {
        match &self.parts {
            Some(parts) if self.sidebar => parts.sidebar.update(cx, |_, cx| cx.notify()),
            _ => cx.notify(),
        }
    }
}

/// A listed project's row in the Manual sort: notes where it was laid out, and draws the dragged one at `top` instead,
/// under the pointer, over the other rows as the desktop's `z-index` puts it, with its hitbox, so it stays the row
/// pointed at and the rows it passes over aren't.
pub(super) struct Slot {
    row: Option<AnyElement>,
    project: PathBuf,
    rows: Rows,
    top: Option<Pixels>,
}

impl Slot {
    pub(super) fn new(row: AnyElement, project: PathBuf, rows: Rows, top: Option<Pixels>) -> Self {
        Self {
            row: Some(row),
            project,
            rows,
            top,
        }
    }
}

impl IntoElement for Slot {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Slot {
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
        let row = self.row.as_mut().expect("laid out once");
        (row.request_layout(window, cx), ())
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
        self.rows.borrow_mut().insert(self.project.clone(), bounds);
        match (self.top, self.row.take()) {
            // Drawn after everything else, still clipped by the list.
            (Some(top), Some(row)) => {
                let at = window.element_offset() + point(px(0.0), top - bounds.top());
                window.defer_draw(row, at, 1, Some(window.content_mask()));
            }
            (None, Some(mut row)) => {
                row.prepaint(window, cx);
                self.row = Some(row);
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
        if let Some(row) = &mut self.row {
            row.paint(window, cx);
        }
    }
}
