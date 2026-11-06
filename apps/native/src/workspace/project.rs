//! The current project: entering it the first time, switching between projects and its bar above the panes.

use std::path::PathBuf;

use gpui::{AnyElement, Context, Role, Window, div, prelude::*, px, rgb, rgba};

use crate::{
    icons::{Icon, icon},
    plugin, saved, theme,
};

use super::{HAIRLINE, Parked, Workspace};

/// The desktop's `--sidebar-accent` behind the shown project, and `--sidebar-foreground`.
pub(super) const SIDEBAR_ACCENT: u32 = 0xffffff14;
pub(super) const SIDEBAR_FOREGROUND: u32 = 0xe4e0da;

impl Workspace {
    /// The current project's saved tabs, or one new terminal, and its Issues.
    pub(super) fn enter_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restored = self
            .config
            .state
            .as_deref()
            .and_then(|path| saved::load(path, &self.project_key()));
        self.issues_open = !restored
            .as_ref()
            .is_some_and(|project| project.issues_closed);
        match restored {
            Some(project) if !(project.tabs.is_empty() && project.plugin_tabs.is_empty()) => {
                self.restore(project, window, cx)
            }
            _ => self.new_terminal(window, cx),
        }
        self.load_issues(cx);
        if self.issues_open
            && self.has_tasks_board()
            && !self
                .plugin_tabs
                .iter()
                .any(|tab| tab.plugin == "tasks" && tab.view == "board")
        {
            self.new_plugin_tab("tasks", "board", None, cx);
        }
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
        self.side.touch(&self.config.project);
        self.side.touch(&path);
        let current = Parked {
            terminals: std::mem::take(&mut self.terminals),
            plugin_tabs: std::mem::take(&mut self.plugin_tabs),
            project_page: self.project_page.take(),
            issues_open: self.issues_open,
            active: self.active,
        };
        self.parked.insert(self.config.project.clone(), current);
        self.config.project = path.clone();
        match self.parked.remove(&path) {
            Some(parked) => {
                self.terminals = parked.terminals;
                self.plugin_tabs = parked.plugin_tabs;
                self.project_page = parked.project_page;
                self.issues_open = parked.issues_open;
                self.show(parked.active, window, cx);
                self.load_issues(cx);
            }
            None => self.enter_project(window, cx),
        }
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
