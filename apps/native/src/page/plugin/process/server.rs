//! The programs of tabs' pages that the terminal server runs (`spawn`'s `survive`), so they go on while Wings quits,
//! restarts or updates. The server keeps what they print since their page's last note, and the note, and identifies each
//! by what it is for (`owner`): its plugin, the key of its tab, its token and its Claude Code session. The next run of
//! Wings hands each to its plugin's host as the plugin starts (`take`), which checks the plugin may still run it.

use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use serde_json::Value;
use wings_plugins::process::{self, Launch};
use wings_ptyd::{
    client::{self, Control, JobEvent, JobEvents},
    proto::JobSpawn,
};

use super::{LeftJob, Out, Place, Proc, Running, Shared};

/// A program the terminal server runs for a tab's page.
pub(in crate::page::plugin) struct Job {
    job: client::Job,
    /// What the server keeps it for, which changes as its page says which session it goes on with.
    owner: Mutex<Value>,
    /// Wings quits: it runs on for Wings' next run, and nothing here stops it any more.
    let_go: AtomicBool,
}

impl Job {
    fn new(job: client::Job, owner: Value) -> Arc<Job> {
        Arc::new(Job { job, owner: Mutex::new(owner), let_go: AtomicBool::new(false) })
    }

    pub(super) fn write(&self, text: String, done: impl FnOnce(Result<(), String>) + Send + 'static) {
        self.job.write(text.as_bytes(), done);
    }

    pub(super) fn stop(&self) {
        if !self.let_go.load(Ordering::Relaxed)
            && let Err(error) = self.job.stop()
        {
            eprintln!("[plugins] couldn't stop a program in the terminal server: {error}");
        }
    }

    pub(super) fn pid(&self) -> Option<u32> {
        self.job.pid()
    }

    pub(super) fn stopping(&self) -> bool {
        self.job.stopping()
    }

    /// What the server keeps for the page that takes the program over in Wings' next run.
    pub(super) fn note(&self, since: Option<u64>, session: Option<&str>, note: &Value) {
        let owner = session.and_then(|session| {
            let mut owner = self.owner.lock().unwrap();
            (owner["session"] != session).then(|| {
                owner["session"] = session.into();
                owner.clone()
            })
        });
        if let Err(error) = self.job.note(since, owner, note.to_string().as_bytes()) {
            eprintln!("[plugins] couldn't keep a program's note in the terminal server: {error}");
        }
    }

    /// Wings quits: the program runs on in the server, for the page of its tab in Wings' next run.
    pub(super) fn let_go(&self) {
        self.let_go.store(true, Ordering::Relaxed);
        self.job.detach();
    }
}

/// Starts the program `launch` in the terminal server at `socket`, for `owner`, with `input` as what it reads first.
pub(super) fn start(socket: &Path, launch: Launch, owner: Value, input: Option<&str>) -> io::Result<(Arc<Job>, JobEvents)> {
    let spawn = JobSpawn { program: launch.program, args: launch.args, cwd: launch.cwd, env: launch.env, owner: owner.clone() };
    let (job, events) = client::Job::start(socket, spawn, input.unwrap_or_default().as_bytes())?;
    Ok((Job::new(job, owner), events))
}

/// Takes over a program Wings' run before this one left running in the terminal server, for `host`, as a program left
/// at the tab it is for. `None`, and stopped, when this run of the plugin may not run it.
pub(super) fn take(host: &Arc<Shared>, left: &LeftJob) -> Option<(Arc<Job>, JobEvents, Arc<Running>)> {
    let owner = &left.info.owner;
    let text = |key: &str| owner[key].as_str().map(str::to_owned);
    let args: Vec<String> = owner["args"].as_array().into_iter().flatten().filter_map(|arg| arg.as_str().map(str::to_owned)).collect();
    let allowed = match (&host.access, text("program"), text("cwd"), text("tab")) {
        (Some(access), Some(program), Some(cwd), Some(tab)) => {
            let allowed = access.builtin && process::check(&access.plugin, &program, &args, Path::new(&cwd)).is_ok();
            allowed.then_some((program, cwd, tab))
        }
        _ => None,
    };
    let Some((program, cwd, tab)) = allowed else {
        if let Err(error) = Control::connect(&left.socket).and_then(|mut control| control.stop_job(left.info.job)) {
            eprintln!("[plugins] couldn't stop a program the terminal server kept: {error}");
        }
        return None;
    };
    let note = Control::connect(&left.socket)
        .and_then(|mut control| control.job_note(left.info.job))
        .ok()
        .and_then(|note| serde_json::from_slice(&note).ok())
        .unwrap_or(Value::Null);
    let (job, events, attached) = client::Job::attach(&left.socket, left.info.job, left.info.pid)
        .inspect_err(|error| eprintln!("[plugins] couldn't take over a program the terminal server kept: {error}"))
        .ok()?;
    let job = Job::new(job, owner.clone());
    // Numbered as the server numbers its lines, from the last one its page noted.
    let out = Out { last: attached.since, since: attached.since, note, ..Out::default() };
    let running = Arc::new(Running {
        process: Proc::Server(job.clone()),
        program,
        args,
        cwd: cwd.into(),
        place: Some(Place::Tab(tab)),
        session: Mutex::new(text("session")),
        host: Mutex::new(Arc::downgrade(host)),
        out: Mutex::new(out),
        order: Mutex::default(),
        asks: Mutex::default(),
        reloaded: Mutex::default(),
        // A message armed before Wings restarted is gone with it: the page asks your click again.
        armed: Mutex::default(),
    });
    Some((job, events, running))
}

/// Hands what the program prints to `running` until it ends, on a thread of its own.
pub(super) fn follow(running: Arc<Running>, job: Arc<Job>, mut events: JobEvents) {
    let spawned = thread::Builder::new().name("plugin-job".into()).spawn(move || {
        let exit = loop {
            match events.next() {
                JobEvent::Line(n, line) => running.heard(Some(n), &String::from_utf8_lossy(&line)),
                JobEvent::Dropped(lines) => running.skipped(lines),
                JobEvent::Ended(exit) => {
                    break match exit.error {
                        Some(error) => Err(error),
                        None => Ok(serde_json::json!({ "code": exit.code, "stderr": exit.stderr }).to_string()),
                    };
                }
                // Wings let go of it as it quit, and it runs on for the next run.
                JobEvent::Closed if job.let_go.load(Ordering::Relaxed) => return,
                JobEvent::Closed => break Err("The terminal server that ran the program stopped".into()),
            }
        };
        let host = running.host.lock().unwrap().upgrade();
        if let Some(host) = host {
            host.forget(&running);
        }
        running.end(exit);
    });
    if let Err(error) = spawned {
        eprintln!("[plugins] couldn't follow a program in the terminal server: {error}");
    }
}
