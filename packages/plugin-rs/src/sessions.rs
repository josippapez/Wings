//! Follows the Claude sessions that run in panes, as `wings-plugin-sdk/sessions` does: which pane runs which session,
//! each run's backlog from the transcript, then the entries Claude appends, oldest first and each once.

use crate::{format, host, task};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    rc::Rc,
};

/// The process start time is only to the second, so entries a little before it still belong to the run.
const SLACK: f64 = 2000.0;

/// What a plugin does with the sessions. They are called while the sessions are being changed, so none of them may
/// read the `Sessions` they belong to: `changed` is the one called when it is safe to.
pub struct Handlers<T> {
    /// A session seen for the first time, by its id.
    pub create: fn(&str) -> T,
    /// The pane that runs it, as `wings.onPanes` lists it, on every change of the panes.
    pub pane: fn(&mut T, &Value),
    /// A new `claude` process runs it: a new session, or a resumed one. `started_at` is in ms.
    pub start_run: fn(&mut T, f64),
    /// The transcript entries a run starts from, by the session's id.
    pub backlog: fn(String) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, String>>>>,
    /// One entry: `live` when Claude just wrote it, `this_run` unless it is from before this `claude` started.
    pub entry: fn(&mut T, &Value, bool, bool),
    /// Its pane runs something else now, or closed (`still_open` false). The session is dropped after.
    pub gone: fn(&T, &str, bool),
    /// Something about a session changed.
    pub changed: fn(),
}

struct Tracked<T> {
    id: String,
    value: T,
    started_at: Option<f64>,
    /// Live entries while the run's backlog loads, then none.
    queue: Option<Vec<Value>>,
    /// The ids of the backlog's entries, which the transcript can send again.
    seen: HashSet<String>,
}

struct Inner<T> {
    handlers: Handlers<T>,
    /// In the order they were first seen.
    tracked: RefCell<Vec<Tracked<T>>>,
    /// Pane id to the id of the session it runs.
    panes: RefCell<HashMap<String, String>>,
}

/// The sessions being followed.
pub struct Sessions<T>(Rc<Inner<T>>);

impl<T> Clone for Sessions<T> {
    fn clone(&self) -> Self {
        Sessions(self.0.clone())
    }
}

impl<T: 'static> Inner<T> {
    fn deliver(&self, tracked: &mut Tracked<T>, entry: &Value, live: bool) {
        let id = entry["uuid"].as_str().unwrap_or_default();
        // The backlog is read after Wings starts following the transcript, so it can hold what the transcript sends too.
        if entry["isSidechain"] == true || (live && tracked.seen.contains(id)) {
            return;
        }
        let at = entry["timestamp"].as_str().and_then(format::parse_time).unwrap_or_else(host::now);
        let this_run = at >= tracked.started_at.unwrap_or(0.0) - SLACK;
        (self.handlers.entry)(&mut tracked.value, entry, live, this_run);
    }

    async fn load(self: Rc<Self>, id: String, started_at: f64) {
        // A newer run, or a session no pane runs any more, makes this one pointless.
        let current = |tracked: &mut Vec<Tracked<T>>| tracked.iter().position(|t| t.id == id && t.started_at == Some(started_at));
        let started = {
            let mut tracked = self.tracked.borrow_mut();
            current(&mut tracked).map(|at| (self.handlers.start_run)(&mut tracked[at].value, started_at)).is_some()
        };
        if !started {
            return;
        }
        let entries = (self.handlers.backlog)(id.clone()).await.unwrap_or_default();
        {
            let mut tracked = self.tracked.borrow_mut();
            let Some(at) = current(&mut tracked) else {
                return;
            };
            let run = &mut tracked[at];
            run.seen = entries.iter().filter_map(|entry| entry["uuid"].as_str().map(str::to_owned)).collect();
            let queued = run.queue.take().unwrap_or_default();
            for entry in &entries {
                self.deliver(run, entry, false);
            }
            for entry in &queued {
                self.deliver(run, entry, true);
            }
        }
        (self.handlers.changed)();
    }

    fn panes(self: &Rc<Self>, list: &[Value]) {
        // Only Claude Code writes a transcript.
        let running: Vec<&Value> = list.iter().filter(|pane| pane["session"].is_object() && pane["session"]["kind"].as_str().is_none_or(|kind| kind == "claude")).collect();
        let session_of = |pane: &str| running.iter().find(|p| p["paneId"] == pane).and_then(|p| p["session"]["sessionId"].as_str());
        let mut changed = false;
        let mut loads = Vec::new();
        {
            let mut tracked = self.tracked.borrow_mut();
            for (pane, id) in self.panes.borrow().iter() {
                if session_of(pane) == Some(id.as_str()) {
                    continue;
                }
                if let Some(gone) = tracked.iter().find(|t| t.id == *id) {
                    (self.handlers.gone)(&gone.value, pane, list.iter().any(|open| open["paneId"] == pane.as_str()));
                    changed = true;
                }
            }
            for pane in &running {
                let run = &pane["session"];
                let (Some(id), started_at) = (run["sessionId"].as_str(), run["startedAt"].as_f64().unwrap_or(0.0)) else {
                    continue;
                };
                let at = tracked.iter().position(|t| t.id == id).unwrap_or_else(|| {
                    tracked.push(Tracked { id: id.to_owned(), value: (self.handlers.create)(id), started_at: None, queue: Some(Vec::new()), seen: HashSet::new() });
                    tracked.len() - 1
                });
                let session = &mut tracked[at];
                (self.handlers.pane)(&mut session.value, pane);
                if session.started_at != Some(started_at) {
                    session.started_at = Some(started_at);
                    session.queue = Some(Vec::new());
                    loads.push((id.to_owned(), started_at));
                }
                changed = true;
            }
            *self.panes.borrow_mut() =
                running.iter().filter_map(|p| Some((p["paneId"].as_str()?.to_owned(), p["session"]["sessionId"].as_str()?.to_owned()))).collect();
            let panes = self.panes.borrow();
            tracked.retain(|t| panes.values().any(|id| *id == t.id));
        }
        for (id, started_at) in loads {
            task::spawn(self.clone().load(id, started_at));
        }
        if changed {
            (self.handlers.changed)();
        }
    }

    fn transcript(&self, message: &Value) {
        {
            let mut tracked = self.tracked.borrow_mut();
            let Some(session) = tracked.iter_mut().find(|t| message["sessionId"] == t.id.as_str()) else {
                return;
            };
            if let Some(queue) = &mut session.queue {
                return queue.push(message["entry"].clone());
            }
            self.deliver(session, &message["entry"], true);
        }
        (self.handlers.changed)();
    }
}

/// Tracks the sessions `wings.onPanes` shows and calls `handlers`, one run of one session at a time. A new `claude`
/// process starts a run: `start_run`, then `backlog`, then `entry` for each entry it returned. Entries Claude writes
/// while the backlog loads wait for it, and an entry that is in both is passed on once. Sidechain entries are skipped.
/// A session no pane runs any more is dropped after `gone`.
pub fn follow<T: 'static>(handlers: Handlers<T>) -> Sessions<T> {
    let inner = Rc::new(Inner { handlers, tracked: RefCell::new(Vec::new()), panes: RefCell::new(HashMap::new()) });
    let (panes, transcript) = (inner.clone(), inner.clone());
    host::on_panes(move |list| panes.panes(list));
    host::on_transcript(move |message| transcript.transcript(message));
    Sessions(inner)
}

impl<T> Sessions<T> {
    /// Reads all the sessions together, in the order they were first seen.
    pub fn all<R>(&self, read: impl FnOnce(&[&T]) -> R) -> R {
        read(&self.0.tracked.borrow().iter().map(|t| &t.value).collect::<Vec<_>>())
    }

    /// Reads the sessions, in the order they were first seen, each with whether its run's backlog is in.
    pub fn each<R>(&self, mut read: impl FnMut(&T, bool) -> R) -> Vec<R> {
        self.0.tracked.borrow().iter().map(|t| read(&t.value, t.queue.is_none())).collect()
    }
}
