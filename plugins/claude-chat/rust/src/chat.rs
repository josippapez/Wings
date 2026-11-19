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
    messages, task,
};

use crate::{
    images,
    commands,
    fork::{self, Launch},
    fork_view,
    mentions::{self, Files},
    plugins, prefs,
    composer::{self, Escape},
    session::{self, Command, Draft, Item, Restore, Session, exits, view_of},
    setup,
    usage::Panel,
};

/// What a chat may run Claude Code as: Claude Code itself, or the wrapper a terminal started it through.
const PROGRAMS: [&str; 2] = ["claude", "claude-rtk"];
/// How many of the newest items the chat shows at first, and how many more each scroll up to the top adds.
pub const PAGE: usize = 80;
/// How many entries of a resumed session's transcript are read back.
pub(crate) const HISTORY: u32 = 400;
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

/// The project's past sessions, while their sheet is open.
#[derive(Default)]
pub struct Past {
    pub query: String,
    /// None while they load.
    pub sessions: Option<Vec<Value>>,
    pub error: Option<String>,
    /// How recently a listed one was last active: `today`, `week` or `month`, and any time when empty.
    pub since: String,
    /// The branch a listed one ran on, or any.
    pub branch: Option<String>,
}

/// A terminal Claude opened in Wings, which Wings draws where the page puts its `Terminal` node.
#[derive(Debug, PartialEq)]
pub struct Terminal {
    pub pane: String,
}

impl Terminal {
    fn of(listed: &Value) -> Option<Terminal> {
        Some(Terminal { pane: listed["paneId"].as_str()?.to_owned() })
    }
}

pub struct Chat {
    pub session: Session,
    /// The message being typed.
    pub draft: String,
    /// The suggestion you put in the message box with Tab: editing it keeps the suggestion, so emptying the box shows it
    /// again.
    took_suggestion: Option<String>,
    /// The pictures pasted into the message, as (media type, base64), sent with it.
    pub images: Vec<(String, String)>,
    /// How many of the newest items are shown.
    pub shown: usize,
    /// The name of the folder Claude Code runs in.
    pub folder: String,
    pub state: State,
    pub past: Option<Past>,
    /// Your setting for how tool calls show: Compact folds them, Detailed opens every run and each call's output.
    pub detailed: bool,
    /// Your setting for the conversation's text size, as the `textScale` its scroll takes.
    pub text_scale: f32,
    /// The usage or context dialog, over the chat while it is open, and whether the usage dialog lists the last week's
    /// contributors in place of the last day's.
    pub panel: Option<Panel>,
    pub panel_week: bool,
    /// The plugins dialog, over the chat while it is open.
    pub plugins: Option<plugins::Dialog>,
    /// The picture shown large in a dialog, by id.
    pub picture: Option<String>,
    /// The groups of tool calls you clicked, by the index of their first item: each flips what `detailed` shows.
    pub open_steps: HashSet<usize>,
    /// The tool calls whose output you clicked, by their id: each flips what `detailed` shows.
    pub open_outputs: HashSet<String>,
    /// The queued messages shown in full, by uuid.
    pub open_queued: HashSet<String>,
    /// The project's files, for @-mentions.
    pub files: Files,
    /// What you have picked and typed on the cards that wait for an answer, by request.
    pub drafts: HashMap<String, Draft>,
    /// The checklist above the message box is folded to its summary line.
    pub todos_folded: bool,
    /// What Claude is doing, for the status line, and since when (ms).
    pub doing: (String, u64),
    /// The background task whose output is open in a dialog, by its id.
    pub task: Option<String>,
    /// The folders you added with /add-dir, which Claude Code is let into each time it starts here, and the tab keeps.
    pub dirs: Vec<String>,
    /// The message the Fork from here dialog is about, by its uuid.
    pub fork_from: Option<String>,
    /// The header's Fork is asking before it forks.
    pub fork_ask: bool,
    /// Where this chat was forked from, as the note at its top says: the name of the original session.
    pub forked_from: Option<String>,
    /// Whether the tab is narrower than the side panel and the conversation need side by side, as the page's split says: None
    /// until it has measured. Then the panel floats over the conversation, and Escape or a press outside puts it away.
    pub narrow: Option<bool>,
    /// Whether the side panel shows: your choice for every chat (`prefs::side_panel`, on by default), which Escape or a press
    /// outside a panel over a narrow tab sets off for this tab only, until the header's button.
    pub side_open: bool,
    /// The background agent a message is being written to, by its id, and the message.
    pub message_to: Option<(String, String)>,
    /// The lines added and removed in the working tree since the last commit, and the branch it is on.
    pub changes: Option<(u64, u64)>,
    /// A count of the changes runs, and the one asked for while it did, which runs after it.
    counting: bool,
    count_next: Option<(u32, String)>,
    pub branch: Option<String>,
    /// The terminals Claude opened with Wings' `open_pane`, as Wings lists them: shown in the side panel.
    pub terminals: Vec<Terminal>,
    /// What the sidebar's Agents was last told of this chat: its state, name and what it does.
    agent: Option<(String, String, Option<String>)>,
    /// What the tab was last named, none for its view's title.
    titled: Option<String>,
    /// Going on in a terminal once Claude Code here has ended.
    moving: bool,
    input: Option<ProcessInput>,
    /// Each request the chat writes gets the next number, for its id.
    requests: u32,
    /// Which start of Claude Code the page shows: one started before it is let go of, and its lines are dropped.
    pub(crate) run: u32,
    /// The folder Claude Code runs in.
    pub cwd: String,
    program: String,
}

thread_local! {
    static CHAT: RefCell<Chat> = RefCell::new(Chat {
        session: Session::default(),
        draft: String::new(),
        took_suggestion: None,
        images: Vec::new(),
        shown: PAGE,
        folder: String::new(),
        state: State::Starting,
        past: None,
        detailed: false,
        text_scale: 1.0,
        panel: None,
        panel_week: false,
        plugins: None,
        picture: None,
        open_steps: HashSet::new(),
        open_outputs: HashSet::new(),
        open_queued: HashSet::new(),
        files: Files::default(),
        drafts: HashMap::new(),
        todos_folded: false,
        doing: (String::new(), 0),
        task: None,
        dirs: Vec::new(),
        fork_from: None,
        fork_ask: false,
        forked_from: None,
        narrow: None,
        side_open: true,
        message_to: None,
        changes: None,
        counting: false,
        count_next: None,
        branch: None,
        terminals: Vec::new(),
        agent: None,
        titled: None,
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

#[cfg(not(feature = "subagent"))]
#[unsafe(no_mangle)]
pub fn start() {
    // A subagent's tab is a page of this view too, opened with the subagent in its args.
    if crate::subagent_tab::start() {
        return;
    }
    wings_plugin::run_filled(crate::view::view);
    crate::subagent_relay::start();
    with(|c| {
        c.detailed = prefs::detailed().get();
        c.text_scale = prefs::scale_of(&prefs::text_size().get());
        c.side_open = prefs::side_panel().get();
    });
    prefs::text_size().on_change(|size| with(|c| c.text_scale = prefs::scale_of(size)));
    prefs::side_panel().on_change(|shown| with(|c| c.side_open = *shown));
    host::on_terminals(|listed| with(|c| c.terminals = listed.iter().filter_map(Terminal::of).collect()));
    // The clicks were made against the old setting, so they go with it.
    prefs::detailed().on_change(|detailed| {
        with(|c| {
            c.detailed = *detailed;
            c.open_steps.clear();
            c.open_outputs.clear();
        })
    });
    task::spawn(open());
}

/// Finds the folder and the session the tab is for, and starts Claude Code there.
async fn open() {
    let args = host::view_args().unwrap_or(Value::Null);
    let saved = host::call("tabState", json!({})).await.unwrap_or(Value::Null);
    let resume = saved["sessionId"].as_str().or(args["resume"].as_str()).map(str::to_owned);
    let dirs: Vec<String> = saved["dirs"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
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
        c.dirs = dirs;
    });
    // After an update or restart of the plugin, the Claude Code this tab ran is still going: the chat takes it over.
    if crate::reattach::take_over(resume.clone()).await {
        return;
    }
    launch(resume);
}

/// Starts Claude Code anew in the tab, going on with `resume` or in a new session, in place of the one running.
fn launch(resume: Option<String>) {
    launch_with(resume, Launch::default());
}

/// `launch`, as a copy of the session when `how` says so, and with the folders you added.
pub fn launch_with(resume: Option<String>, how: Launch) {
    let (run, cwd, program, start_mode, dirs) = with(|c| {
        if how.fork {
            c.forked_from = Some(name(c).unwrap_or_else(|| "an earlier session".into()));
        }
        let run = begin(c);
        (run, c.cwd.clone(), c.program.clone(), c.session.start_mode.clone(), c.dirs.clone())
    });
    load_files(run, cwd.clone());
    load_git(run, cwd.clone());
    task::spawn(async move {
        if let Some(id) = &resume {
            if let Ok(entries) = host::transcript_with_images(id, &["user", "assistant", "last-prompt"], Some(HISTORY)).await {
                with(|c| {
                    if c.run == run {
                        entries.as_array().into_iter().flatten().for_each(|entry| c.session.hear(entry));
                    }
                });
            }
            with(|c| c.session.id = Some(id.clone()));
        }
        save_tab();
        // Each value joined to its flag: Wings passes a program nothing else.
        let mut args: Vec<String> = ["-p", "--input-format=stream-json", "--output-format=stream-json", "--verbose", "--include-partial-messages", "--permission-prompt-tool=stdio", "--replay-user-messages", "--prompt-suggestions=true"]
            .map(str::to_owned)
            .to_vec();
        args.extend(fork::flags(resume.as_deref(), &how, &dirs));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        // Under the account the session's transcript is in, which a plain `claude --resume` would not find if it isn't the
        // default one; a new chat gets its project's, as a terminal there does.
        let account = host::Account::of_session(resume.as_deref());
        // A fork before a message asks for its rewind as it starts, as the opening requests do: the fork is a new session.
        let opening = Session::opening(start_mode.as_deref()) + &with(|c| fork::opening(&how, &c.session.prompts));
        let process = host::spawn_as(&account, &program, &args, &cwd, &opening, move |line| hear(run, line));
        crate::reattach::started(run, process.input());
        follow(run, process).await;
    });
}

/// Starts the chat over for a new start of Claude Code, in place of the one running: the number of that start.
pub(crate) fn begin(c: &mut Chat) -> u32 {
    if let Some(input) = c.input.take() {
        input.stop();
    }
    c.run += 1;
    composer::forget();
    c.session = Session::default();
    c.session.start_mode = prefs::start_mode();
    c.state = State::Starting;
    c.shown = PAGE;
    c.past = None;
    c.panel = None;
    c.plugins = None;
    c.picture = None;
    c.open_steps.clear();
    c.open_outputs.clear();
    c.open_queued.clear();
    c.drafts.clear();
    c.task = None;
    c.fork_from = None;
    c.fork_ask = false;
    c.message_to = None;
    c.moving = false;
    c.run
}

/// Follows the Claude Code of the start `run` until it ends.
pub(crate) async fn follow(run: u32, process: host::Process) {
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
        c.session.stopped_running();
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
}

/// What the tab keeps for when it opens again: the session, and the folders you added.
pub(crate) fn save_tab() {
    let (id, dirs) = with(|c| (c.session.id.clone(), c.dirs.clone()));
    drop(host::call("setTabState", json!({ "state": { "sessionId": id, "dirs": dirs } })));
}

/// Lists the project's files for @-mentions: git's own list, so ignored files stay out. Done here, not by Claude Code's
/// file suggestions, because Wings lets the page write to Claude Code only on a click or a key.
pub(crate) fn load_files(run: u32, cwd: String) {
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

/// What the working tree changed since the last commit, and the branch, for the side panel. Asked when the chat starts,
/// when a call that can change files ends, and when a turn ends, not on a timer. One count runs at a time: one asked for
/// meanwhile runs once after it.
pub(crate) fn load_git(run: u32, cwd: String) {
    let start = with(|c| {
        if c.counting {
            c.count_next = Some((run, cwd.clone()));
        }
        !std::mem::replace(&mut c.counting, true)
    });
    if !start {
        return;
    }
    task::spawn(async move {
        let (mut run, mut cwd) = (run, cwd);
        loop {
            let changes = counted(&cwd).await;
            let branch = host::exec_in(&cwd, "git", &["rev-parse", "--abbrev-ref", "HEAD"]).await;
            let next = with(|c| {
                if c.run == run {
                    c.changes = changes;
                    // A detached head has no branch to name.
                    c.branch = stdout(branch).filter(|name| !name.is_empty() && name != "HEAD");
                }
                let next = c.count_next.take();
                c.counting = next.is_some();
                next
            });
            let Some(next) = next else { break };
            (run, cwd) = next;
        }
    });
}

/// What a git call printed, when it succeeded.
fn stdout(out: Result<Value, String>) -> Option<String> {
    out.ok().filter(|out| out["code"] == 0).map(|out| out["stdout"].as_str().unwrap_or_default().trim().to_owned())
}

/// The lines added and removed, as the Changes plugin counts them for its sidebar: new files too. With Changes off, git's
/// count of the files it already tracks.
async fn counted(cwd: &str) -> Option<(u64, u64)> {
    let asked = host::call("callTool", json!({ "name": "changes__changed_files", "arguments": { "cwd": cwd } })).await;
    if let Some(counts) = asked.ok().filter(|answer| answer["isError"] != true).and_then(|answer| changes_counted(answer["text"].as_str()?)) {
        return Some(counts);
    }
    stdout(host::exec_in(cwd, "git", &["diff", "--shortstat", "HEAD"]).await).map(|out| shortstat(&out))
}

/// The totals in what Changes' `changed_files` answers: `{ "added": 12, "removed": 3, ... }`.
fn changes_counted(text: &str) -> Option<(u64, u64)> {
    let found: Value = serde_json::from_str(text).ok()?;
    Some((found["added"].as_u64()?, found["removed"].as_u64()?))
}

/// The lines added and removed in `git diff --shortstat`'s " 3 files changed, 590 insertions(+), 277 deletions(-)", which
/// leaves out a count that is zero, and is empty when nothing changed.
fn shortstat(out: &str) -> (u64, u64) {
    let count = |word: &str| out.split(',').find(|part| part.contains(word)).and_then(|part| part.split_whitespace().next()?.parse().ok()).unwrap_or(0);
    (count("insertion"), count("deletion"))
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
pub(crate) fn hear(run: u32, line: &Value) {
    let (named, alerts, ended) = with(|c| {
        if c.run != run {
            return (None, Vec::new(), false);
        }
        let before = c.session.id.clone();
        let was_working = c.session.working;
        c.session.now = host::now() as u64;
        c.session.hear(line);
        // A rewound conversation gives the message it went back before to the message box, to edit and send again.
        if let Some(said) = c.session.restored.take() {
            c.draft = said;
            let shown = c.session.items.len();
            c.open_steps.retain(|first| *first < shown);
        }
        // A cleared conversation takes what the page kept of it along: which folds were open, what was half answered.
        if std::mem::take(&mut c.session.cleared) {
            c.shown = PAGE;
            c.open_steps.clear();
            c.open_outputs.clear();
            c.open_queued.clear();
            c.drafts.clear();
            c.task = None;
        }
        let doing = c.session.activity();
        if doing != c.doing.0 {
            c.doing = (doing, host::now() as u64);
        }
        let named = c.session.id.clone().filter(|id| Some(id) != before.as_ref());
        (named, std::mem::take(&mut c.session.alerts), was_working && !c.session.working)
    });
    // Pictures in what was heard go to Wings once, as the page names them by id.
    images::flush();
    crate::subagent_relay::heard(line);
    // Only for what you'd miss: someone looking at this tab sees it happen.
    if !host::focused() {
        for (title, body) in alerts {
            drop(host::notify(&title, &body));
        }
    }
    if named.is_some() {
        save_tab();
    }
    // The sidebar's Chats list reads sessions from their transcripts, which Claude Code has written by the end of a turn.
    if ended {
        messages::tell(crate::SESSION_SAVED, &Value::Null);
    }
    // Files change in a turn, as each edit, command or subagent ends, and when the code is rewound.
    let touched = with(|c| c.run == run && std::mem::take(&mut c.session.files_touched));
    if touched || line["type"] == "result" || line["response"]["request_id"] == session::REWIND_FILES {
        if let Some(cwd) = with(|c| (c.run == run).then(|| c.cwd.clone())) {
            load_git(run, cwd);
        }
    }
    report_agent();
    crate::reattach::heard(run, line);
}

/// Tells the sidebar's Agents what Claude does in this chat, when that changed: it needs you while a question of
/// Claude's waits, works while a turn runs, and is done once one ended while you were looking elsewhere. And the tabs of
/// its subagents how they stand.
pub(crate) fn report_agent() {
    let (agent, retitled) = with(|c| {
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
        // The tab is named after the session too, as a terminal's tab shows what runs in it.
        let title = name(c);
        let retitled = (c.titled != title).then(|| {
            c.titled = title.clone();
            title.clone()
        });
        let agent = (state.to_owned(), title.unwrap_or_else(|| "Claude chat".into()), summary);
        let agent = (c.agent.as_ref() != Some(&agent)).then(|| {
            c.agent = Some(agent.clone());
            agent
        });
        (agent, retitled)
    });
    if let Some((state, name, summary)) = agent {
        drop(host::set_agent(Some((&state, &name, summary.as_deref()))));
    }
    if let Some(title) = retitled {
        drop(host::set_tab_title(title.as_deref()));
    }
    crate::subagent_relay::sync();
}

/// The session's name: the one Claude Code says (`/rename`), else the first line of your first message, cut short. None
/// before you have said anything.
pub fn name(c: &Chat) -> Option<String> {
    if let Some(title) = &c.session.title {
        return Some(truncate(title, 60));
    }
    c.session
        .items
        .iter()
        .find_map(|item| match item {
            Item::User(said) | Item::UserImages(said, _) => Some(truncate(said.lines().next().unwrap_or_default().trim(), 60)),
            _ => None,
        })
        .filter(|name| !name.is_empty())
}

/// Writes what `make` makes of the chat and the next request number, while Claude Code runs.
pub(crate) fn write(make: impl FnOnce(&mut Chat, u32) -> Option<String>) {
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
    // An answer or a queued message is the page's own to know: kept with Claude Code for the page after an update.
    crate::reattach::sync();
}

/// Sends the draft as your message, or queues it while Claude works.
pub fn send() {
    // /exit ends the session, as in Claude Code: the tab closes, and the Claude Code it runs stops with it.
    if with(|c| c.images.is_empty() && exits(&c.draft)) {
        drop(host::close_tab());
        return;
    }
    // /add-dir starts Claude Code again with the folder, and isn't sent as a message.
    if let Some(path) = with(|c| fork::add_dir_command(&c.draft).filter(|_| c.images.is_empty()).map(str::to_owned)) {
        return fork_view::add_dir(path);
    }
    write(|c, n| {
        let text = c.draft.trim().to_owned();
        if text.is_empty() && c.images.is_empty() {
            return None;
        }
        // /usage, /cost and /context open a dialog here, and aren't sent as a message, not even queued.
        if c.images.is_empty() {
            if let Some((panel, line)) = c.session.view_command(&text) {
                c.draft.clear();
                c.past = None;
                c.panel = Some(panel);
                return Some(line);
            }
            // /plugins, /plugin and /mcp open their dialog over the chat, filled by what the line asks.
            if let Some((show, line)) = c.session.dialog_command(&text) {
                c.draft.clear();
                c.plugins = Some(plugins::Dialog::new(show));
                return Some(line);
            }
            // /hooks, /memory and /permissions open theirs, filled by the reads the line asks for.
            if let Some(line) = c.session.setup.open(&text) {
                c.draft.clear();
                return Some(line);
            }
            // /status, /export and /btw open a dialog, and /plan sets the mode and sends its task, all in this one write.
            if let Some(line) = c.session.typed_command(&text, host::now() as u64, n) {
                c.draft.clear();
                c.past = None;
                c.doing = (c.session.activity(), host::now() as u64);
                return Some(line);
            }
        }
        c.draft.clear();
        composer::forget();
        let images = std::mem::take(&mut c.images);
        if c.session.working {
            return Some(c.session.queue(&text, images, host::now() as u64, n));
        }
        let line = c.session.send(&text, &images, host::now() as u64, n);
        // The status line's time counts from here, not from the end of the turn before.
        c.doing = (c.session.activity(), host::now() as u64);
        Some(line)
    });
    images::flush();
    report_agent();
}

/// Shows the picture `id` large, in a dialog.
pub fn show_picture(id: &str) {
    with(|c| c.picture = Some(id.to_owned()));
}

pub fn close_picture() {
    with(|c| c.picture = None);
}

/// Adds a picture pasted into the message box.
pub fn add_image(media_type: &str, data: &str) {
    if !media_type.is_empty() && !data.is_empty() {
        with(|c| c.images.push((media_type.to_owned(), data.to_owned())));
    }
}

/// Adds the pictures you pick in the system's file picker, as pasted ones are. Wings opens it only on your click.
pub fn attach() {
    let picked = host::pick_images(true);
    task::spawn(async move {
        match picked.await {
            Ok(images) => with(|c| c.images.extend(images)),
            Err(error) => with(|c| c.session.items.push(Item::Error(format!("Couldn't attach pictures: {error}")))),
        }
    });
}

/// Takes the `index`th pasted picture out of the message.
pub fn remove_image(index: usize) {
    with(|c| {
        if index < c.images.len() {
            c.images.remove(index);
        }
    });
}

/// Takes a queued message back.
pub fn unqueue(uuid: &str) {
    write(|c, n| c.session.unqueue(uuid, n));
    forget_open_queued();
}

/// Shows a queued message in full, or folds it back to one line.
pub fn toggle_queued(uuid: &str) {
    with(|c| {
        if !c.open_queued.remove(uuid) {
            c.open_queued.insert(uuid.to_owned());
        }
    });
}

/// Drops the open state of queued messages that are gone: taken back, read, or sent now under a new uuid.
fn forget_open_queued() {
    with(|c| {
        let queued = &c.session.queued;
        c.open_queued.retain(|uuid| queued.iter().any(|queued| &queued.uuid == uuid));
    });
}

/// Has Claude read a queued message at its next step instead of after the turn.
pub fn send_now(uuid: &str) {
    write(|c, n| c.session.send_now(uuid, host::now() as u64, n));
    forget_open_queued();
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

/// Opens the Claude chat page in Wings Settings, where the defaults for every chat are.
pub fn open_settings() {
    drop(host::call("openSettings", json!({})));
}

/// Switches to the model and effort picked together in the message box, as `<model>@<effort>`.
pub fn pick_model_effort(choice: &str) {
    write(|c, n| c.session.pick(choice, n));
}

/// Switches the output style for this session, from its menu in the message box.
pub fn pick_style(name: &str) {
    write(|c, n| c.session.options.set_style(name, n));
}

/// Turns fast mode on or off for this session, from its button in the message box.
pub fn toggle_fast() {
    write(|c, n| {
        let on = !c.session.options.fast;
        c.session.options.set_fast(on, n)
    });
}

pub fn pick_mode(mode: &str) {
    write(|c, n| Some(c.session.set_mode(mode, n)));
}

/// Shift-Tab: the next permission mode, as the mode menu lists them.
pub fn cycle_mode() {
    if let Some(next) = with(|c| c.session.mode.as_deref().map(composer::next_mode)) {
        pick_mode(next);
    }
}

/// Escape: closes the dialog or sheet that is open, else stops the turn while Claude works.
pub fn escape() {
    let (dialog, working) = with(|c| {
        let open = over_conversation(c) || c.past.is_some() || c.panel.is_some() || c.plugins.is_some() || c.task.is_some() || c.session.rewind.is_some() || c.session.setup.open.is_some();
        (open, c.session.working)
    });
    match composer::escape(dialog, working) {
        Escape::Close => with(|c| {
            // The one opened last is on top: the setup dialog's form, then the dialogs over the chat, then the sheet.
            if over_conversation(c) {
                c.side_open = false;
                return;
            }
            if c.session.setup.form.take().is_none() && c.session.setup.open.take().is_none() && c.session.rewind.take().is_none() && c.task.take().is_none() && c.plugins.take().is_none() && c.panel.take().is_none() {
                c.past = None;
            }
        }),
        Escape::Stop => stop(),
        Escape::Nothing => {}
    }
}

/// Up from the first line: the prompt you sent before the one in the box, the newest first. What you had typed comes back
/// with Down.
pub fn recall_older() {
    with(|c| {
        if let Some(said) = composer::older(&c.draft, &composer::prompts(&c.session.items)) {
            c.draft = said;
        }
    });
}

/// Down from the last line: the next prompt, and after the newest what you had typed.
pub fn recall_newer() {
    with(|c| {
        if let Some(said) = composer::newer(&composer::prompts(&c.session.items)) {
            c.draft = said;
        }
    });
}

/// Puts the command in the message box, to add what it takes.
pub fn insert_command(name: &str) {
    with(|c| c.draft = format!("/{name} "));
}

/// The commands whose name holds what the draft has after its `/`, while that is one word: Claude Code's, and the ones the
/// chat answers itself that it doesn't list.
pub fn command_matches(c: &Chat) -> Vec<Command> {
    let Some(typed) = c.draft.strip_prefix('/').filter(|typed| !typed.contains(char::is_whitespace)) else {
        return Vec::new();
    };
    // Typed out in full, the command is ready: a list over it would take the Enter that opens its view.
    if view_of(&c.draft).is_some() || exits(&c.draft) || plugins::command(&c.draft).is_some() || setup::command(&c.draft).is_some() || commands::is_whole(&c.draft) {
        return Vec::new();
    }
    let typed = typed.to_lowercase();
    let own = commands::own_commands(&c.session.commands);
    c.session.commands.iter().cloned().chain(own).filter(|command| command.name.to_lowercase().contains(&typed)).take(8).collect()
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
    with(|c| {
        c.dirs.clear();
        c.forked_from = None;
    });
    launch(None);
}

pub fn resume(id: &str) {
    with(|c| {
        c.dirs.clear();
        c.forked_from = None;
    });
    launch(Some(id.to_owned()));
}

/// Opens the sheet of the project's past sessions over the chat.
pub fn open_past() {
    let opened = with(|c| {
        let closed = c.past.is_none();
        if closed {
            c.past = Some(Past::default());
        }
        closed
    });
    if opened {
        load_past(String::new());
    }
}

/// Closes the sheet of past sessions.
pub fn close_past() {
    with(|c| c.past = None);
}

/// Lists only the past sessions last active `since` (`today`, `week` or `month`), or at any time.
pub fn set_since(since: &str) {
    with(|c| {
        if let Some(past) = &mut c.past {
            past.since = since.to_owned();
        }
    });
}

/// Lists only the past sessions on `branch`, or on every branch when it is empty.
pub fn set_branch(branch: &str) {
    with(|c| {
        if let Some(past) = &mut c.past {
            past.branch = Some(branch.to_owned()).filter(|branch| !branch.is_empty());
        }
    });
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

/// Asks the Changes plugin to show its sidebar: a plugin can only open its own pages, so it hears this event instead.
pub fn open_changes() {
    drop(host::call("emit", json!({ "name": "open-changes" })));
}

/// The suggested next message to show in the empty message box, if there is one.
pub fn suggested(c: &Chat) -> Option<&str> {
    let empty = c.draft.is_empty() && c.images.is_empty();
    c.session.suggestion.as_deref().filter(|_| empty && c.state == State::Running)
}

/// Puts the suggested next message in the message box, to send or change. The suggestion stays until the next turn, so
/// emptying the box shows it again.
pub fn accept_suggestion() {
    with(|c| {
        if let Some(text) = suggested(c).map(str::to_owned) {
            c.took_suggestion = Some(text.clone());
            c.draft = text;
        }
    });
}

/// What you typed in the message box. Typing your own message dismisses the suggestion, as in Claude Code; editing the
/// one you took with Tab doesn't.
pub fn typed(value: &str) {
    with(|c| {
        c.draft = value.to_owned();
        if !c.draft.is_empty() && c.took_suggestion.as_ref().is_none_or(|took| Some(took) != c.session.suggestion.as_ref()) {
            c.session.suggestion = None;
        }
    });
}

/// Opens the usage or context dialog and reads what it shows: one request, from your click.
pub fn open_panel(panel: Panel) {
    write(|c, _| {
        c.past = None;
        c.panel = Some(panel);
        Some(c.session.open_view(panel))
    });
}

/// Closes the usage or context dialog.
pub fn close_panel() {
    with(|c| c.panel = None);
}

/// Switches the usage dialog between the last day's contributors and the last week's.
pub fn set_panel_week(week: bool) {
    with(|c| c.panel_week = week);
}

pub fn toggle_output(id: &str) {
    with(|c| {
        if !c.open_outputs.remove(id) {
            c.open_outputs.insert(id.to_owned());
        }
    });
}

/// The page's split says whether the tab is narrower than `view::SIDE_BELOW`, so the side panel covers the conversation.
pub fn set_narrow(narrow: bool) {
    with(|c| c.narrow = Some(narrow));
}

/// Whether the side panel shows over the conversation, which then has Escape and a press outside to put it away.
fn over_conversation(c: &Chat) -> bool {
    c.side_open && c.narrow == Some(true)
}

/// The header's button for the side panel, at any width: shows it or puts it away in every chat, and keeps the choice.
pub fn toggle_side() {
    let shown = with(|c| {
        c.side_open = !c.side_open;
        c.side_open
    });
    drop(prefs::side_panel().set(shown));
}

/// Escape or a press outside a panel over the conversation: put away in this tab only, so the choice you made stands.
pub fn close_side() {
    with(|c| c.side_open = false);
}

/// Opens what the background task `id` printed in a dialog, and reads it again: one request, from your click.
pub fn open_task(id: &str) {
    with(|c| c.task = Some(id.to_owned()));
    write(|c, _| c.session.read_task(id));
}

/// Reads the open task's output again.
pub fn refresh_task() {
    if let Some(id) = with(|c| c.task.clone()) {
        write(|c, _| c.session.read_task(&id));
    }
}

pub fn close_task() {
    with(|c| c.task = None);
}

/// Stops the background task `id`.
pub fn stop_task(id: &str) {
    write(|c, _| c.session.stop_task(id));
}

/// Opens the rewind dialog for your message `uuid`, with the dry run that lists the files restoring its code changes.
pub fn open_rewind(uuid: &str) {
    write(|c, _| c.session.open_rewind(uuid));
}

/// Restores what the rewind dialog's choice says.
pub fn restore(how: Restore) {
    write(|c, _| c.session.restore(how));
}

pub fn close_rewind() {
    with(|c| c.session.rewind = None);
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

#[cfg(test)]
mod tests {
    use super::{changes_counted, shortstat};

    #[test]
    fn the_changes_plugins_totals_are_the_count() {
        let answer = serde_json::to_string_pretty(&serde_json::json!({ "repo": "/r", "added": 120, "removed": 7, "total": 3, "files": [] })).unwrap();
        assert_eq!(changes_counted(&answer), Some((120, 7)));
        assert_eq!(changes_counted("wings is not inside a git repo."), None);
        assert_eq!(changes_counted(r#"{ "added": 1 }"#), None);
    }

    #[test]
    fn a_shortstat_gives_the_lines_added_and_removed() {
        assert_eq!(shortstat(" 3 files changed, 590 insertions(+), 277 deletions(-)"), (590, 277));
        assert_eq!(shortstat(" 1 file changed, 1 insertion(+)"), (1, 0));
        assert_eq!(shortstat(" 2 files changed, 4 deletions(-)"), (0, 4));
        assert_eq!(shortstat(""), (0, 0));
    }
}
