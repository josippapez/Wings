//! Which agent CLI each pane runs and what it's doing, from the desktop app's detector, checked as
//! often as the desktop app checks.

use std::{path::PathBuf, thread, time::Duration};

use async_channel::{Receiver, Sender};
pub use wings_agents::detect::{Agent, AgentKind, AgentState, PaneInfo, PaneProbe, Scan};
use wings_agents::{detect::Detector, opencode};
use wings_spaces::claude_accounts;

/// The desktop's detection interval.
pub const EVERY: Duration = Duration::from_millis(500);

/// Runs the detector on its own thread, so reading processes never holds up drawing.
pub struct Watcher {
    requests: Sender<(Vec<PaneProbe>, Option<String>)>,
    scans: Receiver<Scan>,
}

impl Watcher {
    pub fn start(claude_dir: PathBuf) -> Self {
        let (requests, requests_rx) = async_channel::bounded::<(Vec<PaneProbe>, Option<String>)>(1);
        let (scans_tx, scans) = async_channel::bounded(1);
        thread::Builder::new()
            .name("agent-detection".into())
            .spawn(move || {
                let mut detector = Detector::new(&claude_dir);
                opencode::spawn(detector.opencode());
                while let Ok((panes, focused)) = requests_rx.recv_blocking() {
                    // An account can appear while Wings runs, and each has its own session files.
                    detector
                        .set_claude_dirs(&claude_accounts::dirs_with_default(claude_dir.clone()));
                    let scan = detector.scan(&panes, focused.as_deref());
                    if scans_tx.send_blocking(scan).is_err() {
                        break;
                    }
                }
            })
            .expect("Could not start agent detection");
        Self { requests, scans }
    }

    /// What `panes` run now. `focused` is the pane you're looking at, which never turns Done.
    pub async fn scan(&self, panes: Vec<PaneProbe>, focused: Option<String>) -> Option<Scan> {
        self.requests.send((panes, focused)).await.ok()?;
        self.scans.recv().await.ok()
    }
}

/// Whatever needs you most wins, as in the desktop's `rollUp`.
pub fn roll_up(states: impl IntoIterator<Item = AgentState>) -> Option<AgentState> {
    let rank = |state: AgentState| match state {
        AgentState::Blocked => 3,
        AgentState::Done => 2,
        AgentState::Working => 1,
        AgentState::Idle => 0,
    };
    states.into_iter().max_by_key(|state| rank(*state))
}

/// The name shown for an agent that has none of its own, as in the desktop's Settings > Agents.
pub fn label(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "Claude",
        AgentKind::Opencode => "opencode",
        AgentKind::Codex => "Codex",
        AgentKind::Cursor => "Cursor CLI",
        AgentKind::Grok => "Grok Build",
    }
}

pub fn state_label(state: AgentState) -> &'static str {
    match state {
        AgentState::Working => "Working",
        AgentState::Blocked => "Needs you",
        AgentState::Done => "Done",
        AgentState::Idle => "Idle",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_that_needs_you_most_wins() {
        use AgentState::*;
        assert_eq!(roll_up([]), None);
        assert_eq!(roll_up([Idle, Working]), Some(Working));
        assert_eq!(roll_up([Working, Done, Idle]), Some(Done));
        assert_eq!(roll_up([Done, Blocked, Working]), Some(Blocked));
    }
}
