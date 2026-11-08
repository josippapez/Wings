//! Agent status for the pane headers, tabs and sidebar: which agent each pane runs and what it's
//! doing, from the desktop app's detector.

use std::{collections::HashMap, path::PathBuf};

use gpui::{App, Context, Task};

use crate::{
    agents::{self, Agent, AgentKind, AgentState, PaneProbe, Scan, Watcher},
    saved::{SavedSession, SessionKind},
    transcripts,
};

use super::{Active, Workspace};

impl Workspace {
    /// Checks the panes every half second, like the desktop app, for as long as the window lives. With `transcripts`,
    /// the transcripts of the Claude sessions found are followed as well, for the plugins that read them.
    pub(super) fn detect(
        claude_dir: PathBuf,
        transcripts: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let followed = transcripts.then(|| transcripts::Watcher::start(claude_dir.clone()));
        let watcher = Watcher::start(claude_dir);
        let activity = watcher.activity();
        let announce = cx.spawn(async move |this, cx| {
            while let Ok(first) = activity.recv().await {
                cx.background_executor().timer(agents::ACTIVITY_BURST).await;
                let mut sessions = vec![first];
                while let Ok(next) = activity.try_recv() {
                    sessions.push(next);
                }
                sessions.sort();
                sessions.dedup();
                if this
                    .update(cx, |this, _| this.publish_agent_activity(&sessions))
                    .is_err()
                {
                    break;
                }
            }
        });
        cx.spawn(async move |this, cx| {
            // Stops with the detection.
            let _announce = announce;
            loop {
                cx.background_executor().timer(agents::EVERY).await;
                let Ok((panes, focused)) = this.update(cx, |this, cx| this.probes(cx)) else {
                    break;
                };
                let Some(scan) = watcher.scan(panes, focused).await else {
                    break;
                };
                if this.update(cx, |this, cx| this.apply(scan, cx)).is_err() {
                    break;
                }
                if let Some(followed) = &followed {
                    let Ok(sessions) = this.update(cx, |this, _| this.claude_sessions()) else {
                        break;
                    };
                    let Some(lines) = followed.poll(sessions).await else {
                        break;
                    };
                    if this
                        .update(cx, |this, _| this.publish_transcripts(lines))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        })
    }

    /// Every pane in every project, and the one you're looking at while the window has focus,
    /// which never turns Done.
    fn probes(&self, cx: &App) -> (Vec<PaneProbe>, Option<String>) {
        let mut projects = HashMap::new();
        let shown = self.terminals.iter().map(|tab| (&self.config.project, tab));
        let parked = self
            .parked
            .iter()
            .flat_map(|(path, parked)| parked.terminals.iter().map(move |tab| (path, tab)));
        for (path, tab) in shown.chain(parked) {
            for pane in tab.layout.panes() {
                projects.insert(pane, path.to_string_lossy().into_owned());
            }
        }
        let probes = self
            .panes
            .iter()
            .filter_map(|(id, pane)| {
                let view = pane.view.as_ref().ok()?.read(cx);
                Some(PaneProbe {
                    pane_id: id.to_string(),
                    space_id: projects.get(id)?.clone(),
                    shell_pid: view.process_id(),
                    foreground_pid: view.foreground_pid(),
                    title: view.title().unwrap_or_default(),
                    codex_status: None,
                })
            })
            .collect();
        let focused = match self.active {
            Active::Terminal(id) if self.window_active => self
                .terminals
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| tab.focused.to_string()),
            _ => None,
        };
        (probes, focused)
    }

    fn apply(&mut self, scan: Scan, cx: &mut Context<Self>) {
        let by_pane = |id: &str| id.parse::<usize>().ok();
        let agents: HashMap<usize, Agent> = scan
            .agents
            .into_iter()
            .filter_map(|agent| Some((by_pane(&agent.pane_id)?, agent)))
            .collect();
        let info = scan
            .panes
            .into_iter()
            .filter_map(|(id, info)| Some((by_pane(&id)?, info)))
            .collect();
        if agents != self.agents || info != self.pane_info {
            // An agent changing state counts as work in its project, for the sidebar's "Older" group.
            for (pane, agent) in &agents {
                if self.agents.get(pane).map(|old| old.state) != Some(agent.state) {
                    self.side.touch(std::path::Path::new(&agent.space_id));
                }
            }
            let before = std::mem::replace(&mut self.agents, agents);
            self.sync_sessions();
            self.inbox_agents(&before);
            self.pane_info = info;
            cx.notify();
        }
    }

    /// Keeps each pane's agent session for the next restore, as the desktop's `syncPaneSession`: the session of the agent
    /// in it, the one before kept while a resumed agent has yet to report its id; none for an agent that can't resume;
    /// and none once an agent seen this launch has left, so a pane whose agent you quit doesn't resume it. A restored
    /// session stays until its agent shows.
    fn sync_sessions(&mut self) {
        for (id, pane) in &mut self.panes {
            let Some(agent) = self.agents.get(id) else {
                if std::mem::take(&mut pane.agent_seen) {
                    pane.session = None;
                }
                continue;
            };
            pane.agent_seen = true;
            let kind = match agent.kind {
                AgentKind::Claude => SessionKind::Claude,
                AgentKind::Opencode => SessionKind::Opencode,
                AgentKind::Codex | AgentKind::Cursor | AgentKind::Grok => {
                    pane.session = None;
                    continue;
                }
            };
            let id = agent.session_id.clone().or_else(|| {
                let before = pane
                    .session
                    .as_ref()
                    .filter(|session| session.kind == kind)?;
                Some(before.id.clone())
            });
            pane.session = id.map(|id| SavedSession {
                kind,
                id,
                args: agent.args.clone(),
            });
        }
    }

    /// What a tab shows: its focused pane's agent, else whichever of its agents needs you most.
    pub(super) fn tab_state(&self, tab: usize) -> Option<AgentState> {
        let tab = self.terminals.iter().find(|t| t.id == tab)?;
        self.agents
            .get(&tab.focused)
            .map(|agent| agent.state)
            .or_else(|| {
                agents::roll_up(
                    tab.layout
                        .panes()
                        .iter()
                        .filter_map(|pane| self.agents.get(pane).map(|agent| agent.state)),
                )
            })
    }

    /// A project's agents, for its sidebar row.
    pub(super) fn project_agents(&self, path: &std::path::Path) -> Vec<AgentState> {
        let key = path.to_string_lossy();
        self.agents
            .values()
            .filter(|agent| agent.space_id == key)
            .map(|agent| agent.state)
            .collect()
    }
}
