//! Headless window tests for Settings' sections, Open in editor, the quick terminal and the new tab choices.

use super::{
    sound::{SoundId, SoundSettings},
    tests::{open_config, open_with, press, state},
    *,
};
use crate::{
    installed::{
        WINGS_VERSION,
        fixture::{Approval, Data},
    },
    terminal_view::{InactiveCursor, look},
};
use gpui::TestAppWindow;
use wings_editors::EditorView;

fn editor(id: &str, name: &str) -> EditorView {
    EditorView {
        id: id.into(),
        name: name.into(),
    }
}

#[test]
fn the_new_settings_are_saved_and_come_back() {
    let dir = std::env::temp_dir().join(format!("wings-native-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("workspace.json");
    {
        let (_app, mut window) = open_with(Some(file.clone()));
        window.update(|w, _, cx| {
            w.set_look(
                |look| {
                    look.line_height = 1.5;
                    look.min_contrast = 4.5;
                    look.inactive_cursor = InactiveCursor::Hidden;
                    look.cursor_blink = false;
                    look.option_as_meta = crate::terminal_view::OptionAsMeta::Both;
                    look.scrollback = 50_000;
                    look.copy_on_select = true;
                    look.clipboard_write = false;
                    look.images = false;
                },
                cx,
            );
            w.set_prefs(
                |prefs| {
                    prefs.quick_terminal = false;
                    prefs.editor = Some("zed".into());
                    prefs.new_tab = "project".into();
                    prefs.new_project_page = true;
                    prefs.agents_run_commands = true;
                    prefs.agents_message = true;
                    prefs.notify_sound = SoundSettings {
                        enabled: false,
                        sound: SoundId::Ping,
                        watching: false,
                    };
                    prefs.diff_wrap = true;
                    prefs.diff_tree = true;
                },
                cx,
            );
        });
    }
    let (_app, window) = open_with(Some(file));
    window.read(|w, cx| {
        let look = look(cx);
        assert_eq!(
            (
                look.line_height,
                look.min_contrast,
                look.inactive_cursor,
                look.scrollback
            ),
            (1.5, 4.5, InactiveCursor::Hidden, 50_000)
        );
        assert!(look.copy_on_select && !look.clipboard_write && !look.cursor_blink && !look.images);
        assert_eq!(
            look.option_as_meta,
            crate::terminal_view::OptionAsMeta::Both
        );
        assert_eq!(
            w.prefs,
            settings::Prefs {
                quick_terminal: false,
                editor: Some("zed".into()),
                new_tab: "project".into(),
                new_project_page: true,
                agents_run_commands: true,
                agents_message: true,
                notify_sound: SoundSettings {
                    enabled: false,
                    sound: SoundId::Ping,
                    watching: false,
                },
                seen_plugins: Vec::new(),
                diff_wrap: true,
                diff_tree: true,
                global_hotkey: Default::default(),
            }
        );
    });
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_plugins_section_lists_the_plugins_and_writes_plugins_json_only_for_a_change() {
    let data = Data::temp();
    data.install("demo", WINGS_VERSION, true, true, Approval::Current, "s1");
    data.install("idle", WINGS_VERSION, false, false, Approval::Current, "s2");
    data.install("new", WINGS_VERSION, false, true, Approval::Never, "s3");
    let before = std::fs::read(data.paths().state).unwrap();
    let (_app, mut window) = open_config(|config| config.plugins = Some(data.paths()));
    press(&mut window, ",");
    let rows = |window: &TestAppWindow<Workspace>| {
        window.read(|w, _| {
            w.plugin_views()
                .iter()
                .map(|view| (view.manifest.name.clone(), w.plugin_status(view)))
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(
        rows(&window),
        [
            ("Plugin demo".into(), "On · Runs here".into()),
            ("Plugin idle".into(), "Off".into()),
            ("Plugin new".into(), "Waiting for approval".into()),
        ]
    );
    window.update(|w, _, cx| {
        w.settings.as_mut().unwrap().section = settings::Section::Plugins;
        cx.notify();
    });
    window.draw();
    // Looking writes nothing.
    assert_eq!(std::fs::read(data.paths().state).unwrap(), before);
    // Turning one on writes it, as the desktop's switch does; without a native build it is on but doesn't run here.
    window.update(|w, window, cx| {
        let idle = w
            .plugin_views()
            .into_iter()
            .find(|v| v.manifest.id == "idle")
            .unwrap();
        w.toggle_plugin(idle, true, window, cx);
    });
    assert_eq!(rows(&window)[1].1, "On · No native build");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(data.paths().state).unwrap()).unwrap();
    assert_eq!(saved["idle"]["enabled"], true);
}

#[test]
fn open_in_editor_uses_the_chosen_editor_and_the_palette_offers_it_only_with_one() {
    let (_app, mut window) = open_config(|config| {
        config.editors = Some(vec![editor("vscode", "VS Code"), editor("zed", "Zed")]);
    });
    let titles = |w: &Workspace| {
        w.palette_commands()
            .into_iter()
            .map(|(title, ..)| title)
            .collect::<Vec<_>>()
    };
    window.update(|w, _, cx| {
        assert_eq!(w.default_editor().unwrap().id, "vscode");
        assert!(titles(w).contains(&"Open Project in Editor"));
        w.set_prefs(|prefs| prefs.editor = Some("zed".into()), cx);
        assert_eq!(w.default_editor().unwrap().id, "zed");
        // A choice that's no longer installed falls back to the first one.
        w.set_prefs(|prefs| prefs.editor = Some("gone".into()), cx);
        assert_eq!(w.default_editor().unwrap().id, "vscode");
    });
    let (_app, window) = open_config(|_| {});
    window.read(|w, _| {
        assert!(w.default_editor().is_none());
        assert!(!titles(w).contains(&"Open Project in Editor"));
    });
}

#[test]
fn ctrl_backquote_shows_and_hides_the_quick_terminal_with_its_shell_still_running() {
    use quick::Place;
    let (_app, mut window) = open();
    let place =
        |window: &TestAppWindow<Workspace>| window.read(|w, _| w.loose.first().map(|q| q.place));
    let shell = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            w.loose
                .first()
                .unwrap()
                .view()
                .read(cx)
                .process_id()
                .unwrap()
        })
    };
    window.draw();
    window.simulate_keystroke("ctrl-`");
    assert_eq!(place(&window), Some(Place::Floating));
    let pid = shell(&window);
    let focused = |window: &mut TestAppWindow<Workspace>| {
        window.update(|w, window, cx| {
            w.loose
                .first()
                .unwrap()
                .view()
                .focus_handle(cx)
                .is_focused(window)
        })
    };
    assert!(focused(&mut window));

    window.draw();
    window.simulate_keystroke("ctrl-`");
    assert_eq!(place(&window), Some(Place::Hidden));
    assert!(!focused(&mut window));
    window.draw();
    window.simulate_keystroke("ctrl-`");
    assert_eq!(place(&window), Some(Place::Floating));
    assert_eq!(shell(&window), pid, "hiding keeps the same shell");

    // Filling the window, it goes away with the next tab, as another tab would.
    window.update(|w, window, cx| w.place_quick(0, Place::Full, false, window, cx));
    press(&mut window, "t");
    assert_eq!(place(&window), Some(Place::Hidden));

    // Turned off, the key does nothing here.
    window.update(|w, _, cx| w.set_prefs(|prefs| prefs.quick_terminal = false, cx));
    window.draw();
    window.simulate_keystroke("ctrl-`");
    assert_eq!(place(&window), Some(Place::Hidden));

    window.update(|w, window, cx| w.close_quick(0, window, cx));
    assert_eq!(place(&window), None);
}

#[test]
fn the_new_tab_button_and_a_new_project_open_what_settings_say() {
    let dir = std::env::temp_dir().join(format!("wings-native-new-tab-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let other = dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let (_app, mut window) = open_config(|config| config.project_info = Some(dir.join("data")));
    window.update(|w, window, cx| {
        w.new_tab_button(window, cx);
        assert!(matches!(w.active, Active::Terminal(_)));
        w.set_prefs(|prefs| prefs.new_tab = "project".into(), cx);
        w.new_tab_button(window, cx);
        assert_eq!(w.active, Active::Project);

        w.set_prefs(|prefs| prefs.new_project_page = true, cx);
        w.just_added = true;
        w.switch_project(other.clone(), window, cx);
        assert_eq!(w.active, Active::Project);
        assert!(w.terminals.is_empty());
    });
    assert_eq!(state(&window).1, Active::Project);
    std::fs::remove_dir_all(dir).unwrap();
}

fn open() -> (gpui::TestApp, TestAppWindow<Workspace>) {
    open_config(|_| {})
}
