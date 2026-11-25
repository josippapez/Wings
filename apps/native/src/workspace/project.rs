//! The current project: entering it the first time, switching between projects and its bar above the panes.

use crate::workspace::NotifyWorkspace;
use std::path::PathBuf;

use gpui::{AnyElement, Context, Role, Window, div, prelude::*, px, rgb, rgba};

use crate::{
    icons::{Icon, icon},
    saved, theme,
    ui::HAIRLINE,
};

use super::{Parked, Workspace, editor::EditorForm};

/// The desktop's `--sidebar-accent` behind the shown project, and `--sidebar-foreground`.
pub(super) const SIDEBAR_ACCENT: u32 = 0xffffff14;
pub(super) const SIDEBAR_FOREGROUND: u32 = 0xe4e0da;

/// The project bar's height, which the right sidebar's header matches.
pub(super) const PROJECT_BAR: f32 = 36.0;

/// What the project bar keeps, in points, when its column is narrow: its padding, the gap between its parts, the few
/// letters of the name and of the branch, an icon button and the gap between plugin buttons.
const BAR_PADDING: f32 = 24.0;
const BAR_GAP: f32 = 6.0;
const NAME_MIN: f32 = 44.0;
const BRANCH_MIN: f32 = 12.0 + BAR_GAP + 36.0;
const BAR_BUTTON: f32 = 28.0;
const PLUGIN_GAP: f32 = 4.0;
/// What "Open in <editor>" takes besides its editor's name, and a letter of the name.
const EDITOR_LABEL: f32 = 40.0;
const LETTER: f32 = 6.5;
/// What a plugin button with its title takes besides the title's letters: its padding, icon and the gap after it, and
/// room for a count.
const LABELLED_BUTTON: f32 = 20.0 + 16.0 + 6.0 + 26.0;

/// How the project bar fits its column: what it draws, in the order a narrow column takes things away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BarPlan {
    pub(super) editor: EditorForm,
    /// The plugin buttons in the bar; the rest are in its menu.
    pub(super) inline: usize,
    /// The branch after the name.
    pub(super) branch: bool,
    /// The plugin buttons with their titles after the icons.
    pub(super) labels: bool,
}

impl BarPlan {
    /// The least width the bar needs for the parts of this plan, the name and the branch at their shortest.
    fn needs(&self, editor: Option<(usize, bool)>, plugins: &Plugins) -> f32 {
        let mut parts = 2; // the name, the spacer
        let mut width = BAR_PADDING + NAME_MIN;
        if self.branch {
            parts += 2;
            width += BRANCH_MIN - BAR_GAP;
        }
        if let Some((letters, more)) = editor {
            parts += 1;
            width += match self.editor {
                EditorForm::Full => EDITOR_LABEL + LETTER * letters as f32,
                _ => BAR_BUTTON,
            };
            if more && self.editor != EditorForm::Merged {
                width += BAR_BUTTON + 2.0;
            }
        }
        if !plugins.letters.is_empty() {
            parts += 1;
            width += plugins.width(self.inline, self.labels);
        }
        width + (parts - 1) as f32 * BAR_GAP
    }
}

/// The plugin buttons the bar wants: the letters in each one's title, in the order the bar draws them, and whether its
/// menu shows even with all of them in the bar.
pub(super) struct Plugins<'a> {
    pub(super) letters: &'a [usize],
    pub(super) menu: bool,
}

impl Plugins<'_> {
    /// What the first `inline` buttons take, with their titles or as icons, and the menu's button after them when some
    /// are left out of the bar or it shows anyway.
    fn width(&self, inline: usize, labels: bool) -> f32 {
        let menu = self.menu || inline < self.letters.len();
        let buttons = inline + menu as usize;
        let mut width = buttons as f32 * BAR_BUTTON + buttons.saturating_sub(1) as f32 * PLUGIN_GAP;
        if labels {
            let letters: usize = self.letters[..inline].iter().sum();
            width += inline as f32 * (LABELLED_BUTTON - BAR_BUTTON) + LETTER * letters as f32;
        }
        width
    }
}

/// What the bar draws in a column `width` wide, for an `editor` of that many letters, with whether others are installed,
/// and the `plugins` buttons: the buttons show their titles while those fit and take no more than half the bar, then
/// only their icons, then the ones that don't fit go into the bar's menu, then the editor's words go, then its menu of
/// the other editors joins its button, and the branch goes last. All of it when it fits.
pub(super) fn bar_plan(width: f32, editor: Option<(usize, bool)>, plugins: &Plugins) -> BarPlan {
    let most = plugins.letters.len();
    let last = BarPlan { editor: EditorForm::Merged, inline: 0, branch: false, labels: false };
    let mut candidates = Vec::new();
    // Titles only while they take no more than half the bar.
    if most > 0 && plugins.width(most, true) <= width / 2.0 {
        candidates.push(BarPlan { editor: EditorForm::Full, inline: most, branch: true, labels: true });
    }
    for form in [EditorForm::Full, EditorForm::Icon, EditorForm::Merged] {
        for inline in (0..=most).rev() {
            candidates.push(BarPlan { editor: form, inline, branch: true, labels: false });
        }
    }
    candidates.push(last);
    candidates
        .iter()
        .find(|plan| plan.needs(editor, plugins) <= width)
        .copied()
        .unwrap_or(last)
}

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
        if let Some(right) = restored.as_ref().and_then(|project| project.right.as_deref()) {
            self.apply_right(super::right_stack::Stack::from_saved(right), cx);
        }
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
            let right = project.right.as_deref().map(super::right_stack::Stack::from_saved);
            let agents = panes.iter().any(|pane| pane.session.is_some());
            // A page whose program the terminal server kept, like a chat's Claude Code, opens now, so it takes the
            // program over before the server's wait for it runs out. The project's other pages open with it.
            let keeps_jobs = servers.as_ref().is_some_and(|servers| {
                let tabs = servers.0.job_tabs();
                project.plugin_tabs.iter().any(|tab| tab.key.as_ref().is_some_and(|key| tabs.contains(key)))
            });
            let (plugin_tabs, pages) = if keeps_jobs {
                let shown = std::mem::take(&mut self.plugin_tabs);
                let pages = self.restore_plugin_tabs(&project.plugin_tabs, &path, cx);
                (std::mem::replace(&mut self.plugin_tabs, shown), Some(pages))
            } else {
                (Vec::new(), None)
            };
            if !attached && adopted.is_none() && !agents && pages.is_none() {
                continue;
            }
            self.parked.insert(
                path,
                Parked {
                    terminals,
                    plugin_tabs,
                    project_page: None,
                    issues_open: !project.issues_closed,
                    // Decided when it's first shown, once its pages are open.
                    active: super::Active::Issues,
                    right,
                    waiting: Some(super::persist::Waiting {
                        panes,
                        project,
                        adopted,
                        pages,
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
        let seen = self.see_project(&path);
        if self.welcome {
            // The removed project left nothing to keep.
            self.welcome = false;
            self.config.project = path;
            return self.enter_project(window, cx);
        }
        if path == self.config.project {
            if seen {
                self.notify_pieces(&[super::parts::Piece::Sidebar], cx);
            }
            return;
        }
        self.save(cx);
        self.side.touch(&self.config.project);
        self.side.touch(&path);
        self.take_in(path, window, cx);
    }

    /// Puts the project on screen away and brings `path` in as it was left: its parked tabs, or the ones it restores or
    /// opens when it wasn't entered yet.
    fn take_in(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.park_shown();
        self.config.project = path.clone();
        match self.parked.remove(&path) {
            Some(parked) => {
                self.terminals = parked.terminals;
                self.plugin_tabs = parked.plugin_tabs;
                self.project_page = parked.project_page;
                self.issues_open = parked.issues_open;
                if let Some(right) = parked.right {
                    self.apply_right(right, cx);
                }
                match parked.waiting {
                    Some(waiting) => self.start_waiting(waiting, window, cx),
                    None => self.show(parked.active, window, cx),
                }
            }
            None => self.enter_project(window, cx),
        }
    }

    /// Puts the project on screen away with its tabs and shells, for when it comes back.
    fn park_shown(&mut self) {
        let current = Parked {
            terminals: std::mem::take(&mut self.terminals),
            plugin_tabs: std::mem::take(&mut self.plugin_tabs),
            project_page: self.project_page.take(),
            issues_open: self.issues_open,
            active: self.active,
            right: Some(self.right.clone()),
            waiting: None,
        };
        self.parked.insert(self.config.project.clone(), current);
    }

    /// Runs `work` in the project `path` without showing it, for what the user didn't ask to see: the project, tab and
    /// keys on screen and the sidebar's order stay as they are. A project with no tabs to work in yet, one not entered or
    /// restored at launch and not shown since, gets them first.
    pub(super) fn in_project<R>(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> R,
    ) -> R {
        let parked = self
            .parked
            .get(&path)
            .is_some_and(|parked| parked.waiting.is_none());
        let result = if parked && !self.welcome {
            let shown = self.config.project.clone();
            self.unpark(path);
            let result = work(self, window, cx);
            self.unpark(shown);
            result
        } else if self.welcome || path == self.config.project {
            self.switch_project(path, window, cx);
            work(self, window, cx)
        } else {
            // Not entered yet, or restored at launch and not shown since: it gets its tabs now and goes back, without
            // being shown, the sidebar's order or its agents' "seen" changing, and the keys where they were.
            let (shown, right, keys) = (self.config.project.clone(), self.right.clone(), window.focused(cx));
            self.take_in(path, window, cx);
            let result = work(self, window, cx);
            self.unpark(shown);
            self.apply_right(right, cx);
            if let Some(keys) = keys {
                keys.focus(window, cx);
            }
            result
        };
        self.sync_pane_visibility(cx);
        cx.notify_workspace();
        result
    }

    /// Parks the project on screen and takes the parked `path` out as it was left, without showing it.
    fn unpark(&mut self, path: PathBuf) {
        self.park_shown();
        let parked = self.parked.remove(&path).expect("a parked project");
        self.terminals = parked.terminals;
        self.plugin_tabs = parked.plugin_tabs;
        self.project_page = parked.project_page;
        self.issues_open = parked.issues_open;
        self.active = parked.active;
        self.config.project = path;
    }

    /// The desktop's project bar above the panes: the project's name and its branch, and Open in <editor>.
    pub(super) fn project_bar(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let entry = self.projects.iter().find(|p| p.path == self.config.project);
        let name = entry.map_or_else(
            || self.config.project.to_string_lossy().into_owned(),
            |p| p.name.clone(),
        );
        let editor = self
            .default_editor()
            .map(|editor| (editor.name.chars().count(), self.editors.len() > 1));
        let (letters, menu) = self.bar_fit();
        let plan = bar_plan(width, editor, &Plugins { letters: &letters, menu });
        let branch = entry.and_then(|p| p.branch.clone()).filter(|_| plan.branch);
        let buttons = self.sidebar_buttons(plan.inline, plan.labels, cx);
        div()
            .id("project-bar")
            .children(crate::ui::spot(|| "project-bar".into()))
            .role(Role::Toolbar)
            .aria_label("Project")
            .h(px(PROJECT_BAR))
            .flex_none()
            // Kept inside its column, as the desktop's panel keeps it: a narrow one never draws it over the sidebar.
            .min_w_0()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(14.0))
            .pr(px(10.0))
            // The panes start under its line.
            .border_b_1()
            .border_color(rgba(HAIRLINE))
            // A name cut short in a narrow column says itself on hover.
            .child(
                div()
                    .id("project-name")
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .tooltip(crate::ui::tooltip(name.clone(), None))
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
            .children(self.editor_button(plan.editor, cx))
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
