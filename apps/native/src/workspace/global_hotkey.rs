//! What the global shortcut does in the window, as the desktop's `App.tsx`: it is held only while it is on, a change
//! releases the old one first, and a press brings Wings forward and goes to the agent that needs you, or stays where
//! you were when none does.

use std::path::PathBuf;

use gpui::{Context, SharedString, Task, Window};

use crate::{
    agents::AgentState,
    hotkey::{self, Hotkeys},
    saved::GlobalHotkey,
};

use super::Workspace;

#[derive(Default)]
pub(super) struct HotkeyState {
    /// Why the shortcut couldn't be held, shown under its row in Settings.
    pub(super) error: Option<SharedString>,
    _presses: Option<Task<()>>,
}

impl Workspace {
    /// Holds the saved shortcut and waits for its presses, when the app has a registrar: only `main` installs one.
    pub(super) fn start_global_hotkey(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(presses) = cx.try_global::<Hotkeys>().map(|h| h.presses.clone()) else {
            return;
        };
        self.hotkey._presses = Some(cx.spawn_in(window, async move |this, cx| {
            while presses.recv().await.is_ok() {
                if this
                    .update_in(cx, |this, window, cx| {
                        this.global_hotkey_pressed(window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        self.hold_global_hotkey(cx);
    }

    /// Tells the registrar the shortcut as it is now: the chord while it's on, none while it's off.
    fn hold_global_hotkey(&mut self, cx: &mut Context<Self>) {
        let Some(registrar) = cx.try_global::<Hotkeys>().map(|h| h.registrar.clone()) else {
            return;
        };
        let hotkey = &self.prefs.global_hotkey;
        let result = registrar.set(hotkey.enabled.then_some(hotkey.accelerator.as_str()));
        self.hotkey.error = result.as_ref().err().map(|e| e.clone().into());
        if let Err(error) = result {
            self.show_notice("Couldn't use that global shortcut".into(), error, cx);
        }
        cx.notify();
    }

    /// Saves the shortcut and holds it in place of the old one.
    pub(super) fn set_global_hotkey(&mut self, hotkey: GlobalHotkey, cx: &mut Context<Self>) {
        if self.prefs.global_hotkey == hotkey {
            return;
        }
        self.set_prefs(|prefs| prefs.global_hotkey = hotkey, cx);
        self.hold_global_hotkey(cx);
    }

    /// Pressed from any app: Wings comes forward, then goes to the agent that needs you, if one does.
    pub(super) fn global_hotkey_pressed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        hotkey::raise(window, cx);
        if let Some((project, pane)) = self.next_needing_you() {
            self.go_to_pane(project, pane, window, cx);
        }
    }

    /// The desktop's `nextNeedingYou(agents, null)`: the first agent waiting on you, else the first one done.
    fn next_needing_you(&self) -> Option<(PathBuf, usize)> {
        [AgentState::Blocked, AgentState::Done]
            .into_iter()
            .find_map(|state| {
                self.agents
                    .iter()
                    .filter(|(_, agent)| agent.state == state)
                    .min_by_key(|(pane, _)| **pane)
            })
            .map(|(pane, agent)| (PathBuf::from(&agent.space_id), *pane))
    }
}
