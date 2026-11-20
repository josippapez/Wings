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
    fork_view, issues,
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
/// How many entries of a session read back the chat asks for, and how much of each string of them: a whole long session,
/// its tool outputs cut as the conversation cuts them anyway.
pub(crate) const HISTORY_ALL: u32 = 10_000;
pub(crate) const HISTORY_CUT: u32 = 4_000;
/// What a session read back is made of: your messages (those sent while Claude worked are attachments), Claude's, where a
/// rewind cut it, and its name.
pub(crate) const HISTORY_KINDS: [&str; 6] = ["user", "assistant", "last-prompt", "attachment:queued_command", "ai-title", "custom-title"];

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
    /// How many items are shown, back from where they end.
    pub shown: usize,
    /// Where the items shown end while you read an older part of a session too long to show whole, none at the newest.
    pub until: Option<usize>,
    /// The pull requests and issues the conversation names, for the side panel.
    pub refs: crate::refs::Refs,
    /// The name of the folder Claude Code runs in.
    pub folder: String,
    pub state: State,
    /// Your setting for how tool calls show: Compact folds them, Detailed opens every run and each call's output.
    pub detailed: bool,
    /// Your settings for which kinds of fold Detailed starts open (`folds.rs`).
    pub opens: crate::folds::Opens,
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
    /// The project's files, for @-mentions.
    pub files: Files,
    /// The Wings and GitHub issues for #-references.
    pub issues: crate::issues::Issues,
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
    /// Your choice for every chat of showing diffs side by side (`prefs::diff_split`). A narrow tab keeps one column.
    pub diff_split: bool,
    /// Your choice for every chat of whether the session panel shows (`prefs::side_panel`, on by default): in the chat, or
    /// opened with it in Wings' sidebar, as `side_in_wings` says.
    pub side_open: bool,
    /// Where the session panel is, as you set for every chat (`prefs::session_panel`): in Wings' sidebar, else over the
    /// chat's right side.
    pub side_in_wings: bool,
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
    pub(crate) agent: Option<(String, String, Option<String>)>,
    /// And the session and account it was told with them, by name (`Some(None)` the default one), for the other Claude
    /// Code sessions that list and message this chat through Wings.
    told: (Option<String>, Option<Option<String>>),
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
    /// The Claude Code account you picked for this tab's new chats, by name, `Some(None)` being the default one. None
    /// until you pick one: a new chat then runs under its project's account.
    pub picked: Option<Option<String>>,
    /// The Claude Code accounts there are, for the picker in the message box. Empty until Wings lists them, and in a
    /// Wings that won't.
    pub accounts: Vec<host::AccountChoice>,
}

thread_local! {
    static CHAT: RefCell<Chat> = RefCell::new(Chat {
        session: Session::default(),
        draft: String::new(),
        took_suggestion: None,
        images: Vec::new(),
        shown: PAGE,
        until: None,
        refs: crate::refs::Refs::default(),
        folder: String::new(),
        state: State::Starting,
        detailed: false,
        opens: crate::folds::Opens::default(),
        text_scale: 1.0,
        panel: None,
        panel_week: false,
        plugins: None,
        picture: None,
        open_steps: HashSet::new(),
        open_outputs: HashSet::new(),
        files: Files::default(),
        issues: Default::default(),
        drafts: HashMap::new(),
        todos_folded: false,
        doing: (String::new(), 0),
        task: None,
        dirs: Vec::new(),
        fork_from: None,
        fork_ask: false,
        forked_from: None,
        narrow: None,
        diff_split: false,
        side_open: true,
        side_in_wings: false,
        message_to: None,
        changes: None,
        counting: false,
        count_next: None,
        branch: None,
        terminals: Vec::new(),
        agent: None,
        told: (None, None),
        titled: None,
        moving: false,
        input: None,
        requests: 0,
        run: 0,
        cwd: String::new(),
        program: "claude".into(),
        picked: None,
        accounts: Vec::new(),
    });
}

pub fn with<T>(change: impl FnOnce(&mut Chat) -> T) -> T {
    CHAT.with(|chat| change(&mut chat.borrow_mut()))
}

/// The Claude Code account a program for `session` runs under: the one its transcript is in, else the one you picked
/// for this tab, else its project's.
pub fn account(c: &Chat, session: Option<&str>) -> host::Account {
    match &c.picked {
        Some(name) => host::Account::picked(session, name.as_deref()),
        None => host::Account::of_session(session),
    }
}

/// What a tab keeps of the account you picked: `{ "name": ... }`, or null for none.
fn picked_json(picked: &Option<Option<String>>) -> Value {
    picked.as_ref().map_or(Value::Null, |name| json!({ "name": name }))
}

/// The account a tab's saved state or its args picked: `{ "name": null | "<name>" }`.
fn picked_of(saved: &Value, args: &Value) -> Option<Option<String>> {
    let pick = saved.get("account").filter(|pick| pick.is_object()).or_else(|| args.get("account").filter(|pick| pick.is_object()))?;
    Some(pick["name"].as_str().map(str::to_owned))
}

#[cfg(not(any(feature = "subagent", feature = "pane", feature = "session")))]
#[unsafe(no_mangle)]
pub fn start() {
    // A subagent's tab is a page of this view too, opened with the subagent in its args.
    if crate::subagent_tab::start() {
        return;
    }
    wings_plugin::run_filled(crate::view::view);
    crate::subagent_relay::start();
    crate::mod_pane::start();
    with(|c| {
        c.detailed = prefs::detailed().get();
        c.text_scale = prefs::scale_of(&prefs::text_size().get());
        c.side_open = prefs::side_panel().get();
        c.diff_split = prefs::diff_split().get();
        c.side_in_wings = prefs::in_sidebar(&prefs::session_panel().get());
    });
    prefs::text_size().on_change(|size| with(|c| c.text_scale = prefs::scale_of(size)));
    prefs::side_panel().on_change(|shown| with(|c| c.side_open = *shown));
    prefs::diff_split().on_change(|split| with(|c| c.diff_split = *split));
    // The panel moves with the setting, in every open chat.
    prefs::session_panel().on_change(|place| session_panel_moved(place));
    // After your setting is read: a chat in front opens the session sidebar with it.
    crate::session_side::start();
    wings_plugin::poll::while_focused(crate::refs_view::ISSUES_EVERY_MS, false, crate::refs_view::refresh_issues);
    host::on_terminals(|listed| with(|c| c.terminals = listed.iter().filter_map(Terminal::of).collect()));
    host::on_agent_message(take_agent_message);
    // The clicks were made against the old setting, so they go with it.
    prefs::detailed().on_change(|detailed| {
        with(|c| {
            c.detailed = *detailed;
            c.open_steps.clear();
            c.open_outputs.clear();
        })
    });
    crate::folds::follow();
    task::spawn(open());
}

/// Finds the folder and the session the tab is for, and starts Claude Code there.
async fn open() {
    let args = host::view_args().unwrap_or(Value::Null);
    let saved = host::call("tabState", json!({})).await.unwrap_or(Value::Null);
    let resume = saved["sessionId"].as_str().or(args["resume"].as_str()).map(str::to_owned);
    // A fork opened beside a chat in use (`fork_beside`) starts as one only the first time: after a restart the tab goes on
    // with the session it saved.
    let how = match saved["sessionId"].as_str() {
        Some(_) => Launch::default(),
        None => Launch { fork: args["fork"] == true, before: args["before"].as_str().map(str::to_owned) },
    };
    let dirs = if saved["dirs"].is_array() { &saved["dirs"] } else { &args["dirs"] };
    let dirs: Vec<String> = dirs.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    let cwd = match args["cwd"].as_str() {
        Some(cwd) => cwd.to_owned(),
        None => match host::call("project", json!({})).await.ok().and_then(|project| project["path"].as_str().map(str::to_owned)) {
            Some(path) => path,
            None => return with(|c| c.state = State::Failed("Open a project first. A chat runs Claude Code in a project's folder.".into())),
        },
    };
    let program = args["program"].as_str().filter(|program| PROGRAMS.contains(program)).unwrap_or("claude").to_owned();
    let picked = picked_of(&saved, &args);
    with(|c| {
        c.folder = basename(&cwd).to_owned();
        c.cwd = cwd;
        c.program = program;
        c.dirs = dirs;
        c.picked = picked;
        c.forked_from = args["forkedFrom"].as_str().filter(|_| how.fork).map(str::to_owned);
    });
    // After an update or restart of the plugin or of Wings, the Claude Code this tab ran is still going: the chat takes
    // it over, and never starts a second one on its session. A fork is a new session, while the chat it came from still
    // runs the one it copies.
    if !how.fork && crate::reattach::take_over(resume.clone()).await {
        return;
    }
    launch_with(resume, how);
}

/// Starts Claude Code anew in the tab, going on with `resume` or in a new session, in place of the one running.
fn launch(resume: Option<String>) {
    launch_with(resume, Launch::default());
}

/// `launch`, as a copy of the session when `how` says so, and with the folders you added.
pub fn launch_with(resume: Option<String>, how: Launch) {
    let (run, cwd, program, start_mode, dirs) = with(|c| {
        if how.fork {
            // A fork in a new tab has no conversation yet to name it by: the chat it came from named it.
            c.forked_from = Some(name(c).or_else(|| c.forked_from.clone()).unwrap_or_else(|| "an earlier session".into()));
        }
        let run = begin(c);
        (run, c.cwd.clone(), c.program.clone(), c.session.start_mode.clone(), c.dirs.clone())
    });
    load_files(run, cwd.clone());
    load_git(run, cwd.clone());
    task::spawn(async move {
        if let Some(id) = &resume {
            if let Ok(entries) = host::transcript_cut(id, &HISTORY_KINDS, HISTORY_ALL, HISTORY_CUT).await {
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
        // default one; a new chat gets the account you picked, else its project's, as a terminal there does.
        let account = with(|c| account(c, resume.as_deref()));
        load_accounts(run, resume.as_deref(), cwd.clone());
        // A fork before a message asks for its rewind as it starts, as the opening requests do: the fork is a new session.
        let opening = Session::opening(start_mode.as_deref()) + &with(|c| fork::opening(&how, &c.session.prompts)) + &with(remote_on_start);
        // In Wings' terminal server, so the turn goes on while Wings restarts or updates, and this tab takes it over after.
        let process = host::spawn_surviving(&account, &program, &args, &cwd, &opening, move |line| hear(run, line));
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
    c.until = None;
    c.panel = None;
    c.plugins = None;
    c.picture = None;
    c.open_steps.clear();
    c.open_outputs.clear();
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
    let (id, dirs, picked) = with(|c| (c.session.id.clone(), c.dirs.clone(), picked_json(&c.picked)));
    drop(host::call("setTabState", json!({ "state": { "sessionId": id, "dirs": dirs, "account": picked } })));
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
/// The accounts, with the one the chat runs `session` under marked, for the header's account button. A Wings that doesn't
/// list them, or doesn't let the chat, leaves the button out. A chat that took over the Claude Code it ran before Wings
/// restarted asks too, or it would lose the button with the update.
pub(crate) fn load_accounts(run: u32, session: Option<&str>, folder: String) {
    let asked = with(|c| account(c, session));
    task::spawn(async move {
        if let Ok(accounts) = host::accounts_under(&asked, &folder).await {
            with(|c| {
                if c.run == run {
                    c.accounts = accounts;
                }
            });
        }
    });
}

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
        // The terminal takes this tab's place, as a chat takes a terminal's when a session moves the other way.
        let opened = host::open_pane_in_place(&format!("{program} --resume"), &[&id], &cwd).await;
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
        // One sentence: Claude Code's "Error: Invalid API key · Please run /login" has an "Error:" the banner's own "stopped"
        // already says, and a dot between its two parts.
        Some(line) => format!("Claude Code stopped: {}", line.trim_start_matches("Error:").trim().replace(" · ", ". ")),
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
            c.until = None;
            c.open_steps.clear();
            c.open_outputs.clear();
            c.drafts.clear();
            c.task = None;
        }
        let doing = c.session.activity();
        if doing != c.doing.0 {
            c.doing = (doing, host::now() as u64);
            // A mod that rewords the line under the conversation hears what it says now.
            if c.session.working {
                c.session.mods.redraw(crate::mods::SPINNER);
            }
        }
        let named = c.session.id.clone().filter(|id| Some(id) != before.as_ref());
        (named, std::mem::take(&mut c.session.alerts), was_working && !c.session.working)
    });
    // Pictures in what was heard go to Wings once, as the page names them by id.
    images::flush();
    crate::subagent_relay::heard(line);
    // What a mod shows that changed is asked for again, Claude Code's requests of the chat are answered, and a toast
    // goes once its time is up.
    for key in with(|c| if c.run == run { c.session.mods.take_stale() } else { Vec::new() }) {
        write(move |c, n| crate::mods_view::request_for(c, &key, n));
    }
    // A pane Claude Code asked to show, as `/feedback` opens its pane, opens in Wings' sidebar.
    if let Some(pane) = with(|c| if c.run == run { c.session.mods.to_open.take() } else { None }) {
        crate::mod_pane::open(&pane);
    }
    for (id, asked) in with(|c| if c.run == run { std::mem::take(&mut c.session.mods.asks) } else { Vec::new() }) {
        write(move |c, _| Some(crate::mods_view::answer_ask(c, &id, &asked)));
    }
    if line["type"] == "system" && line["subtype"] == "ui_toast" {
        let shown = line["timeout_ms"].as_u64().unwrap_or(4_000).min(60_000) as u32;
        task::spawn(async move {
            host::sleep(shown + 50).await;
            with(|c| c.session.mods.expire(host::now() as u64));
        });
    }
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
    crate::diff_view::fetch(run);
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
        let told = (c.session.id.clone(), c.accounts.iter().find(|account| account.current).map(|account| account.name.clone()));
        let agent = (c.agent.as_ref() != Some(&agent) || c.told != told).then(|| {
            c.agent = Some(agent.clone());
            c.told = told.clone();
            (agent, told)
        });
        (agent, retitled)
    });
    if let Some(((state, name, summary), (session, account))) = agent {
        let account = account.as_ref().map(Option::as_deref);
        let report = host::AgentReport { state: &state, name: &name, summary: summary.as_deref(), session: session.as_deref(), account };
        drop(host::set_agent_report(Some(report)));
    }
    if let Some(title) = retitled {
        drop(host::set_tab_title(title.as_deref()));
    }
    crate::subagent_relay::sync();
    crate::mod_pane::sync();
    crate::session_side::sync();
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
            // /remote-control turns Remote Control on, or off when it is on, as in Claude Code.
            if let Some(name) = crate::remote::command(&text) {
                c.draft.clear();
                return Some(remote_line(c, name));
            }
            if let Some((panel, line)) = c.session.view_command(&text) {
                c.draft.clear();
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
                c.doing = (c.session.activity(), host::now() as u64);
                return Some(line);
            }
        }
        c.draft.clear();
        composer::forget();
        // Sent while reading an older part, the conversation goes to the newest, where the message shows.
        if c.until.is_some() {
            crate::view::show_latest(c);
        }
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

/// A message another agent sent this chat through Wings' `send_to_agent`: it goes in the message box, and with `submit`,
/// which only your "Let agents message each other" gives, it is sent as yours would be, or queued while Claude works.
/// Whether it was sent, or why the chat won't take it.
pub(crate) fn take_agent_message(text: &str, submit: bool) -> Result<bool, String> {
    with(|c| {
        if c.input.is_none() {
            return Err("Claude Code isn't running in this chat, so it can't take a message now".to_owned());
        }
        // What you started typing stays yours, as in a terminal's prompt.
        if !c.draft.trim().is_empty() || !c.images.is_empty() {
            return Err("Someone typed into this chat's message box and hasn't sent it. Wings won't type over it.".to_owned());
        }
        c.draft = text.to_owned();
        Ok(())
    })?;
    if submit {
        send();
    }
    Ok(submit)
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
        let open = c.panel.is_some() || c.plugins.is_some() || c.task.is_some() || c.session.rewind.is_some() || c.session.setup.open.is_some();
        (open, c.session.working)
    });
    match composer::escape(dialog, working) {
        Escape::Close => with(|c| {
            // The one opened last is on top: the setup dialog's form, then the dialogs over the chat.
            let _ = c.session.setup.form.take().is_some() || c.session.setup.open.take().is_some() || c.session.rewind.take().is_some() || c.task.take().is_some() || c.plugins.take().is_some() || c.panel.take().is_some();
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
    commands::menu(&c.session.commands, typed)
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

/// The issues that match the `#word` the draft ends in, with where the word starts. None while a slash command is being
/// typed (that menu wins), or when the word matches none.
pub fn issue_matches(c: &Chat) -> Option<(usize, Vec<&issues::Entry>)> {
    if c.draft.starts_with('/') {
        return None;
    }
    let (start, query) = issues::active(&c.draft)?;
    let found = c.issues.matches(query);
    (!found.is_empty()).then_some((start, found))
}

/// Replaces the `#word` the draft ends in with the issue's key or number.
pub fn insert_issue(id: &str) {
    with(|c| {
        if let Some((start, _)) = issues::active(&c.draft) {
            c.draft = issues::insert(&c.draft, start, id);
        }
    });
}

/// Replaces the `@word` the draft ends in with a mention of `path`.
pub fn insert_mention(path: &str) {
    with(|c| {
        if let Some((start, _)) = mentions::active(&c.draft) {
            c.draft = mentions::insert(&c.draft, start, path);
        }
    });
}

/// Whether Claude Code is doing something in this chat that starting another one in its tab would cut off: a turn, messages
/// waiting for it, or a background task or subagent. A new chat, a resume or a fork then opens in a new tab beside this one,
/// and this one keeps running.
pub fn in_use(c: &Chat) -> bool {
    let subagent = |item: &Item| matches!(item, Item::Tool(tool) if matches!(tool.name.as_str(), "Task" | "Agent") && tool.output.is_none());
    c.state == State::Running
        && (c.session.working || !c.session.queued.is_empty() || c.session.tasks.iter().any(|task| task.ended.is_none()) || c.session.items.iter().any(subagent))
}

/// The args of a chat opened beside this one, in its folder and with its program, and `more`.
fn beside(c: &Chat, more: Value) -> Value {
    let mut args = json!({ "cwd": c.cwd, "program": c.program });
    if let (Some(args), Value::Object(more)) = (args.as_object_mut(), more) {
        args.extend(more);
    }
    args
}

/// Opens a chat with `args` in a new tab right after this one. Always a new tab: another chat of the same folder has the same
/// args, and opening by them would show that one.
pub(crate) fn open_beside(args: &Value) {
    drop(host::open_new_tab_beside("chat", args));
}

/// The header's +: a new chat here, or, while this one is in use, in a new tab beside it, under the account you picked here.
pub fn new_chat() {
    let beside = with(|c| {
        if in_use(c) {
            return Some(beside(c, json!({ "account": picked_json(&c.picked) })));
        }
        c.dirs.clear();
        c.forked_from = None;
        None
    });
    match beside {
        Some(args) => open_beside(&args),
        None => launch(None),
    }
}

/// The line that turns Remote Control on as the chat starts, with your setting on; nothing with it off.
fn remote_on_start(c: &mut Chat) -> String {
    if prefs::remote_on_start().get() { crate::remote::connect(&mut c.session.remote, None) } else { String::new() }
}

/// The line that turns Remote Control on, under `name` when given, or off when it is on.
fn remote_line(c: &mut Chat, name: Option<&str>) -> String {
    match &c.session.remote {
        crate::remote::Remote::On(_) => crate::remote::disconnect(&mut c.session.remote),
        _ => crate::remote::connect(&mut c.session.remote, name),
    }
}

/// The composer's Remote Control button: on when it is off, and off when it is on.
pub fn toggle_remote() {
    write(|c, _| match c.session.remote {
        crate::remote::Remote::Connecting | crate::remote::Remote::Disconnecting(_) => None,
        _ => Some(remote_line(c, None)),
    });
}

/// The value the account picker gives the default account: not a name an account can have.
pub const DEFAULT_ACCOUNT: &str = "~";

/// Starts a chat under the account you picked in the message box (`DEFAULT_ACCOUNT` for the default one): here, in place
/// of a chat you haven't written in yet, else in a new tab beside this one, so a conversation never moves account.
pub fn pick_account(value: &str) {
    let name = match value {
        "" => return,
        DEFAULT_ACCOUNT => None,
        name => Some(name.to_owned()),
    };
    let written = with(|c| {
        let written = c.session.items.iter().any(|item| matches!(item, Item::User(_) | Item::UserImages(..)));
        written.then(|| beside(c, json!({ "account": { "name": name } })))
    });
    if let Some(args) = written {
        return open_beside(&args);
    }
    with(|c| c.picked = Some(name));
    new_chat();
}

/// Goes on with a past session here, or, while this chat is in use, in a new tab beside it.
pub fn resume(id: &str) {
    let beside = with(|c| {
        if in_use(c) {
            return Some(beside(c, json!({ "resume": id })));
        }
        c.dirs.clear();
        c.forked_from = None;
        None
    });
    match beside {
        Some(args) => open_beside(&args),
        None => launch(Some(id.to_owned())),
    }
}

/// Starts a fork of the session `id` in a new tab beside this chat, which goes on running: the session as it is, or cut
/// before your message `before`. It keeps the folders you added here.
pub fn fork_beside(c: &Chat, id: &str, before: Option<&str>) -> Value {
    beside(c, json!({ "resume": id, "fork": true, "before": before, "dirs": c.dirs, "forkedFrom": name(c) }))
}

/// Opens Wings' Past sessions over the project, whose Resume goes on with a session in this chat (`resume`). One that ran
/// in another folder of the project goes on in a new chat beside this one, in that folder.
pub fn open_past() {
    task::spawn(async {
        let Ok(picked) = host::open_history_here().await else { return };
        let (Some(id), Some(cwd)) = (picked["resume"].as_str(), picked["cwd"].as_str()) else { return };
        match with(|c| (c.cwd != cwd).then(|| beside(c, json!({ "resume": id, "cwd": cwd })))) {
            Some(args) => open_beside(&args),
            None => resume(id),
        }
    });
}

/// Asks the Changes plugin to show its sidebar: a plugin can only open its own pages, so it hears this event instead.
pub fn open_changes() {
    crate::session_side::covered();
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
    // A `#word` lists issues, which are read the first time and then once a minute at most.
    if with(|c| !c.draft.starts_with('/') && issues::active(&c.draft).is_some()) {
        issues::want();
    }
}

/// Opens the usage or context dialog and reads what it shows: one request, from your click.
pub fn open_panel(panel: Panel) {
    write(|c, _| {
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

/// The session panel setting changed to `place`: the panel moves between the chat and Wings' sidebar.
pub(crate) fn session_panel_moved(place: &str) {
    let sidebar = prefs::in_sidebar(place);
    if with(|c| std::mem::replace(&mut c.side_in_wings, sidebar)) != sidebar {
        crate::session_side::moved(sidebar);
    }
}

/// The page's split says whether the tab is narrower than `view::SIDE_BELOW`, so the side panel covers the conversation.
pub fn set_narrow(narrow: bool) {
    with(|c| c.narrow = Some(narrow));
}

/// The header's button for the session panel, at any width: in Wings' sidebar it opens it on this chat or closes it
/// (`session_side::toggle`); in the chat it shows the panel or puts it away in every chat. Either way the choice is kept.
pub fn toggle_side() {
    if with(|c| c.side_in_wings) {
        crate::session_side::toggle();
        return;
    }
    let shown = with(|c| {
        c.side_open = !c.side_open;
        c.side_open
    });
    drop(prefs::side_panel().set(shown));
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
    use super::{State, new_chat, resume, with};
    use serde_json::{Value, json};
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::host;

    /// Wings, as far as the chat asks it, keeping every `openView` it is asked for.
    fn opened() -> Rc<RefCell<Vec<Value>>> {
        let opened = Rc::new(RefCell::new(Vec::new()));
        let kept = opened.clone();
        host::testing::answer(move |method, params| {
            if method == "openView" {
                kept.borrow_mut().push(params.clone());
            }
            Ok(Value::Null)
        });
        opened
    }

    fn running(working: bool) -> u32 {
        with(|c| {
            c.cwd = "/work/shop".into();
            c.program = "claude".into();
            c.picked = Some(Some("work".into()));
            c.state = State::Running;
            c.session.working = working;
            c.run
        })
    }

    #[test]
    fn the_failed_banner_is_one_sentence_without_a_doubled_error() {
        let said = super::why(&json!({ "code": 1, "stderr": "Error: Invalid API key · Please run /login\n" }));
        assert_eq!(said, "Claude Code stopped: Invalid API key. Please run /login");
    }

    #[test]
    fn a_new_chat_while_a_turn_runs_opens_in_a_new_tab_and_leaves_this_chat_running() {
        let opened = opened();
        let run = running(true);
        new_chat();
        assert_eq!(with(|c| (c.run, c.state == State::Running)), (run, true));
        assert_eq!(
            *opened.borrow(),
            [json!({ "id": "chat", "as": "tab", "args": { "cwd": "/work/shop", "program": "claude", "account": { "name": "work" } }, "newTab": true, "after": "self" })]
        );
    }

    #[test]
    fn a_resume_while_a_turn_runs_opens_in_a_new_tab_too() {
        let opened = opened();
        let run = running(true);
        resume("s1");
        assert_eq!(with(|c| c.run), run);
        assert_eq!(opened.borrow()[0]["args"]["resume"], "s1");
        assert_eq!(opened.borrow()[0]["newTab"], true);
    }

    /// Wings, answering `openHistory` with `picked` and keeping every call.
    fn wings_picks(picked: Value) -> Rc<RefCell<Vec<(String, Value)>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(if method == "openHistory" { picked.clone() } else { Value::Null })
        });
        calls
    }

    fn asked(calls: &Rc<RefCell<Vec<(String, Value)>>>, method: &str) -> Vec<Value> {
        calls.borrow().iter().filter(|(called, _)| called == method).map(|(_, params)| params.clone()).collect()
    }

    #[test]
    fn past_sessions_opens_wings_own_sheet_and_a_session_picked_there_goes_on_in_this_chat() {
        use super::open_past;
        let calls = wings_picks(json!({ "resume": "s2", "cwd": "/work/shop" }));
        let run = running(false);
        open_past();
        wings_plugin::task::block_on(async {});
        assert_eq!(asked(&calls, "openHistory"), [json!({ "resumeIn": "this tab" })]);
        // Here, as the chat is idle.
        assert_eq!(with(|c| c.run), run + 1);
        assert!(asked(&calls, "openView").is_empty());
        // While a turn runs, beside this chat.
        let calls = wings_picks(json!({ "resume": "s2", "cwd": "/work/shop" }));
        let run = running(true);
        open_past();
        wings_plugin::task::block_on(async {});
        assert_eq!(with(|c| c.run), run);
        assert_eq!(asked(&calls, "openView")[0]["args"]["resume"], "s2");
    }

    #[test]
    fn a_past_session_of_another_folder_goes_on_in_a_new_chat_in_that_folder() {
        use super::open_past;
        let calls = wings_picks(json!({ "resume": "s3", "cwd": "/work/shop/api" }));
        let run = running(false);
        open_past();
        wings_plugin::task::block_on(async {});
        assert_eq!(with(|c| c.run), run);
        let opened = asked(&calls, "openView");
        assert_eq!((&opened[0]["args"]["resume"], &opened[0]["args"]["cwd"]), (&json!("s3"), &json!("/work/shop/api")));
    }

    #[test]
    fn open_in_terminal_puts_the_terminal_in_this_tabs_place() {
        use super::to_terminal;
        let calls = wings_picks(Value::Null);
        running(false);
        with(|c| (c.input, c.session.id) = (None, Some("s1".into())));
        to_terminal();
        wings_plugin::task::block_on(async {});
        assert_eq!(
            asked(&calls, "openPane"),
            [json!({ "command": "claude --resume", "args": ["s1"], "cwd": "/work/shop", "placement": "replace" })]
        );
        assert!(with(|c| c.state == State::Moved));
    }

    #[test]
    fn a_new_chat_while_idle_starts_here_and_opens_no_tab() {
        let opened = opened();
        let run = running(false);
        new_chat();
        assert_eq!(with(|c| c.run), run + 1);
        assert!(opened.borrow().is_empty());
    }

    #[test]
    fn a_message_from_another_agent_fills_the_message_box_and_is_sent_only_when_wings_says_so() {
        use super::take_agent_message;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        running(false);
        with(|c| (c.input, c.draft) = (None, String::new()));
        assert!(take_agent_message("hi", true).unwrap_err().contains("isn't running"));
        let process = host::spawn("claude", &[], "/work/shop", "", |_| {});
        with(|c| c.input = Some(process.input()));
        // Without "Let agents message each other" it waits in the message box for you.
        assert_eq!(take_agent_message("(Message from another agent)\nReview it.", false), Ok(false));
        assert_eq!(with(|c| c.draft.clone()), "(Message from another agent)\nReview it.");
        // What is in the box stays yours.
        assert!(take_agent_message("Run the tests.", true).unwrap_err().contains("hasn't sent it"));
        with(|c| c.draft.clear());
        // With it, it is sent as your message, in one write.
        assert_eq!(take_agent_message("Run the tests.", true), Ok(true));
        wings_plugin::task::block_on(async {});
        let writes: Vec<String> = calls.borrow().iter().filter(|(method, _)| method == "processWrite").map(|(_, params)| params["text"].as_str().unwrap().to_owned()).collect();
        assert_eq!(writes.len(), 1, "{writes:?}");
        assert!(writes[0].contains("Run the tests."), "{}", writes[0]);
        assert!(with(|c| c.draft.is_empty() && c.session.working));
    }

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
