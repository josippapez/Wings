//! The chat page: it runs Claude Code headless in the tab's folder (`claude -p` with stream-json both ways and its
//! permission prompts sent here), shows the session as it goes, and sends what you type. Opened with `{ resume, cwd,
//! program }` it continues a session that ran in a terminal. The tab keeps the session's id, so after Wings restarts it
//! continues that session again. Past sessions of the project open in the same tab, and so does a new chat.
//!
//! Everything the chat writes to Claude Code is your doing (a message, an answer, a pick, Stop), which Wings checks:
//! what it asks as Claude Code starts goes with the start.

use std::{cell::RefCell, collections::HashSet};

use serde_json::{Value, json};
use wings_plugin::{
    format::basename,
    host::{self, ProcessInput},
    task,
};

use crate::session::{Command, Item, Session};

/// What a chat may run Claude Code as: Claude Code itself, or the wrapper a terminal started it through.
const PROGRAMS: [&str; 2] = ["claude", "claude-rtk"];
/// How many of the newest items the chat shows at first, and how many more each Show earlier adds.
pub const PAGE: usize = 80;
/// How many entries of a resumed session's transcript are read back.
const HISTORY: u32 = 400;
/// How many past sessions the list shows.
const PAST: u32 = 60;

#[derive(Debug, PartialEq)]
pub enum State {
    Starting,
    Running,
    /// Claude Code ended, as it does when its input closes.
    Ended,
    /// It couldn't start, or stopped on an error: why.
    Failed(String),
    /// The session went on in a terminal.
    Moved,
}

/// The project's past sessions, while their list is open.
#[derive(Default)]
pub struct Past {
    pub query: String,
    /// None while they load.
    pub sessions: Option<Vec<Value>>,
    pub error: Option<String>,
}

pub struct Chat {
    pub session: Session,
    /// The message being typed.
    pub draft: String,
    /// How many of the newest items are shown.
    pub shown: usize,
    /// The name of the folder Claude Code runs in.
    pub folder: String,
    pub state: State,
    pub past: Option<Past>,
    /// The groups of tool calls you opened, by the index of their first item.
    pub open_steps: HashSet<usize>,
    /// The tool calls whose output you opened, by their id.
    pub open_outputs: HashSet<String>,
    /// Going on in a terminal once Claude Code here has ended.
    moving: bool,
    input: Option<ProcessInput>,
    /// Each request the chat writes gets the next number, for its id.
    requests: u32,
    /// Which start of Claude Code the page shows: one started before it is let go of, and its lines are dropped.
    run: u32,
    cwd: String,
    program: String,
}

thread_local! {
    static CHAT: RefCell<Chat> = RefCell::new(Chat {
        session: Session::default(),
        draft: String::new(),
        shown: PAGE,
        folder: String::new(),
        state: State::Starting,
        past: None,
        open_steps: HashSet::new(),
        open_outputs: HashSet::new(),
        moving: false,
        input: None,
        requests: 0,
        run: 0,
        cwd: String::new(),
        program: "claude".into(),
    });
}

pub fn with<T>(change: impl FnOnce(&mut Chat) -> T) -> T {
    CHAT.with(|chat| change(&mut chat.borrow_mut()))
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run_filled(crate::view::view);
    task::spawn(open());
}

/// Finds the folder and the session the tab is for, and starts Claude Code there.
async fn open() {
    let args = host::view_args().unwrap_or(Value::Null);
    let saved = host::call("tabState", json!({})).await.unwrap_or(Value::Null);
    let resume = saved["sessionId"].as_str().or(args["resume"].as_str()).map(str::to_owned);
    let cwd = match args["cwd"].as_str() {
        Some(cwd) => cwd.to_owned(),
        None => match host::call("project", json!({})).await.ok().and_then(|project| project["path"].as_str().map(str::to_owned)) {
            Some(path) => path,
            None => return with(|c| c.state = State::Failed("Open a project first. A chat runs Claude Code in a project's folder.".into())),
        },
    };
    let program = args["program"].as_str().filter(|program| PROGRAMS.contains(program)).unwrap_or("claude").to_owned();
    with(|c| {
        c.folder = basename(&cwd).to_owned();
        c.cwd = cwd;
        c.program = program;
    });
    launch(resume);
}

/// Starts Claude Code anew in the tab, going on with `resume` or in a new session, in place of the one running.
fn launch(resume: Option<String>) {
    let (run, cwd, program) = with(|c| {
        if let Some(input) = c.input.take() {
            input.stop();
        }
        c.run += 1;
        c.session = Session::default();
        c.state = State::Starting;
        c.shown = PAGE;
        c.past = None;
        c.open_steps.clear();
        c.open_outputs.clear();
        c.moving = false;
        (c.run, c.cwd.clone(), c.program.clone())
    });
    task::spawn(async move {
        if let Some(id) = &resume {
            if let Ok(entries) = host::transcript(id, &["user", "assistant"], Some(HISTORY)).await {
                with(|c| {
                    if c.run == run {
                        entries.as_array().into_iter().flatten().for_each(|entry| c.session.hear(entry));
                    }
                });
            }
            with(|c| c.session.id = Some(id.clone()));
            drop(host::call("setTabState", json!({ "state": { "sessionId": id } })));
        }
        // Each value joined to its flag: Wings passes a program nothing else.
        let mut args: Vec<String> = ["-p", "--input-format=stream-json", "--output-format=stream-json", "--verbose", "--include-partial-messages", "--permission-prompt-tool=stdio"]
            .map(str::to_owned)
            .to_vec();
        args.extend(resume.as_ref().map(|id| format!("--resume={id}")));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let process = host::spawn(&program, &args, &cwd, &Session::opening(), move |line| hear(run, line));
        let current = with(|c| {
            if c.run != run {
                return false;
            }
            c.input = Some(process.input());
            c.state = State::Running;
            true
        });
        if !current {
            return process.input().stop();
        }
        let ended = match process.await {
            Ok(exit) if exit["code"] == 0 => State::Ended,
            Ok(exit) => State::Failed(why(&exit)),
            Err(error) => State::Failed(error),
        };
        let moving = with(|c| {
            if c.run != run {
                return false;
            }
            c.session.working = false;
            c.input = None;
            if c.moving {
                return true;
            }
            c.state = ended;
            false
        });
        if moving {
            open_terminal();
        }
    });
}

/// Goes on with the session in a terminal, as Claude Code itself: once Claude Code here has ended, so that only one
/// writes to the session.
pub fn to_terminal() {
    let now = with(|c| {
        c.session.id.as_ref()?;
        c.moving = true;
        match c.input.take() {
            Some(input) => {
                input.stop();
                Some(false)
            }
            None => Some(true),
        }
    });
    if now == Some(true) {
        open_terminal();
    }
}

fn open_terminal() {
    let Some((id, cwd, program)) = with(|c| Some((c.session.id.clone()?, c.cwd.clone(), c.program.clone()))) else {
        return;
    };
    task::spawn(async move {
        let opened = host::call("openPane", json!({ "command": format!("{program} --resume"), "args": [id], "cwd": cwd, "placement": "tab" })).await;
        with(|c| {
            c.moving = false;
            c.state = match opened {
                Ok(_) => State::Moved,
                Err(error) => State::Failed(format!("Couldn't open a terminal: {error}")),
            };
        });
    });
}

/// The end of what Claude Code printed to stderr, or its exit code.
fn why(exit: &Value) -> String {
    let said = exit["stderr"].as_str().unwrap_or_default().lines().rev().find(|line| !line.trim().is_empty()).map(str::trim);
    match said {
        Some(line) => format!("Claude Code stopped: {line}"),
        None => format!("Claude Code stopped with code {}", exit["code"]),
    }
}

/// One line Claude Code printed, from the start `run`. The tab keeps the session's id once Claude Code names it.
fn hear(run: u32, line: &Value) {
    let named = with(|c| {
        if c.run != run {
            return None;
        }
        let before = c.session.id.clone();
        c.session.hear(line);
        c.session.id.clone().filter(|id| Some(id) != before.as_ref())
    });
    if let Some(id) = named {
        drop(host::call("setTabState", json!({ "state": { "sessionId": id } })));
    }
}

/// Writes what `make` makes of the chat and the next request number, while Claude Code runs.
fn write(make: impl FnOnce(&mut Chat, u32) -> Option<String>) {
    let written = with(|c| {
        let input = c.input.clone()?;
        c.requests += 1;
        let n = c.requests;
        Some((input, make(c, n)?))
    });
    let Some((input, line)) = written else {
        return;
    };
    task::spawn(async move {
        if let Err(error) = input.write(&line).await {
            with(|c| c.session.items.push(Item::Error(format!("Claude Code didn't get that: {error}"))));
        }
    });
}

/// Sends the draft as your message.
pub fn send() {
    write(|c, _| {
        let text = c.draft.trim().to_owned();
        if text.is_empty() {
            return None;
        }
        c.draft.clear();
        Some(c.session.send(&text))
    });
}

/// Allows or denies what Claude asked to do.
pub fn answer(request: &str, allow: bool) {
    write(|c, _| c.session.answer(request, allow));
}

/// Asks Claude to stop the turn it is on.
pub fn stop() {
    write(|c, n| Some(c.session.interrupt(n)));
}

pub fn pick_model(model: &str) {
    write(|c, n| Some(c.session.set_model(model, n)));
}

pub fn pick_effort(effort: &str) {
    write(|c, n| Some(c.session.set_effort(effort, n)));
}

pub fn pick_mode(mode: &str) {
    write(|c, n| Some(c.session.set_mode(mode, n)));
}

/// Puts the command in the message box, to add what it takes.
pub fn insert_command(name: &str) {
    with(|c| c.draft = format!("/{name} "));
}

/// The commands whose name holds what the draft has after its `/`, while that is one word.
pub fn command_matches(c: &Chat) -> Vec<&Command> {
    let Some(typed) = c.draft.strip_prefix('/').filter(|typed| !typed.contains(char::is_whitespace)) else {
        return Vec::new();
    };
    let typed = typed.to_lowercase();
    c.session.commands.iter().filter(|command| command.name.to_lowercase().contains(&typed)).take(8).collect()
}

pub fn new_chat() {
    launch(None);
}

pub fn resume(id: &str) {
    launch(Some(id.to_owned()));
}

/// Opens the list of the project's past sessions, or closes it.
pub fn toggle_past() {
    let open = with(|c| {
        c.past = match c.past.take() {
            Some(_) => None,
            None => Some(Past::default()),
        };
        c.past.is_some()
    });
    if open {
        load_past(String::new());
    }
}

/// Lists the past sessions that match `query`.
pub fn load_past(query: String) {
    with(|c| {
        if let Some(past) = &mut c.past {
            past.query = query.clone();
        }
    });
    task::spawn(async move {
        let found = host::past_sessions(&query, PAST).await;
        with(|c| {
            let Some(past) = c.past.as_mut().filter(|past| past.query == query) else {
                return;
            };
            match found {
                Ok(list) => past.sessions = Some(list.as_array().cloned().unwrap_or_default()),
                Err(error) => past.error = Some(error),
            }
        });
    });
}

pub fn toggle_output(id: &str) {
    with(|c| {
        if !c.open_outputs.remove(id) {
            c.open_outputs.insert(id.to_owned());
        }
    });
}

pub fn toggle_steps(first: usize) {
    with(|c| {
        if !c.open_steps.remove(&first) {
            c.open_steps.insert(first);
        }
    });
}
