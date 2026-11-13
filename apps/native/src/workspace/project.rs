//! The current project: entering it the first time, switching between projects and its bar above the panes.

use std::path::PathBuf;

use gpui::{AnyElement, Context, Role, Window, div, prelude::*, px, rgb, rgba};

use crate::{
    icons::{Icon, icon},
    saved, theme,
};

use super::{HAIRLINE, Parked, Workspace};

/// The desktop's `--sidebar-accent` behind the shown project, and `--sidebar-foreground`.
pub(super) const SIDEBAR_ACCENT: u32 = 0xffffff14;
pub(super) const SIDEBAR_FOREGROUND: u32 = 0xe4e0da;

impl Workspace {
    /// The current project's saved tabs, or one new terminal.
    pub(super) fn enter_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restored = self
            .config
            .state
            .as_deref()
            .and_then(|path| saved::load(path, &self.project_key()));
        self.issues_open = !restored
            .as_ref()
            .is_some_and(|project| project.issues_closed);
        let just_added = std::mem::take(&mut self.just_added);
        match restored {
            Some(project) if !(project.tabs.is_empty() && project.plugin_tabs.is_empty()) => {
                self.restore(project, window, cx)
            }
            _ if just_added && self.prefs.new_project_page && self.project_info.is_some() => {
                self.open_project_page(self.config.project.clone(), window, cx)
            }
            _ => self.new_terminal(window, cx),
        }
        #[cfg(unix)]
        self.adopt_orphans(window, cx);
        self.open_tasks_board(cx);
    }

    /// The Issues plugin's board, as the Issues tab, when Issues is open and the board isn't yet.
    pub(super) fn open_tasks_board(&mut self, cx: &mut Context<Self>) {
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

    /// At launch, the other projects the terminal server still runs panes of, restored behind the shown one with those
    /// panes attached, so their agents show in the sidebar before you open them, as the desktop restores every
    /// project's tabs at launch. So are those with a saved agent session, which the sidebar lists until it resumes, as
    /// after a restart. Their other panes start, and their pages open, when each is first shown (`start_waiting`); a
    /// project with neither is restored then, as before.
    #[cfg(unix)]
    pub(super) fn restore_hidden(&mut self, cx: &mut Context<Self>) {
        let servers = self.config.terminal_server.clone();
        let state = self.config.state.clone();
        let mut saved = state
            .as_deref()
            .and_then(saved::read_all)
            .map(|(_, projects)| projects)
            .unwrap_or_default();
        let saved_ids = state.as_deref().map(saved::history_ids).unwrap_or_default();
        let hidden: Vec<PathBuf> = self
            .projects
            .iter()
            .map(|project| project.path.clone())
            .filter(|path| *path != self.config.project && !self.parked.contains_key(path))
            .collect();
        for path in hidden {
            let key = path.to_string_lossy().into_owned();
            let project = saved.remove(&key).unwrap_or_default();
            let mut panes = Vec::new();
            let mut terminals: Vec<super::TerminalTab> = project
                .tabs
                .iter()
                .map(|tab| self.restore_tab(tab, Some(&mut panes), cx))
                .collect();
            let attached = terminals
                .iter()
                .flat_map(|tab| tab.layout.panes())
                .any(|pane| self.panes.contains_key(&pane));
            let mut adopted = None;
            let orphans = servers
                .as_ref()
                .map(|servers| servers.0.orphans(&key, &saved_ids));
            for history in orphans.into_iter().flatten() {
                if let Some(pane) = self.attach_hidden(&history, None, None, cx) {
                    let tab = self.pane_tab(pane, cx);
                    adopted = Some(tab.id);
                    terminals.push(tab);
                }
            }
            let agents = panes.iter().any(|pane| pane.session.is_some());
            if !attached && adopted.is_none() && !agents {
                continue;
            }
            self.parked.insert(
                path,
                Parked {
                    terminals,
                    plugin_tabs: Vec::new(),
                    project_page: None,
                    issues_open: !project.issues_closed,
                    // Decided when it's first shown, once its pages are open.
                    active: super::Active::Issues,
                    waiting: Some(super::persist::Waiting {
                        panes,
                        project,
                        adopted,
                    }),
                },
            );
        }
    }

    /// A new tab of the one pane `pane`.
    #[cfg(unix)]
    fn pane_tab(&mut self, pane: usize, cx: &mut Context<Self>) -> super::TerminalTab {
        let id = self.next_tab;
        self.next_tab += 1;
        super::TerminalTab {
            id,
            layout: crate::layout::Layout::Pane(pane),
            focused: pane,
            zoomed: None,
            focus: cx.focus_handle().tab_stop(true),
        }
    }

    /// A tab for each pane of this project the terminal server still runs but no saved pane names, so no program the
    /// server kept goes on unseen.
    #[cfg(unix)]
    fn adopt_orphans(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(servers) = self.config.terminal_server.clone() else {
            return;
        };
        let saved = self
            .config
            .state
            .as_deref()
            .map(saved::history_ids)
            .unwrap_or_default();
        for history in servers.0.orphans(&self.project_key(), &saved) {
            let pane = self.open_pane(None, Some(history), cx);
            let tab = self.pane_tab(pane, cx);
            let id = tab.id;
            self.terminals.push(tab);
            self.show(super::Active::Terminal(id), window, cx);
        }
    }

    pub fn project(&self) -> &std::path::Path {
        &self.config.project
    }

    /// Shows another project. The one shown now keeps its tabs and shells for when it comes back.
    pub fn switch_project(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.welcome {
            // The removed project left nothing to keep.
            self.welcome = false;
            self.config.project = path;
            return self.enter_project(window, cx);
        }
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
            waiting: None,
        };
        self.parked.insert(self.config.project.clone(), current);
        self.config.project = path.clone();
        match self.parked.remove(&path) {
            Some(parked) => {
                self.terminals = parked.terminals;
                self.plugin_tabs = parked.plugin_tabs;
                self.project_page = parked.project_page;
                self.issues_open = parked.issues_open;
                match parked.waiting {
                    Some(waiting) => self.start_waiting(waiting, window, cx),
                    None => self.show(parked.active, window, cx),
                }
            }
            None => self.enter_project(window, cx),
        }
    }

    /// The desktop's project bar above the panes: the project's name and its branch, and Open in <editor>.
    pub(super) fn project_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let entry = self.projects.iter().find(|p| p.path == self.config.project);
        let name = entry.map_or_else(
            || self.config.project.to_string_lossy().into_owned(),
            |p| p.name.clone(),
        );
        let branch = entry.and_then(|p| p.branch.clone());
        let buttons = self.sidebar_buttons(cx);
        div()
            .id("project-bar")
            .role(Role::Toolbar)
            .aria_label("Project")
            .h(px(36.0))
            .flex_none()
            // Kept inside its column, as the desktop's panel keeps it: a narrow one never draws it over the sidebar.
            .min_w_0()
            .overflow_hidden()
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
            .child(div().flex_1())
            .children(self.editor_button(cx))
            // The plugins' sidebar buttons at the right end, as in the desktop's project bar.
            .when(!buttons.is_empty(), |bar| {
                bar.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .children(buttons),
                )
            })
            .into_any_element()
    }

    pub(super) fn project_key(&self) -> String {
        self.config.project.to_string_lossy().into_owned()
    }
}
