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
        let folder = cwd
            .filter(|folder| folder.is_dir())
            .unwrap_or_else(|| self.config.project.clone());
        let history = history
            .filter(|history| saved::valid_history_id(history))
            .unwrap_or_else(|| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                format!("{:x}-{id}", now.as_nanos())
            });
        let mut output = self
            .config
            .state
            .as_deref()
            .map(|state| saved::load_history(state, &history))
            .unwrap_or_default();
        let account = match &pick {
            Some(pick) => wings_spaces::claude_accounts::pick_dir(pick).unwrap_or_default(),
            None => self.spaces.claude_config_dir(&self.config.project),
        };
        super::claude::sync_account_settings(self.config.agents.clone(), account.clone(), cx);
        #[cfg(unix)]
        let served = self.open_in_server(&folder, &history, &mut output, &account, cx);
        #[cfg(not(unix))]
        let served = None;
        let (view, attached) = match served {
            Some((view, attached)) => (Ok(view), attached),
            None => (
                TerminalView::open(folder, output, account, cx).map_err(SharedString::from),
                false,
            ),
        };
        if !attached
            && let Ok(view) = &view
            && let Some(line) = session.as_ref().and_then(super::history::resume_line)
        {
            view.update(cx, |view, cx| view.write_input(&line, cx));
        }
        self.insert_pane(id, view, history, pick, session, cx);
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
