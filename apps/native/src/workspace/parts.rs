//! Pieces of the window that keep their last drawing until the workspace changes. GPUI draws again every view on the
//! way down to one that changed and reuses a cached view that didn't, so with the sidebar in a piece of its own, a
//! terminal's output or a page's update no longer rebuilds the sidebar each frame.

use gpui::{
    AnyElement, App, AppContext, Context, Entity, IntoElement, Render, Styled, Subscription,
    WeakEntity, Window, div,
};

use super::Workspace;

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
        let draw = self.draw;
        let id = window.window_handle().window_id();
        crate::ui::drawing_piece(self.name, id, || {
            self.workspace
                .update(cx, |workspace, cx| draw(workspace, window, cx))
                .unwrap_or_else(|_| div().into_any_element())
        })
    }
}

/// The window's cached pieces. They are made once the workspace exists and before a draw: GPUI only learns that a view
/// it draws can change when the view is older than the frame it is first drawn in, so pieces made while drawing would
/// never draw again.
pub(super) struct Parts {
    pub(super) sidebar: Entity<Part>,
    pub(super) title_bar: Entity<Part>,
    pub(super) main: Entity<Part>,
}

impl Parts {
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
        }
    }
}
