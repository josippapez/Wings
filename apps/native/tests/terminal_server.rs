//! The terminal server as the app runs it: the real binary, started as a plain child in a temp folder.

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
        for entry in std::fs::read_dir(&self.0).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy().ends_with(".sock")
                && let Ok(mut control) = Control::connect(&entry.path())
            {
                for pane in control.list().unwrap_or_default() {
                    let _ = control.kill(pane.pane);
                }
                let _ = control.drain();
            }
        }
        let _ = std::fs::remove_dir_all(&self.0);
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
    assert_eq!(again.current(), Some(socket.as_path()));
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
    assert_eq!(next.current(), Some(socket.as_path()));
    assert!(next.claim("h-killed").is_none(), "its panes went with it");
}

#[test]
fn with_no_server_to_start_new_panes_run_in_the_app() {
    let folder = Folder::new("none");
    let servers = Servers::connect(&folder.0, None);
    assert_eq!(servers.current(), None);
    let broken = Start::Child(folder.0.join("missing-binary"));
    assert_eq!(Servers::connect(&folder.0, Some(&broken)).current(), None);
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

#[test]
fn the_server_is_copied_out_of_the_bundle_once_and_its_agent_is_a_valid_plist() {
    let folder = Folder::new("install");
    std::fs::create_dir_all(&folder.0).unwrap();
    let program = terminal_server::install(Path::new(BIN), &folder.0).unwrap();
    assert_eq!(
        program,
        folder
            .0
            .join(format!("bin/wings-terminal-server-v{SERVER_VERSION}"))
    );
    assert_eq!(
        std::fs::metadata(&program).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let first = std::fs::metadata(&program).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    terminal_server::install(Path::new(BIN), &folder.0).unwrap();
    assert_eq!(
        std::fs::metadata(&program).unwrap().modified().unwrap(),
        first,
        "a running server keeps its file"
    );

    let args = [
        program.to_string_lossy().into_owned(),
        "serve".into(),
        "--socket".into(),
        "/a & <b>.sock".into(),
    ];
    let plist = folder.0.join("agent.plist");
    let label = terminal_server::label(SERVER_VERSION);
    std::fs::write(
        &plist,
        terminal_server::plist_xml(&label, &args, &folder.0.join("v.log")),
    )
    .unwrap();
    let lint = Command::new("/usr/bin/plutil")
        .arg("-lint")
        .arg(&plist)
        .output()
        .unwrap();
    assert!(
        lint.status.success(),
        "{}",
        String::from_utf8_lossy(&lint.stdout)
    );
    let json = Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&plist)
        .output()
        .unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(agent["Label"], label.as_str());
    assert_eq!(agent["ProgramArguments"][3], "/a & <b>.sock");
    assert_eq!(agent["KeepAlive"]["SuccessfulExit"], false);
    assert_eq!(agent["ProcessType"], "Interactive");
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
