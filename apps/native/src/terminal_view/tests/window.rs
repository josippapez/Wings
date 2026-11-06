//! Keys, text, mouse and wheel go through GPUI's own dispatch to the view, without a screen.

use super::*;
use crate::terminal_view::{find::FindResults, snapshot::Found, worker::start_process};
use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, Point, TestApp, TestAppWindow, point,
};

type View = TestAppWindow<TerminalView>;

fn open(script: &str) -> (TestApp, View) {
    let mut app = TestApp::new();
    app.background_executor().allow_parking();
    app.update(crate::actions::bind);
    let mut command = wings_terminal::CommandBuilder::new("/bin/sh");
    command.args(["-c", script]);
    let process =
        Arc::new(PtyProcess::spawn_command(command, wings_terminal::PtySize::default()).unwrap());
    let window = app.open_window(move |_, cx| {
        let (runtime, latest, updates) =
            start_process(process, "window-test".into(), Vec::new()).unwrap();
        TerminalView::new(runtime, latest, updates, cx)
    });
    (app, window)
}

fn settle(app: &TestApp, window: &mut View, done: impl Fn(&TerminalView) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.run_until_parked();
        app.advance_clock(REDRAW_DELAY + RESIZE_DELAY);
        app.run_until_parked();
        window.draw();
        if window.read(|view, _| done(view)) {
            return;
        }
        let text = window.read(|view, _| visible_text(&view.snapshot));
        assert!(Instant::now() < deadline, "window timed out: {text}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn shows(text: &'static str) -> impl Fn(&TerminalView) -> bool {
    move |view| visible_text(&view.snapshot).contains(text)
}

/// The middle of a cell in the top row, on its left or right half.
fn cell(window: &View, col: usize, right: bool) -> Point<Pixels> {
    window.read(|view, _| {
        let bounds = view.bounds.unwrap();
        let half = if right { 0.75 } else { 0.25 };
        point(
            bounds.left() + view.cell_width * (col as f32 + half),
            bounds.top() + px(line_height(DEFAULT_FONT_SIZE, 1.2) / 2.0),
        )
    })
}

fn chord(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-shift-{key}")
    }
}

#[test]
fn ctrl_c_still_interrupts_with_the_app_keys_bound() {
    let (app, mut window) =
        open(r#"trap 'printf got-int' INT; printf ready; while :; do sleep 0.1; done"#);
    settle(&app, &mut window, shows("ready"));
    window.simulate_keystroke("ctrl-c");
    settle(&app, &mut window, shows("got-int"));
    window.update(|view, _, _| view.runtime.shutdown());
}

#[test]
fn a_new_font_size_refits_the_rows() {
    let (app, mut window) = open("printf ready; sleep 30");
    settle(&app, &mut window, shows("ready"));
    let rows = |window: &View| window.read(|view, _| view.geometry.unwrap().rows);
    let before = rows(&window);
    window.update(|_, _, cx| cx.set_global(TerminalFont(20.0)));
    window.draw();
    assert_eq!(window.read(|view, _| view.line_height), 28.0);
    assert!(
        rows(&window) < before,
        "{} rows at 20px vs {before}",
        rows(&window)
    );
    window.update(|view, _, _| view.runtime.shutdown());
}

#[test]
fn the_line_height_setting_spaces_the_rows() {
    let (app, mut window) = open("printf ready; sleep 30");
    settle(&app, &mut window, shows("ready"));
    let rows = |window: &View| window.read(|view, _| view.geometry.unwrap().rows);
    let before = rows(&window);
    window.update(|_, _, cx| {
        cx.set_global(TerminalLook {
            line_height: 1.6,
            ..TerminalLook::default()
        })
    });
    window.draw();
    // 13px at 1.6, by the font's own 7/6 line.
    assert_eq!(window.read(|view, _| view.line_height), 24.0);
    assert!(rows(&window) < before);
    window.update(|view, _, _| view.runtime.shutdown());
}

#[test]
fn with_copy_on_select_letting_go_of_a_drag_copies_it() {
    let (app, mut window) = open("printf 'alpha beta'; sleep 30");
    settle(&app, &mut window, shows("beta"));
    window.update(|_, _, cx| {
        cx.set_global(TerminalLook {
            copy_on_select: true,
            ..TerminalLook::default()
        })
    });
    let (from, to) = (cell(&window, 6, false), cell(&window, 9, true));
    window.simulate_mouse_down(from, MouseButton::Left);
    window.simulate_event(MouseMoveEvent {
        position: to,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::default(),
    });
    window.simulate_mouse_up(to, MouseButton::Left);
    settle(&app, &mut window, |_| {
        app.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref()
            == Some("beta")
    });
    window.update(|view, _, _| view.runtime.shutdown());
}

#[test]
fn typed_text_and_enter_reach_the_shell() {
    let (app, mut window) =
        open(r#"stty -echo; printf ready; read line; printf 'got:%s' "$line"; sleep 30"#);
    settle(&app, &mut window, shows("ready"));
    window.simulate_input("hello");
    window.simulate_keystroke("enter");
    settle(&app, &mut window, shows("got:hello"));
    window.update(|view, _, _| view.runtime.shutdown());
}

#[test]
fn dragging_selects_and_the_clipboard_keys_copy_and_paste() {
    let (app, mut window) =
        open(r#"stty -echo; printf 'alpha beta'; read line; printf '\ngot:%s' "$line"; sleep 30"#);
    settle(&app, &mut window, shows("beta"));
    let (from, to) = (cell(&window, 0, false), cell(&window, 4, true));
    window.simulate_mouse_down(from, MouseButton::Left);
    window.simulate_event(MouseMoveEvent {
        position: to,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::default(),
    });
    window.simulate_mouse_up(to, MouseButton::Left);
    settle(&app, &mut window, |view| {
        view.snapshot
            .cells
            .iter()
            .filter(|cell| cell.selected)
            .count()
            == 5
    });
    window.simulate_keystroke(&chord("c"));
    settle(&app, &mut window, |_| {
        app.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref()
            == Some("alpha")
    });

    app.write_to_clipboard(ClipboardItem::new_string("pasted".into()));
    window.simulate_keystroke(&chord("v"));
    window.simulate_keystroke("enter");
    settle(&app, &mut window, shows("got:pasted"));
    window.update(|view, _, _| view.runtime.shutdown());
}

#[test]
fn hovering_underlines_a_link_and_only_a_modified_click_opens_it() {
    let (app, mut window) =
        open(r#"stty -echo; printf 'go https://example.com/path now'; sleep 30"#);
    settle(&app, &mut window, shows("now"));
    let on_link = cell(&window, 5, false);
    window.simulate_mouse_move(on_link);
    assert!(window.read(|view, _| view.link.is_some()));
    window.simulate_click(on_link, MouseButton::Left);
    assert_eq!(app.opened_url(), None);
    window.simulate_event(MouseDownEvent {
        position: on_link,
        button: MouseButton::Left,
        modifiers: Modifiers {
            platform: cfg!(target_os = "macos"),
            control: !cfg!(target_os = "macos"),
            ..Default::default()
        },
        click_count: 1,
        first_mouse: false,
    });
    assert_eq!(
        app.opened_url().as_deref(),
        Some("https://example.com/path")
    );
    window.update(|view, _, _| view.runtime.shutdown());
}

#[test]
fn cmd_f_steps_through_matches_and_escape_gives_keys_back_to_the_shell() {
    let (app, mut window) = open(
        r#"printf 'alpha\r\nneedle one\r\nbeta\r\nneedle two\r\n'; stty -echo; read line; printf 'got:%s' "$line"; sleep 30"#,
    );
    settle(&app, &mut window, shows("needletwo"));
    window.simulate_keystroke(&chord("f"));
    window.draw();
    assert!(window.read(|view, _| view.find.as_ref().is_some_and(|find| find.open)));
    window.simulate_input("needle");
    let at = |index| {
        move |view: &TerminalView| {
            view.snapshot.find
                == Some(FindResults {
                    index: Some(index),
                    count: 2,
                })
        }
    };
    settle(&app, &mut window, at(0));
    window.simulate_keystroke("enter");
    settle(&app, &mut window, at(1));
    window.simulate_keystroke("shift-enter");
    settle(&app, &mut window, at(0));
    assert!(window.read(|view, _| {
        view.snapshot
            .cells
            .iter()
            .any(|cell| cell.found == Found::Active)
    }));

    window.simulate_keystroke("escape");
    settle(&app, &mut window, |view| {
        view.snapshot.find.is_none()
            && view
                .snapshot
                .cells
                .iter()
                .all(|cell| cell.found == Found::No)
    });
    window.simulate_input("typed");
    window.simulate_keystroke("enter");
    settle(&app, &mut window, shows("got:typed"));
    window.update(|view, _, _| view.runtime.shutdown());
}

#[test]
fn the_current_match_is_found_again_after_output_moves_it() {
    let (app, mut window) = open(
        r#"stty -echo; printf 'needle-a\r\n'; sleep 1; i=0; while [ $i -lt 80 ]; do printf 'line-%s\r\n' $i; i=$((i+1)); done; sleep 30"#,
    );
    settle(&app, &mut window, shows("needle-a"));
    window.simulate_keystroke(&chord("f"));
    window.draw();
    window.simulate_input("needle");
    settle(&app, &mut window, |view| {
        view.snapshot.find
            == Some(FindResults {
                index: Some(0),
                count: 1,
            })
    });
    settle(&app, &mut window, |view| {
        visible_text(&view.snapshot).contains("line-79")
            && !view.snapshot.find_stale
            && view.snapshot.find
                == Some(FindResults {
                    index: Some(0),
                    count: 1,
                })
    });
    window.update(|view, _, _| view.runtime.shutdown());
}

#[test]
fn the_wheel_scrolls_history_and_typing_returns_to_the_bottom() {
    let (app, mut window) = open(
        r#"i=1; while [ $i -le 200 ]; do printf 'line-%s\r\n' $i; i=$((i+1)); done; stty -echo; sleep 30"#,
    );
    settle(&app, &mut window, shows("line-200"));
    let middle = cell(&window, 10, false);
    window.simulate_scroll(
        middle,
        point(px(0.0), px(line_height(DEFAULT_FONT_SIZE, 1.2) * 5.0)),
    );
    settle(&app, &mut window, |view| view.snapshot.display_offset == 5);
    window.simulate_input("x");
    settle(&app, &mut window, |view| view.snapshot.display_offset == 0);
    window.update(|view, _, _| view.runtime.shutdown());
}
