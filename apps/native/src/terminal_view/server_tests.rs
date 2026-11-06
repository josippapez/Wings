//! A pane in the terminal server through the worker: reattached after the app lets go, and closed when the server
//! goes away.

use std::{
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

use wings_ptyd::{
    client::{Control, Pane},
    host::{Config, test_host},
    proto::Spawn,
};

use super::{
    tests::{visible_text, wait_snapshot},
    worker::{self, Command, start_remote},
};

fn spawn_request() -> Spawn {
    Spawn {
        space_id: "s".into(),
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 24,
        history_id: Some("h1".into()),
        env: wings_terminal::shell::pane_env(None)
            .into_iter()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .collect(),
    }
}

#[test]
fn a_server_pane_reattaches_with_what_it_printed_meanwhile_and_types_again() {
    let (path, _host) = test_host(Config::default());
    let (pane, events) = Pane::spawn(&path, spawn_request(), b"").unwrap();
    let shell = pane.shell_pid();
    let (runtime, latest, _updates) = start_remote(
        pane,
        events,
        (80, 24),
        Vec::new(),
        worker::Options::default(),
    )
    .unwrap();
    let send = |runtime: &worker::Runtime, text: &str| {
        runtime
            .commands
            .send_blocking(Command::Input(text.as_bytes().to_vec()))
            .unwrap()
    };
    send(&runtime, "echo o''ne\r");
    wait_snapshot(&latest, |frame| visible_text(frame).contains("one"));
    assert_eq!(runtime.process.process_id(), shell);
    // The app quits while the shell is busy: it prints with nobody attached.
    send(&runtime, "sleep 0.5; echo t''wo\r");
    wait_snapshot(&latest, |frame| visible_text(frame).contains("t''wo"));
    runtime.detach();
    drop(runtime);
    thread::sleep(Duration::from_millis(900));

    let info = Control::connect(&path).unwrap().list().unwrap().remove(0);
    assert!(!info.exited, "letting go of a pane leaves it running");
    let (pane, events, attached) = Pane::attach(&path, &info).unwrap();
    let (runtime, latest, _updates) = start_remote(
        pane,
        events,
        (attached.cols, attached.rows),
        Vec::new(),
        worker::Options::default(),
    )
    .unwrap();
    let frame = wait_snapshot(&latest, |frame| visible_text(frame).contains("two"));
    assert!(
        visible_text(&frame).contains("one"),
        "the replay starts before the app let go"
    );
    assert_eq!(runtime.process.process_id(), shell, "the same shell");
    send(&runtime, "echo th''ree\r");
    wait_snapshot(&latest, |frame| visible_text(frame).contains("three"));
    // Closing the pane ends its shell in the server.
    drop(runtime);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !Control::connect(&path).unwrap().list().unwrap().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the closed pane still runs in the server"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// Passes one connection through to `server`, until `cut` ends it as a server that died would.
struct Proxy {
    path: PathBuf,
    streams: Arc<std::sync::Mutex<Vec<UnixStream>>>,
}

impl Proxy {
    fn new(server: &Path) -> Proxy {
        let path = server.with_file_name("proxy.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let streams = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (server, kept) = (server.to_owned(), streams.clone());
        thread::spawn(move || {
            let (client, _) = listener.accept().unwrap();
            let upstream = UnixStream::connect(&server).unwrap();
            kept.lock()
                .unwrap()
                .extend([client.try_clone().unwrap(), upstream.try_clone().unwrap()]);
            let pipe = |mut from: UnixStream, mut to: UnixStream| {
                thread::spawn(move || {
                    let mut buf = [0; 64 << 10];
                    while let Ok(n) = from.read(&mut buf) {
                        if n == 0 || to.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                    let _ = to.shutdown(Shutdown::Both);
                });
            };
            pipe(client.try_clone().unwrap(), upstream.try_clone().unwrap());
            pipe(upstream, client);
        });
        Proxy { path, streams }
    }

    fn cut(&self) {
        for stream in self.streams.lock().unwrap().iter() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

#[test]
fn a_pane_whose_server_stopped_says_so_and_closes() {
    let (path, _host) = test_host(Config::default());
    let proxy = Proxy::new(&path);
    let (pane, events) = Pane::spawn(&proxy.path, spawn_request(), b"").unwrap();
    let (runtime, latest, _updates) = start_remote(
        pane,
        events,
        (80, 24),
        Vec::new(),
        worker::Options::default(),
    )
    .unwrap();
    runtime
        .commands
        .send_blocking(Command::Input(b"echo re''ady\r".to_vec()))
        .unwrap();
    wait_snapshot(&latest, |frame| visible_text(frame).contains("ready"));
    proxy.cut();
    let frame = wait_snapshot(&latest, |frame| frame.closed);
    assert_eq!(
        frame.status.as_deref(),
        Some("The terminal server stopped.")
    );
    assert!(
        visible_text(&frame).contains("ready"),
        "the screen stays as it was"
    );
    let mut control = Control::connect(&path).unwrap();
    for pane in control.list().unwrap() {
        control.kill(pane.pane).unwrap();
    }
}

/// What reattaching costs the app: drawing a full ring (1 MiB) of Claude Code's fullscreen output on a fresh grid.
/// Run with `cargo test --release --lib -- --ignored --nocapture bench_replay`.
#[test]
#[ignore]
fn bench_replay_of_a_full_ring() {
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../desktop/bench/claude-fullscreen.rec"
    ))
    .unwrap();
    let (cols, rows) = (
        u16::from_le_bytes([data[0], data[1]]),
        u16::from_le_bytes([data[2], data[3]]),
    );
    let mut bytes = Vec::new();
    let mut at = 4;
    while at + 8 <= data.len() {
        let len = u32::from_le_bytes(data[at + 4..at + 8].try_into().unwrap()) as usize;
        bytes.extend_from_slice(&data[at + 8..at + 8 + len]);
        at += 8 + len;
    }
    let mut ring = Vec::new();
    while ring.len() < wings_ptyd::output::REPLAY_BYTES {
        ring.extend_from_slice(&bytes);
    }
    ring.truncate(wings_ptyd::output::REPLAY_BYTES);
    let mut times = Vec::new();
    for _ in 0..10 {
        let start = std::time::Instant::now();
        let mut pane = wings_terminal::grid::GridPane::new(cols as usize, rows as usize);
        pane.feed(&ring);
        let _ = pane.take_events();
        times.push(start.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "replay of {} bytes at {cols}x{rows}: median {:.1} ms, max {:.1} ms",
        ring.len(),
        times[5],
        times[9]
    );
}
