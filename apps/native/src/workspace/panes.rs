//! Panes: opening, splitting, closing, focusing, zooming, renaming and resizing them.

use std::path::PathBuf;

use gpui::{
    App, Bounds, Context, Entity, Pixels, Role, SharedString, Subscription, Window, point,
    prelude::*, px, size,
};

use crate::{
    agents,
    layout::{Dir, Layout, Toward, neighbor},
    saved,
    terminal_view::{TerminalEvent, TerminalView},
    text_field::{FieldEvent, TextField},
};

#[cfg(unix)]
use crate::terminal_server;

#[cfg(all(test, unix, feature = "test-support"))]
#[path = "server_tests.rs"]
mod server_tests;

use super::{Active, Drag, Pane, Workspace};

/// Whose shell ended: a pane's, or a sidebar terminal's.
#[derive(Clone, Copy)]
pub(super) enum Ended {
    Pane(usize),
    Loose(usize),
}

impl Workspace {
    /// A terminal tab's panes in reading order, and the one with focus.
    pub fn panes(&self, tab: usize) -> Option<(Vec<usize>, usize)> {
        let tab = self.terminals.iter().find(|t| t.id == tab)?;
        Some((tab.layout.panes(), tab.focused))
    }

    /// Like the desktop's pane label: the name you gave it, else its agent's name, else the
    /// program it runs. Until the first detection, the title its program set, or "Terminal".
    pub(super) fn pane_title(&self, id: usize, cx: &App) -> SharedString {
        let pane = self.panes.get(&id);
        pane.and_then(|pane| pane.name.clone())
            .or_else(|| {
                let agent = self.agents.get(&id)?;
                Some(
                    agent
                        .name
                        .clone()
                        .unwrap_or_else(|| agents::label(agent.kind).into())
                        .into(),
                )
            })
            .or_else(|| {
                let command = &self.pane_info.get(&id)?.command;
                (!command.is_empty()).then(|| command.clone().into())
            })
            .or_else(|| {
                pane.and_then(|pane| pane.view.as_ref().ok())
                    .and_then(|view| view.read(cx).title())
                    .map(SharedString::from)
            })
            .unwrap_or_else(|| "Terminal".into())
    }

    /// Beside the title: an agent's project, or the folder the program is in.
    pub(super) fn pane_detail(&self, id: usize) -> Option<SharedString> {
        if let Some(agent) = self.agents.get(&id) {
            let project = self
                .projects
                .iter()
                .find(|p| p.path.as_os_str() == agent.space_id.as_str())?;
            return Some(project.name.clone().into());
        }
        let cwd = self.pane_info.get(&id)?.cwd.as_deref()?;
        Some(tildify(cwd).into())
    }

    /// Double-clicking a pane's title edits it. Enter keeps the name, an empty one goes back to the
    /// program's title, and Escape leaves it as it was.
    pub(super) fn start_rename(
        &mut self,
        pane: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self.pane_title(pane, cx);
        let field = cx.new(|cx| {
            let mut field = TextField::new(Role::TextInput, "Pane name", "Terminal", px(160.0), cx);
            field.text = title.to_string();
            field
        });
        let events = cx.subscribe_in(&field, window, move |this, field, event, window, cx| {
            let name = field.read(cx).text.trim().to_string();
            match event {
                FieldEvent::Changed => return,
                FieldEvent::Submit { .. } => {
                    if let Some(pane) = this.panes.get_mut(&pane) {
                        pane.name = (!name.is_empty()).then(|| name.into());
                    }
                }
                FieldEvent::Cancel => {}
            }
            if let Some(pane) = this.panes.get_mut(&pane) {
                pane.renaming = None;
            }
            this.focus_pane(pane, window, cx);
        });
        field.update(cx, |field, cx| field.select_all(window, cx));
        if let Some(pane) = self.panes.get_mut(&pane) {
            pane.renaming = Some((field, events));
        }
        cx.notify();
    }

    /// The terminal in a pane, for tests and anything that needs to reach its shell.
    pub fn terminal(&self, pane: usize) -> Option<Entity<TerminalView>> {
        self.panes.get(&pane)?.view.as_ref().ok().cloned()
    }

    /// A shell in `cwd`, or in the project folder when there's none or it's gone, below the output
    /// saved under `history` on the last quit.
    pub(super) fn open_pane(
        &mut self,
        cwd: Option<PathBuf>,
        history: Option<String>,
        cx: &mut Context<Self>,
    ) -> usize {
        self.open_pane_as(cwd, history, None, cx)
    }

    /// `open_pane` under the Claude Code account `account` instead of the project's, with `Some`. A name that isn't an
    /// account's, from a file edited by hand, is the project's.
    pub(super) fn open_pane_as(
        &mut self,
        cwd: Option<PathBuf>,
        history: Option<String>,
        account: Option<wings_spaces::claude_accounts::AccountPick>,
        cx: &mut Context<Self>,
    ) -> usize {
        let id = self.next_pane;
        self.next_pane += 1;
        self.open_pane_at(id, cwd, history, account, None, cx);
        id
    }

    /// `open_pane_as` under `id`, which a tab restored at launch already holds for the pane (`start_waiting`). A restored
    /// pane's agent `session` resumes in the new shell when the pane doesn't attach to a program the terminal server
    /// still runs, as the desktop's restore types its resume line only into a pane that can't attach.
    pub(super) fn open_pane_at(
        &mut self,
        id: usize,
        cwd: Option<PathBuf>,
        history: Option<String>,
        account: Option<wings_spaces::claude_accounts::AccountPick>,
        session: Option<saved::SavedSession>,
        cx: &mut Context<Self>,
    ) {
        let pick = account.filter(|pick| wings_spaces::claude_accounts::pick_dir(pick).is_ok());
        let history = history
            .filter(|history| saved::valid_history_id(history))
            .unwrap_or_else(|| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                format!("{:x}-{id}", now.as_nanos())
            });
        // Started in another folder, its agent's session wouldn't be found there.
        if let Some(volume) =
            unmounted_volume(cwd.as_deref(), &self.config.project, &self.config.volumes)
        {
            let waiting = format!("Waiting for {} to be mounted", volume.display());
            self.insert_pane(id, Err(waiting.into()), history, pick, session, cx);
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.mount = Some(MountWait { volume, cwd });
            }
            self.watch_mounts(cx);
            return;
        }
        let folder = cwd
            .filter(|folder| folder.is_dir())
            .unwrap_or_else(|| self.config.project.clone());
        let mut output = self
            .config
            .state
            .as_deref()
            .map(|state| saved::load_history(state, &history))
            .unwrap_or_default();
        let account = self.account_dir(pick.as_ref());
        super::claude::sync_account_settings(self.config.agents.clone(), account.clone(), cx);
        #[cfg(unix)]
        let served = self.open_in_server(&folder, &history, &mut output, &account, cx);
        #[cfg(not(unix))]
        let served = None;
        let (view, attached) = match served {
            Some((view, attached)) => (Ok(view), attached),
            None => (
                TerminalView::open(folder, output, account.clone(), cx).map_err(SharedString::from),
                false,
            ),
        };
        let line = session.as_ref().filter(|_| !attached).and_then(|session| {
            let dir = account.as_deref().or(self.config.agents.as_deref());
            super::history::restore_line(session, dir)
        });
        let resuming = match (&view, &line) {
            (Ok(view), Some(line)) => {
                view.update(cx, |view, cx| view.write_input(line, cx));
                true
            }
            _ => false,
        };
        self.insert_pane(id, view, history, pick, session, cx);
        if resuming && let Some(pane) = self.panes.get_mut(&id) {
            pane.resuming = Some(cx.background_executor().now());
            // Its row says it couldn't resume once the time is up, which nothing else would draw.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(RESUME_TIMEOUT).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
    }

    /// Starts the panes waiting for a drive that is now mounted, in the folders they were in, and resumes their agents.
    /// Whether any still wait.
    fn start_mounted(&mut self, cx: &mut Context<Self>) -> bool {
        let mounted: Vec<usize> = self
            .panes
            .iter()
            .filter(|(_, pane)| pane.mount.as_ref().is_some_and(|wait| wait.volume.is_dir()))
            .map(|(id, _)| *id)
            .collect();
        for id in &mounted {
            let Some(pane) = self.panes.remove(id) else {
                continue;
            };
            let cwd = pane.mount.and_then(|wait| wait.cwd);
            self.open_pane_at(
                *id,
                cwd,
                Some(pane.history),
                pane.claude_account,
                pane.session,
                cx,
            );
            if let Some(started) = self.panes.get_mut(id) {
                started.name = pane.name;
            }
        }
        if !mounted.is_empty() {
            // Shown only if its tab is, and with the keys if it has focus there.
            let workspace = cx.entity();
            cx.defer(move |cx| {
                cx.with_window(workspace.entity_id(), |window, cx| {
                    workspace.update(cx, |this, cx| this.show(this.active, window, cx))
                });
            });
        }
        self.panes.values().any(|pane| pane.mount.is_some())
    }

    /// Checks for the drives panes wait for until none does. A check is one `stat` of each drive's folder.
    fn watch_mounts(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.watching_mounts, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(MOUNT_CHECK).await;
                let waiting = this.update(cx, |this, cx| {
                    let waiting = this.start_mounted(cx);
                    this.watching_mounts = waiting;
                    waiting
                });
                if !matches!(waiting, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The folder for `CLAUDE_CONFIG_DIR` in a pane of the project on show that was asked for the account `pick`, else
    /// the project's; `None` for the default account.
    pub(super) fn account_dir(
        &self,
        pick: Option<&wings_spaces::claude_accounts::AccountPick>,
    ) -> Option<PathBuf> {
        match pick {
            Some(pick) => wings_spaces::claude_accounts::pick_dir(pick).unwrap_or_default(),
            None => self.spaces.claude_config_dir(&self.config.project),
        }
    }

    /// Keeps `view` as the pane `id`, which closes when its shell exits.
    fn insert_pane(
        &mut self,
        id: usize,
        view: Result<Entity<TerminalView>, SharedString>,
        history: String,
        claude_account: Option<wings_spaces::claude_accounts::AccountPick>,
        session: Option<saved::SavedSession>,
        cx: &mut Context<Self>,
    ) {
        let exit = view
            .as_ref()
            .ok()
            .map(|view| self.close_on_exit(view, Ended::Pane(id), cx));
        self.panes.insert(
            id,
            Pane {
                view,
                focus: cx.focus_handle(),
                history,
                name: None,
                claude_account,
                session,
                agent_seen: false,
                resuming: None,
                mount: None,
                renaming: None,
                _exit: exit,
            },
        );
    }

    /// A pane of a project not on screen, attached hidden to the program the terminal server still runs for the saved
    /// pane `history`, as a parked pane is. `None` when no server runs it or its program ended: that pane starts when
    /// its project is shown, as the desktop starts a restored pane's shell the first time it's shown.
    #[cfg(unix)]
    pub(super) fn attach_hidden(
        &mut self,
        history: &str,
        account: Option<wings_spaces::claude_accounts::AccountPick>,
        session: Option<saved::SavedSession>,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let servers = self.config.terminal_server.as_ref()?.0.clone();
        let (socket, pane) = servers.claim_running(history)?;
        let view = TerminalView::attach_in_server(&socket, &pane, cx)
            .map_err(|error| eprintln!("{error}"))
            .ok()?;
        // A new view is visible, and its model would make every screen for a window that never shows it.
        view.update(cx, |view, cx| view.set_visible(false, cx));
        let id = self.next_pane;
        self.next_pane += 1;
        let pick = account.filter(|pick| wings_spaces::claude_accounts::pick_dir(pick).is_ok());
        self.insert_pane(id, Ok(view), history.to_owned(), pick, session, cx);
        Some(id)
    }

    /// Closes the pane or the sidebar terminal once its shell exits on its own, as the desktop's `onPaneExited`.
    pub(super) fn close_on_exit(
        &self,
        view: &Entity<TerminalView>,
        ended: Ended,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(view, move |_, _, _: &TerminalEvent, cx| {
            // The event arrives with no window at hand; closing needs one to move the focus.
            let workspace = cx.entity();
            cx.defer(move |cx| {
                cx.with_window(workspace.entity_id(), |window, cx| {
                    workspace.update(cx, |this, cx| match ended {
                        Ended::Pane(pane) => this.close_pane(pane, window, cx),
                        Ended::Loose(key) => this.close_quick(key, window, cx),
                    })
                });
            });
        })
    }

    /// The pane in the terminal server: the one it kept running for `history`, attached (`true`), else a new one there
    /// (`open_new_in_server`). `None` runs the pane in the app. A pane that ended while the app was away leaves what it
    /// printed in `output`, above the new shell.
    #[cfg(unix)]
    fn open_in_server(
        &mut self,
        folder: &std::path::Path,
        history: &str,
        output: &mut Vec<u8>,
        account: &Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Option<(gpui::Entity<TerminalView>, bool)> {
        let servers = self.config.terminal_server.as_ref()?.0.clone();
        if let Some((socket, pane)) = servers.claim(history) {
            if !pane.exited {
                match TerminalView::attach_in_server(&socket, &pane, cx) {
                    Ok(view) => return Some((view, true)),
                    Err(error) => eprintln!("{error}"),
                }
            } else {
                match terminal_server::read(&socket, pane.pane) {
                    Ok(kept) => *output = kept,
                    Err(error) => {
                        eprintln!("Couldn't read a pane that ended in the terminal server: {error}")
                    }
                }
                terminal_server::kill(&socket, pane.pane);
            }
        } else if let Some(kept) = servers.claim_retired(history) {
            // Its idle shell on an older server ended at launch: the new one starts below what it printed.
            *output = kept;
        }
        let space = self.project_key();
        self.open_new_in_server(&space, history, folder, output, account, cx)
            .map(|view| (view, false))
    }

    /// A new shell in the terminal server under `space_id` and `history`. A server that stopped is replaced by a new
    /// one. `None` runs it in the app: no server, or none could start, which a notice says.
    #[cfg(unix)]
    pub(super) fn open_new_in_server(
        &mut self,
        space_id: &str,
        history: &str,
        folder: &std::path::Path,
        output: &[u8],
        account: &Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Entity<TerminalView>> {
        let servers = self.config.terminal_server.as_ref()?.0.clone();
        let socket = servers.current()?;
        let open = |socket: &std::path::Path, cx: &mut Context<Self>| {
            TerminalView::open_in_server(
                socket,
                space_id.to_owned(),
                history.to_owned(),
                folder.to_owned(),
                output.to_owned(),
                account.clone(),
                cx,
            )
            .map_err(|error| eprintln!("{error}"))
            .ok()
        };
        if let Some(view) = open(&socket, cx) {
            return Some(view);
        }
        let view = servers
            .replace(&socket)
            .and_then(|socket| open(&socket, cx));
        if view.is_none() {
            self.show_notice(
                "The terminal server isn't running".into(),
                "This terminal runs in Wings and ends when Wings quits.".into(),
                cx,
            );
        }
        view
    }

    /// Opens a shell beside the focused pane: to the right for a row, below for a column.
    pub fn split(&mut self, dir: Dir, window: &mut Window, cx: &mut Context<Self>) {
        let Active::Terminal(tab) = self.active else {
            return;
        };
        let pane = self.open_pane(None, None, cx);
        if let Some(tab) = self.terminals.iter_mut().find(|t| t.id == tab) {
            tab.layout.split(tab.focused, dir, pane);
            tab.focused = pane;
            tab.zoomed = None;
        }
        self.show(self.active, window, cx);
    }

    /// Ends the pane's shell. The pane before it takes focus, and the tab closes with its last pane.
    pub fn close_pane(&mut self, pane: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self
            .terminals
            .iter()
            .position(|tab| tab.layout.panes().contains(&pane))
        else {
            return;
        };
        #[cfg(unix)]
        let older = self.on_older_server(pane, cx);
        self.panes.remove(&pane);
        let tab = &mut self.terminals[index];
        if tab.zoomed == Some(pane) {
            tab.zoomed = None;
        }
        let order = tab.layout.panes();
        let at = order.iter().position(|id| *id == pane).unwrap_or(0);
        match std::mem::replace(&mut tab.layout, Layout::Pane(pane)).remove(pane) {
            Some(layout) => {
                if tab.focused == pane {
                    tab.focused = if at == 0 { order[1] } else { order[at - 1] };
                }
                tab.layout = layout;
                if self.active == Active::Terminal(tab.id) {
                    self.show(self.active, window, cx);
                }
            }
            None => self.close_tab(index, window, cx),
        }
        // What's left on its older server may now be able to move.
        #[cfg(unix)]
        if older {
            self.tidy_older_servers(cx);
        }
        cx.notify();
    }

    pub(super) fn focus_pane(&mut self, pane: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self
            .terminals
            .iter_mut()
            .find(|tab| tab.layout.panes().contains(&pane))
        {
            tab.focused = pane;
            // Another pane taking focus brings the rest back.
            if tab.zoomed != Some(pane) {
                tab.zoomed = None;
            }
        }
        self.show(self.active, window, cx);
    }

    /// Fills the tab with the pane, or brings the others back if it already does.
    pub fn toggle_zoom(&mut self, pane: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self
            .terminals
            .iter_mut()
            .find(|tab| tab.layout.panes().contains(&pane))
        {
            tab.zoomed = (tab.zoomed != Some(pane)).then_some(pane);
            tab.focused = pane;
        }
        self.show(self.active, window, cx);
    }

    pub(super) fn focus_toward(
        &mut self,
        toward: Toward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Active::Terminal(tab) = self.active else {
            return;
        };
        let area = self
            .area
            .unwrap_or_else(|| Bounds::new(point(px(0.0), px(0.0)), size(px(1000.0), px(600.0))));
        // A zoomed pane has no neighbours on screen, as in the desktop app.
        let next = self
            .terminals
            .iter()
            .find(|t| t.id == tab && t.zoomed.is_none())
            .and_then(|tab| {
                let placed = tab.layout.place(area, &self.floating_in(&tab.layout));
                neighbor(&placed.panes, tab.focused, toward)
            });
        if let Some(next) = next {
            self.focus_pane(next, window, cx);
        }
    }

    pub(super) fn focused_pane(&self) -> Option<usize> {
        match self.active {
            Active::Terminal(tab) => self.panes(tab).map(|(_, focused)| focused),
            Active::Plugin(_) | Active::Project | Active::Issues => None,
        }
    }

    pub(super) fn start_drag(&mut self, tab: usize, path: Vec<bool>, cx: &mut Context<Self>) {
        let Some(area) = self.area else {
            return;
        };
        let split = self.terminals.iter().find(|t| t.id == tab).and_then(|t| {
            t.layout
                .place(area, &self.floating_in(&t.layout))
                .splits
                .into_iter()
                .find(|(at, _, _)| *at == path)
        });
        if let Some((path, dir, bounds)) = split {
            self.drag = Some(Drag {
                tab,
                path,
                dir,
                bounds,
            });
            cx.notify();
        }
    }

    /// Registered on the window, so the drag keeps going wherever the pointer is.
    pub(super) fn drag_to(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else {
            return;
        };
        let b = drag.bounds;
        let ratio = match drag.dir {
            Dir::Row => (position.x - b.left()) / b.size.width,
            Dir::Column => (position.y - b.top()) / b.size.height,
        };
        if let Some(tab) = self.terminals.iter_mut().find(|t| t.id == drag.tab) {
            tab.layout.set_ratio(&drag.path, ratio);
            cx.notify();
        }
    }
}

/// How long a resumed agent may take to show up before its row says it couldn't resume, as in the desktop app.
pub(super) const RESUME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// How often panes waiting for a drive look for it.
const MOUNT_CHECK: std::time::Duration = std::time::Duration::from_secs(2);

/// What a pane waiting for a drive starts with once the drive is mounted.
pub(super) struct MountWait {
    /// The drive's folder, like `/Volumes/encrypted`.
    pub(super) volume: PathBuf,
    pub(super) cwd: Option<PathBuf>,
}

/// The drive in `volumes` that the pane's folder is on, when that drive isn't mounted: its own folder `cwd`, else the
/// project's. `None` when the folder is there, or is missing for another reason, which starts the shell in the project
/// folder as before.
fn unmounted_volume(
    cwd: Option<&std::path::Path>,
    project: &std::path::Path,
    volumes: &std::path::Path,
) -> Option<PathBuf> {
    for folder in cwd.into_iter().chain([project]) {
        if folder.is_dir() {
            return None;
        }
        let Some(drive) = folder
            .strip_prefix(volumes)
            .ok()
            .and_then(|rest| rest.components().next())
        else {
            continue;
        };
        let volume = volumes.join(drive);
        if !volume.is_dir() {
            return Some(volume);
        }
    }
    None
}

/// `/Users/me/x` becomes `~/x`, like the desktop's `tildify`.
pub(super) fn tildify(path: &str) -> String {
    match std::env::home_dir().and_then(|home| Some(path.strip_prefix(home.to_str()?)?.to_owned()))
    {
        Some(rest) if rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\') => {
            format!("~{rest}")
        }
        _ => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::unmounted_volume;

    #[test]
    fn a_pane_waits_only_for_a_drive_that_isnt_mounted() {
        let root = std::env::temp_dir().join(format!("wings-volumes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let volumes = root.join("Volumes");
        let mounted = volumes.join("work");
        std::fs::create_dir_all(mounted.join("app")).unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let locked = volumes.join("encrypted");
        let wait = |cwd: Option<&std::path::Path>, project: &std::path::Path| {
            unmounted_volume(cwd, project, &volumes)
        };

        // Its folder is there.
        assert_eq!(wait(Some(&mounted.join("app")), &locked.join("x")), None);
        // Its folder, or with none the project's, is on a drive that isn't mounted.
        assert_eq!(
            wait(Some(&locked.join("x/web")), &home),
            Some(locked.clone())
        );
        assert_eq!(wait(None, &locked.join("x")), Some(locked.clone()));
        assert_eq!(
            wait(Some(&home.join("gone")), &locked.join("x")),
            Some(locked.clone()),
            "a gone folder falls back to the project's, which waits for its drive"
        );
        // A folder gone from a mounted drive or from the main disk starts in the project folder, as before.
        assert_eq!(wait(Some(&mounted.join("gone")), &home), None);
        assert_eq!(wait(Some(&home.join("gone")), &home), None);
        assert_eq!(wait(None, &home.join("gone")), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
