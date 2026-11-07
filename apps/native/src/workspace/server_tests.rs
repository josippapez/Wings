//! Panes in the terminal server across app launches: reattached by their saved history id, and left alone by an app
//! that runs without the server.

use std::{path::PathBuf, sync::Arc, time::Duration};

use gpui::{TestApp, TestAppWindow};
use wings_ptyd::host;

use crate::workspace::{
    Workspace,
    tests::{open_config, wait_for},
};
use crate::{
    host::TerminalServers,
    terminal_server::{self, Servers},
};

/// A server in its own folder, on this test's thread pool, as the app would find one already running.
fn server(dir: &std::path::Path) -> PathBuf {
    let socket = terminal_server::socket(dir, wings_ptyd::SERVER_VERSION);
    let listener = host::bind(&socket).unwrap();
    let serving = socket.clone();
    std::thread::spawn(move || host::serve(listener, &serving, host::Config::default()));
    socket
}

fn launch(
    state: &std::path::Path,
    servers: Option<&std::path::Path>,
) -> (TestApp, TestAppWindow<Workspace>) {
    let state = state.to_owned();
    let servers = servers.map(|dir| TerminalServers(Arc::new(Servers::connect(dir, None))));
    open_config(move |config| {
        config.state = Some(state);
        config.terminal_server = servers;
    })
}

fn output(window: &TestAppWindow<Workspace>) -> String {
    window.read(|w, cx| {
        String::from_utf8_lossy(&w.terminal(0).unwrap().read(cx).recent_output()).into_owned()
    })
}

fn shell(window: &TestAppWindow<Workspace>) -> u32 {
    window.read(|w, cx| w.terminal(0).unwrap().read(cx).process_id().unwrap())
}

/// Types `line` and presses Enter, then waits for `shows`, which must be what the command prints, never the line itself:
/// the line is typed a key at a time, and its echo shows while later keys, Enter among them, still wait to be sent, so a
/// quit then would drop them.
fn type_line(app: &TestApp, window: &mut TestAppWindow<Workspace>, line: &str, shows: &str) {
    window.simulate_input(line);
    window.simulate_keystroke("enter");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !output(window).contains(shows) {
        assert!(
            std::time::Instant::now() < deadline,
            "{shows:?} never showed: {:?}",
            output(window)
        );
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn quit(app: &mut TestApp) {
    app.update(|cx| cx.shutdown());
}

#[test]
fn a_restored_pane_reattaches_to_its_shell_even_after_a_launch_without_the_server() {
    let dir = PathBuf::from("/tmp").join(format!("wsn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let servers = dir.join("srv");
    std::fs::create_dir_all(&servers).unwrap();
    let state = dir.join("workspace.json");
    let socket = server(&servers);

    let (mut app, mut window) = launch(&state, Some(&servers));
    type_line(&app, &mut window, "echo fi''rst", "first");
    let kept = shell(&window);
    let listed = wings_ptyd::client::Control::connect(&socket)
        .unwrap()
        .list()
        .unwrap();
    assert_eq!(
        listed.iter().map(|p| p.shell_pid).collect::<Vec<_>>(),
        [Some(kept)],
        "the shell runs in the server"
    );
    type_line(
        &app,
        &mut window,
        "echo wai''ting; sleep 0.5; echo mean''while",
        "waiting",
    );
    quit(&mut app);
    drop((window, app));

    // A launch without the server runs the saved pane in the app, under the same history id, and leaves the
    // server's pane alone.
    let (mut app, mut window) = launch(&state, None);
    let local = shell(&window);
    assert_ne!(local, kept, "a shell of its own, in the app");
    type_line(&app, &mut window, "echo lo''cal", "local");
    quit(&mut app);
    drop((window, app));
    let listed = wings_ptyd::client::Control::connect(&socket)
        .unwrap()
        .list()
        .unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|p| (p.shell_pid, p.exited))
            .collect::<Vec<_>>(),
        [(Some(kept), false)]
    );

    // With the server again, the pane comes back to the shell it kept, with what it printed meanwhile.
    let (app, mut window) = launch(&state, Some(&servers));
    assert_eq!(shell(&window), kept);
    wait_for(
        &app,
        &window,
        |_| output(&window).contains("meanwhile"),
        "the replay",
    );
    type_line(&app, &mut window, "echo ag''ain", "again");
    // Closing it ends it in the server.
    window.update(|w, window, cx| w.close_pane(0, window, cx));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !wings_ptyd::client::Control::connect(&socket)
        .unwrap()
        .list()
        .unwrap()
        .is_empty()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the closed pane still runs in the server"
        );
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    drop((window, app));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pane_that_ended_while_the_app_was_away_shows_its_output_above_a_new_shell() {
    let dir = PathBuf::from("/tmp").join(format!("wse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let servers = dir.join("srv");
    std::fs::create_dir_all(&servers).unwrap();
    let state = dir.join("workspace.json");
    let socket = server(&servers);

    let (mut app, mut window) = launch(&state, Some(&servers));
    type_line(
        &app,
        &mut window,
        "echo star''ted; sleep 0.3; echo fini''shed; exit",
        "started",
    );
    let ended = shell(&window);
    quit(&mut app);
    drop((window, app));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !wings_ptyd::client::Control::connect(&socket)
        .unwrap()
        .list()
        .unwrap()[0]
        .exited
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the shell never exited"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let (app, mut window) = launch(&state, Some(&servers));
    assert_ne!(shell(&window), ended, "a new shell");
    wait_for(
        &app,
        &window,
        |_| output(&window).contains("finished"),
        "the old pane's output",
    );
    let listed = wings_ptyd::client::Control::connect(&socket)
        .unwrap()
        .list()
        .unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|p| (p.shell_pid, p.exited))
            .collect::<Vec<_>>(),
        [(Some(shell(&window)), false)],
        "the ended pane is gone from the server, and the new shell runs there"
    );
    window.update(|w, window, cx| w.close_pane(0, window, cx));
    drop((window, app));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_pane_after_the_server_stopped_runs_in_the_app_and_says_so() {
    let dir = PathBuf::from("/tmp").join(format!("wsg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let servers = dir.join("srv");
    std::fs::create_dir_all(&servers).unwrap();
    let state = dir.join("workspace.json");
    let socket = server(&servers);
    let (app, mut window) = launch(&state, Some(&servers));
    let served = shell(&window);
    // The server stops once its pane is closed: an app that starts no server (as here) has none to replace it with.
    window.update(|w, window, cx| w.close_pane(0, window, cx));
    wings_ptyd::client::Control::connect(&socket)
        .unwrap()
        .drain()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while socket.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the server never stopped"
        );
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }

    window.update(|w, window, cx| w.new_terminal(window, cx));
    let pane = window.read(|w, _| *w.panes.keys().max().unwrap());
    let local = window.read(|w, cx| w.terminal(pane).unwrap().read(cx).process_id().unwrap());
    assert_ne!(local, served);
    let parent = std::process::Command::new("/bin/ps")
        .args(["-o", "ppid=", "-p", &local.to_string()])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&parent.stdout).trim(),
        std::process::id().to_string(),
        "the shell runs in the app"
    );
    window.read(|w, _| {
        assert!(
            w.plugin_ui
                .notices
                .iter()
                .any(|notice| notice.title.as_ref() == "The terminal server isn't running"),
            "a notice says so"
        )
    });
    drop((window, app));
    let _ = std::fs::remove_dir_all(&dir);
}
