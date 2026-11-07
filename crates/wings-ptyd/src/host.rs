//! `wings ptyd`: the process that owns the terminals, so they can keep running while Wings restarts.
//! It reads every pane's output into its ring whether or not anyone is connected (a macOS PTY stops its writer after
//! about 1 KB nobody reads), and serves any number of this user's Wings processes over a Unix socket.
//!
//! Nothing a client does can stop a pane's output: each client has its own queue, written by its own thread, and a
//! client that falls behind is told it lagged and catches up from the ring.

use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{self, BufReader, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{DirBuilderExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::Duration,
};

use portable_pty::{native_pty_system, Child, MasterPty, PtySize};
use serde_json::{json, Value};

use wings_terminal::shell::pane_command;

use crate::{
    output::{Kept, REPLAY_BYTES},
    pane_id,
    proto::{self, Attached, Call, Frame, HostMsg, PaneInfo, Request, Spawn, Spawned, CHUNK, PROTO},
    SERVER_VERSION,
};

pub struct Config {
    /// Output queued for one client before it is told it lagged.
    pub client_queue: usize,
    /// The only user who may connect.
    pub uid: u32,
    /// The executable the host runs from. Once a new one is put at this path, as an update does, the host exits as
    /// soon as it has no pane, and launchd starts the new one.
    pub exe: Option<PathBuf>,
    /// With no pane and no client for this long, the server exits. `None` runs it until it is replaced or drained.
    pub idle: Option<Duration>,
}

impl Default for Config {
    fn default() -> Config {
        Config { client_queue: 8 << 20, uid: unsafe { libc::getuid() }, exe: None, idle: None }
    }
}

/// Why `serve` returned.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Stopped {
    /// Its executable was replaced and it had no pane.
    Replaced,
    /// It was told to drain and its last pane ended.
    Drained,
    /// Nothing used it for `Config::idle`.
    Idle,
}

/// How often the foreground process of each pane is checked while a client is connected. Twice per tick of the
/// app's agent detection, which reads it.
const FG_POLL: Duration = Duration::from_millis(250);

/// How often the host checks whether its executable was replaced.
const TICK: Duration = Duration::from_secs(1);

/// Saved output a client may send ahead of a spawn: a ring's worth, and its end.
const MAX_STAGED: usize = 2 * REPLAY_BYTES;

/// Serves on `path` until `serve` returns, with the executable it runs from watched for an update.
pub fn main(path: &Path, idle: Option<Duration>) -> Result<Stopped, io::Error> {
    let listener = bind(path)?;
    Ok(serve(listener, path, Config { exe: std::env::current_exe().ok(), idle, ..Config::default() }))
}

/// Listens at `path`, in a folder only this user can open. Refuses if another host already answers there.
pub fn bind(path: &Path) -> io::Result<UnixListener> {
    let dir = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder"))?;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    if fs::metadata(dir)?.uid() != unsafe { libc::getuid() } {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the folder belongs to another user"));
    }
    // Others can't reach the socket through a 0700 folder, even before it is chmod'ed below.
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    if UnixStream::connect(path).is_ok() {
        return Err(io::Error::new(io::ErrorKind::AddrInUse, "another terminal host is running"));
    }
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Serves clients until its executable was replaced and it has no pane, it drained, or it sat idle, then removes the
/// socket.
pub fn serve(listener: UnixListener, path: &Path, config: Config) -> Stopped {
    // The file it started from: an update swaps the whole bundle, so the path then names another file.
    let started = config.exe.as_ref().and_then(|exe| fs::metadata(exe).ok()).map(|m| (m.dev(), m.ino()));
    let replaced = |exe: &Path| started.is_some_and(|ino| fs::metadata(exe).is_ok_and(|m| (m.dev(), m.ino()) != ino));
    let host = Arc::new(Host::new(config));
    let poller = host.clone();
    thread::Builder::new().name("ptyd-fg".into()).spawn(move || poller.poll_foreground()).expect("thread");
    let mut used = std::time::Instant::now();
    let stopped = loop {
        if readable(&listener, TICK) {
            if let Ok((stream, _)) = listener.accept() {
                host.accept(stream);
            }
        }
        // Drained once nothing runs in it and no app is left to read the panes that exited.
        if host.draining.load(Ordering::Relaxed)
            && host.clients.lock().unwrap().is_empty()
            && host.all_panes().iter().all(|pane| pane.out.lock().unwrap().exited)
        {
            break Stopped::Drained;
        }
        if !host.panes.lock().unwrap().is_empty() {
            used = std::time::Instant::now();
            continue;
        }
        // A connected app keeps an idle server, but not one that was replaced or drained.
        if !host.clients.lock().unwrap().is_empty() {
            used = std::time::Instant::now();
        }
        if host.config.exe.as_deref().is_some_and(replaced) {
            break Stopped::Replaced;
        }

        if host.config.idle.is_some_and(|idle| used.elapsed() >= idle) {
            break Stopped::Idle;
        }
    };
    let _ = fs::remove_file(path);
    host.stopped.store(true, Ordering::Relaxed);
    host.clients_changed.notify_all();
    stopped
}

/// Waits up to `timeout` for a connection, so the accept loop also checks its executable while nobody connects.
fn readable(listener: &UnixListener, timeout: Duration) -> bool {
    let mut fd = libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut fd, 1, timeout.as_millis() as libc::c_int) > 0 }
}

/// The user on the other end of a connection.
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let (mut uid, mut gid) = (0, 0);
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

struct Host {
    config: Config,
    panes: Mutex<HashMap<u32, Arc<HostPane>>>,
    next_pane: AtomicU32,
    clients: Mutex<HashMap<u64, Arc<Client>>>,
    /// Wakes the foreground poller when the first client connects.
    clients_changed: Condvar,
    next_client: AtomicU64,
    stopped: AtomicBool,
    /// Told to drain: it starts no pane, and exits once its last one ends.
    draining: AtomicBool,
}

struct HostPane {
    /// Never given twice while the host runs, so the app can use it as the pane's id across its restarts.
    id: u32,
    space_id: String,
    cwd: PathBuf,
    history_id: Option<String>,
    shell_pid: Option<u32>,
    title: Mutex<String>,
    /// Columns and rows, as last resized.
    size: Mutex<(u16, u16)>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    out: Mutex<PaneOut>,
}

struct PaneOut {
    kept: Kept,
    /// Bytes this pane has produced, its saved output included. The ring holds the newest of them.
    seq: u64,
    /// The clients its output streams to.
    attached: Vec<Arc<Client>>,
    /// The title and foreground process clients were last told.
    sent_title: String,
    sent_codex_status: Option<wings_agents::program_status::CodexReport>,
    fg: Option<u32>,
    exited: bool,
    /// Killed: forget it as soon as its output ends.
    forget: bool,
}

impl Host {
    fn new(config: Config) -> Host {
        Host {
            config,
            panes: Mutex::default(),
            next_pane: AtomicU32::new(1),
            clients: Mutex::default(),
            clients_changed: Condvar::new(),
            next_client: AtomicU64::new(1),
            stopped: AtomicBool::new(false),
            draining: AtomicBool::new(false),
        }
    }

    fn pane(&self, id: u32) -> Option<Arc<HostPane>> {
        self.panes.lock().unwrap().get(&id).cloned()
    }

    fn all_panes(&self) -> Vec<Arc<HostPane>> {
        self.panes.lock().unwrap().values().cloned().collect()
    }

    /// Sends an event to every client. Its lock order: a pane's output, then the client list, then a queue.
    fn broadcast(&self, msg: &HostMsg) {
        let Some(frame) = ctrl_frame(msg) else { return };
        for client in self.clients.lock().unwrap().values() {
            client.send(None, frame.clone());
        }
    }

    fn accept(self: &Arc<Self>, stream: UnixStream) {
        // Another user gets the connection closed before a byte is read.
        if peer_uid(&stream).ok() != Some(self.config.uid) {
            return;
        }
        let id = self.next_client.fetch_add(1, Ordering::Relaxed);
        let client = Arc::new(Client::new(id, self.config.client_queue));
        let host = self.clone();
        let _ = thread::Builder::new().name("ptyd-client".into()).spawn(move || {
            host.serve_client(&client, stream);
            client.close();
            host.leave(&client);
        });
    }

    fn leave(&self, client: &Arc<Client>) {
        self.clients.lock().unwrap().remove(&client.id);
        for pane in self.all_panes() {
            pane.out.lock().unwrap().attached.retain(|c| !Arc::ptr_eq(c, client));
        }
    }

    fn serve_client(self: &Arc<Self>, client: &Arc<Client>, stream: UnixStream) {
        let Ok(write_half) = stream.try_clone() else { return };
        let mut reader = BufReader::with_capacity(64 << 10, stream);
        let Ok(Some(Frame::Ctrl(json))) = proto::read(&mut reader) else { return };
        let Ok(Call { req: Request::Hello { proto, min_proto }, .. }) = serde_json::from_slice(&json) else { return };
        let both = proto::negotiate(proto, min_proto);
        let welcome = HostMsg::Welcome {
            proto: both.unwrap_or(PROTO),
            host_version: SERVER_VERSION,
            features: Vec::new(),
            error: both.is_none().then(|| format!("this host speaks protocols {}..={PROTO}, not {min_proto}..={proto}", proto::MIN_PROTO)),
        };
        let Some(welcome) = ctrl_frame(&welcome) else { return };
        if both.is_none() {
            let _ = (&write_half).write_all(&welcome);
            return;
        }
        client.send(None, welcome);
        // Only now does it hear what every app hears: a title or an exit sent before the welcome would take its place.
        self.clients.lock().unwrap().insert(client.id, client.clone());
        self.clients_changed.notify_all();
        let writer = client.clone();
        let Ok(mut socket) = write_half.try_clone() else { return };
        let spawned = thread::Builder::new().name("ptyd-write".into()).spawn(move || {
            while let Some(frame) = writer.next() {
                if socket.write_all(&frame).is_err() {
                    break;
                }
            }
            writer.close();
            // Ends the read below too, when the client was closed for not reading.
            let _ = socket.shutdown(std::net::Shutdown::Both);
        });
        if spawned.is_err() {
            return;
        }
        let mut staged: HashMap<u32, Vec<u8>> = HashMap::new();
        // Until the client hangs up, or the write thread does: keystrokes sent just before either still count.
        while let Ok(Some(frame)) = proto::read(&mut reader) {
            match frame {
                Frame::In { pane, data } => {
                    if let Some(pane) = self.pane(pane) {
                        let _ = pane.write(&data);
                    }
                }
                Frame::Blob { req, data } => {
                    let staged = staged.entry(req).or_default();
                    if staged.len() + data.len() > MAX_STAGED {
                        break;
                    }
                    staged.extend_from_slice(&data);
                }
                Frame::Ctrl(json) => match serde_json::from_slice::<Call>(&json) {
                    Ok(call) => {
                        let blob = staged.remove(&call.id);
                        self.request(client, call.id, call.req, blob.unwrap_or_default());
                    }
                    Err(e) => {
                        let id = serde_json::from_slice::<Value>(&json).ok().and_then(|v| v["id"].as_u64()).unwrap_or(0) as u32;
                        reply(client, id, Err(e.to_string()));
                    }
                },
                Frame::Out { .. } => break,
            }
        }
        // Ends the write thread too, if it is stuck writing to a client that stopped reading.
        let _ = write_half.shutdown(std::net::Shutdown::Both);
    }

    fn request(self: &Arc<Self>, client: &Arc<Client>, id: u32, req: Request, blob: Vec<u8>) {
        let unknown = |pane: u32| Err(format!("unknown pane {pane}"));
        match req {
            Request::Hello { .. } => reply(client, id, Err("hello twice".into())),
            Request::Spawn(_) if self.draining.load(Ordering::Relaxed) => reply(client, id, Err("this server is draining".into())),
            Request::Spawn(spawn) => {
                if let Err(e) = self.spawn(client, id, spawn, blob) {
                    reply(client, id, Err(format!("{e:#}")));
                }
            }
            Request::Attach { pane, from_seq } => match self.pane(pane) {
                Some(pane) => pane.attach(client, id, from_seq),
                None => reply(client, id, unknown(pane)),
            },
            Request::Resize { pane, cols, rows } => {
                let resized = match self.pane(pane) {
                    Some(pane) => pane.resize(cols, rows).map(|_| Value::Null).map_err(|e| e.to_string()),
                    None => unknown(pane),
                };
                reply(client, id, resized);
            }
            Request::Kill { pane } => {
                self.kill(pane);
                reply(client, id, Ok(Value::Null));
            }
            Request::List => reply(client, id, Ok(serde_json::to_value(self.list()).expect("serializes"))),
            Request::Read { pane } => match self.pane(pane) {
                Some(pane) => {
                    let kept = pane.out.lock().unwrap().kept.ring.contents();
                    for chunk in kept.chunks(CHUNK) {
                        client.send(None, Arc::new(proto::blob(id, chunk)));
                    }
                    reply(client, id, Ok(json!({ "len": kept.len() })));
                }
                None => reply(client, id, unknown(pane)),
            },
            Request::Drain => {
                self.draining.store(true, Ordering::Relaxed);
                reply(client, id, Ok(Value::Null));
            }
        }
    }

    fn spawn(self: &Arc<Self>, client: &Arc<Client>, id: u32, spawn: Spawn, history: Vec<u8>) -> anyhow::Result<()> {
        let number = self.next_pane.fetch_add(1, Ordering::Relaxed);
        let pair = native_pty_system().openpty(PtySize { rows: spawn.rows, cols: spawn.cols, pixel_width: 0, pixel_height: 0 })?;
        let env = spawn.env.into_iter().map(|(k, v)| (k.into(), v.into()));
        let child = pair.slave.spawn_command(pane_command(&spawn.cwd, env, &pane_id(number)))?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader()?;
        let pane = Arc::new(HostPane {
            id: number,
            space_id: spawn.space_id,
            cwd: spawn.cwd,
            history_id: spawn.history_id,
            shell_pid: child.process_id(),
            title: Mutex::default(),
            size: Mutex::new((spawn.cols, spawn.rows)),
            writer: Mutex::new(pair.master.take_writer()?),
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
            out: Mutex::new(PaneOut {
                kept: Kept::default(),
                seq: 0,
                attached: vec![client.clone()],
                sent_title: String::new(),
                sent_codex_status: None,
                fg: None,
                exited: false,
                forget: false,
            }),
        });
        {
            let mut out = pane.out.lock().unwrap();
            if !history.is_empty() {
                // Kept for the next quit and for `read`, never streamed: the UI writes it into the terminal itself.
                out.kept.push(&mut vte::Parser::new(), &pane.title, &history);
                out.seq = history.len() as u64;
                // The old program's title isn't the new shell's.
                pane.title.lock().unwrap().clear();
                out.kept.codex_status = None;
            }
            // Queued before the read loop starts, so the client knows the pane before its first output.
            let spawned = Spawned { pane: pane.id, shell_pid: pane.shell_pid, seq: out.seq };
            reply(client, id, Ok(serde_json::to_value(spawned).expect("serializes")));
        }
        self.panes.lock().unwrap().insert(pane.id, pane.clone());
        let host = self.clone();
        thread::Builder::new().name(format!("ptyd-pane-{}", pane.id)).spawn(move || host.read_loop(&pane, reader))?;
        Ok(())
    }

    /// Reads the pane until its output ends, into the ring and to every attached client. Never waits for a client.
    fn read_loop(&self, pane: &Arc<HostPane>, mut reader: Box<dyn Read + Send>) {
        let mut parser = vte::Parser::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let mut out = pane.out.lock().unwrap();
            let previous_report = out.kept.codex_status;
            out.kept.push(&mut parser, &pane.title, &buf[..n]);
            if out.kept.codex_status != previous_report {
                if let Some(report) = out.kept.codex_status.as_mut() {
                    report.pid = pane.master.lock().unwrap().process_group_leader().map(|p| p as u32);
                }
            }
            out.seq += n as u64;
            if !out.attached.is_empty() {
                let frame = Arc::new(proto::out(pane.id, out.seq, &buf[..n]));
                out.attached.retain(|client| client.stream(pane.id, &frame));
            }
            let title = pane.title.lock().unwrap();
            if *title != out.sent_title || out.kept.codex_status != out.sent_codex_status {
                out.sent_title.clone_from(&title);
                out.sent_codex_status = out.kept.codex_status;
                drop(title);
                self.broadcast(&HostMsg::Title { pane: pane.id, title: out.sent_title.clone(), codex_status: out.sent_codex_status });
            }
        }
        // Reaps the shell, which exits as its side of the PTY closes.
        for _ in 0..50 {
            if !matches!(pane.child.lock().unwrap().try_wait(), Ok(None)) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let forget = {
            let mut out = pane.out.lock().unwrap();
            out.exited = true;
            out.attached.clear();
            self.broadcast(&HostMsg::Exited { pane: pane.id });
            out.forget
        };
        if forget {
            self.panes.lock().unwrap().remove(&pane.id);
        }
    }

    fn kill(&self, id: u32) {
        let Some(pane) = self.pane(id) else { return };
        let exited = {
            let mut out = pane.out.lock().unwrap();
            out.forget = true;
            out.attached.clear();
            out.exited
        };
        if exited {
            self.panes.lock().unwrap().remove(&id);
        } else {
            // portable_pty gives a SIGHUP 200 ms before it sends SIGKILL; this client's next keystroke shouldn't wait.
            thread::spawn(move || {
                let _ = pane.child.lock().unwrap().kill();
            });
        }
    }

    fn list(&self) -> Vec<PaneInfo> {
        let mut panes: Vec<PaneInfo> = self
            .all_panes()
            .iter()
            .filter_map(|pane| {
                // Asked now, since nobody polls it while no client is connected.
                let fg = pane.master.lock().unwrap().process_group_leader().map(|pid| pid as u32);
                let mut out = pane.out.lock().unwrap();
                if out.forget {
                    return None;
                }
                if !out.exited {
                    out.fg = fg;
                }
                let (cols, rows) = *pane.size.lock().unwrap();
                Some(PaneInfo {
                    pane: pane.id,
                    space_id: pane.space_id.clone(),
                    cwd: pane.cwd.clone(),
                    history_id: pane.history_id.clone(),
                    shell_pid: pane.shell_pid,
                    fg: out.fg,
                    title: pane.title.lock().unwrap().clone(),
                    codex_status: out.kept.codex_status,
                    seq: out.seq,
                    exited: out.exited,
                    cols,
                    rows,
                })
            })
            .collect();
        panes.sort_by_key(|p| p.pane);
        panes
    }

    /// While a client is connected, tells clients when a pane's foreground process changes, so they never ask.
    fn poll_foreground(&self) {
        loop {
            {
                let mut clients = self.clients.lock().unwrap();
                while clients.is_empty() && !self.stopped.load(Ordering::Relaxed) {
                    clients = self.clients_changed.wait(clients).unwrap();
                }
            }
            if self.stopped.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(FG_POLL);
            for pane in self.all_panes() {
                let pid = pane.master.lock().unwrap().process_group_leader().map(|pid| pid as u32);
                let mut out = pane.out.lock().unwrap();
                if out.exited || out.fg == pid {
                    continue;
                }
                out.fg = pid;
                self.broadcast(&HostMsg::Fg { pane: pane.id, pid });
            }
        }
    }
}

fn reply(client: &Client, id: u32, result: Result<Value, String>) {
    let msg = match result {
        Ok(ok) => HostMsg::Reply { id, ok, error: None },
        Err(error) => HostMsg::Reply { id, ok: Value::Null, error: Some(error) },
    };
    // A reply too big for a frame still answers, so the client doesn't wait for one.
    let too_big = || HostMsg::Reply { id, ok: Value::Null, error: Some("the reply doesn't fit a frame".into()) };
    if let Some(frame) = ctrl_frame(&msg).or_else(|| ctrl_frame(&too_big())) {
        client.send(None, frame);
    }
}

/// A control frame, or `None` for a message too big for one: it is dropped and logged, never a panic, which in a
/// release build would end every pane in the host.
fn ctrl_frame(msg: &HostMsg) -> Option<Arc<Vec<u8>>> {
    match proto::ctrl(msg) {
        Ok(frame) => Some(Arc::new(frame)),
        Err(e) => {
            eprintln!("[ptyd] dropped a message: {e}");
            None
        }
    }
}

impl HostPane {
    fn write(&self, data: &[u8]) -> io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data)?;
        writer.flush()
    }

    fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.master.lock().unwrap().resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        *self.size.lock().unwrap() = (cols, rows);
        Ok(())
    }

    /// Replies, then sends what the client is missing and streams the rest, all under the output lock, so it gets
    /// every byte once and in order.
    fn attach(&self, client: &Arc<Client>, id: u32, from_seq: Option<u64>) {
        let mut out = self.out.lock().unwrap();
        out.attached.retain(|c| !Arc::ptr_eq(c, client));
        client.drop_output(self.id);
        let delta = from_seq.filter(|&from| from <= out.seq).and_then(|from| out.kept.ring.tail((out.seq - from) as usize));
        let reset = delta.is_none();
        let bytes = delta.unwrap_or_else(|| out.kept.replay());
        let (cols, rows) = *self.size.lock().unwrap();
        reply(client, id, Ok(serde_json::to_value(Attached { pane: self.id, reset, seq: out.seq, cols, rows }).expect("serializes")));
        // A reset replay starts with the modes in force and loses part of its first line, but it always ends where
        // the ring does, so each piece's `seq_end` counts back from there.
        // An empty replay is still one frame, so the client can tell where the replay ends.
        if bytes.is_empty() {
            client.send(Some(self.id), Arc::new(proto::out(self.id, out.seq, &[])));
        }
        let mut end = 0;
        for chunk in bytes.chunks(CHUNK) {
            end += chunk.len();
            let seq_end = out.seq.saturating_sub((bytes.len() - end) as u64);
            client.send(Some(self.id), Arc::new(proto::out(self.id, seq_end, chunk)));
        }
        if !out.exited {
            out.attached.push(client.clone());
        }
    }
}

/// One connection's outgoing frames, written by its own thread.
struct Client {
    id: u64,
    queue: Mutex<Queue>,
    ready: Condvar,
    /// Streamed output stops here and the client lags.
    cap: usize,
}

#[derive(Default)]
struct Queue {
    /// Each frame with the pane whose output it carries.
    frames: VecDeque<(Option<u32>, Arc<Vec<u8>>)>,
    bytes: usize,
    closed: bool,
}

impl Client {
    fn new(id: u64, cap: usize) -> Client {
        Client { id, queue: Mutex::default(), ready: Condvar::new(), cap }
    }

    /// Queues a reply, an event or a replay. These aren't dropped like streamed output, but a client that lets them
    /// pile up past two replays beyond `cap` isn't reading at all, and is closed.
    fn send(&self, pane: Option<u32>, frame: Arc<Vec<u8>>) {
        let mut queue = self.queue.lock().unwrap();
        if queue.closed {
            return;
        }
        queue.bytes += frame.len();
        queue.frames.push_back((pane, frame));
        if queue.bytes > self.cap + 2 * REPLAY_BYTES {
            queue.closed = true;
            queue.frames.clear();
        }
        self.ready.notify_one();
    }

    /// Queues a pane's new output. Past `cap`, drops what is queued for that pane instead and tells the client it
    /// lagged. Returns whether the pane still streams to this client.
    fn stream(&self, pane: u32, frame: &Arc<Vec<u8>>) -> bool {
        let mut queue = self.queue.lock().unwrap();
        if queue.closed {
            return false;
        }
        if queue.bytes + frame.len() <= self.cap {
            queue.bytes += frame.len();
            queue.frames.push_back((Some(pane), frame.clone()));
            self.ready.notify_one();
            return true;
        }
        drop_pane(&mut queue, pane);
        drop(queue);
        if let Some(lagged) = ctrl_frame(&HostMsg::Lagged { pane }) {
            self.send(None, lagged);
        }
        false
    }

    fn drop_output(&self, pane: u32) {
        drop_pane(&mut self.queue.lock().unwrap(), pane);
    }

    fn close(&self) {
        let mut queue = self.queue.lock().unwrap();
        queue.closed = true;
        queue.frames.clear();
        self.ready.notify_one();
    }

    /// The next frame to write, or `None` once closed.
    fn next(&self) -> Option<Arc<Vec<u8>>> {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if queue.closed {
                return None;
            }
            if let Some((_, frame)) = queue.frames.pop_front() {
                queue.bytes -= frame.len();
                return Some(frame);
            }
            queue = self.ready.wait(queue).unwrap();
        }
    }
}

fn drop_pane(queue: &mut Queue, pane: u32) {
    let mut dropped = 0;
    queue.frames.retain(|(p, frame)| {
        let keep = *p != Some(pane);
        if !keep {
            dropped += frame.len();
        }
        keep
    });
    queue.bytes -= dropped;
}

/// A host serving on its own thread, for tests here and in `remote.rs`. Its socket's folder goes when this does,
/// since the host itself only removes the socket when it exits idle.
#[doc(hidden)]
pub struct TestHost {
    dir: PathBuf,
    pub serving: thread::JoinHandle<()>,
    /// Why it stopped, once it has.
    pub stopped: Arc<Mutex<Option<Stopped>>>,
}

impl Drop for TestHost {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A host on a socket in a fresh folder, and the folder's path.
#[doc(hidden)]
pub fn test_host(config: Config) -> (PathBuf, TestHost) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    // Short: a socket path can't be longer than 104 bytes.
    let dir = std::env::temp_dir().join(format!("wptyd-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let path = dir.join("s.sock");
    let listener = bind(&path).unwrap();
    let serving = path.clone();
    let stopped = Arc::new(Mutex::new(None));
    let result = stopped.clone();
    (path, TestHost { dir, serving: thread::spawn(move || *result.lock().unwrap() = Some(serve(listener, &serving, config))), stopped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use wings_terminal::shell::pane_env;

    fn config() -> Config {
        Config::default()
    }

    /// Speaks the protocol by hand, so a test controls when it reads, and records everything it receives.
    struct TestClient {
        stream: UnixStream,
        reader: BufReader<UnixStream>,
        next_id: u32,
        output: HashMap<u32, Vec<u8>>,
        /// Where each pane's received output ends. Every `Out` frame must start exactly here.
        seq: HashMap<u32, u64>,
        events: Vec<HostMsg>,
        replies: HashMap<u32, (Value, Option<String>)>,
        blobs: HashMap<u32, Vec<u8>>,
    }

    impl TestClient {
        fn hello(path: &Path, proto: u32, min_proto: u32) -> (TestClient, Option<HostMsg>) {
            let stream = UnixStream::connect(path).unwrap();
            // macOS refuses socket options once the host has closed the connection, which a refusal does at once.
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let hello = proto::ctrl(&Call { id: 0, req: Request::Hello { proto, min_proto } }).unwrap();
            let _ = (&stream).write_all(&hello);
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let welcome = match proto::read(&mut reader) {
                Ok(Some(Frame::Ctrl(json))) => Some(serde_json::from_slice(&json).unwrap()),
                _ => None,
            };
            let client = TestClient {
                stream,
                reader,
                next_id: 1,
                output: HashMap::new(),
                seq: HashMap::new(),
                events: Vec::new(),
                replies: HashMap::new(),
                blobs: HashMap::new(),
            };
            (client, welcome)
        }

        fn connect(path: &Path) -> TestClient {
            let (client, welcome) = TestClient::hello(path, PROTO, proto::MIN_PROTO);
            assert!(matches!(welcome, Some(HostMsg::Welcome { error: None, .. })), "{welcome:?}");
            client
        }

        fn send(&mut self, req: Request, blob: &[u8]) -> u32 {
            let id = self.next_id;
            self.next_id += 1;
            for chunk in blob.chunks(CHUNK) {
                self.stream.write_all(&proto::blob(id, chunk)).unwrap();
            }
            self.stream.write_all(&proto::ctrl(&Call { id, req }).unwrap()).unwrap();
            id
        }

        /// Reads one frame into the record; false once the host closed the connection.
        fn read_one(&mut self) -> bool {
            match proto::read(&mut self.reader) {
                Ok(Some(Frame::Out { pane, seq_end, data })) => {
                    // A full replay starts where the ring does.
                    let at = self.seq.entry(pane).or_insert(seq_end - data.len() as u64);
                    assert_eq!(seq_end - data.len() as u64, *at, "pane {pane}'s output has a gap or a repeat");
                    *at = seq_end;
                    self.output.entry(pane).or_default().extend(data);
                }
                Ok(Some(Frame::Blob { req, data })) => self.blobs.entry(req).or_default().extend(data),
                Ok(Some(Frame::Ctrl(json))) => match serde_json::from_slice(&json).unwrap() {
                    HostMsg::Reply { id, ok, error } => drop(self.replies.insert(id, (ok, error))),
                    event => self.events.push(event),
                },
                Ok(Some(frame)) => panic!("the host sent {frame:?}"),
                // The end, or nothing for the read timeout.
                Ok(None) | Err(_) => return false,
            }
            true
        }

        fn until(&mut self, what: &str, done: impl Fn(&TestClient) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !done(self) {
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                assert!(self.read_one(), "nothing more from the host before {what}");
            }
        }

        fn call(&mut self, req: Request) -> Value {
            let id = self.send(req, &[]);
            self.until("a reply", |c| c.replies.contains_key(&id));
            let (ok, error) = self.replies.remove(&id).unwrap();
            assert_eq!(error, None);
            ok
        }

        fn spawn(&mut self, name: &str) -> u32 {
            let spawned: Spawned = serde_json::from_value(self.call(Request::Spawn(spawn_request(name)))).unwrap();
            self.seq.insert(spawned.pane, spawned.seq);
            spawned.pane
        }

        fn type_in(&mut self, pane: u32, text: &str) {
            self.stream.write_all(&proto::input(pane, text.as_bytes())).unwrap();
        }

        fn text(&self, pane: u32) -> String {
            String::from_utf8_lossy(self.output.get(&pane).map_or(&[][..], |o| o)).into_owned()
        }

        fn wait_for(&mut self, pane: u32, text: &str) {
            self.until(text, |c| c.text(pane).contains(text));
        }

        fn list(&mut self) -> Vec<PaneInfo> {
            serde_json::from_value(self.call(Request::List)).unwrap()
        }
    }

    fn spawn_request(name: &str) -> Spawn {
        Spawn {
            space_id: "s".into(),
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 24,
            history_id: Some(format!("h-{name}")),
            env: pane_env(None).into_iter().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect(),
        }
    }

    /// Lists until `done` holds for the pane.
    fn wait_listed(client: &mut TestClient, pane: u32, what: &str, done: impl Fn(&PaneInfo) -> bool) -> PaneInfo {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(info) = client.list().into_iter().find(|p| p.pane == pane && done(p)) {
                return info;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn an_app_connecting_while_a_pane_changes_its_title_is_welcomed_first() {
        let (path, _host) = test_host(config());
        let mut client = TestClient::connect(&path);
        let pane = client.spawn("titles");
        // Every title change goes to every connected app, a new one included.
        client.type_in(pane, "i=0; while true; do i=$((i+1)); printf '\\033]0;t%s\\007' $i; done\r");
        client.until("the titles", |c| c.events.iter().filter(|e| matches!(e, HostMsg::Title { .. })).count() > 10);
        for _ in 0..300 {
            let (_, welcome) = TestClient::hello(&path, PROTO, proto::MIN_PROTO);
            assert!(matches!(welcome, Some(HostMsg::Welcome { error: None, .. })), "{welcome:?}");
        }
        client.call(Request::Kill { pane });
    }

    #[test]
    fn spawns_a_pane_and_streams_its_echo_title_and_foreground() {
        let (path, _host) = test_host(config());
        let mut client = TestClient::connect(&path);
        let pane = client.spawn("p1");
        client.type_in(pane, "printf '\\033]0;bu''ild\\007'; echo he''llo $WINGS_PANE_ID\r");
        client.wait_for(pane, &format!("hello p{pane}"));
        client.until("the title", |c| c.events.contains(&HostMsg::Title { pane, title: "build".into(), codex_status: None }));
        client.until("the foreground process", |c| c.events.iter().any(|e| matches!(e, HostMsg::Fg { pane: p, pid: Some(_) } if *p == pane)));
        let listed = client.list();
        assert_eq!(listed.len(), 1);
        let info = &listed[0];
        assert_eq!((info.pane, info.space_id.as_str(), info.history_id.as_deref()), (pane, "s", Some("h-p1")));
        assert!(info.shell_pid.is_some() && !info.exited && info.title == "build");
        let other = client.spawn("p2");
        assert_ne!(other, pane, "one number for each pane");
        client.call(Request::Kill { pane: other });
        client.call(Request::Kill { pane });
    }

    #[test]
    fn keeps_reading_with_no_client_and_a_new_client_catches_up() {
        let (path, _host) = test_host(config());
        let mut first = TestClient::connect(&path);
        let pane = first.spawn("p1");
        first.type_in(pane, "echo be''fore\r");
        first.wait_for(pane, "before");
        let seen = first.seq[&pane];
        // 300 KB with nobody connected: a PTY nobody reads stops its writer after about 1 KB, so `done` only
        // prints if the host keeps reading.
        first.type_in(pane, "sleep 0.3; yes 0123456789 | head -c 300000; echo do''ne\r");
        drop(first);
        thread::sleep(Duration::from_millis(100));
        let mut second = TestClient::connect(&path);
        let info = wait_listed(&mut second, pane, "the output to finish", |p| p.seq > 300_000);
        assert!(!info.exited);
        // Missed bytes only, from where the first client left off.
        let attached: Attached = serde_json::from_value(second.call(Request::Attach { pane, from_seq: Some(seen) })).unwrap();
        assert!(!attached.reset);
        second.seq.insert(pane, seen);
        second.wait_for(pane, "done");
        assert!(!second.text(pane).contains("before"), "sent again");
        assert!(second.text(pane).matches("0123456789").count() >= 300_000 / 11);
        // A fresh terminal gets everything kept.
        let mut third = TestClient::connect(&path);
        let attached: Attached = serde_json::from_value(third.call(Request::Attach { pane, from_seq: None })).unwrap();
        assert!(attached.reset);
        third.wait_for(pane, "done");
        assert!(third.text(pane).contains("before"));
        second.type_in(pane, "echo af''ter\r");
        second.wait_for(pane, "after");
        third.wait_for(pane, "after");
        second.call(Request::Kill { pane });
    }

    #[test]
    fn a_slow_client_lags_and_catches_up_from_the_ring_without_stalling_the_pane() {
        let (path, _host) = test_host(Config { client_queue: 64 << 10, ..config() });
        let mut client = TestClient::connect(&path);
        let pane = client.spawn("p1");
        // A login shell can take a while to start, and the output has to come while the client isn't reading.
        client.type_in(pane, "echo re''ady\r");
        client.wait_for(pane, "ready");
        client.type_in(pane, "yes 0123456789 | head -c 600000; echo do''ne\r");
        // Not reading: the socket fills, then the queue.
        thread::sleep(Duration::from_millis(1500));
        client.until("lagged", |c| c.events.contains(&HostMsg::Lagged { pane }));
        let from = client.seq[&pane];
        let attached: Attached = serde_json::from_value(client.call(Request::Attach { pane, from_seq: Some(from) })).unwrap();
        assert!(!attached.reset, "the ring still has it");
        assert!(attached.seq > 600_000, "the pane kept running while the client lagged");
        // `read_one` checks that every byte arrives exactly once.
        client.wait_for(pane, "done");
        assert!(client.text(pane).matches("0123456789").count() >= 600_000 / 11);
        client.call(Request::Kill { pane });
    }

    #[test]
    fn a_title_bigger_than_a_frame_does_not_take_the_pane_down() {
        let (path, _host) = test_host(config());
        let mut client = TestClient::connect(&path);
        let pane = client.spawn("p1");
        client.type_in(pane, "printf '\\033]0;'; head -c 1100000 /dev/zero | tr '\\0' x; printf '\\007'; echo af''ter\r");
        client.wait_for(pane, "after");
        client.until("the title", |c| c.events.iter().any(|e| matches!(e, HostMsg::Title { title, .. } if title.len() == crate::output::MAX_TITLE)));
        client.type_in(pane, "echo st''ill here\r");
        client.wait_for(pane, "still here");
        assert_eq!(client.list()[0].title, "x".repeat(crate::output::MAX_TITLE));
        client.call(Request::Kill { pane });
    }

    #[test]
    fn two_clients_see_the_same_pane() {
        let (path, _host) = test_host(config());
        let mut a = TestClient::connect(&path);
        let pane = a.spawn("p1");
        let mut b = TestClient::connect(&path);
        b.call(Request::Attach { pane, from_seq: None });
        a.type_in(pane, "echo fr''om a\r");
        b.type_in(pane, "echo fr''om b\r");
        for client in [&mut a, &mut b] {
            client.wait_for(pane, "from a");
            client.wait_for(pane, "from b");
        }
        b.call(Request::Kill { pane });
        a.until("the exit", |c| c.events.contains(&HostMsg::Exited { pane }));
        assert!(b.list().is_empty() && a.list().is_empty());
    }

    #[test]
    fn a_client_with_no_protocol_in_common_is_refused() {
        let (path, _host) = test_host(config());
        let (mut client, welcome) = TestClient::hello(&path, PROTO + 1, PROTO + 1);
        let Some(HostMsg::Welcome { proto, error: Some(_), host_version, .. }) = welcome else { panic!("{welcome:?}") };
        assert_eq!((proto, host_version), (PROTO, SERVER_VERSION));
        assert!(!client.read_one(), "closed");
        let (_, welcome) = TestClient::hello(&path, PROTO + 1, proto::MIN_PROTO);
        assert!(matches!(welcome, Some(HostMsg::Welcome { proto: PROTO, error: None, .. })), "a newer app that still speaks ours");
    }

    #[test]
    fn another_users_connection_is_closed_unanswered() {
        let ours = unsafe { libc::getuid() };
        let (a, b) = UnixStream::pair().unwrap();
        assert_eq!(peer_uid(&a).unwrap(), ours);
        drop((a, b));
        // As if this user were someone else.
        let (path, _host) = test_host(Config { uid: ours + 1, ..config() });
        let (_, welcome) = TestClient::hello(&path, PROTO, proto::MIN_PROTO);
        assert!(welcome.is_none());
    }

    #[test]
    fn an_exited_pane_stays_with_its_output_until_killed() {
        let (path, _host) = test_host(config());
        let mut first = TestClient::connect(&path);
        let pane = first.spawn("p1");
        first.type_in(pane, "sleep 0.3; echo by''e; exit\r");
        drop(first);
        let mut second = TestClient::connect(&path);
        wait_listed(&mut second, pane, "the exit", |p| p.exited);
        let read = second.send(Request::Read { pane }, &[]);
        second.until("the read", |c| c.replies.contains_key(&read));
        assert!(String::from_utf8_lossy(&second.blobs[&read]).contains("bye"));
        assert_eq!(second.list().len(), 1, "still there");
        second.call(Request::Kill { pane });
        assert!(second.list().is_empty());
    }

    #[test]
    fn spawn_keeps_saved_output_without_streaming_it() {
        let (path, _host) = test_host(config());
        let mut client = TestClient::connect(&path);
        let saved = b"\x1b]0;old\x07old output\r\n".repeat(30_000);
        let id = client.send(Request::Spawn(spawn_request("p1")), &saved);
        client.until("the spawn", |c| c.replies.contains_key(&id));
        let spawned: Spawned = serde_json::from_value(client.replies[&id].0.clone()).unwrap();
        assert_eq!(spawned.seq, saved.len() as u64);
        let pane = spawned.pane;
        client.seq.insert(pane, spawned.seq);
        client.type_in(pane, "echo n''ew\r");
        client.wait_for(pane, "new");
        assert!(!client.text(pane).contains("old output"));
        let read = client.send(Request::Read { pane }, &[]);
        client.until("the read", |c| c.replies.contains_key(&read));
        assert!(String::from_utf8_lossy(&client.blobs[&read]).contains("old output\r\n"));
        assert_ne!(client.list()[0].title, "old", "the old program's title isn't the new shell's");
        client.call(Request::Kill { pane });
    }

    #[test]
    fn exits_once_its_executable_was_replaced_and_it_has_no_pane() {
        let exe = std::env::temp_dir().join(format!("wptyd-exe-{}", std::process::id()));
        fs::write(&exe, "old").unwrap();
        let (path, host) = test_host(Config { exe: Some(exe.clone()), ..config() });
        let mut client = TestClient::connect(&path);
        thread::sleep(TICK + Duration::from_millis(300));
        assert!(!host.serving.is_finished(), "exited with no pane, but not replaced");
        let pane = client.spawn("p1");
        // As an update does: a new file at the same path.
        let new = exe.with_extension("new");
        fs::write(&new, "new").unwrap();
        fs::rename(&new, &exe).unwrap();
        thread::sleep(TICK + Duration::from_millis(300));
        assert!(!host.serving.is_finished(), "exited while it still had a pane");
        client.call(Request::Kill { pane });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !host.serving.is_finished() {
            assert!(Instant::now() < deadline, "still running from a replaced executable with no pane");
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!path.exists(), "left its socket behind");
        let _ = fs::remove_file(exe);
    }
    #[test]
    fn a_draining_server_starts_no_pane_and_exits_after_its_last_one() {
        let (path, host) = test_host(config());
        let mut client = TestClient::connect(&path);
        let pane = client.spawn("p1");
        client.call(Request::Drain);
        let id = client.send(Request::Spawn(spawn_request("p2")), &[]);
        client.until("the refusal", |c| c.replies.contains_key(&id));
        assert_eq!(client.replies[&id].1.as_deref(), Some("this server is draining"));
        thread::sleep(TICK + Duration::from_millis(300));
        assert!(!host.serving.is_finished(), "exited while its pane still ran");
        client.type_in(pane, "echo st''ill; exit\r");
        wait_listed(&mut client, pane, "the exit", |p| p.exited);
        thread::sleep(TICK + Duration::from_millis(300));
        assert!(!host.serving.is_finished(), "exited while an app could still read the pane");
        drop(client);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !host.serving.is_finished() {
            assert!(Instant::now() < deadline, "still running, drained and with no pane");
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(*host.stopped.lock().unwrap(), Some(Stopped::Drained));
        assert!(!path.exists(), "left its socket behind");
    }

    #[test]
    fn exits_idle_only_with_no_pane_and_no_client() {
        let (path, host) = test_host(Config { idle: Some(Duration::from_millis(100)), ..config() });
        let client = TestClient::connect(&path);
        thread::sleep(TICK * 2 + Duration::from_millis(300));
        assert!(!host.serving.is_finished(), "exited idle with a client connected");
        drop(client);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !host.serving.is_finished() {
            assert!(Instant::now() < deadline, "still running idle");
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(*host.stopped.lock().unwrap(), Some(Stopped::Idle));
    }

    #[test]
    fn attach_and_list_tell_the_size_the_pane_was_drawn_at() {
        let (path, _host) = test_host(config());
        let mut client = TestClient::connect(&path);
        let pane = client.spawn("p1");
        client.call(Request::Resize { pane, cols: 132, rows: 40 });
        let info = &client.list()[0];
        assert_eq!((info.cols, info.rows), (132, 40));
        let mut other = TestClient::connect(&path);
        let attached: Attached = serde_json::from_value(other.call(Request::Attach { pane, from_seq: None })).unwrap();
        assert_eq!((attached.cols, attached.rows), (132, 40));
        client.call(Request::Kill { pane });
    }

    #[test]
    fn codex_status_is_broadcast_without_a_title_change_and_retained_for_a_new_client() {
        use wings_agents::program_status::CodexStatus;
        let (path, _host) = test_host(config());
        let mut client = TestClient::connect(&path);
        let pane = client.spawn("codex-state");
        client.type_in(pane, "printf '\\033]7501;state=work''ing:app=codex\\007'\r");
        client.until("Codex status", |c| {
            c.events
                .iter()
                .any(|e| matches!(e, HostMsg::Title { pane: p, codex_status: Some(report), .. } if *p == pane && report.state == CodexStatus::Working))
        });
        assert_eq!(client.list()[0].codex_status.map(|s| s.state), Some(CodexStatus::Working));
        let mut again = TestClient::connect(&path);
        assert_eq!(again.list()[0].codex_status.map(|s| s.state), Some(CodexStatus::Working));
        client.type_in(pane, "printf '\\033]7501;state=clear\\007'\r");
        wait_listed(&mut client, pane, "cleared Codex status", |p| p.codex_status.is_none());
        client.call(Request::Kill { pane });
    }
}
