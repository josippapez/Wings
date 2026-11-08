//! The right-hand panel's width: the edge you drag to resize it, within the desktop's `SIDES.right`, and the width the
//! workspace file keeps, as the project sidebar's (`sidebar/mod.rs`).

use gpui::{AnyElement, Context, Pixels, prelude::*};

use crate::{saved, ui};

use super::Workspace;

/// The desktop's `SIDES.right`: how narrow and how wide the panel can be dragged, and where it starts.
pub(super) const MIN_WIDTH: f32 = 250.0;
pub(super) const MAX_WIDTH: f32 = 640.0;
pub(super) const INITIAL_WIDTH: f32 = 360.0;
/// The desktop's `minSize={320}` of the panel between the sidebars: the panes keep this much while the window has it.
const MAIN_MIN: f32 = 320.0;

pub(super) struct Size {
    pub(super) width: f32,
    /// While the edge is dragged: where the pointer was, and how wide the panel was drawn, when the drag started.
    resize: Option<(Pixels, f32)>,
}

impl Default for Size {
    fn default() -> Self {
        Self {
            width: INITIAL_WIDTH,
            resize: None,
        }
    }
}

impl Size {
    /// What the workspace file kept. A width outside the limits starts again from the initial one, as the desktop's
    /// `fit` does.
    pub(super) fn load(settings: &saved::Settings) -> Self {
        let width = settings
            .right_width
            .filter(|width| (MIN_WIDTH..=MAX_WIDTH).contains(width))
            .unwrap_or(INITIAL_WIDTH);
        Self {
            width,
            resize: None,
        }
    }
}

impl Workspace {
    /// The widest the panel can be in a window `viewport` wide: the panes keep their 320 beside the docked project
    /// sidebar, but the panel never goes under its 250.
    fn right_room(&self, viewport: Pixels) -> f32 {
        let left = if self.sidebar { self.side.width } else { 0.0 };
        (viewport.as_f32() - left - MAIN_MIN).clamp(MIN_WIDTH, MAX_WIDTH)
    }

    /// How wide the panel is drawn: as dragged, within the room the window leaves it.
    pub(super) fn right_width(&self, viewport: Pixels) -> f32 {
        self.plugin_ui
            .right_size
            .width
            .min(self.right_room(viewport))
    }

    /// The edge was grabbed with the pointer at `x`.
    fn start_right_resize(&mut self, x: Pixels, viewport: Pixels, cx: &mut Context<Self>) {
        let from = self.right_width(viewport);
        self.plugin_ui.right_size.resize = Some((x, from));
        cx.notify();
    }

    /// Follows the pointer within the limits. Nothing is saved until the pointer is released: the terminals refit when
    /// the panes settle (`RESIZE_DELAY`), and the file is written once.
    fn right_resize_to(&mut self, x: Pixels, viewport: Pixels, cx: &mut Context<Self>) {
        let Some((from_x, from_width)) = self.plugin_ui.right_size.resize else {
            return;
        };
        let room = self.right_room(viewport);
        let width = (from_width + (from_x - x).as_f32()).clamp(MIN_WIDTH, room);
        if width != self.plugin_ui.right_size.width {
            self.plugin_ui.right_size.width = width;
            cx.notify();
        }
    }

    fn end_right_resize(&mut self, cx: &mut Context<Self>) {
        if self.plugin_ui.right_size.resize.take().is_some() {
            self.save(cx);
            cx.notify();
        }
    }

    /// The panel's left edge, which you drag. Invisible until you point at it. The window listens for the pointer while
    /// it is down, so the drag goes on wherever the pointer is.
    pub(super) fn right_resize_edge(&self, cx: &mut Context<Self>) -> AnyElement {
        ui::resize_edge(
            "right-sidebar-edge",
            "Resize right sidebar",
            ui::Side::Left,
            self.plugin_ui.right_size.resize.is_some(),
            cx,
            |this, x, window, cx| this.start_right_resize(x, window.viewport_size().width, cx),
            |this, x, window, cx| this.right_resize_to(x, window.viewport_size().width, cx),
            Self::end_right_resize,
        )
        .left_0()
        .into_any_element()
    }
}
