//! Saving and restoring the project's tabs, splits and pane histories.

use gpui::{App, Context, Window};

use crate::{
    layout::{Dir, Layout},
    saved::{self, SavedDir, SavedLayout, SavedPluginTab, SavedProject, SavedTab},
    terminal_view::{
        CURSOR_STYLES, DEFAULT_FONT_FAMILY, INACTIVE_CURSORS, OPTION_AS_META, font_size, look,
    },
};

use super::{Active, TerminalTab, Workspace};

/// What a project restored at launch behind the shown one (`restore_hidden`) still restores when it's first shown: the
/// panes the terminal server wasn't running, each under the id its tab holds for it, and the project's pages.
pub(super) struct Waiting {
    pub(super) panes: Vec<WaitingPane>,
    pub(super) project: SavedProject,
    /// The tab of the last pane adopted from the server, which `enter_project` would have left on screen.
    pub(super) adopted: Option<usize>,
}

pub(super) struct WaitingPane {
    pub(super) id: usize,
    cwd: Option<std::path::PathBuf>,
    history: Option<String>,
    pub(super) name: Option<gpui::SharedString>,
    claude_account: Option<wings_spaces::claude_accounts::AccountPick>,
    pub(super) session: Option<saved::SavedSession>,
}

impl Workspace {
    /// Each restored pane starts a new shell in the folder its shell was last in.
    pub(super) fn restore(
        &mut self,
        project: SavedProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for tab in &project.tabs {
            let tab = self.restore_tab(tab, None, cx);
            self.terminals.push(tab);
        }
        self.restore_pages(&project, window, cx);
    }

    /// Shows a project restored at launch behind another (`restore_hidden`) for the first time: the panes the server
    /// wasn't running start, and its pages open, as entering it would have restored them.
    pub(super) fn start_waiting(
        &mut self,
        waiting: Waiting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for pane in waiting.panes {
            self.open_pane_at(
                pane.id,
                pane.cwd,
                pane.history,
                pane.claude_account,
                pane.session,
                cx,
            );
            if let Some(saved) = self.panes.get_mut(&pane.id) {
                saved.name = pane.name;
            }
        }
        let project = &waiting.project;
        if project.tabs.is_empty() && project.plugin_tabs.is_empty() {
            self.new_terminal(window, cx);
        } else {
            self.restore_pages(project, window, cx);
        }
        if let Some(tab) = waiting.adopted {
            self.show(Active::Terminal(tab), window, cx);
        }
        self.open_tasks_board(cx);
    }

    /// A saved tab. With `waiting`, as for a project not on screen, a pane only attaches to the program the terminal
    /// server runs for it, and the others wait there for the project to be shown; without, each starts now.
    pub(super) fn restore_tab(
        &mut self,
        tab: &SavedTab,
        mut waiting: Option<&mut Vec<WaitingPane>>,
        cx: &mut Context<Self>,
    ) -> TerminalTab {
        let layout = self.restore_layout(&tab.layout, &mut waiting, cx);
        let panes = layout.panes();
        let focused = panes[tab.focused.min(panes.len() - 1)];
        let zoomed = tab.zoomed.and_then(|index| panes.get(index).copied());
        let id = self.next_tab;
        self.next_tab += 1;
        TerminalTab {
            id,
            layout,
            focused,
            zoomed,
            focus: cx.focus_handle().tab_stop(true),
        }
    }

    /// The project's plugin pages, then the tab it last showed.
    fn restore_pages(
        &mut self,
        project: &SavedProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let restored = self.restore_plugin_tabs(&project.plugin_tabs, cx);
        let terminal = project
            .active
            .and_then(|index| self.terminals.get(index))
            .map(|tab| Active::Terminal(tab.id));
        let plugin = project
            .active_plugin
            .and_then(|index| restored.get(index).copied().flatten())
            .map(Active::Plugin);
        match terminal.or(plugin) {
            Some(active) => self.show(active, window, cx),
            None if self.issues_open => self.show(Active::Issues, window, cx),
            None => self.new_terminal(window, cx),
        }
    }

    fn restore_layout(
        &mut self,
        saved: &SavedLayout,
        waiting: &mut Option<&mut Vec<WaitingPane>>,
        cx: &mut Context<Self>,
    ) -> Layout {
        match saved {
            SavedLayout::Pane {
                cwd,
                history,
                name,
                claude_account,
                session,
            } => {
                let name: Option<gpui::SharedString> =
                    name.clone().filter(|name| !name.is_empty()).map(Into::into);
                let pane = match waiting {
                    None => {
                        let id = self.next_pane;
                        self.next_pane += 1;
                        self.open_pane_at(
                            id,
                            cwd.clone(),
                            history.clone(),
                            claude_account.clone(),
                            session.clone(),
                            cx,
                        );
                        id
                    }
                    Some(waiting) => {
                        #[cfg(unix)]
                        let attached = history.as_deref().and_then(|history| {
                            self.attach_hidden(history, claude_account.clone(), session.clone(), cx)
                        });
                        #[cfg(not(unix))]
                        let attached = None;
                        attached.unwrap_or_else(|| {
                            let id = self.next_pane;
                            self.next_pane += 1;
                            waiting.push(WaitingPane {
                                id,
                                cwd: cwd.clone(),
                                history: history.clone(),
                                name: name.clone(),
                                claude_account: claude_account.clone(),
                                session: session.clone(),
                            });
                            id
                        })
                    }
                };
                if let Some(saved) = self.panes.get_mut(&pane) {
                    saved.name = name;
                }
                Layout::Pane(pane)
            }
            SavedLayout::Split { dir, ratio, a, b } => {
                let mut layout = Layout::Split {
                    dir: match dir {
                        SavedDir::Row => Dir::Row,
                        SavedDir::Column => Dir::Column,
                    },
                    ratio: 0.5,
                    a: Box::new(self.restore_layout(a, waiting, cx)),
                    b: Box::new(self.restore_layout(b, waiting, cx)),
                };
                // Through the same limits as a drag, in case the file was edited by hand.
                layout.set_ratio(&[], *ratio);
                layout
            }
        }
    }

    /// Saves after every change to tabs, splits or focus. The file is small, so this stays on the UI thread.
    pub(super) fn save(&self, cx: &App) {
        let Some(path) = &self.config.state else {
            return;
        };
        let pane = |pane: &usize| {
            let pane = self.panes.get(pane);
            SavedLayout::Pane {
                cwd: pane.and_then(|pane| match &pane.view {
                    Ok(view) => view.read(cx).cwd(),
                    // Waiting for its drive, it keeps the folder it starts in.
                    Err(_) => pane.mount.as_ref()?.cwd.clone(),
                }),
                history: pane.map(|pane| pane.history.clone()),
                name: pane.and_then(|pane| pane.name.as_ref().map(ToString::to_string)),
                claude_account: pane.and_then(|pane| pane.claude_account.clone()),
                session: pane.and_then(|pane| pane.session.clone()),
            }
        };
        let layout = |node: &Layout| saved_layout(node, &pane);
        fn saved_layout(node: &Layout, pane: &dyn Fn(&usize) -> SavedLayout) -> SavedLayout {
            match node {
                Layout::Pane(id) => pane(id),
                Layout::Split { dir, ratio, a, b } => SavedLayout::Split {
                    dir: match dir {
                        Dir::Row => SavedDir::Row,
                        Dir::Column => SavedDir::Column,
                    },
                    ratio: *ratio,
                    a: Box::new(saved_layout(a, pane)),
                    b: Box::new(saved_layout(b, pane)),
                },
            }
        }
        let project = SavedProject {
            tabs: self
                .terminals
                .iter()
                .map(|tab| {
                    let panes = tab.layout.panes();
                    let at = |pane: usize| panes.iter().position(|id| *id == pane);
                    SavedTab {
                        layout: layout(&tab.layout),
                        focused: at(tab.focused).unwrap_or(0),
                        zoomed: tab.zoomed.and_then(at),
                    }
                })
                .collect(),
            active: self
                .terminals
                .iter()
                .position(|tab| self.active == Active::Terminal(tab.id)),
            plugin_tabs: self
                .plugin_tabs
                .iter()
                .map(|tab| SavedPluginTab {
                    plugin: tab.plugin.clone(),
                    view: tab.view.clone(),
                    args: tab.args.clone(),
                    state: Some(tab.state.value()).filter(|value| !value.is_null()),
                })
                .collect(),
            active_plugin: self
                .plugin_tabs
                .iter()
                .position(|tab| self.active == Active::Plugin(tab.id)),
            issues_closed: !self.issues_open,
        };
        let look = look(cx);
        let settings = saved::Settings {
            font_size: Some(font_size(cx)),
            sidebar_open: Some(self.sidebar),
            font_family: (look.family != DEFAULT_FONT_FAMILY).then(|| look.family.to_string()),
            shift_return_meta: Some(look.shift_return_meta),
            option_as_meta: OPTION_AS_META
                .iter()
                .find(|(choice, ..)| *choice == look.option_as_meta)
                .map(|(_, name, _)| name.to_string()),
            cursor_style: CURSOR_STYLES
                .iter()
                .find(|(shape, ..)| *shape == look.cursor)
                .map(|(_, name, _)| name.to_string()),
            cursor_blink: Some(look.cursor_blink),
            cursor_inactive_style: INACTIVE_CURSORS
                .iter()
                .find(|(style, ..)| *style == look.inactive_cursor)
                .map(|(_, name, _)| name.to_string()),
            line_height: Some(look.line_height),
            minimum_contrast_ratio: Some(look.min_contrast),
            scrollback: Some(look.scrollback),
            copy_on_select: Some(look.copy_on_select),
            clipboard_write: Some(look.clipboard_write),
            images: Some(look.images),
            quick_terminal: Some(self.prefs.quick_terminal),
            editor: self.prefs.editor.clone(),
            new_tab: Some(self.prefs.new_tab.clone()),
            new_project_opens: Some(
                if self.prefs.new_project_page {
                    "project"
                } else {
                    "terminal"
                }
                .into(),
            ),
            agents_run_commands: Some(self.prefs.agents_run_commands),
            agents_message: Some(self.prefs.agents_message),
            agent_terminals_open: Some(self.prefs.agent_terminals_open.name().into()),
            notify_sound: Some(self.prefs.notify_sound.to_saved()),
            seen_plugins: self.prefs.seen_plugins.clone(),
            diff_wrap: Some(self.prefs.diff_wrap),
            diff_tree: Some(self.prefs.diff_tree),
            diff_dock_files: Some(self.prefs.diff_dock_files),
            diff_width: Some(self.plugin_ui.diff_dock.width),
            pinned_buttons: Some(self.plugin_ui.pinned.clone()),
            desktop_order: Some(true),
            global_hotkey: Some(self.prefs.global_hotkey.clone()),
            terminal_server: self.prefs.terminal_server,
            right_width: Some(self.plugin_ui.right_size.width),
            ..self.side.saved()
        };
        if let Err(error) = saved::save(path, &self.project_key(), project, settings) {
            eprintln!("Couldn't save the workspace to {}: {error}", path.display());
        }
    }

    /// On quit, like the desktop app: each pane's latest output, for the same pane next launch.
    pub(super) fn save_histories(&self, cx: &App) {
        let Some(state) = &self.config.state else {
            return;
        };
        for pane in self.panes.values() {
            if let Ok(view) = &pane.view
                && let Err(error) =
                    saved::save_history(state, &pane.history, &view.read(cx).recent_output())
            {
                eprintln!("Couldn't save a pane's output: {error}");
            }
        }
        saved::prune_histories(state);
    }
}
