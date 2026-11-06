//! Saving and restoring the project's tabs, splits and pane histories.

use gpui::{App, Context, Window};

use crate::{
    layout::{Dir, Layout},
    saved::{self, SavedDir, SavedLayout, SavedPluginTab, SavedProject, SavedTab},
    terminal_view::{CURSOR_STYLES, DEFAULT_FONT_FAMILY, font_size, look},
};

use super::{Active, TerminalTab, Workspace};

impl Workspace {
    /// Each restored pane starts a new shell in the folder its shell was last in.
    pub(super) fn restore(
        &mut self,
        project: SavedProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for tab in &project.tabs {
            let layout = self.restore_layout(&tab.layout, cx);
            let panes = layout.panes();
            let focused = panes[tab.focused.min(panes.len() - 1)];
            let zoomed = tab.zoomed.and_then(|index| panes.get(index).copied());
            let id = self.next_tab;
            self.next_tab += 1;
            self.terminals.push(TerminalTab {
                id,
                layout,
                focused,
                zoomed,
                focus: cx.focus_handle().tab_stop(true),
            });
        }
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

    fn restore_layout(&mut self, saved: &SavedLayout, cx: &mut Context<Self>) -> Layout {
        match saved {
            SavedLayout::Pane { cwd, history, name } => {
                let pane = self.open_pane(cwd.clone(), history.clone(), cx);
                if let Some(saved) = self.panes.get_mut(&pane) {
                    saved.name = name.clone().filter(|name| !name.is_empty()).map(Into::into);
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
                    a: Box::new(self.restore_layout(a, cx)),
                    b: Box::new(self.restore_layout(b, cx)),
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
                cwd: pane
                    .and_then(|pane| pane.view.as_ref().ok())
                    .and_then(|view| view.read(cx).cwd()),
                history: pane.map(|pane| pane.history.clone()),
                name: pane.and_then(|pane| pane.name.as_ref().map(ToString::to_string)),
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
            cursor_style: CURSOR_STYLES
                .iter()
                .find(|(shape, ..)| *shape == look.cursor)
                .map(|(_, name, _)| name.to_string()),
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
