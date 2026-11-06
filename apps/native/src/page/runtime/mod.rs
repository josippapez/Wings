//! Runs one frame's bundle in QuickJS on a thread of its own: a page, or a plugin's main.
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
//! budget, and a frame that overruns it is stopped for good. Its thread ends, and the others go on.

mod timers;
mod wings;

use super::tree::Tree;
use rquickjs::{CatchResultExt, Context, Ctx, Exception, Function, Runtime, function::Rest};
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
/// Clicks and broadcasts waiting for a frame that is busy. More than this are dropped, as a stuck frame can't use
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
    queue: Option<mpsc::SyncSender<Command>>,
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
    /// Nothing: ends a wait, so a frame that was let go of notices.
    Wake,
}

/// What the host can tell a frame of a plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Projects,
    Focus,
    Git,
    DiffRefresh,
    DiffClosed,
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
            Message::Focus => "focus",
            Message::Git => "git",
            Message::DiffRefresh => "diffRefresh",
            Message::DiffClosed => "diffClosed",
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
    queue: mpsc::SyncSender<Command>,
}

impl Reply {
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
            Err(mpsc::TrySendError::Full(command)) => {
                let queue = self.queue;
                thread::spawn(move || {
                    let _ = queue.send(command);
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

/// Where what the host tells a frame goes. Cloneable, so a host can keep one for each frame it runs.
#[derive(Clone)]
pub struct Inbox(mpsc::SyncSender<Command>);

impl Inbox {
    /// Hands the frame the JSON of a broadcast, which it parses for its `onBroadcast` listeners. Never waits.
    pub fn deliver(&self, json: String) -> Delivery {
        self.send(Message::Broadcast, 0, json)
    }

    /// Tells the frame `message`, with the text its listeners parse. Never waits.
    pub fn send(&self, message: Message, id: u32, text: String) -> Delivery {
        match self.0.try_send(Command::Deliver { message, id, text }) {
            Ok(()) => Delivery::Queued,
            Err(mpsc::TrySendError::Full(_)) => Delivery::Full,
            Err(mpsc::TrySendError::Disconnected(_)) => Delivery::Stopped,
        }
    }
}

pub struct PageRuntime {
    link: Option<Arc<dyn Link>>,
    inbox: Inbox,
    /// Set when the frame is let go of. A frame keeps a sender of its own for the answers to its calls, so its queue
    /// never runs out of senders to say so.
    closed: Arc<AtomicBool>,
    /// What the host keeps alive for as long as the frame is, like its place among a plugin's frames.
    _attached: Option<Box<dyn Any + Send>>,
}

impl Drop for PageRuntime {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
        let _ = self.inbox.0.try_send(Command::Wake);
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
        Self::start(source.into(), limits, services, on_event)
    }

    /// Starts a frame of a plugin: its main, or one of its pages. It talks to the plugin's other frames and to its
    /// storage through `link`.
    pub fn spawn_plugin(
        source: impl Into<String>,
        role: Role,
        limits: Limits,
        link: Arc<dyn Link>,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<Self> {
        let services = Services {
            render: role == Role::Page,
            link: Some(link),
            queue: None,
        };
        Self::start(source.into(), limits, services, on_event)
    }

    fn start(
        source: String,
        limits: Limits,
        mut services: Services,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (commands, received) = mpsc::sync_channel(QUEUED_COMMANDS);
        let closed = Arc::new(AtomicBool::new(false));
        services.queue = services.link.is_some().then(|| commands.clone());
        let link = services.link.clone();
        thread::Builder::new()
            .name("page-runtime".into())
            .stack_size(THREAD_STACK_BYTES)
            .spawn({
                let closed = closed.clone();
                move || serve(&source, limits, &services, &received, &closed, on_event)
            })?;
        Ok(Self {
            link,
            inbox: Inbox(commands),
            closed,
            _attached: None,
        })
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
        self.inbox
            .0
            .try_send(Command::Event {
                handler: handler.to_owned(),
                payload: payload.map(ToString::to_string).unwrap_or_default(),
            })
            .is_ok()
    }
}

/// When the turn the frame is in has to end. The engine asks `expired` while it runs.
struct Budget {
    start: Instant,
    /// Nanoseconds after `start`.
    until: AtomicU64,
    turn: Duration,
}

impl Budget {
    fn begin(&self) {
        let until = self.start.elapsed() + self.turn;
        self.until.store(until.as_nanos() as u64, Ordering::Relaxed);
    }

    fn expired(&self) -> bool {
        self.start.elapsed().as_nanos() as u64 >= self.until.load(Ordering::Relaxed)
    }
}

fn serve(
    source: &str,
    limits: Limits,
    services: &Services,
    commands: &mpsc::Receiver<Command>,
    closed: &AtomicBool,
    on_event: impl FnMut(Event) + 'static,
) {
    let emit = Rc::new(RefCell::new(on_event));
    let Ok(runtime) = Runtime::new() else {
        return stopped(&emit, "the JS engine did not start".into());
    };
    runtime.set_memory_limit(limits.heap_bytes);
    runtime.set_max_stack_size(STACK_BYTES);
    let budget = Arc::new(Budget {
        start: Instant::now(),
        until: AtomicU64::new(0),
        turn: limits.turn,
    });
    runtime.set_interrupt_handler(Some(Box::new({
        let budget = budget.clone();
        move || budget.expired()
    })));
    let Ok(context) = Context::full(&runtime) else {
        return stopped(&emit, "the JS engine did not start".into());
    };
    let timers = Rc::new(RefCell::new(Timers::default()));

    let run = Run {
        runtime: &runtime,
        context: &context,
        budget: &budget,
        emit: &emit,
        timers: &timers,
        services,
        closed,
    };
    if let Err(message) = run.frame(source, commands) {
        stopped(&emit, message);
    }
    timers.borrow_mut().clear();
}

fn stopped(emit: &RefCell<impl FnMut(Event)>, message: String) {
    (emit.borrow_mut())(Event::Error {
        message,
        fatal: true,
    });
}

struct Run<'a, F> {
    runtime: &'a Runtime,
    context: &'a Context,
    budget: &'a Budget,
    emit: &'a Rc<RefCell<F>>,
    timers: &'a Rc<RefCell<Timers>>,
    services: &'a Services,
    closed: &'a AtomicBool,
}

impl<F: FnMut(Event) + 'static> Run<'_, F> {
    /// Runs the bundle, then waits for events, broadcasts and timers until the host lets go of the frame, which is
    /// `Ok`, or the frame overruns its budget, which is an error.
    fn frame(&self, source: &str, commands: &mpsc::Receiver<Command>) -> Result<(), String> {
        self.budget.begin();
        let started = self.context.with(|ctx| {
            let deliver = wings::install(&ctx, self.services, self.emit.clone(), self.timers)?;
            ctx.eval::<(), _>(source).catch(&ctx).map_err(message)?;
            Ok(deliver)
        });
        // A bundle that throws is a frame that never started, not one with an error to show.
        let deliver = started.map_err(|error| {
            if self.budget.expired() {
                overran()
            } else {
                error
            }
        })?;
        self.drain()?;
        loop {
            let wait = self
                .timers
                .borrow()
                .next_due()
                .map(|due| due.saturating_duration_since(Instant::now()));
            let command = match wait {
                Some(wait) => match commands.recv_timeout(wait) {
                    Ok(command) => Some(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                },
                None => match commands.recv() {
                    Ok(command) => Some(command),
                    Err(_) => return Ok(()),
                },
            };
            if self.closed.load(Ordering::Relaxed) {
                return Ok(());
            }
            match command {
                Some(Command::Event { handler, payload }) => self.turn(|ctx| {
                    let dispatch: Function = ctx
                        .globals()
                        .get("__wingsEvent")
                        .catch(ctx)
                        .map_err(message)?;
                    dispatch
                        .call::<_, bool>((handler, payload))
                        .catch(ctx)
                        .map(drop)
                        .map_err(message)
                })?,
                Some(Command::Deliver {
                    message: kind,
                    id,
                    text,
                }) => {
                    if let Some(deliver) = &deliver {
                        self.turn(|ctx| {
                            let deliver = deliver.clone().restore(ctx).map_err(message)?;
                            deliver
                                .call::<_, ()>((kind.name(), id, text))
                                .catch(ctx)
                                .map_err(message)
                        })?;
                    }
                }
                Some(Command::Wake) | None => {}
            }
            // Timers run whether or not a command came, so a frame that is clicked all the time still gets its
            // timers. One that was set during this pass waits for the next, so a timer that sets itself again can't
            // keep the frame from its commands.
            let now = Instant::now();
            loop {
                let Some(due) = self.timers.borrow_mut().take_due(now) else {
                    break;
                };
                self.turn(|ctx| {
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
                })?;
            }
        }
    }

    /// One turn: `run` in the engine on a fresh budget, then the promise jobs it left behind.
    fn turn(&self, run: impl FnOnce(&Ctx<'_>) -> Result<(), String>) -> Result<(), String> {
        self.budget.begin();
        let outcome = self.context.with(|ctx| run(&ctx));
        self.turn_ended(outcome)
    }

    /// What a turn left: an error the frame threw is the frame's to show and it runs on, but one thrown because the
    /// turn ran too long, which it can't catch, ends it.
    fn turn_ended(&self, outcome: Result<(), String>) -> Result<(), String> {
        match outcome {
            Err(_) if self.budget.expired() => return Err(overran()),
            Err(message) => self.error(message),
            Ok(()) => {}
        }
        self.drain()
    }

    fn error(&self, message: String) {
        (self.emit.borrow_mut())(Event::Error {
            message,
            fatal: false,
        });
    }

    /// Runs the promise jobs the frame has queued, until there are none or the turn's time is up.
    fn drain(&self) -> Result<(), String> {
        while self.runtime.is_job_pending() {
            if self.budget.expired() {
                return Err(overran());
            }
            if let Err(job) = self.runtime.execute_pending_job() {
                if self.budget.expired() {
                    return Err(overran());
                }
                self.error(job.0.with(|ctx| describe(&ctx)));
            }
        }
        Ok(())
    }
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
