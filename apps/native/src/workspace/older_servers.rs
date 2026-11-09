//! Terminal servers older than the app's, which drain after an update: their terminals keep the permissions the older
//! server had, until they end. Settings lists every server with its terminals and stops an older one; a pane on one
//! says so in its header. When nothing worth keeping runs on an older server any more (idle shells, and idle agents
//! with a session to resume), its panes move to the current server, in the same tab and with their output kept, so it
//! drains and exits (`tidy_older_servers`). It's checked when the agent detection finds a pane's program or agent
//! changed, and when a pane closes; nothing polls.

use std::{collections::HashMap, path::PathBuf};

use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, KeyDownEvent, MouseButton, SharedString, div,
    prelude::*, px, rgb,
};

use crate::{
    agents::{AgentKind, AgentState},
    saved::SavedSession,
    terminal_server::{self, Running},
    theme,
    ui::{self, Variant},
};

use super::{Workspace, panes::Ended};

#[derive(Default)]
pub(super) struct OlderServers {
    /// The servers Settings lists, as found when it opened or after a stop.
    pub(super) list: Vec<ServerRow>,
    /// The older server whose stop waits for you to confirm, and the dialog's focus.
    pub(super) stopping: Option<(PathBuf, FocusHandle)>,
    /// Panes whose idle agent was told to `/exit`, with the session to resume once they're on the current server.
    pub(super) exiting: HashMap<usize, SavedSession>,
}

/// A server as Settings lists it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ServerRow {
    pub(super) socket: PathBuf,
    pub(super) version: u32,
    pub(super) current: bool,
    /// Terminals whose programs still run.
    pub(super) running: usize,
    /// How many of them a pane in this window shows.
    pub(super) shown: usize,
}

/// What happens to one terminal on an older server when everything on it may go.
#[derive(Debug, PartialEq)]
enum Step {
    /// A pane's idle shell starts again in the current server, typing `resume` if it ran an agent.
    Move(usize, Option<SavedSession>),
    /// A pane's idle agent is told to exit; it moves once its shell is back at the prompt.
    Exit(usize, SavedSession),
    /// An idle shell no pane shows just ends.
    End(u32),
}

/// One terminal of an older server as the app sees it.
#[derive(Debug)]
pub(super) struct Seen {
    /// The server's number for it.
    pub(super) number: u32,
    /// The pane that shows it, if any.
    pub(super) pane: Option<usize>,
    /// Its shell is at the prompt with nothing running (`terminal_server::idle_shell`).
    pub(super) idle_shell: bool,
    /// You typed in it and didn't send it, or it's the shown tab's focused pane.
    pub(super) touched: bool,
    /// The session of its idle agent, which may be ended and resumed (`may_exit`).
    pub(super) idle_agent: Option<SavedSession>,
    /// It was told to exit before, and this is the session to resume.
    pub(super) exiting: Option<SavedSession>,
}

/// What to do with an older server's terminals: every one must be able to go, or nothing happens and the server keeps
/// running until the next check. A terminal you're working in, or that runs anything but an idle shell or agent, waits.
fn plan(terminals: &[Seen]) -> Option<Vec<Step>> {
    terminals
        .iter()
        .map(|seen| match (seen.pane, seen.touched) {
            (_, true) => None,
            (None, false) => seen.idle_shell.then_some(Step::End(seen.number)),
            (Some(pane), false) if seen.idle_shell => Some(Step::Move(pane, seen.exiting.clone())),
            (Some(pane), false) => seen
                .idle_agent
                .clone()
                .map(|session| Step::Exit(pane, session)),
        })
        .collect()
}

impl Workspace {
    /// Whether the pane runs on a server older than the app's.
    pub(super) fn on_older_server(&self, pane: usize, cx: &gpui::App) -> bool {
        let Some(servers) = &self.config.terminal_server else {
            return false;
        };
        let Some(Ok(view)) = self.panes.get(&pane).map(|pane| &pane.view) else {
            return false;
        };
        view.read(cx)
            .server()
            .is_some_and(|(socket, _)| servers.0.is_older(socket))
    }

    /// The quiet note in the header of a pane on an older server.
    pub(super) fn older_server_marker(&self, pane: usize, cx: &gpui::App) -> Option<AnyElement> {
        if !self.on_older_server(pane, cx) {
            return None;
        }
        Some(
            div()
                .id(("older-server", pane))
                // Short, it stays whole in a narrow pane; the folder beside the title gives way first.
                .flex_none()
                .whitespace_nowrap()
                .text_size(px(11.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .tooltip(ui::tooltip(
                    "Runs on the terminal server from before Wings updated, with that server's permissions, until \
                     you close and reopen it.",
                    None,
                ))
                .child("older server")
                .into_any_element(),
        )
    }

    /// The pane's agent may be ended to move it, as the desktop's `canSleep`: idle or done, started straight from the
    /// pane's shell, and a Claude Code or opencode with a session to resume. Whether it started work that still runs
    /// reads the process table, so `tidy_older_servers` asks that last (`agent_busy`).
    fn may_exit(&self, pane: usize) -> Option<SavedSession> {
        let agent = self.agents.get(&pane)?;
        let session = self.panes.get(&pane)?.session.clone()?;
        let resumable = matches!(agent.kind, AgentKind::Claude | AgentKind::Opencode);
        let idle = matches!(agent.state, AgentState::Idle | AgentState::Done);
        (resumable && idle && agent.direct).then_some(session)
    }

    /// Moves what older servers run to the current server once nothing worth keeping runs there (`plan`), so they
    /// drain and exit and the panes get the current server's permissions.
    pub(super) fn tidy_older_servers(&mut self, cx: &mut Context<Self>) {
        let Some(servers) = self.config.terminal_server.clone() else {
            return;
        };
        if servers.0.current().is_none() {
            return;
        }
        let mut shown: HashMap<PathBuf, HashMap<u32, usize>> = HashMap::new();
        for (id, pane) in &self.panes {
            if let Ok(view) = &pane.view
                && let Some((socket, number)) = view.read(cx).server()
                && servers.0.is_older(socket)
            {
                shown
                    .entry(socket.to_owned())
                    .or_default()
                    .insert(number, *id);
            }
        }
        // A pane that left an older server no longer waits to move.
        self.older.exiting.retain(|pane, _| {
            shown
                .values()
                .any(|panes| panes.values().any(|id| id == pane))
        });
        for (socket, panes) in shown {
            let Ok(listed) = wings_ptyd::client::Control::connect(&socket)
                .and_then(|mut control| control.list())
            else {
                continue;
            };
            // The pane the shown tab gives the keys to, even with the window in the back: it'd lose them.
            let focused = self.focused_pane();
            let terminals: Vec<Seen> = listed
                .iter()
                .filter(|info| !info.exited)
                .map(|info| {
                    let pane = panes.get(&info.pane).copied();
                    let touched = pane.is_some_and(|pane| {
                        focused == Some(pane)
                            || self
                                .terminal(pane)
                                .is_some_and(|view| view.read(cx).has_typed())
                    });
                    let idle_shell = terminal_server::idle_shell(info);
                    Seen {
                        number: info.pane,
                        pane,
                        idle_shell,
                        touched,
                        idle_agent: pane
                            .filter(|_| !idle_shell)
                            .and_then(|pane| self.may_exit(pane)),
                        exiting: pane.and_then(|pane| self.older.exiting.get(&pane).cloned()),
                    }
                })
                .collect();
            let Some(steps) = plan(&terminals) else {
                continue;
            };
            // An agent with a background command or dev server still at work keeps the server, as it would keep the
            // desktop's agent awake.
            let working = steps.iter().any(|step| match step {
                Step::Exit(pane, _) if !self.older.exiting.contains_key(pane) => self
                    .agents
                    .get(pane)
                    .is_none_or(|agent| wings_agents::detect::agent_busy(agent.pid)),
                _ => false,
            });
            if working {
                continue;
            }
            for step in steps {
                match step {
                    Step::End(number) => terminal_server::kill(&socket, number),
                    Step::Exit(pane, session) => {
                        if self.older.exiting.contains_key(&pane) {
                            continue;
                        }
                        if let Some(view) = self.terminal(pane) {
                            view.update(cx, |view, cx| view.write_input("/exit\r", cx));
                            self.older.exiting.insert(pane, session);
                        }
                    }
                    Step::Move(pane, resume) => {
                        self.older.exiting.remove(&pane);
                        self.move_to_current_server(pane, resume, cx);
                    }
                }
            }
        }
    }

    /// Starts the pane's shell again in the current server, in the folder it was in and below what it printed, and
    /// ends it on the older one. The pane keeps its place, name and focus. With `resume`, its agent's session comes
    /// back in the new shell, as a restored pane's does.
    fn move_to_current_server(
        &mut self,
        id: usize,
        resume: Option<SavedSession>,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.panes.get(&id) else {
            return;
        };
        let Ok(old) = pane.view.clone() else {
            return;
        };
        let (output, cwd, server) = {
            let view = old.read(cx);
            (
                view.recent_output(),
                view.cwd(),
                view.server()
                    .map(|(socket, number)| (socket.to_owned(), number)),
            )
        };
        let Some((socket, number)) = server else {
            return;
        };
        let space = wings_ptyd::client::Control::connect(&socket)
            .and_then(|mut control| control.list())
            .ok()
            .and_then(|panes| panes.into_iter().find(|info| info.pane == number))
            .map(|info| info.space_id);
        let Some(space) = space else {
            return;
        };
        let project = PathBuf::from(&space);
        let folder = cwd
            .filter(|folder| folder.is_dir())
            .unwrap_or_else(|| project.clone());
        let account = match pane.claude_account.as_ref() {
            Some(pick) => wings_spaces::claude_accounts::pick_dir(pick).unwrap_or_default(),
            None => self.spaces.claude_config_dir(&project),
        };
        let history = pane.history.clone();
        let Some(view) = self.open_new_in_server(&space, &history, &folder, &output, &account, cx)
        else {
            return;
        };
        let line = resume.as_ref().and_then(|session| {
            let dir = account.as_deref().or(self.config.agents.as_deref());
            super::history::restore_line(session, dir)
        });
        if let Some(line) = &line {
            view.update(cx, |view, cx| view.write_input(line, cx));
        }
        let exit = self.close_on_exit(&view, Ended::Pane(id), cx);
        let now = cx.background_executor().now();
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.view = Ok(view);
            pane._exit = Some(exit);
            if let Some(session) = resume.filter(|_| line.is_some()) {
                pane.session = Some(session);
                pane.agent_seen = false;
                pane.resuming = Some(now);
            }
        }
        // Its old shell ends; with the pane's subscription gone, that closes nothing.
        terminal_server::kill(&socket, number);
        drop(old);
        self.sync_pane_visibility(cx);
        cx.notify();
    }

    /// Finds the servers again for Settings' list: when it opens, and after a stop.
    pub(super) fn refresh_servers(&mut self, cx: &mut Context<Self>) {
        let Some(servers) = self.config.terminal_server.clone() else {
            return;
        };
        let current = servers.0.current();
        let in_panes: Vec<(PathBuf, u32)> = self
            .panes
            .values()
            .filter_map(|pane| pane.view.as_ref().ok())
            .filter_map(|view| {
                view.read(cx)
                    .server()
                    .map(|(socket, number)| (socket.to_owned(), number))
            })
            .collect();
        self.older.list = servers
            .0
            .running()
            .into_iter()
            .filter_map(
                |Running {
                     socket,
                     version,
                     panes,
                 }| {
                    let is_current = current.as_deref() == Some(socket.as_path());
                    // An older server with nothing left running is on its way out.
                    if !is_current && (panes.is_empty() || !servers.0.is_older(&socket)) {
                        return None;
                    }
                    let shown = panes
                        .iter()
                        .filter(|pane| in_panes.contains(&(socket.clone(), pane.pane)))
                        .count();
                    Some(ServerRow {
                        socket,
                        version,
                        current: is_current,
                        running: panes.len(),
                        shown,
                    })
                },
            )
            .collect();
        cx.notify();
    }

    /// Settings' rows for the servers: each with its terminals, and a Stop button on an older one.
    pub(super) fn server_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.older
            .list
            .iter()
            .map(|row| {
                let label = if row.current {
                    format!("Terminal server {} (current)", row.version)
                } else {
                    format!("Terminal server {} (older)", row.version)
                };
                let mut hint = terminals_line(row);
                if !row.current {
                    hint.push_str(
                        " Started before an update, it keeps its terminals and their permissions, and ends once \
                         they do.",
                    );
                }
                let control = if row.current {
                    div().into_any_element()
                } else {
                    let socket = row.socket.clone();
                    ui::button(
                        SharedString::from(format!("stop-server-{}", row.version)),
                        format!("Stop terminal server {}", row.version),
                        Variant::Secondary,
                        false,
                        "Stop",
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let focus = cx.focus_handle();
                        focus.focus(window, cx);
                        this.older.stopping = Some((socket.clone(), focus));
                        cx.notify();
                    }))
                    .into_any_element()
                };
                ui::setting_row(label, hint, None, control)
            })
            .collect()
    }

    /// Asks to stop the first older server Settings lists, for the screenshots.
    #[cfg(feature = "test-support")]
    pub fn sample_stop_server(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.refresh_servers(cx);
        if let Some(row) = self.older.list.iter().find(|row| !row.current) {
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            self.older.stopping = Some((row.socket.clone(), focus));
        }
        cx.notify();
    }

    fn cancel_stop(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if let Some(stopping) = self.older.stopping.take() {
            self.leave("stop-server", stopping, ui::motion::POPUP_IN, cx);
        }
        if let Some(dialog) = &self.settings {
            dialog.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// Ends the older server and its terminals; the panes that showed them close.
    pub(super) fn stop_server(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some((socket, _)) = self.older.stopping.clone() else {
            return;
        };
        if let Some(servers) = self.config.terminal_server.clone()
            && let Err(error) = servers.0.stop(&socket)
        {
            eprintln!(
                "Couldn't stop the terminal server at {}: {error}",
                socket.display()
            );
        }
        self.cancel_stop(window, cx);
        self.refresh_servers(cx);
    }

    /// "Stop terminal server N?", over Settings.
    pub(super) fn stop_server_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (socket, focus) = self.older.stopping.as_ref()?;
        let row = self.older.list.iter().find(|row| &row.socket == socket)?;
        let title = format!("Stop terminal server {}?", row.version);
        let detail = match (row.running, row.shown) {
            (1, 1) => {
                "Its terminal ends, and its pane closes. Anything running in it stops.".to_owned()
            }
            (1, _) => "Its terminal ends. Anything running in it stops.".to_owned(),
            (n, 0) => format!("Its {n} terminals end. Anything running in them stops."),
            (n, 1) => {
                format!("Its {n} terminals end, and 1 pane closes. Anything running in them stops.")
            }
            (n, shown) => format!(
                "Its {n} terminals end, and {shown} panes close. Anything running in them stops."
            ),
        };
        let (backdrop, panel) = ui::dialog("stop-server", title.clone(), focus, 400.0, 176.0);
        Some(
            backdrop
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.cancel_stop(window, cx)),
                )
                .child(ui::dialog_panel(
                    panel
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                this.cancel_stop(window, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .p(px(20.0))
                                .child(
                                    div()
                                        .text_size(px(15.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(13.0))
                                        .text_color(rgb(theme::MUTED_FOREGROUND))
                                        .child(detail),
                                )
                                .child(
                                    ui::dialog_footer()
                                        .child(
                                            ui::button(
                                                "cancel-stop-server",
                                                "Cancel",
                                                Variant::Secondary,
                                                false,
                                                "Cancel",
                                            )
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    this.cancel_stop(window, cx)
                                                }),
                                            ),
                                        )
                                        .child(
                                            ui::button(
                                                "confirm-stop-server",
                                                "Stop",
                                                Variant::Destructive,
                                                false,
                                                "Stop",
                                            )
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    this.stop_server(window, cx)
                                                }),
                                            ),
                                        ),
                                ),
                        ),
                ))
                .into_any_element(),
        )
    }
}

/// "Runs 3 terminals: 2 in Wings' panes, 1 not shown anywhere."
fn terminals_line(row: &ServerRow) -> String {
    let count = |n: usize| format!("{n} terminal{}", if n == 1 { "" } else { "s" });
    let hidden = row.running - row.shown;
    match (row.running, row.shown, hidden) {
        (0, _, _) => "Runs no terminals.".to_owned(),
        (n, s, 0) if s == n => format!("Runs {}, all in Wings' panes.", count(n)),
        (n, 0, _) => format!("Runs {}, none shown in Wings.", count(n)),
        (n, s, h) => format!(
            "Runs {}: {s} in Wings' panes, {h} not shown anywhere.",
            count(n)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saved::SessionKind;

    fn session() -> SavedSession {
        SavedSession {
            kind: SessionKind::Claude,
            id: "s1".into(),
            args: Vec::new(),
            name: None,
        }
    }

    fn seen(number: u32, pane: Option<usize>, idle_shell: bool) -> Seen {
        Seen {
            number,
            pane,
            idle_shell,
            touched: false,
            idle_agent: None,
            exiting: None,
        }
    }

    #[test]
    fn everything_moves_only_when_every_terminal_can_go() {
        let idle = [seen(1, Some(10), true), seen(2, None, true)];
        assert_eq!(plan(&idle), Some(vec![Step::Move(10, None), Step::End(2)]));
        let busy = [seen(1, Some(10), true), seen(2, None, false)];
        assert_eq!(
            plan(&busy),
            None,
            "a program runs in a terminal no pane shows"
        );
        let busy_pane = [seen(1, Some(10), false), seen(2, None, true)];
        assert_eq!(
            plan(&busy_pane),
            None,
            "a pane runs a program that isn't an idle agent"
        );
    }

    #[test]
    fn an_idle_agent_exits_first_then_moves_with_its_session() {
        let agent = Seen {
            idle_agent: Some(session()),
            ..seen(1, Some(10), false)
        };
        assert_eq!(plan(&[agent]), Some(vec![Step::Exit(10, session())]));
        let exited = Seen {
            exiting: Some(session()),
            ..seen(1, Some(10), true)
        };
        assert_eq!(plan(&[exited]), Some(vec![Step::Move(10, Some(session()))]));
    }

    #[test]
    fn a_pane_you_are_typing_in_or_looking_at_waits() {
        let touched = Seen {
            touched: true,
            ..seen(1, Some(10), true)
        };
        assert_eq!(plan(&[touched, seen(2, None, true)]), None);
    }

    #[test]
    fn the_row_says_where_the_terminals_are() {
        let row = |running, shown| ServerRow {
            socket: PathBuf::new(),
            version: 5,
            current: false,
            running,
            shown,
        };
        assert_eq!(
            terminals_line(&row(3, 2)),
            "Runs 3 terminals: 2 in Wings' panes, 1 not shown anywhere."
        );
        assert_eq!(
            terminals_line(&row(1, 1)),
            "Runs 1 terminal, all in Wings' panes."
        );
        assert_eq!(
            terminals_line(&row(2, 0)),
            "Runs 2 terminals, none shown in Wings."
        );
    }
}
