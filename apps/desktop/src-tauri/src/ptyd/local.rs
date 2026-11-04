use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
    thread,
};

use portable_pty::{native_pty_system, Child, MasterPty, PtySize};
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{
    output::Kept,
    pane_id,
    shell::{pane_command, pane_env},
    OnExit, PaneMeta, PaneMirror, PtyBackend, SpawnRequest,
};

/// A shell running in a pseudo-terminal, bound to one Wings tab.
pub struct Pane {
    pub meta: Arc<PaneMeta>,
    /// Latest OSC 0/2 window title the program set. Claude Code puts its state glyph here.
    pub title: Arc<Mutex<String>>,
    output: Arc<Mutex<Output>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
}

impl Pane {
    pub fn spawn(
        id: String,
        req: SpawnRequest,
        output: Channel<InvokeResponseBody>,
        on_exit: impl FnOnce(&str) + Send + 'static,
    ) -> anyhow::Result<Arc<Pane>> {
        let pair = native_pty_system().openpty(PtySize {
            rows: req.rows,
            cols: req.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let child = pair.slave.spawn_command(pane_command(req.cwd, pane_env(), &id))?;
        drop(pair.slave);

        let pane = Arc::new(Pane {
            meta: Arc::new(PaneMeta {
                id,
                space_id: req.space_id,
                shell_pid: child.process_id(),
                cwd: req.cwd.to_path_buf(),
                history_id: req.history_id,
            }),
            title: Arc::new(Mutex::new(String::new())),
            output: Arc::new(Mutex::new(Output::default())),
            writer: Mutex::new(pair.master.take_writer()?),
            child: Mutex::new(child),
            master: Mutex::new(pair.master),
        });

        {
            let mut kept = pane.output.lock().unwrap();
            if !req.history.is_empty() {
                kept.push(&mut vte::Parser::new(), &pane.title, &req.history);
                // The old program's title isn't the new shell's.
                pane.title.lock().unwrap().clear();
                kept.kept.codex_status = None;
            }
            kept.channel = Some(output);
        }

        let mut reader = pane.master.lock().unwrap().try_clone_reader()?;
        let title = pane.title.clone();
        let shared = pane.output.clone();
        let pane_id = pane.meta.id.clone();
        let status_source = pane.clone();
        thread::Builder::new()
            .name(format!("pty-{pane_id}"))
            .spawn(move || {
                let mut parser = vte::Parser::new();
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut output = shared.lock().unwrap();
                            let previous = output.kept.codex_status;
                            output.push(&mut parser, &title, &buf[..n]);
                            if output.kept.codex_status != previous {
                                if let Some(report) = output.kept.codex_status.as_mut() {
                                    report.pid = status_source.foreground_pid();
                                }
                            }
                        }
                    }
                }
                on_exit(&pane_id);
            })?;

        Ok(pane)
    }

    /// Sends the UI that loaded since (a webview reload) the recent output, then streams to it instead.
    pub fn attach(&self, channel: Channel<InvokeResponseBody>) -> anyhow::Result<()> {
        self.output.lock().unwrap().attach(channel)
    }

    pub fn write(&self, data: &[u8]) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.master.lock().unwrap().resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
    }

    pub fn kill(&self) {
        let _ = self.child.lock().unwrap().kill();
    }

    /// The running command's process (the shell itself at a prompt), from the PTY's foreground process group.
    #[cfg(unix)]
    pub fn foreground_pid(&self) -> Option<u32> {
        self.master.lock().unwrap().process_group_leader().map(|pid| pid as u32)
    }

    #[cfg(not(unix))]
    pub fn foreground_pid(&self) -> Option<u32> {
        None
    }

    /// The raw output kept for this pane (up to `REPLAY_BYTES`), oldest first, for `read_pane`.
    pub fn recent_output(&self) -> Vec<u8> {
        self.output.lock().unwrap().kept.ring.contents()
    }
}

/// Panes that run in this process, each with its own reader thread.
#[derive(Default)]
pub struct LocalBackend {
    panes: Arc<Mutex<HashMap<String, Arc<Pane>>>>,
    /// The last pane's number. Counts from 1 each launch, since no pane outlives this process.
    numbered: AtomicU32,
}

impl LocalBackend {
    fn pane(&self, id: &str) -> anyhow::Result<Arc<Pane>> {
        self.panes.lock().unwrap().get(id).cloned().ok_or_else(|| anyhow::anyhow!("unknown pane {id}"))
    }
}

impl PtyBackend for LocalBackend {
    fn spawn(&self, req: SpawnRequest<'_>, output: Channel<InvokeResponseBody>, on_exit: OnExit) -> anyhow::Result<Arc<PaneMeta>> {
        let panes = self.panes.clone();
        let id = pane_id(self.numbered.fetch_add(1, Ordering::Relaxed) + 1);
        let pane = Pane::spawn(id, req, output, move |id| {
            panes.lock().unwrap().remove(id);
            on_exit(id);
        })?;
        let meta = pane.meta.clone();
        self.panes.lock().unwrap().insert(meta.id.clone(), pane);
        Ok(meta)
    }

    fn attach(&self, id: &str, output: Channel<InvokeResponseBody>) -> anyhow::Result<()> {
        self.pane(id)?.attach(output)
    }

    fn write(&self, id: &str, data: &[u8]) -> anyhow::Result<()> {
        self.pane(id)?.write(data)
    }

    fn resize(&self, id: &str, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.pane(id)?.resize(cols, rows)
    }

    fn kill(&self, id: &str) {
        let pane = self.panes.lock().unwrap().remove(id);
        if let Some(pane) = pane {
            pane.kill();
        }
    }

    fn meta(&self, id: &str) -> Option<Arc<PaneMeta>> {
        self.panes.lock().unwrap().get(id).map(|p| p.meta.clone())
    }

    fn list(&self) -> Vec<Arc<PaneMeta>> {
        self.panes.lock().unwrap().values().map(|p| p.meta.clone()).collect()
    }

    fn each_mirror(&self, visit: &mut dyn FnMut(PaneMirror<'_>)) {
        for pane in self.panes.lock().unwrap().values() {
            // Read output before title, matching the reader thread's lock order.
            let codex_status = pane.output.lock().unwrap().kept.codex_status;
            visit(PaneMirror { meta: &pane.meta, foreground_pid: pane.foreground_pid(), title: &pane.title.lock().unwrap(), codex_status });
        }
    }

    fn foreground_pid(&self, id: &str) -> Option<u32> {
        self.pane(id).ok()?.foreground_pid()
    }

    fn recent_output(&self, id: &str) -> Option<Vec<u8>> {
        Some(self.pane(id).ok()?.recent_output())
    }
}

#[derive(Default)]
struct Output {
    kept: Kept,
    /// The UI's terminal, if one is attached.
    channel: Option<Channel<InvokeResponseBody>>,
}

impl Output {
    fn push(&mut self, parser: &mut vte::Parser, title: &Mutex<String>, chunk: &[u8]) {
        self.kept.push(parser, title, chunk);
        // debt: one IPC message per read; coalesce per frame if many busy panes show lag.
        if let Some(channel) = &self.channel {
            if channel.send(InvokeResponseBody::Raw(chunk.to_vec())).is_err() {
                self.channel = None;
            }
        }
    }

    /// Under the same lock as `push`, so the new terminal gets every byte once and in order.
    fn attach(&mut self, channel: Channel<InvokeResponseBody>) -> anyhow::Result<()> {
        let replay = self.kept.replay();
        if !replay.is_empty() {
            channel.send(InvokeResponseBody::Raw(replay))?;
        }
        self.channel = Some(channel);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ptyd::output::restored;

    type Got = Arc<Mutex<Vec<u8>>>;

    fn recorder() -> (Channel<InvokeResponseBody>, Got) {
        let got = Got::default();
        let sink = got.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                sink.lock().unwrap().extend(bytes);
            }
            Ok(())
        });
        (channel, got)
    }

    #[test]
    fn attach_replays_then_streams_to_the_new_terminal_only() {
        let (old, old_got) = recorder();
        let (new, new_got) = recorder();
        let title = Mutex::new(String::new());
        let mut parser = vte::Parser::new();
        let mut output = Output { channel: Some(old), ..Output::default() };
        output.push(&mut parser, &title, b"$ ls\r\n");
        output.attach(new).unwrap();
        output.push(&mut parser, &title, b"a.txt\r\n");
        assert_eq!(*old_got.lock().unwrap(), b"$ ls\r\n");
        assert_eq!(*new_got.lock().unwrap(), b"$ ls\r\na.txt\r\n");
    }

    #[test]
    fn saved_output_ends_with_its_modes_off_and_is_kept_without_reaching_the_terminal() {
        let history = restored(b"\x1b]0;old\x07\x1b[?2004h\x1b[?1049hold output".to_vec());
        assert_eq!(history, b"\x1b]0;old\x07\x1b[?2004h\x1b[?1049hold output\x1b[?1049l\x1b[?2004l\x1b[0m\r\n");
        let (channel, got) = recorder();
        let dir = std::env::temp_dir();
        let pane = Pane::spawn(
            "t".into(),
            SpawnRequest { space_id: "s".into(), cwd: &dir, cols: 80, rows: 24, history_id: Some("h".into()), history },
            channel,
            |_| {},
        )
        .unwrap();
        // Its queries would be answered into the new shell, so the terminal gets it from the UI before it listens.
        assert!(!got.lock().unwrap().starts_with(b"\x1b]0;old"), "not sent to the terminal");
        assert!(pane.recent_output().starts_with(b"\x1b]0;old"), "saved again on the next quit");
        pane.kill();
    }

    #[test]
    fn a_running_pane_reattaches_with_its_output() {
        let (first, _) = recorder();
        let dir = std::env::temp_dir();
        let pane = Pane::spawn(
            "t".into(),
            SpawnRequest { space_id: "s".into(), cwd: &dir, cols: 80, rows: 24, history_id: None, history: Vec::new() },
            first,
            |_| {},
        )
        .unwrap();
        let wait_for = |got: &Got, text: &str| {
            for _ in 0..200 {
                if String::from_utf8_lossy(&got.lock().unwrap()).contains(text) {
                    return;
                }
                thread::sleep(std::time::Duration::from_millis(50));
            }
            panic!("never saw {text:?}");
        };
        pane.write(b"echo be''fore\r").unwrap();
        let (probe, probe_got) = recorder();
        pane.attach(probe).unwrap();
        wait_for(&probe_got, "before");
        let (second, got) = recorder();
        pane.attach(second).unwrap();
        assert!(String::from_utf8_lossy(&got.lock().unwrap()).contains("before"), "replayed");
        pane.write(b"echo af''ter\r").unwrap();
        wait_for(&got, "after");
        pane.kill();
    }

    #[test]
    fn the_backend_finds_a_pane_by_id_until_it_is_killed() {
        let backend = LocalBackend::default();
        let (output, _) = recorder();
        let (exited, exit_rx) = std::sync::mpsc::channel();
        let dir = std::env::temp_dir();
        let req = SpawnRequest { space_id: "s".into(), cwd: &dir, cols: 80, rows: 24, history_id: Some("h".into()), history: Vec::new() };
        let meta = backend.spawn(req, output, Box::new(move |id| exited.send(id.to_string()).unwrap())).unwrap();
        assert_eq!((meta.id.as_str(), meta.space_id.as_str(), meta.history_id.as_deref()), ("p1", "s", Some("h")));
        assert!(meta.shell_pid.is_some());
        assert_eq!(backend.list().len(), 1);
        backend.write("p1", b"printf '\\033]0;hel''lo\\007'; echo id=$WINGS_PANE_ID\r").unwrap();
        let mut title = String::new();
        // Its id is in its environment.
        let echoed = || backend.recent_output("p1").is_some_and(|out| String::from_utf8_lossy(&out).contains("id=p1"));
        for _ in 0..200 {
            backend.each_mirror(&mut |p| title = p.title.to_string());
            if title == "hello" && echoed() {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(title == "hello" && echoed(), "title {title:?}");
        assert!(backend.write("nope", b"x").is_err() && backend.meta("nope").is_none());
        backend.kill("p1");
        assert!(backend.meta("p1").is_none() && backend.list().is_empty(), "forgotten as soon as it is killed");
        assert_eq!(exit_rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap(), "p1");
        let (output, _) = recorder();
        let req = SpawnRequest { space_id: "s".into(), cwd: &dir, cols: 80, rows: 24, history_id: None, history: Vec::new() };
        let next = backend.spawn(req, output, Box::new(|_| {})).unwrap();
        assert_eq!(next.id, "p2", "an id is never given twice");
        backend.kill("p2");
    }
    #[test]
    fn local_mirror_exposes_live_codex_status_and_ignores_saved_status() {
        use crate::program_status::CodexStatus;
        let backend = LocalBackend::default();
        let (output, _) = recorder();
        let dir = std::env::temp_dir();
        let req = SpawnRequest { space_id: "s".into(), cwd: &dir, cols: 80, rows: 24, history_id: None, history: b"\x1b]7501;state=working:app=codex\x07".to_vec() };
        let meta = backend.spawn(req, output, Box::new(|_| {})).unwrap();
        let mut status = None;
        backend.each_mirror(&mut |p| status = p.codex_status);
        assert_eq!(status, None, "a saved transcript is not a live report");
        backend.write(&meta.id, b"printf '\\033]7501;state=block''ed:app=codex:kind=permission\\007'; echo sta''tus-sent\r").unwrap();
        for _ in 0..200 {
            backend.each_mirror(&mut |p| status = p.codex_status);
            if status.is_some_and(|s| s.state == CodexStatus::Permission) { break; }
            thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(status.map(|s| s.state), Some(CodexStatus::Permission));
        backend.kill(&meta.id);
    }

}

#[cfg(test)]
pub(super) mod bench {
    use super::*;
    use crate::ptyd::output::Tap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    /// MB and seconds for 20 MB of `yes` through a backend's output stream, timed from the first MB.
    pub(in crate::ptyd) fn throughput(backend: &dyn PtyBackend) -> (f64, f64) {
        let bytes = Arc::new(AtomicU64::new(0));
        let start = Arc::new(Mutex::new(None::<Instant>));
        let (b, s) = (bytes.clone(), start.clone());
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(v) = body {
                if b.fetch_add(v.len() as u64, Ordering::Relaxed) > 1 << 20 {
                    s.lock().unwrap().get_or_insert_with(Instant::now);
                }
            }
            Ok(())
        });
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = std::env::temp_dir();
        let req = SpawnRequest { space_id: "s".into(), cwd: &dir, cols: 120, rows: 40, history_id: None, history: Vec::new() };
        let id = backend.spawn(req, channel, Box::new(move |_| tx.send(Instant::now()).unwrap())).unwrap().id.clone();
        backend.write(&id, b"exec sh -c 'yes | head -c 20000000'\r").unwrap();
        let end = rx.recv().unwrap();
        let total = bytes.load(Ordering::Relaxed) as f64;
        let secs = (end - start.lock().unwrap().unwrap()).as_secs_f64();
        (total / 1e6, secs)
    }

    /// `cargo test --release pty_throughput -- --ignored --nocapture`: MB/s through the read loop.
    #[test]
    #[ignore]
    fn pty_throughput() {
        let (mb, secs) = throughput(&LocalBackend::default());
        println!("pty_throughput: {mb:.0} MB in {secs:.2}s = {:.0} MB/s", mb / secs);
    }

    /// Microseconds from writing one key to seeing it back in the output stream, sorted, for `keys` keys, from a
    /// `cat` that echoes it (the tty's own echo is off, so it is the process's).
    pub(in crate::ptyd) fn echo_latency(backend: &dyn PtyBackend, keys: usize) -> Vec<f64> {
        let (tx, rx) = std::sync::mpsc::channel::<Instant>();
        let channel = Channel::new(move |body| {
            if matches!(body, InvokeResponseBody::Raw(_)) {
                let _ = tx.send(Instant::now());
            }
            Ok(())
        });
        let dir = std::env::temp_dir();
        let req = SpawnRequest { space_id: "s".into(), cwd: &dir, cols: 120, rows: 40, history_id: None, history: Vec::new() };
        let id = &backend.spawn(req, channel, Box::new(|_| {})).unwrap().id.clone();
        backend.write(id, b"stty raw -echo; echo RE''ADY; exec cat\r").unwrap();
        let ready_by = Instant::now() + std::time::Duration::from_secs(10);
        // Raw mode turns off the tty's \n to \r\n, so the marker ends in a bare \n.
        while !String::from_utf8_lossy(&backend.recent_output(id).unwrap()).contains("READY\n") {
            assert!(Instant::now() < ready_by, "shell never got ready");
            thread::sleep(std::time::Duration::from_millis(20));
        }
        while rx.recv_timeout(std::time::Duration::from_millis(200)).is_ok() {}
        let mut micros = Vec::new();
        for _ in 0..keys {
            let start = Instant::now();
            backend.write(id, b"k").unwrap();
            let at = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("no echo");
            micros.push(at.duration_since(start).as_secs_f64() * 1e6);
            thread::sleep(std::time::Duration::from_millis(2));
        }
        backend.kill(id);
        micros.sort_by(|a, b| a.partial_cmp(b).unwrap());
        micros
    }

    /// The value below which `q` of the sorted `micros` fall.
    pub(in crate::ptyd) fn quantile(micros: &[f64], q: f64) -> f64 {
        micros[((micros.len() - 1) as f64 * q) as usize]
    }

    /// `cargo test --release pty_echo_latency -- --ignored --nocapture`: how long one keystroke takes to come back
    /// through the output stream (and the backend).
    #[test]
    #[ignore]
    fn pty_echo_latency() {
        let micros = echo_latency(&LocalBackend::default(), 2000);
        let at = |q| quantile(&micros, q);
        println!("pty_echo_latency: {} keys, median {:.0} us, p95 {:.0} us, max {:.0} us", micros.len(), at(0.5), at(0.95), micros[micros.len() - 1]);
    }

    /// `cargo test --release read_loop_cost -- --ignored --nocapture`: the work per read without the PTY, with and
    /// without the replay ring, at the read sizes a PTY hands out.
    #[test]
    #[ignore]
    fn read_loop_cost() {
        let sent = Arc::new(AtomicU64::new(0));
        let s = sent.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(v) = body {
                s.fetch_add(v.len() as u64, Ordering::Relaxed);
            }
            Ok(())
        });
        let title = Mutex::new(String::new());
        let line = b"\x1b[32mok\x1b[0m src/pty.rs: some ordinary build output here\r\n";
        for size in [16usize, 1024, 64 * 1024] {
            let chunk: Vec<u8> = line.iter().copied().cycle().take(size).collect();
            let reads = (256 << 20) / size;
            let mut parser = vte::Parser::new();
            let mut modes = Kept::default().modes;
            let t = Instant::now();
            for _ in 0..reads {
                parser.advance(&mut Tap { title: &title, modes: &mut modes, codex_status: &mut None, codex_generation: &mut 0 }, &chunk);
                channel.send(InvokeResponseBody::Raw(chunk.to_vec())).unwrap();
            }
            let without = t.elapsed().as_secs_f64();
            let mut output = Output { channel: Some(channel.clone()), ..Output::default() };
            let shared = Mutex::new(());
            let t = Instant::now();
            for _ in 0..reads {
                let _lock = shared.lock().unwrap();
                output.push(&mut parser, &title, &chunk);
            }
            let with = t.elapsed().as_secs_f64();
            println!(
                "read_loop_cost {size:>6} B reads: without ring {:>5.0} MB/s, with ring {:>5.0} MB/s ({:+.1} ns per read)",
                256.0 * 1.048576 / without,
                256.0 * 1.048576 / with,
                (with - without) * 1e9 / reads as f64,
            );
        }
        assert!(sent.load(Ordering::Relaxed) > 0);
    }

}
