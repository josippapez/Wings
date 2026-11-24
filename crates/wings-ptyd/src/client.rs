//! How an app reaches a terminal server. Each pane gets a connection of its own, so a pane whose terminal is slow to
//! take its output (a hidden one, a flood) only lags itself: the server never waits for it, and it catches up from the
//! server's ring. Short requests (list, kill, drain, read) go over a `Control` connection. So does each job (`Job`).

use std::{
    collections::HashMap,
    io::{self, BufReader, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use serde_json::Value;

use crate::proto::{
    self, Attached, Call, Frame, HostMsg, JobAttached, JobExit, JobInfo, JobSpawn, JobStarted, PaneInfo, Request, Spawn, Spawned, CHUNK, JOBS,
    MIN_PROTO, PROTO,
};

/// How long the server has to answer a hello or a request.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What the server said about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Welcome {
    pub proto: u32,
    pub server_version: u32,
    pub features: Vec<String>,
}

impl Welcome {
    /// Whether the server runs jobs.
    pub fn runs_jobs(&self) -> bool {
        self.features.iter().any(|feature| feature == JOBS)
    }
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
    let HostMsg::Welcome { proto, host_version, error, features } = serde_json::from_slice(&json)? else {
        return Err(invalid("the server didn't answer hello"));
    };
    if let Some(error) = error {
        return Err(io::Error::new(io::ErrorKind::Unsupported, error));
    }
    Ok((stream, reader, Welcome { proto, server_version: host_version, features }))
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

    /// Every job the server runs, and those that ended that no app heard end.
    pub fn jobs(&mut self) -> io::Result<Vec<JobInfo>> {
        Ok(serde_json::from_value(self.call(Request::JobList)?.0)?)
    }

    /// What the app that heard the job last kept with it (`Job::note`).
    pub fn job_note(&mut self, job: u32) -> io::Result<Vec<u8>> {
        Ok(self.call(Request::JobReadNote { job })?.1)
    }

    /// Stops the job, or forgets it once it ended.
    pub fn stop_job(&mut self, job: u32) -> io::Result<()> {
        self.call(Request::JobStop { job }).map(drop)
    }
}

/// Hears how a write to a job went.
pub type Written = Box<dyn FnOnce(Result<(), String>) + Send>;

const INPUT_CLOSED: &str = "The program's input is closed";

/// A job on the server, as an app writes to it from any thread. What it prints comes from the `JobEvents` it was
/// opened with, each on a connection of its own.
pub struct Job {
    number: u32,
    pid: Option<u32>,
    writer: Mutex<UnixStream>,
    next_id: AtomicU32,
    /// The writes the server hasn't said went yet, by request. None once nothing hears the server any more.
    written: Arc<Mutex<Option<HashMap<u32, Written>>>>,
    stopping: AtomicBool,
}

/// What comes of a job, in order.
#[derive(Debug, PartialEq)]
pub enum JobEvent {
    /// The line numbered `n`, without its line ending.
    Line(u64, Vec<u8>),
    /// That many lines before the next were let go of for room.
    Dropped(u64),
    /// The program ended, after its last line.
    Ended(JobExit),
    /// The connection ended: the server stopped, or `Job::detach`.
    Closed,
}

impl Job {
    /// Starts a job and hears it. `input` is what it reads first. Fails as `Unsupported` on a server that runs no jobs.
    pub fn start(path: &Path, spawn: JobSpawn, input: &[u8]) -> io::Result<(Job, JobEvents)> {
        let (stream, mut reader, welcome) = connect(path)?;
        if !welcome.runs_jobs() {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "this terminal server runs no jobs"));
        }
        send(&stream, 1, Request::JobStart(spawn), input)?;
        // The reply comes before the job's first line.
        let (ok, _) = reply(&mut reader, 1, |_| {})?;
        let started: JobStarted = serde_json::from_value(ok)?;
        stream.set_read_timeout(None)?;
        Ok(Job::open(started.job, started.pid, stream, reader))
    }

    /// Hears a job the server runs, from the line after its last note; another app that heard it no longer does.
    pub fn attach(path: &Path, job: u32, pid: Option<u32>) -> io::Result<(Job, JobEvents, JobAttached)> {
        let (stream, mut reader, _) = connect(path)?;
        send(&stream, 1, Request::JobAttach { job }, &[])?;
        let (ok, _) = reply(&mut reader, 1, |_| {})?;
        let attached: JobAttached = serde_json::from_value(ok)?;
        stream.set_read_timeout(None)?;
        let (job, events) = Job::open(job, pid, stream, reader);
        Ok((job, events, attached))
    }

    fn open(number: u32, pid: Option<u32>, stream: UnixStream, reader: BufReader<UnixStream>) -> (Job, JobEvents) {
        let written = Arc::new(Mutex::new(Some(HashMap::new())));
        let job = Job { number, pid, writer: Mutex::new(stream), next_id: AtomicU32::new(2), written: written.clone(), stopping: AtomicBool::new(false) };
        (job, JobEvents { number, reader, piece: None, written })
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Whether it was told to stop.
    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::Relaxed)
    }

    fn request(&self, req: Request, blob: &[u8]) -> io::Result<u32> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        send(&self.writer.lock().unwrap(), id, req, blob)?;
        Ok(id)
    }

    /// Writes `data` to the program's input, after the writes before, and tells `done` how that went once the server
    /// says. Never waits for the program.
    pub fn write(&self, data: &[u8], done: impl FnOnce(Result<(), String>) + Send + 'static) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let refused = match self.written.lock().unwrap().as_mut() {
            Some(written) => {
                written.insert(id, Box::new(done));
                None
            }
            None => Some(done),
        };
        if let Some(done) = refused {
            return done(Err(INPUT_CLOSED.into()));
        }
        let sent = send(&self.writer.lock().unwrap(), id, Request::JobWrite { job: self.number }, data);
        if let Err(error) = sent {
            let done = self.written.lock().unwrap().as_mut().and_then(|written| written.remove(&id));
            if let Some(done) = done {
                done(Err(format!("{INPUT_CLOSED}: {error}")));
            }
        }
    }

    /// Keeps `note` with the job for the next app that hears it. With `since`, the lines up to that one need not be kept
    /// for it; with `owner`, the job is for that from now on.
    pub fn note(&self, since: Option<u64>, owner: Option<Value>, note: &[u8]) -> io::Result<()> {
        self.request(Request::JobNote { job: self.number, since, owner }, note).map(drop)
    }

    /// Closes the program's input, which tells a program like Claude to finish, and the server kills it if it still runs a
    /// few seconds later.
    pub fn stop(&self) -> io::Result<()> {
        self.stopping.store(true, Ordering::Relaxed);
        self.request(Request::JobStop { job: self.number }, &[]).map(drop)
    }

    /// Lets go of the job, which goes on running in the server. Its `JobEvents` end with `Closed`.
    pub fn detach(&self) {
        let _ = self.writer.lock().unwrap().shutdown(Shutdown::Both);
    }
}

/// What a job prints and how it ends, read on a thread of its own.
pub struct JobEvents {
    number: u32,
    reader: BufReader<UnixStream>,
    /// A line whose pieces are still coming.
    piece: Option<(u64, Vec<u8>)>,
    written: Arc<Mutex<Option<HashMap<u32, Written>>>>,
}

impl JobEvents {
    /// The next event. Blocks until there is one. After `Ended` or `Closed` there are no more.
    pub fn next(&mut self) -> JobEvent {
        loop {
            let frame = match proto::read(&mut self.reader) {
                Ok(Some(frame)) => frame,
                Ok(None) | Err(_) => return self.closed(),
            };
            match frame {
                Frame::Line { job, n, more, data } if job == self.number => {
                    let line = match self.piece.take() {
                        Some((at, mut line)) if at == n => {
                            line.extend_from_slice(&data);
                            line
                        }
                        _ => data,
                    };
                    if more {
                        self.piece = Some((n, line));
                        continue;
                    }
                    return JobEvent::Line(n, line);
                }
                Frame::Ctrl(json) => match serde_json::from_slice::<HostMsg>(&json) {
                    Ok(HostMsg::Reply { id, error, .. }) => {
                        let done = self.written.lock().unwrap().as_mut().and_then(|written| written.remove(&id));
                        if let Some(done) = done {
                            done(error.map_or(Ok(()), Err));
                        }
                    }
                    Ok(HostMsg::JobEnded { job, exit }) if job == self.number => {
                        self.fail_writes("The program ended");
                        return JobEvent::Ended(exit);
                    }
                    Ok(HostMsg::JobDropped { job, lines }) if job == self.number => return JobEvent::Dropped(lines),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn closed(&mut self) -> JobEvent {
        self.fail_writes(INPUT_CLOSED);
        JobEvent::Closed
    }

    /// Answers the writes the server will never say went, and those to come at once.
    fn fail_writes(&self, why: &str) {
        let waiting = self.written.lock().unwrap().take().unwrap_or_default();
        for done in waiting.into_values() {
            done(Err(why.to_owned()));
        }
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
            replay_bytes: None,
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

    /// A pane asked to keep more than 1 MB keeps it: a 3 MB history comes back whole.
    #[test]
    fn a_pane_spawned_with_a_bigger_replay_keeps_more_than_a_megabyte() {
        let (path, _host) = host(Config::default());
        let mut command = Spawn { replay_bytes: Some(4 << 20), ..spawn_request() };
        command.env.retain(|(k, _)| k != "SHELL");
        command.env.push(("SHELL".into(), "/bin/cat".into()));
        let history = b"a line of saved output\n".repeat(130_000);
        let (pane, _events) = Pane::spawn(&path, command, &history).unwrap();
        let info = Control::connect(&path).unwrap().list().unwrap().remove(0);
        let (attached, mut events, _) = Pane::attach(&path, &info).unwrap();
        let mut replay = Vec::new();
        if let Event::Replay(bytes, _) = events.next(&attached) {
            replay = bytes;
        }
        assert!(history.len() > 2 << 20);
        assert!(replay.len() >= history.len(), "{} of {} bytes kept", replay.len(), history.len());
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

    // Jobs

    /// A job that runs `script` with `sh`, in the test's environment.
    fn job_spawn(script: &str) -> JobSpawn {
        JobSpawn {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: std::env::temp_dir(),
            env: std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect(),
            owner: serde_json::json!({ "plugin": "probe", "tab": "t1" }),
        }
    }

    /// Reads a job's events on a thread, as the app does, into a channel.
    fn read_job(mut events: JobEvents) -> mpsc::Receiver<JobEvent> {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || loop {
            let event = events.next();
            let end = matches!(event, JobEvent::Ended(_) | JobEvent::Closed);
            if tx.send(event).is_err() || end {
                return;
            }
        });
        rx
    }

    fn next_event(rx: &mpsc::Receiver<JobEvent>) -> JobEvent {
        rx.recv_timeout(Duration::from_secs(10)).expect("the job said nothing more")
    }

    fn line(n: u64, text: &str) -> JobEvent {
        JobEvent::Line(n, text.as_bytes().to_vec())
    }

    /// Writes `text` and waits for the server to say it went.
    fn write_job(job: &Job, text: &[u8]) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        job.write(text, move |result| tx.send(result).unwrap());
        rx.recv_timeout(Duration::from_secs(10)).unwrap()
    }

    /// The server's jobs once `done` holds of them.
    fn jobs_once(path: &Path, what: &str, done: impl Fn(&[JobInfo]) -> bool) -> Vec<JobInfo> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let jobs = Control::connect(path).unwrap().jobs().unwrap();
            if done(&jobs) {
                return jobs;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}: {jobs:?}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_for_file(file: &Path, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !file.exists() {
            assert!(Instant::now() < deadline, "{what}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_job_runs_in_a_process_group_of_its_own_so_a_signal_to_its_group_spares_the_server() {
        let (path, _host) = host(Config::default());
        let (job, events) = Job::start(&path, job_spawn("ps -o pgid= -p $$ | tr -d ' '; cat"), b"").unwrap();
        let rx = read_job(events);
        let group = match next_event(&rx) {
            JobEvent::Line(_, text) => String::from_utf8(text).unwrap(),
            other => panic!("{other:?}"),
        };
        assert_eq!(group, job.pid().unwrap().to_string(), "the job leads a group of its own");
        assert_ne!(group, unsafe { libc::getpgrp() }.to_string(), "not the server's");
        job.stop().unwrap();
        assert!(matches!(next_event(&rx), JobEvent::Ended(_)));
    }

    #[test]
    fn a_job_reads_what_it_starts_with_and_each_write_and_prints_lines_bigger_than_a_frame() {
        let (path, _host) = host(Config::default());
        let (job, events) = Job::start(&path, job_spawn("read a; echo \"got $a\"; cat"), b"hello\n").unwrap();
        let rx = read_job(events);
        assert_eq!(next_event(&rx), line(1, "got hello"));
        assert_eq!(write_job(&job, b"{\"type\":\"user\"}\r\n"), Ok(()));
        assert_eq!(next_event(&rx), line(2, "{\"type\":\"user\"}"), "without its line ending");
        // Longer than a frame, so it comes in pieces, and no line discipline cuts it.
        let long = "x".repeat(3 * CHUNK + 5);
        assert_eq!(write_job(&job, format!("{long}\n").as_bytes()), Ok(()));
        assert_eq!(next_event(&rx), line(3, &long));
        job.stop().unwrap();
        assert_eq!(next_event(&rx), JobEvent::Ended(JobExit { code: Some(0), stderr: String::new(), error: None }));
        assert!(write_job(&job, b"late\n").is_err());
    }

    #[test]
    fn a_job_runs_on_while_no_app_hears_it_and_the_next_app_hears_what_it_missed_then_the_rest() {
        let (path, _host) = host(Config::default());
        let flag = std::env::temp_dir().join(format!("wptyd-job-flag-{}", std::process::id()));
        let _ = std::fs::remove_file(&flag);
        let script = format!("read a; echo \"$a\"; while [ ! -f '{}' ]; do sleep 0.05; done; echo during; cat", flag.display());
        let (job, events) = Job::start(&path, job_spawn(&script), b"first\n").unwrap();
        let rx = read_job(events);
        assert_eq!(next_event(&rx), line(1, "first"));
        // The app has what it needs of line 1, and keeps a note for the next one.
        job.note(Some(1), None, b"{\"turn\":1}").unwrap();
        // The app quits: the job goes on, and prints while nobody hears it.
        job.detach();
        assert_eq!(next_event(&rx), JobEvent::Closed);
        std::fs::write(&flag, "").unwrap();
        let listed = jobs_once(&path, "the line printed meanwhile", |jobs| jobs.len() == 1 && jobs[0].last == 2 && !jobs[0].attached);
        assert_eq!((listed[0].since, listed[0].ended, &listed[0].owner), (1, false, &serde_json::json!({ "plugin": "probe", "tab": "t1" })));
        assert_eq!(Control::connect(&path).unwrap().job_note(listed[0].job).unwrap(), b"{\"turn\":1}");
        let (again, events, attached) = Job::attach(&path, listed[0].job, listed[0].pid).unwrap();
        assert_eq!((attached.since, attached.last), (1, 2));
        let rx = read_job(events);
        // What it missed, and not what the app before had.
        assert_eq!(next_event(&rx), line(2, "during"));
        assert_eq!(write_job(&again, b"again\n"), Ok(()));
        assert_eq!(next_event(&rx), line(3, "again"));
        again.stop().unwrap();
        assert!(matches!(next_event(&rx), JobEvent::Ended(_)));
        jobs_once(&path, "the job to be forgotten once its end was heard", <[JobInfo]>::is_empty);
        std::fs::remove_file(&flag).unwrap();
    }

    #[test]
    fn a_job_no_app_hears_keeps_its_server_and_has_its_input_closed_after_a_while() {
        let marker = std::env::temp_dir().join(format!("wptyd-job-orphan-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let (path, host) = host(Config { idle: Some(Duration::from_millis(100)), job_orphaned: Duration::from_millis(1500), ..Config::default() });
        let (job, _events) = Job::start(&path, job_spawn(&format!("cat; touch '{}'", marker.display())), b"").unwrap();
        job.detach();
        thread::sleep(Duration::from_millis(1200));
        assert!(!host.serving.is_finished(), "exited idle while a job ran");
        assert!(!marker.exists(), "its input closed before the wait was over");
        // Its input closes, so `cat` ends and the script goes on to its end.
        wait_for_file(&marker, "the job no app heard kept running");
        std::fs::remove_file(&marker).unwrap();
    }

    #[test]
    fn a_draining_server_starts_no_job() {
        let (path, _host) = host(Config::default());
        Control::connect(&path).unwrap().drain().unwrap();
        let refused = Job::start(&path, job_spawn("cat"), b"").err().expect("a job started on a draining server");
        assert_eq!(refused.to_string(), "this server is draining");
    }
}
