use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{Arc, Mutex},
    thread,
};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{OnExit, PaneMeta, PaneMirror, PtyBackend, SpawnRequest};

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

/// Variables the dev host (a Claude Code or Herdr session) would otherwise leak into every pane.
/// `CLAUDE_CODE_CHILD_SESSION`, for one, hides a nested `claude` from `--resume` and the agents list.
fn is_host_session_var(key: &str) -> bool {
    key == "CLAUDECODE"
        || key.starts_with("CLAUDE_CODE_")
        || matches!(
            key,
            "CLAUDE_PID" | "CLAUDE_PROJECT_DIR" | "CLAUDE_ENV_FILE" | "CLAUDE_JOB_DIR" | "CLAUDE_EFFORT"
        )
        || key.starts_with("HERDR_")
        || key.starts_with("TERM_PROGRAM")
}

impl Pane {
    pub fn spawn(
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

        // Default prog = the user's login shell, so their PATH and profile load as in any terminal.
        let mut cmd = CommandBuilder::new_default_prog();
        cmd.cwd(req.cwd);
        for (key, _) in std::env::vars_os() {
            if key.to_str().is_some_and(is_host_session_var) {
                cmd.env_remove(key);
            }
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "Wings");
        cmd.env("WINGS_ENV", "1");
        cmd.env("WINGS_PANE_ID", &req.id);

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let pane = Arc::new(Pane {
            meta: Arc::new(PaneMeta {
                id: req.id,
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
            }
            kept.channel = Some(output);
        }

        let mut reader = pane.master.lock().unwrap().try_clone_reader()?;
        let title = pane.title.clone();
        let shared = pane.output.clone();
        let pane_id = pane.meta.id.clone();
        thread::Builder::new()
            .name(format!("pty-{pane_id}"))
            .spawn(move || {
                let mut parser = vte::Parser::new();
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => shared.lock().unwrap().push(&mut parser, &title, &buf[..n]),
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
        self.output.lock().unwrap().ring.contents()
    }
}

/// Panes that run in this process, each with its own reader thread.
#[derive(Default)]
pub struct LocalBackend {
    panes: Arc<Mutex<HashMap<String, Arc<Pane>>>>,
}

impl LocalBackend {
    fn pane(&self, id: &str) -> anyhow::Result<Arc<Pane>> {
        self.panes.lock().unwrap().get(id).cloned().ok_or_else(|| anyhow::anyhow!("unknown pane {id}"))
    }
}

impl PtyBackend for LocalBackend {
    fn spawn(&self, req: SpawnRequest<'_>, output: Channel<InvokeResponseBody>, on_exit: OnExit) -> anyhow::Result<Arc<PaneMeta>> {
        let panes = self.panes.clone();
        let pane = Pane::spawn(req, output, move |id| {
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
            visit(PaneMirror { meta: &pane.meta, foreground_pid: pane.foreground_pid(), title: &pane.title.lock().unwrap() });
        }
    }

    fn foreground_pid(&self, id: &str) -> Option<u32> {
        self.pane(id).ok()?.foreground_pid()
    }

    fn recent_output(&self, id: &str) -> Option<Vec<u8>> {
        Some(self.pane(id).ok()?.recent_output())
    }
}

/// Output kept per pane for a UI that loads again (a webview reload). xterm keeps 10,000 lines of scrollback, about
/// 1 MB of plain shell output; output heavy in escape codes, like Claude Code's, fills it faster, but the repaint
/// after attaching redraws whatever is on screen. It grows only as output arrives, so a quiet pane holds little.
const REPLAY_BYTES: usize = 1 << 20;

/// The last `cap` bytes written, oldest first from `start` once full.
struct Ring {
    buf: Vec<u8>,
    start: usize,
    cap: usize,
    /// Older bytes were dropped, so the oldest one kept can be in the middle of an escape sequence.
    wrapped: bool,
}

impl Ring {
    fn new(cap: usize) -> Ring {
        Ring { buf: Vec::new(), start: 0, cap, wrapped: false }
    }

    fn push(&mut self, mut data: &[u8]) {
        if data.len() >= self.cap {
            self.wrapped |= !self.buf.is_empty() || data.len() > self.cap;
            self.buf.clear();
            self.buf.extend_from_slice(&data[data.len() - self.cap..]);
            self.start = 0;
            return;
        }
        if self.buf.len() < self.cap {
            let take = data.len().min(self.cap - self.buf.len());
            // Doubling as usual, but never past `cap`, which a plain `extend` could nearly double.
            let want = (self.buf.len() + take).max(self.buf.len() * 2).min(self.cap);
            self.buf.reserve_exact(want - self.buf.len());
            self.buf.extend_from_slice(&data[..take]);
            data = &data[take..];
        }
        if data.is_empty() {
            return;
        }
        self.wrapped = true;
        let first = data.len().min(self.cap - self.start);
        self.buf[self.start..self.start + first].copy_from_slice(&data[..first]);
        self.buf[..data.len() - first].copy_from_slice(&data[first..]);
        self.start = (self.start + data.len()) % self.cap;
    }

    /// Everything kept, oldest first. Once wrapped, it starts after the first newline, so a sequence cut in half
    /// doesn't garble the first line.
    fn contents(&self) -> Vec<u8> {
        let (newer, older) = self.buf.split_at(self.start);
        let mut out = [older, newer].concat();
        if self.wrapped {
            let line = out.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1);
            out.drain(..line);
        }
        out
    }
}

/// Output saved before a restart, ended so the new shell starts as in a fresh terminal: the modes it left on go off.
pub fn restored(mut history: Vec<u8>) -> Vec<u8> {
    let mut output = Output::default();
    output.push(&mut vte::Parser::new(), &Mutex::new(String::new()), &history);
    history.extend(output.history_end());
    history
}

/// DEC private modes a program set long ago, like Claude Code's bracketed paste and mouse reporting, that the replay
/// no longer contains once the ring has wrapped. Without them a reattached pane would paste and click differently.
const MODES: [u16; 11] = [1, 25, 47, 1000, 1002, 1003, 1004, 1006, 1047, 1049, 2004];

struct Output {
    ring: Ring,
    /// Whether each of `MODES` is on.
    modes: [bool; MODES.len()],
    /// The UI's terminal, if one is attached.
    channel: Option<Channel<InvokeResponseBody>>,
}

impl Default for Output {
    fn default() -> Output {
        // Only the cursor (25) starts on, as in a fresh xterm.
        Output { ring: Ring::new(REPLAY_BYTES), modes: MODES.map(|m| m == 25), channel: None }
    }
}

impl Output {
    fn push(&mut self, parser: &mut vte::Parser, title: &Mutex<String>, chunk: &[u8]) {
        parser.advance(&mut Tap { title, modes: &mut self.modes }, chunk);
        self.ring.push(chunk);
        // debt: one IPC message per read; coalesce per frame if many busy panes show lag.
        if let Some(channel) = &self.channel {
            if channel.send(InvokeResponseBody::Raw(chunk.to_vec())).is_err() {
                self.channel = None;
            }
        }
    }

    /// Under the same lock as `push`, so the new terminal gets every byte once and in order.
    fn attach(&mut self, channel: Channel<InvokeResponseBody>) -> anyhow::Result<()> {
        let replay = self.replay();
        if !replay.is_empty() {
            channel.send(InvokeResponseBody::Raw(replay))?;
        }
        self.channel = Some(channel);
        Ok(())
    }

    /// Ends output from before a restart: the modes it left on go off, so the new shell starts as in a fresh terminal.
    fn history_end(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (mode, on) in MODES.iter().zip(self.modes) {
            if on != (*mode == 25) {
                out.extend_from_slice(format!("\x1b[?{mode}{}", if on { 'l' } else { 'h' }).as_bytes());
            }
        }
        out.extend_from_slice(b"\x1b[0m\r\n");
        out
    }

    /// What a fresh terminal needs to catch up: the modes in force if the ring has dropped where they were set,
    /// then the output kept.
    fn replay(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.ring.wrapped {
            for (mode, on) in MODES.iter().zip(self.modes) {
                if on != (*mode == 25) {
                    out.extend_from_slice(format!("\x1b[?{mode}{}", if on { 'h' } else { 'l' }).as_bytes());
                }
            }
        }
        out.extend(self.ring.contents());
        out
    }
}

/// Watches the byte stream for OSC 0/2 title changes and the modes in `MODES`, without keeping a screen model.
struct Tap<'a> {
    title: &'a Mutex<String>,
    modes: &'a mut [bool; MODES.len()],
}

impl vte::Perform for Tap<'_> {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let [kind, rest @ ..] = params {
            if matches!(*kind, b"0" | b"2") && !rest.is_empty() {
                *self.title.lock().unwrap() = String::from_utf8_lossy(&rest.join(&b';')).into_owned();
            }
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, action: char) {
        if intermediates != b"?" || !matches!(action, 'h' | 'l') {
            return;
        }
        for param in params.iter() {
            if let Some(i) = MODES.iter().position(|m| param.first() == Some(m)) {
                self.modes[i] = action == 'h';
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title_after(bytes: &[u8]) -> String {
        let title = Mutex::new(String::new());
        let mut modes = Output::default().modes;
        vte::Parser::new().advance(&mut Tap { title: &title, modes: &mut modes }, bytes);
        title.into_inner().unwrap()
    }

    #[test]
    fn reads_osc_titles_including_semicolons() {
        assert_eq!(title_after(b"\x1b]0;\xe2\x9c\xb3 Fix login\x07"), "✳ Fix login");
        assert_eq!(title_after(b"\x1b]2;a;b\x1b\\"), "a;b");
        assert_eq!(title_after(b"\x1b]7;file:///tmp\x07"), "");
    }

    #[test]
    fn scrubs_only_host_session_vars() {
        assert!(is_host_session_var("CLAUDE_CODE_CHILD_SESSION"));
        assert!(is_host_session_var("CLAUDECODE"));
        assert!(is_host_session_var("HERDR_PANE_ID"));
        assert!(!is_host_session_var("CLAUDE_CONFIG_DIR"));
        assert!(!is_host_session_var("PATH"));
    }

    fn ring_after(cap: usize, pushes: &[&[u8]]) -> (Vec<u8>, bool) {
        let mut ring = Ring::new(cap);
        for p in pushes {
            ring.push(p);
        }
        // A Vec of bytes never allocates fewer than 8.
        assert!(ring.buf.capacity() <= cap.max(8), "grew past its cap");
        (ring.contents(), ring.wrapped)
    }

    #[test]
    fn ring_keeps_the_newest_bytes_from_a_line_start() {
        assert_eq!(ring_after(8, &[b"ab", b"cd"]), (b"abcd".to_vec(), false));
        assert_eq!(ring_after(8, &[b"abcdefgh"]), (b"abcdefgh".to_vec(), false));
        // Wrapped: "\x1b[3" lost its start, so the replay begins after the next newline.
        assert_eq!(ring_after(8, &[b"x\x1b[31m", b"r\nok"]), (b"ok".to_vec(), true));
        assert_eq!(ring_after(8, &[b"12345", b"6789\nab"]), (b"ab".to_vec(), true));
        assert_eq!(ring_after(4, &[b"1\n345678"]), (b"5678".to_vec(), true));
        assert_eq!(ring_after(4, &[b"1", b"2", b"3", b"4", b"\n", b"6"]), (b"6".to_vec(), true));
    }

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
    fn a_wrapped_replay_restores_modes_set_before_it() {
        let title = Mutex::new(String::new());
        let mut parser = vte::Parser::new();
        let mut output = Output::default();
        output.push(&mut parser, &title, b"\x1b[?2004h\x1b[?1000;1006h\x1b[?25l\x1b[?1000l");
        assert!(output.replay().starts_with(b"\x1b[?2004h"), "not wrapped: the modes are in the replay itself");
        output.push(&mut parser, &title, &vec![b'y'; REPLAY_BYTES]);
        output.push(&mut parser, &title, b"\nprompt");
        assert_eq!(output.replay(), b"\x1b[?25l\x1b[?1006h\x1b[?2004hprompt");
    }

    #[test]
    fn saved_output_ends_with_its_modes_off_and_is_kept_without_reaching_the_terminal() {
        let history = restored(b"\x1b]0;old\x07\x1b[?2004h\x1b[?1049hold output".to_vec());
        assert_eq!(history, b"\x1b]0;old\x07\x1b[?2004h\x1b[?1049hold output\x1b[?1049l\x1b[?2004l\x1b[0m\r\n");
        let (channel, got) = recorder();
        let dir = std::env::temp_dir();
        let pane = Pane::spawn(
            SpawnRequest { id: "t".into(), space_id: "s".into(), cwd: &dir, cols: 80, rows: 24, history_id: Some("h".into()), history },
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
            SpawnRequest { id: "t".into(), space_id: "s".into(), cwd: &dir, cols: 80, rows: 24, history_id: None, history: Vec::new() },
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
        let req = SpawnRequest { id: "p7".into(), space_id: "s".into(), cwd: &dir, cols: 80, rows: 24, history_id: Some("h".into()), history: Vec::new() };
        let meta = backend.spawn(req, output, Box::new(move |id| exited.send(id.to_string()).unwrap())).unwrap();
        assert_eq!((meta.id.as_str(), meta.space_id.as_str(), meta.history_id.as_deref()), ("p7", "s", Some("h")));
        assert!(meta.shell_pid.is_some());
        assert_eq!(backend.list().len(), 1);
        backend.write("p7", b"printf '\\033]0;hel''lo\\007'\r").unwrap();
        let mut title = String::new();
        for _ in 0..200 {
            backend.each_mirror(&mut |p| title = p.title.to_string());
            if title == "hello" {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(title, "hello");
        assert!(backend.recent_output("p7").is_some_and(|out| !out.is_empty()));
        assert!(backend.write("nope", b"x").is_err() && backend.meta("nope").is_none());
        backend.kill("p7");
        assert!(backend.meta("p7").is_none() && backend.list().is_empty(), "forgotten as soon as it is killed");
        assert_eq!(exit_rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap(), "p7");
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    /// `cargo test --release pty_throughput -- --ignored --nocapture`: MB/s through the read loop.
    #[test]
    #[ignore]
    fn pty_throughput() {
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
        let pane = Pane::spawn(
            SpawnRequest { id: "bench".into(), space_id: "s".into(), cwd: &dir, cols: 120, rows: 40, history_id: None, history: Vec::new() },
            channel,
            move |_| tx.send(Instant::now()).unwrap(),
        )
        .unwrap();
        pane.write(b"exec sh -c 'yes | head -c 20000000'\r").unwrap();
        let end = rx.recv().unwrap();
        let total = bytes.load(Ordering::Relaxed) as f64;
        let secs = (end - start.lock().unwrap().unwrap()).as_secs_f64();
        println!("pty_throughput: {:.0} MB in {secs:.2}s = {:.0} MB/s", total / 1e6, total / 1e6 / secs);
    }

    /// `cargo test --release pty_echo_latency -- --ignored --nocapture`: how long one keystroke takes to come back
    /// through the output stream (and the backend), from a `cat` that echoes it (the tty's own echo is off, so it is the process's).
    #[test]
    #[ignore]
    fn pty_echo_latency() {
        let (tx, rx) = std::sync::mpsc::channel::<Instant>();
        let channel = Channel::new(move |body| {
            if matches!(body, InvokeResponseBody::Raw(_)) {
                let _ = tx.send(Instant::now());
            }
            Ok(())
        });
        let dir = std::env::temp_dir();
        let backend = LocalBackend::default();
        let req = SpawnRequest { id: "bench".into(), space_id: "s".into(), cwd: &dir, cols: 120, rows: 40, history_id: None, history: Vec::new() };
        backend.spawn(req, channel, Box::new(|_| {})).unwrap();
        backend.write("bench", b"stty raw -echo; echo RE''ADY; exec cat\r").unwrap();
        let ready_by = Instant::now() + std::time::Duration::from_secs(10);
        // Raw mode turns off the tty's \n to \r\n, so the marker ends in a bare \n.
        while !String::from_utf8_lossy(&backend.recent_output("bench").unwrap()).contains("READY\n") {
            assert!(Instant::now() < ready_by, "shell never got ready");
            thread::sleep(std::time::Duration::from_millis(20));
        }
        while rx.recv_timeout(std::time::Duration::from_millis(200)).is_ok() {}
        let mut micros = Vec::new();
        for _ in 0..2000 {
            let start = Instant::now();
            backend.write("bench", b"k").unwrap();
            let at = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("no echo");
            micros.push(at.duration_since(start).as_secs_f64() * 1e6);
            thread::sleep(std::time::Duration::from_millis(2));
        }
        micros.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let at = |q: f64| micros[((micros.len() - 1) as f64 * q) as usize];
        println!("pty_echo_latency: {} keys, median {:.0} us, p95 {:.0} us, max {:.0} us", micros.len(), at(0.5), at(0.95), micros[micros.len() - 1]);
        backend.kill("bench");
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
            let mut modes = Output::default().modes;
            let t = Instant::now();
            for _ in 0..reads {
                parser.advance(&mut Tap { title: &title, modes: &mut modes }, &chunk);
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
