//! Runs frames' bundles in QuickJS: a page, or a plugin's main. An `Engine` is one thread and one JS runtime, which
//! runs any number of frames, each in turn: a plugin's main has one of its own, and its pages share another (see
//! `PluginHost`), so they share one copy of the engine's own tables and one thread rather than one each.
//!
//! A bare page sees two things of the host: `wings.render(json)`, which hands over its tree, and `setTimeout` and
//! `clearTimeout`, which run on real deadlines from this thread's loop. A frame of a plugin, started with a `Link`
//! (see `wings`), also has broadcast, storage and the calls the host answers (`Link::call`), which the host may answer
//! later from another thread through a `Reply`. Nothing else is registered: no files, network or process. What goes
//! in is `dispatch`, a handler id and a payload, and `Inbox::send`, what the host tells the frame (a broadcast from
//! another frame, the panes, an answer); what comes out is each new tree, parsed and checked, or the frame's error.
//!
//! A frame is code from the plugin, so a loop that never ends must not take the host, or another frame, with it:
//! every turn (the bundle running, an event, a broadcast, a timer, and the promise jobs it leaves behind) has a time
//! budget, and a frame that overruns it is stopped for good. Its context and timers are let go of, and the other
//! frames of its engine go on.
//!
//! A frame's bundle runs in a context of its own, with its own globals, as a browser frame would. A page built to
//! share React and `wings-plugin-ui` with the plugin's other pages (`Library`) runs instead in a context its engine
//! keeps for such pages, where the library's code is compiled once: each such page calls it for an instance of its
//! own, so its React state, scheduler and roots are its own, and its globals are an object of its own (`Shared`).

mod script;
mod timers;
pub mod wasm;
mod wings;

use super::tree::Tree;
use rquickjs::{
    CatchResultExt, Context, Ctx, Exception, Function, Object, Persistent, Runtime, Value,
    function::Rest,
};
use sha2::{Digest, Sha256};
use std::{
    any::Any,
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use timers::Timers;
pub use wings::{Link, Store};

const HEAP_BYTES: usize = 64 * 1024 * 1024;
/// React's commit walk recurses once for each level of the tree, and the pilot page alone needs more than the 256 KiB
/// the plugin worker gets. The thread has room for this and the engine's own frames.
const STACK_BYTES: usize = 1024 * 1024;
const THREAD_STACK_BYTES: usize = 4 * 1024 * 1024;
/// Clicks and broadcasts waiting for an engine that is busy. More than this are dropped, as a stuck engine can't use
/// them anyway, and the sender never waits for it.
const QUEUED_COMMANDS: usize = 256;
const MAX_MESSAGE_CHARS: usize = 500;

/// What a frame may use of the host.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// How long one turn may run.
    pub turn: Duration,
    pub heap_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            turn: Duration::from_secs(1),
            heap_bytes: HEAP_BYTES,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    /// The page rendered: what it shows now.
    Tree(Tree),
    /// The frame threw, or sent a tree the host refused. A `fatal` one is the last event: the frame has stopped.
    Error { message: String, fatal: bool },
}

/// What a frame of a plugin is: a page draws a tree with `wings.render`, and main only runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Main,
    Page,
}

/// What the host gave a frame to use.
struct Services {
    render: bool,
    link: Option<Arc<dyn Link>>,
    /// The frame's own queue, which a `Reply` answers through. Set when the frame starts.
    queue: Option<Queue>,
}

enum Command {
    Event {
        handler: String,
        payload: String,
    },
    /// Something the host tells the frame, for its setup script to hand to the listeners or the waiting promise.
    Deliver {
        message: Message,
        id: u32,
        text: String,
    },
}

/// What the host can tell a frame of a plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Projects,
    /// The JSON of the project on screen, or `null`, for `onProject`.
    Project,
    Focus,
    Git,
    DiffRefresh,
    DiffClosed,
    /// A reply typed in the diff viewer: `id` names it for `Link::call("actionDone")`, the text is the JSON of
    /// `{ diffId, replyTo, body }`.
    Reply,
    /// The JSON of another plugin's event, `{ from, name, data }`, for `onEvent`.
    PluginEvent,
    /// The JSON `{ sectionId, sort }` of a sort picked in a project section's menu, for `onSectionSort`.
    SectionSort,
    /// JSON from another frame of the plugin, for `onBroadcast`.
    Broadcast,
    /// The JSON list of panes, for `onPanes`.
    Panes,
    /// The JSON of a transcript entry, for `onTranscript`.
    Transcript,
    /// The JSON `{ sessionId }` of an OpenCode session that did something, for `onAgentActivity`.
    AgentActivity,
    /// A badge action was clicked: `id` names it for `Link::call("actionDone")`, the text is the JSON of `{ paneId, actionId }`.
    Action,
    /// Another plugin calls one of this plugin's tools: `id` names the call for `Link::call("toolDone")`, the text is
    /// the JSON of `{ name, arguments, project }`.
    Tool,
    /// The call `id` succeeded; the text is its JSON result.
    Resolve,
    /// The call `id` failed; the text is the error.
    Reject,
    /// One line the running call `id` printed.
    Output,
}

impl Message {
    fn name(self) -> &'static str {
        match self {
            Message::Projects => "projects",
            Message::Project => "project",
            Message::Focus => "focus",
            Message::Git => "git",
            Message::DiffRefresh => "diffRefresh",
            Message::DiffClosed => "diffClosed",
            Message::Reply => "reply",
            Message::PluginEvent => "pluginEvent",
            Message::SectionSort => "sectionSort",
            Message::Broadcast => "broadcast",
            Message::Panes => "panes",
            Message::Transcript => "transcript",
            Message::AgentActivity => "agentActivity",
            Message::Action => "action",
            Message::Tool => "tool",
            Message::Resolve => "resolve",
            Message::Reject => "reject",
            Message::Output => "output",
        }
    }
}

/// How a call of a frame is answered. The host holds it as long as it likes, on any thread, and answers once. It is
/// `Clone` so that a host that couldn't hand the work to a thread can still answer.
#[derive(Clone)]
pub struct Reply {
    call: u32,
    queue: Queue,
}

/// The answer to a `Reply::channel`'s reply.
pub struct Answer(mpsc::Receiver<Job>);

impl Answer {
    /// Waits up to `timeout` for the answer: the JSON result, or the error. `None` when none came in time. A zero
    /// `timeout` still takes an answer already given, as the window's own calls do.
    pub fn wait(&self, timeout: std::time::Duration) -> Option<Result<String, String>> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.0.recv_timeout(left).ok()? {
                Job::Frame(
                    _,
                    Command::Deliver {
                        message: Message::Resolve,
                        text,
                        ..
                    },
                ) => return Some(Ok(text)),
                Job::Frame(
                    _,
                    Command::Deliver {
                        message: Message::Reject,
                        text,
                        ..
                    },
                ) => return Some(Err(text)),
                _ => {}
            }
        }
    }
}

impl Reply {
    /// A reply no frame waits for: whoever calls waits with the `Answer`, as a call from Claude Code over the socket does.
    pub fn channel() -> (Reply, Answer) {
        let (jobs, answers) = mpsc::sync_channel(8);
        (
            Reply {
                call: 0,
                queue: Queue { jobs, frame: 0 },
            },
            Answer(answers),
        )
    }

    /// The call succeeded with the JSON `result`.
    pub fn ok(self, result: String) {
        self.send(Message::Resolve, result);
    }

    /// The call failed: the frame's promise rejects with an `Error` of `message`.
    pub fn err(self, message: String) {
        self.send(Message::Reject, message);
    }

    /// Answers with `result`, which is JSON on success and the message on failure.
    pub fn done(self, result: Result<String, String>) {
        match result {
            Ok(json) => self.ok(json),
            Err(message) => self.err(message),
        }
    }

    /// Where the lines the call prints while it runs go, for the `onOutput` the frame passed. A line is lost when the
    /// frame is behind.
    pub fn lines(&self) -> Arc<dyn Fn(&str) + Send + Sync> {
        let (queue, call) = (self.queue.clone(), self.call);
        Arc::new(move |line| {
            let _ = queue.try_send(Command::Deliver {
                message: Message::Output,
                id: call,
                text: line.to_owned(),
            });
        })
    }

    fn send(self, message: Message, text: String) {
        let command = Command::Deliver {
            message,
            id: self.call,
            text,
        };
        // An answer is never dropped. A call answered on the frame's own thread, with its queue full, must not wait
        // for itself, so that one is handed to a thread that can.
        match self.queue.try_send(command) {
            Err(mpsc::TrySendError::Full(job)) => {
                let jobs = self.queue.jobs;
                thread::spawn(move || {
                    let _ = jobs.send(job);
                });
            }
            Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
        }
    }
}

/// How a frame's queue took a broadcast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Queued,
    /// The frame is too far behind: the message is dropped.
    Full,
    /// The frame has stopped, or been let go of.
    Stopped,
}

/// What an engine's thread is asked to do.
enum Job {
    Open(Box<Opening>),
    /// A command for the frame with this id.
    Frame(u64, Command),
    /// Nothing: ends a wait, so an engine whose frames, or itself, were let go of notices.
    Wake,
}

/// A frame's way into its engine: the engine's queue, and the frame's id on it.
#[derive(Clone)]
pub(crate) struct Queue {
    jobs: mpsc::SyncSender<Job>,
    frame: u64,
}

impl Queue {
    fn try_send(&self, command: Command) -> Result<(), mpsc::TrySendError<Job>> {
        self.jobs.try_send(Job::Frame(self.frame, command))
    }
}

/// Where what the host tells a frame goes. Cloneable, so a host can keep one for each frame it runs.
#[derive(Clone)]
pub struct Inbox {
    queue: Queue,
    stopped: Arc<AtomicBool>,
}

impl Inbox {
    /// Hands the frame the JSON of a broadcast, which it parses for its `onBroadcast` listeners. Never waits.
    pub fn deliver(&self, json: String) -> Delivery {
        self.send(Message::Broadcast, 0, json)
    }

    /// Tells the frame `message`, with the text its listeners parse. Never waits.
    pub fn send(&self, message: Message, id: u32, text: String) -> Delivery {
        if self.stopped.load(Ordering::Relaxed) {
            return Delivery::Stopped;
        }
        match self.queue.try_send(Command::Deliver { message, id, text }) {
            Ok(()) => Delivery::Queued,
            Err(mpsc::TrySendError::Full(_)) => Delivery::Full,
            Err(mpsc::TrySendError::Disconnected(_)) => Delivery::Stopped,
        }
    }
}

/// The React and `wings-plugin-ui` a plugin's pages share: its `dist/native/shared.js`, which the plugin's build
/// writes beside the pages built to use it (`dist/native/shared/<page>.js`). Such a page starts with `SHARED_MARKER`
/// and the hash of the library it was built with, and runs only with that one, so a page and its library always come
/// from one build. The script defines `__wingsLib`, which this runs in a function of the page's globals (see `Shared`).
pub struct Library {
    hash: String,
    source: Arc<str>,
}

/// The first line of a page built to share its plugin's `Library`, before the library's hash.
pub const SHARED_MARKER: &str = "// wings-shared ";

impl Library {
    pub fn new(source: impl Into<Arc<str>>) -> Arc<Self> {
        let source = source.into();
        let hash = Sha256::digest(source.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Arc::new(Self { hash, source })
    }

    /// The hash a page built with this library names, in hex.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// The hash of the library `source` was built to share, if it is such a page.
    pub fn needed_by(source: &str) -> Option<&str> {
        Some(source.strip_prefix(SHARED_MARKER)?.lines().next()?.trim())
    }
}

/// One thread and one JS runtime, which runs the frames opened in it one turn at a time. It ends when the last
/// `PageRuntime` opened in it, and whoever started it, have let go of it.
pub struct Engine {
    jobs: mpsc::SyncSender<Job>,
    quit: Arc<AtomicBool>,
    next_frame: AtomicU64,
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        let _ = self.jobs.try_send(Job::Wake);
    }
}

impl Engine {
    /// Starts an engine whose frames may use `limits`: each turn of a frame has `limits.turn`, and the frames share
    /// `limits.heap_bytes`.
    pub fn start(limits: Limits) -> std::io::Result<Arc<Self>> {
        let (jobs, received) = mpsc::sync_channel(QUEUED_COMMANDS);
        let quit = Arc::new(AtomicBool::new(false));
        thread::Builder::new()
            .name("page-runtime".into())
            .stack_size(THREAD_STACK_BYTES)
            .spawn({
                let quit = quit.clone();
                move || serve(limits, &received, &quit)
            })?;
        Ok(Arc::new(Self {
            jobs,
            quit,
            next_frame: AtomicU64::new(1),
        }))
    }
}

pub struct PageRuntime {
    link: Option<Arc<dyn Link>>,
    inbox: Inbox,
    /// Set when the frame is let go of: its engine drops it then.
    closed: Arc<AtomicBool>,
    /// What the host keeps alive for as long as the frame is, like its place among a plugin's frames.
    _attached: Option<Box<dyn Any + Send>>,
    engine: Arc<Engine>,
}

impl Drop for PageRuntime {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
        let _ = self.engine.jobs.try_send(Job::Wake);
    }
}

impl PageRuntime {
    /// Starts a bare page on its own thread. `on_event` is called from that thread, so it must only hand the event on.
    pub fn spawn(
        source: impl Into<String>,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::spawn_with(source, Limits::default(), on_event)
    }

    pub fn spawn_with(
        source: impl Into<String>,
        limits: Limits,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<Self> {
        let services = Services {
            render: true,
            link: None,
            queue: None,
        };
        Ok(Self::open(&Engine::start(limits)?, source.into(), services, None, on_event))
    }

    /// Starts a frame of a plugin on a thread of its own: its main, or one of its pages. It talks to the plugin's
    /// other frames and to its storage through `link`.
    pub fn spawn_plugin(
        source: impl Into<String>,
        role: Role,
        limits: Limits,
        link: Arc<dyn Link>,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<Self> {
        Ok(Self::open_plugin(&Engine::start(limits)?, source, role, link, None, on_event))
    }

    /// Opens a frame of a plugin in `engine`, beside the frames already there. A page built to share `library` (see
    /// `Library`) needs it given here; one that isn't, or names another build's, stops at once with an error that says so.
    pub fn open_plugin(
        engine: &Arc<Engine>,
        source: impl Into<String>,
        role: Role,
        link: Arc<dyn Link>,
        library: Option<Arc<Library>>,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> Self {
        let source = source.into();
        if let Some(path) = source.strip_prefix(wasm::MARKER) {
            return wasm::open(path, link, on_event);
        }
        let services = Services {
            render: role == Role::Page,
            link: Some(link),
            queue: None,
        };
        Self::open(engine, source, services, library, on_event)
    }

    fn open(
        engine: &Arc<Engine>,
        source: String,
        mut services: Services,
        library: Option<Arc<Library>>,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> Self {
        let queue = Queue {
            jobs: engine.jobs.clone(),
            frame: engine.next_frame.fetch_add(1, Ordering::Relaxed),
        };
        let closed = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        services.queue = services.link.is_some().then(|| queue.clone());
        let link = services.link.clone();
        let opening = Job::Open(Box::new(Opening {
            id: queue.frame,
            source,
            services,
            library,
            on_event: Box::new(on_event),
            closed: closed.clone(),
            stopped: stopped.clone(),
        }));
        // A frame is never lost: with the engine's queue full, a thread waits to hand it over.
        if let Err(mpsc::TrySendError::Full(opening)) = engine.jobs.try_send(opening) {
            let jobs = engine.jobs.clone();
            thread::spawn(move || {
                let _ = jobs.send(opening);
            });
        }
        Self {
            link,
            inbox: Inbox { queue, stopped },
            closed,
            _attached: None,
            engine: engine.clone(),
        }
    }

    /// Keeps `held` alive until the frame is let go of, which is when its thread ends.
    pub fn attach(mut self, held: impl Any + Send) -> Self {
        self._attached = Some(Box::new(held));
        self
    }

    pub fn inbox(&self) -> Inbox {
        self.inbox.clone()
    }

    /// A hidden plugin page pauses focused work without stopping its frame.
    pub fn set_visible(&self, visible: bool) {
        if let Some(link) = &self.link {
            link.set_visible(visible);
        }
    }

    /// Sends the frame an event for the handler it named in its tree. False when it can't take it: it has stopped, or
    /// is too far behind.
    pub fn dispatch(&self, handler: &str, payload: Option<&serde_json::Value>) -> bool {
        !self.inbox.stopped.load(Ordering::Relaxed)
            && self
                .inbox
                .queue
                .try_send(Command::Event {
                    handler: handler.to_owned(),
                    payload: payload.map(ToString::to_string).unwrap_or_default(),
                })
                .is_ok()
    }

    /// `dispatch` for a click, a key press or a pick of yours, which lets the page copy for a moment after, as a
    /// browser's user activation does. Hovers, blurs, typing and scrolling are plain `dispatch`es.
    pub fn dispatch_gesture(&self, handler: &str, payload: Option<&serde_json::Value>) -> bool {
        if let Some(link) = &self.link {
            link.user_event();
        }
        self.dispatch(handler, payload)
    }
}

/// A frame on its way to its engine.
struct Opening {
    id: u64,
    source: String,
    services: Services,
    library: Option<Arc<Library>>,
    on_event: Box<dyn FnMut(Event) + Send>,
    closed: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
}

/// When the turn the engine is in has to end, on the clock of `used`. The engine asks `expired` while it runs. All of
/// it is called on the engine's thread.
struct Budget {
    /// On that clock, in nanoseconds.
    until: AtomicU64,
    turn: Duration,
}

/// How long the engine's thread has run, in nanoseconds: its own CPU time, not the time that passed. A turn that
/// waits for the CPU on a busy Mac, or in a host call, has not run for that long, and a page was stopped for good for
/// it when a second went by.
#[cfg(unix)]
fn used() -> u64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `clock_gettime` writes the `timespec` it is given and keeps no pointer.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) };
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}

/// debt: the time that passed, where the thread's CPU time isn't read yet (Windows: `GetThreadTimes`).
#[cfg(not(unix))]
fn used() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

impl Budget {
    fn begin(&self) {
        self.until
            .store(used() + self.turn.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Ends the turn now: whatever runs next is stopped at the engine's next check.
    fn expire(&self) {
        self.until.store(0, Ordering::Relaxed);
    }

    fn expired(&self) -> bool {
        used() >= self.until.load(Ordering::Relaxed)
    }
}

type Emit = Rc<RefCell<Box<dyn FnMut(Event)>>>;

/// A frame its engine runs.
struct Frame {
    id: u64,
    context: Context,
    /// Where its globals are: its context's global object, or the object of its own that a page sharing the plugin's
    /// library has in the shared context.
    scope: Persistent<Object<'static>>,
    deliver: Option<Persistent<Function<'static>>>,
    timers: Rc<RefCell<Timers>>,
    emit: Emit,
    closed: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
}

/// The context of an engine's pages that share their plugin's `Library`, which is compiled there once. Library code
/// runs in the realm it was compiled in, so the pages that call it run there too: an error a page throws is then an
/// `Error` to the library's `instanceof` as well. Each page's globals are an object of its own, which its bundle and
/// its instance of the library see as `globalThis` and as each global the host defines (`wrap`).
struct Shared {
    context: Context,
    /// The library compiled last, by its hash: a function of a page's globals, which returns that page's instance.
    library: Option<(String, Persistent<Function<'static>>)>,
}

/// The globals the host defines for a frame (`wings::SETUP`), and `__wingsShared`, the page's instance of the library.
const PAGE_GLOBALS: &str = "var globalThis = g, wings = g.wings, setTimeout = g.setTimeout, \
    clearTimeout = g.clearTimeout, setInterval = g.setInterval, clearInterval = g.clearInterval, \
    console = g.console, AbortController = g.AbortController, __wingsShared = g.__wingsShared;";

/// `source` as a function of a page's globals `g`, which it sees as its own, followed by `tail`.
fn wrap(source: &str, tail: &str) -> String {
    format!("(function (g) {{ {PAGE_GLOBALS}\n{source}\n;{tail}\n}})")
}

fn serve(limits: Limits, jobs: &mpsc::Receiver<Job>, quit: &AtomicBool) {
    let Ok(runtime) = Runtime::new() else {
        // Every frame opened here is told, until the engine is let go of.
        for job in jobs {
            if let Job::Open(mut opening) = job {
                opening.stopped.store(true, Ordering::Relaxed);
                (opening.on_event)(Event::Error {
                    message: "the JS engine did not start".into(),
                    fatal: true,
                });
            }
        }
        return;
    };
    runtime.set_memory_limit(limits.heap_bytes);
    runtime.set_max_stack_size(STACK_BYTES);
    let budget = Arc::new(Budget {
        until: AtomicU64::new(0),
        turn: limits.turn,
    });
    runtime.set_interrupt_handler(Some(Box::new({
        let budget = budget.clone();
        move || budget.expired()
    })));
    let mut run = Run {
        runtime: &runtime,
        budget: &budget,
        frames: Vec::new(),
        shared: None,
    };
    run.serve(jobs, quit);
    // Persistent values left in a frame would keep the engine from freeing what they hold.
    for frame in run.frames.drain(..) {
        frame.timers.borrow_mut().clear();
    }
}

struct Run<'a> {
    runtime: &'a Runtime,
    budget: &'a Budget,
    /// The open frames, in the order they opened.
    frames: Vec<Frame>,
    shared: Option<Shared>,
}

impl Run<'_> {
    /// Opens frames and runs their events, broadcasts and timers until the engine is let go of.
    fn serve(&mut self, jobs: &mpsc::Receiver<Job>, quit: &AtomicBool) {
        loop {
            let wait = self
                .frames
                .iter()
                .filter_map(|frame| frame.timers.borrow().next_due())
                .min()
                .map(|due| due.saturating_duration_since(Instant::now()));
            let job = match wait {
                Some(wait) => match jobs.recv_timeout(wait) {
                    Ok(job) => Some(job),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                },
                None => match jobs.recv() {
                    Ok(job) => Some(job),
                    Err(_) => return,
                },
            };
            if quit.load(Ordering::Relaxed) {
                return;
            }
            // Frames the host let go of leave before anything else runs.
            let open = self.frames.len();
            self.frames.retain(|frame| {
                let open = !frame.closed.load(Ordering::Relaxed);
                if !open {
                    frame.stopped.store(true, Ordering::Relaxed);
                    frame.timers.borrow_mut().clear();
                }
                open
            });
            if self.frames.len() < open {
                // What a frame held refers to itself, as a page's tree and its handlers do, so only a collection
                // frees it, and the host's link with it.
                self.runtime.run_gc();
            }
            match job {
                Some(Job::Open(opening)) => self.open(*opening),
                Some(Job::Frame(id, command)) => {
                    if let Some(at) = self.frames.iter().position(|frame| frame.id == id)
                        && let Err(message) = self.command(&self.frames[at], command)
                    {
                        self.stop(at, message);
                    }
                }
                Some(Job::Wake) | None => {}
            }
            // Timers run whether or not a job came, so a frame that is clicked all the time still gets its timers.
            // One that was set during this pass waits for the next, so a timer that sets itself again can't keep the
            // engine from its jobs.
            let now = Instant::now();
            let mut at = 0;
            'frames: while at < self.frames.len() {
                loop {
                    let Some(due) = self.frames[at].timers.borrow_mut().take_due(now) else {
                        break;
                    };
                    let ran = self.turn(&self.frames[at], |ctx| {
                        let callback = due.callback.restore(ctx).map_err(message)?;
                        let args = due
                            .args
                            .into_iter()
                            .map(|arg| arg.restore(ctx))
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(message)?;
                        callback
                            .call::<_, ()>((Rest(args),))
                            .catch(ctx)
                            .map_err(message)
                    });
                    if let Err(message) = ran {
                        self.stop(at, message);
                        continue 'frames;
                    }
                }
                at += 1;
            }
        }
    }

    /// Starts a frame: gives it its globals and runs its bundle. A bundle that throws is a frame that never started,
    /// not one with an error to show.
    fn open(&mut self, opening: Opening) {
        let Opening {
            id,
            source,
            services,
            library,
            mut on_event,
            closed,
            stopped,
        } = opening;
        let emit: Emit = Rc::new(RefCell::new(Box::new({
            let stopped = stopped.clone();
            // Nothing is heard of a frame after the error that stopped it. That error is marked stopped before it's
            // sent, so whoever hears it finds the frame refusing events already.
            move |event| {
                if matches!(event, Event::Error { fatal: true, .. }) || !stopped.load(Ordering::Relaxed) {
                    on_event(event)
                }
            }
        })));
        let timers = Rc::new(RefCell::new(Timers::default()));
        self.budget.begin();
        let started = match Library::needed_by(&source) {
            None => self.start_own(&source, &services, &emit, &timers),
            Some(hash) => match library.filter(|library| library.hash == hash) {
                Some(library) => self.start_shared(&source, &library, &services, &emit, &timers),
                None => Err("This page was built to share React with its plugin's dist/native/shared.js, and \
                     that file is missing or from another build. Rebuild the plugin."
                    .into()),
            },
        };
        let frame = |(context, scope, deliver)| Frame {
            id,
            context,
            scope,
            deliver,
            timers: timers.clone(),
            emit: emit.clone(),
            closed: closed.clone(),
            stopped: stopped.clone(),
        };
        match started {
            Ok(started) => {
                self.frames.push(frame(started));
                if let Err(message) = self.drain(self.frames.last().unwrap()) {
                    self.stop(self.frames.len() - 1, message);
                }
            }
            Err(error) => {
                let message = if self.budget.expired() { overran() } else { error };
                stopped.store(true, Ordering::Relaxed);
                (emit.borrow_mut())(Event::Error {
                    message,
                    fatal: true,
                });
                timers.borrow_mut().clear();
                self.discard_jobs();
            }
        }
    }

    /// A frame in a context of its own, whose globals are the context's.
    fn start_own(
        &self,
        source: &str,
        services: &Services,
        emit: &Emit,
        timers: &Rc<RefCell<Timers>>,
    ) -> Result<Started, String> {
        let context =
            Context::full(self.runtime).map_err(|_| "the JS engine did not start".to_owned())?;
        let (scope, deliver) = context.with(|ctx| {
            let scope = ctx.globals();
            let deliver = wings::install(&ctx, &scope, services, emit.clone(), timers)?;
            script::run(&ctx, source).catch(&ctx).map_err(message)?;
            Ok::<_, String>((Persistent::save(&ctx, scope), deliver))
        })?;
        Ok((context, scope, deliver))
    }

    /// A page that shares `library` with the engine's other such pages (see `Shared`).
    fn start_shared(
        &mut self,
        source: &str,
        library: &Library,
        services: &Services,
        emit: &Emit,
        timers: &Rc<RefCell<Timers>>,
    ) -> Result<Started, String> {
        if self.shared.is_none() {
            let context = Context::full(self.runtime)
                .map_err(|_| "the JS engine did not start".to_owned())?;
            self.shared = Some(Shared {
                context,
                library: None,
            });
        }
        let shared = self.shared.as_mut().unwrap();
        let context = shared.context.clone();
        let (scope, deliver) = context.with(|ctx| {
            let compiled = match &shared.library {
                Some((hash, factory)) if *hash == library.hash => factory.clone().restore(&ctx).map_err(message)?,
                _ => {
                    #[cfg(test)]
                    {
                        *tests::COMPILED.lock().unwrap().entry(library.hash.clone()).or_default() += 1;
                    }
                    let factory: Function = script::run(&ctx, &wrap(&library.source, "return __wingsLib;"))
                        .and_then(|value| value.get())
                        .catch(&ctx)
                        .map_err(|error| format!("dist/native/shared.js: {}", message(error)))?;
                    shared.library = Some((library.hash.clone(), Persistent::save(&ctx, factory.clone())));
                    factory
                }
            };
            let scope = Object::new(ctx.clone()).map_err(message)?;
            let deliver = wings::install(&ctx, &scope, services, emit.clone(), timers)?;
            let instance: Value = compiled.call((scope.clone(),)).catch(&ctx).map_err(message)?;
            scope.set("__wingsShared", instance).map_err(message)?;
            let page: Function = script::run(&ctx, &wrap(source, ""))
                .and_then(|value| value.get())
                .catch(&ctx)
                .map_err(message)?;
            page.call::<_, ()>((scope.clone(),)).catch(&ctx).map_err(message)?;
            Ok::<_, String>((Persistent::save(&ctx, scope), deliver))
        })?;
        Ok((context, scope, deliver))
    }

    /// Runs a command of `frame`'s in a turn of its own. An error is one the frame can't go on after.
    fn command(&self, frame: &Frame, command: Command) -> Result<(), String> {
        match command {
            Command::Event { handler, payload } => self.turn(frame, |ctx| {
                let scope = frame.scope.clone().restore(ctx).map_err(message)?;
                let dispatch: Function = scope.get("__wingsEvent").catch(ctx).map_err(message)?;
                dispatch
                    .call::<_, bool>((handler, payload))
                    .catch(ctx)
                    .map(drop)
                    .map_err(message)
            }),
            Command::Deliver {
                message: kind,
                id,
                text,
            } => match &frame.deliver {
                Some(deliver) => self.turn(frame, |ctx| {
                    let deliver = deliver.clone().restore(ctx).map_err(message)?;
                    deliver
                        .call::<_, ()>((kind.name(), id, text))
                        .catch(ctx)
                        .map_err(message)
                }),
                None => Ok(()),
            },
        }
    }

    /// One turn of `frame`: `run` in the engine on a fresh budget, then the promise jobs it left behind.
    fn turn(
        &self,
        frame: &Frame,
        run: impl FnOnce(&Ctx<'_>) -> Result<(), String>,
    ) -> Result<(), String> {
        self.budget.begin();
        let outcome = frame.context.with(|ctx| run(&ctx));
        // An error the frame threw is the frame's to show and it runs on, but one thrown because the turn ran too
        // long, which it can't catch, ends it.
        match outcome {
            Err(_) if self.budget.expired() => return Err(overran()),
            Err(message) => error(frame, message),
            Ok(()) => {}
        }
        self.drain(frame)
    }

    /// Runs the promise jobs `frame` has queued, until there are none or the turn's time is up. Every turn ends with
    /// none left, so the jobs queued are the frame's own.
    fn drain(&self, frame: &Frame) -> Result<(), String> {
        while self.runtime.is_job_pending() {
            if self.budget.expired() {
                return Err(overran());
            }
            if let Err(job) = self.runtime.execute_pending_job() {
                if self.budget.expired() {
                    return Err(overran());
                }
                error(frame, job.0.with(|ctx| describe(&ctx)));
            }
        }
        Ok(())
    }

    /// Stops the frame at `at` for good with `message`, and lets go of all it holds.
    fn stop(&mut self, at: usize, message: String) {
        let frame = self.frames.remove(at);
        frame.stopped.store(true, Ordering::Relaxed);
        (frame.emit.borrow_mut())(Event::Error {
            message,
            fatal: true,
        });
        frame.timers.borrow_mut().clear();
        self.discard_jobs();
        drop(frame);
        self.runtime.run_gc();
    }

    /// The promise jobs a frame that just stopped left behind, which would otherwise run in the next frame's turn.
    /// They run with no time left, so the engine stops each at its first check, and what they throw goes nowhere.
    fn discard_jobs(&self) {
        self.budget.expire();
        while self.runtime.is_job_pending() {
            let _ = self.runtime.execute_pending_job();
        }
    }
}

/// What a frame that started has: its context, its globals, and the function that delivers what the host tells it.
type Started = (
    Context,
    Persistent<Object<'static>>,
    Option<Persistent<Function<'static>>>,
);

fn error(frame: &Frame, message: String) {
    (frame.emit.borrow_mut())(Event::Error {
        message,
        fatal: false,
    });
}

fn overran() -> String {
    "the page ran for too long and was stopped".into()
}

/// An error as one short text.
fn message(error: impl ToString) -> String {
    error.to_string().chars().take(MAX_MESSAGE_CHARS).collect()
}

/// What the frame threw, as one short text.
fn describe(ctx: &Ctx<'_>) -> String {
    let thrown = ctx.catch();
    match Exception::from_value(thrown.clone()) {
        Ok(exception) => message(exception.message().unwrap_or_default()),
        Err(_) => message(
            thrown
                .as_string()
                .and_then(|text| text.to_string().ok())
                .unwrap_or_else(|| "the page threw".into()),
        ),
    }
}

#[cfg(test)]
mod tests;
