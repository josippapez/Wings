//! Pieces of the window that keep their last drawing until the workspace changes. GPUI draws again every view on the
//! way down to one that changed and reuses a cached view that didn't, so with the sidebar in a piece of its own, a
//! terminal's output or a page's update no longer rebuilds the sidebar each frame.

use gpui::{
    AnyElement, App, AppContext, Context, Entity, IntoElement, Render, Styled, Subscription,
    WeakEntity, Window, div,
};

use super::Workspace;
use crate::ui::live::{Kind, Layer};

/// How the workspace draws a piece.
type Draw = fn(&mut Workspace, &mut Window, &mut Context<Workspace>) -> AnyElement;

pub(super) struct Part {
    workspace: WeakEntity<Workspace>,
    /// Its name, for the spinners in it (`ui::drawing_piece`).
    name: &'static str,
    draw: Draw,
    _changes: Subscription,
    /// How many times it drew itself, for tests of what draws again.
    #[cfg(test)]
    pub(super) drawn: usize,
}

impl Part {
    pub(super) fn new(
        workspace: &Entity<Workspace>,
        name: &'static str,
        draw: Draw,
        cx: &mut App,
    ) -> Entity<Part> {
        cx.new(|cx| Part {
            workspace: workspace.downgrade(),
            name,
            draw,
            // Anything that changes the workspace draws it again; a terminal or a page changing on its own doesn't.
            _changes: cx.observe(workspace, |_, _, cx| cx.notify()),
            #[cfg(test)]
            drawn: 0,
        })
    }
}

impl Render for Part {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.drawn += 1;
        }
        crate::ui::readout::render(self.name);
        let draw = self.draw;
        let id = window.window_handle().window_id();
        let drawn = crate::ui::drawing_piece(self.name, id, || {
            self.workspace
                .update(cx, |workspace, cx| draw(workspace, window, cx))
                .unwrap_or_else(|_| div().into_any_element())
        });
        crate::ui::in_piece(Some(self.name.into()), false, drawn)
    }
}

/// The window's cached pieces. They are made once the workspace exists and before a draw: GPUI only learns that a view
/// it draws can change when the view is older than the frame it is first drawn in, so pieces made while drawing would
/// never draw again.
pub(super) struct Parts {
    pub(super) sidebar: Entity<Part>,
    pub(super) title_bar: Entity<Part>,
    pub(super) main: Entity<Part>,
    /// The backdrop a tab's page asks for, drawn under the main column in the place the tab's card leaves for it, so a
    /// step of it draws neither the column nor the page again (`ui::backdrop_slot`).
    pub(super) backdrop: Entity<crate::ui::BackdropLayer>,
    /// The pieces' spinners, and what moves in the pages a piece puts on screen (spinners, pulsing lines, counting lines),
    /// each kind a view of its own, so a step of them draws none of the pieces again (`ui::live`).
    pub(super) spinners: Entity<Layer>,
    pub(super) view_layers: [Entity<Layer>; 3],
    /// The performance readout (`ui::readout`), a view of its own so its update draws none of the pieces.
    pub(super) readout: Entity<crate::ui::readout::Readout>,
    /// The text of the field being typed in, so a key draws none of the pieces (`text_field::FieldLayer`).
    pub(super) fields: Entity<crate::text_field::FieldLayer>,
}

impl Parts {
    /// Draws the window again and none of its pieces, for a change only the workspace's own drawing shows, as the layers
    /// it draws over the pieces (the palette): telling the workspace it changed draws every piece again. The field layer
    /// is told instead, a view the workspace draws itself, which GPUI draws again with the workspace around it.
    pub(super) fn redraw_window(&self, cx: &mut App) {
        self.fields.update(cx, |_, cx| cx.notify());
    }

    pub(super) fn new(workspace: &Entity<Workspace>, cx: &mut App) -> Parts {
        Parts {
            sidebar: Part::new(
                workspace,
                "sidebar",
                |workspace, _, cx| workspace.sidebar_column(cx).w_full().into_any_element(),
                cx,
            ),
            title_bar: Part::new(workspace, "title-bar", Workspace::title_bar_part, cx),
            main: Part::new(workspace, "main", Workspace::main_part, cx),
            backdrop: cx.new(|_| crate::ui::BackdropLayer::default()),
            spinners: cx.new(|_| Layer::new(Kind::Spinners)),
            view_layers: [Kind::ViewSpinners, Kind::Pulses, Kind::Counts].map(|kind| cx.new(|_| Layer::new(kind))),
            readout: cx.new(|_| crate::ui::readout::Readout::new()),
            fields: cx.new(|_| crate::text_field::FieldLayer),
        }
    }
}
