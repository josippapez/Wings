//! Panes that run in the terminal host (`host.rs`), reached over its socket. A mirror of each pane, kept current by
//! the host's events, answers what the app reads often (agent detection reads every pane each 500 ms) without asking
//! the host. Keystrokes are one write each; spawning, attaching, resizing and reading kept output wait for a reply.

use std::{
    collections::HashMap,
    io::{BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail};
use serde_json::Value;
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{
    proto::{self, Attached, Call, Frame, HostMsg, PaneInfo, Request, Spawn, Spawned, CHUNK, MIN_PROTO, PROTO},
    pane_id, pane_number,
    shell::pane_env,
    OnExit, PaneMeta, PaneMirror, PtyBackend, SpawnRequest,
};

/// How long a request waits for the host's reply.
const TIMEOUT: Duration = Duration::from_secs(10);

pub struct RemoteBackend {
    inner: Arc<Inner>,
}

struct Inner {
    writer: Mutex<UnixStream>,
    next_req: AtomicU32,
    pending: Mutex<HashMap<u32, Pending>>,
    mirror: Mutex<Mirror>,
    alive: AtomicBool,
    /// This backend closed the connection itself: the panes it leaves keep running, so they don't exit for the app.
    closing: AtomicBool,
    #[cfg(test)]
    lags: AtomicU32,
}

/// What to do with a reply, on the thread that reads the socket, in order with the frames around it.
type Done = Box<dyn FnOnce(&Inner, Result<Value, String>, Vec<u8>) + Send>;

struct Pending {
    blob: Vec<u8>,
    done: Done,
}

/// The panes by the host's number for each, which their id carries.
#[derive(Default)]
struct Mirror {
    panes: HashMap<u32, Pane>,
}

struct Pane {
    number: u32,
    meta: Arc<PaneMeta>,
    fg: Option<u32>,
    title: String,
    /// Where the output received so far ends, to attach again from there after a lag.
    seq: u64,
    sink: Option<Channel<InvokeResponseBody>>,
    /// Attaching a new terminal: output before the host's reply is in the replay that follows it.
    syncing: bool,
    on_exit: Option<OnExit>,
    /// Gone for the app; the host forgets it once its output ends.
    killed: bool,
}

impl Mirror {
    fn by_number(&mut self, number: u32) -> Option<&mut Pane> {
        self.panes.get_mut(&number)
    }

    fn live(&self, id: &str) -> Option<&Pane> {
        self.panes.get(&pane_number(id)?).filter(|p| !p.killed)
    }

    fn live_mut(&mut self, id: &str) -> Option<&mut Pane> {
        self.panes.get_mut(&pane_number(id)?).filter(|p| !p.killed)
    }

    fn insert(&mut self, pane: Pane) {
        self.panes.insert(pane.number, pane);
    }

    fn remove(&mut self, number: u32) -> Option<Pane> {
        self.panes.remove(&number)
    }
}

impl RemoteBackend {
    /// Connects to the host at `path`, starting one from this executable if none answers.
    // debt: a plain child of this app, so it can't outlive Wings for long; the launcher replaces this.
    pub fn connect_or_start(path: &Path) -> anyhow::Result<RemoteBackend> {
        let stream = match UnixStream::connect(path) {
            Ok(stream) => stream,
            Err(_) => start_host(path)?,
        };
        RemoteBackend::over(stream)
    }

    #[cfg(test)]
    pub fn connect(path: &Path) -> anyhow::Result<RemoteBackend> {
        RemoteBackend::over(UnixStream::connect(path)?)
    }

    fn over(stream: UnixStream) -> anyhow::Result<RemoteBackend> {
        stream.set_read_timeout(Some(TIMEOUT))?;
        (&stream).write_all(&proto::ctrl(&Call { id: 0, req: Request::Hello { proto: PROTO, min_proto: MIN_PROTO } })?)?;
        let mut reader = BufReader::with_capacity(64 << 10, stream.try_clone()?);
        let Some(Frame::Ctrl(json)) = proto::read(&mut reader)? else { bail!("the terminal host didn't answer hello") };
        let HostMsg::Welcome { proto, error, .. } = serde_json::from_slice(&json)? else { bail!("the terminal host didn't answer hello") };
        if let Some(error) = error {
            bail!("the terminal host refused: {error}");
        }
        if !(MIN_PROTO..=PROTO).contains(&proto) {
            bail!("the terminal host speaks protocol {proto}");
        }
        stream.set_read_timeout(None)?;
        let inner = Arc::new(Inner {
            writer: Mutex::new(stream),
            next_req: AtomicU32::new(1),
            pending: Mutex::default(),
            mirror: Mutex::default(),
            alive: AtomicBool::new(true),
            closing: AtomicBool::new(false),
            #[cfg(test)]
            lags: AtomicU32::new(0),
        });
        let reading = inner.clone();
        thread::Builder::new().name("ptyd-remote".into()).spawn(move || reading.read_loop(reader))?;
        // Panes still running from before (Wings restarted while the host kept them) join the mirror in the reply
        // handler, so no event about them can come first.
        let (tx, rx) = mpsc::channel();
        inner.send(
            Request::List,
            Vec::new(),
            Box::new(move |inner, result, _| {
                let listed = result.and_then(|v| serde_json::from_value::<Vec<PaneInfo>>(v).map_err(|e| e.to_string()));
                if let Ok(panes) = &listed {
                    inner.adopt(panes);
                }
                let _ = tx.send(listed.map(|_| ()));
            }),
        )?;
        rx.recv_timeout(TIMEOUT).map_err(|_| anyhow!("the terminal host didn't list its panes"))?.map_err(anyhow::Error::msg)?;
        Ok(RemoteBackend { inner })
    }

    fn number(&self, id: &str) -> anyhow::Result<u32> {
        self.inner.mirror.lock().unwrap().live(id).map(|p| p.number).ok_or_else(|| anyhow!("unknown pane {id}"))
    }

    /// Sends a request and waits for its reply and the bytes that came with it.
    fn call(&self, req: Request) -> anyhow::Result<(Value, Vec<u8>)> {
        let (tx, rx) = mpsc::channel();
        self.inner.send(req, Vec::new(), Box::new(move |_, result, blob| drop(tx.send(result.map(|v| (v, blob))))))?;
        rx.recv_timeout(TIMEOUT).map_err(|_| anyhow!("the terminal host didn't answer"))?.map_err(anyhow::Error::msg)
    }
}

impl Drop for RemoteBackend {
    fn drop(&mut self) {
        self.inner.closing.store(true, Ordering::Relaxed);
        let _ = self.inner.writer.lock().unwrap().shutdown(std::net::Shutdown::Both);
    }
}

/// Starts `wings ptyd` from this executable and waits for it to answer at `path`.
fn start_host(path: &Path) -> anyhow::Result<UnixStream> {
    let mut child = Command::new(std::env::current_exe()?)
        .arg("ptyd")
        .arg("--socket")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()?;
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(stream) = UnixStream::connect(path) {
            // Reaped when it exits, since nothing else waits for it.
            thread::spawn(move || child.wait());
            return Ok(stream);
        }
        if let Some(status) = child.try_wait()? {
            bail!("the terminal host exited: {status}");
        }
        if Instant::now() > until {
            bail!("the terminal host didn't start");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

impl Inner {
    /// Writes a request, after the bytes it carries, and registers what to do with the reply.
    fn send(&self, req: Request, blob: Vec<u8>, done: Done) -> anyhow::Result<()> {
        let id = self.next_req.fetch_add(1, Ordering::Relaxed);
        let mut wire = Vec::new();
        for chunk in blob.chunks(CHUNK) {
            wire.extend(proto::blob(id, chunk));
        }
        wire.extend(proto::ctrl(&Call { id, req })?);
        self.pending.lock().unwrap().insert(id, Pending { blob: Vec::new(), done });
        // Checked after registering: a host that went away before then has already failed every request it had.
        let written = if self.alive.load(Ordering::Relaxed) {
            self.writer.lock().unwrap().write_all(&wire).map_err(anyhow::Error::from)
        } else {
            Err(anyhow!("the terminal host is gone"))
        };
        if written.is_err() {
            self.pending.lock().unwrap().remove(&id);
        }
        written
    }

    /// A request whose reply nobody waits for.
    fn tell(&self, req: Request) {
        let _ = self.send(req, Vec::new(), Box::new(|_, _, _| {}));
    }

    fn read_loop(&self, mut reader: BufReader<UnixStream>) {
        while let Ok(Some(frame)) = proto::read(&mut reader) {
            match frame {
                Frame::Out { pane, seq_end, data } => self.output(pane, seq_end, data),
                Frame::Blob { req, data } => {
                    if let Some(pending) = self.pending.lock().unwrap().get_mut(&req) {
                        pending.blob.extend(data);
                    }
                }
                Frame::Ctrl(json) => {
                    if let Ok(msg) = serde_json::from_slice::<HostMsg>(&json) {
                        self.event(msg);
                    }
                }
                Frame::In { .. } => {}
            }
        }
        self.gone();
    }

    fn output(&self, number: u32, seq_end: u64, data: Vec<u8>) {
        let sink = {
            let mut mirror = self.mirror.lock().unwrap();
            let Some(pane) = mirror.by_number(number).filter(|p| !p.syncing) else { return };
            pane.seq = seq_end;
            pane.sink.clone()
        };
        // Sent outside the lock, so a keystroke looking up its pane never waits for the webview.
        if let Some(sink) = sink {
            if sink.send(InvokeResponseBody::Raw(data)).is_err() {
                if let Some(pane) = self.mirror.lock().unwrap().by_number(number) {
                    if pane.sink.as_ref().is_some_and(|s| s.id() == sink.id()) {
                        pane.sink = None;
                    }
                }
            }
        }
    }

    fn event(&self, msg: HostMsg) {
        match msg {
            HostMsg::Reply { id, ok, error } => {
                let Some(pending) = self.pending.lock().unwrap().remove(&id) else { return };
                (pending.done)(self, error.map_or(Ok(ok), Err), pending.blob);
            }
            HostMsg::Title { pane, title } => {
                if let Some(pane) = self.mirror.lock().unwrap().by_number(pane) {
                    pane.title = title;
                }
            }
            HostMsg::Fg { pane, pid } => {
                if let Some(pane) = self.mirror.lock().unwrap().by_number(pane) {
                    pane.fg = pid;
                }
            }
            HostMsg::Exited { pane: number } => {
                let Some(pane) = self.mirror.lock().unwrap().remove(number) else { return };
                if !pane.killed {
                    // The host keeps an exited pane for a client that wasn't there to see it end. This one was.
                    self.tell(Request::Kill { pane: number });
                }
                if let Some(on_exit) = pane.on_exit {
                    on_exit(&pane.meta.id);
                }
            }
            HostMsg::Lagged { pane: number } => {
                #[cfg(test)]
                self.lags.fetch_add(1, Ordering::Relaxed);
                let Some(from) = self.mirror.lock().unwrap().by_number(number).map(|p| p.seq) else { return };
                let _ = self.send(
                    Request::Attach { pane: number, from_seq: Some(from) },
                    Vec::new(),
                    Box::new(move |inner, result, _| {
                        let Ok(attached) = result.and_then(|v| serde_json::from_value::<Attached>(v).map_err(|e| e.to_string())) else { return };
                        if !attached.reset {
                            return;
                        }
                        // The ring no longer has what was missed: start the terminal over before the full replay.
                        let sink = inner.mirror.lock().unwrap().by_number(number).and_then(|p| p.sink.clone());
                        if let Some(sink) = sink {
                            let _ = sink.send(InvokeResponseBody::Raw(b"\x1bc".to_vec()));
                        }
                    }),
                );
            }
            HostMsg::Welcome { .. } => {}
        }
    }

    /// Panes the host already runs, from an earlier connection. Exited ones are forgotten; nothing shows them yet.
    fn adopt(&self, panes: &[PaneInfo]) {
        let mut mirror = self.mirror.lock().unwrap();
        for info in panes {
            if info.exited {
                self.tell(Request::Kill { pane: info.pane });
                continue;
            }
            mirror.insert(Pane {
                number: info.pane,
                meta: Arc::new(PaneMeta {
                    id: pane_id(info.pane),
                    space_id: info.space_id.clone(),
                    shell_pid: info.shell_pid,
                    cwd: info.cwd.clone(),
                    history_id: info.history_id.clone(),
                }),
                fg: info.fg,
                title: info.title.clone(),
                seq: info.seq,
                sink: None,
                syncing: false,
                on_exit: None,
                killed: false,
            });
        }
    }

    /// The connection ended: every request fails, and unless this backend closed it, every pane has exited, since
    /// the host took them with it.
    fn gone(&self) {
        self.alive.store(false, Ordering::Relaxed);
        let pending: Vec<Pending> = self.pending.lock().unwrap().drain().map(|(_, p)| p).collect();
        for p in pending {
            (p.done)(self, Err("the terminal host is gone".into()), Vec::new());
        }
        let panes: Vec<Pane> = std::mem::take(&mut *self.mirror.lock().unwrap()).panes.into_values().collect();
        if self.closing.load(Ordering::Relaxed) {
            return;
        }
        for pane in panes {
            if let Some(on_exit) = pane.on_exit {
                on_exit(&pane.meta.id);
            }
        }
    }
}

impl PtyBackend for RemoteBackend {
    fn spawn(&self, req: SpawnRequest<'_>, output: Channel<InvokeResponseBody>, on_exit: OnExit) -> anyhow::Result<Arc<PaneMeta>> {
        // Only what JSON can carry: a variable that isn't UTF-8 doesn't reach a pane in the host.
        let env = pane_env().into_iter().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect();
        let cwd: PathBuf = req.cwd.to_path_buf();
        let spawn = Spawn { space_id: req.space_id.clone(), cwd: cwd.clone(), cols: req.cols, rows: req.rows, history_id: req.history_id.clone(), env };
        let (tx, rx) = mpsc::channel();
        let (space_id, history_id) = (req.space_id, req.history_id);
        self.inner.send(
            Request::Spawn(spawn),
            req.history,
            // In the mirror before the reader thread reads the pane's first output.
            Box::new(move |inner, result, _| {
                let spawned = result.and_then(|v| serde_json::from_value::<Spawned>(v).map_err(|e| e.to_string()));
                let _ = tx.send(spawned.map(|spawned| {
                    let meta = Arc::new(PaneMeta { id: pane_id(spawned.pane), space_id, shell_pid: spawned.shell_pid, cwd, history_id });
                    inner.mirror.lock().unwrap().insert(Pane {
                        number: spawned.pane,
                        meta: meta.clone(),
                        fg: None,
                        title: String::new(),
                        seq: spawned.seq,
                        sink: Some(output),
                        syncing: false,
                        on_exit: Some(on_exit),
                        killed: false,
                    });
                    meta
                }));
            }),
        )?;
        rx.recv_timeout(TIMEOUT).map_err(|_| anyhow!("the terminal host didn't answer"))?.map_err(anyhow::Error::msg)
    }

    fn attach(&self, id: &str, output: Channel<InvokeResponseBody>) -> anyhow::Result<()> {
        let number = {
            let mut mirror = self.inner.mirror.lock().unwrap();
            let pane = mirror.live_mut(id).ok_or_else(|| anyhow!("unknown pane {id}"))?;
            pane.sink = Some(output);
            pane.syncing = true;
            pane.number
        };
        let (tx, rx) = mpsc::channel();
        self.inner.send(
            Request::Attach { pane: number, from_seq: None },
            Vec::new(),
            Box::new(move |inner, result, _| {
                if let Some(pane) = inner.mirror.lock().unwrap().by_number(number) {
                    pane.syncing = false;
                }
                let _ = tx.send(result);
            }),
        )?;
        rx.recv_timeout(TIMEOUT).map_err(|_| anyhow!("the terminal host didn't answer"))?.map_err(anyhow::Error::msg)?;
        Ok(())
    }

    fn write(&self, id: &str, data: &[u8]) -> anyhow::Result<()> {
        let number = self.number(id)?;
        let mut writer = self.inner.writer.lock().unwrap();
        for chunk in data.chunks(CHUNK) {
            writer.write_all(&proto::input(number, chunk))?;
        }
        Ok(())
    }

    fn resize(&self, id: &str, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.call(Request::Resize { pane: self.number(id)?, cols, rows }).map(drop)
    }

    fn kill(&self, id: &str) {
        let number = {
            let mut mirror = self.inner.mirror.lock().unwrap();
            let Some(pane) = mirror.live_mut(id) else { return };
            pane.killed = true;
            pane.number
        };
        self.inner.tell(Request::Kill { pane: number });
    }

    fn meta(&self, id: &str) -> Option<Arc<PaneMeta>> {
        self.inner.mirror.lock().unwrap().live(id).map(|p| p.meta.clone())
    }

    fn list(&self) -> Vec<Arc<PaneMeta>> {
        self.inner.mirror.lock().unwrap().panes.values().filter(|p| !p.killed).map(|p| p.meta.clone()).collect()
    }

    fn each_mirror(&self, visit: &mut dyn FnMut(PaneMirror<'_>)) {
        for pane in self.inner.mirror.lock().unwrap().panes.values().filter(|p| !p.killed) {
            visit(PaneMirror { meta: &pane.meta, foreground_pid: pane.fg, title: &pane.title });
        }
    }

    fn foreground_pid(&self, id: &str) -> Option<u32> {
        self.inner.mirror.lock().unwrap().live(id)?.fg
    }

    fn recent_output(&self, id: &str) -> Option<Vec<u8>> {
        self.call(Request::Read { pane: self.number(id).ok()? }).ok().map(|(_, blob)| blob)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ptyd::host::{test_host, Config};

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

    fn text(got: &Got) -> String {
        String::from_utf8_lossy(&got.lock().unwrap()).into_owned()
    }

    fn wait_for(got: &Got, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !text(got).contains(what) {
            assert!(Instant::now() < deadline, "never saw {what:?}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn request<'a>(name: &str, cwd: &'a Path, history: Vec<u8>) -> SpawnRequest<'a> {
        SpawnRequest { space_id: "s".into(), cwd, cols: 80, rows: 24, history_id: Some(format!("h-{name}")), history }
    }

    #[test]
    fn runs_panes_in_the_host_and_reads_them_from_its_mirror() {
        let (path, _host) = test_host(Config::default());
        let backend = RemoteBackend::connect(&path).unwrap();
        let (output, got) = recorder();
        let (exited, exit_rx) = mpsc::channel();
        let dir = std::env::temp_dir();
        let history = crate::ptyd::output::restored(b"saved line\r\n".to_vec());
        let meta = backend.spawn(request("p7", &dir, history), output, Box::new(move |id| exited.send(id.to_string()).unwrap())).unwrap();
        assert_eq!((meta.id.as_str(), meta.space_id.as_str(), meta.history_id.as_deref()), ("p1", "s", Some("h-p7")), "the host's number");
        assert!(meta.shell_pid.is_some() && meta.cwd == dir);
        backend.write("p1", b"printf '\\033]0;hel''lo\\007'; echo ec''ho $WINGS_PANE_ID\r").unwrap();
        wait_for(&got, "echo p1");
        assert!(!text(&got).contains("saved line"), "the UI writes saved output itself");
        let (mut title, mut fg) = (String::new(), None);
        let deadline = Instant::now() + Duration::from_secs(10);
        while title != "hello" || fg.is_none() {
            assert!(Instant::now() < deadline, "title {title:?}, foreground {fg:?}");
            backend.each_mirror(&mut |p| (title, fg) = (p.title.to_string(), p.foreground_pid));
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(backend.foreground_pid("p1"), fg);
        assert!(backend.recent_output("p1").unwrap().starts_with(b"saved line"), "kept for the next quit");
        // A UI that loaded again gets what was kept, then the stream instead of the old terminal.
        let (again, got_again) = recorder();
        backend.attach("p1", again).unwrap();
        wait_for(&got_again, "echo");
        backend.write("p1", b"echo af''ter\r").unwrap();
        wait_for(&got_again, "after");
        assert!(!text(&got).contains("after"));
        backend.resize("p1", 100, 30).unwrap();
        assert!(backend.write("nope", b"x").is_err() && backend.meta("nope").is_none() && backend.resize("nope", 1, 1).is_err());
        // Another Wings connecting, as after a restart, finds it running; closing that connection leaves it be.
        let other = RemoteBackend::connect(&path).unwrap();
        assert_eq!(other.list().iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["p1"]);
        drop(other);
        backend.kill("p1");
        assert!(backend.meta("p1").is_none() && backend.list().is_empty(), "forgotten as soon as it is killed");
        assert_eq!(exit_rx.recv_timeout(Duration::from_secs(10)).unwrap(), "p1");
        assert!(RemoteBackend::connect(&path).unwrap().list().is_empty(), "the host forgot it too");
    }

    #[test]
    fn a_shell_that_exits_is_reported_and_forgotten() {
        let (path, _host) = test_host(Config::default());
        let backend = RemoteBackend::connect(&path).unwrap();
        let (output, _) = recorder();
        let (exited, exit_rx) = mpsc::channel();
        let dir = std::env::temp_dir();
        backend.spawn(request("p1", &dir, Vec::new()), output, Box::new(move |id| exited.send(id.to_string()).unwrap())).unwrap();
        backend.write("p1", b"exit\r").unwrap();
        assert_eq!(exit_rx.recv_timeout(Duration::from_secs(10)).unwrap(), "p1");
        assert!(backend.list().is_empty());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !RemoteBackend::connect(&path).unwrap().list().is_empty() {
            assert!(Instant::now() < deadline, "the host kept a pane its client saw exit");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn catches_up_after_lagging_without_losing_or_repeating_a_byte() {
        let (path, _host) = test_host(Config { client_queue: 64 << 10, ..Config::default() });
        let backend = RemoteBackend::connect(&path).unwrap();
        // A webview slow to take the first frames stalls the reading thread, so the host's queue overflows.
        let got = Got::default();
        let sink = got.clone();
        let frames = Arc::new(AtomicU32::new(0));
        let output = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                if frames.fetch_add(1, Ordering::Relaxed) < 5 {
                    thread::sleep(Duration::from_millis(300));
                }
                sink.lock().unwrap().extend(bytes);
            }
            Ok(())
        });
        let dir = std::env::temp_dir();
        backend.spawn(request("p1", &dir, Vec::new()), output, Box::new(|_| {})).unwrap();
        backend.write("p1", b"seq 1 100000; echo do''ne\r").unwrap();
        wait_for(&got, "done");
        assert!(backend.inner.lags.load(Ordering::Relaxed) > 0, "never lagged");
        // A shell's integration marker (OSC 133;C) can come right before the first number.
        let numbers: Vec<u32> = text(&got).split("\r\n").filter_map(|line| line.rsplit('\x07').next()?.parse().ok()).collect();
        assert_eq!(numbers.len(), 100_000);
        assert!(numbers.iter().zip(1..).all(|(&n, i)| n == i), "out of order, lost or repeated");
        backend.kill("p1");
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use crate::ptyd::{
        host::{test_host, Config, TestHost},
        local::bench::{echo_latency, quantile, throughput},
        LocalBackend,
    };

    /// The host being measured. Dropping it stops a host process and removes its folder.
    enum Measured {
        Thread(#[allow(dead_code)] TestHost),
        Process(PathBuf, std::process::Child),
    }

    impl Drop for Measured {
        fn drop(&mut self) {
            if let Measured::Process(dir, child) = self {
                let _ = child.kill();
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    /// The real `wings ptyd` in its own process when `WINGS_PTYD_BIN` names the binary, else a host on a thread of
    /// this test.
    fn host() -> (PathBuf, Measured) {
        let Some(bin) = std::env::var_os("WINGS_PTYD_BIN") else {
            let (path, serving) = test_host(Config::default());
            return (path, Measured::Thread(serving));
        };
        let dir = std::env::temp_dir().join(format!("wptyd-bench-{}", std::process::id()));
        let path = dir.join("s.sock");
        let child = Command::new(bin).arg("ptyd").arg("--socket").arg(&path).spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while UnixStream::connect(&path).is_err() {
            assert!(Instant::now() < deadline, "the host didn't start");
            thread::sleep(Duration::from_millis(10));
        }
        (path, Measured::Process(dir, child))
    }

    /// `cargo test --release --lib pty_echo_latency_remote_vs_local -- --ignored --nocapture`, rounds of each
    /// backend in turn, since load on the machine drifts.
    #[test]
    #[ignore]
    fn pty_echo_latency_remote_vs_local() {
        let (path, _host) = host();
        let remote = RemoteBackend::connect(&path).unwrap();
        let local = LocalBackend::default();
        let (mut all_local, mut all_remote) = (Vec::new(), Vec::new());
        for round in 0..8 {
            let run = |backend: &dyn PtyBackend, name: &str, all: &mut Vec<f64>| {
                let micros = echo_latency(backend, 500);
                let line = format!("{name} median {:>4.0} us p95 {:>4.0} us", quantile(&micros, 0.5), quantile(&micros, 0.95));
                all.extend(micros);
                line
            };
            let (l, r) = if round % 2 == 0 {
                let l = run(&local, "local", &mut all_local);
                (l, run(&remote, "remote", &mut all_remote))
            } else {
                let r = run(&remote, "remote", &mut all_remote);
                (run(&local, "local", &mut all_local), r)
            };
            println!("round {round}: {l} | {r}");
        }
        for (name, all) in [("local", &mut all_local), ("remote", &mut all_remote)] {
            all.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("pty_echo_latency {name}: {} keys, median {:.0} us, p95 {:.0} us, max {:.0} us", all.len(), quantile(all, 0.5), quantile(all, 0.95), all[all.len() - 1]);
        }
    }

    /// `cargo test --release --lib pty_throughput_remote_vs_local -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn pty_throughput_remote_vs_local() {
        let (path, _host) = host();
        let remote = RemoteBackend::connect(&path).unwrap();
        let local = LocalBackend::default();
        for round in 0..6 {
            let backend: &dyn PtyBackend = if round % 2 == 0 { &local } else { &remote };
            let (mb, secs) = throughput(backend);
            println!("pty_throughput {}: {mb:.0} MB in {secs:.2}s = {:.0} MB/s", if round % 2 == 0 { "local" } else { "remote" }, mb / secs);
        }
    }
}
