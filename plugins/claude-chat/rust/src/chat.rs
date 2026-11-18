//! The chat page: it runs Claude Code headless in the tab's folder (`claude -p` with stream-json both ways and its
//! permission prompts sent here), shows the session as it goes, and sends what you type. Opened with `{ resume, cwd,
//! program }` it continues a session that ran in a terminal. The tab keeps the session's id, so after Wings restarts it
//! continues that session again.

use std::cell::RefCell;

use serde_json::{Value, json};
use wings_plugin::{
    format::basename,
    host::{self, ProcessInput},
    task,
};

use crate::session::{Item, Session};

/// What a chat may run Claude Code as: Claude Code itself, or the wrapper a terminal started it through.
const PROGRAMS: [&str; 2] = ["claude", "claude-rtk"];
/// How many of the newest items the chat shows at first, and how many more each Show earlier adds.
pub const PAGE: usize = 80;
/// How many entries of a resumed session's transcript are read back.
const HISTORY: u32 = 400;

#[derive(Debug, PartialEq)]
pub enum State {
    Starting,
    Running,
    /// Claude Code ended, as it does when its input closes.
    Ended,
    /// It couldn't start, or stopped on an error: why.
    Failed(String),
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
    input: Option<ProcessInput>,
    interrupts: u32,
}

thread_local! {
    static CHAT: RefCell<Chat> = RefCell::new(Chat {
        session: Session::default(),
        draft: String::new(),
        shown: PAGE,
        folder: String::new(),
        state: State::Starting,
        input: None,
        interrupts: 0,
    });
}

pub fn with<T>(change: impl FnOnce(&mut Chat) -> T) -> T {
    CHAT.with(|chat| change(&mut chat.borrow_mut()))
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run_filled(crate::view::view);
    task::spawn(begin());
}

async fn begin() {
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
    with(|c| c.folder = basename(&cwd).to_owned());
    if let Some(id) = &resume {
        if let Ok(entries) = host::transcript(id, &["user", "assistant"], Some(HISTORY)).await {
            with(|c| entries.as_array().into_iter().flatten().for_each(|entry| c.session.hear(entry)));
        }
        with(|c| c.session.id = Some(id.clone()));
    }
    // Each value joined to its flag: Wings passes a program nothing else.
    let mut claude: Vec<String> = ["-p", "--input-format=stream-json", "--output-format=stream-json", "--verbose", "--include-partial-messages", "--permission-prompt-tool=stdio"]
        .map(str::to_owned)
        .to_vec();
    claude.extend(resume.as_ref().map(|id| format!("--resume={id}")));
    let claude: Vec<&str> = claude.iter().map(String::as_str).collect();
    let process = host::spawn(&program, &claude, &cwd, hear);
    with(|c| {
        c.input = Some(process.input());
        c.state = State::Running;
    });
    let ended = match process.await {
        Ok(exit) if exit["code"] == 0 => State::Ended,
        Ok(exit) => State::Failed(why(&exit)),
        Err(error) => State::Failed(error),
    };
    with(|c| {
        c.state = ended;
        c.session.working = false;
        c.input = None;
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

/// One line Claude Code printed. The tab keeps the session's id once Claude Code names it.
fn hear(line: &Value) {
    let named = with(|c| {
        let before = c.session.id.clone();
        c.session.hear(line);
        c.session.id.clone().filter(|id| Some(id) != before.as_ref())
    });
    if let Some(id) = named {
        drop(host::call("setTabState", json!({ "state": { "sessionId": id } })));
    }
}

fn write(input: ProcessInput, line: String) {
    task::spawn(async move {
        if let Err(error) = input.write(&line).await {
            with(|c| c.session.items.push(Item::Error(format!("Claude Code didn't get that: {error}"))));
        }
    });
}

/// Sends the draft as your message.
pub fn send() {
    let sent = with(|c| {
        let text = c.draft.trim().to_owned();
        let input = c.input.clone().filter(|_| !text.is_empty())?;
        c.draft.clear();
        Some((input, c.session.send(&text)))
    });
    if let Some((input, line)) = sent {
        write(input, line);
    }
}

/// Allows or denies what Claude asked to do.
pub fn answer(request: &str, allow: bool) {
    let answered = with(|c| Some((c.input.clone()?, c.session.answer(request, allow)?)));
    if let Some((input, line)) = answered {
        write(input, line);
    }
}

/// Asks Claude to stop the turn it is on.
pub fn stop() {
    let asked = with(|c| {
        c.interrupts += 1;
        let input = c.input.clone()?;
        Some((input, c.session.interrupt(c.interrupts)))
    });
    if let Some((input, line)) = asked {
        write(input, line);
    }
}
