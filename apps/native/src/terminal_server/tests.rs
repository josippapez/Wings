//! The server logic that keeps terminals alive and correct, against servers that run inside the test (`host::serve` on
//! a thread, saying any version), so nothing starts a process. `tests/terminal_server.rs` runs the real binary.

use std::{
    fs,
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use wings_ptyd::{
    SERVER_VERSION,
    client::{Control, Event, Events, Pane},
    host::{self, Stopped},
    proto::{PaneInfo, Spawn},
};

use super::*;

/// A short folder (a socket path is limited to 104 bytes) for one test, with its servers drained and removed after.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let dir = PathBuf::from("/tmp").join(format!("wts-unit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Folder(dir)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        for entry in fs::read_dir(&self.0).into_iter().flatten().flatten() {
            if let Ok(mut control) = Control::connect(&entry.path()) {
                for pane in control.list().unwrap_or_default() {
                    let _ = control.kill(pane.pane);
                }
                let _ = control.drain();
            }
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A server in `dir` that says it is `version`, and why it stopped once it has.
fn serve(dir: &Path, version: u32) -> (PathBuf, Arc<Mutex<Option<Stopped>>>) {
    let socket = socket(dir, version);
    let listener = host::bind(&socket).unwrap();
    let stopped = Arc::new(Mutex::new(None));
    let (result, serving) = (stopped.clone(), socket.clone());
    thread::spawn(move || {
        let config = host::Config {
            version,
            ..host::Config::default()
        };
        *result.lock().unwrap() = Some(host::serve(listener, &serving, config));
    });
    (socket, stopped)
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn spawn_request(history_id: &str) -> Spawn {
    Spawn {
        space_id: "/project".into(),
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 24,
        history_id: Some(history_id.into()),
        replay_bytes: None,
        env: std::env::vars().collect(),
    }
}

/// Reads events until one carries `text`, and returns everything read.
fn read_until(pane: &Pane, events: &mut Events, text: &str) -> String {
    let mut seen = String::new();
    while !seen.contains(text) {
        match events.next(pane) {
            Event::Output(bytes) | Event::Replay(bytes, _) => {
                seen.push_str(&String::from_utf8_lossy(&bytes))
            }
            other => panic!("{other:?} before {text:?}: {seen:?}"),
        }
    }
    seen
}

fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn app_log(dir: &Path) -> String {
    fs::read_to_string(dir.join(LOG)).unwrap_or_default()
}

#[test]
fn a_socket_names_its_server_version_and_nothing_else_does() {
    let version = |name: &str| socket_version(Path::new("/x").join(name).as_path());
    assert_eq!(version("v10.sock"), Some(10));
    assert_eq!(version("v0.sock"), Some(0));
    for other in [
        "v.sock", "10.sock", "vx.sock", "v-1.sock", "v10.sock.bak", "v10.log", "v10", "V10.sock",
    ] {
        assert_eq!(version(other), None, "{other}");
    }
    assert_eq!(socket_version(Path::new("/")), None);
    assert_eq!(socket_version(&socket(Path::new("/d"), 7)), Some(7));
}

#[test]
fn only_a_different_version_asks_the_server_to_say_so() {
    assert!(play_version(SERVER_VERSION).is_empty());
    assert_eq!(
        play_version(SERVER_VERSION + 1),
        ["--server-version", &(SERVER_VERSION + 1).to_string()]
    );
}

#[test]
fn a_shell_counts_as_idle_only_at_its_prompt_and_when_that_can_be_told() {
    let pane = |exited, shell_pid, fg| PaneInfo {
        pane: 1,
        space_id: "p".into(),
        cwd: "/".into(),
        history_id: None,
        shell_pid,
        fg,
        title: String::new(),
        codex_status: None,
        seq: 0,
        exited,
        cols: 80,
        rows: 24,
    };
    assert!(!idle_shell(&pane(true, Some(5), Some(5))), "an exited pane");
    assert!(!idle_shell(&pane(false, None, None)), "no shell pid");
    assert!(!idle_shell(&pane(false, Some(5), Some(9))), "a program in front");
    assert!(!idle_shell(&pane(false, Some(5), None)), "the foreground isn't known");
}

#[test]
fn a_server_that_answers_is_used_and_no_other_is_started() {
    let folder = Folder::new("use");
    let (socket, _) = serve(&folder.0, SERVER_VERSION);
    // A start that can only fail: if the app tried it, its log would say so.
    let servers = Servers::connect(&folder.0, Some(&Start::Child("/nonexistent/server".into())));
    assert_eq!(servers.current(), Some(socket));
    assert_eq!(app_log(&folder.0), "");
}

#[test]
fn a_newer_server_is_used_and_an_older_one_drains_but_keeps_its_panes() {
    let folder = Folder::new("versions");
    let (old, old_stopped) = serve(&folder.0, SERVER_VERSION - 1);
    let (new, new_stopped) = serve(&folder.0, SERVER_VERSION + 1);
    let (pane, events) = Pane::spawn(&old, spawn_request("kept"), &[]).unwrap();
    let servers = Servers::connect(&folder.0, None);
    // The app doesn't start one when a newer server answers, and it takes that one for new panes.
    assert_eq!(servers.current(), Some(new.clone()));
    assert!(servers.is_older(&old) && !servers.is_older(&new));
    // The old server's pane is still there to claim, on the old server.
    let (at, info) = servers.claim("kept").expect("the older server's pane");
    assert_eq!((at, info.exited), (old.clone(), false));
    assert!(alive(info.shell_pid.unwrap()));
    // Drained: it starts nothing new, and stays up while its pane runs.
    let refused = Pane::spawn(&old, spawn_request("late"), &[]).err().map(|e| e.to_string());
    assert!(refused.is_some_and(|why| why.contains("draining")), "a drained server took a pane");
    assert_eq!(*old_stopped.lock().unwrap(), None);
    // The newer one is never drained.
    assert!(Pane::spawn(&new, spawn_request("fresh"), &[]).is_ok());
    assert_eq!(*new_stopped.lock().unwrap(), None);
    pane.kill().unwrap();
    // Its last pane ended and no app reads it any more: it exits.
    drop((pane, events));
    wait_until("the drained server to exit", || old_stopped.lock().unwrap().is_some());
    assert_eq!(*old_stopped.lock().unwrap(), Some(Stopped::Drained));
}

#[test]
fn a_socket_left_by_a_dead_server_is_skipped_and_only_ours_is_reported() {
    let folder = Folder::new("dead");
    // A listener that goes away leaves its socket file: what a killed server leaves.
    for version in [SERVER_VERSION - 1, SERVER_VERSION] {
        drop(UnixListener::bind(socket(&folder.0, version)).unwrap());
    }
    let servers = Servers::connect(&folder.0, None);
    assert_eq!(servers.current(), None);
    let log = app_log(&folder.0);
    assert!(
        log.contains(&format!("v{SERVER_VERSION} ended without stopping")),
        "{log}"
    );
    // An older server's stale socket says nothing: that app's own update replaced it.
    assert_eq!(log.matches("ended without stopping").count(), 1, "{log}");
}

#[test]
fn a_live_server_is_found_beside_a_dead_ones_socket() {
    let folder = Folder::new("beside");
    drop(UnixListener::bind(socket(&folder.0, SERVER_VERSION + 1)).unwrap());
    let (live, _) = serve(&folder.0, SERVER_VERSION);
    let servers = Servers::connect(&folder.0, None);
    assert_eq!(servers.current(), Some(live));
}

#[test]
fn a_server_that_stopped_answering_is_replaced_once_and_nothing_replaces_a_live_one() {
    let folder = Folder::new("replace");
    let (socket, stopped) = serve(&folder.0, SERVER_VERSION);
    let servers = Servers::connect(&folder.0, None);
    // Answering, so another pane that failed to reach it first doesn't make a second server.
    assert_eq!(servers.replace(&socket), Some(socket.clone()));
    assert_eq!(app_log(&folder.0), "");
    // Another server's path that was reported gone isn't this one's: the current one stays.
    assert_eq!(servers.replace(Path::new("/elsewhere/v1.sock")), Some(socket.clone()));
    let mut control = Control::connect(&socket).unwrap();
    control.drain().unwrap();
    drop(control);
    wait_until("the server to exit", || stopped.lock().unwrap().is_some());
    // This app starts no servers, so the pane runs in the app: `None`, and the log says what happened.
    assert_eq!(servers.replace(&socket), None);
    assert!(app_log(&folder.0).contains("stopped answering"));
    assert_eq!(servers.current(), None);
}

#[test]
fn a_pane_keeps_its_shell_and_scrollback_while_the_app_is_away_and_is_claimed_once() {
    let folder = Folder::new("away");
    let (socket, _) = serve(&folder.0, SERVER_VERSION);
    let (pane, mut events) = Pane::spawn(&socket, spawn_request("tab-1"), &[]).unwrap();
    let shell = pane.shell_pid().unwrap();
    pane.write(b"echo before-$((20+22))\n").unwrap();
    read_until(&pane, &mut events, "before-42");
    // The app quits: its connection ends, the shell does not.
    pane.detach();
    // The shell's next prompt can still be on its way when the connection ends.
    loop {
        match events.next(&pane) {
            Event::Output(_) => continue,
            other => break assert_eq!(other, Event::Closed),
        }
    }
    assert!(alive(shell));
    pane.write(b"x").expect_err("a detached pane takes no more input");
    drop((pane, events));

    // The next app finds it by its saved history id, once.
    let servers = Servers::connect(&folder.0, None);
    assert_eq!(servers.orphans("/project", &[]), ["tab-1"]);
    assert!(servers.claim_running("other").is_none());
    let (at, info) = servers.claim_running("tab-1").expect("the running pane");
    assert!(servers.claim("tab-1").is_none() && servers.claim_running("tab-1").is_none());
    assert_eq!((at.as_path(), info.shell_pid), (socket.as_path(), Some(shell)));
    let (pane, mut events, attached) = Pane::attach(&at, &info).unwrap();
    // Everything it printed while nobody listened is in the replay, and typing still reaches the same shell.
    let replay = read_until(&pane, &mut events, "before-42");
    assert!(attached.reset && replay.contains("before-42"), "{replay:?}");
    pane.write(b"echo after-$((2+2))\n").unwrap();
    read_until(&pane, &mut events, "after-4");
    assert_eq!(pane.shell_pid(), Some(shell));
}

#[test]
fn a_pane_that_ended_while_the_app_was_away_is_claimed_but_not_as_running() {
    let folder = Folder::new("ended");
    let (socket, _) = serve(&folder.0, SERVER_VERSION);
    let (pane, mut events) = Pane::spawn(&socket, spawn_request("tab-2"), &[]).unwrap();
    pane.write(b"echo last-words; exit\n").unwrap();
    read_until(&pane, &mut events, "last-words");
    pane.detach();
    drop((pane, events));
    wait_until("the shell to end", || {
        Control::connect(&socket)
            .and_then(|mut control| control.list())
            .is_ok_and(|panes| panes.iter().all(|pane| pane.exited))
    });
    let servers = Servers::connect(&folder.0, None);
    assert!(servers.claim_running("tab-2").is_none());
    let (_, info) = servers.claim("tab-2").expect("an ended pane is still claimed");
    assert!(info.exited);
    // Its output is still readable, above the new shell the app starts.
    assert!(String::from_utf8_lossy(&read(&socket, info.pane).unwrap()).contains("last-words"));
}

#[test]
fn only_an_older_server_can_be_stopped_and_it_takes_its_panes_with_it() {
    let folder = Folder::new("stop");
    let (old, old_stopped) = serve(&folder.0, SERVER_VERSION - 1);
    let (current, _) = serve(&folder.0, SERVER_VERSION);
    let (pane, events) = Pane::spawn(&old, spawn_request("old-pane"), &[]).unwrap();
    let shell = pane.shell_pid().unwrap();
    let servers = Servers::connect(&folder.0, None);
    assert_eq!(
        servers.stop(&current).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(Control::connect(&current).is_ok());
    servers.stop(&old).unwrap();
    assert!(servers.claim("old-pane").is_none(), "its pane is gone with it");
    wait_until("the shell to end", || !alive(shell));
    drop((pane, events));
    wait_until("the older server to exit", || old_stopped.lock().unwrap().is_some());
}

#[test]
fn copies_of_the_server_are_deleted_unless_a_running_server_is_in_them() {
    let folder = Folder::new("copies");
    let dir = &folder.0;
    let make = |path: &str| {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "x").unwrap();
        file
    };
    let drained = make("replaced/a/Wings Terminals.app/Contents/MacOS/server");
    let unused = make("replaced/b/Wings Terminals.app/Contents/MacOS/server");
    let bare = make("bin/old-server");

    // A running server's file couldn't be told: nothing is deleted.
    remove_old_copies(dir, None);
    assert!(drained.exists() && unused.exists() && bare.exists());

    remove_old_copies(dir, Some(&[drained.canonicalize().unwrap()]));
    assert!(drained.exists(), "a drained server still runs from it");
    assert!(!unused.exists() && !bare.exists());
    assert!(!dir.join("bin").exists(), "an emptied folder goes");

    remove_old_copies(dir, Some(&[]));
    assert!(!dir.join("replaced").exists());
}

#[test]
fn jobs_of_a_plugin_are_taken_once_and_leave_the_others() {
    let folder = Folder::new("jobs");
    let servers = Servers::connect(&folder.0, None);
    let job = |number: u32, plugin: &str| {
        (
            PathBuf::from("/s"),
            wings_ptyd::proto::JobInfo {
                job: number,
                owner: serde_json::json!({ "plugin": plugin, "tab": format!("t{number}") }),
                pid: None,
                since: 0,
                last: 0,
                attached: false,
                stopping: false,
                ended: false,
            },
        )
    };
    *servers.jobs.lock().unwrap() = vec![job(1, "chat"), job(2, "other"), job(3, "chat")];
    let taken: Vec<u32> = servers.claim_jobs("chat").iter().map(|(_, job)| job.job).collect();
    assert_eq!(taken, [1, 3]);
    assert!(servers.claim_jobs("chat").is_empty());
    assert_eq!(servers.claim_jobs("other").len(), 1);
}
