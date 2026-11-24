//! Completed work stays bold until its project or agent is opened. No clock is needed.

use std::{collections::{HashMap, HashSet}, path::Path};

use crate::{agents::AgentState, workspace::{Active, Workspace}};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::workspace) enum AgentKey {
    Pane(usize),
    Loose(usize),
    Tab(usize),
}

#[cfg(feature = "test-support")]
impl Workspace {
    /// Made-up completion and model data for the offscreen sidebar scene.
    pub fn sample_sidebar_polish(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        use crate::workspace::NotifyWorkspace;
        let Some(web) = self.projects.iter().find(|entry| entry.name == "acme-web").map(|entry| entry.path.clone()) else { return };
        let Some(api) = self.projects.iter().find(|entry| entry.name == "acme-api").map(|entry| entry.path.clone()) else { return };
        self.switch_project(web.clone(), window, cx);
        for (pane, path, state, name, model) in [
            (900, api, AgentState::Done, "Fix checkout redirect", Some("opus")),
            (901, web, AgentState::Working, "Review API changes", None),
        ] {
            self.agents.insert(pane, crate::agents::Agent {
                pane_id: pane.to_string(), space_id: path.to_string_lossy().into_owned(),
                pid: 1, kind: crate::agents::AgentKind::Claude, session_id: None,
                name: Some(name.into()), state, waiting_for: None,
                args: model.map(|model| vec!["--model".into(), model.into()]).unwrap_or_default(),
                launcher: None, started_at: super::now_ms() - 240_000,
                state_since: Some(super::now_ms() - 240_000),
                direct: true, summary: None, weak_name: false, claude_dir: None,
            });
        }
        self.sync_sidebar_activity();
        cx.notify_workspace();
    }
}

#[derive(Default)]
pub(super) struct Activity {
    seen_agents: HashSet<AgentKey>,
    seen_projects: HashSet<AgentKey>,
    tab_since: HashMap<usize, (AgentState, u64)>,
    completions: HashMap<AgentKey, (u64, Option<u64>, Option<String>)>,
}

impl Workspace {
    pub(in crate::workspace) fn agent_unread(&self, key: AgentKey, state: AgentState) -> bool {
        state == AgentState::Done && !self.side.activity.seen_agents.contains(&key)
    }

    pub(in crate::workspace) fn project_unread(&self, path: &Path) -> bool {
        self.agents.iter().any(|(pane, agent)| {
            Path::new(&agent.space_id) == path
                && agent.state == AgentState::Done
                && !self.side.activity.seen_projects.contains(&AgentKey::Pane(*pane))
        }) || self.tab_agents().iter().any(|(project, tab, agent)| {
            project == path && agent.state == "done"
                && !self.side.activity.seen_projects.contains(&AgentKey::Tab(*tab))
        })
    }

    pub(in crate::workspace) fn see_agent(&mut self, key: AgentKey) {
        self.side.activity.seen_agents.insert(key);
    }

    pub(in crate::workspace) fn see_project(&mut self, path: &Path) -> bool {
        let mut changed = false;
        for (pane, agent) in &self.agents {
            if Path::new(&agent.space_id) == path && agent.state == AgentState::Done {
                changed |= self.side.activity.seen_projects.insert(AgentKey::Pane(*pane));
            }
        }
        for (project, tab, agent) in self.tab_agents() {
            if project == path && agent.state == "done" {
                changed |= self.side.activity.seen_projects.insert(AgentKey::Tab(tab));
            }
        }
        changed
    }

    /// State changes, not elapsed time, clear the acknowledgements for a new turn.
    pub(in crate::workspace) fn sync_sidebar_activity(&mut self) {
        let tabs = self.tab_agents();
        let activity = &mut self.side.activity;
        let live: HashSet<_> = self.agents.iter().map(|(pane, _)| AgentKey::Pane(*pane))
            .chain(self.loose_agents.iter().map(|(key, _)| AgentKey::Loose(*key))).collect();
        activity.completions.retain(|key, _| live.contains(key));
        for (key, agent) in self.agents.iter().map(|(pane, agent)| (AgentKey::Pane(*pane), agent))
            .chain(self.loose_agents.iter().map(|(key, agent)| (AgentKey::Loose(*key), agent)))
        {
            let completion = (agent.started_at, agent.state_since, agent.session_id.clone());
            if activity.completions.get(&key) != Some(&completion) {
                activity.seen_agents.remove(&key);
                activity.seen_projects.remove(&key);
                activity.completions.insert(key, completion);
            }
        }
        let done: HashSet<_> = self.agents.iter()
            .filter(|(_, agent)| agent.state == AgentState::Done)
            .map(|(pane, _)| AgentKey::Pane(*pane))
            .chain(self.loose_agents.iter()
                .filter(|(_, agent)| agent.state == AgentState::Done)
                .map(|(key, _)| AgentKey::Loose(*key)))
            .chain(tabs.iter().filter(|(_, _, agent)| agent.state == "done")
                .map(|(_, tab, _)| AgentKey::Tab(*tab)))
            .collect();
        self.side.activity.seen_agents.retain(|key| done.contains(key));
        self.side.activity.seen_projects.retain(|key| done.contains(key));
        self.side.activity.tab_since.retain(|tab, _| tabs.iter().any(|(_, id, _)| id == tab));
        for (_, tab, agent) in tabs {
            let state = super::super::agent_list::tab_state(&agent.state);
            let since = self.side.activity.tab_since.entry(tab).or_insert((state, super::now_ms()));
            if since.0 != state {
                *since = (state, super::now_ms());
            }
        }
        if self.window_active {
            self.see_project(&self.config.project.clone());
            if let Some(key) = self.loose_page() {
                self.see_agent(AgentKey::Loose(key));
            } else {
                match self.active {
                    Active::Plugin(tab) => self.see_agent(AgentKey::Tab(tab)),
                    Active::Terminal(_) => {
                        if let Some(pane) = self.focused_pane() {
                            self.see_agent(AgentKey::Pane(pane));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    pub(in crate::workspace) fn tab_agent_since(&self, tab: usize) -> Option<u64> {
        self.side.activity.tab_since.get(&tab).map(|(_, since)| *since)
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use crate::{installed::{WINGS_VERSION, fixture::Data}, page::{plugin::TabAgent, runtime::fake}, workspace::tests::open_config};

    #[test]
    fn sidebar_tab_completion_is_seen_on_open_and_a_new_turn_is_unread_again() {
        let data = Data::temp();
        data.install_script("chatty", serde_json::json!({
            "contributes": { "ui": ["agents"], "views": [{ "id": "chat", "title": "Chat", "page": "dist/chat.html", "opens": ["tab"] }] }
        }), &fake::idle("main"), &[("chat", &fake::page("page", |_| serde_json::json!({
            "id": 0, "type": "#root", "children": []
        })))], WINGS_VERSION);
        let (app, mut window) = open_config(|config| config.plugins = Some(data.paths()));
        let (tab, state, terminal) = window.update(|w, window, cx| {
            let terminal = w.active;
            assert!(w.open_plugin_view("chatty", "chat", None, window, cx));
            let tab = w.plugin_tabs.last().unwrap();
            let (id, state) = (tab.id, tab.state.clone());
            w.show(terminal, window, cx);
            (id, state, terminal)
        });
        let report = |status: &str| {
            state.set_agent(Some(TabAgent { state: status.into(), name: "Review".into(), summary: None, session_id: None, account: None })).unwrap();
            app.run_until_parked();
        };
        report("working");
        report("done");
        assert!(window.read(|w, _| w.agent_unread(AgentKey::Tab(tab), AgentState::Done)));
        window.update(|w, window, cx| w.show(Active::Plugin(tab), window, cx));
        assert!(!window.read(|w, _| w.agent_unread(AgentKey::Tab(tab), AgentState::Done)));
        window.update(|w, window, cx| w.show(terminal, window, cx));
        report("working");
        report("done");
        assert!(window.read(|w, _| w.agent_unread(AgentKey::Tab(tab), AgentState::Done)));
    }
}
