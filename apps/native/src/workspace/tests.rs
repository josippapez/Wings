//! Headless window tests: tabs, splits, focus, zoom, renaming, saving and switching projects.

use super::*;
use crate::{
    installed::{
        WINGS_VERSION,
        fixture::{Approval::Current, Data},
    },
    layout::DIVIDER,
};
use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, TestApp, TestAppWindow, point, size,
};

fn open() -> (TestApp, TestAppWindow<Workspace>) {
    open_with(None)
}

pub(super) fn open_with(state: Option<std::path::PathBuf>) -> (TestApp, TestAppWindow<Workspace>) {
    open_config(|config| config.state = state)
}

/// Never the user's saved tabs or project list unless a test points at its own files.
pub(super) fn open_config(change: impl FnOnce(&mut Config)) -> (TestApp, TestAppWindow<Workspace>) {
    open_sized(None, change)
}

/// `open_config` in a window of that size, or a screen-sized one.
pub(super) fn open_sized(
    window_size: Option<gpui::Size<gpui::Pixels>>,
    change: impl FnOnce(&mut Config),
) -> (TestApp, TestAppWindow<Workspace>) {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    app.update(crate::actions::bind);
    let dir = std::env::temp_dir();
    let args = ["--project".into(), dir.clone().into_os_string()];
    let mut config = Config::parse(args, &dir).unwrap().unwrap();
    config.state = None;
    config.projects = None;
    config.agents = None;
    config.plugins = None;
    config.project_info = None;
    config.sessions = None;
    config.editors = Some(Vec::new());
    change(&mut config);
    let build =
        move |window: &mut Window, cx: &mut Context<Workspace>| Workspace::new(config, window, cx);
    let window = match window_size {
        None => app.open_window(build),
        Some(size) => app.open_window_with_options(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                    point(px(0.0), px(0.0)),
                    size,
                ))),
                ..Default::default()
            },
            build,
        ),
    };
    (app, window)
}

#[test]
fn switching_projects_keeps_each_projects_tabs_and_shells() {
    use Active::{Issues, Terminal};
    let dir = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("wings-native-switch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (alpha, beta) = (dir.join("alpha"), dir.join("beta"));
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::create_dir_all(&beta).unwrap();
    let list = dir.join("spaces.json");
    let spaces = serde_json::json!([
        { "id": "a", "name": "alpha", "path": alpha },
        { "id": "b", "name": "beta", "path": beta, "pinned": true },
    ]);
    std::fs::write(&list, serde_json::to_vec(&spaces).unwrap()).unwrap();
    let (_app, mut window) = open_config(|config| {
        config.project = alpha.clone();
        config.projects = Some(list.clone());
    });
    let names = window.read(|w, _| {
        w.projects
            .iter()
            .map(|p| p.name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(names, ["beta", "alpha"]);
    press(&mut window, "d");
    let shell = window.read(|w, cx| w.terminal(0).unwrap().read(cx).process_id().unwrap());

    // The pinned project is the first row: below the 40px title bar, the nav's four 28px rows 1px apart and its 8px of
    // padding and the 32px Projects heading, rows of 24px and their 1px border, 1px apart.
    let nav = 4.0 * 28.0 + 3.0;
    let row = |index: f32| point(px(60.0), px(40.0 + nav + 8.0 + 32.0 + 13.0 + index * 27.0));
    window.draw();
    window.simulate_click(row(0.0), MouseButton::Left);
    assert_eq!(window.read(|w, _| w.project().to_path_buf()), beta);
    assert_eq!(state(&window), (vec![Terminal(1), Issues], Terminal(1)));
    assert!(
        running(shell),
        "alpha's shells keep running while beta is shown"
    );

    window.draw();
    window.simulate_click(row(1.0), MouseButton::Left);
    assert_eq!(window.read(|w, _| w.project().to_path_buf()), alpha);
    assert_eq!(state(&window), (vec![Terminal(0), Issues], Terminal(0)));
    assert_eq!(panes(&window), (vec![0, 1], 1));

    press(&mut window, "b");
    assert!(!window.read(|w, _| w.sidebar));
    std::fs::remove_dir_all(dir).unwrap();
}

pub(super) fn press(window: &mut TestAppWindow<Workspace>, key: &str) {
    window.draw();
    let prefix = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl-shift"
    };
    window.simulate_keystroke(&format!("{prefix}-{key}"));
}

/// Cmd-Shift on macOS, Ctrl-Shift-Alt elsewhere: the desktop's "extra" chord.
fn press_extra(window: &mut TestAppWindow<Workspace>, key: &str) {
    press(
        window,
        &if cfg!(target_os = "macos") {
            format!("shift-{key}")
        } else {
            format!("alt-{key}")
        },
    );
}

fn panes(window: &TestAppWindow<Workspace>) -> (Vec<usize>, usize) {
    window.read(|workspace, _| workspace.panes(0).unwrap())
}

fn running(pid: u32) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string()])
        .output()
        .unwrap()
        .status
        .success()
}

#[test]
fn a_terminals_output_draws_neither_the_main_column_nor_the_other_terminal_again() {
    let (_app, mut window) = open();
    press(&mut window, "d");
    window.draw();
    let (busy, other) = window.read(|w, _| (w.terminal(0).unwrap(), w.terminal(1).unwrap()));
    let drawn = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            let main = w.parts.as_ref().expect("the pieces").main.read(cx).drawn;
            (main, busy.read(cx).drawn(), other.read(cx).drawn())
        })
    };
    let before = drawn(&window);
    window.update(|_, _, cx| busy.update(cx, |_, cx| cx.notify()));
    window.draw();
    let (main, busy_drawn, other_drawn) = drawn(&window);
    assert_eq!(busy_drawn, before.1 + 1, "the busy terminal");
    assert_eq!(main, before.0, "the main column kept its drawing");
    assert_eq!(other_drawn, before.2, "the other terminal kept its drawing");
}

#[test]
fn splits_move_focus_by_position_and_closing_a_pane_ends_its_shell() {
    let (app, mut window) = open();
    assert_eq!(panes(&window), (vec![0], 0));
    press(&mut window, "d");
    assert_eq!(panes(&window), (vec![0, 1], 1));
    press_extra(&mut window, "d");
    assert_eq!(panes(&window), (vec![0, 1, 2], 2));
    // 0 on the left; 1 above 2 on the right.
    press(&mut window, "alt-left");
    assert_eq!(panes(&window).1, 0);
    press(&mut window, "alt-right");
    assert_eq!(panes(&window).1, 1);
    press(&mut window, "alt-down");
    assert_eq!(panes(&window).1, 2);

    let shell = window.read(|workspace, cx| {
        let terminal = workspace.terminal(2).unwrap();
        terminal.read(cx).process_id().unwrap()
    });
    assert!(running(shell));
    press(&mut window, "w");
    assert_eq!(panes(&window), (vec![0, 1], 1));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while running(shell) {
        assert!(
            std::time::Instant::now() < deadline,
            "the closed pane's shell is still running"
        );
        app.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    press(&mut window, "w");
    press(&mut window, "w");
    assert_eq!(state(&window), (vec![Active::Issues], Active::Issues));
}

#[test]
fn a_pane_whose_shell_exits_closes_and_the_pane_before_it_takes_focus() {
    let (app, mut window) = open();
    press(&mut window, "d");
    assert_eq!(panes(&window), (vec![0, 1], 1));
    window.update(|w, _, cx| {
        w.terminal(1)
            .unwrap()
            .update(cx, |view, cx| view.write_input("exit\r", cx))
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while panes(&window).0 != [0] {
        assert!(
            std::time::Instant::now() < deadline,
            "the exited pane is still there"
        );
        // The redraw that carries the exit waits on the clock.
        app.advance_clock(std::time::Duration::from_millis(20));
        app.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(panes(&window), (vec![0], 0));
    window.update(|w, window, cx| {
        assert!(w.terminal(0).unwrap().focus_handle(cx).is_focused(window));
    });
}

#[test]
fn typing_and_selecting_still_work_in_a_pane_that_was_not_redrawn() {
    let (app, mut window) = open();
    press(&mut window, "d");
    // Several quiet frames, so the panes are reused from the cache.
    for _ in 0..5 {
        app.run_until_parked();
        window.draw();
    }
    window.simulate_input("echo cached-''ok");
    window.simulate_keystroke("enter");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let output = |window: &TestAppWindow<Workspace>| {
        window.read(|workspace, cx| workspace.terminal(1).unwrap().read(cx).recent_output())
    };
    while !String::from_utf8_lossy(&output(&window)).contains("cached-ok") {
        assert!(
            std::time::Instant::now() < deadline,
            "typing never reached pane 1"
        );
        app.run_until_parked();
        app.advance_clock(std::time::Duration::from_millis(50));
        window.draw();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for _ in 0..5 {
        app.run_until_parked();
        window.draw();
    }
    // Clicking in pane 0 focuses it, through the cached pane's own mouse handler.
    let area = window.read(|workspace, _| workspace.area.unwrap());
    let in_pane_0 = point(area.left() + px(60.0), area.top() + px(120.0));
    window.simulate_mouse_down(in_pane_0, MouseButton::Left);
    window.simulate_mouse_up(in_pane_0, MouseButton::Left);
    window.draw();
    assert_eq!(panes(&window).1, 0);
}

#[test]
fn a_terminal_of_a_project_with_an_account_starts_with_that_accounts_config_folder() {
    let dir = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("wings-native-account-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let alpha = dir.join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let list = dir.join("spaces.json");
    let spaces = serde_json::json!([
        { "id": alpha, "name": "alpha", "path": alpha, "claudeAccount": "work" },
    ]);
    std::fs::write(&list, serde_json::to_vec(&spaces).unwrap()).unwrap();
    let (app, mut window) = open_config(|config| {
        config.project = alpha.clone();
        config.projects = Some(list.clone());
    });
    window.draw();
    window.simulate_input("echo account-''dir:$CLAUDE_CONFIG_DIR");
    window.simulate_keystroke("enter");
    let expected = format!(
        "account-dir:{}",
        wings_spaces::claude_accounts::dir(Some("work")).display()
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let output =
            window.read(|workspace, cx| workspace.terminal(0).unwrap().read(cx).recent_output());
        if String::from_utf8_lossy(&output).contains(&expected) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the shell never printed {expected}"
        );
        app.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn back_and_forward_return_to_the_tabs_shown_before() {
    use Active::Terminal;
    let (_app, mut window) = open();
    press(&mut window, "t");
    press(&mut window, "1");
    let go = |window: &mut TestAppWindow<Workspace>, key: &str| {
        window.draw();
        window.simulate_keystroke(&if cfg!(target_os = "macos") {
            format!("cmd-alt-{key}")
        } else {
            format!("ctrl-shift-alt-{key}")
        });
        window.read(|w, _| w.active())
    };
    assert_eq!(go(&mut window, "["), Terminal(1));
    assert_eq!(go(&mut window, "["), Terminal(0));
    assert_eq!(go(&mut window, "["), Terminal(0), "nothing further back");
    assert_eq!(go(&mut window, "]"), Terminal(1));
    // A closed tab is skipped.
    press(&mut window, "t");
    press(&mut window, "1");
    window.draw();
    window.update(|w, window, cx| w.close_tab_of(Terminal(2), window, cx));
    assert_eq!(go(&mut window, "["), Terminal(1));
}

#[test]
fn moving_a_tab_swaps_it_with_its_neighbour_and_issues_stays_last() {
    use Active::{Issues, Terminal};
    let (_app, mut window) = open();
    press(&mut window, "t");
    press(&mut window, "t");
    let dispatch = |window: &mut TestAppWindow<Workspace>, action: Box<dyn gpui::Action>| {
        window.draw();
        window.update(|_, window, cx| window.dispatch_action(action, cx));
    };
    dispatch(&mut window, Box::new(crate::actions::MoveTabLeft));
    assert_eq!(
        state(&window).0,
        vec![Terminal(0), Terminal(2), Terminal(1), Issues]
    );
    dispatch(&mut window, Box::new(crate::actions::MoveTabLeft));
    dispatch(&mut window, Box::new(crate::actions::MoveTabLeft));
    assert_eq!(
        state(&window),
        (
            vec![Terminal(2), Terminal(0), Terminal(1), Issues],
            Terminal(2)
        )
    );
    press(&mut window, "3");
    dispatch(&mut window, Box::new(crate::actions::MoveTabRight));
    assert_eq!(
        state(&window).0,
        vec![Terminal(2), Terminal(0), Terminal(1), Issues]
    );
}

#[test]
fn double_clicking_a_title_renames_the_pane_and_the_tab() {
    let (_app, mut window) = open();
    window.draw();
    let area = window.read(|workspace, _| workspace.area.unwrap());
    let title = point(area.left() + px(40.0), area.top() + px(15.0));
    let rename = |window: &mut TestAppWindow<Workspace>, typed: &str, key: &str| {
        window.simulate_event(MouseDownEvent {
            position: title,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 2,
            first_mouse: false,
        });
        window.simulate_mouse_up(title, MouseButton::Left);
        window.draw();
        window.simulate_input(typed);
        window.simulate_keystroke(key);
        window.draw();
    };
    let label = |window: &TestAppWindow<Workspace>| {
        window.read(|workspace, cx| workspace.label(0, cx).to_string())
    };
    rename(&mut window, "server", "enter");
    assert_eq!(label(&window), "server");
    rename(&mut window, "ignored", "escape");
    assert_eq!(label(&window), "server");
    window.update(|workspace, window, cx| {
        workspace.start_rename(0, window, cx);
    });
    window.draw();
    window.simulate_keystroke("backspace");
    window.simulate_keystroke("enter");
    assert_eq!(label(&window), "Terminal");
}

fn zoomed(window: &TestAppWindow<Workspace>) -> Option<usize> {
    window.read(|workspace, _| workspace.terminals[0].zoomed)
}

fn zoom(window: &mut TestAppWindow<Workspace>) {
    window.draw();
    window.update(|_, window, cx| window.dispatch_action(Box::new(crate::actions::ZoomPane), cx));
}

#[test]
fn a_zoomed_pane_fills_the_tab_until_a_split_or_another_pane_takes_focus() {
    let (_app, mut window) = open();
    press(&mut window, "d");
    zoom(&mut window);
    assert_eq!(zoomed(&window), Some(1));
    // Nothing beside it on screen, so focus stays.
    press(&mut window, "alt-left");
    assert_eq!((panes(&window).1, zoomed(&window)), (1, Some(1)));
    zoom(&mut window);
    assert_eq!(zoomed(&window), None);

    zoom(&mut window);
    press(&mut window, "alt-left");
    assert_eq!(panes(&window).1, 1);
    window.update(|workspace, window, cx| workspace.focus_pane(0, window, cx));
    assert_eq!((panes(&window).1, zoomed(&window)), (0, None));

    zoom(&mut window);
    assert_eq!(zoomed(&window), Some(0));
    press(&mut window, "d");
    assert_eq!(
        (panes(&window), zoomed(&window)),
        ((vec![0, 2, 1], 2), None)
    );
}

#[test]
fn dragging_a_divider_resizes_the_split_within_its_limits() {
    let (_app, mut window) = open();
    press(&mut window, "d");
    window.draw();
    let area = window.read(|workspace, _| workspace.area.unwrap());
    let y = area.top() + area.size.height / 2.0;
    let x = |fraction: f32| area.left() + area.size.width * fraction;
    let ratio = |window: &TestAppWindow<Workspace>| {
        window.read(|workspace, _| match &workspace.terminals[0].layout {
            Layout::Split { ratio, .. } => *ratio,
            Layout::Pane(_) => panic!("a split"),
        })
    };
    let drag = |window: &mut TestAppWindow<Workspace>, to: f32| {
        let on_divider = point(x(ratio(window)) + px(DIVIDER / 2.0), y);
        window.simulate_mouse_down(on_divider, MouseButton::Left);
        window.simulate_event(MouseMoveEvent {
            position: point(x(to), y),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        });
        window.simulate_mouse_up(point(x(to), y), MouseButton::Left);
        window.draw();
    };
    drag(&mut window, 0.3);
    assert!((ratio(&window) - 0.3).abs() < 0.01, "{}", ratio(&window));
    drag(&mut window, 0.02);
    assert_eq!(ratio(&window), 0.15);
}

fn wait_for_cwd(app: &TestApp, window: &TestAppWindow<Workspace>, pane: usize) {
    let tmp = std::path::Path::new("/tmp").canonicalize().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let cwd = window.read(|workspace, cx| workspace.terminal(pane)?.read(cx).cwd());
        if cwd.and_then(|cwd| cwd.canonicalize().ok()).as_ref() == Some(&tmp) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pane {pane} never reached /tmp"
        );
        app.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn wait_for_output(app: &TestApp, window: &TestAppWindow<Workspace>, pane: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let output =
            window.read(|workspace, cx| workspace.terminal(pane).unwrap().read(cx).recent_output());
        if String::from_utf8_lossy(&output).contains("history-marker") {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pane {pane} never showed the marker"
        );
        app.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn tabs_splits_and_focus_come_back_after_a_relaunch() {
    use Active::{Issues, Terminal};
    let dir = std::env::temp_dir().join(format!("wings-native-restore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("workspace.json");
    {
        let (app, mut window) = open_with(Some(file.clone()));
        press(&mut window, "d");
        press(&mut window, "alt-left");
        press(&mut window, "t");
        press(&mut window, "1");
        assert_eq!(panes(&window), (vec![0, 1], 0));
        window.draw();
        window.simulate_input("cd /tmp; echo history-''marker");
        window.simulate_keystroke("enter");
        wait_for_cwd(&app, &window, 0);
        wait_for_output(&app, &window, 0);
        // Any change saves, and it reads each shell's folder as it does.
        press(&mut window, "1");
        // What quitting does after that.
        window.read(|workspace, cx| workspace.save_histories(cx));
    }
    let (app, window) = open_with(Some(file.clone()));
    wait_for_cwd(&app, &window, 0);
    // Replayed before the new shell prints anything.
    wait_for_output(&app, &window, 0);
    assert_eq!(
        state(&window),
        (vec![Terminal(0), Terminal(1), Issues], Terminal(0))
    );
    assert_eq!(panes(&window), (vec![0, 1], 0));
    window.read(|workspace, _| {
        assert!(matches!(
            workspace.terminals[0].layout,
            Layout::Split { dir: Dir::Row, .. }
        ));
        assert_eq!(workspace.panes(1), Some((vec![2], 2)));
    });
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn font_size_steps_within_the_desktop_range_and_comes_back_after_a_relaunch() {
    let dir = std::env::temp_dir().join(format!("wings-native-font-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("workspace.json");
    let size = |window: &TestAppWindow<Workspace>| window.read(|_, cx| font_size(cx));
    {
        let (_app, mut window) = open_with(Some(file.clone()));
        assert_eq!(size(&window), 13.0);
        press(&mut window, "=");
        assert_eq!(size(&window), 14.0);
        for _ in 0..10 {
            press(&mut window, "-");
        }
        assert_eq!(size(&window), 9.0);
        press(&mut window, "0");
        assert_eq!(size(&window), 13.0);
        for _ in 0..20 {
            press(&mut window, "=");
        }
        assert_eq!(size(&window), 24.0);
        press(&mut window, "0");
        press(&mut window, "=");
        press(&mut window, "=");
    }
    let (_app, window) = open_with(Some(file));
    assert_eq!(size(&window), 15.0);
    std::fs::remove_dir_all(dir).unwrap();
}

pub(super) fn state(window: &TestAppWindow<Workspace>) -> (Vec<Active>, Active) {
    window.read(|workspace, _| (workspace.tabs(), workspace.active()))
}

#[test]
fn tab_shortcuts_open_cycle_jump_and_close_like_the_desktop() {
    use Active::{Issues, Terminal};
    let (_app, mut window) = open();
    assert_eq!(state(&window), (vec![Terminal(0), Issues], Terminal(0)));
    press(&mut window, "t");
    assert_eq!(
        state(&window),
        (vec![Terminal(0), Terminal(1), Issues], Terminal(1))
    );
    press(&mut window, "]");
    assert_eq!(state(&window).1, Issues);
    press(&mut window, "]");
    assert_eq!(state(&window).1, Terminal(0));
    press(&mut window, "[");
    assert_eq!(state(&window).1, Issues);
    press(&mut window, "2");
    assert_eq!(state(&window).1, Terminal(1));
    press(&mut window, "9");
    assert_eq!(state(&window).1, Terminal(1));
    window.read(|workspace, cx| assert_eq!(workspace.label(1, cx), "Terminal"));

    press(&mut window, "w");
    assert_eq!(state(&window), (vec![Terminal(0), Issues], Terminal(0)));
    press(&mut window, "w");
    assert_eq!(state(&window), (vec![Issues], Issues));
    // Issues closes too, and a terminal takes over the empty strip.
    press(&mut window, "w");
    assert_eq!(state(&window), (vec![Terminal(2)], Terminal(2)));
    press(&mut window, "t");
    assert_eq!(
        state(&window),
        (vec![Terminal(2), Terminal(3)], Terminal(3))
    );
}

#[test]
fn a_closed_issues_tab_stays_closed_after_a_relaunch_and_opening_issues_brings_it_back() {
    use Active::{Issues, Terminal};
    let dir = std::env::temp_dir().join(format!("wings-native-issues-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("workspace.json");
    {
        let (_app, mut window) = open_with(Some(file.clone()));
        window.update(|w, window, cx| w.show(Issues, window, cx));
        press(&mut window, "w");
        assert_eq!(state(&window), (vec![Terminal(0)], Terminal(0)));
    }
    let (_app, mut window) = open_with(Some(file));
    assert!(!state(&window).0.contains(&Issues));
    press(&mut window, "k");
    window.simulate_input("Issues");
    window.simulate_keystroke("enter");
    assert_eq!(state(&window).0.last(), Some(&Issues));
    std::fs::remove_dir_all(dir).unwrap();
}

fn assert_active_tab_is_visible(window: &mut TestAppWindow<Workspace>) {
    window.draw();
    window.read(|workspace, _| {
        let index = workspace
            .tabs()
            .iter()
            .position(|tab| *tab == workspace.active())
            .unwrap();
        let tab = workspace.tab_scroll.bounds_for_item(index).unwrap();
        let strip = workspace.tab_scroll.bounds();
        let offset = workspace.tab_scroll.offset();
        assert!(tab.left() + offset.x >= strip.left());
        assert!(tab.right() + offset.x <= strip.right());
    });
}

#[test]
fn overflowing_tabs_keep_the_selected_tab_in_the_visible_strip() {
    for width in [640.0, 1000.0] {
        let (_app, mut window) = open_sized(Some(size(px(width), px(640.0))), |_| {});
        for _ in 0..8 {
            press(&mut window, "t");
        }
        assert_active_tab_is_visible(&mut window);

        press(&mut window, "]");
        assert_active_tab_is_visible(&mut window);

        if width > 640.0 {
            window.simulate_resize(size(px(640.0), px(640.0)));
            assert_active_tab_is_visible(&mut window);
        }

        press(&mut window, "]");
        assert_active_tab_is_visible(&mut window);

        press(&mut window, "[");
        assert_active_tab_is_visible(&mut window);
    }
}

#[test]
fn an_unrelated_redraw_does_not_reset_the_tab_strip_scroll_position() {
    let (_app, mut window) = open_sized(Some(size(px(640.0), px(640.0))), |_| {});
    for _ in 0..8 {
        press(&mut window, "t");
    }
    assert_active_tab_is_visible(&mut window);

    window.update(|workspace, _, cx| {
        workspace.tab_scroll.set_offset(point(px(0.0), px(0.0)));
        cx.notify();
    });
    window.draw();

    window.read(|workspace, _| assert_eq!(workspace.tab_scroll.offset().x, px(0.0)));
}

#[test]
fn a_claude_in_a_pane_shows_its_name_and_state_on_the_pane_tab_and_project() {
    use crate::agents::AgentState;
    let dir = std::env::temp_dir().join(format!("wings-native-agents-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let claude_dir = dir.join("claude");
    std::fs::create_dir_all(claude_dir.join("sessions")).unwrap();
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let fake = dir.join("bin/claude");
    std::os::unix::fs::symlink("/bin/sleep", &fake).unwrap();
    let (app, mut window) = open_config(|config| config.agents = Some(claude_dir.clone()));
    window.draw();
    // A shell's pane is named after the program it runs.
    wait_for(
        &app,
        &window,
        |w| w.pane_info.contains_key(&0),
        "the shell was never detected",
    );
    window.read(|w, cx| assert_eq!(w.pane_title(0, cx), w.pane_info[&0].command.as_str()));

    window.simulate_input(&format!("exec '{}' 30", fake.display()));
    window.simulate_keystroke("enter");
    wait_for(
        &app,
        &window,
        |w| w.agents.contains_key(&0),
        "the agent was never detected",
    );
    let pid = window.read(|w, _| w.agents[&0].pid);
    std::fs::write(
        claude_dir.join(format!("sessions/{pid}.json")),
        r#"{"status":"waiting","name":"fix the tabs"}"#,
    )
    .unwrap();
    wait_for(
        &app,
        &window,
        |w| w.agents[&0].state == AgentState::Blocked,
        "the session's state never showed",
    );
    window.read(|w, cx| {
        assert_eq!(w.pane_title(0, cx), "fix the tabs");
        assert_eq!(w.tab_state(0), Some(AgentState::Blocked));
        assert_eq!(w.project_agents(&w.config.project), [AgentState::Blocked]);
    });
    window.draw();
    let _ = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status();
    std::fs::remove_dir_all(dir).unwrap();
}

/// Runs the app until `done`, while real processes and the detection clock move on.
pub(super) fn wait_for(
    app: &TestApp,
    window: &TestAppWindow<Workspace>,
    done: impl Fn(&Workspace) -> bool,
    what: &str,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !window.read(|workspace, _| done(workspace)) {
        assert!(std::time::Instant::now() < deadline, "{what}");
        app.run_until_parked();
        app.advance_clock(crate::agents::EVERY);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn settings_open_with_the_shortcut_and_change_every_terminal() {
    use crate::terminal_view::{DEFAULT_FONT_FAMILY, look};
    let (_app, mut window) = open();
    press(&mut window, ",");
    assert!(window.read(|w, _| w.settings.is_some()));

    window.update(|w, _, cx| w.set_shift_return_meta(false, cx));
    assert!(!window.read(|_, cx| look(cx).shift_return_meta));
    window.update(|w, _, cx| {
        w.set_look(
            |look| look.cursor = crate::terminal_view::CursorShape::Beam,
            cx,
        )
    });
    assert_eq!(
        window.read(|_, cx| look(cx).cursor),
        crate::terminal_view::CursorShape::Beam
    );

    let type_family = |window: &mut TestAppWindow<Workspace>, family: &str| {
        window.update(|w, _, cx| {
            let field = w.settings.as_ref().unwrap().family.clone();
            field.update(cx, |field, _| field.text = family.into());
            w.apply_family(cx);
        });
    };
    type_family(&mut window, "No Such Family 4f2a");
    window.read(|w, cx| {
        assert_eq!(look(cx).family, DEFAULT_FONT_FAMILY);
        assert!(w.settings.as_ref().unwrap().family_error.is_some());
    });
    // Headless windows have no fonts installed, so clearing the field stands in for a good name.
    type_family(&mut window, "");
    window.read(|w, cx| {
        assert_eq!(look(cx).family, DEFAULT_FONT_FAMILY);
        assert!(w.settings.as_ref().unwrap().family_error.is_none());
    });

    window.draw();
    window.simulate_keystroke("escape");
    assert!(window.read(|w, _| w.settings.is_none()));
}

/// The sidebar's Plugins row for the only page, in a 1000 x 640 window: the settings row (28px, 4px above it and 8px
/// under it) is last, and above it the section's 8px of padding under the 24px row.
const PLUGIN_ROW: (f32, f32) = (60.0, 640.0 - 8.0 - 28.0 - 4.0 - 8.0 - 12.0);
/// Where the fixture page's button is in that window: 248px of sidebar and the card's 8px gutter and border, then below
/// the 40px title bar, the 36px project bar and the card's border, the page's 44px toolbar and 14px of padding.
const PAGE_BUTTON: (f32, f32) = (248.0 + 9.0 + 22.0, 40.0 + 36.0 + 1.0 + 44.0 + 14.0 + 16.0);

/// A workspace whose desktop data folder holds the fixture plugin, turned on and approved, as the plugin `demo`.
fn open_with_plugin(
    data: &Data,
    state: Option<std::path::PathBuf>,
) -> (TestApp, TestAppWindow<Workspace>) {
    let (app, mut window) = open_sized(Some(size(px(1000.0), px(640.0))), |config| {
        config.plugins = Some(data.paths());
        config.state = state;
    });
    window.draw();
    (app, window)
}

fn install_demo(data: &Data, enabled: bool) {
    data.install("demo", WINGS_VERSION, true, enabled, Current, "demo-scope");
}

fn page_text(window: &TestAppWindow<Workspace>) -> String {
    window.read(|w, cx| {
        w.plugin_tabs
            .first()
            .map(|tab| tab.page.read(cx).text())
            .unwrap_or_default()
    })
}

/// The page runs on its own thread, so this waits in real time for it to show `text`.
fn wait_for_page(app: &TestApp, window: &mut TestAppWindow<Workspace>, text: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !page_text(window).contains(text) {
        assert!(
            std::time::Instant::now() < deadline,
            "the page shows {:?}, not {text:?}",
            page_text(window)
        );
        app.run_until_parked();
        window.draw();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn frames(window: &TestAppWindow<Workspace>) -> usize {
    window.read(|w, _| w.plugins[0].host.frames())
}

#[test]
fn a_plugin_page_opens_from_the_sidebar_works_with_its_main_and_stops_when_its_tab_closes() {
    use Active::{Issues, Plugin, Terminal};
    let data = Data::temp();
    install_demo(&data, true);
    let (app, mut window) = open_with_plugin(&data, None);
    // The plugin's main runs, and of its three views the two with a native page can open as a tab.
    assert_eq!(
        window.read(|w, _| w.plugins.iter().map(|p| p.pages.len()).collect::<Vec<_>>()),
        [2]
    );
    assert_eq!(frames(&window), 1);
    assert_eq!(state(&window), (vec![Terminal(0), Issues], Terminal(0)));

    window.simulate_click(point(px(PLUGIN_ROW.0), px(PLUGIN_ROW.1)), MouseButton::Left);
    assert_eq!(
        state(&window),
        (vec![Terminal(0), Plugin(1), Issues], Plugin(1))
    );
    assert_eq!(
        window.read(|w, _| w.plugin_tabs[0].title.to_string()),
        "Fixture"
    );
    wait_for_page(&app, &mut window, "Count: 0");
    assert_eq!(frames(&window), 2);

    // Its button asks main, which saves the count in the desktop's storage under the desktop's scope.
    window.simulate_click(
        point(px(PAGE_BUTTON.0), px(PAGE_BUTTON.1)),
        MouseButton::Left,
    );
    wait_for_page(&app, &mut window, "Count: 1");
    assert_eq!(
        data.saved("demo", "demo-scope", "count"),
        Some(serde_json::json!(1))
    );

    // The same page again shows its tab, and the tab strip cycles through it.
    window.draw();
    window.simulate_click(point(px(PLUGIN_ROW.0), px(PLUGIN_ROW.1)), MouseButton::Left);
    assert_eq!(window.read(|w, _| w.plugin_tabs.len()), 1);
    press(&mut window, "]");
    assert_eq!(state(&window).1, Issues);
    press(&mut window, "[");
    assert_eq!(state(&window).1, Plugin(1));

    // Closing the tab drops the page and its frame, and the terminal next to it is shown. Main goes on.
    press(&mut window, "w");
    assert_eq!(state(&window), (vec![Terminal(0), Issues], Terminal(0)));
    window.draw();
    app.run_until_parked();
    assert_eq!(frames(&window), 1);
}

#[test]
fn plugin_pages_come_back_after_a_relaunch_unless_their_plugin_is_off() {
    use Active::{Issues, Plugin, Terminal};
    let dir = std::env::temp_dir().join(format!("wings-native-plugin-tabs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("workspace.json");
    let data = Data::temp();
    install_demo(&data, true);
    {
        let (_app, mut window) = open_with_plugin(&data, Some(file.clone()));
        window.simulate_click(point(px(PLUGIN_ROW.0), px(PLUGIN_ROW.1)), MouseButton::Left);
        press(&mut window, "t");
        press(&mut window, "3");
        assert_eq!(
            state(&window),
            (vec![Terminal(0), Terminal(2), Plugin(1), Issues], Plugin(1))
        );
    }
    {
        let (app, mut window) = open_with_plugin(&data, Some(file.clone()));
        assert_eq!(
            state(&window),
            (vec![Terminal(0), Terminal(1), Plugin(2), Issues], Plugin(2))
        );
        wait_for_page(&app, &mut window, "Count: 0");
    }
    // Turned off in the desktop's manager, the page has no tab, and what was shown falls back to Issues.
    install_demo(&data, false);
    let (_app, window) = open_with_plugin(&data, Some(file));
    assert_eq!(
        state(&window),
        (vec![Terminal(0), Terminal(1), Issues], Issues)
    );
    assert!(window.read(|w, _| w.plugins.is_empty()));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn switching_projects_keeps_each_projects_plugin_pages_open() {
    use Active::{Issues, Plugin, Terminal};
    let dir = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("wings-native-plugin-switch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (alpha, beta) = (dir.join("alpha"), dir.join("beta"));
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::create_dir_all(&beta).unwrap();
    let data = Data::temp();
    install_demo(&data, true);
    let (_app, mut window) = open_config(|config| {
        config.project = alpha.clone();
        config.plugins = Some(data.paths());
    });
    window.update(|w, window, cx| assert!(w.open_plugin_view("demo", "fixture", None, window, cx)));
    assert_eq!(state(&window).1, Plugin(1));

    window.update(|w, window, cx| w.switch_project(beta.clone(), window, cx));
    assert_eq!(state(&window), (vec![Terminal(2), Issues], Terminal(2)));
    // The page of the project that isn't shown keeps running.
    assert_eq!(frames(&window), 2);

    window.update(|w, window, cx| w.switch_project(alpha.clone(), window, cx));
    assert_eq!(
        state(&window),
        (vec![Terminal(0), Plugin(1), Issues], Plugin(1))
    );
    window.update(|w, window, cx| assert!(!w.open_plugin_view("demo", "later", None, window, cx)));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn right_clicking_a_pane_title_opens_its_menu_and_escape_closes_it() {
    let (_app, mut window) = open();
    window.draw();
    let area = window.read(|workspace, _| workspace.area.unwrap());
    window.simulate_event(MouseDownEvent {
        position: point(area.left() + px(40.0), area.top() + px(15.0)),
        button: MouseButton::Right,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    });
    window.draw();
    assert_eq!(
        window.read(|w, _| w.menu.as_ref().map(|menu| menu.target.clone())),
        Some(context_menu::MenuTarget::Pane(0))
    );
    window.simulate_keystroke("escape");
    assert!(window.read(|w, _| w.menu.is_none()));
}

#[test]
fn moving_a_pane_to_another_tab_puts_it_beside_that_tabs_focused_pane() {
    use Active::{Issues, Terminal};
    let (_app, mut window) = open();
    press(&mut window, "d");
    press(&mut window, "t");
    // Pane 1 leaves the first tab for the second, where it sits beside pane 2 and is shown.
    window.update(|w, window, cx| w.move_pane_to(1, Some(1), window, cx));
    assert_eq!(
        state(&window),
        (vec![Terminal(0), Terminal(1), Issues], Terminal(1))
    );
    window.read(|w, _| {
        assert_eq!(w.terminals[0].layout.panes(), [0]);
        assert_eq!(
            (w.terminals[1].layout.panes(), w.terminals[1].focused),
            (vec![2, 1], 1)
        );
    });
    // The last pane of a tab takes the tab with it.
    window.update(|w, window, cx| w.move_pane_to(0, None, window, cx));
    assert_eq!(
        state(&window),
        (vec![Terminal(1), Terminal(2), Issues], Terminal(2))
    );
}

#[test]
fn an_agent_that_comes_to_need_you_or_finishes_goes_to_the_inbox_read_if_you_are_looking_at_it() {
    use crate::agents::{AgentKind, AgentState};
    let (_app, mut window) = open();
    press(&mut window, "d");
    let agent = |pane: usize, state: AgentState| crate::agents::Agent {
        pane_id: pane.to_string(),
        space_id: String::new(),
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
    };
    window.update(|w, _, _| {
        w.window_active = true;
        let before = HashMap::from([
            (0, agent(0, AgentState::Working)),
            (1, agent(1, AgentState::Working)),
        ]);
        w.agents = HashMap::from([
            (0, agent(0, AgentState::Blocked)),
            (1, agent(1, AgentState::Done)),
        ]);
        w.inbox_agents(&before);
        // Nothing changed this time.
        let before = w.agents.clone();
        w.inbox_agents(&before);
    });
    window.read(|w, _| {
        let events: Vec<_> = w
            .inbox
            .events
            .iter()
            .map(|e| (e.title.to_string(), e.detail.clone(), e.pane, e.read))
            .collect();
        // Newest first; pane 1, which has focus, counts as seen.
        assert_eq!(
            events,
            [
                ("Claude finished".to_string(), String::new(), Some(1), true),
                (
                    "Claude needs you".to_string(),
                    "waiting for input".to_string(),
                    Some(0),
                    false
                ),
            ]
        );
    });
}

#[test]
fn a_new_notification_plays_the_chosen_sound_as_settings_say_and_once_per_burst() {
    use super::sound::{PLAYED, SoundId};
    use crate::agents::{AgentKind, AgentState};
    let played = || PLAYED.with(|played| std::mem::take(&mut *played.borrow_mut()));
    let (_app, mut window) = open();
    press(&mut window, "d");
    let agent = |pane: usize, state: AgentState| crate::agents::Agent {
        pane_id: pane.to_string(),
        space_id: String::new(),
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
    };
    // Pane 1 has focus; pane 0 isn't the one you're looking at.
    let change = |w: &mut Workspace, pane: usize, from: AgentState, to: AgentState| {
        let before = HashMap::from([(pane, agent(pane, from))]);
        w.agents = HashMap::from([(pane, agent(pane, to))]);
        w.inbox_agents(&before);
    };
    window.update(|w, _, _| {
        w.window_active = true;
        played();
        w.prefs.notify_sound.sound = SoundId::Drop;

        w.prefs.notify_sound.enabled = false;
        change(w, 0, AgentState::Working, AgentState::Blocked);
        assert_eq!(played(), [], "sound off");

        w.prefs.notify_sound.enabled = true;
        w.prefs.notify_sound.watching = false;
        change(w, 1, AgentState::Working, AgentState::Done);
        assert_eq!(played(), [], "the pane on screen, with that turned off");

        change(w, 0, AgentState::Blocked, AgentState::Done);
        assert_eq!(played(), [SoundId::Drop]);

        // Within a second of the last one, a plugin's notification shares that sound.
        w.inbox_plugin(crate::page::plugin::Notification {
            plugin: "ci".into(),
            name: "Build bot".into(),
            title: "Build passed".into(),
            body: String::new(),
            project: None,
            buttons: Vec::new(),
        });
        assert_eq!(played(), []);
        assert_eq!(w.inbox.events.len(), 4);
    });
}

/// Fourteen tabs don't fit in 1000 points: they scroll, and the + button after them stays in the window.
#[test]
fn overflowing_tabs_scroll_inside_the_window() {
    let (_app, mut window) = open_sized(Some(size(px(1000.0), px(640.0))), |_| {});
    for _ in 0..14 {
        press(&mut window, "t");
    }
    window.draw();
    window.read(|workspace, _| {
        // The + button and its menu's chevron take 44 points after the strip.
        assert!(workspace.tab_scroll.bounds().right() <= px(1000.0 - 44.0));
    });
}
