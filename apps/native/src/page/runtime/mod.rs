//! Runs a plugin's frames: a page or a plugin's main, each a WebAssembly module (`wasm`) on a thread of its own. A frame
//! reaches the host through a `Link`: broadcast, storage and the calls the host answers (`Link::call`), which the host
//! may answer later from another thread through a `Reply`. What goes in is `dispatch`, a handler id and a payload, and
//! `Inbox::send`, what the host tells the frame (a broadcast from another frame, the panes, an answer); what comes out
//! is each new tree, parsed and checked, or the frame's error.
//!
//! A frame is code from the plugin, so a loop that never ends must not take the host, or another frame, with it: every
//! turn has a fuel budget (see `wasm`), and a frame that overruns it is stopped for good.

#[cfg(any(test, feature = "test-support"))]
pub mod fake;
pub mod wasm;
mod wings;

use super::tree::Tree;
use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};
pub use wings::{Link, Store};

const HEAP_BYTES: usize = 64 * 1024 * 1024;
const THREAD_STACK_BYTES: usize = 4 * 1024 * 1024;
/// Clicks and broadcasts waiting for a frame that is busy. More than this are dropped, as a stuck frame can't use
/// them anyway, and the sender never waits for it.
const QUEUED_COMMANDS: usize = 256;
const MAX_MESSAGE_CHARS: usize = 500;

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

enum Command {
    Event {
        handler: String,
        payload: String,
    },
    /// Something the host tells the frame, for the frame's listeners or the call it waits on.
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
                Job::Frame(Command::Deliver {
                        message: Message::Resolve,
                        text,
                        ..
                    },
                ) => return Some(Ok(text)),
                Job::Frame(Command::Deliver {
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
                queue: Queue { jobs },
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

/// What a frame's thread is asked to do.
enum Job {
    Frame(Command),
    /// Nothing: ends a wait, so a frame that was let go of notices.
    Wake,
}

/// A frame's way into its thread: the thread's queue.
#[derive(Clone)]
pub(crate) struct Queue {
    jobs: mpsc::SyncSender<Job>,
}

impl Queue {
    fn try_send(&self, command: Command) -> Result<(), mpsc::TrySendError<Job>> {
        self.jobs.try_send(Job::Frame(command))
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

pub struct PageRuntime {
    link: Option<Arc<dyn Link>>,
    inbox: Inbox,
    /// Set when the frame is let go of: its thread ends then.
    closed: Arc<AtomicBool>,
    /// What the host keeps alive for as long as the frame is, like its place among a plugin's frames.
    _attached: Option<Box<dyn Any + Send>>,
    jobs: mpsc::SyncSender<Job>,
}

impl Drop for PageRuntime {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
        let _ = self.jobs.try_send(Job::Wake);
    }
}

impl PageRuntime {
    /// Starts a frame of a plugin on a thread of its own: its main, or one of its pages. It talks to the plugin's
    /// other frames and to its storage through `link`. `source` names the module (see `wasm::MARKER`); anything else
    /// is not a frame this runs.
    pub fn open_plugin(
        source: impl Into<String>,
        role: Role,
        link: Arc<dyn Link>,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> std::io::Result<Self> {
        let source = source.into();
        match source.strip_prefix(wasm::MARKER) {
            Some(path) => Ok(wasm::open(path, role, link, on_event)),
            None => Err(std::io::Error::other("a frame is a WebAssembly module")),
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

/// An error as one short text.
fn message(error: impl ToString) -> String {
    error.to_string().chars().take(MAX_MESSAGE_CHARS).collect()
}

fn overran() -> String {
    "the page ran for too long and was stopped".into()
}

#[cfg(test)]
mod tests;
