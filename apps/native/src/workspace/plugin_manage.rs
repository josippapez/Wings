//! Installing, approving, turning on and off, updating and removing plugins, as the desktop's Plugins section and its
//! `wings plugin` CLI do (`apps/desktop/src-tauri/src/lib.rs`, `control.rs`): through `wings-plugins`' `Store`, which
//! writes `plugins.json` and the plugin folders exactly as the desktop does. Every check is the store's: a new plugin
//! starts off and unapproved, a file always asks again, and an update that asks for more access stops the plugin until
//! it is approved again. A change starts or stops the plugin's main and pages right away.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui::{Context, ExternalPaths, PathPromptOptions, Task, Window};
use wings_plugins::store::{
    self, Available, Grant, Linked, PluginView, SecretScope, Source, Store,
};

use crate::{installed, markdown};

use super::{Active, Workspace, settings::Section};

/// What the Plugins section is doing, kept by the window so a download that ends after Settings closed still lands.
#[derive(Default)]
pub(super) struct Manage {
    /// `link` or `file` while that install runs.
    pub(super) installing: Option<&'static str>,
    pub(super) error: Option<String>,
    /// Plugins being turned on, updated or removed.
    pub(super) busy: HashSet<String>,
    pub(super) row_error: HashMap<String, String>,
    /// Plugins waiting for you to review their access; the dialog shows the first and steps through the rest.
    pub(super) reviews: Vec<PluginView>,
    /// The plugin whose Remove was pressed and waits for a second click.
    pub(super) removing: Option<String>,
    /// The plugin whose page is open in the list.
    pub(super) selected: Option<String>,
    pub(super) browsing_available: bool,
    /// Newer versions the last check found, by plugin id.
    pub(super) updates: HashMap<String, String>,
    /// The official catalogue from the most recent successful check.
    pub(super) available: Vec<Available>,
    pub(super) checking: bool,
    /// A periodic check elapsed while the window was inactive.
    pub(super) check_due: bool,
    /// Version announcements are once per plugin/version for this workspace.
    pub(super) announced: HashSet<String>,
    /// One persistent notice each for installed updates and newly available plugins.
    pub(super) announcements: Vec<Announcement>,
    plugin_timer: Option<Task<()>>,
    plugin_check: Option<Task<()>>,
    pub(super) update_source: Option<Arc<dyn UpdateSource>>,
    /// The review shown has been up long enough for Turn on to count, as the setup approval's Allow.
    pub(super) review_armed: bool,
    review_arm: Option<Task<()>>,
    /// The README the selected plugin's page shows, read once per installed version.
    readme: RefCell<Option<Readme>>,
}

struct Readme {
    id: String,
    version: String,
    blocks: Option<Rc<Vec<markdown::Block>>>,
}

/// The desktop's `README_MAX_BYTES`: a bigger README isn't shown.
const README_MAX_BYTES: u64 = 256 * 1024;

/// `README.md` (any case) from a plugin folder, if it's there, inside the folder and not over 256 KB, as the desktop's
/// `read_readme`.
fn read_readme(dir: &Path) -> Option<String> {
    use std::io::Read;
    let name = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("readme.md"))
        })?
        .file_name();
    // A symlink out of the folder is refused.
    let path = wings_plugins::loading::resolve(dir, name.to_str()?)?;
    let file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > README_MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(README_MAX_BYTES).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// A dropped path the desktop installs: a `.wings-plugin` file, by its name.
fn is_package(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|path| path.ends_with(".wings-plugin"))
}

/// Whether a drag over the window carries a plugin package, for the drop highlight.
pub(super) fn carries_package(paths: &ExternalPaths) -> bool {
    paths.paths().iter().any(|path| is_package(path))
}

pub(super) trait UpdateSource: Send + Sync {
    fn latest(&self, repo: &str, folder: Option<&str>) -> Result<Option<String>, String>;
    fn available(&self) -> Result<Vec<Available>, String>;
}

struct GithubUpdates;

impl UpdateSource for GithubUpdates {
    fn latest(&self, repo: &str, folder: Option<&str>) -> Result<Option<String>, String> {
        store::latest_version(repo, folder)
    }

    fn available(&self) -> Result<Vec<Available>, String> {
        store::available()
    }
}

pub(super) struct Announcement {
    pub(super) title: String,
    pub(super) body: String,
    pub(super) review: Review,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Review {
    Installed(String),
    Available,
}

impl Review {
    pub(super) fn slot(&self) -> usize {
        match self {
            Self::Installed(_) => 0,
            Self::Available => 1,
        }
    }
}

impl Manage {
    fn announce(&mut self, notice: Announcement) {
        self.announcements
            .retain(|old| old.review.slot() != notice.review.slot());
        self.announcements.push(notice);
    }
}

/// How long a review shows before Turn on answers, so a click meant for one can't land on the next.
const REVIEW_ARM_AFTER: std::time::Duration = std::time::Duration::from_secs(1);
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

impl Workspace {
    /// Queues plugins for review, each once, as the desktop's `enqueue`. A review that comes on screen arms after its
    /// moment.
    pub(super) fn queue_reviews(
        &mut self,
        plugins: impl IntoIterator<Item = PluginView>,
        cx: &mut Context<Self>,
    ) {
        let shown = self
            .manage
            .reviews
            .first()
            .map(|view| view.manifest.id.clone());
        for plugin in plugins {
            if !self
                .manage
                .reviews
                .iter()
                .any(|queued| queued.manifest.id == plugin.manifest.id)
            {
                self.manage.reviews.push(plugin);
            }
        }
        if shown.is_none() && !self.manage.reviews.is_empty() {
            self.arm_review(cx);
        }
    }

    /// Turn on waits a moment each time the plugin shown changes.
    fn arm_review(&mut self, cx: &mut Context<Self>) {
        self.manage.review_armed = false;
        self.manage.review_arm = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REVIEW_ARM_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                this.manage.review_armed = true;
                cx.notify();
            });
        }));
    }

    /// The review shown is done: the next one, if any, arms after its moment.
    fn next_review(&mut self, cx: &mut Context<Self>) {
        let done = (!self.manage.reviews.is_empty()).then(|| self.manage.reviews.remove(0));
        if !self.manage.reviews.is_empty() {
            self.arm_review(cx);
        } else if let Some(done) = done {
            self.leave("review", done, crate::ui::motion::POPUP_IN, cx);
        }
        cx.notify();
    }

    fn store(&self) -> Result<Arc<Mutex<Store>>, String> {
        self.plugin_env
            .store
            .clone()
            .ok_or_else(|| "This Wings has no plugin folder".into())
    }

    /// The installed plugins, as the Plugins section lists them.
    pub(super) fn plugin_views(&self) -> Vec<PluginView> {
        self.plugin_env
            .store
            .as_ref()
            .map(|store| store.lock().unwrap().list())
            .unwrap_or_default()
    }

    /// Whether the plugin's main runs in this window.
    pub(super) fn plugin_runs(&self, id: &str) -> bool {
        self.plugins.iter().any(|installed| installed.id == id)
    }

    /// Installs a package, deletes what a plugin it replaced from somewhere else kept, and starts it again if it may run.
    pub(super) fn install_package(
        &mut self,
        bytes: &[u8],
        source: Source,
        expected_id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<PluginView, String> {
        let (view, dropped) = self
            .store()?
            .lock()
            .unwrap()
            .install(bytes, source, expected_id)?;
        if let Some(old) = dropped {
            self.forget_plugin_data(&view.manifest.id, &old);
        }
        self.restart_plugin(&view.manifest.id, window, cx);
        Ok(view)
    }

    /// Runs a plugin folder where it is, or reads one that's linked again, which restarts it.
    pub(super) fn link_plugin(
        &mut self,
        folder: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Linked, String> {
        let linked = self.store()?.lock().unwrap().link(folder)?;
        self.restart_plugin(&linked.view.manifest.id, window, cx);
        Ok(linked)
    }

    /// Turning a plugin on approves `shown`, the access the dialog showed, if it is still what the manifest asks for.
    pub(super) fn set_plugin_enabled(
        &mut self,
        id: &str,
        enabled: bool,
        shown: Option<Grant>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<PluginView, String> {
        let view = self
            .store()?
            .lock()
            .unwrap()
            .set_enabled(id, enabled, shown)?;
        self.restart_plugin(id, window, cx);
        Ok(view)
    }

    pub(super) fn remove_plugin(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let old = self.store()?.lock().unwrap().remove(id)?;
        if let Some(old) = old {
            self.forget_plugin_data(id, &old);
        }
        self.manage.updates.remove(id);
        self.restart_plugin(id, window, cx);
        Ok(())
    }

    /// Deletes the secrets and storage of a scope nothing can reach any more. A failure only leaves an orphan.
    pub(super) fn forget_plugin_data(&self, id: &str, old: &SecretScope) {
        for name in &old.names {
            if let Err(error) = crate::secrets::delete(id, &old.scope, name) {
                eprintln!("[plugins] couldn't delete {id} secret {name}: {error}");
            }
        }
        if let Some(paths) = &self.config.plugins
            && let Err(error) =
                wings_plugin_storage::Storage::new(paths.storage.clone()).remove(id, &old.scope)
        {
            eprintln!("[plugins] couldn't delete {id} storage: {error}");
        }
    }

    /// Stops the plugin if it runs, then starts what the store says may run of it now, and tells Claude Code's sessions
    /// its tools may have changed.
    pub(super) fn restart_plugin(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let had_blocks = self.stop_plugin(id, window, cx);
        let started = match (&self.config.plugins, &self.plugin_env.store) {
            (Some(paths), Some(store)) => {
                let found: Vec<_> = installed::discover_in(&store.lock().unwrap())
                    .into_iter()
                    .filter(|found| found.id == id)
                    .collect();
                let env: Arc<dyn crate::page::plugin::Env> = self.plugin_env.clone();
                installed::start(found, paths, &env)
            }
            _ => Vec::new(),
        };
        self.plugin_env.set_plugins(&started);
        let has_blocks = started
            .iter()
            .any(|installed| !installed.plugin.manifest.contributes.blocks.is_empty());
        self.plugins.extend(started);
        self.plugins
            .sort_by_key(|installed| installed.name.to_lowercase());
        self.after_plugins_changed(had_blocks || has_blocks, cx);
    }

    /// Closes everything of the plugin's that shows, in every project, and drops its main. Whether it had blocks.
    fn stop_plugin(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(at) = self.plugins.iter().position(|installed| installed.id == id) else {
            return false;
        };
        while let Some(index) = self.plugin_tabs.iter().position(|tab| tab.plugin == id) {
            self.close_plugin_tab(index, window, cx);
        }
        for parked in self.parked.values_mut() {
            let gone: Vec<usize> = parked
                .plugin_tabs
                .iter()
                .filter(|tab| tab.plugin == id)
                .map(|tab| tab.id)
                .collect();
            parked.plugin_tabs.retain(|tab| tab.plugin != id);
            if matches!(parked.active, Active::Plugin(tab) if gone.contains(&tab)) {
                parked.active = parked
                    .terminals
                    .first()
                    .map_or(Active::Issues, |tab| Active::Terminal(tab.id));
            }
        }
        if self.right.as_ref().is_some_and(|(plugin, _)| plugin == id) {
            self.right = None;
        }
        self.sidebar_pages.retain(|(plugin, _), _| plugin != id);
        let windows: Vec<_> = self
            .plugin_ui
            .windows
            .keys()
            .filter(|(plugin, _)| plugin == id)
            .cloned()
            .collect();
        for key in windows {
            if let Some(handle) = self.plugin_ui.windows.remove(&key) {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        }
        if self
            .plugin_ui
            .diff
            .as_ref()
            .is_some_and(|(viewer, _)| viewer.read(cx).view.plugin_id == id)
        {
            self.close_diff_view(window, cx);
        }
        for shown in self.plugin_ui.badges.values_mut() {
            shown.remove(id);
        }
        self.plugin_ui.badges.retain(|_, shown| !shown.is_empty());
        for shown in self.plugin_ui.statuses.values_mut() {
            shown.remove(id);
        }
        self.plugin_ui.statuses.retain(|_, shown| !shown.is_empty());
        self.plugin_ui.labels.retain(|(plugin, _), _| plugin != id);
        self.plugin_ui.popover.forget(id);
        if self
            .badge_card
            .as_ref()
            .is_some_and(|card| card.plugin == id)
        {
            self.badge_card = None;
        }
        self.sections.forget_plugin(id);
        self.plugin_env.running.remove(id);
        let stopped = self.plugins.remove(at);
        !stopped.plugin.manifest.contributes.blocks.is_empty()
    }

    /// What every start or stop changes besides the plugin's own things: who reads transcripts, the project page's
    /// blocks, what the plugins were last told, and the tools Claude Code is offered.
    fn after_plugins_changed(&mut self, blocks: bool, cx: &mut Context<Self>) {
        let followed = !self.transcript_plugins.is_empty();
        self.transcript_plugins = self
            .plugins
            .iter()
            .filter(|installed| !installed.plugin.manifest.permissions.transcript.is_empty())
            .map(|installed| installed.plugin.clone())
            .collect();
        // The first plugin that reads transcripts needs the detection that follows them.
        if !followed
            && !self.transcript_plugins.is_empty()
            && let Some(dir) = self.config.agents.clone()
        {
            self._detection = Some(Self::detect(dir, true, cx));
        }
        if blocks {
            self.reload_project_pages(cx);
        }
        // A new main hears of the panes on the next frame.
        self.plugin_ui.signature = 0;
        self.plugin_env.clients.notify_changed();
        cx.notify();
    }

    // ---- What the Plugins section's buttons do ----

    /// Starts an install from the GitHub link typed in the Plugins section.
    pub(super) fn install_from_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(link) = self
            .settings
            .as_ref()
            .map(|dialog| dialog.link.read(cx).text.trim().to_owned())
            .filter(|link| !link.is_empty())
        else {
            return;
        };
        if self.manage.installing.is_some() {
            return;
        }
        let download = cx
            .background_executor()
            .spawn(async move { store::fetch_github(&link) });
        self.run_install("link", download, window, cx);
    }

    /// Installs an official catalogue entry through the same GitHub download and permission-review path as a pasted link.
    pub(super) fn install_available(
        &mut self,
        link: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.manage.installing.is_some() {
            return;
        }
        let download = cx
            .background_executor()
            .spawn(async move { store::fetch_github(&link) });
        self.run_install("available", download, window, cx);
    }

    /// Asks for a `.wings-plugin` file and installs it.
    pub(super) fn install_from_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.manage.installing.is_some() {
            return;
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Install".into()),
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| this.install_file(path, window, cx));
        });
        task.detach();
    }

    /// Installs the `.wings-plugin` file at `path`, as dropping it on the desktop's window does.
    pub(super) fn install_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let read = cx
            .background_executor()
            .spawn(async move { store::read_package(&path).map(|bytes| (bytes, Source::File)) });
        self.run_install("file", read, window, cx);
    }

    /// A file dropped on the window: a `.wings-plugin` opens Settings, Plugins and installs, as dropping one on the
    /// desktop's window does. Anything else dropped does nothing.
    pub(super) fn drop_package(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = paths.paths().iter().find(|path| is_package(path)).cloned() else {
            return;
        };
        self.open_settings(window, cx);
        if let Some(dialog) = &mut self.settings {
            dialog.section = Section::Plugins;
        }
        if self.manage.installing.is_none() {
            self.install_file(file, window, cx);
        }
        cx.notify();
    }

    /// The README of the plugin, parsed, read again when its version changes, as the desktop's `plugin_readme`.
    pub(super) fn plugin_readme(&self, view: &PluginView) -> Option<Rc<Vec<markdown::Block>>> {
        let mut cached = self.manage.readme.borrow_mut();
        if let Some(readme) = cached.as_ref().filter(|readme| {
            readme.id == view.manifest.id && readme.version == view.manifest.version
        }) {
            return readme.blocks.clone();
        }
        // Only ids of installed plugins get a folder, so the id can't point anywhere else.
        let dir = self
            .plugin_env
            .store
            .as_ref()?
            .lock()
            .unwrap()
            .dir(&view.manifest.id);
        let blocks = dir
            .and_then(|dir| read_readme(&dir))
            .map(|text| Rc::new(markdown::parse(&text)));
        *cached = Some(Readme {
            id: view.manifest.id.clone(),
            version: view.manifest.version.clone(),
            blocks: blocks.clone(),
        });
        blocks
    }

    /// Waits for the package, installs it, shows it, and opens its review when it isn't on yet.
    fn run_install(
        &mut self,
        kind: &'static str,
        package: Task<Result<(Vec<u8>, Source), String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.manage.installing = Some(kind);
        self.manage.error = None;
        cx.notify();
        let task = cx.spawn_in(window, async move |this, cx| {
            let package = package.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.manage.installing = None;
                match package.and_then(|(bytes, source)| {
                    this.install_package(&bytes, source, None, window, cx)
                }) {
                    Ok(view) => {
                        this.manage.selected = Some(view.manifest.id.clone());
                        if kind == "link"
                            && let Some(dialog) = &this.settings
                        {
                            dialog.link.update(cx, |field, _| field.text.clear());
                        }
                        // A new plugin, or one whose update asks for more, waits for you to approve it.
                        if !view.approved || !view.enabled {
                            this.queue_reviews([view], cx);
                        }
                    }
                    Err(error) => this.manage.error = Some(error),
                }
                cx.notify();
            });
        });
        task.detach();
    }

    /// The switch of a plugin: on asks for its review first unless what it asks for is approved.
    pub(super) fn toggle_plugin(
        &mut self,
        view: PluginView,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if on && !view.approved {
            self.queue_reviews([view], cx);
            cx.notify();
            return;
        }
        let id = view.manifest.id.clone();
        let shown = on.then(|| Grant::of(&view.manifest));
        let result = self.set_plugin_enabled(&id, on, shown, window, cx);
        self.note_row(&id, result.map(drop));
    }

    /// Approves the plugin the review shows, with what it showed, and moves on to the next one.
    pub(super) fn approve_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Before the review has been up its moment, Turn on does nothing.
        let Some(shown) = self
            .manage
            .reviews
            .first()
            .filter(|_| self.manage.review_armed)
            .cloned()
        else {
            return;
        };
        let id = shown.manifest.id.clone();
        let result =
            self.set_plugin_enabled(&id, true, Some(Grant::of(&shown.manifest)), window, cx);
        self.note_row(&id, result.map(drop));
        self.next_review(cx);
    }

    /// Closes the review shown, leaving the plugin off: Cancel, or Not now with more waiting.
    pub(super) fn skip_review(&mut self, cx: &mut Context<Self>) {
        self.next_review(cx);
    }

    /// Opens Settings, Plugins with the plugin's review, as the desktop does for `wings plugin enable` and new installs.
    pub(super) fn review_plugin(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self
            .plugin_views()
            .into_iter()
            .find(|view| view.manifest.id == id)
        else {
            return;
        };
        self.open_settings(window, cx);
        if let Some(dialog) = &mut self.settings {
            dialog.section = Section::Plugins;
        }
        self.manage.selected = Some(id.to_owned());
        self.queue_reviews([view], cx);
        cx.notify();
    }

    pub(super) fn remove_plugin_now(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.remove_plugin(id, window, cx);
        if result.is_ok() {
            self.manage.removing = None;
        }
        self.note_row(id, result);
    }

    /// Shows an action's error under the plugin, or clears the last one.
    fn note_row(&mut self, id: &str, result: Result<(), String>) {
        match result {
            Ok(()) => self.manage.row_error.remove(id),
            Err(error) => self.manage.row_error.insert(id.to_owned(), error),
        };
    }

    /// Reinstalls GitHub plugins from their newest release or default branch, one after another as the desktop's Update
    /// all does, then asks to review each that now asks for more access.
    pub(super) fn update_plugins(
        &mut self,
        ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(store) = self.store() else {
            return;
        };
        for id in &ids {
            self.manage.busy.insert(id.clone());
            self.manage.row_error.remove(id);
        }
        cx.notify();
        let task = cx.spawn_in(window, async move |this, cx| {
            let mut asking = Vec::new();
            for id in ids {
                let source = store.lock().unwrap().source(&id);
                let download = cx
                    .background_executor()
                    .spawn(async move {
                        let (repo, folder) = store::update_source(source)?;
                        let bytes = store::download_github(&repo, folder.as_deref())?;
                        Ok::<_, String>((bytes, Source::Github { repo, folder }))
                    })
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.manage.busy.remove(&id);
                    let result = download.and_then(|(bytes, source)| {
                        this.install_package(&bytes, source, Some(&id), window, cx)
                    });
                    match result {
                        Ok(view) => {
                            this.manage.updates.remove(&id);
                            if !view.approved {
                                asking.push(view);
                            }
                        }
                        Err(error) => {
                            this.manage.row_error.insert(id.clone(), error);
                        }
                    }
                    cx.notify();
                });
            }
            let _ = this.update(cx, |this, cx| {
                this.queue_reviews(asking, cx);
                cx.notify();
            });
        });
        task.detach();
    }

    /// Starts the desktop-matched cadence. A check due while unfocused waits for activation instead of doing network work.
    pub(super) fn start_plugin_update_checks(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.manage.plugin_timer = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(FIRST_CHECK_AFTER).await;
            let mut first = true;
            loop {
                if this
                    .update_in(cx, |this, _, cx| {
                        if this.window_active {
                            this.check_plugin_updates(false, cx);
                        } else {
                            this.manage.check_due = true;
                        }
                    })
                    .is_err()
                {
                    break;
                }
                cx.background_executor()
                    .timer(if first {
                        CHECK_EVERY - FIRST_CHECK_AFTER
                    } else {
                        CHECK_EVERY
                    })
                    .await;
                first = false;
            }
        }));
    }

    /// Asks GitHub for installed releases and the official catalogue. The manual and automatic paths share this work.
    pub(super) fn check_plugin_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        // Headless tests supply a source explicitly, never contact the real catalogue on their fake clock.
        #[cfg(test)]
        if self.manage.update_source.is_none() {
            return;
        }
        if self.manage.checking {
            return;
        }
        let asked: Vec<(String, String, String, Option<String>)> = self
            .plugin_views()
            .into_iter()
            .filter(|view| !view.dev)
            .filter_map(|view| match view.source {
                Some(Source::Github { repo, folder }) => {
                    Some((view.manifest.id, view.manifest.version, repo, folder))
                }
                _ => None,
            })
            .collect();
        self.manage.checking = true;
        cx.notify();
        let source = self
            .manage
            .update_source
            .clone()
            .unwrap_or_else(|| Arc::new(GithubUpdates));
        let found = cx.background_executor().spawn(async move {
            let versions = asked
                .into_iter()
                .map(|(id, version, repo, folder)| {
                    let latest = source.latest(&repo, folder.as_deref());
                    (
                        id,
                        latest.map(|latest| store::newer_version(&version, latest.as_deref())),
                    )
                })
                .collect::<Vec<_>>();
            (versions, source.available())
        });
        self.manage.plugin_check = Some(cx.spawn(async move |this, cx| {
            let (found, offered) = found.await;
            let _ = this.update(cx, |this, cx| {
                this.manage.checking = false;
                let installed: HashSet<_> = this
                    .plugin_views()
                    .into_iter()
                    .map(|view| view.manifest.id)
                    .collect();
                let mut fresh = Vec::new();
                let mut found_updates = Vec::new();
                let mut failures = 0;
                let mut asked_count = 0;
                for (id, latest) in found {
                    if !installed.contains(&id) {
                        continue;
                    }
                    asked_count += 1;
                    match latest {
                        Ok(Some(version)) => {
                            found_updates.push((id.clone(), version.clone()));
                            if !manual && this.manage.announced.insert(format!("{id}@{version}")) {
                                fresh.push((id.clone(), version.clone()));
                            }
                            this.manage.updates.insert(id, version);
                        }
                        Ok(None) => {
                            this.manage.updates.remove(&id);
                        }
                        Err(error) => {
                            failures += 1;
                            this.manage.row_error.insert(id, error);
                        }
                    }
                }
                if let Ok(available) = offered {
                    let unseen: Vec<_> = available
                        .iter()
                        .filter(|plugin| !installed.contains(&plugin.id))
                        .filter(|plugin| !this.prefs.seen_plugins.contains(&plugin.id))
                        .cloned()
                        .collect();
                    let mut changed_seen = false;
                    for plugin in &available {
                        if !this.prefs.seen_plugins.contains(&plugin.id) {
                            this.prefs.seen_plugins.push(plugin.id.clone());
                            changed_seen = true;
                        }
                    }
                    this.manage.available = available;
                    if !unseen.is_empty() {
                        this.manage.announce(Announcement {
                            title: if unseen.len() == 1 {
                                format!("New plugin: {}", unseen[0].name)
                            } else {
                                format!("{} new plugins", unseen.len())
                            },
                            body: if unseen.len() == 1 {
                                unseen[0].description.clone()
                            } else {
                                unseen
                                    .iter()
                                    .map(|plugin| plugin.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            },
                            review: Review::Available,
                        });
                    }
                    if changed_seen {
                        this.save(cx);
                    }
                }
                if !manual && !fresh.is_empty() {
                    this.notification_sound(false);
                    this.manage.announce(Announcement {
                        title: if fresh.len() == 1 {
                            let name = this
                                .plugin_views()
                                .into_iter()
                                .find(|view| view.manifest.id == fresh[0].0)
                                .map_or_else(|| fresh[0].0.clone(), |view| view.manifest.name);
                            format!("{} {} is available", name, fresh[0].1)
                        } else {
                            "Plugin updates available".into()
                        },
                        body: if fresh.len() == 1 {
                            String::new()
                        } else {
                            fresh
                                .iter()
                                .filter_map(|(id, version)| {
                                    this.plugin_views()
                                        .into_iter()
                                        .find(|view| view.manifest.id == *id)
                                        .map(|view| format!("{} {version}", view.manifest.name))
                                })
                                .collect::<Vec<_>>()
                                .join(", ")
                        },
                        review: Review::Installed(fresh[0].0.clone()),
                    });
                }
                if manual {
                    if asked_count > 0 && failures == asked_count {
                        this.show_notice(
                            "Couldn't check for plugin updates".into(),
                            String::new(),
                            cx,
                        );
                    } else if found_updates.is_empty() {
                        this.show_notice("All plugins are up to date".into(), String::new(), cx);
                    }
                }
                cx.notify();
            });
        }));
    }
}

/// What the screenshots put on screen without a download or an agent.
#[cfg(feature = "test-support")]
impl Workspace {
    /// Settings, Plugins with `selected` open and `updates` found, as `(plugin, version)`.
    pub fn show_plugins(
        &mut self,
        selected: &str,
        updates: &[(&str, &str)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings(window, cx);
        if let Some(dialog) = &mut self.settings {
            dialog.section = Section::Plugins;
        }
        self.manage.selected = Some(selected.to_owned());
        for (id, version) in updates {
            self.manage
                .updates
                .insert((*id).to_owned(), (*version).to_owned());
        }
        cx.notify();
    }

    /// The review of `id`, as installing it or `wings plugin enable` opens it.
    pub fn show_plugin_review(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.review_plugin(id, window, cx);
    }

    /// An agent's setup command waiting for you; Allow arms after its moment.
    pub fn show_setup_approval(
        &mut self,
        project: &str,
        pane: &str,
        current: Option<&str>,
        proposed: &str,
        cx: &mut Context<Self>,
    ) {
        // Nobody waits for this answer.
        let _ = self.ask_setup_approval(
            project.into(),
            pane.into(),
            current.map(Into::into),
            proposed.into(),
            cx,
        );
    }

    /// Closes Settings and anything over it, for the next screenshot.
    pub fn close_dialogs(&mut self, cx: &mut Context<Self>) {
        self.settings = None;
        self.manage.reviews.clear();
        self.setup_asks.queue.clear();
        cx.notify();
    }
}
