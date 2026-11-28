//! `spawn`, `processWrite` and `processStop`: programs a frame keeps running and talks to, under the manifest's
//! `processes`, like an agent run headless. The frame names each with a token, which its link has prefixed with the
//! frame's id, so a frame reaches only its own. They stop when the frame closes or the plugin is turned off.
//!
//! An update or restart of the plugin doesn't stop them: the programs of a tab's page, or of main, are handed to the
//! plugin's next run (`hand_over`), where the page that loads in the same tab, or the next main, lists them
//! (`processes`) and takes them over (`adopt`). It hears what they printed since its page before noted it had what it
//! needs of their output (`processNote`), then what they print next. One that no frame takes over within `GRACE`, or
//! that the new manifest doesn't allow, is stopped.
//!
//! A tab's page can ask for more (`survive`): its program runs in the terminal server (`server`), so it goes on while
//! Wings itself quits, restarts or updates. The host of the next run of Wings takes it over as the plugin starts
//! (`take_jobs`), and the page of the tab with the same key takes it over from there as above, within `RESTART_GRACE`.
//!
//! No two programs of a plugin go on with one Claude Code session at once: a start that names the session another one
//! runs is refused, so two Claudes never write to one transcript.

use super::{Access, FrameState, LeftJob, Shared, account::AccountRequest};
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

#[cfg(unix)]
mod server;

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
/// How long after Wings starts a program the terminal server kept through the restart waits for its tab's page.
pub(super) const RESTART_GRACE: Duration = Duration::from_secs(120);

/// The calls of this module, which the frame's link hands here with where the frame runs.
pub(super) const CALLS: [&str; 7] = ["spawn", "processWrite", "processArm", "processStop", "processNote", "processes", "adopt"];

/// How long after its time an armed message may still go (`processArm`), as a computer asleep at the time wakes later.
const ARMED_FOR_MS: u64 = 24 * 3_600_000;
/// The shortest time between two reloads of Claude Code's skills or plugins a page writes without a click.
const RELOAD_EVERY_MS: u64 = 10_000;
/// How far ahead a message may be armed: a plan's limit resets within a day.
const ARM_AHEAD_MOST_MS: u64 = 25 * 3_600_000;
/// The longest message that may be armed.
const ARMED_TEXT_MAX: usize = 4096;

/// The one message a page may write later without a click, as your click armed it (`processArm`): its text, and the
/// time (ms since the epoch) from which it may go.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Armed {
    text: String,
    from: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
}

#[derive(Deserialize)]
struct SpawnRequest {
    program: String,
    args: Vec<String>,
    cwd: String,
    /// Runs under a Claude Code account: see `account`.
    #[serde(default)]
    account: Option<AccountRequest>,
    /// Keep it running through a restart or update of Wings, in the terminal server.
    #[serde(default)]
    survive: bool,
}

/// Where a frame runs, which a program it started stays with through an update: the plugin's main, or the tab or block
/// that holds the `FrameState`. Only a frame that runs there next may take the program over.
#[derive(Clone)]
pub(super) enum Place {
    Main,
    Page(FrameState),
    /// The tab with this key, as Wings' run before this one had it: the terminal server kept the program through the
    /// restart.
    Tab(String),
}

impl Place {
    fn same(&self, other: &Place) -> bool {
        match (self, other) {
            (Place::Main, Place::Main) => true,
            (Place::Page(a), Place::Page(b)) => a.same(b),
            (Place::Page(page), Place::Tab(key)) | (Place::Tab(key), Place::Page(page)) => page.is_tab() && page.key() == *key,
            (Place::Tab(a), Place::Tab(b)) => a == b,
            _ => false,
        }
    }
}

/// Where a program runs.
pub(super) enum Proc {
    /// In the app, which it ends with.
    Here(Arc<Process>),
    /// In the terminal server, which keeps it through a restart of the app.
    #[cfg(unix)]
    Server(Arc<server::Job>),
}

impl Proc {
    fn write(&self, text: String, done: impl FnOnce(Result<(), String>) + Send + 'static) {
        match self {
            Proc::Here(process) => process.write(text, done),
            #[cfg(unix)]
            Proc::Server(job) => job.write(text, done),
        }
    }

    fn stop(&self) {
        match self {
            Proc::Here(process) => process.stop(),
            #[cfg(unix)]
            Proc::Server(job) => job.stop(),
        }
    }

    fn pid(&self) -> Option<u32> {
        match self {
            Proc::Here(process) => process.pid(),
            #[cfg(unix)]
            Proc::Server(job) => job.pid(),
        }
    }

    fn stopping(&self) -> bool {
        match self {
            Proc::Here(process) => process.stopping(),
            #[cfg(unix)]
            Proc::Server(job) => job.stopping(),
        }
    }

    /// What the terminal server keeps for the next run of Wings, for a program it runs.
    #[cfg_attr(not(unix), allow(unused_variables))]
    fn note(&self, since: Option<u64>, session: Option<&str>, note: &Value) {
        match self {
            Proc::Here(_) => {}
            #[cfg(unix)]
            Proc::Server(job) => job.note(since, session, note),
        }
    }

    /// Wings quits: a program the terminal server runs goes on for its next run.
    fn let_go(&self) {
        match self {
            Proc::Here(_) => {}
            #[cfg(unix)]
            Proc::Server(job) => job.let_go(),
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
    process: Proc,
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    /// Where the frame that started it runs, when a frame that runs there next may take it over.
    place: Option<Place>,
    /// The Claude Code session it goes on with, as its start or its page said.
    session: Mutex<Option<String>>,
    /// The run of the plugin it belongs to now, which lets go of it when it ends.
    host: Mutex<Weak<Shared>>,
    out: Mutex<Out>,
    /// Held while lines go to a frame, so they reach it in order, and one that takes the program over hears what was
    /// kept before what comes next.
    order: Mutex<()>,
    /// What Claude Code asked of the chat's message box and the page hasn't answered yet, by request id: a plugin's
    /// read (`ui_prompt_read`) or fill (`ui_prompt_fill`), the only answers a page may write unprompted (`unprompted`).
    pub(super) asks: Mutex<HashMap<String, &'static str>>,
    /// The message your click armed to go later, like a chat's "Continue when it resets" (`processArm`).
    armed: Mutex<Option<Armed>>,
    /// When a page last wrote a reload of Claude Code's skills or plugins without a click (`unprompted`), in ms.
    reloaded: Mutex<Option<u64>>,
}

impl Running {
    /// A line the program printed: kept for a frame that may take it over, and handed to the frame that hears it. `n` is
    /// its number when the terminal server counted it.
    fn heard(&self, n: Option<u64>, line: &str) {
        // Only a line that names one is read as JSON: most are a turn's messages.
        if line.contains("\"ui_prompt_")
            && let Ok(asked) = serde_json::from_str::<Value>(line)
            && asked["type"] == "control_request"
            && let Some(id) = asked["request_id"].as_str()
        {
            let kind = match asked["request"]["subtype"].as_str() {
                Some("ui_prompt_read") => Some("read"),
                Some("ui_prompt_fill") => Some("fill"),
                _ => None,
            };
            if let Some(kind) = kind {
                self.asks.lock().unwrap().insert(id.to_owned(), kind);
            }
        }
        let _order = self.order.lock().unwrap();
        let lines = {
            let mut out = self.out.lock().unwrap();
            out.last = n.unwrap_or(out.last + 1);
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

    /// The terminal server let go of `lines` lines no frame heard, for room.
    #[cfg(unix)]
    fn skipped(&self, lines: u64) {
        let _order = self.order.lock().unwrap();
        let listener = {
            let mut out = self.out.lock().unwrap();
            out.last += lines;
            out.listener.as_ref().map(|listener| listener.lines.clone())
        };
        if let Some(listener) = listener {
            listener(&json!({ "wingsDropped": lines }).to_string());
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
    /// it has heard, it has what it needs of those, which are let go of. With `session`, the program goes on with that
    /// Claude Code session.
    fn note(&self, note: Value, heard: Option<u64>, session: Option<String>) {
        if session.is_some() {
            *self.session.lock().unwrap() = session.clone();
        }
        let since = {
            let mut out = self.out.lock().unwrap();
            out.note = note.clone();
            let before = out.listener.as_ref().map(|listener| listener.before);
            match (heard, before) {
                (Some(heard), Some(before)) => {
                    let since = before.saturating_add(heard).min(out.last);
                    out.since = out.since.max(since);
                    out.forget_noted();
                    Some(out.since)
                }
                _ => None,
            }
        };
        self.process.note(since, session.as_deref(), &note);
    }

    /// Whether it goes on with the Claude Code session `session` and isn't being stopped.
    fn runs(&self, session: &str) -> bool {
        self.session.lock().unwrap().as_deref() == Some(session) && !self.process.stopping()
    }
}

/// The Claude Code session a program goes on with, from its arguments: its `--resume=<id>`, unless it forks a new
/// session from that one.
fn resumes(args: &[String]) -> Option<String> {
    if args.iter().any(|arg| arg == "--fork-session") {
        return None;
    }
    args.iter().find_map(|arg| arg.strip_prefix("--resume=")).filter(|id| !id.is_empty()).map(str::to_owned)
}

/// Whether a program is Claude Code started for a chat: `claude` with the chat's stream-json input and output. Wings sets
/// `CLAUDE_CODE_NOTIFICATIONS_HOST` for it, so Claude Code pushes a mod's notifications as `ui_notify` lines. Any other
/// program, and any other use of `claude`, runs without it.
fn is_chat_claude(program: &str, args: &[String]) -> bool {
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    program == "claude" && has("--input-format=stream-json") && has("--output-format=stream-json")
}

/// A session's id as a page says it: letters, digits and dashes.
fn session_id(value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if (1..=128).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') => Ok(Some(id.clone())),
        Some(_) => Err("A session is an id of letters, digits and dashes".into()),
    }
}

const SESSION_TAKEN: &str = "Claude Code is already running this session elsewhere in Wings. Open it again here once it has finished.";

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

/// The message `processArm`'s params arm, checked: none for a null `text`.
fn armed_of(params: &Value) -> Result<Option<Armed>, String> {
    let text = match params.get("text") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(text)) if !text.trim().is_empty() && text.len() <= ARMED_TEXT_MAX => text.clone(),
        Some(_) => return Err("processArm text is null or a message of 1 to 4,096 bytes".into()),
    };
    let from = params["at"]
        .as_f64()
        .filter(|at| at.is_finite() && *at >= 0.0)
        .ok_or("processArm needs at, the time in ms since the epoch the message may go from")? as u64;
    if from > now_ms() + ARM_AHEAD_MOST_MS {
        return Err("processArm's at is at most 25 hours ahead".into());
    }
    Ok(Some(Armed { text, from }))
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
            "processArm" => self.arm(&token, &params).map(|()| reply.clone().ok("null".into())),
            "processNote" => self.note(&token, &params).map(|()| reply.clone().ok("null".into())),
            "processes" => session_id(params.get("session")).map(|session| reply.clone().ok(self.left(place.as_ref(), session.as_deref()).to_string())),
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
        let SpawnRequest { program, args, cwd, account, survive } =
            serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
        let cwd = exec_cwd(&program, Some(&cwd), &access.env.projects(), &self.pane_folders(), false)?;
        let config_dir = access.claude_config_dir(account.as_ref(), &cwd)?;
        let notifications = is_chat_claude(&program, &args);
        // What the program reads first, sent with its start: what the page asks as it opens, never an answer to the
        // program's questions, whose ids it makes up later.
        let input = match params.get("input") {
            None | Some(Value::Null) => None,
            Some(Value::String(input)) if input.len() <= WRITE_MAX => Some(input.clone()),
            Some(_) => return Err("spawn input is text of up to 8 MB".into()),
        };
        let session = resumes(&args);
        if let Some(session) = &session
            && self.runs_session(session, None)
        {
            return Err(SESSION_TAKEN.into());
        }
        let listener = Listener { lines: Arc::new(reply.lines_in_order()), done: reply, before: 0 };
        let running = |process: Proc, program: String, args: Vec<String>, cwd: PathBuf, place: Option<Place>, listener: Listener| {
            Arc::new(Running {
                process,
                program,
                args,
                cwd,
                place,
                session: Mutex::new(session.clone()),
                host: Mutex::new(Arc::downgrade(self)),
                out: Mutex::new(Out { listener: Some(listener), ..Out::default() }),
                order: Mutex::default(),
                asks: Mutex::default(),
                armed: Mutex::default(),
                reloaded: Mutex::default(),
            })
        };
        // Only a tab's page has a place that comes back after a restart, and only Wings' own plugins keep a program
        // running once Wings has quit.
        #[cfg(unix)]
        let tab = match (&place, survive && access.builtin) {
            (Some(Place::Page(state)), true) if state.is_tab() => Some(state.key()),
            _ => None,
        };
        #[cfg(unix)]
        if let Some(tab) = tab
            && let Some(socket) = access.env.job_server()
        {
            self.room(&self.processes.lock().unwrap(), &token)?;
            let launch = || process::launch(&access.plugin, &program, &args, &cwd, config_dir.as_deref(), notifications);
            let owner = json!({
                "plugin": self.config.id, "tab": tab, "token": shown(&token), "program": program, "args": args,
                "cwd": cwd.to_string_lossy(), "session": session,
            });
            // A server that stopped answering (killed, say) is replaced, as for a pane, and the program tried there once.
            let started = server::start(&socket, launch()?, owner.clone(), input.as_deref()).or_else(|error| {
                let again = access.env.replace_job_server(&socket, &error.to_string()).ok_or(error)?;
                server::start(&again, launch().map_err(std::io::Error::other)?, owner, input.as_deref())
            });
            match started {
                Ok((job, events)) => {
                    let started = running(Proc::Server(job.clone()), program, args, cwd, place, listener);
                    self.processes.lock().unwrap().insert(token, started.clone());
                    server::follow(started, job, events);
                    return Ok(());
                }
                // The terminal server can't run it: the app does, as for a program that needn't survive.
                Err(error) => crate::page::log::line(
                    &self.config.id,
                    "process",
                    &format!("the terminal server didn't start {program}, so Wings runs it and it ends when Wings quits: {error}"),
                ),
            }
        }
        #[cfg(not(unix))]
        let _ = survive;
        let here = Process::new();
        let running = running(Proc::Here(here.clone()), program, args, cwd, place, listener);
        // Kept from now, before it starts, so that the page can write to it at once.
        {
            let mut processes = self.processes.lock().unwrap();
            self.room(&processes, &token)?;
            processes.insert(token, running.clone());
        }
        if let Some(input) = input {
            // A failed write shows as the program's own end.
            here.write(input, |_| {});
        }
        let started = running.clone();
        let spawned = thread::Builder::new().name("plugin-process".into()).spawn(move || {
            let exit = process::run_under(&access.plugin, &here, &started.program, &started.args, &started.cwd, config_dir.as_deref(), notifications, |line| {
                started.heard(None, line);
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

    /// Whether a program of the plugin goes on with the Claude Code session `session` and isn't being stopped, but those
    /// left at `here`.
    fn runs_session(&self, session: &str, here: Option<&Place>) -> bool {
        let at_here = |running: &Running| here.is_some_and(|here| running.place.as_ref().is_some_and(|at| at.same(here)));
        let running = self.processes.lock().unwrap().values().any(|running| running.runs(session));
        running || self.orphans.lock().unwrap().iter().any(|orphan| orphan.running.runs(session) && !at_here(&orphan.running))
    }

    /// Whether `text` is only what a page may write to its program `token` without a click: Claude Code control requests
    /// that draw what its plugins show (`ui_attach`, `ui_render`, `ui_panes`) or read the plan's usage (`get_usage`), or have it read its skills and plugins
    /// from disk again (`reload_skills`, `reload_plugins`, one every 10 s),
    /// answers to the reads and fills of the message box the program asked for (`asks`) and nobody answered yet, each in
    /// its ask's shape: `{ text, cursor }` for a read, `{ filled }` for a fill, and the one message your click armed
    /// (`processArm`), word for word, once its time has come. Apart from that message none of it reaches the agent,
    /// answers its questions or starts a turn. What it lets through is no longer pending.
    pub(super) fn unprompted(&self, token: &str, text: &str) -> bool {
        let Some(running) = self.processes.lock().unwrap().get(token).cloned() else {
            return false;
        };
        let mut asks = running.asks.lock().unwrap();
        let mut armed = running.armed.lock().unwrap();
        let mut reloaded = running.reloaded.lock().unwrap();
        let now = now_ms();
        let mut sent_armed = false;
        let mut reload_sent = false;
        let mut answered = Vec::new();
        let mut lines = text.lines().filter(|line| !line.trim().is_empty()).peekable();
        let allowed = lines.peek().is_some()
            && lines.all(|line| {
                let Ok(line) = serde_json::from_str::<Value>(line) else {
                    return false;
                };
                match line["type"].as_str() {
                    // Stopping the turn makes Claude do nothing, so a chat's "stop near the limit" may send it unasked.
                    // Which of a mod's panes Wings shows and which has the keys, as Claude Code asked: they tell, and start nothing.
                    // A reload of skills or plugins only has Claude Code read the disk again, which a chat's slash menu asks for
                    // when you type `/`: typing isn't a click, so at most one in RELOAD_EVERY_MS goes.
                    Some("control_request") if matches!(line["request"]["subtype"].as_str(), Some("reload_skills" | "reload_plugins")) => {
                        let fits = !reload_sent && reloaded.is_none_or(|last| now.saturating_sub(last) >= RELOAD_EVERY_MS);
                        reload_sent |= fits;
                        fits
                    }
                    Some("control_request") => matches!(line["request"]["subtype"].as_str(), Some("ui_attach" | "ui_render" | "ui_panes" | "ui_pane_show" | "ui_pane_focus" | "interrupt" | "get_usage")),
                    Some("user") => {
                        let fits = !sent_armed
                            && line["message"]["role"] == "user"
                            && armed.as_ref().is_some_and(|armed| line["message"]["content"] == armed.text.as_str() && (armed.from..armed.from + ARMED_FOR_MS).contains(&now));
                        sent_armed |= fits;
                        fits
                    }
                    Some("control_response") if line["response"]["subtype"] == "success" => {
                        let id = line["response"]["request_id"].as_str().unwrap_or_default();
                        let Some(fields) = line["response"]["response"].as_object() else {
                            return false;
                        };
                        let keys: Vec<&str> = fields.keys().map(String::as_str).collect();
                        let fits = match asks.get(id) {
                            Some(&"read") => fields.contains_key("text") && keys.iter().all(|key| matches!(*key, "text" | "cursor")),
                            Some(&"fill") => keys == ["filled"],
                            _ => false,
                        };
                        if fits {
                            answered.push(id.to_owned());
                        }
                        fits
                    }
                    _ => false,
                }
            });
        if allowed {
            if reload_sent {
                *reloaded = Some(now);
            }
            for id in answered {
                asks.remove(&id);
            }
            if sent_armed {
                *armed = None;
            }
        }
        allowed
    }

    /// Whether `params` asks `processArm` for what the program `token` has armed already: a page that comes back after an
    /// update of its plugin arms again without a click, and changes nothing.
    pub(super) fn armed_already(&self, token: &str, params: &Value) -> bool {
        let Some(running) = self.processes.lock().unwrap().get(token).cloned() else {
            return false;
        };
        let wanted = armed_of(params);
        matches!((&*running.armed.lock().unwrap(), wanted), (Some(armed), Ok(Some(wanted))) if *armed == wanted)
    }

    /// `processArm`: with `text` and `at`, the one message the page may write to the program later without a click, from
    /// `at` (ms since the epoch) for a day; with `text` null, none. Arming needs your click (`PluginFrame::call`).
    fn arm(&self, token: &str, params: &Value) -> Result<(), String> {
        let armed = armed_of(params)?;
        let running = self
            .processes
            .lock()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or_else(|| format!("No program is running as {}", shown(token)))?;
        *running.armed.lock().unwrap() = armed;
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

    /// `processNote`: what the frame keeps with one of its programs for the frame that may take it over, with `heard`,
    /// how many of its lines it has what it needs of, and with `session`, the Claude Code session it goes on with.
    fn note(&self, token: &str, params: &Value) -> Result<(), String> {
        let note = params.get("note").cloned().unwrap_or(Value::Null);
        if note.to_string().len() > NOTE_MAX {
            return Err("A program's note is limited to 1 MB".into());
        }
        let heard = match params.get("heard") {
            None | Some(Value::Null) => None,
            Some(heard) => Some(heard.as_u64().ok_or("processNote heard is a number of lines")?),
        };
        let session = session_id(params.get("session"))?;
        let running = self
            .processes
            .lock()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or_else(|| format!("No program is running as {}", shown(token)))?;
        running.note(note, heard, session);
        Ok(())
    }

    /// `processes`: the programs the frame before this one, where it runs, left running for it to take over. With
    /// `session`, also whether another program of the plugin goes on with that Claude Code session: one entry
    /// `{ elsewhere: true, session }`, which can't be taken over.
    fn left(&self, place: Option<&Place>, session: Option<&str>) -> Value {
        let Some(place) = place else {
            return json!([]);
        };
        let mut listed: Vec<Value> = {
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
                        "session": *running.session.lock().unwrap(),
                    })
                })
                .collect()
        };
        if let Some(session) = session
            && self.runs_session(session, Some(place))
        {
            listed.push(json!({ "elsewhere": true, "session": session }));
        }
        Value::Array(listed)
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
        self.stop_left_after(grace);
    }

    /// Takes over the programs of this plugin's tabs that the terminal server kept running from Wings' run before this
    /// one: the page of each one's tab may take it over in `grace`, after which it is stopped. One this run's manifest
    /// doesn't allow is stopped now.
    #[cfg(unix)]
    pub(super) fn take_jobs(self: &Arc<Self>, jobs: Vec<LeftJob>, grace: Duration) {
        if jobs.is_empty() {
            return;
        }
        let until = Instant::now() + grace;
        for left in jobs {
            let Some((job, events, running)) = server::take(self, &left) else {
                continue;
            };
            let id = format!("left-{}", self.next_orphan.fetch_add(1, Ordering::Relaxed));
            let token = left.info.owner["token"].as_str().unwrap_or_default().to_owned();
            self.orphans.lock().unwrap().push(Orphan { id, token, running: running.clone(), until });
            server::follow(running, job, events);
        }
        self.stop_left_after(grace);
    }

    /// Only a Unix app has a terminal server.
    #[cfg(not(unix))]
    pub(super) fn take_jobs(self: &Arc<Self>, _jobs: Vec<LeftJob>, _grace: Duration) {}

    /// Lets go of the programs the terminal server runs as Wings quits: they run on for its next run.
    pub(super) fn let_go_of_jobs(&self) {
        let processes = self.processes.lock().unwrap();
        let orphans = self.orphans.lock().unwrap();
        for running in processes.values().chain(orphans.iter().map(|orphan| &orphan.running)) {
            running.process.let_go();
        }
    }

    fn stop_left_after(self: &Arc<Self>, grace: Duration) {
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

#[cfg(test)]
mod chat_claude_tests {
    use super::is_chat_claude;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn only_a_chats_claude_code_gets_the_notifications_variable() {
        let chat = args(&["-p", "--input-format=stream-json", "--output-format=stream-json", "--verbose"]);
        assert!(is_chat_claude("claude", &chat));
        assert!(!is_chat_claude("git", &chat), "another program a plugin runs");
        assert!(!is_chat_claude("claude", &args(&["auth", "status"])), "claude for something else");
        assert!(!is_chat_claude("claude", &args(&["-p", "--output-format=stream-json"])), "a one-shot run");
    }
}
