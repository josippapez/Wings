//! Headless window tests for the global shortcut: saved under the desktop's name, held while it's on, recorded in
//! Settings, and what a press does. A fake registrar stands in for the system, so no key is taken from the Mac.

use std::rc::Rc;

use super::{
    tests::{open_with, press},
    *,
};
use crate::{
    agents::{Agent, AgentKind, AgentState},
    hotkey::fake::{self, Fake},
    saved::{DEFAULT_HOTKEY, GlobalHotkey},
};
use gpui::{TestApp, TestAppWindow};

fn temp_state(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("wings-native-hotkey-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("workspace.json")
}

/// A window with the fake registrar installed, as `main` installs the system's.
fn open_held(
    state: Option<PathBuf>,
) -> (
    TestApp,
    TestAppWindow<Workspace>,
    Rc<Fake>,
    async_channel::Sender<()>,
) {
    let (app, mut window) = open_with(state);
    let (hotkeys, fake, press) = fake::hotkeys();
    window.update(|w, window, cx| {
        cx.set_global(hotkeys);
        w.start_global_hotkey(window, cx);
    });
    (app, window, fake, press)
}

fn agent(pane: usize, project: &str, state: AgentState) -> Agent {
    Agent {
        pane_id: pane.to_string(),
        space_id: project.into(),
        pid: 1,
        kind: AgentKind::Claude,
        session_id: None,
        name: None,
        state,
        waiting_for: None,
        args: Vec::new(),
        started_at: 0,
        state_since: None,
        direct: true,
        summary: None,
        weak_name: false,
        claude_dir: None,
    }
}

#[test]
fn the_shortcut_is_held_from_launch_and_turning_it_off_lets_it_go() {
    let (_app, mut window, fake, _) = open_held(None);
    assert_eq!(*fake.calls.borrow(), [Some(DEFAULT_HOTKEY.to_owned())]);
    window.update(|w, _, cx| {
        w.set_global_hotkey(
            GlobalHotkey {
                enabled: false,
                accelerator: DEFAULT_HOTKEY.into(),
            },
            cx,
        )
    });
    assert_eq!(fake.calls.borrow().last(), Some(&None));
}

#[test]
fn a_recorded_shortcut_replaces_the_old_one_and_comes_back_next_launch() {
    let file = temp_state("record");
    {
        let (_app, mut window, fake, _) = open_held(Some(file.clone()));
        window.update(|w, window, cx| {
            w.open_settings(window, cx);
            w.record_global_hotkey(cx);
        });
        window.draw();
        // A key without Ctrl, Alt or Cmd is no shortcut: still waiting.
        window.simulate_keystrokes("shift-k");
        window.read(|w, _| assert!(w.settings.as_ref().unwrap().recording.is_some()));
        window.simulate_keystrokes("ctrl-alt-k");
        window.read(|w, _| {
            assert_eq!(w.prefs.global_hotkey.accelerator, "Ctrl+Alt+KeyK");
            assert!(w.settings.as_ref().unwrap().recording.is_none());
        });
        assert_eq!(
            fake.calls.borrow().last(),
            Some(&Some("Ctrl+Alt+KeyK".to_owned()))
        );
    }
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        saved["globalHotkey"],
        serde_json::json!({ "enabled": true, "accelerator": "Ctrl+Alt+KeyK" })
    );
    let (_app, _window, fake, _) = open_held(Some(file.clone()));
    assert_eq!(*fake.calls.borrow(), [Some("Ctrl+Alt+KeyK".to_owned())]);
    std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
}

#[test]
fn recording_takes_the_apps_own_chords_escape_cancels_and_backspace_puts_the_default_back() {
    let (_app, mut window, _, _) = open_held(None);
    let tabs = window.read(|w, _| w.terminals.len());
    window.update(|w, window, cx| {
        w.open_settings(window, cx);
        w.record_global_hotkey(cx);
    });
    // Cmd-T opens a tab, except while it is being recorded.
    press(&mut window, "t");
    window.read(|w, _| {
        assert_eq!(w.terminals.len(), tabs);
        assert!(w.prefs.global_hotkey.accelerator.ends_with("+KeyT"));
    });
    window.update(|w, _, cx| w.record_global_hotkey(cx));
    window.simulate_keystrokes("escape");
    window.read(|w, _| {
        // Escape ends recording only: Settings stays open and the shortcut as it was.
        assert!(w.settings.as_ref().is_some_and(|d| d.recording.is_none()));
        assert!(w.prefs.global_hotkey.accelerator.ends_with("+KeyT"));
    });
    window.update(|w, _, cx| w.record_global_hotkey(cx));
    window.simulate_keystrokes("backspace");
    window.read(|w, _| assert_eq!(w.prefs.global_hotkey.accelerator, DEFAULT_HOTKEY));
}

#[test]
fn a_shortcut_another_app_holds_says_so() {
    let (_app, mut window, fake, _) = open_held(None);
    fake.refuse.borrow_mut().push("Ctrl+Alt+KeyK".into());
    window.update(|w, _, cx| {
        w.set_global_hotkey(
            GlobalHotkey {
                enabled: true,
                accelerator: "Ctrl+Alt+KeyK".into(),
            },
            cx,
        )
    });
    window.read(|w, _| {
        assert_eq!(
            w.hotkey.error.as_deref(),
            Some("Ctrl+Alt+KeyK can't be used: another app holds it")
        );
        assert_eq!(
            w.plugin_ui
                .notices
                .last()
                .map(|n| n.title.to_string())
                .as_deref(),
            Some("Couldn't use that global shortcut")
        );
    });
}

#[test]
fn a_press_goes_to_the_agent_that_needs_you_or_stays_put() {
    let (app, mut window, _, press_key) = open_held(None);
    press(&mut window, "d");
    let project = window.read(|w, _| w.config.project.to_string_lossy().into_owned());
    let focused = window.read(|w, _| w.focused_pane().unwrap());
    // Nobody needs you: the focus stays where it was.
    press_key.try_send(()).unwrap();
    app.run_until_parked();
    window.read(|w, _| assert_eq!(w.focused_pane(), Some(focused)));
    let other = window.read(|w, _| *w.panes.keys().find(|pane| **pane != focused).unwrap());
    window.update(|w, _, _| {
        w.agents = HashMap::from([
            (focused, agent(focused, &project, AgentState::Done)),
            (other, agent(other, &project, AgentState::Blocked)),
        ]);
    });
    // One waiting on you comes before one that's done.
    press_key.try_send(()).unwrap();
    app.run_until_parked();
    window.read(|w, _| assert_eq!(w.focused_pane(), Some(other)));
}
