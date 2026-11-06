//! The current project: its sidebar list, entering it the first time and switching between projects.

use std::path::PathBuf;

use gpui::{AnyElement, Context, Role, Window, div, prelude::*, px, rgb, rgba};

use crate::{
    agents,
    icons::{Icon, icon},
    plugin, saved, theme,
};

use super::{HAIRLINE, Parked, Workspace};

/// The desktop's `--sidebar-accent` behind the shown project, and `--sidebar-foreground`.
pub(super) const SIDEBAR_ACCENT: u32 = 0xffffff14;
pub(super) const SIDEBAR_FOREGROUND: u32 = 0xe4e0da;
/// The desktop sidebar's starting width.
pub(super) const SIDEBAR_WIDTH: f32 = 248.0;

impl Workspace {
    /// The current project's saved tabs, or one new terminal, and its Issues.
    pub(super) fn enter_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restored = self
            .config
            .state
            .as_deref()
            .and_then(|path| saved::load(path, &self.project_key()));
        match restored {
            Some(project) if !(project.tabs.is_empty() && project.plugin_tabs.is_empty()) => {
                self.restore(project, window, cx)
            }
            _ => self.new_terminal(window, cx),
        }
        self.load_issues(cx);
    }

    fn load_issues(&mut self, cx: &mut Context<Self>) {
        if self.config.issues.is_some() {
            // A load still running belongs to the project shown before.
            self.load_task = None;
            self.in_flight = false;
            self.load(None, cx);
        } else {
            self.issues.update(cx, |view, cx| view.set_setup(cx));
        }
    }

    pub fn project(&self) -> &std::path::Path {
        &self.config.project
    }

    /// Shows another project. The one shown now keeps its tabs and shells for when it comes back.
    pub fn switch_project(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if path == self.config.project {
            return;
        }
        self.save(cx);
        let current = Parked {
            terminals: std::mem::take(&mut self.terminals),
            plugin_tabs: std::mem::take(&mut self.plugin_tabs),
            active: self.active,
        };
        self.parked.insert(self.config.project.clone(), current);
        self.config.project = path.clone();
        match self.parked.remove(&path) {
            Some(parked) => {
                self.terminals = parked.terminals;
                self.plugin_tabs = parked.plugin_tabs;
                self.show(parked.active, window, cx);
                self.load_issues(cx);
            }
            None => self.enter_project(window, cx),
        }
    }

    /// The desktop's project list, left of the tabs.
    pub(super) fn sidebar_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<_> = (0..self.projects.len())
            .map(|index| self.sidebar_row(index, cx))
            .collect();
        let projects = div()
            .id("sidebar")
            .role(Role::Navigation)
            .aria_label("Projects")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(px(8.0))
            .overflow_y_scroll()
            .child(
                div()
                    .h(px(28.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(8.0))
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(rgb(theme::MUTED_FOREGROUND))
                    .child("Projects"),
            )
            .children(rows);
        // The project list scrolls, and the agents stay at the bottom, as in the desktop sidebar.
        div()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .child(projects)
            .children(self.plugin_list(cx))
            .children(self.agent_list(cx))
            .child(div().p(px(8.0)).pt_0().child(self.settings_row(cx)))
            .into_any_element()
    }

    fn sidebar_row(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let entry = &self.projects[index];
        let active = entry.path == self.config.project;
        let states = self.project_agents(&entry.path);
        let count = states.len();
        let glyph = match agents::roll_up(states) {
            Some(state) => div()
                .size(px(16.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(crate::ui::status_dot(("project-dot", index), state))
                .into_any_element(),
            None => project_icon(entry.child),
        };
        let path = entry.path.clone();
        div()
            .id(("project", index))
            .role(Role::Button)
            .aria_label(entry.name.clone())
            .aria_selected(active)
            .flex()
            .items_center()
            .gap(px(10.0))
            .py(px(3.0))
            .pr(px(8.0))
            .pl(px(if entry.child { 24.0 } else { 8.0 }))
            .rounded(px(8.0))
            .cursor_pointer()
            .when(active, |row| row.bg(rgba(SIDEBAR_ACCENT)))
            .when(!active, |row| {
                row.hover(|style| style.bg(rgba(theme::HOVER)))
            })
            .on_click(
                cx.listener(move |this, _, window, cx| {
                    this.switch_project(path.clone(), window, cx)
                }),
            )
            .child(glyph)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.0))
                            .line_height(px(18.0))
                            .font_weight(if active {
                                gpui::FontWeight::SEMIBOLD
                            } else {
                                gpui::FontWeight::MEDIUM
                            })
                            .text_color(rgb(SIDEBAR_FOREGROUND))
                            .child(entry.name.clone()),
                    )
                    .when_some(entry.branch.clone(), |text, branch| {
                        text.child(
                            div()
                                .truncate()
                                .text_size(px(11.0))
                                .line_height(px(16.0))
                                .text_color(rgb(theme::MUTED_FOREGROUND))
                                .child(branch),
                        )
                    }),
            )
            .when(count == 0 && entry.pinned, |row| {
                row.child(
                    icon(Icon::Pin, 12.0, theme::MUTED_FOREGROUND)
                        .opacity(0.7)
                        .mx(px(4.0)),
                )
            })
            .when(count > 0, |row| {
                row.child(
                    div()
                        .px(px(4.0))
                        .text_size(px(11.0))
                        .text_color(rgb(theme::MUTED_FOREGROUND))
                        .child(count.to_string()),
                )
            })
    }

    /// The desktop's project bar above the panes: the project's name and its branch.
    pub(super) fn project_bar(&self) -> AnyElement {
        let entry = self.projects.iter().find(|p| p.path == self.config.project);
        let name = entry.map_or_else(
            || self.config.project.to_string_lossy().into_owned(),
            |p| p.name.clone(),
        );
        let branch = entry.and_then(|p| p.branch.clone());
        div()
            .id("project-bar")
            .role(Role::Toolbar)
            .aria_label("Project")
            .h(px(36.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(12.0))
            .pr(px(8.0))
            .border_b_1()
            .border_color(rgba(HAIRLINE))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(name),
            )
            .when_some(branch, |bar, branch| {
                bar.child(icon(Icon::ChevronRight, 12.0, theme::MUTED_FOREGROUND).opacity(0.6))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(branch),
                    )
            })
            .into_any_element()
    }

    pub(super) fn project_key(&self) -> String {
        self.config.project.to_string_lossy().into_owned()
    }

    pub(super) fn load(&mut self, action: Option<plugin::Action>, cx: &mut Context<Self>) {
        if self.in_flight {
            return;
        }
        self.in_flight = true;
        self.issues.update(cx, |view, cx| view.set_loading(cx));
        let config = self.config.clone();
        let load = cx
            .background_executor()
            .spawn(async move { config.load_issues(action) });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |this, cx| {
                this.in_flight = false;
                this.issues.update(cx, |view, cx| match result {
                    Ok(screen) => view.set_screen(screen, cx),
                    Err(error) => view.set_error(error, cx),
                });
            });
        }));
    }
}

/// A folder for a project, a branch for its worktrees, as in the desktop sidebar.
fn project_icon(worktree: bool) -> AnyElement {
    icon(
        if worktree {
            Icon::GitBranch
        } else {
            Icon::Folder
        },
        16.0,
        theme::MUTED_FOREGROUND,
    )
    .into_any_element()
}
