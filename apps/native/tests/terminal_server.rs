//! The terminal server as the app runs it: the real binary, started as a plain child in a temp folder, or detached as
//! a launchd job would start it.

#![cfg(unix)]

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use wings_native::terminal_server::{self, Servers, Start};
use wings_ptyd::{
    SERVER_VERSION,
    client::{Control, Event, Pane},
    proto::Spawn,
};

const BIN: &str = env!("CARGO_BIN_EXE_wings-terminal-server");

/// A folder for one test's servers, short enough for a socket path (104 bytes), which ends them when it goes.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let dir = PathBuf::from("/tmp").join(format!("wts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Folder(dir)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        // Servers sit at the top or one folder down (a data folder's `srv/`), and each left running keeps its shells.
        let subfolders = std::fs::read_dir(&self.0)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir());
        for folder in std::iter::once(self.0.clone()).chain(subfolders) {
            for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                if entry.file_name().to_string_lossy().ends_with(".sock")
                    && let Ok(mut control) = Control::connect(&entry.path())
                {
                    for pane in control.list().unwrap_or_default() {
                        let _ = control.kill(pane.pane);
                    }
                    let _ = control.drain();
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.0);
        if std::thread::panicking() {
            return;
        }
        // Each of them exits once drained, so one still running after a while is a server the test lost track of.
        let pattern = format!("{}/", self.0.display());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let out = Command::new("/usr/bin/pgrep")
                .arg("-f")
                .arg(&pattern)
                .output()
                .unwrap();
            let left: Vec<i32> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|line| line.trim().parse().ok())
                .collect();
            if left.is_empty() {
                break;
            }
            if Instant::now() >= deadline {
                for &pid in &left {
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                }
                panic!("servers left running in {pattern}: {left:?}");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn spawn_request(history_id: &str) -> Spawn {
    Spawn {
        space_id: "/project".into(),
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 24,
        history_id: Some(history_id.into()),
        // The app's own panes get `pane_env`; this test's environment is enough for a shell.
        env: std::env::vars().collect(),
    }
}

/// Reads events until one carries `text`.
fn wait_for(pane: &Pane, events: &mut wings_ptyd::client::Events, text: &str) {
    let mut seen = String::new();
    while !seen.contains(text) {
        match events.next(pane) {
            Event::Output(bytes) | Event::Replay(bytes) => {
                seen.push_str(&String::from_utf8_lossy(&bytes))
            }
            other => panic!("{other:?} before {text:?}: {seen:?}"),
        }
    }
}

fn server_pid(socket: &Path) -> u32 {
    let out = Command::new("/usr/sbin/lsof")
        .arg("-t")
        .arg(socket)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .find(|&pid| pid != std::process::id())
        .expect("a server process has the socket open")
}

#[test]
fn the_app_starts_a_server_and_the_next_app_finds_its_panes() {
    let folder = Folder::new("start");
    let start = Start::Child(BIN.into());
    let servers = Servers::connect(&folder.0, Some(&start));
    let socket = servers.current().expect("started a server").to_owned();
    assert_eq!(socket, terminal_server::socket(&folder.0, SERVER_VERSION));
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode(&folder.0),
        0o700,
        "only this user may open the server's folder"
    );
    assert_eq!(mode(&socket), 0o600);
    let (pane, mut events) = Pane::spawn(&socket, spawn_request("h-start"), b"").unwrap();
    pane.write(b"echo fi''rst\r").unwrap();
    wait_for(&pane, &mut events, "first");
    pane.detach();

    // The next app: the same server answers, so it starts none, and finds the pane by its history id.
    let again = Servers::connect(&folder.0, Some(&start));
    assert_eq!(again.current(), Some(socket.clone()));
    assert!(again.claim("h-other").is_none());
    let (at, info) = again.claim("h-start").expect("the pane the server kept");
    assert_eq!(
        (at.as_path(), info.shell_pid, info.exited),
        (socket.as_path(), pane.shell_pid(), false)
    );
    assert!(again.claim("h-start").is_none(), "claimed once");
    let (attached, mut events, _) = Pane::attach(&socket, &info).unwrap();
    wait_for(&attached, &mut events, "first");
}

#[test]
fn a_newer_app_drains_an_older_server_and_keeps_its_panes() {
    let folder = Folder::new("drain");
    let start = Start::Child(BIN.into());
    let old = Servers::connect(&folder.0, Some(&start));
    let old_socket = old.current().unwrap().to_owned();
    let (pane, mut events) = Pane::spawn(&old_socket, spawn_request("h-old"), b"").unwrap();
    pane.write(b"echo ol''d\r").unwrap();
    wait_for(&pane, &mut events, "old");
    let old_pid = server_pid(&old_socket);

    // An app whose server is one version newer: the old one drains, and a new one starts for new panes.
    let newer = Servers::connect_as(&folder.0, Some(&start), SERVER_VERSION + 1);
    let new_socket = newer.current().unwrap().to_owned();
    assert_eq!(
        new_socket,
        terminal_server::socket(&folder.0, SERVER_VERSION + 1)
    );
    assert!(
        Pane::spawn(&old_socket, spawn_request("h-refused"), b"").is_err(),
        "the old server starts no pane"
    );
    let (at, info) = newer.claim("h-old").expect("the old server's pane");
    assert_eq!(at, old_socket);
    let (attached, mut again, _) = Pane::attach(&old_socket, &info).unwrap();
    pane.write(b"echo sti''ll\r").unwrap();
    wait_for(&attached, &mut again, "still");
    Pane::spawn(&new_socket, spawn_request("h-new"), b"").expect("new panes go to the new server");

    // Its last pane ends and no app is attached: the old server exits on its own.
    attached.kill().unwrap();
    pane.detach();
    attached.detach();
    let deadline = Instant::now() + Duration::from_secs(10);
    while unsafe { libc::kill(old_pid as i32, 0) } == 0 {
        assert!(
            Instant::now() < deadline,
            "the drained server is still running"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!old_socket.exists());
}

#[test]
fn ending_a_folder_no_app_reads_ends_its_server_and_shells() {
    let folder = Folder::new("end-all");
    let start = Start::Child(BIN.into());
    let socket = Servers::connect(&folder.0, Some(&start))
        .current()
        .unwrap()
        .to_owned();
    let (pane, mut events) = Pane::spawn(&socket, spawn_request("h-left"), b"").unwrap();
    pane.write(b"echo le''ft\r").unwrap();
    wait_for(&pane, &mut events, "left");
    let (server, shell) = (server_pid(&socket), pane.shell_pid().unwrap());
    pane.detach();

    terminal_server::end_all(&folder.0);
    let deadline = Instant::now() + Duration::from_secs(10);
    for (pid, what) in [(shell, "shell"), (server, "server")] {
        while unsafe { libc::kill(pid as i32, 0) } == 0 {
            assert!(Instant::now() < deadline, "the {what} is still running");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    assert!(!socket.exists());
}

#[test]
fn a_killed_server_ends_its_panes_events_and_the_next_app_starts_another() {
    let folder = Folder::new("killed");
    let start = Start::Child(BIN.into());
    let servers = Servers::connect(&folder.0, Some(&start));
    let socket = servers.current().unwrap().to_owned();
    let (pane, mut events) = Pane::spawn(&socket, spawn_request("h-killed"), b"").unwrap();
    pane.write(b"echo up\r").unwrap();
    wait_for(&pane, &mut events, "up");
    let pid = server_pid(&socket);
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match events.next(&pane) {
            Event::Closed => break,
            Event::Output(_) => assert!(Instant::now() < deadline),
            other => panic!("{other:?}"),
        }
    }
    // Its socket file is left behind, and answers nothing: the next app starts a server there.
    let next = Servers::connect(&folder.0, Some(&start));
    assert_eq!(next.current(), Some(socket.clone()));
    assert!(next.claim("h-killed").is_none(), "its panes went with it");
}

#[test]
fn a_server_killed_while_the_app_runs_is_replaced_by_a_new_one_for_new_panes() {
    let folder = Folder::new("replaced");
    let servers = Servers::connect(&folder.0, Some(&Start::Child(BIN.into())));
    let socket = servers.current().unwrap();
    let old = server_pid(&socket);
    assert_eq!(unsafe { libc::kill(old as i32, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Control::connect(&socket).is_ok() {
        assert!(Instant::now() < deadline, "the killed server still answers");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        Pane::spawn(&socket, spawn_request("h-new"), b"").is_err(),
        "nothing answers at the old socket"
    );

    let replaced = servers.replace(&socket).expect("a new server");
    assert_eq!(servers.current(), Some(replaced.clone()));
    let new = server_pid(&replaced);
    assert_ne!(new, old);
    let (pane, mut events) = Pane::spawn(&replaced, spawn_request("h-new"), b"").unwrap();
    pane.write(b"echo n''ew\r").unwrap();
    wait_for(&pane, &mut events, "new");
    assert_eq!(
        ps(pane.shell_pid().unwrap(), "ppid"),
        new,
        "the shell runs in the new server"
    );
    // A pane that also found the old server gone takes the new one, not a third.
    assert_eq!(servers.replace(&socket), Some(replaced));
}

#[test]
fn with_no_server_to_start_new_panes_run_in_the_app() {
    let folder = Folder::new("none");
    let servers = Servers::connect(&folder.0, None);
    assert_eq!(servers.current(), None);
    let broken = Start::Child(folder.0.join("missing-binary"));
    let servers = Servers::connect(&folder.0, Some(&broken));
    assert_eq!(servers.current(), None);
    assert_eq!(
        servers.replace(&terminal_server::socket(&folder.0, SERVER_VERSION)),
        None
    );
}

#[test]
fn orphans_are_running_panes_of_the_project_no_saved_pane_names() {
    let folder = Folder::new("orphans");
    let start = Start::Child(BIN.into());
    let socket = Servers::connect(&folder.0, Some(&start))
        .current()
        .unwrap()
        .to_owned();
    let mut panes = Vec::new();
    for (history, space) in [
        ("h-saved", "/project"),
        ("h-lost", "/project"),
        ("h-elsewhere", "/other"),
    ] {
        let mut request = spawn_request(history);
        request.space_id = space.into();
        panes.push(Pane::spawn(&socket, request, b"").unwrap().0);
    }
    let servers = Servers::connect(&folder.0, Some(&start));
    assert_eq!(servers.orphans("/project", &["h-saved".into()]), ["h-lost"]);
}

/// Always to the same path, under the helper app's own name, and only when the bundle brings another one.
#[test]
fn the_helper_app_is_copied_out_of_the_bundle_to_the_same_path_each_time() {
    let folder = Folder::new("install");
    let bundled = bundle(&folder, "a", "1");
    let copy = folder.0.join(format!("{}.app", terminal_server::HELPER));
    let program = terminal_server::install(&bundled, &folder.0).unwrap();
    assert_eq!(program, terminal_server::executable(&copy));
    assert_eq!(
        std::fs::read(&program).unwrap(),
        std::fs::read(BIN).unwrap()
    );
    let first = std::fs::metadata(&program).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    terminal_server::install(&bundle(&folder, "b", "1"), &folder.0).unwrap();
    assert_eq!(
        std::fs::metadata(&program).unwrap().modified().unwrap(),
        first,
        "the same helper app isn't copied again"
    );

    // An update brings another helper app: it takes the same path, and the server running from the old copy goes on
    // while it has a pane, from the copy put aside.
    let socket = terminal_server::socket(&folder.0, SERVER_VERSION);
    let mut running = serve(&program, &socket).spawn().unwrap();
    answers(&socket);
    let (pane, _events) = Pane::spawn(&socket, spawn_request("h-keep"), b"").unwrap();
    let newer = bundle(&folder, "c", "2");
    assert_eq!(
        terminal_server::install(&newer, &folder.0).unwrap(),
        program
    );
    let plist = |app: &Path| std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
    assert_eq!(plist(&copy), plist(&newer));
    let aside = put_aside(&folder.0);
    assert_eq!(plist(&aside), plist(&bundled), "the old copy is kept");
    #[cfg(target_os = "macos")]
    assert_eq!(
        executable(running.id()),
        terminal_server::executable(&aside.canonicalize().unwrap())
    );
    // Past the second in which it looks at its file.
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        Control::connect(&socket).is_ok(),
        "the old server still answers"
    );
    pane.kill().unwrap();
    let _ = running.kill();
    let _ = running.wait();
}

/// A build's app bundle's helper app, `Contents/Library/<HELPER>.app` holding its own server binary, as each update
/// brings a new one and deletes the old one. Builds of the same `version` bring the same helper app.
fn bundle(folder: &Folder, build: &str, version: &str) -> PathBuf {
    let app = folder
        .0
        .join(build)
        .join(format!("{}.app", terminal_server::HELPER));
    let program = terminal_server::executable(&app);
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::copy(BIN, &program).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!("<plist><dict><key>CFBundleVersion</key><string>{version}</string></dict></plist>"),
    )
    .unwrap();
    app
}

/// The one helper app `install` put aside in `dir`.
fn put_aside(dir: &Path) -> PathBuf {
    let aside: Vec<_> = std::fs::read_dir(dir.join("replaced"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(aside.len(), 1, "{aside:?}");
    aside[0].join(format!("{}.app", terminal_server::HELPER))
}

/// The executable `pid` runs.
#[cfg(target_os = "macos")]
fn executable(pid: u32) -> PathBuf {
    let mut path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let count =
        unsafe { libc::proc_pidpath(pid as i32, path.as_mut_ptr().cast(), path.len() as u32) };
    assert!(count > 0, "no path for {pid}");
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(&path[..count as usize]))
}

/// A pane the next app attached: it shows what it printed, and still runs what it's sent.
fn still_runs(socket: &Path, info: &wings_ptyd::proto::PaneInfo, before: &str, word: &str) -> Pane {
    let (pane, mut events, _) = Pane::attach(socket, info).unwrap();
    wait_for(&pane, &mut events, before);
    let typed = format!("echo {}''{}\r", &word[..1], &word[1..]);
    pane.write(typed.as_bytes()).unwrap();
    wait_for(&pane, &mut events, word);
    pane
}

/// An update as the updater runs one, without its network or launchd. An app from before the helper app left its
/// server running from a bare copy in `bin/`. Build A's app drains it, starts its server from the copy of its helper
/// app and opens panes, quits, and its bundle is deleted. Build B, with the same helper app, finds the server, attaches
/// its panes and copies nothing. Build C ships a newer server version: A's server drains and keeps its panes, running
/// from its copy put aside, which C and its next launch still attach, and new panes go to C's server. Each old copy
/// goes once its server does.
#[test]
fn an_update_keeps_the_panes_running_and_the_new_app_attaches_them_again() {
    let folder = Folder::new("update");
    let dir = folder.0.join("srv");
    let program =
        terminal_server::executable(&dir.join(format!("{}.app", terminal_server::HELPER)));

    // The app from before the helper app: its server, an older version, with a pane.
    let old = SERVER_VERSION - 1;
    let legacy = dir.join("bin").join(terminal_server::BINARY);
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::copy(BIN, &legacy).unwrap();
    let legacy_socket = terminal_server::socket(&dir, old);
    let mut legacy_server = serve(&legacy, &legacy_socket)
        .arg("--server-version")
        .arg(old.to_string())
        .spawn()
        .unwrap();
    answers(&legacy_socket);
    let (legacy_pane, _events) =
        Pane::spawn(&legacy_socket, spawn_request("h-legacy"), b"").unwrap();

    let a = bundle(&folder, "a", "1");
    let servers = Servers::connect(&dir, Some(&Start::Direct { bundled: a.clone() }));
    let socket = servers.current().expect("build A started a server");
    assert_eq!(socket, terminal_server::socket(&dir, SERVER_VERSION));
    let pid = server_pid(&socket);
    #[cfg(target_os = "macos")]
    assert_eq!(
        executable(pid),
        program.canonicalize().unwrap(),
        "it runs from the copy, not the bundle"
    );
    assert!(
        legacy.is_file(),
        "the old app's copy stays while its server runs"
    );
    let mut opened = Vec::new();
    for (history, word) in [("h-one", "one"), ("h-two", "two")] {
        let (pane, mut events) = Pane::spawn(&socket, spawn_request(history), b"").unwrap();
        pane.write(format!("echo {}''{}\r", &word[..1], &word[1..]).as_bytes())
            .unwrap();
        wait_for(&pane, &mut events, word);
        opened.push(pane);
    }
    let shells: Vec<_> = opened.iter().map(Pane::shell_pid).collect();
    let copied_at = std::fs::metadata(&program).unwrap().modified().unwrap();
    // The app quits to install the update: its panes let go, and the old bundle is deleted.
    drop(servers);
    for pane in opened {
        pane.detach();
    }
    std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    // The old app's last pane ends, and its drained server with it.
    legacy_pane.kill().unwrap();
    legacy_pane.detach();
    legacy_server.wait().unwrap();

    // Build B: the same server answers, with the same panes, and nothing new starts.
    let b = bundle(&folder, "b", "1");
    let servers = Servers::connect(&dir, Some(&Start::Direct { bundled: b }));
    assert_eq!(servers.current(), Some(socket.clone()));
    assert_eq!(server_pid(&socket), pid, "the same server");
    let mut attached = Vec::new();
    for (history, before, word) in [("h-one", "one", "again"), ("h-two", "two", "again")] {
        let (at, info) = servers.claim(history).expect("the pane the server kept");
        assert_eq!(at, socket);
        attached.push(still_runs(&at, &info, before, word));
    }
    assert_eq!(
        attached.iter().map(Pane::shell_pid).collect::<Vec<_>>(),
        shells,
        "the same shells"
    );
    assert_eq!(
        std::fs::metadata(&program).unwrap().modified().unwrap(),
        copied_at,
        "the copy is kept"
    );
    assert!(
        !dir.join("bin").exists(),
        "the old app's copy is gone with its server"
    );
    drop(servers);
    for pane in attached {
        pane.detach();
    }

    // Build C ships a newer server: the old one drains and keeps its panes from the copy put aside, and C starts its
    // own from the same path.
    let newer = SERVER_VERSION + 1;
    let c = bundle(&folder, "c", "2");
    let start = Start::Direct { bundled: c };
    let servers = Servers::connect_as(&dir, Some(&start), newer);
    let new_socket = servers.current().expect("build C started its server");
    assert_eq!(new_socket, terminal_server::socket(&dir, newer));
    assert_eq!(
        Control::connect(&new_socket)
            .unwrap()
            .welcome()
            .server_version,
        newer
    );
    assert!(program.is_file(), "the newer server's copy");
    let aside = put_aside(&dir);
    #[cfg(target_os = "macos")]
    assert_eq!(
        executable(pid),
        terminal_server::executable(&aside.canonicalize().unwrap()),
        "the old server runs from its copy put aside"
    );
    assert!(
        Pane::spawn(&socket, spawn_request("h-refused"), b"").is_err(),
        "the old server starts no pane"
    );
    let (at, info) = servers.claim("h-one").expect("the old server's pane");
    assert_eq!(at, socket);
    still_runs(&at, &info, "again", "still").detach();
    let (new_pane, mut events) = Pane::spawn(&new_socket, spawn_request("h-new"), b"").unwrap();
    new_pane.write(b"echo n''ew\r").unwrap();
    wait_for(&new_pane, &mut events, "new");
    drop(servers);
    new_pane.detach();

    // C's next launch keeps its own server, and attaches the panes of both.
    let servers = Servers::connect_as(&dir, Some(&start), newer);
    assert_eq!(servers.current(), Some(new_socket.clone()));
    assert!(
        aside.is_dir(),
        "the copy put aside stays while its server runs"
    );
    assert_eq!(
        servers.claim("h-new").map(|(at, _)| at),
        Some(new_socket.clone())
    );
    let mut old = Vec::new();
    for history in ["h-one", "h-two"] {
        let (at, info) = servers.claim(history).expect("the old server's pane");
        assert_eq!(at, socket);
        old.push(still_runs(&at, &info, "again", "later"));
    }
    Pane::spawn(&new_socket, spawn_request("h-newer"), b"")
        .expect("C's server still takes new panes");

    // The old panes end: the drained server exits, and the next launch deletes its copy.
    for pane in old {
        pane.kill().unwrap();
        pane.detach();
    }
    drop(servers);
    let deadline = Instant::now() + Duration::from_secs(10);
    while unsafe { libc::kill(pid as i32, 0) } == 0 {
        assert!(
            Instant::now() < deadline,
            "the drained server is still running"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let servers = Servers::connect_as(&dir, Some(&start), newer);
    assert_eq!(servers.current(), Some(new_socket));
    assert!(
        !dir.join("replaced").exists(),
        "the copy put aside is gone with its server"
    );
    assert!(program.is_file());
}

/// Turned off, an app starts no server and runs new panes in itself, yet finds the panes a server runs. Turned on, it
/// starts one.
#[test]
fn turned_off_the_app_starts_no_server_but_finds_the_panes_one_runs() {
    let folder = Folder::new("off");
    let start = Start::Child(BIN.into());
    let off = Servers::connect_off(&folder.0, Some(&start));
    assert_eq!(off.current(), None);
    assert!(!off.is_on());
    assert!(
        !terminal_server::socket(&folder.0, SERVER_VERSION).exists(),
        "no server started"
    );

    let socket = Servers::connect(&folder.0, Some(&start)).current().unwrap();
    let (pane, mut events) = Pane::spawn(&socket, spawn_request("h-kept"), b"").unwrap();
    pane.write(b"echo ke''pt\r").unwrap();
    wait_for(&pane, &mut events, "kept");
    pane.detach();
    let off = Servers::connect_off(&folder.0, Some(&start));
    assert_eq!(off.current(), None, "new panes run in the app");
    let (at, info) = off.claim("h-kept").expect("the pane the server runs");
    still_runs(&at, &info, "kept", "attached");

    off.set_on(true);
    off.start_if_none();
    assert_eq!(
        off.current(),
        Some(socket.clone()),
        "new panes go to the server again"
    );
    off.set_on(false);
    assert_eq!(off.current(), None);
    assert!(
        Control::connect(&socket).is_ok(),
        "turning it off ends no server"
    );

    // An app off at launch with no server running starts one once turned on.
    let empty = Folder::new("off-empty");
    let servers = Servers::connect_off(&empty.0, Some(&start));
    servers.set_on(true);
    assert_eq!(
        servers.current(),
        None,
        "until one answers, new panes run in the app"
    );
    servers.start_if_none();
    assert_eq!(
        servers.current(),
        Some(terminal_server::socket(&empty.0, SERVER_VERSION))
    );
}

#[test]
fn the_server_is_on_unless_settings_turn_it_off_and_the_variable_wins_over_both() {
    use std::ffi::OsStr;
    let release =
        |env: Option<&str>, saved| terminal_server::enabled_with(env.map(OsStr::new), saved, false);
    assert!(release(None, None), "on by default");
    assert!(release(None, Some(true)));
    assert!(!release(None, Some(false)), "Settings turned it off");
    assert!(release(Some("1"), Some(false)), "1 forces it on");
    assert!(!release(Some("0"), None), "0 forces it off");
    assert!(
        release(Some("yes"), None),
        "anything else leaves it to Settings"
    );
    let debug =
        |env: Option<&str>, saved| terminal_server::enabled_with(env.map(OsStr::new), saved, true);
    assert!(
        !debug(None, None) && !debug(None, Some(true)),
        "a debug build keeps panes in the app"
    );
    assert!(debug(Some("1"), Some(false)));
}

/// `ps`'s `field` for `pid`.
fn ps(pid: u32, field: &str) -> u32 {
    let out = Command::new("/bin/ps")
        .args(["-o", &format!("{field}="), "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

/// The process macOS holds responsible for `pid`'s privacy access (`launchctl procinfo` says the same, as root).
#[cfg(target_os = "macos")]
fn responsible(pid: u32) -> u32 {
    type Responsible = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;
    let symbol = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            c"responsibility_get_pid_responsible_for_pid".as_ptr(),
        )
    };
    assert!(!symbol.is_null());
    let get = unsafe { std::mem::transmute::<*mut libc::c_void, Responsible>(symbol) };
    unsafe { get(pid as libc::pid_t) as u32 }
}

/// Waits for a server to answer at `socket`, and gives its pid.
fn answers(socket: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Control::connect(socket).is_err() {
        assert!(Instant::now() < deadline, "no server at {socket:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    server_pid(socket)
}

/// The server's own command, as the release app starts it, from outside the protected `~/Desktop`.
fn serve(program: &Path, socket: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.arg("serve")
        .arg("--socket")
        .arg(socket)
        .current_dir("/");
    cmd
}

/// `check-access` lists Chrome's data folder: 0 when it can, 1 when it's refused, 3 without Chrome. A folder this user
/// can't open stands in for one macOS refuses, with the same error.
#[test]
fn the_server_says_whether_its_shells_can_read_other_apps_data() {
    let folder = Folder::new("access");
    let home = folder.0.join("home");
    let chrome = home.join("Library/Application Support/Google/Chrome");
    std::fs::create_dir_all(&chrome).unwrap();
    let check = || {
        Command::new(BIN)
            .arg("check-access")
            .env("HOME", &home)
            .status()
            .unwrap()
            .code()
    };
    assert_eq!(check(), Some(0));
    std::fs::set_permissions(&chrome, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(check(), Some(1));
    std::fs::set_permissions(&chrome, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::remove_dir_all(home.join("Library")).unwrap();
    assert_eq!(check(), Some(3), "nothing to tell from");
}

/// A release app's server: the helper app's copy outside the bundle, started directly as when launchd fails. Disclaimed, the
/// server needs its own grant to read a binary under `~/Desktop`, so it runs the copy in the temp folder as the app does.
#[cfg(target_os = "macos")]
#[test]
fn a_release_server_owns_its_privacy_responsibility_and_its_shells_inherit_it() {
    let folder = Folder::new("resp");
    std::fs::create_dir_all(&folder.0).unwrap();
    let socket = terminal_server::socket(&folder.0, SERVER_VERSION);
    let cmd = serve(
        &terminal_server::install(&bundle(&folder, "app", "1"), &folder.0).unwrap(),
        &socket,
    );
    terminal_server::start_detached(cmd, &folder.0.join("v.log"), |_, _| {
        Err(std::io::Error::other("no launchd in tests"))
    })
    .unwrap();
    let pid = answers(&socket);
    assert_eq!(responsible(pid), pid, "not the test's responsible process");
    assert_ne!(responsible(std::process::id()), pid);
    let (pane, mut events) = Pane::spawn(&socket, spawn_request("h-resp"), b"").unwrap();
    pane.write(b"echo u''p\r").unwrap();
    wait_for(&pane, &mut events, "up");
    assert_eq!(responsible(pane.shell_pid().unwrap()), pid);
}

#[cfg(target_os = "macos")]
#[test]
fn a_debug_apps_child_server_keeps_the_apps_responsibility() {
    let folder = Folder::new("child");
    let servers = Servers::connect(&folder.0, Some(&Start::Child(BIN.into())));
    let pid = server_pid(&servers.current().unwrap());
    assert_eq!(responsible(pid), responsible(std::process::id()));
}

/// The launchd job's program run as a plain child instead of a job: it forks, the forked server detaches into a
/// session of its own under pid 1, takes the app's environment over the `ready` socket and serves.
#[cfg(target_os = "macos")]
#[test]
fn the_launchd_job_detaches_the_server_from_the_app() {
    let folder = Folder::new("launchd");
    std::fs::create_dir_all(&folder.0).unwrap();
    let socket = terminal_server::socket(&folder.0, SERVER_VERSION);
    let log = folder.0.join("v.log");
    // `/tmp`, as in a release app: the job's folder under `folder` would be too long for the `ready` socket.
    let program = terminal_server::install(&bundle(&folder, "app", "1"), &folder.0).unwrap();
    terminal_server::start_detached(serve(&program, &socket), &log, |cmd, log| {
        wings_ptyd::launchd::spawn_without_launchd(cmd, log, Path::new("/tmp"))
    })
    .unwrap();
    let pid = answers(&socket);
    assert_eq!(
        ps(pid, "ppid"),
        1,
        "the job's process exited and left it to launchd"
    );
    assert_eq!(ps(pid, "pgid"), pid, "a session of its own");
    assert_eq!(responsible(pid), pid);
    let log = std::fs::read_to_string(&log).unwrap();
    assert!(!log.contains("starts directly"), "{log}");
}

#[test]
fn when_launchd_fails_the_server_starts_directly_in_its_own_session() {
    let folder = Folder::new("direct");
    std::fs::create_dir_all(&folder.0).unwrap();
    let socket = terminal_server::socket(&folder.0, SERVER_VERSION);
    let log = folder.0.join("v.log");
    let program = terminal_server::install(&bundle(&folder, "app", "1"), &folder.0).unwrap();
    terminal_server::start_detached(serve(&program, &socket), &log, |_, _| {
        Err(std::io::Error::other("bootstrap refused"))
    })
    .unwrap();
    let pid = answers(&socket);
    assert_eq!(ps(pid, "ppid"), std::process::id());
    assert_eq!(ps(pid, "pgid"), pid);
    let log = std::fs::read_to_string(&log).unwrap();
    assert!(
        log.contains("starts directly and may end with the app: bootstrap refused"),
        "{log}"
    );
}

/// Keystroke echo and output throughput, a shell in this process against one in the server: run with
/// `cargo test --release --test terminal_server -- --ignored --nocapture bench`.
#[cfg(feature = "ui")]
mod bench {
    use super::*;
    use std::{io::Read, sync::mpsc, thread};
    use wings_terminal::{CommandBuilder, PtyProcess, PtySize};

    /// What both sides run: `cat` in raw mode echoes each key at once, and `yes` floods.
    trait Side {
        fn write(&self, bytes: &[u8]);
    }

    impl Side for PtyProcess {
        fn write(&self, bytes: &[u8]) {
            PtyProcess::write(self, bytes).unwrap();
        }
    }

    impl Side for Pane {
        fn write(&self, bytes: &[u8]) {
            Pane::write(self, bytes).unwrap();
        }
    }

    fn local() -> (PtyProcess, mpsc::Receiver<(Instant, usize)>) {
        let mut command = CommandBuilder::new("/bin/sh");
        command.env("PS1", "");
        let process = PtyProcess::spawn_command(command, PtySize::default()).unwrap();
        let mut reader = process.reader().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0; 16 << 10];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send((Instant::now(), n)).is_err() {
                    break;
                }
            }
        });
        (process, rx)
    }

    fn remote(socket: &Path) -> (std::sync::Arc<Pane>, mpsc::Receiver<(Instant, usize)>) {
        let mut request = spawn_request("bench");
        request.env.retain(|(k, _)| k != "SHELL");
        request.env.extend([
            ("SHELL".into(), "/bin/sh".into()),
            ("PS1".into(), String::new()),
        ]);
        let (pane, mut events) = Pane::spawn(socket, request, b"").unwrap();
        let pane = std::sync::Arc::new(pane);
        let reading = pane.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            while let Event::Output(bytes) = events.next(&reading) {
                if tx.send((Instant::now(), bytes.len())).is_err() {
                    break;
                }
            }
        });
        (pane, rx)
    }

    fn drain(rx: &mpsc::Receiver<(Instant, usize)>) {
        while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
    }

    fn echo(side: &dyn Side, rx: &mpsc::Receiver<(Instant, usize)>) -> Vec<f64> {
        side.write(b"stty raw -echo; exec cat\n");
        drain(rx);
        let mut micros: Vec<f64> = (0..2000)
            .map(|_| {
                let start = Instant::now();
                side.write(b"k");
                let (at, _) = rx.recv_timeout(Duration::from_secs(5)).expect("no echo");
                thread::sleep(Duration::from_micros(500));
                at.duration_since(start).as_secs_f64() * 1e6
            })
            .collect();
        micros.sort_by(|a, b| a.partial_cmp(b).unwrap());
        micros
    }

    /// Seconds to receive `bytes` of `yes`.
    fn flood(side: &dyn Side, rx: &mpsc::Receiver<(Instant, usize)>, bytes: usize) -> f64 {
        drain(rx);
        let start = Instant::now();
        side.write(format!("yes 0123456789 | head -c {bytes}\n").as_bytes());
        let mut got = 0;
        while got < bytes {
            got += rx
                .recv_timeout(Duration::from_secs(30))
                .expect("the flood stalled")
                .1;
        }
        start.elapsed().as_secs_f64()
    }

    fn quantile(sorted: &[f64], q: f64) -> f64 {
        sorted[((sorted.len() - 1) as f64 * q) as usize]
    }

    #[test]
    #[ignore]
    fn bench_echo_and_throughput_local_against_the_server() {
        let folder = Folder::new("bench");
        let socket = Servers::connect(&folder.0, Some(&Start::Child(BIN.into())))
            .current()
            .unwrap()
            .to_owned();
        const BYTES: usize = 50 << 20;
        for round in 0..3 {
            let (process, rx) = local();
            let throughput = flood(&process, &rx, BYTES);
            let micros = echo(&process, &rx);
            println!(
                "round {round} local:  echo p50 {:.0} us p95 {:.0} us p99 {:.0} us; 50 MiB in {throughput:.2} s ({:.0} MiB/s)",
                quantile(&micros, 0.5),
                quantile(&micros, 0.95),
                quantile(&micros, 0.99),
                50.0 / throughput
            );
            process.kill();
            let (pane, rx) = remote(&socket);
            let throughput = flood(pane.as_ref(), &rx, BYTES);
            let micros = echo(pane.as_ref(), &rx);
            println!(
                "round {round} server: echo p50 {:.0} us p95 {:.0} us p99 {:.0} us; 50 MiB in {throughput:.2} s ({:.0} MiB/s)",
                quantile(&micros, 0.5),
                quantile(&micros, 0.95),
                quantile(&micros, 0.99),
                50.0 / throughput
            );
            pane.kill().unwrap();
        }
    }
}
