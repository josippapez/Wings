//! How an app reaches a terminal server. Each pane gets a connection of its own, so a pane whose terminal is slow to
//! take its output (a hidden one, a flood) only lags itself: the server never waits for it, and it catches up from the
//! server's ring. Short requests (list, kill, drain, read) go over a `Control` connection.

use std::{
    io::{self, BufReader, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::Path,
    sync::{
        atomic::{AtomicU32, Ordering},
        Mutex,
    },
    time::Duration,
};

use serde_json::Value;

use crate::proto::{self, Attached, Call, Frame, HostMsg, PaneInfo, Request, Spawn, Spawned, CHUNK, MIN_PROTO, PROTO};

/// How long the server has to answer a hello or a request.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What the server said about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Welcome {
    pub proto: u32,
    pub server_version: u32,
}

fn invalid(what: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.into())
}

/// Connects and says hello. Fails when nothing answers at `path`, or the server speaks none of this build's protocols.
fn connect(path: &Path) -> io::Result<(UnixStream, BufReader<UnixStream>, Welcome)> {
    let stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    (&stream).write_all(&proto::ctrl(&Call { id: 0, req: Request::Hello { proto: PROTO, min_proto: MIN_PROTO } })?)?;
    let mut reader = BufReader::with_capacity(64 << 10, stream.try_clone()?);
    let Some(Frame::Ctrl(json)) = proto::read(&mut reader)? else { return Err(invalid("the server didn't answer hello")) };
    let HostMsg::Welcome { proto, host_version, error, .. } = serde_json::from_slice(&json)? else {
        return Err(invalid("the server didn't answer hello"));
    };
    if let Some(error) = error {
        return Err(io::Error::new(io::ErrorKind::Unsupported, error));
    }
    Ok((stream, reader, Welcome { proto, server_version: host_version }))
}

fn send(stream: &UnixStream, id: u32, req: Request, blob: &[u8]) -> io::Result<()> {
    let mut wire = Vec::new();
    for chunk in blob.chunks(CHUNK) {
        wire.extend(proto::blob(id, chunk));
    }
    wire.extend(proto::ctrl(&Call { id, req })?);
    let mut stream = stream;
    stream.write_all(&wire)
}

/// Reads until the reply to `id`, keeping the blobs sent for it and handing every other frame to `other`.
fn reply(reader: &mut BufReader<UnixStream>, id: u32, mut other: impl FnMut(Frame)) -> io::Result<(Value, Vec<u8>)> {
    let mut blob = Vec::new();
    loop {
        match proto::read(reader)? {
            None => return Err(io::ErrorKind::UnexpectedEof.into()),
            Some(Frame::Blob { req, data }) if req == id => blob.extend(data),
            Some(Frame::Ctrl(json)) => match serde_json::from_slice(&json)? {
                HostMsg::Reply { id: got, ok, error } if got == id => {
                    return match error {
                        Some(error) => Err(io::Error::other(error)),
                        None => Ok((ok, blob)),
                    }
                }
                _ => other(Frame::Ctrl(json)),
            },
            Some(frame) => other(frame),
        }
    }
}

/// A connection for requests that wait for their answer.
pub struct Control {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    welcome: Welcome,
    next_id: u32,
}

impl Control {
    pub fn connect(path: &Path) -> io::Result<Control> {
        let (stream, reader, welcome) = connect(path)?;
        Ok(Control { stream, reader, welcome, next_id: 1 })
    }

    pub fn welcome(&self) -> &Welcome {
        &self.welcome
    }

    /// The server's process id, from the socket, so an app can tell which file the server runs from.
    #[cfg(target_os = "macos")]
    pub fn server_pid(&self) -> io::Result<u32> {
        use std::os::fd::AsRawFd;
        let mut pid: libc::pid_t = 0;
        let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        let got = unsafe { libc::getsockopt(self.stream.as_raw_fd(), libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&raw mut pid).cast(), &mut len) };
        if got != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(pid as u32)
    }

    fn call(&mut self, req: Request) -> io::Result<(Value, Vec<u8>)> {
        let id = self.next_id;
        self.next_id += 1;
        send(&self.stream, id, req, &[])?;
        reply(&mut self.reader, id, |_| {})
    }

    /// Every pane the server runs, and those that exited and weren't killed yet.
    pub fn list(&mut self) -> io::Result<Vec<PaneInfo>> {
        Ok(serde_json::from_value(self.call(Request::List)?.0)?)
    }

    /// Ends the pane's program, and the server forgets the pane.
    pub fn kill(&mut self, pane: u32) -> io::Result<()> {
        self.call(Request::Kill { pane }).map(drop)
    }

    /// Tells the server to start no more panes and to exit after its last one.
    pub fn drain(&mut self) -> io::Result<()> {
        self.call(Request::Drain).map(drop)
    }

    /// The output the server kept for the pane.
    pub fn read(&mut self, pane: u32) -> io::Result<Vec<u8>> {
        Ok(self.call(Request::Read { pane })?.1)
    }
}

/// A pane on the server, as the app's terminal uses it from any thread. Its output comes from the `Events` it was
/// opened with.
pub struct Pane {
    number: u32,
    shell_pid: Option<u32>,
    writer: Mutex<UnixStream>,
    /// The pane's foreground process as the server last said, 0 for none.
    fg: AtomicU32,
    next_id: AtomicU32,
}

/// What comes from the pane, in order.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// New output.
    Output(Vec<u8>),
    /// Everything the server kept, to draw on a fresh screen: on attaching, and after falling so far behind that the
    /// server no longer had the bytes in between.
    /// Everything the server kept, for a terminal that starts again, and the sizes the pane had while it printed it
    /// (`output::pieces`).
    Replay(Vec<u8>, crate::output::Sizes),
    /// The pane's program ended.
    Exited,
    /// The connection ended: the server stopped, or `Pane::detach`.
    Closed,
}

impl Pane {
    /// Starts a pane, with `history` (output saved from an earlier shell) as the start of what the server keeps of it.
    pub fn spawn(path: &Path, spawn: Spawn, history: &[u8]) -> io::Result<(Pane, Events)> {
        let (stream, mut reader, _) = connect(path)?;
        send(&stream, 1, Request::Spawn(spawn), history)?;
        // The reply comes before the pane's first output.
        let (ok, _) = reply(&mut reader, 1, |_| {})?;
        let spawned: Spawned = serde_json::from_value(ok)?;
        stream.set_read_timeout(None)?;
        let pane = Pane::new(spawned.pane, spawned.shell_pid, None, stream)?;
        Ok((pane, Events::new(spawned.pane, spawned.seq, reader)))
    }

    /// Attaches to a pane the server runs. The first event is its `Replay`, unless it has no output yet.
    pub fn attach(path: &Path, info: &PaneInfo) -> io::Result<(Pane, Events, Attached)> {
        let (stream, mut reader, _) = connect(path)?;
        send(&stream, 1, Request::Attach { pane: info.pane, from_seq: None }, &[])?;
        let (ok, _) = reply(&mut reader, 1, |_| {})?;
        let attached: Attached = serde_json::from_value(ok)?;
        stream.set_read_timeout(None)?;
        let pane = Pane::new(info.pane, info.shell_pid, info.fg, stream)?;
        let mut events = Events::new(info.pane, 0, reader);
        events.replay = Some((attached.seq, Vec::new(), attached.sizes.clone()));
        Ok((pane, events, attached))
    }

    fn new(number: u32, shell_pid: Option<u32>, fg: Option<u32>, stream: UnixStream) -> io::Result<Pane> {
        Ok(Pane { number, shell_pid, writer: Mutex::new(stream), fg: AtomicU32::new(fg.unwrap_or(0)), next_id: AtomicU32::new(2) })
    }

    /// The server's number for the pane.
    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn shell_pid(&self) -> Option<u32> {
        self.shell_pid
    }

    pub fn foreground_pid(&self) -> Option<u32> {
        Some(self.fg.load(Ordering::Relaxed)).filter(|&pid| pid != 0)
    }

    /// Keystrokes: one write each.
    pub fn write(&self, data: &[u8]) -> io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        for chunk in data.chunks(CHUNK) {
            writer.write_all(&proto::input(self.number, chunk))?;
        }
        Ok(())
    }

    fn request(&self, req: Request) -> io::Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        send(&self.writer.lock().unwrap(), id, req, &[])
    }

    /// Resizes the pane. Its reply is left unread.
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        self.request(Request::Resize { pane: self.number, cols, rows })
    }

    /// Ends the pane's program; the server forgets the pane, and its `Events` end with `Exited`.
    pub fn kill(&self) -> io::Result<()> {
        self.request(Request::Kill { pane: self.number })
    }

    /// Lets go of the pane, which goes on running in the server. Its `Events` end with `Closed`.
    pub fn detach(&self) {
        let _ = self.writer.lock().unwrap().shutdown(Shutdown::Both);
    }
}

/// The pane's output and what happens to it, read on a thread of its own.
pub struct Events {
    number: u32,
    reader: BufReader<UnixStream>,
    /// Where the output received so far ends.
    seq: u64,
    /// A replay being received: where it ends, and its bytes so far.
    replay: Option<(u64, Vec<u8>, crate::output::Sizes)>,
    /// The request that attaches again after a lag.
    reattach: Option<u32>,
}

impl Events {
    fn new(number: u32, seq: u64, reader: BufReader<UnixStream>) -> Events {
        Events { number, reader, seq, replay: None, reattach: None }
    }

    /// The next event. Blocks until there is one. After `Exited` or `Closed` there are no more.
    pub fn next(&mut self, pane: &Pane) -> Event {
        loop {
            let frame = match proto::read(&mut self.reader) {
                Ok(Some(frame)) => frame,
                Ok(None) | Err(_) => return Event::Closed,
            };
            match frame {
                Frame::Out { pane: number, seq_end, data } if number == self.number => {
                    if let Some((end, bytes, _)) = &mut self.replay {
                        bytes.extend_from_slice(&data);
                        if seq_end >= *end {
                            self.seq = seq_end;
                            let (_, bytes, sizes) = self.replay.take().expect("a replay");
                            return Event::Replay(bytes, sizes);
                        }
                        continue;
                    }
                    // Only output past what was received counts: a resumed stream starts exactly there.
                    let new = seq_end.saturating_sub(self.seq) as usize;
                    if new == 0 {
                        continue;
                    }
                    self.seq = seq_end;
                    let data = if new < data.len() { data[data.len() - new..].to_vec() } else { data };
                    return Event::Output(data);
                }
                Frame::Ctrl(json) => {
                    let Ok(msg) = serde_json::from_slice::<HostMsg>(&json) else { continue };
                    match msg {
                        HostMsg::Fg { pane: number, pid } if number == self.number => {
                            pane.fg.store(pid.unwrap_or(0), Ordering::Relaxed);
                        }
                        HostMsg::Exited { pane: number } if number == self.number => return Event::Exited,
                        HostMsg::Lagged { pane: number } if number == self.number => {
                            let id = pane.next_id.fetch_add(1, Ordering::Relaxed);
                            let attach = Request::Attach { pane: self.number, from_seq: Some(self.seq) };
                            if send(&pane.writer.lock().unwrap(), id, attach, &[]).is_err() {
                                return Event::Closed;
                            }
                            self.reattach = Some(id);
                        }
                        HostMsg::Reply { id, ok, error } if Some(id) == self.reattach => {
                            self.reattach = None;
                            let Some(attached) = error.is_none().then(|| serde_json::from_value::<Attached>(ok).ok()).flatten() else {
                                return Event::Closed;
                            };
                            // A reset means the bytes in between are gone: the terminal starts again from all the
                            // server kept. Otherwise the stream just goes on from `seq`.
                            if attached.reset {
                                self.replay = Some((attached.seq, Vec::new(), attached.sizes));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{test_host, Config, TestHost};
    use std::{path::PathBuf, sync::mpsc, thread, time::Instant};
    use wings_terminal::shell::pane_env;

    fn spawn_request() -> Spawn {
        Spawn {
            space_id: "s".into(),
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 24,
            history_id: Some("h1".into()),
            env: pane_env(None).into_iter().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect(),
        }
    }

    /// Reads a pane's events on a thread, as the app's terminal does, into a channel.
    fn read_events(pane: std::sync::Arc<Pane>, mut events: Events) -> mpsc::Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || loop {
            let event = events.next(&pane);
            let end = matches!(event, Event::Exited | Event::Closed);
            if tx.send(event).is_err() || end {
                return;
            }
        });
        rx
    }

    /// The text of every output and replay event until `text` shows up; panics on the end or a timeout.
    fn until(rx: &mpsc::Receiver<Event>, text: &str) -> (String, Vec<Event>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut seen, mut events) = (String::new(), Vec::new());
        while !seen.contains(text) {
            let event =
                rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap_or_else(|_| panic!("timed out waiting for {text:?} in {seen:?}"));
            match &event {
                Event::Output(bytes) | Event::Replay(bytes, _) => seen.push_str(&String::from_utf8_lossy(bytes)),
                end => panic!("{end:?} before {text:?}"),
            }
            events.push(event);
        }
        (seen, events)
    }

    fn host(config: Config) -> (PathBuf, TestHost) {
        test_host(config)
    }

    #[test]
    fn a_pane_reattached_after_the_app_lets_go_replays_what_it_printed_meanwhile() {
        let (path, _host) = host(Config::default());
        let (pane, events) = Pane::spawn(&path, spawn_request(), b"").unwrap();
        let pane = std::sync::Arc::new(pane);
        let rx = read_events(pane.clone(), events);
        pane.write(b"echo be''fore\r").unwrap();
        until(&rx, "before");
        // The app quits: the pane goes on, and prints while nobody is attached.
        pane.write(b"sleep 0.3; echo mean''while\r").unwrap();
        pane.detach();
        let deadline = Instant::now() + Duration::from_secs(10);
        while rx.recv_timeout(Duration::from_secs(10)).unwrap() != Event::Closed {
            assert!(Instant::now() < deadline);
        }
        thread::sleep(Duration::from_millis(600));
        let info = Control::connect(&path).unwrap().list().unwrap().remove(0);
        assert!(!info.exited);
        let (again, events, attached) = Pane::attach(&path, &info).unwrap();
        assert!(attached.reset);
        let again = std::sync::Arc::new(again);
        let rx = read_events(again.clone(), events);
        let (seen, events) = until(&rx, "meanwhile");
        assert!(matches!(events[0], Event::Replay(..)), "starts with a replay: {events:?}");
        assert!(seen.contains("before"));
        again.write(b"echo af''ter\r").unwrap();
        until(&rx, "after");
        again.kill().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("no exit") {
                Event::Exited => break,
                Event::Output(_) => {}
                other => panic!("{other:?}"),
            }
        }
        assert!(Control::connect(&path).unwrap().list().unwrap().is_empty(), "killed panes are forgotten");
    }

    #[test]
    fn a_new_pane_with_no_output_yet_attaches_with_an_empty_replay() {
        let (path, _host) = host(Config::default());
        let mut command = Spawn { cwd: std::env::temp_dir(), ..spawn_request() };
        // A shell that prints nothing until told to.
        command.env.retain(|(k, _)| k != "SHELL");
        command.env.push(("SHELL".into(), "/bin/cat".into()));
        let (pane, _events) = Pane::spawn(&path, command, b"").unwrap();
        let info = Control::connect(&path).unwrap().list().unwrap().remove(0);
        if info.seq == 0 {
            let (attached, mut events, _) = Pane::attach(&path, &info).unwrap();
            assert_eq!(events.next(&attached), Event::Replay(Vec::new(), vec![(0, 80, 24)]));
        }
        pane.kill().unwrap();
    }

    #[test]
    fn a_lagging_pane_catches_up_with_every_byte_once() {
        let (path, _host) = host(Config { client_queue: 64 << 10, ..Config::default() });
        let (pane, mut events) = Pane::spawn(&path, spawn_request(), b"").unwrap();
        pane.write(b"echo re''ady\r").unwrap();
        let mut got = Vec::new();
        while !String::from_utf8_lossy(&got).contains("ready") {
            match events.next(&pane) {
                Event::Output(bytes) => got.extend(bytes),
                other => panic!("{other:?}"),
            }
        }
        pane.write(b"seq 1 120000; echo do''ne\r").unwrap();
        // Not reading while 800 KB arrive: the socket fills, then the server's queue, and the pane lags.
        thread::sleep(Duration::from_millis(1500));
        let mut replays = 0;
        while !String::from_utf8_lossy(&got).contains("done") {
            match events.next(&pane) {
                Event::Output(bytes) => got.extend(bytes),
                Event::Replay(bytes, _) => {
                    replays += 1;
                    got = bytes;
                }
                other => panic!("{other:?}"),
            }
        }
        let text = String::from_utf8_lossy(&got).replace("\r\r\n", "\r\n");
        // Every number once and in order, from wherever the kept output starts.
        let numbers: Vec<u64> = text.lines().filter_map(|l| l.trim_end_matches('\r').parse().ok()).collect();
        assert!(numbers.windows(2).all(|w| w[1] == w[0] + 1), "a gap or a repeat after {replays} replays");
        assert_eq!(numbers.last(), Some(&120000));
        pane.kill().unwrap();
    }

    #[test]
    fn nothing_listening_is_an_error() {
        assert_eq!(connect(Path::new("/nonexistent/wings.sock")).unwrap_err().kind(), io::ErrorKind::NotFound);
    }
}
