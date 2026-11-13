//! Headless window tests for the sidebar's top rows, as the desktop's `nav aria-label="Wings"`: Search, Terminal, the
//! Inbox and Past sessions, and the terminals the Terminal row opens, listed under them.

use super::*;
use crate::workspace::{
    quick::Place,
    sidebar::{NAV_GAP, NAV_ROW},
    tests::{open_with, press, state},
};
use gpui::{Entity, MouseButton, Point, TestApp, TestAppWindow, point};
use std::time::{Duration, Instant};

fn open() -> (TestApp, TestAppWindow<Workspace>) {
    open_with(None)
}

/// Below the 40px title bar, the middle of the nav's `index`th row.
fn nav_row(index: usize) -> Point<Pixels> {
    point(
        px(60.0),
        px(40.0 + index as f32 * (NAV_ROW + NAV_GAP) + NAV_ROW / 2.0),
    )
}

/// The `index`th row of the Terminals list, under the nav's four rows and its 8px of padding.
fn terminal_row(index: usize) -> Point<Pixels> {
    let top = 40.0 + 4.0 * NAV_ROW + 3.0 * NAV_GAP + 8.0;
    point(
        px(60.0),
        px(top + index as f32 * (NAV_ROW + NAV_GAP) + NAV_ROW / 2.0),
    )
}

/// The row's close button, 4px in from the right end of the 248px sidebar's `px-2` column.
fn close_button(row: Point<Pixels>) -> Point<Pixels> {
    point(px(248.0 - 8.0 - 4.0 - 10.0), row.y)
}

fn click(window: &mut TestAppWindow<Workspace>, at: Point<Pixels>) {
    window.draw();
    window.simulate_click(at, MouseButton::Left);
}

fn loose(window: &TestAppWindow<Workspace>) -> Vec<(usize, Place)> {
    window.read(|w, _| w.loose.iter().map(|t| (t.key, t.place)).collect())
}

fn shell(window: &TestAppWindow<Workspace>, key: usize) -> u32 {
    window.read(|w, cx| {
        w.loose_terminal(key)
            .unwrap()
            .view()
            .read(cx)
            .process_id()
            .unwrap()
    })
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
fn the_inbox_is_the_third_row_as_in_the_desktop() {
    let (_app, mut window) = open();
    click(&mut window, nav_row(2));
    assert!(window.read(|w, _| w.inbox.open.is_some()));
}

#[test]
fn search_opens_the_palette_and_past_sessions_the_history_sheet() {
    let (_app, mut window) = open();
    click(&mut window, nav_row(0));
    assert!(window.read(|w, _| w.palette.is_some()));
    window.update(|w, window, cx| w.close_palette(window, cx));

    click(&mut window, nav_row(3));
    window.read(|w, _| assert!(w.sheet.is_some() && w.palette.is_none()));
}

#[test]
fn past_sessions_does_nothing_without_a_project() {
    let (_app, mut window) = open();
    window.update(|w, _, cx| {
        w.welcome = true;
        cx.notify();
    });
    click(&mut window, nav_row(3));
    assert!(window.read(|w, _| w.sheet.is_none()));
}

#[test]
fn terminal_opens_a_page_in_the_home_folder_listed_under_the_nav() {
    let (app, mut window) = open();
    let tabs = state(&window);
    click(&mut window, nav_row(1));
    assert_eq!(loose(&window), [(0, Place::Full)]);
    // It's in no project: the tabs stay as they were, and none of them is the one shown.
    assert_eq!(state(&window), tabs);
    let home = std::env::home_dir().unwrap().canonicalize().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while window.read(|w, cx| {
        w.loose[0]
            .view()
            .read(cx)
            .cwd()
            .and_then(|cwd| cwd.canonicalize().ok())
    }) != Some(home.clone())
    {
        assert!(Instant::now() < deadline, "the shell never started in ~");
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    window.update(|w, window, cx| {
        assert!(w.loose[0].view().focus_handle(cx).is_focused(window));
        assert_eq!(w.loose[0].title(cx), "~");
    });

    // A second one takes the page; the first keeps running out of sight.
    let first = shell(&window, 0);
    click(&mut window, nav_row(1));
    assert_eq!(loose(&window), [(0, Place::Hidden), (1, Place::Full)]);

    // Its row shows it again.
    click(&mut window, terminal_row(0));
    assert_eq!(loose(&window), [(0, Place::Full), (1, Place::Hidden)]);
    assert_eq!(shell(&window, 0), first, "showing it keeps the same shell");

    // Going to a tab puts it away, as going to another tab would.
    press(&mut window, "t");
    assert_eq!(loose(&window), [(0, Place::Hidden), (1, Place::Hidden)]);
}

/// An agent's row in the sidebar did nothing while the one-off terminal filled the pane area: the agent's tab was
/// already the active one behind it, and only a change of tab put the terminal away.
#[test]
fn going_to_a_pane_of_the_active_tab_puts_the_one_off_terminal_away() {
    let (_app, mut window) = open();
    let (project, pane) = window.read(|w, _| {
        let Active::Terminal(id) = w.active else { panic!("a terminal tab is shown") };
        (w.config.project.clone(), w.terminals.iter().find(|tab| tab.id == id).unwrap().focused)
    });
    click(&mut window, nav_row(1));
    assert_eq!(loose(&window), [(0, Place::Full)]);
    window.update(|w, window, cx| w.go_to_pane(project, pane, window, cx));
    assert_eq!(loose(&window), [(0, Place::Hidden)]);
    window.update(|w, window, cx| assert!(w.panes[&pane].view.as_ref().is_ok_and(|view| view.focus_handle(cx).is_focused(window))));
}

#[test]
fn the_rows_close_button_ends_its_shell() {
    let (app, mut window) = open();
    click(&mut window, nav_row(1));
    click(&mut window, nav_row(1));
    let gone = shell(&window, 0);
    window.simulate_mouse_move(terminal_row(0));
    click(&mut window, close_button(terminal_row(0)));
    assert_eq!(loose(&window), [(1, Place::Full)]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while running(gone) {
        assert!(
            Instant::now() < deadline,
            "the closed terminal's shell is still running"
        );
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    // Closing the one on screen gives the keys back to the tab under it.
    click(&mut window, close_button(terminal_row(0)));
    assert!(loose(&window).is_empty());
    window.update(|w, window, cx| {
        let pane = w.focused_pane().unwrap();
        assert!(
            w.terminal(pane)
                .unwrap()
                .focus_handle(cx)
                .is_focused(window)
        );
    });
}

#[test]
fn close_pane_closes_the_terminal_page_before_the_tab() {
    let (_app, mut window) = open();
    let tabs = state(&window);
    click(&mut window, nav_row(1));
    window.draw();
    window.simulate_keystroke(if cfg!(target_os = "macos") {
        "cmd-w"
    } else {
        "ctrl-shift-w"
    });
    assert!(loose(&window).is_empty());
    assert_eq!(state(&window), tabs);
}

/// Types `exit` in the terminal and waits until `gone` holds.
fn exit_shell(
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
    view: Entity<crate::terminal_view::TerminalView>,
    gone: impl Fn(&TestAppWindow<Workspace>) -> bool,
) {
    window.update(|_, _, cx| view.update(cx, |view, cx| view.write_input("exit\r", cx)));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gone(window) {
        assert!(
            Instant::now() < deadline,
            "the exited terminal is still listed"
        );
        // The redraw that carries the exit waits on the clock.
        app.advance_clock(Duration::from_millis(20));
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_terminal_whose_shell_exits_leaves_the_sidebar_and_gives_the_keys_back() {
    let (app, mut window) = open();
    click(&mut window, nav_row(1));
    click(&mut window, nav_row(1));
    assert_eq!(loose(&window), [(0, Place::Hidden), (1, Place::Full)]);

    // One ending out of sight leaves the keys in the one you're typing in.
    let hidden = window.read(|w, _| w.loose_terminal(0).unwrap().view().clone());
    exit_shell(&app, &mut window, hidden, |window| loose(window).len() == 1);
    assert_eq!(loose(&window), [(1, Place::Full)]);
    window.update(|w, window, cx| {
        assert!(w.loose[0].view().focus_handle(cx).is_focused(window));
    });

    // The one on screen ending gives them back to the tab under it.
    let shown = window.read(|w, _| w.loose_terminal(1).unwrap().view().clone());
    exit_shell(&app, &mut window, shown, |window| loose(window).is_empty());
    window.update(|w, window, cx| {
        let pane = w.focused_pane().unwrap();
        assert!(
            w.terminal(pane)
                .unwrap()
                .focus_handle(cx)
                .is_focused(window)
        );
        assert!(w.terminal(pane).unwrap().read(cx).is_visible());
    });
}

#[test]
fn a_rows_close_button_is_reached_by_keyboard_and_shows_while_the_row_has_focus() {
    let (_app, mut window) = open();
    // Focus moves in a window you're using.
    window.update(|_, window, _| window.activate_window());
    click(&mut window, nav_row(1));
    click(&mut window, nav_row(1));
    window.draw();
    assert!(window.read(|w, _| !w.loose[0].row_focused));

    // Tab reaches the row, then its close button, and the button shows all the while.
    window.update(|w, window, cx| {
        w.loose[0].row.focus(window, cx);
        window.focus_next(cx);
    });
    window.draw();
    assert!(window.read(|w, _| w.loose[0].row_focused));
    window.update(|_, window, cx| window.focus_next(cx));
    window.draw();
    assert!(window.read(|w, _| w.loose[0].row_focused));
    for event in ["down", "up"] {
        let keystroke = gpui::Keystroke::parse("enter").unwrap();
        if event == "down" {
            window.simulate_event(gpui::KeyDownEvent {
                keystroke,
                is_held: false,
                prefer_character_input: false,
            });
        } else {
            window.simulate_event(gpui::KeyUpEvent { keystroke });
        }
    }
    assert_eq!(loose(&window), [(1, Place::Full)]);
    // Its focus went with it, so the keys go back to what's on screen.
    window.update(|w, window, cx| {
        assert!(w.loose[0].view().focus_handle(cx).is_focused(window));
    });
}

/// Lets the shell take its size and the screen redraw, both of which wait on the clock.
fn settle(app: &TestApp, window: &mut TestAppWindow<Workspace>) {
    for _ in 0..3 {
        window.draw();
        app.advance_clock(Duration::from_millis(200));
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    window.draw();
}

#[test]
fn the_tabs_panes_stop_painting_under_a_terminal_page_and_keep_their_shells_and_size() {
    let (app, mut window) = open();
    settle(&app, &mut window);
    let pane = window.read(|w, _| w.focused_pane().unwrap());
    let terminal = window.read(|w, _| w.terminal(pane).unwrap());
    let (pid, grid) = window.read(|_, cx| {
        let view = terminal.read(cx);
        (view.process_id().unwrap(), view.grid())
    });
    assert_ne!(grid, (0, 0), "the shell never took its size");
    let at = window.read(|_, cx| terminal.read(cx).drawn_at());
    assert!(window.read(|_, cx| terminal.read(cx).is_visible()));

    click(&mut window, nav_row(1));
    settle(&app, &mut window);
    window.read(|_, cx| {
        let view = terminal.read(cx);
        assert!(!view.is_visible(), "the covered pane still paints");
        assert_eq!(view.drawn_at(), at, "covering the pane moved or resized it");
    });
    assert!(running(pid));

    // Going back to the tab, as clicking it does, paints it again: the same shell at the same size.
    window.update(|w, window, cx| {
        w.leave_loose(cx);
        w.show(w.active, window, cx);
    });
    assert!(
        loose(&window)
            .iter()
            .all(|(_, place)| *place == Place::Hidden)
    );
    settle(&app, &mut window);
    window.read(|_, cx| {
        let view = terminal.read(cx);
        assert!(view.is_visible());
        assert_eq!(view.grid(), grid);
        assert_eq!(view.process_id(), Some(pid));
    });
}
