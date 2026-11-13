//! Panes in the terminal server across app launches: reattached by their saved history id, and left alone by an app
//! that runs without the server.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

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
    server_at(dir, wings_ptyd::SERVER_VERSION)
}

/// `server`, saying it's `version`.
fn server_at(dir: &std::path::Path, version: u32) -> PathBuf {
    let socket = terminal_server::socket(dir, version);
    let listener = host::bind(&socket).unwrap();
    let serving = socket.clone();
    let config = host::Config {
        version,
        ..host::Config::default()
    };
    std::thread::spawn(move || host::serve(listener, &serving, config));
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

/// The loose terminals the server runs: their shell pids, exited or not.
fn loose_in(socket: &std::path::Path) -> Vec<(Option<u32>, bool)> {
    wings_ptyd::client::Control::connect(socket)
        .unwrap()
        .list()
        .unwrap()
        .into_iter()
        .filter(|pane| pane.space_id == terminal_server::LOOSE)
        .map(|pane| (pane.shell_pid, pane.exited))
        .collect()
}

fn wait_until(app: &TestApp, what: &str, done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "{what}");
        // The redraw that carries a shell's exit waits on the clock.
        app.advance_clock(Duration::from_millis(20));
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_sidebar_terminal_runs_in_the_server_ends_with_the_app_and_is_not_shown_again() {
    use crate::workspace::quick::Place;
    let dir = PathBuf::from("/tmp").join(format!("wsl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let servers = dir.join("srv");
    std::fs::create_dir_all(&servers).unwrap();
    let state = dir.join("workspace.json");
    let socket = server(&servers);
    // One an earlier app left running when it crashed: nothing would show it again.
    let (left, _events) = wings_ptyd::client::Pane::spawn(
        &socket,
        wings_ptyd::proto::Spawn {
            space_id: terminal_server::LOOSE.into(),
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 24,
            history_id: Some("left-loose".into()),
            env: std::env::vars().collect(),
        },
        b"",
    )
    .unwrap();
    left.detach();

    let (mut app, mut window) = launch(&state, Some(&servers));
    wait_until(&app, "the earlier app's terminal still runs", || {
        loose_in(&socket).is_empty()
    });
    let loose_shell = |window: &TestAppWindow<Workspace>| {
        window.read(|w, cx| {
            w.loose
                .last()
                .unwrap()
                .view()
                .read(cx)
                .process_id()
                .unwrap()
        })
    };
    window.update(|w, window, cx| w.open_loose(Place::Full, window, cx));
    let first = loose_shell(&window);
    assert_eq!(
        loose_in(&socket),
        [(Some(first), false)],
        "it runs in the server"
    );
    // Its shell exiting takes its row out, and the server forgets it.
    window.update(|w, _, cx| {
        w.loose[0]
            .view()
            .update(cx, |view, cx| view.write_input("exit\r", cx))
    });
    wait_until(&app, "the exited terminal's row stayed", || {
        window.read(|w, _| w.loose.is_empty())
    });
    wait_until(&app, "the server kept the exited terminal", || {
        loose_in(&socket).is_empty()
    });

    window.update(|w, window, cx| w.open_loose(Place::Full, window, cx));
    let second = loose_shell(&window);
    assert_eq!(loose_in(&socket), [(Some(second), false)]);
    quit(&mut app);
    drop((window, app));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !loose_in(&socket).is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "its shell runs on after the app quit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let (app, window) = launch(&state, Some(&servers));
    assert!(window.read(|w, _| w.loose.is_empty()), "nothing saved it");
    drop((window, app));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The shells the server runs, exited or not.
fn shells_in(socket: &std::path::Path) -> Vec<Option<u32>> {
    wings_ptyd::client::Control::connect(socket)
        .unwrap()
        .list()
        .unwrap()
        .into_iter()
        .map(|pane| pane.shell_pid)
        .collect()
}

fn newest_shell(window: &TestAppWindow<Workspace>) -> u32 {
    window.read(|w, cx| {
        let pane = *w.panes.keys().max().unwrap();
        w.terminal(pane).unwrap().read(cx).process_id().unwrap()
    })
}

#[test]
fn turned_off_in_settings_new_panes_run_in_the_app_and_the_servers_panes_come_back() {
    let dir = PathBuf::from("/tmp").join(format!("wso-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let servers = dir.join("srv");
    std::fs::create_dir_all(&servers).unwrap();
    let state = dir.join("workspace.json");
    let socket = server(&servers);

    let (mut app, mut window) = launch(&state, Some(&servers));
    assert!(window.read(|w, _| w.terminal_server_on()));
    type_line(&app, &mut window, "echo fi''rst", "first");
    let kept = shell(&window);
    // Turned off: the pane in the server stays, and the next one runs in the app.
    window.update(|w, _, cx| w.set_terminal_server(false, cx));
    window.update(|w, window, cx| w.new_terminal(window, cx));
    let local = newest_shell(&window);
    assert_eq!(
        shells_in(&socket),
        [Some(kept)],
        "the new pane isn't in the server"
    );
    assert_ne!(local, kept);
    quit(&mut app);
    drop((window, app));
    assert_eq!(
        crate::saved::load_settings(&state).terminal_server,
        Some(false),
        "saved"
    );

    // The next launch, off as main starts it: the server's pane comes back, and new panes still run in the app.
    let state_for = state.clone();
    let off = servers.clone();
    let (app, mut window) = open_config(move |config| {
        config.state = Some(state_for);
        config.terminal_server = Some(TerminalServers(Arc::new(Servers::connect_off(&off, None))));
    });
    assert!(!window.read(|w, _| w.terminal_server_on()));
    assert_eq!(
        shell(&window),
        kept,
        "attached to the shell the server kept"
    );
    window.update(|w, window, cx| w.new_terminal(window, cx));
    assert_eq!(shells_in(&socket).len(), 1, "a new pane runs in the app");
    // Turned on again: the next pane runs in the server.
    window.update(|w, _, cx| w.set_terminal_server(true, cx));
    window.update(|w, window, cx| w.new_terminal(window, cx));
    let served = newest_shell(&window);
    assert!(
        shells_in(&socket).contains(&Some(served)),
        "the new pane runs in the server"
    );
    assert_eq!(
        crate::saved::load_settings(&state).terminal_server,
        Some(true)
    );
    drop((window, app));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two projects, alpha and beta, a terminal server, and Claude Code's folder with a made-up `claude` (a renamed
/// `sleep`), in their own folder.
struct TwoProjects {
    dir: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
    claude: PathBuf,
    fake_claude: PathBuf,
    servers: PathBuf,
    socket: PathBuf,
}

impl TwoProjects {
    fn new(tag: &str) -> TwoProjects {
        let dir = PathBuf::from("/tmp").join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let (alpha, beta) = (dir.join("alpha"), dir.join("beta"));
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::create_dir_all(&beta).unwrap();
        let spaces = serde_json::json!([
            { "id": "a", "name": "alpha", "path": alpha },
            { "id": "b", "name": "beta", "path": beta },
        ]);
        std::fs::write(
            dir.join("spaces.json"),
            serde_json::to_vec(&spaces).unwrap(),
        )
        .unwrap();
        let claude = dir.join("claude");
        std::fs::create_dir_all(claude.join("sessions")).unwrap();
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let fake_claude = dir.join("bin/claude");
        std::os::unix::fs::symlink("/bin/sleep", &fake_claude).unwrap();
        let servers = dir.join("srv");
        std::fs::create_dir_all(&servers).unwrap();
        let socket = server(&servers);
        TwoProjects {
            dir,
            alpha,
            beta,
            claude,
            fake_claude,
            servers,
            socket,
        }
    }

    /// The app with alpha on screen, as it opens at launch.
    fn launch(&self) -> (TestApp, TestAppWindow<Workspace>) {
        self.launch_with(Servers::connect(&self.servers, None))
    }

    /// `launch` after an update that brought the next server version: the one from `new` is older, and drains.
    fn launch_updated(&self) -> (TestApp, TestAppWindow<Workspace>, PathBuf) {
        let newer = server_at(&self.servers, wings_ptyd::SERVER_VERSION + 1);
        let servers = Servers::connect_as(&self.servers, None, wings_ptyd::SERVER_VERSION + 1);
        let (app, window) = self.launch_with(servers);
        (app, window, newer)
    }

    fn launch_with(&self, servers: Servers) -> (TestApp, TestAppWindow<Workspace>) {
        let (alpha, dir, claude) = (self.alpha.clone(), self.dir.clone(), self.claude.clone());
        let servers = TerminalServers(Arc::new(servers));
        open_config(move |config| {
            config.project = alpha;
            config.projects = Some(dir.join("spaces.json"));
            config.state = Some(dir.join("workspace.json"));
            config.agents = Some(claude);
            config.terminal_server = Some(servers);
        })
    }

    /// The panes the server runs for beta whose program hasn't ended, by shell.
    fn beta_shells(&self) -> Vec<Option<u32>> {
        let mut shells: Vec<_> = wings_ptyd::client::Control::connect(&self.socket)
            .unwrap()
            .list()
            .unwrap()
            .into_iter()
            .filter(|pane| !pane.exited && Path::new(&pane.space_id) == self.beta)
            .map(|pane| pane.shell_pid)
            .collect();
        shells.sort();
        shells
    }

    /// Ends every pane the server runs, which would otherwise outlive the test.
    fn end(self) {
        end_at(&self.socket);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn after_a_relaunch_an_agent_in_a_project_not_shown_is_in_the_agents_list() {
    let projects = TwoProjects::new("wsa");
    let (mut app, mut window) = projects.launch();
    window.update(|w, window, cx| w.switch_project(projects.beta.clone(), window, cx));
    let pane = window.read(|w, _| w.focused_pane().unwrap());
    let run = format!("exec '{}' 60\r", projects.fake_claude.display());
    window.update(|w, _, cx| {
        w.terminal(pane)
            .unwrap()
            .update(cx, |view, cx| view.write_input(&run, cx))
    });
    wait_for(
        &app,
        &window,
        |w| w.agents.contains_key(&pane),
        "the agent was never detected",
    );
    let pid = window.read(|w, _| w.agents[&pane].pid);
    window.update(|w, window, cx| w.switch_project(projects.alpha.clone(), window, cx));
    quit(&mut app);
    drop((window, app));

    // Relaunched on alpha, beta's agent, still running in the server, is listed without opening beta.
    let (app, window) = projects.launch();
    wait_for(
        &app,
        &window,
        |w| !w.project_agents(&projects.beta).is_empty(),
        "beta's agent never showed in the sidebar",
    );
    window.read(|w, _| {
        assert_eq!(w.project(), projects.alpha);
        let beta = projects.beta.to_string_lossy();
        let agents: Vec<_> = w.agents.values().filter(|a| a.space_id == beta).collect();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].pid, pid, "the agent the server kept running");
    });
    drop((window, app));
    projects.end();
}

#[test]
fn a_project_restored_behind_the_shown_one_shows_its_tabs_once_and_starts_only_its_ended_panes() {
    use crate::workspace::Active;
    let projects = TwoProjects::new("wsb");
    let (mut app, mut window) = projects.launch();
    window.update(|w, window, cx| {
        w.switch_project(projects.beta.clone(), window, cx);
        w.new_terminal(window, cx);
        let first = w.terminals[0].id;
        w.show(crate::workspace::Active::Terminal(first), window, cx);
    });
    let (kept, ended) = window.read(|w, cx| {
        let pane = |tab: usize| w.terminals[tab].focused;
        (
            w.terminal(pane(0)).unwrap().read(cx).process_id(),
            w.panes[&pane(1)].history.clone(),
        )
    });
    window.update(|w, window, cx| w.switch_project(projects.alpha.clone(), window, cx));
    quit(&mut app);
    drop((window, app));
    // While the app was away, the second tab's shell ended, and an earlier app left one of beta's panes no tab saved.
    let mut control = wings_ptyd::client::Control::connect(&projects.socket).unwrap();
    let gone = control
        .list()
        .unwrap()
        .into_iter()
        .find(|pane| pane.history_id.as_deref() == Some(ended.as_str()))
        .unwrap();
    control.kill(gone.pane).unwrap();
    let (orphan, _events) = wings_ptyd::client::Pane::spawn(
        &projects.socket,
        wings_ptyd::proto::Spawn {
            space_id: projects.beta.to_string_lossy().into_owned(),
            cwd: projects.beta.clone(),
            cols: 80,
            rows: 24,
            history_id: Some("beta-orphan".into()),
            env: std::env::vars().collect(),
        },
        b"",
    )
    .unwrap();
    let orphan_shell = orphan.shell_pid();
    orphan.detach();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while projects.beta_shells().len() != 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "the ended shell still runs"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let (app, mut window) = projects.launch();
    let mut running = vec![kept, orphan_shell];
    running.sort();
    assert_eq!(
        projects.beta_shells(),
        running,
        "no shell starts for beta while it isn't shown"
    );
    let (waiting, adopted) = window.read(|w, cx| {
        let parked = &w.parked[&projects.beta];
        assert_eq!(parked.terminals.len(), 3, "its two tabs and the orphan's");
        let shell = |tab: usize| {
            w.terminal(parked.terminals[tab].focused)
                .map(|view| view.read(cx).process_id())
        };
        assert_eq!(
            shell(0),
            Some(kept),
            "attached to the shell the server kept"
        );
        assert_eq!(shell(1), None, "the ended pane waits to be shown");
        assert_eq!(shell(2), Some(orphan_shell));
        (parked.terminals[1].focused, parked.terminals[2].id)
    });

    // Shown, it has the same three tabs, and only the ended pane gets a new shell.
    window.update(|w, window, cx| w.switch_project(projects.beta.clone(), window, cx));
    let started = window.read(|w, cx| {
        assert!(!w.parked.contains_key(&projects.beta));
        assert_eq!(w.terminals.len(), 3);
        assert_eq!(w.terminals[1].focused, waiting, "under the id its tab held");
        assert_eq!(
            w.active,
            Active::Terminal(adopted),
            "the orphan's tab, as entering shows it"
        );
        w.terminal(waiting).unwrap().read(cx).process_id()
    });
    let mut running = vec![kept, orphan_shell, started];
    running.sort();
    assert_eq!(projects.beta_shells(), running);
    // Left and shown again, it is the same.
    window.update(|w, window, cx| {
        w.switch_project(projects.alpha.clone(), window, cx);
        w.switch_project(projects.beta.clone(), window, cx);
    });
    window.read(|w, _| assert_eq!(w.terminals.len(), 3));
    assert_eq!(projects.beta_shells(), running);
    drop((window, app));
    projects.end();
}

/// A session id as Claude Code makes them.
const SESSION: &str = "0aa900ec-ed63-4df5-97c1-eecebeb8ef8d";

/// The flags the made-up agent starts with, which a resume types again. With `--version` among them the resume line is
/// harmless in the test's real shell: a `claude` installed there prints its version and exits.
const FLAGS: [&str; 3] = ["--model", "opus", "--version"];

fn agent_session() -> crate::saved::SavedSession {
    crate::saved::SavedSession {
        kind: crate::saved::SessionKind::Claude,
        id: SESSION.into(),
        args: FLAGS.map(String::from).to_vec(),
        name: None,
    }
}

fn resume_line() -> String {
    format!("claude --resume {SESSION} {}\r", FLAGS.join(" "))
}

/// The folder in alpha that the agent's pane works in.
fn sub(projects: &TwoProjects) -> PathBuf {
    projects.alpha.join("sub")
}

/// In the shown pane, `cd`s into `sub` and starts a made-up `claude` there with `FLAGS` (bash under that name, whose
/// flags detection reads as Claude Code's), gives it `SESSION` as Claude Code's session file would, and waits until the
/// pane keeps that session. The pane and the agent's pid.
fn start_agent(
    projects: &TwoProjects,
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
) -> (usize, u32) {
    start_agent_running(projects, app, window, "sleep 60; true")
}

/// `start_agent`, its made-up `claude` running `script`.
fn start_agent_running(
    projects: &TwoProjects,
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
    script: &str,
) -> (usize, u32) {
    std::fs::create_dir_all(sub(projects)).unwrap();
    let claude = projects.dir.join("agent/claude");
    std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("/bin/bash", &claude).unwrap();
    let pane = window.read(|w, _| w.focused_pane().unwrap());
    let run = format!(
        "cd '{}' && '{}' -c '{script}' {}\r",
        sub(projects).display(),
        claude.display(),
        FLAGS.join(" ")
    );
    window.update(|w, _, cx| {
        w.terminal(pane)
            .unwrap()
            .update(cx, |view, cx| view.write_input(&run, cx))
    });
    wait_for(
        app,
        window,
        |w| w.agents.contains_key(&pane),
        "the agent was never detected",
    );
    let pid = window.read(|w, _| w.agents[&pane].pid);
    std::fs::write(
        projects.claude.join(format!("sessions/{pid}.json")),
        format!(r#"{{"sessionId":"{SESSION}"}}"#),
    )
    .unwrap();
    write_transcript(&projects.claude);
    wait_for(
        app,
        window,
        |w| w.panes[&pane].session == Some(agent_session()),
        "the pane never kept its agent's session",
    );
    (pane, pid)
}

/// `SESSION`'s transcript in the Claude Code folder `claude`, as a session you wrote to has one.
fn write_transcript(claude: &Path) {
    let folder = claude.join("projects/-tmp-agent");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(format!("{SESSION}.jsonl")), "{}\n").unwrap();
}

/// A saved project of one pane in `cwd` that ran `session`, as the app saves it under the history id `history`.
fn saved_agent_pane(
    cwd: &Path,
    history: &str,
    session: crate::saved::SavedSession,
) -> crate::saved::SavedProject {
    crate::saved::SavedProject {
        tabs: vec![crate::saved::SavedTab {
            layout: crate::saved::SavedLayout::Pane {
                cwd: Some(cwd.to_owned()),
                history: Some(history.into()),
                name: None,
                asleep: false,
                claude_account: None,
                session: Some(session),
            },
            focused: 0,
            zoomed: None,
        }],
        active: Some(0),
        ..Default::default()
    }
}

#[test]
fn after_a_restart_an_agent_in_a_project_not_shown_is_listed_until_it_is_back() {
    use crate::workspace::{agent_list::Phase, panes::RESUME_TIMEOUT};
    let projects = TwoProjects::new("wsp");
    write_transcript(&projects.claude);
    // Before the restart, beta's pane ran an agent. The restart ended every pane the server ran.
    let session = crate::saved::SavedSession {
        name: Some("beta-fix".into()),
        ..agent_session()
    };
    crate::saved::save(
        &projects.dir.join("workspace.json"),
        &projects.beta.to_string_lossy(),
        saved_agent_pane(&projects.beta, "beta-agent", session),
        crate::saved::Settings::default(),
    )
    .unwrap();

    let (app, mut window) = projects.launch();
    let pane = window.read(|w, cx| {
        assert_eq!(w.project(), projects.alpha);
        let rows = w.pending_agents(cx.background_executor().now());
        assert_eq!(rows.len(), 1, "beta's agent is listed without opening beta");
        assert_eq!(rows[0].project, projects.beta);
        assert_eq!(rows[0].name, "beta-fix", "under the name it had");
        assert_eq!(rows[0].phase, Phase::Restored);
        rows[0].pane
    });
    assert!(
        projects.beta_shells().is_empty(),
        "nothing starts until beta is opened"
    );

    // Its row opens beta, whose pane resumes the session, and the row stays until the agent shows.
    window.update(|w, window, cx| w.go_to_pane(projects.beta.clone(), pane, window, cx));
    window.read(|w, cx| {
        assert_eq!(w.project(), projects.beta);
        assert_eq!(w.focused_pane(), Some(pane));
        assert_eq!(
            w.terminal(pane).unwrap().read(cx).written(),
            [resume_line()]
        );
        let now = cx.background_executor().now();
        assert_eq!(w.pending_agents(now)[0].phase, Phase::Resuming);
        assert_eq!(
            w.pending_agents(now + RESUME_TIMEOUT)[0].phase,
            Phase::Failed,
            "an agent that doesn't show"
        );
    });
    drop((window, app));
    projects.end();
}

#[test]
fn a_pane_on_a_drive_not_mounted_yet_waits_for_it_then_resumes_its_agent_in_its_folder() {
    use crate::workspace::agent_list::Phase;
    let dir = PathBuf::from("/tmp").join(format!("wsv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Volumes")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let volumes = dir.join("Volumes");
    let project = volumes.join("encrypted/acme");
    let folder = project.join("web");
    let claude = dir.join("claude");
    std::fs::create_dir_all(claude.join("sessions")).unwrap();
    write_transcript(&claude);
    let state = dir.join("workspace.json");
    crate::saved::save(
        &state,
        &project.to_string_lossy(),
        saved_agent_pane(&folder, "acme-agent", agent_session()),
        crate::saved::Settings::default(),
    )
    .unwrap();

    let launch = project.clone();
    let (app, window) = open_config(move |config| {
        config.project = launch;
        config.state = Some(state);
        config.agents = Some(claude);
        config.volumes = volumes;
        config.terminal_server = None;
    });
    let pane = window.read(|w, cx| {
        let pane = w.focused_pane().unwrap();
        assert!(
            w.terminal(pane).is_none(),
            "no shell before the drive mounts"
        );
        let rows = w.pending_agents(cx.background_executor().now());
        assert_eq!(rows[0].phase, Phase::Mounting("encrypted".into()));
        pane
    });

    std::fs::create_dir_all(&folder).unwrap();
    wait_for(
        &app,
        &window,
        |w| w.terminal(pane).is_some(),
        "the pane never started once the drive was there",
    );
    window.read(|w, cx| {
        let view = w.terminal(pane).unwrap().read(cx);
        assert_eq!(view.cwd(), Some(folder.clone()), "in the folder it was in");
        assert_eq!(view.written(), [resume_line()]);
    });
    drop((window, app));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The session saved with alpha's first pane.
fn saved_session(projects: &TwoProjects) -> Option<crate::saved::SavedSession> {
    let alpha = crate::saved::load(
        &projects.dir.join("workspace.json"),
        &projects.alpha.to_string_lossy(),
    )
    .unwrap();
    match &alpha.tabs[0].layout {
        crate::saved::SavedLayout::Pane { session, .. } => session.clone(),
        crate::saved::SavedLayout::Split { .. } => panic!("one pane was saved"),
    }
}

/// The shown pane's shell, what Wings typed into it, and the folder it's in.
fn shown_pane(window: &TestAppWindow<Workspace>) -> (Option<u32>, Vec<String>, Option<PathBuf>) {
    window.read(|w, cx| {
        let view = w.terminal(w.focused_pane().unwrap()).unwrap().read(cx);
        (view.process_id(), view.written().to_vec(), view.cwd())
    })
}

/// What the server runs for the saved pane `history`: whether its program ended, or `None` once it's gone.
fn served(socket: &Path, history: &str) -> Option<bool> {
    wings_ptyd::client::Control::connect(socket)
        .unwrap()
        .list()
        .unwrap()
        .into_iter()
        .find(|pane| pane.history_id.as_deref() == Some(history))
        .map(|pane| pane.exited)
}

fn wait_served(socket: &Path, history: &str, want: Option<bool>) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while served(socket, history) != want {
        assert!(
            std::time::Instant::now() < deadline,
            "the server never had {want:?} for the pane"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn after_a_relaunch_a_pane_whose_program_is_gone_resumes_its_agents_session_in_its_folder_with_its_flags()
 {
    let projects = TwoProjects::new("wsr");
    let (mut app, mut window) = projects.launch();
    let (pane, _) = start_agent(&projects, &app, &mut window);
    let history = window.read(|w, _| w.panes[&pane].history.clone());
    let (before, ..) = shown_pane(&window);
    quit(&mut app);
    drop((window, app));
    assert_eq!(
        saved_session(&projects),
        Some(agent_session()),
        "saved with its pane"
    );
    // While the app was away the server lost the pane, as a reboot loses them all.
    let mut control = wings_ptyd::client::Control::connect(&projects.socket).unwrap();
    let gone = control
        .list()
        .unwrap()
        .into_iter()
        .find(|pane| pane.history_id.as_deref() == Some(history.as_str()))
        .unwrap();
    control.kill(gone.pane).unwrap();
    wait_served(&projects.socket, &history, None);

    let (app, window) = projects.launch();
    let (shell, written, cwd) = shown_pane(&window);
    assert_ne!(shell, before, "a new shell");
    assert_eq!(cwd, Some(sub(&projects)), "in the folder the pane was in");
    assert_eq!(
        written,
        [resume_line()],
        "the session resumes with its flags"
    );
    drop((window, app));
    projects.end();
}

#[test]
fn after_a_relaunch_a_pane_whose_shell_ended_while_the_app_was_away_resumes_its_agents_session() {
    let projects = TwoProjects::new("wsq");
    let (mut app, mut window) = projects.launch();
    let (pane, pid) = start_agent(&projects, &app, &mut window);
    let history = window.read(|w, _| w.panes[&pane].history.clone());
    let (shell, ..) = shown_pane(&window);
    quit(&mut app);
    drop((window, app));
    // The agent and then its shell ended; the server keeps what the pane printed.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
        libc::kill(shell.unwrap() as i32, libc::SIGKILL);
    }
    wait_served(&projects.socket, &history, Some(true));

    let (app, window) = projects.launch();
    let (_, written, cwd) = shown_pane(&window);
    assert_eq!(cwd, Some(sub(&projects)));
    assert_eq!(written, [resume_line()]);
    drop((window, app));
    projects.end();
}

#[test]
fn after_a_relaunch_a_pane_the_app_ran_itself_resumes_its_agents_session() {
    let projects = TwoProjects::new("wsx");
    let launch = || {
        let (alpha, dir, claude) = (
            projects.alpha.clone(),
            projects.dir.clone(),
            projects.claude.clone(),
        );
        open_config(move |config| {
            config.project = alpha;
            config.projects = Some(dir.join("spaces.json"));
            config.state = Some(dir.join("workspace.json"));
            config.agents = Some(claude);
            config.terminal_server = None;
        })
    };
    let (mut app, mut window) = launch();
    start_agent(&projects, &app, &mut window);
    quit(&mut app);
    drop((window, app));

    // Its shell ended with the app, so the next one starts the session again.
    let (mut app, window) = launch();
    let (_, written, cwd) = shown_pane(&window);
    assert_eq!(cwd, Some(sub(&projects)));
    assert_eq!(written, [resume_line()]);
    quit(&mut app);
    drop((window, app));
    projects.end();
}

#[test]
fn after_a_relaunch_a_pane_whose_agent_still_runs_is_attached_and_given_nothing_until_its_agent_quits()
 {
    let projects = TwoProjects::new("wsk");
    let (mut app, mut window) = projects.launch();
    let (pane, pid) = start_agent(&projects, &app, &mut window);
    let (shell, ..) = shown_pane(&window);
    quit(&mut app);
    drop((window, app));

    let (mut app, window) = projects.launch();
    let (attached, written, _) = shown_pane(&window);
    assert_eq!(attached, shell, "attached to the shell the server kept");
    assert!(written.is_empty(), "nothing typed into it: {written:?}");
    let pane_now = window.read(|w, _| w.focused_pane().unwrap());
    assert_eq!(pane_now, pane);
    wait_for(
        &app,
        &window,
        |w| w.agents.contains_key(&pane) && w.panes[&pane].session == Some(agent_session()),
        "the running agent and its session",
    );
    // You quit the agent: its pane no longer resumes it.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
    wait_for(
        &app,
        &window,
        |w| !w.agents.contains_key(&pane) && w.panes[&pane].session.is_none(),
        "the session stayed after its agent quit",
    );
    quit(&mut app);
    drop((window, app));
    assert_eq!(saved_session(&projects), None);
    projects.end();
}

#[test]
fn closing_the_window_quits_so_its_panes_run_on_in_the_server_for_the_next_launch() {
    let dir = PathBuf::from("/tmp").join(format!("wsw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let servers = dir.join("srv");
    std::fs::create_dir_all(&servers).unwrap();
    let state = dir.join("workspace.json");
    let socket = server(&servers);
    let listed = || {
        wings_ptyd::client::Control::connect(&socket)
            .unwrap()
            .list()
            .unwrap()
            .into_iter()
            .map(|pane| (pane.shell_pid, pane.exited))
            .collect::<Vec<_>>()
    };

    let (mut app, mut window) = launch(&state, Some(&servers));
    type_line(&app, &mut window, "echo fi''rst", "first");
    let kept = shell(&window);
    // File > Close Window, as the menu sends it.
    window.draw();
    window
        .update(|_, window, cx| window.dispatch_action(Box::new(crate::actions::CloseWindow), cx));
    let open = app.windows().len();
    // What the platform does once the app asks to quit; the test platform's quit does nothing.
    quit(&mut app);
    drop((window, app));
    assert_eq!(
        listed(),
        [(Some(kept), false)],
        "the pane runs on in the server"
    );
    assert_eq!(open, 1, "the quit closes the window, as on Cmd+Q");

    let (app, mut window) = launch(&state, Some(&servers));
    assert_eq!(shell(&window), kept, "the next launch attaches to it again");
    type_line(&app, &mut window, "echo ag''ain", "again");
    window.update(|w, window, cx| w.close_pane(0, window, cx));
    wait_until(&app, "the closed pane still runs in the server", || {
        listed().is_empty()
    });
    drop((window, app));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The socket and number of the pane's terminal in its server.
fn served_at(window: &TestAppWindow<Workspace>, pane: usize) -> Option<(PathBuf, u32)> {
    window.read(|w, cx| {
        w.terminal(pane)?
            .read(cx)
            .server()
            .map(|(socket, number)| (socket.to_owned(), number))
    })
}

#[test]
fn an_idle_shell_on_an_older_server_moves_to_the_current_one_in_the_same_pane_with_its_output() {
    let projects = TwoProjects::new("wsmv");
    let (mut app, mut window) = projects.launch();
    let pane = window.read(|w, _| w.focused_pane().unwrap());
    window.update(|w, _, cx| {
        w.terminal(pane)
            .unwrap()
            .update(cx, |view, cx| view.write_input("echo fi''rst\r", cx))
    });
    let shows = |window: &TestAppWindow<Workspace>, text: &str| {
        window.read(|w, cx| {
            String::from_utf8_lossy(&w.terminal(pane).unwrap().read(cx).recent_output())
                .contains(text)
        })
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !shows(&window, "first") {
        assert!(std::time::Instant::now() < deadline, "it never printed");
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    let history = window.read(|w, _| w.panes[&pane].history.clone());
    quit(&mut app);
    drop((window, app));

    let (app, mut window, newer) = projects.launch_updated();
    assert_eq!(
        served_at(&window, pane).map(|(socket, _)| socket),
        Some(projects.socket.clone())
    );
    window.read(|w, cx| {
        assert!(w.on_older_server(pane, cx));
        assert!(
            w.older_server_marker(pane, cx).is_some(),
            "its header says so"
        );
    });
    window.draw();

    let on_older = |window: &TestAppWindow<Workspace>| {
        served_at(window, pane).map(|(socket, _)| socket) == Some(projects.socket.clone())
    };
    // It has the keys: it stays.
    window.update(|w, _, cx| w.tidy_older_servers(cx));
    assert!(on_older(&window), "the focused pane isn't touched");
    // Another tab in front, but you typed in it and didn't send it: it stays.
    window.update(|w, window, cx| {
        w.new_terminal(window, cx);
        w.terminal(pane).unwrap().update(cx, |view, cx| {
            gpui::EntityInputHandler::replace_text_in_range(view, None, "draft", window, cx)
        });
        w.tidy_older_servers(cx);
    });
    assert!(on_older(&window), "a pane with a draft isn't touched");
    // The draft cleared, nothing else runs on the older server, and you're not in the pane: it moves.
    window.update(|w, window, cx| {
        w.terminal(pane).unwrap().update(cx, |view, cx| {
            gpui::EntityInputHandler::replace_text_in_range(view, None, "\u{3}", window, cx)
        });
        w.tidy_older_servers(cx);
    });
    let (socket, _) = served_at(&window, pane).expect("still in a server");
    assert_eq!(socket, newer, "on the current server");
    window.read(|w, cx| {
        assert!(!w.on_older_server(pane, cx));
        assert!(w.older_server_marker(pane, cx).is_none());
        assert_eq!(w.panes[&pane].history, history, "the same pane");
    });
    assert!(
        shows(&window, "first"),
        "with what it printed above the new shell"
    );
    wait_served(&projects.socket, &history, None);
    wait_served(&newer, &history, Some(false));
    drop((window, app));
    end_at(&newer);
    projects.end();
}

#[test]
fn a_pane_running_a_program_keeps_the_older_server_and_its_neighbours_wait() {
    let projects = TwoProjects::new("wsbz");
    let (mut app, mut window) = projects.launch();
    let pane = window.read(|w, _| w.focused_pane().unwrap());
    window.update(|w, window, cx| w.split(crate::layout::Dir::Row, window, cx));
    let busy = window.read(|w, _| w.focused_pane().unwrap());
    run_in(&app, &mut window, busy, &projects.socket, "sleep 60");
    quit(&mut app);
    drop((window, app));

    let (app, mut window, newer) = projects.launch_updated();
    // Another tab in front, so the pane isn't the one with the keys.
    window.update(|w, window, cx| {
        w.new_terminal(window, cx);
        w.tidy_older_servers(cx);
    });
    for pane in [pane, busy] {
        assert_eq!(
            served_at(&window, pane).map(|(socket, _)| socket),
            Some(projects.socket.clone()),
            "nothing moves while a program runs on the older server"
        );
    }
    drop((window, app));
    end_at(&newer);
    projects.end();
}

#[test]
fn an_idle_agent_on_an_older_server_exits_and_resumes_its_session_on_the_current_one() {
    let projects = TwoProjects::new("wsag");
    let (mut app, mut window) = projects.launch();
    // A made-up agent that ends when it reads `/exit`, as Claude Code does.
    let (pane, pid) = start_agent_running(&projects, &app, &mut window, "read line; true");
    std::fs::write(
        projects.claude.join(format!("sessions/{pid}.json")),
        format!(r#"{{"sessionId":"{SESSION}","status":"idle"}}"#),
    )
    .unwrap();
    quit(&mut app);
    drop((window, app));

    let (app, mut window, newer) = projects.launch_updated();
    // Another tab in front, so the pane isn't the one with the keys.
    window.update(|w, window, cx| w.new_terminal(window, cx));
    wait_for(
        &app,
        &window,
        |w| w.agents.get(&pane).is_some_and(|agent| agent.direct),
        "the agent on the older server was never detected",
    );
    wait_for(
        &app,
        &window,
        |w| w.older.exiting.contains_key(&pane) || !w.agents.contains_key(&pane),
        "the idle agent was never told to exit",
    );
    wait_for(
        &app,
        &window,
        |_| served_at(&window, pane).is_some_and(|(socket, _)| socket == newer),
        "the pane never moved to the current server",
    );
    window.read(|w, cx| {
        assert_eq!(
            w.terminal(pane).unwrap().read(cx).written(),
            [resume_line()],
            "its session resumes in the new shell"
        );
        assert_eq!(w.panes[&pane].session, Some(agent_session()));
    });
    assert!(
        unsafe { libc::kill(pid as i32, 0) } != 0,
        "the agent ended on its own, not killed while working"
    );
    drop((window, app));
    end_at(&newer);
    projects.end();
}

#[test]
fn settings_lists_the_servers_and_stops_an_older_one_after_asking() {
    let projects = TwoProjects::new("wsst");
    let (mut app, mut window) = projects.launch();
    let pane = window.read(|w, _| w.focused_pane().unwrap());
    run_in(&app, &mut window, pane, &projects.socket, "sleep 60");
    quit(&mut app);
    drop((window, app));

    let (app, mut window, newer) = projects.launch_updated();
    window.update(|w, window, cx| w.open_settings(window, cx));
    let rows = window.read(|w, _| w.older.list.clone());
    assert_eq!(
        rows.iter()
            .map(|row| (row.version, row.current, row.running, row.shown))
            .collect::<Vec<_>>(),
        [
            (wings_ptyd::SERVER_VERSION + 1, true, 0, 0),
            (wings_ptyd::SERVER_VERSION, false, 1, 1)
        ]
    );
    window.draw();
    window.update(|w, window, cx| {
        let focus = cx.focus_handle();
        w.older.stopping = Some((projects.socket.clone(), focus));
        w.stop_server_dialog(cx).expect("it asks first");
        w.stop_server(window, cx);
    });
    wait_for(
        &app,
        &window,
        |w| !w.panes.contains_key(&pane),
        "the pane on the stopped server never closed",
    );
    window.update(|w, _, cx| w.refresh_servers(cx));
    window.read(|w, _| {
        assert!(w.older.stopping.is_none());
        assert!(
            w.older.list.iter().all(|row| row.current),
            "{:?}",
            w.older.list
        );
    });
    drop((window, app));
    end_at(&newer);
    projects.end();
}

/// Runs `command` in the pane once its shell takes input, and waits until it runs in front of the shell in the server
/// at `socket`. Typed before the shell is ready, a line can be lost.
fn run_in(
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
    pane: usize,
    socket: &Path,
    command: &str,
) {
    let write = |window: &mut TestAppWindow<Workspace>, text: String| {
        window.update(|w, _, cx| {
            w.terminal(pane)
                .unwrap()
                .update(cx, |view, cx| view.write_input(&text, cx))
        })
    };
    write(window, "echo re''ady\r".into());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !window.read(|w, cx| {
        String::from_utf8_lossy(&w.terminal(pane).unwrap().read(cx).recent_output())
            .contains("ready")
    }) {
        assert!(
            std::time::Instant::now() < deadline,
            "the shell never took input"
        );
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    write(window, format!("{command}\r"));
    let history = window.read(|w, _| w.panes[&pane].history.clone());
    loop {
        let running = wings_ptyd::client::Control::connect(socket)
            .unwrap()
            .list()
            .unwrap()
            .into_iter()
            .any(|info| {
                info.history_id.as_deref() == Some(history.as_str())
                    && !terminal_server::idle_shell(&info)
            });
        if running {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{command:?} never ran"
        );
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Ends every pane the server at `socket` runs.
fn end_at(socket: &Path) {
    if let Ok(mut control) = wings_ptyd::client::Control::connect(socket) {
        for pane in control.list().unwrap_or_default() {
            let _ = control.kill(pane.pane);
        }
    }
}

/// A pane whose agent was asleep when Wings quit starts its shell without resuming the session, says it sleeps, and
/// resumes once its tab is shown, as the desktop kept a sleeping agent asleep until opened.
#[test]
fn an_agent_asleep_at_quit_stays_asleep_until_its_tab_is_shown() {
    use crate::workspace::agent_list::Phase;
    let dir = PathBuf::from("/tmp").join(format!("wsz-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let claude = dir.join("claude");
    std::fs::create_dir_all(claude.join("sessions")).unwrap();
    write_transcript(&claude);
    let mut saved = saved_agent_pane(&dir, "asleep-agent", agent_session());
    if let crate::saved::SavedLayout::Pane { asleep, .. } = &mut saved.tabs[0].layout {
        *asleep = true;
    }
    // A second tab is the one on screen.
    saved.tabs.push(crate::saved::SavedTab {
        layout: crate::saved::SavedLayout::Pane {
            cwd: Some(dir.clone()),
            history: Some("shown".into()),
            name: None,
            asleep: false,
            claude_account: None,
            session: None,
        },
        focused: 0,
        zoomed: None,
    });
    saved.active = Some(1);
    let state = dir.join("workspace.json");
    crate::saved::save(
        &state,
        &dir.to_string_lossy(),
        saved,
        crate::saved::Settings::default(),
    )
    .unwrap();

    let project = dir.clone();
    let (_app, mut window) = open_config(move |config| {
        config.project = project;
        config.state = Some(state);
        config.agents = Some(claude);
        config.terminal_server = None;
    });
    let (pane, first) = window.read(|w, cx| {
        let tab = &w.terminals[0];
        let pane = tab.focused;
        assert_eq!(
            w.terminal(pane).unwrap().read(cx).written(),
            Vec::<String>::new()
        );
        let rows = w.pending_agents(cx.background_executor().now());
        assert_eq!(rows[0].phase, Phase::Sleeping);
        (pane, tab.id)
    });
    window.update(|w, window, cx| w.show(crate::workspace::Active::Terminal(first), window, cx));
    window.read(|w, cx| {
        assert_eq!(
            w.terminal(pane).unwrap().read(cx).written(),
            [resume_line()]
        );
    });
    drop(window);
    let _ = std::fs::remove_dir_all(&dir);
}
