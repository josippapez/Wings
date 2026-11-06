//! The project sidebar: its width you drag, the order and "Older" group of its projects, the git counts on its rows,
//! adding, removing and pinning projects, and the peek of a closed sidebar. The window draws it from `view`.

mod git;
mod order;
mod peek;
#[cfg(all(test, feature = "test-support"))]
mod tests;
mod view;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use gpui::{Context, FocusHandle, PathPromptOptions, Pixels, Window};

use crate::{projects::ProjectEntry, saved};

use super::Workspace;
use order::{Sort, TOUCH_EVERY_MS};

/// The desktop's `SIDES.left`: how narrow and how wide the sidebar can be dragged, and where it starts.
const MIN_WIDTH: f32 = 200.0;
const MAX_WIDTH: f32 = 420.0;
const INITIAL_WIDTH: f32 = 248.0;

/// A drag of the sidebar's edge: where the pointer was, and how wide the sidebar was, when it started.
struct Resize {
    from_x: Pixels,
    from_width: f32,
}

pub(super) struct State {
    pub(super) width: f32,
    sort: Sort,
    older_open: bool,
    /// When each project was last worked on, in ms, by folder.
    last_active: BTreeMap<String, u64>,
    resize: Option<Resize>,
    /// The row the pointer is on, which shows its buttons.
    hovered: Option<usize>,
    /// The sort menu is open.
    sort_menu: bool,
    /// The project asked to be removed while it has tabs open, with the focus of the question.
    removing: Option<(PathBuf, FocusHandle)>,
    git: git::Git,
    peek: peek::Peek,
}

impl Default for State {
    fn default() -> Self {
        Self {
            width: INITIAL_WIDTH,
            sort: Sort::default(),
            older_open: false,
            last_active: BTreeMap::new(),
            resize: None,
            hovered: None,
            sort_menu: false,
            removing: None,
            git: git::Git::default(),
            peek: peek::Peek::default(),
        }
    }
}

impl State {
    /// A refresh of the git counts that you asked for is running.
    pub(in crate::workspace) fn checking_git(&self) -> bool {
        self.git.checking
    }

    /// What the workspace file kept. A width outside the limits starts again from the initial one, as the desktop does.
    fn load(settings: &saved::Settings, projects: &[ProjectEntry]) -> Self {
        let width = settings.sidebar_width.unwrap_or(INITIAL_WIDTH);
        let mut state = Self {
            width: if (MIN_WIDTH..=MAX_WIDTH).contains(&width) {
                width
            } else {
                INITIAL_WIDTH
            },
            sort: Sort::parse(settings.project_sort.as_deref()),
            older_open: settings.older_open.unwrap_or(false),
            last_active: settings.last_active.clone(),
            ..Self::default()
        };
        state.forget_removed(projects);
        state
    }

    /// The part of the workspace file this owns.
    pub(super) fn saved(&self) -> saved::Settings {
        saved::Settings {
            sidebar_width: Some(self.width),
            project_sort: Some(self.sort.key().to_owned()),
            older_open: Some(self.older_open),
            last_active: self.last_active.clone(),
            ..saved::Settings::default()
        }
    }

    /// A project with no time yet counts as worked on now, so none goes under "Older" the first time.
    fn stamp_missing(&mut self, projects: &[ProjectEntry]) {
        let now = now_ms();
        for project in projects {
            self.last_active
                .entry(project.path.to_string_lossy().into_owned())
                .or_insert(now);
        }
    }

    /// Times of projects no longer listed are dropped.
    fn forget_removed(&mut self, projects: &[ProjectEntry]) {
        self.last_active.retain(|folder, _| {
            projects
                .iter()
                .any(|project| project.path.as_os_str() == folder.as_str())
        });
    }

    /// `project` was worked on now, at most once a minute.
    pub(super) fn touch(&mut self, project: &Path) {
        let now = now_ms();
        let last = self
            .last_active
            .entry(project.to_string_lossy().into_owned())
            .or_insert(0);
        if now.saturating_sub(*last) >= TOUCH_EVERY_MS {
            *last = now;
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

impl Workspace {
    /// Reads the saved state and starts keeping the git counts. Called once the window has its projects.
    pub(super) fn start_sidebar(
        &mut self,
        settings: &saved::Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.side = State::load(settings, &self.projects);
        self.side.stamp_missing(&self.projects);
        self.side.touch(&self.config.project);
        self.start_git(window, cx);
    }

    pub(super) fn toggle_docked(&mut self, cx: &mut Context<Self>) {
        self.sidebar = !self.sidebar;
        self.side.peek = peek::Peek::default();
        self.save(cx);
        cx.notify();
    }

    /// The edge was grabbed with the pointer at `x`.
    fn start_resize(&mut self, x: Pixels, cx: &mut Context<Self>) {
        self.side.resize = Some(Resize {
            from_x: x,
            from_width: self.side.width,
        });
        cx.notify();
    }

    /// Follows the pointer within the limits. Nothing is saved until the pointer is released: the terminals refit
    /// when the panes settle (`RESIZE_DELAY`), and the file is written once.
    fn resize_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(resize) = &self.side.resize else {
            return;
        };
        let width = (resize.from_width + (x - resize.from_x).as_f32()).clamp(MIN_WIDTH, MAX_WIDTH);
        if width != self.side.width {
            self.side.width = width;
            cx.notify();
        }
    }

    fn end_resize(&mut self, cx: &mut Context<Self>) {
        if self.side.resize.take().is_some() {
            self.save(cx);
            cx.notify();
        }
    }

    /// Re-reads the projects after they changed, from here or from a plugin.
    pub(super) fn projects_changed(&mut self, cx: &mut Context<Self>) {
        self.projects = self.spaces.entries();
        self.side.stamp_missing(&self.projects);
        self.side.forget_removed(&self.projects);
        let projects = &self.projects;
        self.side
            .git
            .status
            .retain(|folder, _| projects.iter().any(|project| &project.path == folder));
        self.watch_projects_again();
        self.refresh_git(None, false, cx);
        cx.notify();
    }

    fn set_sort(&mut self, sort: Sort, cx: &mut Context<Self>) {
        self.side.sort = sort;
        self.side.sort_menu = false;
        self.save(cx);
        cx.notify();
    }

    fn toggle_older(&mut self, cx: &mut Context<Self>) {
        self.side.older_open = !self.side.older_open;
        self.save(cx);
        cx.notify();
    }

    pub(in crate::workspace) fn pin_project(
        &mut self,
        project: &Path,
        pinned: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(folder) = project.to_str() {
            self.spaces.set_pinned(folder, pinned);
        }
        self.projects = self.spaces.entries();
        cx.notify();
    }

    /// Asks for a folder, as the desktop's "Add project" does, and adds it.
    pub(in crate::workspace) fn choose_project(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(folders))) = chosen.await else {
                return;
            };
            for folder in folders {
                let _ =
                    this.update_in(cx, |this, window, cx| this.add_project(&folder, window, cx));
            }
        })
        .detach();
    }

    /// Lists the folder and shows it.
    fn add_project(&mut self, folder: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = folder.to_str() else {
            eprintln!("Couldn't add {}: its path isn't UTF-8", folder.display());
            return;
        };
        let listed = self.projects.iter().any(|project| project.path == folder);
        let space = self.spaces.add(text);
        self.projects_changed(cx);
        self.just_added = !listed;
        self.switch_project(PathBuf::from(space.path), window, cx);
        self.just_added = false;
    }

    /// Tabs the project has open, shells included.
    fn open_tabs(&self, project: &Path) -> usize {
        if project == self.config.project {
            self.terminals.len() + self.plugin_tabs.len()
        } else {
            self.parked.get(project).map_or(0, |parked| {
                parked.terminals.len() + parked.plugin_tabs.len()
            })
        }
    }

    /// Removing closes the project's tabs, running agents included, so a project with tabs open asks first.
    pub(in crate::workspace) fn request_remove(
        &mut self,
        project: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open_tabs(&project) == 0 {
            return self.remove_project(project, window, cx);
        }
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.side.removing = Some((project, focus));
        cx.notify();
    }

    fn cancel_remove(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.side.removing = None;
        if let Some(pane) = self.focused_pane() {
            self.focus_pane(pane, window, cx);
        }
        cx.notify();
    }

    /// Takes the project off the list and closes its tabs. The project on screen gives way to the first other one; with
    /// none left, the window shows the welcome, as the desktop does with no project.
    fn remove_project(&mut self, project: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.side.removing = None;
        if project == self.config.project {
            match self.projects.iter().find(|other| other.path != project) {
                Some(next) => self.switch_project(next.path.clone(), window, cx),
                None => self.show_welcome(cx),
            }
        }
        let closed = self.parked.remove(&project).map(|parked| parked.terminals);
        for tab in closed.into_iter().flatten() {
            for pane in tab.layout.panes() {
                self.panes.remove(&pane);
            }
        }
        if let Some(folder) = project.to_str() {
            self.spaces.remove(folder);
        }
        self.projects_changed(cx);
    }
}
