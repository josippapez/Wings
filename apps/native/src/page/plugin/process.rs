//! `spawn`, `processWrite` and `processStop`: programs a frame keeps running and talks to, under the manifest's
//! `processes`, like an agent run headless. The frame names each with a token, which its link has prefixed with the
//! frame's id, so a frame reaches only its own. They stop when the frame closes or the plugin is turned off.
//!
//! An update or restart of the plugin doesn't stop them: the programs of a tab's page, or of main, are handed to the
//! plugin's next run (`hand_over`), where the page that loads in the same tab, or the next main, lists them
//! (`processes`) and takes them over (`adopt`). It hears what they printed since its page before noted it had what it
//! needs of their output (`processNote`), then what they print next. One that no frame takes over within `GRACE`, or
//! that the new manifest doesn't allow, is stopped.

use super::{Access, FrameState, Shared, account::AccountRequest};
use crate::page::runtime::Reply;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex, Weak, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};
use wings_plugins::{
    panes::exec_cwd,
    process::{self, Process},
};

/// The most programs one plugin keeps running at once.
const MAX_RUNNING: usize = 8;
/// The most text one `processWrite` sends: a message with a pasted image fits.
const WRITE_MAX: usize = process::LINE_LIMIT;
/// How much of what a program printed since its page's last note is kept for a page that takes it over. Past it, the
/// oldest lines go.
const KEPT_BYTES: usize = 16 * 1024 * 1024;
/// The most a program's note holds, as JSON.
const NOTE_MAX: usize = 1024 * 1024;
/// How long after an update or restart a program waits for a frame to take it over.
pub(super) const GRACE: Duration = Duration::from_secs(30);

/// The calls of this module, which the frame's link hands here with where the frame runs.
pub(super) const CALLS: [&str; 6] = ["spawn", "processWrite", "processStop", "processNote", "processes", "adopt"];

#[derive(Deserialize)]
struct SpawnRequest {
    program: String,
    args: Vec<String>,
    cwd: String,
    /// Runs under a Claude Code account: see `account`.
    #[serde(default)]
    account: Option<AccountRequest>,
}

/// Where a frame runs, which a program it started stays with through an update: the plugin's main, or the tab or block
/// that holds the `FrameState`. Only a frame that runs there next may take the program over.
#[derive(Clone)]
pub(super) enum Place {
    Main,
    Page(FrameState),
}

impl Place {
    fn same(&self, other: &Place) -> bool {
        match (self, other) {
            (Place::Main, Place::Main) => true,
            (Place::Page(a), Place::Page(b)) => a.same(b),
            _ => false,
        }
    }
}

type Lines = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// The frame's call that started a program or took it over, which hears its lines and how it ended.
struct Listener {
    lines: Lines,
    done: Reply,
    /// The number of the line before the first one this frame heard, so the count of lines it heard is a line's number.
    before: u64,
}

/// What a program printed, and who hears it.
#[derive(Default)]
struct Out {
    /// The lines past `since`, oldest first, each with its number; the first line is 1.
    kept: VecDeque<(u64, String)>,
    bytes: usize,
    /// The number of the last line it printed.
    last: u64,
    /// The frame that hears it said it has what it needs of the lines up to this one.
    since: u64,
    listener: Option<Listener>,
    /// What the frame that hears it keeps with it for the one that may take it over.
    note: Value,
    /// How it ended, while no frame was there to hear it.
    ended: Option<Result<String, String>>,
}

impl Out {
    fn keep(&mut self, n: u64, line: &str) {
        self.bytes += line.len();
        self.kept.push_back((n, line.to_owned()));
        while self.bytes > KEPT_BYTES {
            let Some((_, old)) = self.kept.pop_front() else {
                break;
            };
            self.bytes -= old.len();
        }
    }

    fn forget_noted(&mut self) {
        while self.kept.front().is_some_and(|(n, _)| *n <= self.since) {
            let Some((_, old)) = self.kept.pop_front() else {
                break;
            };
            self.bytes -= old.len();
        }
    }
}

/// A program a frame started, from its start until it ends.
pub(super) struct Running {
    process: Arc<Process>,
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    /// Where the frame that started it runs, when a frame that runs there next may take it over.
    place: Option<Place>,
    /// The run of the plugin it belongs to now, which lets go of it when it ends.
    host: Mutex<Weak<Shared>>,
    out: Mutex<Out>,
    /// Held while lines go to a frame, so they reach it in order, and one that takes the program over hears what was
    /// kept before what comes next.
    order: Mutex<()>,
}

impl Running {
    /// A line the program printed: kept for a frame that may take it over, and handed to the frame that hears it.
    fn heard(&self, line: &str) {
        let _order = self.order.lock().unwrap();
        let lines = {
            let mut out = self.out.lock().unwrap();
            out.last += 1;
            let n = out.last;
            out.keep(n, line);
            out.listener.as_ref().map(|listener| listener.lines.clone())
        };
        if let Some(lines) = lines
            && !lines(line)
        {
            // Its frame stopped and nothing took the program over, so nobody can talk to it any more.
            let still = self.out.lock().unwrap().listener.as_ref().is_some_and(|listener| Arc::ptr_eq(&listener.lines, &lines));
            if still {
                self.process.stop();
            }
        }
    }

    /// The program ended: the frame that hears it is told how, or the one that takes it over.
    fn end(&self, exit: Result<String, String>) {
        let _order = self.order.lock().unwrap();
        let mut out = self.out.lock().unwrap();
        match out.listener.take() {
            Some(listener) => {
                drop(out);
                listener.done.done(exit);
            }
            None => out.ended = Some(exit),
        }
    }

    /// Hands the program to the frame whose call `done` took it over: the lines kept past the last note, after a line
    /// `{"wingsDropped": n}` when `n` more were let go of for room, then each new one. `done` is answered once it ends.
    fn attach(&self, lines: Lines, done: Reply) {
        let _order = self.order.lock().unwrap();
        let (kept, dropped, ended) = {
            let mut out = self.out.lock().unwrap();
            let first = out.kept.front().map_or(out.last + 1, |(n, _)| *n);
            let dropped = first - 1 - out.since;
            let kept: Vec<String> = out.kept.iter().map(|(_, line)| line.clone()).collect();
            let ended = out.ended.take();
            if ended.is_none() {
                out.listener = Some(Listener { lines: lines.clone(), done: done.clone(), before: first - 1 });
            }
            (kept, dropped, ended)
        };
        if dropped > 0 {
            lines(&json!({ "wingsDropped": dropped }).to_string());
        }
        for line in &kept {
            if !lines(line) {
                break;
            }
        }
        if let Some(exit) = ended {
            done.done(exit);
        }
    }

    /// Keeps `note` for the frame that may take the program over. With `heard`, the number of lines the frame that hears
    /// it has heard, it has what it needs of those, which are let go of.
    fn note(&self, note: Value, heard: Option<u64>) {
        let mut out = self.out.lock().unwrap();
        out.note = note;
        let before = out.listener.as_ref().map(|listener| listener.before);
        if let (Some(heard), Some(before)) = (heard, before) {
            let since = before.saturating_add(heard).min(out.last);
            out.since = out.since.max(since);
            out.forget_noted();
        }
    }
}

/// A program a frame left running when the plugin was updated or restarted, until a frame takes it over.
pub(super) struct Orphan {
    /// What `processes` names it by, for `adopt`.
    id: String,
    /// The token the frame that started it gave it.
    token: String,
    running: Arc<Running>,
    /// When it is stopped if nothing took it over.
    until: Instant,
}

/// The programs of a plugin's run that was updated or restarted, for its next run to take over
/// (`PluginHost::take_handover`). Those that are left in it when it's dropped are stopped.
#[derive(Default)]
pub struct Handover(Vec<(String, Arc<Running>)>);

impl Drop for Handover {
    fn drop(&mut self) {
        for (_, running) in &self.0 {
            running.process.stop();
        }
    }
}

impl Shared {
    /// A call of `CALLS` from a frame that runs at `place`.
    pub(super) fn process_call(self: &Arc<Self>, place: Option<Place>, method: &str, params: Value, reply: Reply) {
        let Some(access) = self.access.clone() else {
            return reply.err(format!("{} is turned off or not installed", self.config.id));
        };
        // The frame's link has checked it and put the frame's id in front.
        let token = params["token"].as_str().unwrap_or_default().to_owned();
        let result = match method {
            "spawn" => self.spawn(token, place, &params, reply.clone(), access),
            "processWrite" => self.write(&token, &params, reply.clone()),
            "processNote" => self.note(&token, &params).map(|()| reply.clone().ok("null".into())),
            "processes" => {
                reply.clone().ok(self.left(place.as_ref()).to_string());
                Ok(())
            }
            "adopt" => self.adopt(token, place.as_ref(), &params, reply.clone()),
            _ => {
                if let Some(running) = self.processes.lock().unwrap().get(&token) {
                    running.process.stop();
                }
                reply.clone().ok("null".into());
                Ok(())
            }
        };
        if let Err(error) = result {
            reply.err(error);
        }
    }

    fn spawn(
        self: &Arc<Self>,
        token: String,
        place: Option<Place>,
        params: &Value,
        reply: Reply,
        access: Access,
    ) -> Result<(), String> {
        let SpawnRequest { program, args, cwd, account } =
            serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
        let cwd = exec_cwd(&program, Some(&cwd), &access.env.projects(), &self.pane_folders(), false)?;
        let config_dir = access.claude_config_dir(account.as_ref(), &cwd);
        // What the program reads first, sent with its start: what the page asks as it opens, never an answer to the
        // program's questions, whose ids it makes up later.
        let input = match params.get("input") {
            None | Some(Value::Null) => None,
            Some(Value::String(input)) if input.len() <= WRITE_MAX => Some(input.clone()),
            Some(_) => return Err("spawn input is text of up to 8 MB".into()),
        };
        let listener = Listener { lines: Arc::new(reply.lines_in_order()), done: reply, before: 0 };
        let running = Arc::new(Running {
            process: Process::new(),
            program,
            args,
            cwd,
            place,
            host: Mutex::new(Arc::downgrade(self)),
            out: Mutex::new(Out { listener: Some(listener), ..Out::default() }),
            order: Mutex::default(),
        });
        // Kept from now, before it starts, so that the page can write to it at once.
        {
            let mut processes = self.processes.lock().unwrap();
            self.room(&processes, &token)?;
            processes.insert(token, running.clone());
        }
        if let Some(input) = input {
            // A failed write shows as the program's own end.
            running.process.write(input, |_| {});
        }
        let started = running.clone();
        let spawned = thread::Builder::new().name("plugin-process".into()).spawn(move || {
            let exit = process::run_under(&access.plugin, &started.process, &started.program, &started.args, &started.cwd, config_dir.as_deref(), |line| {
                started.heard(line);
                true
            });
            let host = started.host.lock().unwrap().upgrade();
            if let Some(host) = host {
                host.forget(&started);
            }
            started.end(exit.and_then(|exit| serde_json::to_string(&exit).map_err(|error| error.to_string())));
        });
        if let Err(error) = spawned {
            self.forget(&running);
            return Err(format!("The call couldn't start: {error}"));
        }
        Ok(())
    }

    fn write(&self, token: &str, params: &Value, reply: Reply) -> Result<(), String> {
        let text = params["text"]
            .as_str()
            .ok_or("processWrite text must be a string")?
            .to_owned();
        if text.len() > WRITE_MAX {
            return Err("processWrite is limited to 8 MB of text".into());
        }
        let running = self
            .processes
            .lock()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or_else(|| format!("No program is running as {}", shown(token)))?;
        running.process.write(text, move |result| reply.done(result.map(|()| "null".into())));
        Ok(())
    }

    /// `processNote`: what the frame keeps with one of its programs for the frame that may take it over, and with
    /// `heard`, how many of its lines it has what it needs of.
    fn note(&self, token: &str, params: &Value) -> Result<(), String> {
        let note = params.get("note").cloned().unwrap_or(Value::Null);
        if note.to_string().len() > NOTE_MAX {
            return Err("A program's note is limited to 1 MB".into());
        }
        let heard = match params.get("heard") {
            None | Some(Value::Null) => None,
            Some(heard) => Some(heard.as_u64().ok_or("processNote heard is a number of lines")?),
        };
        let running = self
            .processes
            .lock()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or_else(|| format!("No program is running as {}", shown(token)))?;
        running.note(note, heard);
        Ok(())
    }

    /// `processes`: the programs the frame before this one, where it runs, left running for it to take over.
    fn left(&self, place: Option<&Place>) -> Value {
        let Some(place) = place else {
            return json!([]);
        };
        let orphans = self.orphans.lock().unwrap();
        orphans
            .iter()
            .filter(|orphan| orphan.running.place.as_ref().is_some_and(|at| at.same(place)))
            .map(|orphan| {
                let running = &orphan.running;
                let out = running.out.lock().unwrap();
                json!({
                    "id": orphan.id, "token": orphan.token, "program": running.program, "args": running.args,
                    "cwd": running.cwd.to_string_lossy(), "note": out.note, "kept": out.kept.len(),
                })
            })
            .collect()
    }

    /// `adopt`: takes over the program `from` that `processes` listed for this frame, as its own program `token`.
    fn adopt(&self, token: String, place: Option<&Place>, params: &Value, reply: Reply) -> Result<(), String> {
        let from = params["from"].as_str().unwrap_or_default();
        let running = {
            let mut processes = self.processes.lock().unwrap();
            let mut orphans = self.orphans.lock().unwrap();
            let at = orphans
                .iter()
                .position(|orphan| {
                    orphan.id == from
                        && place.is_some_and(|place| orphan.running.place.as_ref().is_some_and(|at| at.same(place)))
                })
                .ok_or_else(|| format!("No program was left running as {from} for this page"))?;
            if processes.contains_key(&token) {
                return Err(format!("A program is already running as {}", shown(&token)));
            }
            let orphan = orphans.remove(at);
            processes.insert(token, orphan.running.clone());
            orphan.running
        };
        let lines: Lines = Arc::new(reply.lines_in_order());
        // On a thread of its own: what was kept goes into the frame's queue, which the frame empties on its own thread.
        thread::Builder::new()
            .name("plugin-process-adopt".into())
            .spawn(move || running.attach(lines, reply))
            .map(drop)
            .map_err(|error| format!("The call couldn't start: {error}"))
    }

    /// Stops the programs whose token starts with `prefix`: a frame's, or with an empty one, all of them.
    pub(super) fn stop_processes(&self, prefix: &str) {
        for (token, running) in self.processes.lock().unwrap().iter() {
            if token.starts_with(prefix) {
                running.process.stop();
            }
        }
    }

    /// The program ended: no frame reaches it any more.
    fn forget(&self, running: &Arc<Running>) {
        self.processes.lock().unwrap().retain(|_, kept| !Arc::ptr_eq(kept, running));
        self.orphans.lock().unwrap().retain(|orphan| !Arc::ptr_eq(&orphan.running, running));
    }

    /// Takes out the programs whose frames may be followed by others where they run, and those still waiting for one:
    /// what an update or restart hands the plugin's next run. They no longer reach the frames that started them.
    pub(super) fn hand_over(&self) -> Handover {
        let mut processes = self.processes.lock().unwrap();
        let mut orphans = self.orphans.lock().unwrap();
        let mut handed: Vec<(String, Arc<Running>)> = orphans.drain(..).map(|orphan| (orphan.token, orphan.running)).collect();
        let placed: Vec<String> = processes.iter().filter(|(_, running)| running.place.is_some()).map(|(token, _)| token.clone()).collect();
        for token in placed {
            if let Some(running) = processes.remove(&token) {
                running.out.lock().unwrap().listener = None;
                handed.push((shown(&token).to_owned(), running));
            }
        }
        Handover(handed)
    }

    /// Keeps the programs of `handover` that this run's manifest allows for its frames to take over in `grace`, and
    /// stops the rest.
    pub(super) fn take_handover(self: &Arc<Self>, mut handover: Handover, grace: Duration) {
        let until = Instant::now() + grace;
        let plugin = self.access.as_ref().map(|access| &access.plugin);
        {
            let mut orphans = self.orphans.lock().unwrap();
            for (token, running) in std::mem::take(&mut handover.0) {
                // One that ended on the way is no longer anyone's.
                if running.out.lock().unwrap().ended.is_some() {
                    continue;
                }
                // The new version has to allow it as the old one did.
                if plugin.is_some_and(|plugin| process::check(plugin, &running.program, &running.args, &running.cwd).is_ok()) {
                    *running.host.lock().unwrap() = Arc::downgrade(self);
                    let id = format!("left-{}", self.next_orphan.fetch_add(1, Ordering::Relaxed));
                    orphans.push(Orphan { id, token, running, until });
                } else {
                    running.process.stop();
                }
            }
        }
        let host = Arc::downgrade(self);
        let _ = thread::Builder::new().name("plugin-process-grace".into()).spawn(move || {
            thread::sleep(grace);
            if let Some(host) = host.upgrade() {
                host.stop_left(false);
            }
        });
    }

    /// Stops the programs no frame took over in time, or with `all`, every one left.
    pub(super) fn stop_left(&self, all: bool) {
        let now = Instant::now();
        let mut stopped = Vec::new();
        self.orphans.lock().unwrap().retain(|orphan| {
            let keep = !all && orphan.until > now;
            if !keep {
                stopped.push(orphan.running.clone());
            }
            keep
        });
        for running in stopped {
            running.process.stop();
        }
    }

    /// The process ids of the programs the frame `frame` runs, and of those the frame before it at `place` left for it.
    pub(super) fn pids(&self, frame: Option<u64>, place: &Place) -> Vec<u32> {
        let prefix = frame.map(|frame| format!("{frame}:"));
        let mut pids: Vec<u32> = self
            .processes
            .lock()
            .unwrap()
            .iter()
            .filter(|(token, _)| prefix.as_ref().is_some_and(|prefix| token.starts_with(prefix.as_str())))
            .filter_map(|(_, running)| running.process.pid())
            .collect();
        let orphans = self.orphans.lock().unwrap();
        let left = orphans.iter().filter(|orphan| orphan.running.place.as_ref().is_some_and(|at| at.same(place)));
        pids.extend(left.filter_map(|orphan| orphan.running.process.pid()));
        pids
    }

    fn room(&self, processes: &HashMap<String, Arc<Running>>, token: &str) -> Result<(), String> {
        if processes.contains_key(token) {
            return Err(format!("A program is already running as {}", shown(token)));
        }
        if processes.len() + self.orphans.lock().unwrap().len() >= MAX_RUNNING {
            return Err(format!("A plugin can keep at most {MAX_RUNNING} programs running"));
        }
        Ok(())
    }
}

/// The token as the frame named it, without the frame's id in front.
fn shown(token: &str) -> &str {
    token.split_once(':').map_or(token, |(_, own)| own)
}
