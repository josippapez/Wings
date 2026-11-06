//! The git counts on the project rows: changed files and commits to push and pull, from `git status` in each project.
//! Like the desktop app, they are looked up when the window starts and gets focus, a minute apart while it has focus,
//! and as soon as git moves a project's HEAD, index or refs (`wings_gitwatch`). Edits in the working tree are picked up
//! by the minute sweep. All of it runs off the UI thread.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Duration,
};

use gpui::{Context, Subscription, Task, Window};
use wings_gitwatch::Watcher;
use wings_spaces::spaces::{self, GitStatus};

use crate::workspace::Workspace;

/// A sweep of 11 repos costs about 200 ms of CPU in the desktop app, so it runs once a minute.
const SWEEP_EVERY: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(super) struct Git {
    /// By project folder. A folder outside a repo has none.
    pub(super) status: HashMap<PathBuf, GitStatus>,
    /// A refresh asked for with the button is running.
    pub(super) checking: bool,
    /// The projects waiting for the sweep that is running, or for the next one.
    queue: HashSet<PathBuf>,
    sweeping: bool,
    watcher: Option<Watcher>,
    _tasks: Vec<Task<()>>,
    _activation: Option<Subscription>,
}

impl Workspace {
    /// Starts keeping the counts and looks them up now.
    pub(super) fn start_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (changed, events) = async_channel::unbounded();
        let spaces = self.spaces.clone();
        self.side.git.watcher = Some(Watcher::start(
            move || {
                spaces
                    .paths()
                    .into_iter()
                    .map(|path| (path.to_string_lossy().into_owned(), path))
                    .collect()
            },
            move |ids| {
                let _ = changed.try_send(ids);
            },
        ));
        let moved = cx.spawn(async move |this, cx| {
            while let Ok(ids) = events.recv().await {
                let paths = ids.into_iter().map(PathBuf::from).collect();
                if this
                    .update(cx, |this, cx| this.refresh_git(Some(paths), false, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SWEEP_EVERY).await;
                let swept = this.update(cx, |this, cx| {
                    if this.window_active {
                        this.refresh_git(None, false, cx);
                    }
                });
                if swept.is_err() {
                    break;
                }
            }
        });
        self.side.git._tasks = vec![moved, poll];
        self.side.git._activation =
            Some(cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.refresh_git(None, false, cx);
                }
            }));
        self.refresh_git(None, false, cx);
    }

    /// Tells the watcher the list of projects changed.
    pub(super) fn watch_projects_again(&self) {
        if let Some(watcher) = &self.side.git.watcher {
            watcher.resync();
        }
    }

    /// Looks up `only` those projects, or all of them, off the UI thread. `manual` is the Refresh button, which shows
    /// that it is checking until the queue is empty.
    pub(in crate::workspace) fn refresh_git(
        &mut self,
        only: Option<Vec<PathBuf>>,
        manual: bool,
        cx: &mut Context<Self>,
    ) {
        let git = &mut self.side.git;
        match only {
            Some(paths) => git.queue.extend(paths),
            None => git
                .queue
                .extend(self.projects.iter().map(|project| project.path.clone())),
        }
        git.checking |= manual;
        if !git.sweeping {
            self.sweep_git(cx);
        }
        if manual {
            cx.notify();
        }
    }

    fn sweep_git(&mut self, cx: &mut Context<Self>) {
        let git = &mut self.side.git;
        let paths: Vec<PathBuf> = git.queue.drain().collect();
        if paths.is_empty() {
            git.checking = false;
            return;
        }
        git.sweeping = true;
        let swept = cx.background_executor().spawn(async move {
            let git = wings_plugins::exec::find_program("git");
            paths
                .into_iter()
                .map(|path| {
                    let status = git.as_ref().and_then(|git| spaces::git_status(git, &path));
                    (path, status)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let swept = swept.await;
            let _ = this.update(cx, |this, cx| {
                let git = &mut this.side.git;
                let mut changed = git.checking;
                for (path, status) in swept {
                    let before = match &status {
                        Some(status) => git.status.insert(path, status.clone()),
                        None => git.status.remove(&path),
                    };
                    changed |= before != status;
                }
                git.sweeping = false;
                this.sweep_git(cx);
                if changed {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
