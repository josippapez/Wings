//! Tests that run a live shell through the worker, and the headless window tests.

use super::*;
use crate::terminal_view::worker::start_process;
use std::{thread, time::Instant};
use wings_terminal::{
    PtyProcess,
    alacritty_terminal::{grid::Scroll, index::Side, selection::SelectionType, term::TermMode},
    history,
};

#[cfg(feature = "test-support")]
mod window;

#[test]
fn live_worker_keeps_the_title_a_program_sets() {
    let mut command = wings_terminal::CommandBuilder::new("/bin/sh");
    command.args([
        "-c",
        r"printf '\033]0;native-title\007'; printf ready; sleep 30",
    ]);
    let process =
        Arc::new(PtyProcess::spawn_command(command, wings_terminal::PtySize::default()).unwrap());
    let (runtime, latest, _updates) =
        start_process(process.clone(), "title-test".into(), Vec::new()).unwrap();
    wait_snapshot(&latest, |frame| visible_text(frame).contains("ready"));
    assert_eq!(
        runtime.title.lock().unwrap().as_deref(),
        Some("native-title")
    );
    drop(runtime);
    wait_reaped(&process);
}

#[test]
fn the_cursor_setting_applies_until_a_program_sets_its_own() {
    let mut command = wings_terminal::CommandBuilder::new("/bin/sh");
    command.args([
        "-c",
        r"printf ready; read go; printf '\033[4 q'; printf set; sleep 30",
    ]);
    let process =
        Arc::new(PtyProcess::spawn_command(command, wings_terminal::PtySize::default()).unwrap());
    let (runtime, latest, _updates) =
        start_process(process.clone(), "cursor-test".into(), Vec::new()).unwrap();
    wait_snapshot(&latest, |frame| visible_text(frame).contains("ready"));
    let shape = |frame: &Snapshot| frame.cursor.map(|(.., shape)| shape);
    runtime
        .commands
        .send_blocking(worker::Command::Cursor(CursorShape::Beam))
        .unwrap();
    wait_snapshot(&latest, |frame| shape(frame) == Some(CursorShape::Beam));
    // DECSCUSR 4 asks for a steady underline, which wins over the setting.
    runtime
        .commands
        .send_blocking(worker::Command::Input(b"go\r".to_vec()))
        .unwrap();
    wait_snapshot(&latest, |frame| {
        shape(frame) == Some(CursorShape::Underline)
    });
    drop(runtime);
    wait_reaped(&process);
}

#[test]
fn replayed_output_ends_its_modes_and_never_answers_into_the_new_shell() {
    let history = b"before-alt\r\n\x1b[?1049h\x1b[?2004h\x1b[6nalt-content".to_vec();
    let mut command = wings_terminal::CommandBuilder::new("/bin/bash");
    command.args([
        "-c",
        r#"stty -echo raw; IFS= read -r -t 1 -n 20 got; printf 'got:[%s]' "$got"; sleep 30"#,
    ]);
    let process =
        Arc::new(PtyProcess::spawn_command(command, wings_terminal::PtySize::default()).unwrap());
    let (runtime, latest, _updates) =
        start_process(process.clone(), "history-test".into(), history.clone()).unwrap();
    let frame = wait_snapshot(&latest, |frame| visible_text(frame).contains("got:[]"));
    assert!(
        visible_text(&frame).contains("before-alt"),
        "{}",
        visible_text(&frame)
    );
    assert!(!visible_text(&frame).contains("alt-content"));
    assert!(
        !frame
            .mode
            .intersects(TermMode::ALT_SCREEN | TermMode::BRACKETED_PASTE)
    );
    let recent = runtime
        .recent
        .lock()
        .unwrap()
        .iter()
        .copied()
        .collect::<Vec<_>>();
    assert!(recent.starts_with(&history));
    drop(runtime);
    wait_reaped(&process);
}

fn recv_clipboard(runtime: &Runtime) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = runtime.clipboard.try_recv() {
            return text;
        }
        assert!(Instant::now() < deadline, "nothing reached the clipboard");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn live_worker_scrolls_history_copies_selections_and_takes_program_clipboard_writes() {
    let script = r#"i=1
while [ $i -le 60 ]; do printf 'line-%s\r\n' $i; i=$((i+1)); done
printf '\033]52;c;%s\007' "$(printf osc-copy | base64)"
printf ready
sleep 30"#;
    let mut command = wings_terminal::CommandBuilder::new("/bin/sh");
    command.args(["-c", script]);
    let process =
        Arc::new(PtyProcess::spawn_command(command, wings_terminal::PtySize::default()).unwrap());
    let (runtime, latest, _updates) =
        start_process(process.clone(), "selection-test".into(), Vec::new()).unwrap();
    wait_snapshot(&latest, |frame| visible_text(frame).contains("ready"));
    assert_eq!(recv_clipboard(&runtime), "osc-copy");

    let send = |command| runtime.commands.send_blocking(command).unwrap();
    send(Command::Scroll(Scroll::Delta(10)));
    let frame = wait_snapshot(&latest, |frame| frame.display_offset == 10);
    let top: String = frame
        .cells
        .iter()
        .filter(|cell| cell.row == 0)
        .map(|cell| cell.text.as_str())
        .collect();
    assert!(top.starts_with("line-") && top.len() == 7, "{top}");

    let at = |col, side| Hit { row: 0, col, side };
    send(Command::SelectFrom(
        at(0, Side::Left),
        SelectionType::Simple,
    ));
    send(Command::SelectTo(at(6, Side::Right)));
    send(Command::Copy);
    assert_eq!(recv_clipboard(&runtime), top);
    let frame = wait_snapshot(&latest, |frame| {
        frame.cells.iter().any(|cell| cell.selected)
    });
    let selected: Vec<_> = frame
        .cells
        .iter()
        .filter(|cell| cell.selected)
        .map(|cell| (cell.row, cell.col))
        .collect();
    assert_eq!(selected, (0..7).map(|col| (0, col)).collect::<Vec<_>>());

    send(Command::Input(b"x".to_vec()));
    wait_snapshot(&latest, |frame| frame.display_offset == 0);
    drop(runtime);
    wait_reaped(&process);
}

#[test]
fn a_hidden_pane_stores_its_scrollback_and_shows_it_again_in_order() {
    let open = || {
        let script = r#"i=1
while [ $i -le 700 ]; do printf 'line-%s\r\n' $i; i=$((i+1)); done
printf ready
read go
printf 'after\r\n'
printf done
sleep 30"#;
        let mut command = wings_terminal::CommandBuilder::new("/bin/sh");
        command.args(["-c", script]);
        let process = Arc::new(
            PtyProcess::spawn_command(command, wings_terminal::PtySize::default()).unwrap(),
        );
        let (runtime, latest, _updates) =
            start_process(process.clone(), "unload-test".into(), Vec::new()).unwrap();
        let frame = wait_snapshot(&latest, |frame| visible_text(frame).contains("ready"));
        runtime.visible.store(false, Ordering::Release);
        runtime.commands.send_blocking(Command::Unload).unwrap();
        (runtime, latest, process, frame.history)
    };

    // A pane that closes while hidden shows what it still held: only the screen.
    let (runtime, latest, process, before) = open();
    assert!(before > history::MIN_LINES, "{before}");
    runtime.commands.send_blocking(Command::Eof(None)).unwrap();
    assert_eq!(wait_snapshot(&latest, |frame| frame.closed).history, 0);
    drop(runtime);
    wait_reaped(&process);

    // Shown again, the stored lines sit above what arrived while it was hidden.
    let (runtime, latest, process, _) = open();
    let send = |command| runtime.commands.send_blocking(command).unwrap();
    send(Command::Input(b"go\r".to_vec()));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !runtime
        .recent
        .lock()
        .unwrap()
        .iter()
        .copied()
        .collect::<Vec<_>>()
        .ends_with(b"done")
    {
        assert!(Instant::now() < deadline, "the hidden pane got no output");
        thread::sleep(Duration::from_millis(10));
    }
    runtime.visible.store(true, Ordering::Release);
    send(Command::Refresh);
    let frame = wait_snapshot(&latest, |frame| visible_text(frame).contains("done"));
    assert!(visible_text(&frame).contains("after"));
    send(Command::Scroll(Scroll::Top));
    let frame = wait_snapshot(&latest, |frame| frame.display_offset == frame.history);
    let row = |frame: &Snapshot, row| -> String {
        frame
            .cells
            .iter()
            .filter(|cell| cell.row == row)
            .map(|cell| cell.text.as_str())
            .collect()
    };
    assert_eq!(
        (row(&frame, 0), row(&frame, 1)),
        ("line-1".into(), "line-2".into())
    );
    drop(runtime);
    wait_reaped(&process);
}

fn wait_snapshot(latest: &Latest, predicate: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = latest.lock().unwrap().clone();
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "terminal worker timed out: {:?}",
            snapshot.status
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn visible_text(snapshot: &Snapshot) -> String {
    snapshot
        .cells
        .iter()
        .map(|cell| cell.text.as_str())
        .collect()
}

fn wait_reaped(process: &PtyProcess) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while process.exit_status().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "child {:?} was not reaped",
            process.process_id()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn live_worker_runs_the_current_shell_in_its_project_and_refits_the_pty() {
    let cwd = std::env::temp_dir().canonicalize().unwrap();
    let (runtime, latest, _updates) = start(cwd.clone(), Vec::new(), None).unwrap();
    runtime
        .commands
        .send_blocking(Command::Input(b"printf '\\nnative-''cwd:'; pwd\r".to_vec()))
        .unwrap();
    wait_snapshot(&latest, |frame| {
        visible_text(frame).contains(&format!("native-cwd:{}", cwd.display()))
    });
    runtime
        .commands
        .send_blocking(Command::Resize(Geometry {
            cols: 100,
            rows: 31,
            cell_width: 8,
            cell_height: 20,
        }))
        .unwrap();
    runtime
        .commands
        .send_blocking(Command::Input(
            b"printf '\\nnative-''size:'; stty size\r".to_vec(),
        ))
        .unwrap();
    wait_snapshot(&latest, |frame| {
        visible_text(frame).contains("native-size:31100")
    });
    let process = runtime.process.clone();
    drop(runtime);
    wait_reaped(&process);
}

#[test]
fn live_worker_services_cursor_color_and_resized_pixel_queries() {
    let script = r#"stty -echo -icanon min 1 time 10
IFS= read -r -n 1 -t 3 start || exit 2
printf '\033[H\033[6n'
IFS= read -r -n 6 -t 3 response || exit 3
[ "$response" = "$(printf '\033[1;1R')" ] || exit 4
printf '\033]11;?\007'
expected=$(printf '\033]11;rgb:1717/1515/1b1b\007')
IFS= read -r -n ${#expected} -t 3 response || exit 5
[ "$response" = "$expected" ] || exit 6
printf '\033[14t'
expected=$(printf '\033[4;620;800t')
IFS= read -r -n ${#expected} -t 3 response || exit 7
[ "$response" = "$expected" ] || exit 8
printf native-protocol-ok
"#;
    let mut command = wings_terminal::CommandBuilder::new("/bin/bash");
    command.args(["-c", script]);
    let process =
        Arc::new(PtyProcess::spawn_command(command, wings_terminal::PtySize::default()).unwrap());
    let (runtime, latest, _updates) =
        start_process(process.clone(), "protocol-test".into(), Vec::new()).unwrap();
    runtime
        .commands
        .send_blocking(Command::Resize(Geometry {
            cols: 100,
            rows: 31,
            cell_width: 8,
            cell_height: 20,
        }))
        .unwrap();
    runtime
        .commands
        .send_blocking(Command::Input(b"g".to_vec()))
        .unwrap();
    let frame = wait_snapshot(&latest, |frame| frame.closed);
    wait_reaped(&process);
    let status = process.exit_status().unwrap().unwrap();
    assert!(
        status.success(),
        "protocol shell exited with {}: {}",
        status.exit_code(),
        visible_text(&frame)
    );
    assert!(
        visible_text(&frame).contains("native-protocol-ok"),
        "{}",
        visible_text(&frame)
    );
    drop(runtime);
}
