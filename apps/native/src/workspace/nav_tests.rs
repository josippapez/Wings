//! Headless window tests for the sidebar's top rows, as the desktop's `nav aria-label="Wings"`: Search, Terminal, the
//! Inbox and Past sessions, and the terminals the Terminal row opens, listed under them.

use super::*;
use crate::workspace::{
    quick::Place,
    sidebar::{NAV_GAP, NAV_ROW},
    tests::{open_with, press, state},
};
use gpui::{MouseButton, Point, TestApp, TestAppWindow, point};
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
