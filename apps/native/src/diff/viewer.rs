//! A plugin's diff, docked beside a tab or filling the window, with its review threads under their lines, or every
//! file stacked in one scroll in a sidebar's place (`stacked.rs`). Only viewport rows become GPUI elements.
mod compose;
mod stacked;
mod threads;
mod content;
mod data;
mod file_list;
mod helpers;
mod input;
mod rows;
mod state;
use helpers::*;

use super::{
    Update, View,
    comments::{self, Comment, Thread},
    highlight::{self, FileColors, Span},
    patch::{DiffLine, DiffLineKind, DiffStatus},
    rows::{CodeRow, ListRow, Notes, code_rows, file_path, list_rows, with_notes},
};
use crate::ui::behaviors::{EscapeExt, Reveal, RevealExt, listbox};
use crate::{
    icons::{Icon, icon},
    markdown::{Block, Opener},
    page::plugin::PluginHost,
    text_field::{FieldEvent, TextField},
    theme, ui,
};
use gpui::{
    AnyElement, App, Context, Div, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    HighlightStyle, KeyDownEvent, ListAlignment, ListOffset, ListState, Pixels, Role, ScrollEvent,
    SharedString, Stateful, StyledText, Subscription, Task,
    UniformListScrollHandle, Window, div, list, point, prelude::*, px, rgb, rgba, uniform_list,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

pub enum Event {
    Close,
    /// Wrap or the folder tree was switched, for the app to keep, as the desktop keeps them in `localStorage`.
    Layout {
        wrap: bool,
        tree: bool,
    },
    /// Expand (true) or Dock beside the tab (false) was pressed.
    Expand(bool),
    /// The docked file list was shown or hidden, for the app to keep, as the desktop's `wings.diff.dockFiles`.
    DockFiles(bool),
    /// A file header's Open in editor: the file's path in the patch, and the line to open it at.
    OpenInEditor { path: String, line: Option<u32> },
}

/// A code row's height, unwrapped.
const ROW: f32 = 22.0;
/// About one character of the 13px code font, to size unwrapped code before it's drawn.
const CHAR: f32 = 8.0;

pub struct Viewer {
    pub view: View,
    host: Arc<PluginHost>,
    selected: usize,
    rows: Vec<CodeRow>,
    /// Side by side, else one column, once chosen (`split`).
    split_choice: Option<bool>,
    /// Drawn beside a tab rather than filling the window.
    pub(crate) docked: bool,
    /// Filling the window, it has a tab to dock beside.
    pub(crate) can_dock: bool,
    /// Shown in a sidebar's place: a Back button leads to the sidebar's page, and there is no expand or close.
    pub(crate) back: bool,
    /// Docked, the file list shows.
    dock_files: bool,
    /// The file list was toggled by hand, so a narrow dock keeps it as chosen instead of hiding it.
    files_touched: bool,
    /// How wide the app draws the viewer, in points; 0 until it says (nothing counts as narrow then).
    width: f32,
    /// Long lines wrap; else they scroll sideways.
    pub(crate) wrap: bool,
    /// The file list is a folder tree, else a flat list.
    pub(crate) tree: bool,
    /// Folders closed in the tree, by path.
    collapsed: HashSet<String>,
    list_rows: Vec<ListRow>,
    filter: Entity<TextField>,
    _filter_events: Subscription,
    /// Wrapped rows, and the rows of a file with threads under its lines, differ in height, so they go in a list that
    /// measures each (`varied_rows`).
    wrapped: ListState,
    /// How far unwrapped code is scrolled sideways when it doesn't scroll as one wide block: side by side, where both
    /// columns move together, or in the list of `varied_rows`, where the threads stay put.
    split_x: Pixels,
    /// The longest line of the open file, in characters.
    widest: usize,
    /// Each file's added and removed lines, for the file list and the header's totals.
    counts: Vec<(usize, usize)>,
    /// The syntax colours of the files opened so far, by index. A file shows plain until its colours come.
    colors: HashMap<usize, FileColors>,
    /// The open file's colours being worked out off the window's thread, for that file of that patch.
    coloring: Option<(usize, Arc<super::patch::Diff>, Task<()>)>,
    code_width: f32,
    scroll: UniformListScrollHandle,
    files_scroll: UniformListScrollHandle,
    horizontal: gpui::ScrollHandle,
    focus: FocusHandle,
    files_focus: FocusHandle,
    /// What was typed in the file list, to jump to the file it starts.
    files_typeahead: listbox::Typeahead,
    code_focus: FocusHandle,
    refresh_focus: FocusHandle,
    close_focus: FocusHandle,
    refreshing: bool,
    refresh_error: Option<String>,
    refresh_task: Option<Task<()>>,
    timeout: Option<Task<()>>,
    /// The threads of `view.update.comments`, oldest first.
    threads: Vec<Thread>,
    /// The threads with no line to sit on, newest first (`comments::discussion`).
    discussion: Vec<usize>,
    /// The discussion is open in place of a file: the desktop's `selected === null`.
    discussion_open: bool,
    discussion_list: ListState,
    /// How many comments each file has, by its index.
    file_comments: Vec<usize>,
    /// The threads under the open file's lines, by `CodeRow::Notes`.
    notes: Vec<Notes>,
    /// The reply boxes open, by the id of their thread's first comment.
    replies: HashMap<i64, threads::ReplyBox>,
    /// The comment being written under a line, when the plugin lets the user start one.
    compose: Option<compose::Compose>,
    /// Comments written here and handed to the plugin, shown with its own until it echoes one back with the same id.
    drafts: Vec<Comment>,
    /// The id the next draft takes: negative, so it can't be a plugin's.
    next_draft: i64,
    /// Why the plugin refused a draft, by its id.
    draft_errors: HashMap<i64, String>,
    #[cfg(feature = "test-support")]
    sent_comments: Vec<serde_json::Value>,
    /// Comment bodies as Markdown, by their place in the comments, parsed the first time they show.
    bodies: RefCell<HashMap<usize, Rc<Vec<Block>>>>,
    /// Hands a comment's link to the plugin's `openUrl` grant.
    opener: Opener,
    /// Why the last comment link didn't open.
    link_error: Option<String>,
    /// Every file in one scroll, when the workspace asks for it.
    stack: stacked::Stack,
    /// The editor a file header opens its file in, by its name, when the app knows one.
    editor: Option<SharedString>,
    #[cfg(feature = "test-support")]
    rendered_rows: std::cell::Cell<usize>,
}

impl EventEmitter<Event> for Viewer {}
impl Focusable for Viewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.host.diff_closed(&self.view.id);
    }
}

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_focus = self.refresh_focus.clone().tab_stop(!self.refreshing);
        #[cfg(feature = "test-support")]
        self.rendered_rows.set(0);
        let content = if let Some(error) = &self.view.update.error {
            self.message(error, true)
        } else {
            self.content(cx)
        };
        let (docked, split) = (self.docked, self.split());
        // Docked, the toggles show only their icons, as the desktop's.
        let text = |label| (!docked).then_some(label);
        div()
            .id("native-diff")
            .size_full()
            .flex()
            .flex_col()
            // Docked, the dock's card is the background.
            .when(!docked, |viewer| viewer.bg(rgb(theme::background())))
            .text_color(rgb(theme::foreground()))
            .text_size(px(13.0))
            // Only filling the window is it a dialog; in the dock it's a part of the window beside the tab.
            .role(if docked { Role::Region } else { Role::Dialog })
            .aria_label(self.view.title.clone())
            .track_focus(&self.focus)
            // A menu open in it closes first; else the viewer does.
            .on_escape(cx.listener(|this, _: &KeyDownEvent, _, cx| {
                if this.close_menus() {
                    cx.notify();
                } else {
                    cx.emit(Event::Close);
                }
            }))
            .on_key_down(cx.listener(Self::key))
            .children(ui::spot(|| "diff".into()))
            .children(self.stack_menus(cx))
            // The desktop's header: `py-3 pr-3` and a 15px title when the diff fills the window, `py-2 pr-2 pl-4` and
            // 13px docked. Under the title, the subtitle, the number of files and the lines added and removed. Filling
            // the window, it starts right of the window's buttons, which sit over its top-left corner.
            .child(if self.stacked() {
                self.stack_bar(cx)
            } else {
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .py(px(if docked { 8.0 } else { 12.0 }))
                    .pr(px(if docked { 8.0 } else { 12.0 }))
                    .pl(px(if self.back {
                        8.0
                    } else if docked {
                        16.0
                    } else {
                        ui::TRAFFIC_LIGHTS
                    }))
                    .border_b_1()
                    .border_color(rgba(ui::HAIRLINE))
                    .when(self.back, |bar| {
                        bar.child(
                            ui::icon_button(
                                "diff-back",
                                Icon::ArrowLeft,
                                "Back",
                                ui::ButtonSize::Sm,
                            )
                            .track_focus(&self.close_focus)
                            .children(ui::spot(|| "diff-back".into()))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close))),
                        )
                    })
                    .children(
                        self.view
                            .update
                            .scopes
                            .clone()
                            .filter(|scopes| !scopes.is_empty())
                            .map(|scopes| self.scope_picker(&scopes, cx)),
                    )
                    .child(
                        div()
                            .mr(px(12.0))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(if docked { 13.0 } else { 15.0 }))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(self.view.title.clone()),
                            )
                            .child(
                                div()
                                    .mt(px(2.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .min_w_0()
                                    .whitespace_nowrap()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme::muted_foreground()))
                                    .when(!self.view.subtitle.is_empty(), |line| {
                                        line.child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .child(self.view.subtitle.clone()),
                                        )
                                    })
                                    .when_some(self.view.update.patch.as_ref(), |line, patch| {
                                        let (added, removed) = self
                                            .counts
                                            .iter()
                                            .fold((0, 0), |(a, r), (fa, fr)| (a + fa, r + fr));
                                        line.child(format!("{} files", patch.files.len()))
                                            .child(
                                                div()
                                                    .text_color(rgb(theme::done()))
                                                    .child(format!("+{added}")),
                                            )
                                            .child(
                                                div()
                                                    .text_color(rgb(theme::blocked()))
                                                    .child(format!("−{removed}")),
                                            )
                                    })
                                    .when(self.view.update.patch.is_some(), |line| {
                                        let count = self.comments().len();
                                        let outdated = comments::outdated(self.comments());
                                        line.when(count > 0, |line| {
                                            // Docked there's room only for the number.
                                            line.child(comment_count(
                                                match (docked, outdated) {
                                                    (true, _) => count.to_string(),
                                                    (false, 0) => format!("{count} comments"),
                                                    (false, _) => format!(
                                                        "{count} comments, {outdated} on code that changed since"
                                                    ),
                                                },
                                                14.0,
                                            )
                                            .when(docked, |count| count.flex_none()))
                                        })
                                    }),
                            ),
                    )
                    .when(self.view.refresh, |bar| {
                        bar.child(
                            ui::icon_button(
                                "diff-refresh",
                                Icon::RefreshCw,
                                if self.refreshing {
                                    "Refreshing…"
                                } else {
                                    "Refresh"
                                },
                                ui::ButtonSize::Sm,
                            )
                            .track_focus(&self.refresh_focus)
                            .when(self.refreshing, |button| button.opacity(0.5))
                            .when(!self.refreshing, |button| {
                                button.on_click(
                                    cx.listener(|this, _, _, cx| this.request_refresh(cx)),
                                )
                            }),
                        )
                    })
                    .child(toggle_group([
                        toggle("diff-split", Icon::Columns, "Split", text("Split"), split)
                            .on_click(cx.listener(|this, _, _, cx| this.set_split(true, cx))),
                        toggle(
                            "diff-unified",
                            Icon::Rows3,
                            "Unified",
                            text("Unified"),
                            !split,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.set_split(false, cx))),
                    ]))
                    .child(
                        toggle(
                            "diff-wrap",
                            Icon::TextWrap,
                            "Wrap long lines",
                            text("Wrap"),
                            self.wrap,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.set_wrap(!this.wrap, cx))),
                    )
                    .when(docked, |bar| {
                        bar.child(
                            toggle(
                                "diff-files-toggle",
                                Icon::PanelLeft,
                                "File list",
                                None,
                                self.files_shown(),
                            )
                            .children(ui::spot(|| "diff-files-toggle".into()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choose_dock_files(!this.files_shown(), cx)
                            })),
                        )
                    })
                    .when(!self.back, |bar| {
                        bar.child(
                            div()
                                .mx(px(4.0))
                                .h(px(16.0))
                                .w(px(1.0))
                                .bg(rgba(ui::HAIRLINE)),
                        )
                    })
                    .when(docked && !self.back, |bar| {
                        bar.child(
                            ui::untipped_ghost_button(
                                "diff-expand",
                                "Expand",
                                ui::ButtonSize::Sm,
                                None,
                                icon(Icon::Maximize, 16.0, theme::muted_foreground()),
                            )
                            .tooltip(ui::tooltip("Fill the window", None))
                            .children(ui::spot(|| "diff-expand".into()))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Expand(true)))),
                        )
                    })
                    .when(!docked && self.can_dock, |bar| {
                        bar.child(
                            ui::icon_button(
                                "diff-dock",
                                Icon::Minimize,
                                "Dock beside the tab",
                                ui::ButtonSize::Sm,
                            )
                            .children(ui::spot(|| "diff-dock".into()))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Expand(false)))),
                        )
                    })
                    .when(!self.back, |bar| {
                        bar.child(
                            ui::icon_button(
                                "diff-close",
                                Icon::X,
                                "Close diff",
                                ui::ButtonSize::Sm,
                            )
                            .track_focus(&self.close_focus)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close))),
                        )
                    })
                    .into_any_element()
            })
            .when_some(self.link_error.clone(), |body, error| {
                body.child(
                    div()
                        .id("diff-link-error")
                        .px(px(16.0))
                        .py(px(8.0))
                        .role(Role::Alert)
                        .text_color(rgb(theme::danger()))
                        .child(error),
                )
            })
            .when_some(self.refresh_error.clone(), |body, error| {
                body.child(
                    div()
                        .id("diff-refresh-error")
                        .px(px(16.0))
                        .py(px(8.0))
                        .role(Role::Alert)
                        .text_color(rgb(theme::danger()))
                        .child(error),
                )
            })
            .child(content)
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests;

#[cfg(all(test, feature = "test-support"))]
mod layout_tests {
    use super::*;
    use crate::{diff, page::plugin::PluginConfig};
    use gpui::{TestApp, TestAppWindow};
    use std::{cell::RefCell, rc::Rc};

    /// Each `Event::Layout` the viewer sent: wrap, then tree.
    type Layouts = Rc<RefCell<Vec<(bool, bool)>>>;

    fn open(patch: &str) -> (TestApp, TestAppWindow<Viewer>, Layouts) {
        let view = View {
            id: "test:2".into(),
            plugin_id: "test".into(),
            sidebar: None,
            title: "Changes".into(),
            subtitle: String::new(),
            cwd: String::new(),
            refresh: false,
            update: diff::clean_update(&serde_json::json!({ "patch": patch })).unwrap(),
        };
        let host = Arc::new(PluginHost::new(PluginConfig {
            id: "test".into(),
            root: std::env::temp_dir(),
            scope: "test".into(),
        }));
        let mut app = TestApp::new();
        let window = app.open_window(move |_, cx| Viewer::new(view, host, cx));
        let layouts = Rc::new(RefCell::new(Vec::new()));
        let seen = layouts.clone();
        let viewer = window.root();
        let subscription = app.update(|cx| {
            cx.subscribe(&viewer, move |_, event: &Event, _| {
                if let Event::Layout { wrap, tree } = event {
                    seen.borrow_mut().push((*wrap, *tree));
                }
            })
        });
        std::mem::forget(subscription);
        (app, window, layouts)
    }

    /// A vertical wheel over one column of unwrapped code scrolls it down, not sideways: with only sideways scrolling,
    /// GPUI turns a vertical wheel into sideways movement unless told to keep to the axis.
    #[test]
    fn a_vertical_wheel_over_unwrapped_code_does_not_move_it_sideways() {
        let long = "x".repeat(400);
        let lines: String = (0..60).map(|i| format!("+{long}{i}\n")).collect();
        let patch =
            format!("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,60 @@\n{lines}");
        let (_app, mut window, _) = open(&patch);
        window.update(|v, _, cx| v.set_split(false, cx));
        window.draw();
        window.simulate_event(gpui::ScrollEvent {
            position: point(px(600.0), px(300.0)),
            delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-200.0))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
            source: Default::default(),
        });
        window.draw();
        window.read(|v, _| {
            assert_eq!(v.horizontal.offset().x, px(0.0));
            assert!(
                v.scroll.0.borrow().base_handle.offset().y < px(0.0),
                "it didn't scroll down"
            );
        });
    }

    #[test]
    fn the_filter_tree_split_and_wrap_choices_change_what_shows() {
        let patch = "diff --git a/src/app.rs b/src/app.rs\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,2 +1,2 @@\n same\n-old\n+new\ndiff --git a/src/lib/api.rs b/src/lib/api.rs\n--- a/src/lib/api.rs\n+++ b/src/lib/api.rs\n@@ -1 +1 @@\n-a\n+b\ndiff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-x\n+y\n";
        let (_app, mut window, layouts) = open(patch);
        window.draw();
        // Side by side first, as the desktop's modal: the removed and added line share a row.
        window.read(|v, _| {
            assert_eq!(
                v.rows,
                [
                    CodeRow::Hunk(0),
                    CodeRow::Split {
                        hunk: 0,
                        left: Some(0),
                        right: Some(0)
                    },
                    CodeRow::Split {
                        hunk: 0,
                        left: Some(1),
                        right: Some(2)
                    },
                ]
            )
        });

        // Typing in the filter narrows the list, and the arrows go through what it shows.
        window.update(|v, window, cx| v.filter.focus_handle(cx).focus(window, cx));
        window.simulate_input("API");
        window.read(|v, _| assert_eq!(v.listed_files(), [1]));
        window.update(|v, window, cx| {
            v.files_focus.focus(window, cx);
            v.select_file(0, cx);
        });
        window.simulate_keystroke("down");
        assert_eq!(window.read(|v, _| v.selected_file()), 1);

        window.update(|v, _, cx| {
            v.filter.update(cx, |field, cx| {
                field.text.clear();
                cx.emit(FieldEvent::Changed);
            });
            v.set_tree(true, cx);
        });
        window.read(|v, _| {
            assert_eq!(
                v.list_rows.first(),
                Some(&ListRow::Folder {
                    path: "src".into(),
                    label: "src".into(),
                    depth: 0,
                    open: true
                })
            );
            assert_eq!(v.listed_files(), [1, 0, 2], "folders first, then files");
        });

        // Typing a file's name goes to the file: "a" from README.md is the first of api.rs and app.rs.
        window.update(|v, window, cx| {
            v.files_focus.focus(window, cx);
            v.select_file(2, cx);
        });
        window.simulate_keystroke("a->a");
        assert_eq!(window.read(|v, _| v.selected_file()), 1);

        window.update(|v, _, cx| {
            v.set_split(false, cx);
            v.select_file(0, cx);
        });
        window.read(|v, _| {
            assert_eq!(
                v.rows[1..],
                [
                    CodeRow::Line(0, 0),
                    CodeRow::Line(0, 1),
                    CodeRow::Line(0, 2)
                ]
            )
        });

        window.update(|v, _, cx| v.set_wrap(true, cx));
        window.draw();
        window.read(|v, _| assert_eq!(v.wrapped.item_count(), v.rows.len()));
        assert_eq!(*layouts.borrow(), [(false, true), (true, true)]);
    }

    /// The desktop's docked diff: one column until you pick, and a file list its toggle hides, which the app keeps, and
    /// Expand. Filling the window it's side by side again, with Dock beside the tab when it has a tab to go back to.
    #[test]
    fn docked_it_starts_in_one_column_with_a_file_list_toggle_and_offers_to_fill_the_window() {
        let patch = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n";
        let (mut app, mut window, _) = open(patch);
        let events = Rc::new(RefCell::new(Vec::new()));
        let seen = events.clone();
        let viewer = window.root();
        let _subscription = app.update(|cx| {
            cx.subscribe(&viewer, move |_, event: &Event, _| match event {
                Event::Expand(full) => seen.borrow_mut().push(format!("expand {full}")),
                Event::DockFiles(shown) => seen.borrow_mut().push(format!("files {shown}")),
                _ => {}
            })
        });
        let unified = [CodeRow::Line(0, 0), CodeRow::Line(0, 1)];
        let spot = |name: &str| crate::ui::spot_at(name).unwrap_or_else(|| panic!("no {name}"));
        let click = |window: &mut TestAppWindow<Viewer>, name: &str| {
            window.simulate_click(spot(name).center(), gpui::MouseButton::Left);
            window.draw();
        };

        window.update(|v, _, cx| v.set_dock(true, true, cx));
        window.draw();
        window.read(|v, _| assert_eq!(v.rows[1..], unified));
        // The code starts after the dock's 224 file list until the toggle hides the list.
        assert_eq!(spot("diff-code").left(), px(224.0));
        click(&mut window, "diff-files-toggle");
        assert_eq!(spot("diff-code").left(), px(0.0));
        click(&mut window, "diff-expand");
        assert_eq!(*events.borrow(), ["files false", "expand true"]);

        // Filling the window it shows the list and goes side by side, and offers to dock again.
        window.update(|v, _, cx| v.set_dock(false, true, cx));
        window.draw();
        window.read(|v, _| assert!(matches!(v.rows[1], CodeRow::Split { .. })));
        assert_eq!(spot("diff-code").left(), px(288.0));
        click(&mut window, "diff-dock");
        assert_eq!(events.borrow().last().unwrap(), "expand false");

        // A layout you chose stays wherever it shows.
        window.update(|v, _, cx| {
            v.set_split(false, cx);
            v.set_dock(true, true, cx);
            v.set_dock(false, true, cx);
        });
        window.read(|v, _| assert_eq!(v.rows[1..], unified));
    }
}

#[cfg(all(test, feature = "test-support"))]
mod comment_tests {
    use super::*;
    use crate::{diff, page::plugin::PluginConfig};
    use gpui::{TestApp, TestAppWindow};
    use serde_json::{Value, json};

    fn open(payload: Value) -> (TestApp, TestAppWindow<Viewer>) {
        let view = View {
            id: "test:3".into(),
            plugin_id: "test".into(),
            sidebar: None,
            title: "Pull request".into(),
            subtitle: String::new(),
            cwd: String::new(),
            refresh: false,
            update: diff::clean_update(&payload).unwrap(),
        };
        let host = Arc::new(PluginHost::new(PluginConfig {
            id: "test".into(),
            root: std::env::temp_dir(),
            scope: "test".into(),
        }));
        let mut app = TestApp::new();
        let window = app.open_window(move |_, cx| Viewer::new(view, host, cx));
        (app, window)
    }

    fn comment(id: i64, reply_to: Option<i64>, path: Option<&str>, line: Option<usize>) -> Value {
        json!({ "id": id, "replyTo": reply_to, "path": path, "line": line, "side": "additions", "author": "maria",
            "body": format!("Comment {id}"), "createdAt": format!("2026-11-01T10:00:{id:02}Z"), "url": "", "canReply": reply_to.is_none() })
    }

    /// The desktop's threads: under the line they're on, after its number column, with their replies; the comments on
    /// the whole change and on code that changed since in the discussion, which the file list opens.
    #[test]
    fn a_diff_with_comments_shows_each_thread_under_its_line() {
        let patch = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";
        let (_app, mut window) = open(
            json!({ "patch": patch, "focus": { "path": "a.rs", "line": 2 }, "comments": [
            comment(1, None, Some("a.rs"), Some(2)),
            comment(2, Some(1), Some("a.rs"), Some(2)),
            comment(3, None, None, None),
            comment(4, None, Some("a.rs"), None),
        ] }),
        );
        window.update(|v, _, cx| v.set_split(false, cx));
        window.draw();
        window.read(|v, _| {
            assert_eq!(
                v.rows,
                [
                    CodeRow::Hunk(0),
                    CodeRow::Line(0, 0),
                    CodeRow::Line(0, 1),
                    CodeRow::Line(0, 2),
                    CodeRow::Notes(0),
                    CodeRow::Line(0, 3),
                ]
            );
            let thread = &v.threads[v.notes[0].new[0]];
            assert_eq!((v.comments()[thread.root].id, thread.replies.len()), (1, 1));
            assert_eq!(
                v.file_comments,
                [3],
                "the thread and its reply, and the one on changed code"
            );
            assert_eq!(
                v.listed(),
                [None, Some(0)],
                "the discussion before the file"
            );
        });
        // The card sits under the added line, the fourth row under the file's 32px name, past the 48px numbers. Its spot
        // is inside its 1px border.
        let code = crate::ui::spot_at("diff-code").unwrap();
        let card = crate::ui::spot_at("diff-thread-1").unwrap();
        assert_eq!(card.top(), code.top() + px(32.0 + 4.0 * ROW + 6.0 + 1.0));
        assert_eq!(card.left(), code.left() + px(48.0 + 1.0));
        // Side by side it's in the new side's column.
        window.update(|v, _, cx| v.set_split(true, cx));
        window.draw();
        let card = crate::ui::spot_at("diff-thread-1").unwrap();
        assert!(card.left() > code.left() + code.size.width / 2.0);

        // The discussion opens from the file list and shows the other two, the newest first.
        window.simulate_click(
            crate::ui::spot_at("diff-discussion").unwrap().center(),
            gpui::MouseButton::Left,
        );
        window.draw();
        window.read(|v, _| {
            assert!(v.discussion_open);
            let roots: Vec<i64> = v
                .discussion
                .iter()
                .map(|&t| v.comments()[v.threads[t].root].id)
                .collect();
            assert_eq!(roots, [4, 3]);
        });
        assert!(
            crate::ui::spot_at("diff-thread-4").unwrap().top()
                < crate::ui::spot_at("diff-thread-3").unwrap().top()
        );
        // It stays open when the layout changes: the plugin's focus picked the file only until something else was.
        window.update(|v, _, cx| v.set_split(false, cx));
        window.read(|v, _| assert!(v.discussion_open));
        window.simulate_keystroke("down");
        window.read(|v, _| assert!(!v.discussion_open && v.selected_file() == 0));
    }

    /// A dock too narrow for the code beside its 224 file list leaves the list out until you open it, and a card in a
    /// narrow column spans it instead of sitting 48 points in: the cards had a word a line.
    #[test]
    fn a_narrow_dock_leaves_out_its_file_list_and_its_cards_span_the_column() {
        let patch = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";
        let (_app, mut window) = open(json!({ "patch": patch, "focus": { "path": "a.rs", "line": 2 }, "comments": [
            comment(1, None, Some("a.rs"), Some(2)),
            comment(3, None, None, None),
            comment(4, None, Some("a.rs"), None),
        ] }));
        window.update(|v, _, cx| {
            v.set_split(false, cx);
            v.set_dock(true, true, cx);
            v.set_width(520.0, cx);
        });
        window.draw();
        window.read(|v, _| {
            assert!(!v.files_shown(), "520 less the 224 list leaves the code under 480");
            assert_eq!(v.discussion_count(), 1, "only the comment on the whole change; the other is its file's");
        });
        let code = crate::ui::spot_at("diff-code").unwrap();
        assert_eq!(code.left(), px(0.0));
        let card = crate::ui::spot_at("diff-thread-1").unwrap();
        assert_eq!(card.left(), code.left() + px(12.0 + 1.0), "no indent past the line numbers");
        // The toggle opens it, and it stays as chosen when the dock grows or shrinks.
        window.simulate_click(crate::ui::spot_at("diff-files-toggle").unwrap().center(), gpui::MouseButton::Left);
        window.draw();
        window.read(|v, _| assert!(v.files_shown()));
        // A wide dock shows it by itself.
        let (_app, mut window) = open(json!({ "patch": patch }));
        window.update(|v, _, cx| {
            v.set_dock(true, true, cx);
            v.set_width(760.0, cx);
        });
        window.read(|v, _| assert!(v.files_shown()));
        // Filling a narrow window it starts in one column; a wide one side by side; a choice is kept.
        let (_app, mut window) = open(json!({ "patch": patch }));
        window.update(|v, _, cx| v.set_width(640.0, cx));
        window.read(|v, _| assert!(!v.split()));
        window.update(|v, _, cx| v.set_width(1000.0, cx));
        window.read(|v, _| assert!(v.split()));
    }

    /// A reply comes back as `updateDiff` with the comments alone, or with the patch already shown on a live diff's next
    /// load: the code stays where it's scrolled to, even when a file gets its first thread above.
    #[test]
    fn new_comments_leave_the_code_where_it_is_scrolled_to() {
        let patch = format!(
            "diff --git a/big b/big\nnew file mode 100644\n--- /dev/null\n+++ b/big\n@@ -0,0 +1,300 @@\n{}",
            (1..=300)
                .map(|n| format!("+line {n}\n"))
                .collect::<String>()
        );
        let (_app, mut window) = open(json!({ "patch": patch }));
        window.update(|v, _, cx| v.set_split(false, cx));
        window.draw();
        // Row 101 is line 101, three pixels scrolled into it.
        window.update(|v, _, _| {
            let handle = v.scroll.0.borrow().base_handle.clone();
            handle.set_offset(point(px(0.0), -px(101.0 * ROW + 3.0)));
        });
        window.draw();
        let update = |comments: Value, patch: Option<&str>| {
            let mut payload = json!({ "comments": comments });
            if let Some(patch) = patch {
                payload["patch"] = json!(patch);
            }
            diff::clean_update(&payload).unwrap()
        };
        let patch_again = update(
            json!([comment(1, None, Some("big"), Some(10))]),
            Some(&patch),
        );
        window.update(|v, _, cx| v.update(patch_again, cx));
        window.draw();
        // A thread on line 10 now sits above: the same line is at the top, under the list's one more row.
        window.read(|v, _| {
            assert!(v.varied_rows());
            let top = v.wrapped.logical_scroll_top();
            assert_eq!((top.item_ix, top.offset_in_item), (102, px(3.0)));
        });
        // The reply arrives as comments alone; the thread at the top keeps its place.
        window.update(|v, _, _| {
            v.wrapped.scroll_to(ListOffset {
                item_ix: 11,
                offset_in_item: px(40.0),
            })
        });
        window.draw();
        let reply = update(
            json!([
                comment(1, None, Some("big"), Some(10)),
                comment(2, Some(1), Some("big"), Some(10))
            ]),
            None,
        );
        window.update(|v, _, cx| v.update(reply, cx));
        window.draw();
        window.read(|v, _| {
            assert_eq!(v.rows[11], CodeRow::Notes(0));
            assert_eq!(v.threads[0].replies.len(), 1);
            let top = v.wrapped.logical_scroll_top();
            assert_eq!((top.item_ix, top.offset_in_item), (11, px(40.0)));
        });
    }

    const ONE_FILE: &str = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";

    fn commentable() -> (TestApp, TestAppWindow<Viewer>) {
        let (app, mut window) = open(json!({ "patch": ONE_FILE, "allowNewComments": true }));
        window.update(|v, _, cx| v.set_split(false, cx));
        window.draw();
        (app, window)
    }

    /// A diff opened with `allowNewComments` has a "+" in each code line's gutter; one without has none.
    #[test]
    fn the_gutter_control_is_there_only_when_the_plugin_allows_new_comments() {
        let (_app, _window) = commentable();
        // Row 0 is the hunk's header, row 1 the unchanged line, row 2 the removed one (its old side).
        assert!(crate::ui::spot_at("diff-add-1-new").is_some());
        assert!(crate::ui::spot_at("diff-add-2-old").is_some());
        assert!(crate::ui::spot_at("diff-add-0-new").is_none());
    }

    #[test]
    fn a_diff_that_does_not_allow_new_comments_has_no_gutter_control() {
        let (_app, mut window) = open(json!({ "patch": ONE_FILE }));
        window.update(|v, _, cx| v.set_split(false, cx));
        window.draw();
        assert!(crate::ui::spot_at("diff-code").is_some());
        assert!(crate::ui::spot_at("diff-add-1-new").is_none());
        window.update(|v, window, cx| v.open_compose(0, 2, false, window, cx));
        window.read(|v, _| assert!(v.compose.is_none()));
    }

    /// Enter saves what's typed: the plugin's `onComment` gets the line, its side and its text, and the draft shows under
    /// the line. The plugin adding it back with the draft's id replaces it, and Delete tells the plugin.
    #[test]
    fn a_saved_comment_reaches_the_plugin_and_shows_as_a_draft_under_its_line() {
        let (_app, mut window) = commentable();
        window.update(|v, window, cx| v.open_compose(0, 2, false, window, cx));
        window.draw();
        window.read(|v, _| assert!(v.compose.is_some()));
        assert!(crate::ui::spot_at("diff-compose").is_some());
        window.simulate_input("Why upper case?");
        window.simulate_keystroke("enter");
        window.draw();
        window.read(|v, _| {
            assert!(v.compose.is_none());
            assert_eq!(
                v.sent_comments,
                [json!({ "path": "a.rs", "side": "new", "line": 2, "quote": "TWO", "body": "Why upper case?", "id": -1 })]
            );
            let draft = &v.comments()[0];
            assert!(draft.draft && draft.line == Some(2) && !draft.old && draft.body == "Why upper case?");
            assert_eq!(
                v.rows.iter().filter(|row| matches!(row, CodeRow::Notes(_))).count(),
                1
            );
        });
        // A comment on the removed side is on its old number.
        window.update(|v, window, cx| v.open_compose(0, 1, true, window, cx));
        window.draw();
        window.simulate_input("Keep it");
        window.simulate_keystroke("cmd-enter");
        window.read(|v, _| {
            assert_eq!(v.sent_comments[1]["side"], "old");
            assert_eq!((v.sent_comments[1]["line"].as_u64(), v.sent_comments[1]["quote"].as_str()), (Some(2), Some("two")));
            assert_eq!(v.comments().len(), 2);
        });
        // The plugin shows the first one itself under the same id: one of them, not two.
        let echoed = diff::clean_update(&json!({ "comments": [
            { "id": -1, "path": "a.rs", "line": 2, "side": "additions", "author": "you", "body": "Why upper case?",
              "createdAt": "2026-11-01T10:00:00Z", "canReply": true }
        ] }))
        .unwrap();
        window.update(|v, _, cx| v.update(echoed, cx));
        window.read(|v, _| {
            let ids: Vec<i64> = v.comments().iter().map(|c| c.id).collect();
            assert_eq!(ids.iter().filter(|id| **id == -1).count(), 1);
            assert_eq!(ids.len(), 2, "the second draft stays");
            assert!(v.comments().iter().find(|c| c.id == -1).is_some_and(|c| !c.draft));
        });
        window.update(|v, _, cx| v.delete_draft(-2, cx));
        window.read(|v, _| {
            assert_eq!(v.sent_comments.last().unwrap(), &json!({ "delete": true, "id": -2 }));
            assert_eq!(v.comments().len(), 1);
        });
    }

    /// The one-file view a diff with new comments gets has the scope picker, and its menu opens.
    #[test]
    fn the_scope_picker_shows_when_new_comments_are_allowed() {
        let (_app, mut window) = open(json!({ "patch": ONE_FILE, "allowNewComments": true,
            "scopes": [{ "id": "uncommitted", "label": "Uncommitted" }, { "id": "branch", "label": "Branch" }],
            "scope": "uncommitted" }));
        window.update(|v, _, cx| v.set_split(false, cx));
        window.draw();
        window.read(|v, _| assert!(!v.stacked()));
        let picker = crate::ui::spot_at("diff-scope").expect("the scope picker was drawn");
        window.simulate_click(picker.center(), gpui::MouseButton::Left);
        window.draw();
        assert!(crate::ui::spot_at("diff-scope-item-1").is_some());
    }

    #[test]
    fn escape_cancels_the_box_and_nothing_is_sent() {
        let (_app, mut window) = commentable();
        window.update(|v, window, cx| v.open_compose(0, 2, false, window, cx));
        window.draw();
        window.simulate_input("never mind");
        window.simulate_keystroke("escape");
        window.draw();
        window.read(|v, _| {
            assert!(v.compose.is_none());
            assert!(v.sent_comments.is_empty() && v.comments().is_empty());
            assert!(!v.rows.iter().any(|row| matches!(row, CodeRow::Notes(_))));
        });
        // An empty box doesn't save.
        window.update(|v, window, cx| v.open_compose(0, 2, false, window, cx));
        window.draw();
        window.simulate_keystroke("enter");
        window.read(|v, _| assert!(v.compose.is_some() && v.sent_comments.is_empty()));
    }
}

#[cfg(all(test, feature = "test-support"))]
mod color_tests {
    use super::*;
    use crate::{
        diff::{self, patch::parse_patch},
        page::plugin::PluginConfig,
    };
    use gpui::TestApp;

    fn view(patch: &str) -> View {
        View {
            id: "test:3".into(),
            plugin_id: "test".into(),
            sidebar: None,
            title: "Changes".into(),
            subtitle: String::new(),
            cwd: String::new(),
            refresh: false,
            update: diff::clean_update(&serde_json::json!({ "patch": patch })).unwrap(),
        }
    }

    fn host() -> Arc<PluginHost> {
        Arc::new(PluginHost::new(PluginConfig {
            id: "test".into(),
            root: std::env::temp_dir(),
            scope: "test".into(),
        }))
    }

    /// The colour of the stretch of `spans` that holds byte `at`.
    fn color_at(spans: &[Span], at: usize) -> Option<u32> {
        spans
            .iter()
            .find(|span| span.end > at)
            .map(|span| span.color)
    }

    #[test]
    fn a_big_file_opens_plain_and_takes_its_colours_from_off_the_window_thread() {
        let lines: String = (0..10_000)
            .map(|i| format!("+let value_{i} = compute({i}); // note\n"))
            .collect();
        let patch = format!(
            "diff --git a/src/big.rs b/src/big.rs\nnew file mode 100644\n--- /dev/null\n+++ b/src/big.rs\n@@ -0,0 +1,10000 @@\n{lines}"
        );
        let (view, host) = (view(&patch), host());
        let mut app = TestApp::new();
        let (window, at_open) = app.update(|cx| {
            let window = cx
                .open_window(Default::default(), |_, cx| {
                    cx.new(|cx| Viewer::new(view, host, cx))
                })
                .unwrap();
            (
                window,
                window.read_with(cx, |v, _| !v.colors.is_empty()).unwrap(),
            )
        });
        assert!(
            !at_open,
            "the colours were worked out before the diff opened"
        );
        // The work ran once the window's thread was free, and the lines take the desktop's colours.
        let spans = app.read(|cx| {
            window
                .read_with(cx, |v, _| v.spans(0, 9_999, false).to_vec())
                .unwrap()
        });
        let text = "let value_9999 = compute(9999); // note";
        assert_eq!(
            color_at(&spans, text.find("9999)").unwrap()),
            Some(0x68cdf2)
        );
        assert_eq!(color_at(&spans, text.find("//").unwrap()), Some(0x737373));
    }

    #[test]
    fn a_reload_keeps_the_colours_of_files_that_did_not_change() {
        let patch = |b: &str| {
            format!(
                "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-let a = 1;\n+let a = 2;\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-let b = 1;\n+{b}\n"
            )
        };
        let (view, host) = (view(&patch("let b = 2;")), host());
        let mut app = TestApp::new();
        let mut window = app.open_window(move |_, cx| Viewer::new(view, host, cx));
        window.update(|v, _, cx| v.select_file(1, cx));
        window.read(|v, _| assert_eq!(v.colors.len(), 2));
        // b.rs changes, and is the file open: a.rs keeps its colours and b.rs takes its new line's.
        let changed = Arc::new(parse_patch(&patch("let b = 3; // three")).unwrap());
        window.update(|v, _, cx| {
            v.update(
                Update {
                    patch: Some(changed),
                    ..Update::default()
                },
                cx,
            );
            assert!(v.colors.contains_key(&0) && !v.colors.contains_key(&1));
        });
        window.read(|v, _| {
            let at = "let b = 3; // three".find("//").unwrap();
            assert_eq!(color_at(v.spans(0, 1, false), at), Some(0x737373));
        });
    }
}
