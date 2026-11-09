//! Putting idle agents to sleep, as the desktop's `sleep.ts`: an idle Claude Code or opencode nobody looks at is ended
//! with its own `/exit` after the minutes Settings, Agents says, which frees about 400 MB each, and its session resumes
//! when its pane is shown again. Nothing polls: the plan runs when the agents or the panes on screen change, and one
//! timer waits for the next deadline.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use gpui::{Context, Task};

use crate::{
    agents::{AgentKind, AgentState},
    saved::SleepAgents,
};

use super::{Active, Workspace};

/// The "Sleep after" choices, in minutes.
pub(super) const MINUTES: [u32; 5] = [5, 10, 15, 30, 60];
/// How long an agent gets to leave after `/exit` before Wings stops counting its pane as asleep.
const EXIT_GRACE: Duration = Duration::from_secs(15);
/// Claude Code opens its command menu as `/exit` is typed, so Enter comes a moment later as its own key press.
const ENTER_AFTER: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SleepSettings {
    pub(super) enabled: bool,
    pub(super) minutes: u32,
}

/// On, after 15 minutes, as the desktop's `DEFAULT_SLEEP`.
impl Default for SleepSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            minutes: 15,
        }
    }
}

impl SleepSettings {
    pub(super) fn from_saved(saved: Option<&SleepAgents>) -> Self {
        let default = Self::default();
        Self {
            enabled: saved.and_then(|s| s.enabled).unwrap_or(default.enabled),
            minutes: saved
                .and_then(|s| s.minutes)
                .filter(|m| MINUTES.contains(m))
                .unwrap_or(default.minutes),
        }
    }

    pub(super) fn to_saved(self) -> SleepAgents {
        SleepAgents {
            enabled: Some(self.enabled),
            minutes: Some(self.minutes),
        }
    }
}

#[derive(Default)]
pub(super) struct Sleep {
    /// Since when each pane that may sleep has been able to.
    since: HashMap<usize, Instant>,
    /// Panes whose agent didn't take `/exit`: left awake from then on, instead of typed into again.
    refused: HashSet<usize>,
    /// Plans again at the next deadline.
    timer: Option<Task<()>>,
}

/// The new clocks for the panes `eligible` now (a pane that stops being eligible loses its clock, one that starts gets
/// `now`), the panes whose clock ran out, and when the next one will: the desktop's `planSleep`.
fn plan(
    since: &HashMap<usize, Instant>,
    eligible: &[usize],
    now: Instant,
    limit: Duration,
) -> (HashMap<usize, Instant>, Vec<usize>, Option<Instant>) {
    let next: HashMap<usize, Instant> = eligible
        .iter()
        .map(|pane| (*pane, since.get(pane).copied().unwrap_or(now)))
        .collect();
    let mut due: Vec<usize> = next
        .iter()
        .filter(|(_, from)| now - **from >= limit)
        .map(|(pane, _)| *pane)
        .collect();
    due.sort();
    let at = next
        .values()
        .map(|from| *from + limit)
        .filter(|at| *at > now)
        .min();
    (next, due, at)
}

impl Workspace {
    /// The terminal panes on screen: the shown tab's, unless a page covers them.
    pub(super) fn shown_terminal_panes(&self) -> Vec<usize> {
        match self.active {
            _ if self.loose_page().is_some() => Vec::new(),
            Active::Terminal(id) => self
                .terminals
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| self.shown_panes(tab))
                .unwrap_or_default(),
            Active::Plugin(_) | Active::Project | Active::Issues => Vec::new(),
        }
    }

    /// Whether the pane's agent may sleep now, as the desktop's `canSleep`: idle (a finished turn you haven't seen is
    /// "done", which needs you), out of sight, nothing typed in its prompt, started straight from the pane's shell so
    /// the shell is back at its prompt once it exits, and a Claude Code or opencode with a session to resume.
    fn may_sleep(&self, pane: usize, shown: &[usize], cx: &gpui::App) -> bool {
        let (Some(agent), Some(open)) = (self.agents.get(&pane), self.panes.get(&pane)) else {
            return false;
        };
        let typed = match &open.view {
            Ok(view) => view.read(cx).has_typed(),
            Err(_) => true,
        };
        !self.sleep.refused.contains(&pane)
            && !open.asleep
            && open.resuming.is_none()
            && open.session.is_some()
            && !typed
            && !shown.contains(&pane)
            && agent.state == AgentState::Idle
            && agent.direct
            && agent.session_id.is_some()
            && matches!(agent.kind, AgentKind::Claude | AgentKind::Opencode)
    }

    /// Puts the agents whose time ran out to sleep, and waits for the next one. Runs when agents or the panes on
    /// screen change, when the setting does, and at the deadline it set.
    pub(super) fn plan_sleep(&mut self, cx: &mut Context<Self>) {
        let settings = self.prefs.sleep;
        if !settings.enabled {
            self.sleep.since.clear();
            self.sleep.timer = None;
            return;
        }
        let shown = self.shown_terminal_panes();
        let mut eligible: Vec<usize> = self
            .panes
            .keys()
            .copied()
            .filter(|pane| self.may_sleep(*pane, &shown, cx))
            .collect();
        eligible.sort();
        let now = cx.background_executor().now();
        let limit = Duration::from_secs(u64::from(settings.minutes) * 60);
        let (since, due, at) = plan(&self.sleep.since, &eligible, now, limit);
        self.sleep.since = since;
        let mut next = at;
        for pane in due {
            // The last check, once per deadline: a command it started, like a dev server, still runs. It stays awake
            // and its clock starts over.
            let working = self
                .agents
                .get(&pane)
                .is_none_or(|agent| wings_agents::detect::agent_busy(agent.pid));
            if working {
                self.sleep.since.insert(pane, now);
                next = Some(next.map_or(now + limit, |at| at.min(now + limit)));
            } else {
                self.sleep.since.remove(&pane);
                self.sleep_pane(pane, cx);
            }
        }
        self.sleep.timer = next.map(|at| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(at - now).await;
                let _ = this.update(cx, |this, cx| this.plan_sleep(cx));
            })
        });
    }

    /// Ends the pane's agent with its own `/exit`, never by killing it, and keeps its session to resume.
    fn sleep_pane(&mut self, pane: usize, cx: &mut Context<Self>) {
        let Some(view) = self.terminal(pane) else {
            return;
        };
        if let Some(open) = self.panes.get_mut(&pane) {
            open.asleep = true;
        }
        view.update(cx, |view, cx| view.write_input("/exit", cx));
        self.save(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ENTER_AFTER).await;
            let _ = view.update(cx, |view, cx| view.write_input("\r", cx));
            cx.background_executor().timer(EXIT_GRACE).await;
            // It didn't take `/exit`: it stays awake, and Wings doesn't type into it again.
            let _ = this.update(cx, |this, cx| {
                let stayed = this.agents.contains_key(&pane);
                if let Some(open) = this.panes.get_mut(&pane)
                    && open.asleep
                    && stayed
                {
                    open.asleep = false;
                    this.sleep.refused.insert(pane);
                    this.save(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Resumes the session of each sleeping pane now on screen, as the desktop wakes a pane when it's shown.
    pub(super) fn wake_shown(&mut self, cx: &mut Context<Self>) {
        let shown = self.shown_terminal_panes();
        let mut woke = false;
        for pane in shown {
            let Some(open) = self.panes.get(&pane) else {
                continue;
            };
            if !open.asleep || self.agents.contains_key(&pane) {
                continue;
            }
            let (Some(session), Ok(view)) = (open.session.clone(), open.view.clone()) else {
                continue;
            };
            let account = self.account_dir(open.claude_account.as_ref());
            let dir = account.as_deref().or(self.config.agents.as_deref());
            let line = super::history::restore_line(&session, dir);
            let now = cx.background_executor().now();
            let Some(open) = self.panes.get_mut(&pane) else {
                continue;
            };
            open.asleep = false;
            woke = true;
            let Some(line) = line else {
                continue;
            };
            view.update(cx, |view, cx| view.write_input(&line, cx));
            open.agent_seen = false;
            open.resuming = Some(now);
            // Its row says it couldn't resume once the time is up, which nothing else would draw.
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(super::panes::RESUME_TIMEOUT)
                    .await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        if woke {
            self.save(cx);
            cx.notify();
        }
    }
}

impl Workspace {
    /// Settings, Agents' "Put idle agents to sleep" and "Sleep after", as the desktop's.
    pub(super) fn sleep_card(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let current = self.prefs.sleep;
        let minutes = MINUTES
            .into_iter()
            .map(|m| {
                let label = if m == 60 {
                    "1 hour".to_owned()
                } else {
                    format!("{m} minutes")
                };
                let choose: super::settings::Choose =
                    Box::new(move |this, cx| this.set_prefs(|prefs| prefs.sleep.minutes = m, cx));
                (label.into(), m == current.minutes, choose)
            })
            .collect();
        vec![
            self.switch_row(
                "sleep-agents",
                "Put idle agents to sleep",
                "Ends an idle Claude Code or opencode that isn't on screen, which frees about 400 MB each. It stays in \
                 the Agents list as sleeping, and opening it picks the session up again.",
                current.enabled,
                |this, on, cx| this.set_prefs(|prefs| prefs.sleep.enabled = on, cx),
                cx,
            ),
            crate::ui::setting_row(
                "Sleep after",
                "Time out of sight and idle. An agent that is working, needs you or has text typed in its prompt is \
                 left alone.",
                None,
                self.select("sleep-after", "Sleep after", minutes, cx),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_sleeps_once_it_has_been_eligible_for_the_whole_time() {
        let start = Instant::now();
        let limit = Duration::from_secs(15 * 60);
        let (since, due, at) = plan(&HashMap::new(), &[1, 2], start, limit);
        assert_eq!((due, at), (vec![], Some(start + limit)));
        // Pane 2 stopped being eligible, so it loses its clock; pane 3 starts one.
        let later = start + Duration::from_secs(10 * 60);
        let (since, due, at) = plan(&since, &[1, 3], later, limit);
        assert_eq!(since.get(&2), None);
        assert_eq!(since[&3], later);
        assert_eq!((due, at), (vec![], Some(start + limit)));
        let (_, due, at) = plan(&since, &[1, 3], start + limit, limit);
        assert_eq!((due, at), (vec![1], Some(later + limit)));
    }

    #[test]
    fn a_saved_setting_out_of_the_choices_takes_the_default() {
        let saved = |enabled, minutes| SleepAgents { enabled, minutes };
        assert_eq!(SleepSettings::from_saved(None), SleepSettings::default());
        assert_eq!(
            SleepSettings::from_saved(Some(&saved(Some(false), Some(30)))),
            SleepSettings {
                enabled: false,
                minutes: 30
            }
        );
        assert_eq!(
            SleepSettings::from_saved(Some(&saved(None, Some(7)))).minutes,
            15
        );
    }
}
