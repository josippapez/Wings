//! The chat page: it runs Claude Code headless in the tab's folder (`claude -p` with stream-json both ways and its
//! permission prompts sent here), shows the session as it goes, and sends what you type. Opened with `{ resume, cwd,
//! program }` it continues a session that ran in a terminal. The tab keeps the session's id, so after Wings restarts it
//! continues that session again. Past sessions of the project open in the same tab, and so does a new chat.
//!
//! Everything the chat writes to Claude Code is your doing (a message, an answer, a pick, Stop), which Wings checks:
//! what it asks as Claude Code starts goes with the start.

use std::{cell::RefCell, collections::{HashMap, HashSet}};

use serde_json::{Value, json};
use wings_plugin::{
    format::{basename, truncate},
    host::{self, ProcessInput},
    task,
};

use crate::{
    mentions::{self, Files},
    session::{Command, Draft, Item, Session},
};

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
    /// The project's files, for @-mentions.
    pub files: Files,
    /// What you have picked and typed on the cards that wait for an answer, by request.
    pub drafts: HashMap<String, Draft>,
    /// The checklist above the message box is folded to its summary line.
    pub todos_folded: bool,
    /// What Claude is doing, for the status line, and since when (ms).
    pub doing: (String, u64),
    /// What the sidebar's Agents was last told of this chat: its state, name and what it does.
    agent: Option<(String, String, Option<String>)>,
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
        files: Files::default(),
        drafts: HashMap::new(),
        todos_folded: false,
        doing: (String::new(), 0),
        agent: None,
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
        c.drafts.clear();
        c.moving = false;
        (c.run, c.cwd.clone(), c.program.clone())
    });
    load_files(run, cwd.clone());
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
        let mut args: Vec<String> = ["-p", "--input-format=stream-json", "--output-format=stream-json", "--verbose", "--include-partial-messages", "--permission-prompt-tool=stdio", "--replay-user-messages"]
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
            // The session goes on in a terminal, which the sidebar lists by itself.
            with(|c| c.agent = None);
            drop(host::set_agent(None));
            open_terminal();
        } else {
            report_agent();
        }
    });
}

/// Lists the project's files for @-mentions: git's own list, so ignored files stay out. Done here, not by Claude Code's
/// file suggestions, because Wings lets the page write to Claude Code only on a click or a key.
fn load_files(run: u32, cwd: String) {
    task::spawn(async move {
        let listed = host::exec_in(&cwd, "git", &["ls-files", "--cached", "--others", "--exclude-standard", "-z"]).await;
        // Not a git folder, or no git: no menu, and nothing to tell.
        let files = listed.ok().filter(|out| out["code"] == 0).map(|out| Files::from_git(out["stdout"].as_str().unwrap_or_default()));
        with(|c| {
            if c.run == run {
                c.files = files.unwrap_or_default();
            }
        });
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
    let (named, alerts) = with(|c| {
        if c.run != run {
            return (None, Vec::new());
        }
        let before = c.session.id.clone();
        c.session.hear(line);
        let doing = c.session.activity();
        if doing != c.doing.0 {
            c.doing = (doing, host::now() as u64);
        }
        (c.session.id.clone().filter(|id| Some(id) != before.as_ref()), std::mem::take(&mut c.session.alerts))
    });
    // Only for what you'd miss: someone looking at this tab sees it happen.
    if !host::focused() {
        for (title, body) in alerts {
            drop(host::notify(&title, &body));
        }
    }
    if let Some(id) = named {
        drop(host::call("setTabState", json!({ "state": { "sessionId": id } })));
    }    report_agent();
}

/// Tells the sidebar's Agents what Claude does in this chat, when that changed: it needs you while a question of
/// Claude's waits, works while a turn runs, and is done once one ended while you were looking elsewhere.
fn report_agent() {
    let agent = with(|c| {
        let asking = c.session.items.iter().find_map(|item| match item {
            Item::Ask(ask) if ask.answer.is_none() => Some(ask.tool.clone()),
            _ => None,
        });
        let (state, summary) = match asking {
            Some(tool) => ("blocked", Some(format!("Asks to use {tool}"))),
            None if c.session.working => ("working", Some(c.doing.0.clone()).filter(|doing| !doing.is_empty())),
            None if c.session.items.is_empty() || host::focused() => ("idle", None),
            None => ("done", None),
        };
        let name = c
            .session
            .items
            .iter()
            .find_map(|item| match item {
                Item::User(said) => Some(truncate(said.lines().next().unwrap_or_default().trim(), 60)),
                _ => None,
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Claude chat".into());
        let agent = (state.to_owned(), name, summary);
        (c.agent.as_ref() != Some(&agent)).then(|| {
            c.agent = Some(agent.clone());
            agent
        })
    });
    if let Some((state, name, summary)) = agent {
        drop(host::set_agent(Some((&state, &name, summary.as_deref()))));
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

/// Sends the draft as your message, or queues it while Claude works.
pub fn send() {
    write(|c, n| {
        let text = c.draft.trim().to_owned();
        if text.is_empty() {
            return None;
        }
        c.draft.clear();
        if c.session.working {
            return Some(c.session.queue(&text, host::now() as u64, n));
        }
        let line = c.session.send(&text);
        // The status line's time counts from here, not from the end of the turn before.
        c.doing = (c.session.activity(), host::now() as u64);
        Some(line)
    });
    report_agent();
}

/// Takes a queued message back.
pub fn unqueue(uuid: &str) {
    write(|c, n| c.session.unqueue(uuid, n));
}

/// Has Claude read a queued message at its next step instead of after the turn.
pub fn send_now(uuid: &str) {
    write(|c, n| c.session.send_now(uuid, host::now() as u64, n));
}

/// Changes what you have picked or typed on the card for `request`.
pub fn edit(request: &str, change: impl FnOnce(&mut Draft)) {
    with(|c| change(c.drafts.entry(request.to_owned()).or_default()));
}

/// Allows what Claude asked to do, once or, with `always`, for the rules Claude Code suggested too.
pub fn allow(request: &str, always: bool) {
    write(|c, _| {
        c.drafts.remove(request);
        c.session.allow(request, always)
    });
}

/// Denies it, with the words you wrote for Claude.
pub fn deny(request: &str) {
    write(|c, _| {
        let draft = c.drafts.remove(request).unwrap_or_default();
        c.session.deny(request, &draft.feedback)
    });
}

/// Answers the questions Claude asked with what you picked and typed.
pub fn submit(request: &str) {
    write(|c, _| {
        let draft = c.drafts.get(request)?;
        let line = c.session.submit(request, draft)?;
        c.drafts.remove(request);
        Some(line)
    });
}

/// Declines to answer the questions.
pub fn skip(request: &str) {
    write(|c, _| {
        c.drafts.remove(request);
        c.session.deny(request, "The user skipped these questions without answering.")
    });
}

/// Approves the plan and goes on in `mode`.
pub fn approve_plan(request: &str, mode: &str) {
    write(|c, n| {
        c.drafts.remove(request);
        c.session.approve_plan(request, mode, n)
    });
}

/// Sends the plan back, with what you want changed.
pub fn keep_planning(request: &str) {
    write(|c, _| {
        let draft = c.drafts.remove(request).unwrap_or_default();
        let said = if draft.feedback.trim().is_empty() { "The user wants you to keep planning. Don't start the work yet." } else { draft.feedback.as_str() };
        c.session.deny(request, said)
    });
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

/// The files that match the `@word` the draft ends in, with where the word starts. None while a slash command is being
/// typed: that menu wins.
pub fn mention_matches(c: &Chat) -> Option<(usize, Vec<&str>)> {
    if c.draft.starts_with('/') {
        return None;
    }
    let (start, query) = mentions::active(&c.draft)?;
    let found = c.files.matches(query);
    (!found.is_empty()).then_some((start, found))
}

/// Replaces the `@word` the draft ends in with a mention of `path`.
pub fn insert_mention(path: &str) {
    with(|c| {
        if let Some((start, _)) = mentions::active(&c.draft) {
            c.draft = mentions::insert(&c.draft, start, path);
        }
    });
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

pub fn toggle_todos() {
    with(|c| c.todos_folded = !c.todos_folded);
}

pub fn toggle_steps(first: usize) {
    with(|c| {
        if !c.open_steps.remove(&first) {
            c.open_steps.insert(first);
        }
    });
}
