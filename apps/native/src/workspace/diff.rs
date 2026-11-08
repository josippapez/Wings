//! A plugin's diff, as the desktop's: docked beside the tab it was opened from and shown only while that tab is, or
//! filling the window. The dock's left edge drags within the desktop's `SIDES.diff`, and its width is kept with the
//! sidebars'.

use std::path::PathBuf;

use gpui::{
    AnyElement, AppContext, Context, Focusable, Pixels, StyleRefinement, Window, div, prelude::*,
    px, rgb, rgba,
};

use super::{Active, HAIRLINE, Workspace};
use crate::{
    diff::{
        View,
        viewer::{Event, Viewer},
    },
    saved, theme, ui,
};

/// The desktop's `SIDES.diff`: how narrow and how wide the dock can be dragged, and where it starts.
pub(super) const MIN_WIDTH: f32 = 360.0;
pub(super) const MAX_WIDTH: f32 = 1600.0;
pub(super) const INITIAL_WIDTH: f32 = 620.0;
/// The desktop's `minSize={320}` of the tabs beside the dock: they keep this much while the window has it.
const TABS_MIN: f32 = 320.0;

pub(super) struct Dock {
    /// The project and tab on screen when the diff opened, which it docks beside. None, as with no project, fills the
    /// window.
    tab: Option<(PathBuf, Active)>,
    /// Filling the window though it has a tab to dock beside.
    expanded: bool,
    /// The window has room for the dock beside the tab's 320. Without it the diff fills the window, as squeezing the
    /// tab left its terminal a sliver.
    fits: bool,
    pub(super) width: f32,
    /// While the edge is dragged: where the pointer was, and how wide the dock was drawn, when the drag started.
    resize: Option<(Pixels, f32)>,
}

impl Default for Dock {
    fn default() -> Self {
        Self {
            tab: None,
            expanded: false,
            fits: true,
            width: INITIAL_WIDTH,
            resize: None,
        }
    }
}

impl Dock {
    /// What the workspace file kept. A width outside the limits starts again from the initial one, as the desktop's
    /// `fit` does.
    pub(super) fn load(settings: &saved::Settings) -> Self {
        let width = settings
            .diff_width
            .filter(|width| (MIN_WIDTH..=MAX_WIDTH).contains(width))
            .unwrap_or(INITIAL_WIDTH);
        Self {
            width,
            ..Self::default()
        }
    }
}

/// How the open diff is on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shown {
    Docked,
    Full,
}

impl Workspace {
    pub(super) fn open_diff_view(
        &mut self,
        view: View,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self
            .plugins
            .iter()
            .find(|installed| installed.id == view.plugin_id)
            .map(|installed| installed.host.clone())
        else {
            return;
        };
        self.plugin_ui.diff = None;
        window.activate_window();
        let fits = self.dock_fits(window.viewport_size().width);
        let dock = &mut self.plugin_ui.diff_dock;
        dock.fits = fits;
        dock.tab = (!self.welcome).then(|| (self.config.project.clone(), self.active));
        dock.expanded = false;
        let can_dock = dock.tab.is_some() && dock.fits;
        let prefs = &self.prefs;
        let (wrap, tree, files) = (prefs.diff_wrap, prefs.diff_tree, prefs.diff_dock_files);
        let viewer = cx.new(|cx| {
            let mut viewer = Viewer::new(view, host, cx);
            viewer.set_layout(wrap, tree, cx);
            viewer.set_dock_files(files, cx);
            viewer.set_dock(can_dock, can_dock, cx);
            viewer
        });
        let events = cx.subscribe_in(&viewer, window, |this, _, event, window, cx| match event {
            Event::Close => this.close_diff_view(window, cx),
            Event::Layout { wrap, tree } => this.set_prefs(
                |prefs| {
                    prefs.diff_wrap = *wrap;
                    prefs.diff_tree = *tree;
                },
                cx,
            ),
            Event::Expand(expanded) => this.expand_diff(*expanded, window, cx),
            Event::DockFiles(shown) => this.set_prefs(|prefs| prefs.diff_dock_files = *shown, cx),
        });
        // Docked, the keys stay with the tab, as the desktop's dock is no dialog.
        if !can_dock {
            viewer.focus_handle(cx).focus(window, cx);
        }
        self.plugin_ui.diff = Some((viewer, events));
        cx.notify();
    }

    pub(super) fn close_diff_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.plugin_ui.diff = None;
        self.plugin_ui.diff_dock.tab = None;
        self.show(self.active, window, cx);
    }

    /// Fills the window with the diff, or docks it beside its tab again.
    pub(super) fn expand_diff(
        &mut self,
        expanded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dock = &mut self.plugin_ui.diff_dock;
        let Some((viewer, _)) = &self.plugin_ui.diff else {
            return;
        };
        dock.expanded = expanded && dock.tab.is_some();
        let can_dock = dock.tab.is_some() && dock.fits;
        let docked = can_dock && !dock.expanded;
        viewer.update(cx, |viewer, cx| viewer.set_dock(docked, can_dock, cx));
        viewer.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Notes whether a window `viewport` wide has room for the dock, and tells the viewer when that changes, so it
    /// draws itself filling the window, with no button to dock.
    pub(super) fn sync_diff_room(&mut self, viewport: Pixels, cx: &mut Context<Self>) {
        let fits = self.dock_fits(viewport);
        let dock = &mut self.plugin_ui.diff_dock;
        if dock.fits == fits {
            return;
        }
        dock.fits = fits;
        if let (Some((viewer, _)), Some(_)) = (&self.plugin_ui.diff, &dock.tab) {
            let docked = fits && !dock.expanded;
            viewer.update(cx, |viewer, cx| viewer.set_dock(docked, fits, cx));
        }
    }

    /// Docked only while its tab is on screen; on another tab it's kept, out of sight, for when that tab comes back.
    pub(super) fn diff_shown(&self) -> Option<Shown> {
        self.plugin_ui.diff.as_ref()?;
        let dock = &self.plugin_ui.diff_dock;
        match &dock.tab {
            Some(_) if dock.expanded || !dock.fits => Some(Shown::Full),
            Some((project, tab)) => {
                (!self.welcome && *project == self.config.project && *tab == self.active)
                    .then_some(Shown::Docked)
            }
            None => Some(Shown::Full),
        }
    }

    /// The diff closes with the tab it docks beside, as the desktop's does once `diffTab` is no longer among the tabs.
    pub(super) fn close_diff_without_tab(&mut self) {
        let Some((project, tab)) = &self.plugin_ui.diff_dock.tab else {
            return;
        };
        let open = if *project == self.config.project {
            !self.welcome && self.tabs().contains(tab)
        } else {
            self.parked.get(project).is_some_and(|parked| match tab {
                Active::Terminal(id) => parked.terminals.iter().any(|t| t.id == *id),
                Active::Plugin(id) => parked.plugin_tabs.iter().any(|t| t.id == *id),
                Active::Project => parked.project_page.is_some(),
                Active::Issues => parked.issues_open,
            })
        };
        if !open {
            self.plugin_ui.diff = None;
            self.plugin_ui.diff_dock.tab = None;
        }
    }

    /// The window leaves the dock its 360 beside the tab's 320.
    fn dock_fits(&self, viewport: Pixels) -> bool {
        self.diff_space(viewport) >= MIN_WIDTH
    }

    /// The width beside the sidebars and the tab's 320.
    fn diff_space(&self, viewport: Pixels) -> f32 {
        let left = if self.sidebar { self.side.width } else { 0.0 };
        let right = if self.right.is_some() {
            self.right_width(viewport)
        } else {
            0.0
        };
        viewport.as_f32() - left - right - TABS_MIN
    }

    /// The widest the dock can be in a window `viewport` wide: the tab beside it keeps its 320, but the dock never goes
    /// under its 360.
    fn diff_room(&self, viewport: Pixels) -> f32 {
        self.diff_space(viewport).clamp(MIN_WIDTH, MAX_WIDTH)
    }

    /// How wide the dock is drawn: as dragged, within the room the window leaves it.
    pub(super) fn diff_width(&self, viewport: Pixels) -> f32 {
        self.plugin_ui.diff_dock.width.min(self.diff_room(viewport))
    }

    /// The edge was grabbed with the pointer at `x`.
    fn start_diff_resize(&mut self, x: Pixels, viewport: Pixels, cx: &mut Context<Self>) {
        let from = self.diff_width(viewport);
        self.plugin_ui.diff_dock.resize = Some((x, from));
        cx.notify();
    }

    /// Follows the pointer within the limits. Nothing is saved until the pointer is released: the terminals refit when
    /// the panes settle (`RESIZE_DELAY`), and the file is written once.
    fn diff_resize_to(&mut self, x: Pixels, viewport: Pixels, cx: &mut Context<Self>) {
        let Some((from_x, from_width)) = self.plugin_ui.diff_dock.resize else {
            return;
        };
        let room = self.diff_room(viewport);
        let width = (from_width + (from_x - x).as_f32()).clamp(MIN_WIDTH, room);
        if width != self.plugin_ui.diff_dock.width {
            self.plugin_ui.diff_dock.width = width;
            cx.notify();
        }
    }

    fn end_diff_resize(&mut self, cx: &mut Context<Self>) {
        if self.plugin_ui.diff_dock.resize.take().is_some() {
            self.save(cx);
            cx.notify();
        }
    }

    /// The docked diff right of the tab, under the project bar, as the desktop's `p-2 pt-0 pl-0` around a card like a
    /// page's. None unless the diff is docked and its tab is on screen.
    pub(super) fn diff_dock(&self, viewport: Pixels, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.diff_shown() != Some(Shown::Docked) {
            return None;
        }
        let (viewer, _) = self.plugin_ui.diff.as_ref()?;
        Some(
            div()
                .relative()
                .flex_none()
                .w(px(self.diff_width(viewport)))
                .h_full()
                .flex()
                .pr(px(8.0))
                .pb(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .overflow_hidden()
                        .rounded(px(14.0))
                        .border_1()
                        .border_color(rgba(HAIRLINE))
                        .bg(rgb(theme::SURFACE))
                        // Kept as last drawn until it changes, so a busy terminal beside it doesn't rebuild it.
                        .child(
                            viewer
                                .clone()
                                .cached(StyleRefinement::default().size_full()),
                        ),
                )
                .child(self.diff_resize_edge(cx))
                .into_any_element(),
        )
    }

    /// The dock's left edge, the gap between the tab's cards and the dock's, which you drag. Its line shows when you
    /// point at it. The window listens for the pointer while it is down, so the drag goes on wherever the pointer is.
    fn diff_resize_edge(&self, cx: &mut Context<Self>) -> AnyElement {
        ui::resize_edge(
            "diff-dock-edge",
            "Resize diff",
            ui::Side::Right,
            self.plugin_ui.diff_dock.resize.is_some(),
            cx,
            |this, x, window, cx| this.start_diff_resize(x, window.viewport_size().width, cx),
            |this, x, window, cx| this.diff_resize_to(x, window.viewport_size().width, cx),
            Self::end_diff_resize,
        )
        .left(px(-ui::RESIZE_EDGE))
        .into_any_element()
    }
}
