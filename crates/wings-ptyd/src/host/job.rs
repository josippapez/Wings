//! Jobs: programs the host runs on pipes rather than a terminal, so that a program an app talks to, like a plugin's
//! headless Claude Code, goes on while the app restarts. A pipe has no line discipline to echo or cut the long JSON lines
//! such a program reads and prints.
//!
//! A job's output is lines. The host keeps those after the last note the app sent (`note`), up to `KEPT_BYTES`, and the
//! note itself, so the next app that attaches hears what it is missing, then each new line, then how the program ended.
//! One client hears a job at a time, and the job's next line waits while that client is behind, as the program would
//! wait on a pipe the app read itself. A job no client has heard for `Config::job_orphaned` has its input closed, so it
//! finishes what it does and ends; one that ended and nobody heard is then forgotten.

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
    sync::{mpsc, Arc, Condvar, Mutex, Weak},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

use super::{ctrl_frame, reply, Client, Host};
use crate::proto::{self, HostMsg, JobExit, JobInfo, JobSpawn, CHUNK};

/// The longest line a job may print: one stream-json line holds a whole message. A longer one ends the job, as it ends
/// a plugin's program in the app.
pub const LINE_LIMIT: usize = 8 << 20;
pub const LINE_TOO_LONG: &str = "The program printed a line longer than 8 MB";
/// How much of what a job printed since its last note is kept. Past it, the oldest lines go.
pub const KEPT_BYTES: usize = 16 << 20;
/// The most a job's note holds.
pub const NOTE_MAX: usize = 1 << 20;
/// How much of what the program printed to stderr is kept, from the end.
const STDERR_KEPT: usize = 16 * 1024;
/// How long a stopped job has to end once its input is closed, before it is killed. Claude ends at once.
const STOP_GRACE: Duration = Duration::from_secs(3);
/// While this much waits for the client that hears a job, its next line waits.
const QUEUED: usize = 1 << 20;

/// A write for the job's input, and the request to answer once it went.
struct Input {
    data: Vec<u8>,
    reply: Option<(Arc<Client>, u32)>,
}

pub(super) struct HostJob {
    pub(super) id: u32,
    pid: Option<u32>,
    child: Mutex<Child>,
    /// Where writes wait for the writer, which closes the program's input once this is gone.
    input: Mutex<Option<mpsc::Sender<Input>>>,
    out: Mutex<JobOut>,
    /// Woken when a line comes, the job ends, or the client that hears it changes.
    changed: Condvar,
}

struct JobOut {
    owner: Value,
    /// The lines past `since`, oldest first, each with its number.
    lines: VecDeque<(u64, Vec<u8>)>,
    bytes: usize,
    last: u64,
    since: u64,
    note: Vec<u8>,
    /// The client that hears it, and the number of the next line it is sent.
    client: Option<(Arc<Client>, u64)>,
    /// Since when no client has heard it.
    alone: Option<Instant>,
    stopping: bool,
    ended: Option<JobExit>,
    /// Gone from the host: its sender stops.
    gone: bool,
}

impl HostJob {
    /// Starts the program `spawn` names, with `input` as what it reads first. Nobody hears it until `attach`.
    pub(super) fn start(host: &Arc<Host>, id: u32, spawn: JobSpawn, input: Vec<u8>) -> std::io::Result<Arc<HostJob>> {
        let mut child = Command::new(&spawn.program)
            .args(&spawn.args)
            .current_dir(&spawn.cwd)
            .env_clear()
            .envs(spawn.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (stdin, stdout, stderr) = (child.stdin.take().unwrap(), child.stdout.take().unwrap(), child.stderr.take().unwrap());
        let (sender, writes) = mpsc::channel();
        if !input.is_empty() {
            let _ = sender.send(Input { data: input, reply: None });
        }
        let job = Arc::new(HostJob {
            id,
            pid: Some(child.id()),
            child: Mutex::new(child),
            input: Mutex::new(Some(sender)),
            out: Mutex::new(JobOut {
                owner: spawn.owner,
                lines: VecDeque::new(),
                bytes: 0,
                last: 0,
                since: 0,
                note: Vec::new(),
                client: None,
                alone: Some(Instant::now()),
                stopping: false,
                ended: None,
                gone: false,
            }),
            changed: Condvar::new(),
        });
        thread::Builder::new().name("ptyd-job-in".into()).spawn(move || write_input(stdin, writes))?;
        let errors = thread::Builder::new().name("ptyd-job-err".into()).spawn(move || tail(stderr))?;
        let reading = job.clone();
        thread::Builder::new().name("ptyd-job-out".into()).spawn(move || reading.read(stdout, errors))?;
        let (sending, host) = (job.clone(), Arc::downgrade(host));
        thread::Builder::new().name("ptyd-job-send".into()).spawn(move || sending.send_lines(host))?;
        Ok(job)
    }

    /// Streams the job to `client` from the line after the last note, replying to `id`, if any, first. Another client it
    /// streamed to hears no more of it.
    pub(super) fn attach(&self, client: &Arc<Client>, id: Option<u32>) {
        let mut out = self.out.lock().unwrap();
        // Queued before the sender can queue a line for this client, so the reply comes first.
        if let Some(id) = id {
            let attached = proto::JobAttached { job: self.id, since: out.since, last: out.last };
            reply(client, id, Ok(serde_json::to_value(attached).expect("serializes")));
        }
        out.client = Some((client.clone(), out.since + 1));
        out.alone = None;
        self.changed.notify_all();
    }

    /// The client that heard it is gone.
    pub(super) fn left_by(&self, client: &Arc<Client>) {
        let mut out = self.out.lock().unwrap();
        if out.client.as_ref().is_some_and(|(heard, _)| Arc::ptr_eq(heard, client)) {
            out.client = None;
            out.alone = Some(Instant::now());
        }
    }

    pub(super) fn write(&self, data: Vec<u8>, client: &Arc<Client>, id: u32) {
        let refused = match &*self.input.lock().unwrap() {
            Some(input) => input.send(Input { data, reply: Some((client.clone(), id)) }).is_err(),
            None => true,
        };
        if refused {
            reply(client, id, Err("The program's input is closed".into()));
        }
    }

    pub(super) fn note(&self, note: Vec<u8>, since: Option<u64>, owner: Option<Value>) -> Result<(), String> {
        if note.len() > NOTE_MAX {
            return Err("A job's note is limited to 1 MB".into());
        }
        let mut out = self.out.lock().unwrap();
        out.note = note;
        if let Some(owner) = owner {
            out.owner = owner;
        }
        if let Some(since) = since {
            out.since = out.since.max(since.min(out.last));
            while out.lines.front().is_some_and(|(n, _)| *n <= out.since) {
                let (_, line) = out.lines.pop_front().expect("a line");
                out.bytes -= line.len();
            }
        }
        Ok(())
    }

    pub(super) fn note_bytes(&self) -> Vec<u8> {
        self.out.lock().unwrap().note.clone()
    }

    /// Closes its input once the writes before have gone, and kills it if it still runs `STOP_GRACE` later.
    pub(super) fn stop(self: &Arc<Self>) {
        self.out.lock().unwrap().stopping = true;
        self.input.lock().unwrap().take();
        let job = self.clone();
        thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < STOP_GRACE {
                if job.out.lock().unwrap().ended.is_some() {
                    return;
                }
                thread::sleep(Duration::from_millis(50));
            }
            let _ = job.child.lock().unwrap().kill();
        });
    }

    pub(super) fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub(super) fn ended(&self) -> bool {
        self.out.lock().unwrap().ended.is_some()
    }

    /// Whether a client hears it.
    pub(super) fn heard(&self) -> bool {
        self.out.lock().unwrap().client.as_ref().is_some_and(|(client, _)| !client.is_closed())
    }

    /// What the host does with a job nobody has heard for `orphaned`: its input closes, so the program finishes what it
    /// does and ends. True for one that ended, which may go.
    pub(super) fn tend(&self, orphaned: Duration) -> bool {
        let mut out = self.out.lock().unwrap();
        if out.client.as_ref().is_some_and(|(client, _)| client.is_closed()) {
            out.client = None;
            out.alone = Some(Instant::now());
        }
        if !out.alone.is_some_and(|since| since.elapsed() >= orphaned) {
            return false;
        }
        if out.ended.is_some() {
            return true;
        }
        drop(out);
        self.input.lock().unwrap().take();
        false
    }

    /// Gone from the host: its sender stops.
    pub(super) fn forget(&self) {
        self.out.lock().unwrap().gone = true;
        self.changed.notify_all();
    }

    pub(super) fn info(&self) -> JobInfo {
        let out = self.out.lock().unwrap();
        JobInfo {
            job: self.id,
            owner: out.owner.clone(),
            pid: self.pid,
            since: out.since,
            last: out.last,
            attached: out.client.as_ref().is_some_and(|(client, _)| !client.is_closed()),
            stopping: out.stopping,
            ended: out.ended.is_some(),
        }
    }

    /// Reads the program's lines until its output ends, then waits for it to exit.
    fn read(&self, stdout: ChildStdout, errors: thread::JoinHandle<String>) {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        let mut too_long = false;
        loop {
            line.clear();
            // One byte past the limit is read to tell a line that is too long.
            match (&mut reader).take(LINE_LIMIT as u64 + 1).read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) if line.len() > LINE_LIMIT => {
                    too_long = true;
                    let _ = self.child.lock().unwrap().kill();
                    break;
                }
                Ok(_) => {}
            }
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            self.push(std::mem::take(&mut line));
        }
        drop(reader);
        // Polled, not waited on, so that `stop` can still reach the child to kill it.
        let status = loop {
            match self.child.lock().unwrap().try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {}
                Err(error) => break Err(error.to_string()),
            }
            thread::sleep(Duration::from_millis(10));
        };
        self.input.lock().unwrap().take();
        let stderr = errors.join().unwrap_or_default();
        let exit = match status {
            _ if too_long => JobExit { code: None, stderr, error: Some(LINE_TOO_LONG.into()) },
            Ok(status) => JobExit { code: status.code(), stderr, error: None },
            Err(error) => JobExit { code: None, stderr, error: Some(error) },
        };
        self.out.lock().unwrap().ended = Some(exit);
        self.changed.notify_all();
    }

    fn push(&self, line: Vec<u8>) {
        let mut out = self.out.lock().unwrap();
        out.last += 1;
        let n = out.last;
        out.bytes += line.len();
        out.lines.push_back((n, line));
        while out.bytes > KEPT_BYTES {
            let Some((_, old)) = out.lines.pop_front() else { break };
            out.bytes -= old.len();
        }
        self.changed.notify_all();
    }

    /// Sends the client that hears the job each line it hasn't had, then how the job ended, after which the job goes.
    fn send_lines(&self, host: Weak<Host>) {
        loop {
            let (client, next) = {
                let mut out = self.out.lock().unwrap();
                loop {
                    if out.gone {
                        return;
                    }
                    if let Some(next) = out.next() {
                        break next;
                    }
                    out = self.changed.wait(out).unwrap();
                }
            };
            match next {
                Next::Dropped(lines) => {
                    if let Some(frame) = ctrl_frame(&HostMsg::JobDropped { job: self.id, lines }) {
                        client.send(None, frame);
                    }
                }
                Next::Line(n, line) => {
                    self.room(&client);
                    let mut pieces = line.chunks(CHUNK).peekable();
                    if pieces.peek().is_none() {
                        client.send(None, Arc::new(proto::line(self.id, n, false, &[])));
                    }
                    while let Some(piece) = pieces.next() {
                        client.send(None, Arc::new(proto::line(self.id, n, pieces.peek().is_some(), piece)));
                    }
                }
                Next::End(exit) => {
                    if let Some(frame) = ctrl_frame(&HostMsg::JobEnded { job: self.id, exit }) {
                        client.send(None, frame);
                    }
                    if let Some(host) = host.upgrade() {
                        host.jobs.lock().unwrap().remove(&self.id);
                    }
                    return;
                }
            }
        }
    }

    /// Waits while `client` is behind, unless another client hears the job by then.
    fn room(&self, client: &Arc<Client>) {
        while !client.wait_room(QUEUED, Duration::from_millis(250)) {
            let still = self.out.lock().unwrap().client.as_ref().is_some_and(|(heard, _)| Arc::ptr_eq(heard, client));
            if !still {
                return;
            }
        }
    }
}

enum Next {
    Dropped(u64),
    Line(u64, Vec<u8>),
    End(JobExit),
}

impl JobOut {
    /// What the client that hears the job gets next, with that client, moving on past it.
    fn next(&mut self) -> Option<(Arc<Client>, Next)> {
        let (client, at) = self.client.clone()?;
        if client.is_closed() {
            self.client = None;
            self.alone = Some(Instant::now());
            return None;
        }
        let first = self.lines.front().map_or(self.last + 1, |(n, _)| *n);
        if at <= self.last && at < first {
            self.client = Some((client.clone(), first));
            return Some((client, Next::Dropped(first - at)));
        }
        if at <= self.last {
            let line = self.lines[(at - first) as usize].1.clone();
            self.client = Some((client.clone(), at + 1));
            return Some((client, Next::Line(at, line)));
        }
        let exit = self.ended.clone()?;
        Some((client, Next::End(exit)))
    }
}

/// Writes what `writes` hands it, in order, until the sender is dropped, then closes the program's input. After a write
/// fails, the rest fail too.
fn write_input(mut stdin: ChildStdin, writes: mpsc::Receiver<Input>) {
    let mut broken: Option<String> = None;
    for Input { data, reply: to } in writes {
        let result = match &broken {
            Some(error) => Err(error.clone()),
            None => stdin.write_all(&data).and_then(|()| stdin.flush()).map_err(|error| format!("The program's input is closed: {error}")),
        };
        if let Err(error) = &result {
            broken = Some(error.clone());
        }
        if let Some((client, id)) = to {
            reply(&client, id, result.map(|()| Value::Null));
        }
    }
}

/// The last `STDERR_KEPT` bytes of the pipe, read to its end.
fn tail(mut pipe: ChildStderr) -> String {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    while let Ok(n @ 1..) = pipe.read(&mut chunk) {
        kept.extend_from_slice(&chunk[..n]);
        if kept.len() > 2 * STDERR_KEPT {
            kept.drain(..kept.len() - STDERR_KEPT);
        }
    }
    let start = kept.len().saturating_sub(STDERR_KEPT);
    String::from_utf8_lossy(&kept[start..]).into_owned()
}
