//! A Claude Code session as the chat shows it, built from the lines `claude -p --output-format stream-json` prints, and
//! from the entries of its transcript when an earlier session is resumed. The two share their `user` and `assistant`
//! shapes.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};
use wings_plugin::format::parse_time;

use crate::{
    actions::Ops,
    edits::Edits,
    elicitation::Elicitation,
    images,
    commands::Commands,
    notices,
    plugins::{self, Plugins},
    setup::Setup,
    spend::{Meter, Spend},
    usage::{ContextDetail, Panel, Usage},
};

/// The most of a tool's output the chat keeps. A whole file read or a long build log is more than anyone reads in a card.
const OUTPUT_KEPT: usize = 4000;
/// The most of a tool's input shown as its one-line summary.
const SUMMARY_KEPT: usize = 200;
/// The most of what a tool would do that a permission card shows, and of the reason. Past the first, the card can't be
/// allowed: you would approve what you can't read. It is what one `Code` node holds in Wings (400,000 characters: bytes
/// here, so never more), so a long Write or script is read whole.
const ASK_KEPT: usize = 400_000;
const REASON_KEPT: usize = 300;
/// The most lines of an edit's diff the chat keeps, and the most of a written file's start.
const DIFF_KEPT: usize = 40;
const WRITE_KEPT: usize = 40;
/// The most items kept of each subagent's own transcript: a subagent that searches a repository makes hundreds of tool
/// calls, and its own tab reads them from its transcript. Older ones go.
pub const SUBAGENT_KEPT: usize = 200;
/// How many entries of a session read back the chat asks for: the most the host gives with the strings cut.
pub const HISTORY_ALL: u32 = 10_000;
/// What the line at the top of a session read back in part says.
pub const EARLIER_NOT_LOADED: &str = "Earlier messages aren't loaded";
/// The tool a subagent hands its final report back to its caller with (Claude Code 2.1.330), its `message` the report.
const HANDBACK: &str = "SubagentHandback";
/// The name a tool call with none shows under.
pub const UNNAMED: &str = "Unnamed tool";

#[derive(Debug, PartialEq)]
pub enum Item {
    /// What you sent.
    User(String),
    /// What you sent with pictures: the text, which may be empty, and the ids of the pictures (`images.rs`). Their data
    /// stays out of the item, as it is large and only Claude Code and Wings need it.
    UserImages(String, Vec<String>),
    /// What Claude wrote, as Markdown.
    Text(String),
    /// Pictures in Claude's own content, by id.
    Images(Vec<String>),
    Tool(Tool),
    /// Claude asks before using a tool.
    Ask(Ask),
    /// Something went wrong, as Claude Code said it.
    Error(String),
    /// A command you ran from the message box with `!`, kept separate from Claude's tool calls.
    Shell(Tool),
    /// You stopped the turn.
    Stopped,
    /// What Claude Code added to the conversation in your name, like a background task's notice: what it is, and
    /// what it said.
    Note(String, String),
    /// A background agent, command or monitor ended.
    Finished(Finished),
    /// A turn ended as it should, after this many milliseconds of work, and what it spent.
    Worked(u64, Spend),
    /// A line across the conversation, like the one where it was compacted.
    Divider(String),
    /// A short note from Claude Code, like a retry that failed or a model it switched to.
    Notice(String),
    /// You allowed an action auto mode blocked (`blocked.rs`): what it was ("this command") and its specifics, the
    /// command or the address, which may be empty.
    Allowed(String, String),
    /// What Claude thought before it answered, as its summary, and how long it took (ms, 0 when unknown).
    Thought(String, u64),
    /// A subagent's final report to this chat, whole.
    Report(Report),
}

/// A subagent's hand-back (`origin.handback` on a message Claude Code adds in the session's name): who sent it, the report
/// as Markdown and the note Claude Code flags it with, which is shown above the report and never folded.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The subagent's id, which its tab opens by.
    pub from: String,
    /// What Claude Code calls it (its agent type), for when nothing better names it.
    pub name: String,
    pub text: String,
    /// Claude Code's warning about the report, like a security warning, when it flagged one.
    pub note: Option<String>,
    /// The call that handed it back, which tells a report read twice from two.
    pub call: String,
}

/// The line where a background task ended, from its notice in the stream or in a transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub task: String,
    /// The tool call that started it, which names the subagent for a click on an agent's line.
    pub call: String,
    /// Agent, Command or Monitor.
    pub kind: &'static str,
    pub description: String,
    /// Claude Code's word: `completed`, `failed`, `stopped`, `killed`.
    pub status: String,
    /// How long it ran (ms), when known.
    pub took: Option<u64>,
    /// What it reported, shown under the line.
    pub report: String,
}

impl Finished {
    pub fn failed(&self) -> bool {
        self.status == "failed"
    }

    /// "Agent “Survey” finished", "Command “Build” failed".
    pub fn title(&self) -> String {
        let word = match self.status.as_str() {
            "failed" => "failed",
            "stopped" | "killed" => "stopped",
            _ if self.kind == "Monitor" => "ended",
            _ => "finished",
        };
        format!("{} “{}” {word}", self.kind, self.description)
    }
}

#[derive(Debug, PartialEq)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub summary: String,
    /// What it printed, once it has, and whether it failed.
    pub output: Option<(String, bool)>,
    /// What its input says beyond the summary, by kind of tool.
    pub detail: Detail,
    /// Its input whole, which its row's hover and its opened fold show.
    pub input: Input,
    /// The pictures it gave back (a Read of an image, a screenshot, an MCP tool), by id.
    pub images: Vec<String>,
}

/// A tool call's input as the summary leaves it out: its one line is cut at the first line break and at 200 characters,
/// and says one field.
#[derive(Debug, Default, PartialEq)]
pub struct Input {
    /// The field the summary is made from, all of it, when the summary cuts it: empty when the summary is all of it.
    pub whole: String,
    /// Its other fields as (name, value), a text as it is and anything else as JSON. None for a call whose fold already
    /// shows what its input does: an edit's change, a file written, a file read, the checklist.
    pub fields: Vec<(String, String)>,
}

impl Input {
    /// Whether an opened call has more of its input to show than its row says.
    pub fn says_more(&self) -> bool {
        !self.whole.is_empty() || !self.fields.is_empty()
    }
}

/// What a tool call's input holds that its one-line summary doesn't.
#[derive(Debug, PartialEq)]
pub enum Detail {
    Plain,
    /// An edit: the lines it takes out and puts in.
    Diff(Diff),
    /// A file written: how many lines, and the first of them.
    Write { lines: usize, head: Vec<String> },
    /// A shell command, whole.
    Command(String),
    /// A file read, and which of its lines when the call says.
    Read(Option<String>),
    /// A Grep or a Glob: its count comes from what it printed.
    Search,
}

/// The lines an Edit or MultiEdit changes, as `(added, text)`, cut at `DIFF_KEPT`.
#[derive(Debug, Default, PartialEq)]
pub struct Diff {
    pub added: usize,
    pub removed: usize,
    pub lines: Vec<(bool, String)>,
    /// The lines past the ones kept.
    pub more: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Progress {
    Pending,
    Active,
    Done,
}

/// One entry of Claude's checklist: from TodoWrite, or from the task tools.
#[derive(Debug, Clone, PartialEq)]
pub struct Todo {
    pub id: String,
    pub text: String,
    /// What it reads as while it is being done, like "Running the tests".
    pub active: String,
    pub progress: Progress,
}

#[derive(Debug, PartialEq)]
pub struct Ask {
    pub request: String,
    pub tool: String,
    pub summary: String,
    /// What the tool is asked to do, in full, as the card shows it.
    pub detail: String,
    /// The detail was too long to show whole.
    pub cut_short: bool,
    /// Why Claude Code asks, in its own words: the request's title, description and reason.
    pub reason: Option<String>,
    /// The permission rules "Always allow" adds. None when Claude Code offers none, or says not to offer them.
    pub suggestions: Vec<Value>,
    pub input: Value,
    /// The change an Edit, MultiEdit or Write asks for, as a diff against the file on disk, once it is read.
    pub preview: crate::diff_preview::Peek,
    /// Allowed or denied, once you have said, or once Claude Code withdrew the question (shown as denied).
    pub answer: Option<bool>,
}

/// A message of yours Claude Code holds until it has finished what it is doing.
#[derive(Debug, Clone, PartialEq)]
pub struct Queued {
    /// What Claude Code knows it by, for cancelling it and for recognising it when it is read.
    pub uuid: String,
    pub text: String,
    /// The pictures sent with it, as (media type, base64), kept in case it is sent again.
    pub images: Vec<(String, String)>,
    /// Text another agent wrote (`relayed`), which it is sent again as, not as one of yours.
    pub relayed: bool,
}

/// One of your messages: the item it is, and the uuid Claude Code knows it by, which a rewind names.
#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    pub at: usize,
    pub uuid: String,
    /// The transcript entry it follows, for one read from the transcript.
    pub(crate) parent: Option<String>,
}

/// The rewind dialog, while it is open: the message it goes back to before.
#[derive(Debug, PartialEq)]
pub struct Rewind {
    pub uuid: String,
    pub text: String,
    /// What restoring the code would change, from the dry run, or why it can't. None until Claude Code answers.
    pub check: Option<Result<Check, String>>,
    /// The requests the choice made are waiting for.
    pub waiting: Vec<&'static str>,
    pub error: Option<String>,
}

/// The files a code rewind writes back, and the lines it adds and takes out.
#[derive(Debug, PartialEq)]
pub struct Check {
    pub files: Vec<String>,
    pub added: u64,
    pub removed: u64,
}

/// What a rewind restores, as Claude Code's own `/rewind` offers it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Restore {
    Both,
    Conversation,
    Code,
}

/// A task Claude Code runs in the background for the session: a command started with `run_in_background`, a monitor, or
/// a subagent sent there.
#[derive(Debug, PartialEq)]
pub struct Task {
    pub id: String,
    /// Claude Code's `task_type`, like `local_bash`.
    pub kind: String,
    pub description: String,
    /// The id of the tool call that started it, once Claude Code says (`task_started`).
    pub call: String,
    /// When the chat first heard of it (ms).
    pub started: u64,
    /// How it ended, in Claude Code's word (`completed`, `failed`, `stopped`, `killed`), once it has.
    pub ended: Option<String>,
    /// When the chat heard it end (ms).
    pub ended_at: u64,
    /// You asked to stop it, and it hasn't ended yet.
    pub stopping: bool,
    /// The end of what it printed, as last read.
    pub output: Option<Output>,
    /// Why the last stop or read failed.
    pub error: Option<String>,
}

impl Task {
    /// Claude Code reads what a shell or a monitor printed, not a subagent.
    pub fn has_output(&self) -> bool {
        self.kind == "local_bash" || self.kind.starts_with("monitor")
    }
}

/// The end of what a task printed: at most its last 8 KiB, and how big the whole is.
#[derive(Debug, PartialEq)]
pub struct Output {
    pub text: String,
    pub bytes: u64,
    pub truncated: bool,
}

/// What you have picked and typed on a card that isn't answered yet.
#[derive(Debug, Default)]
pub struct Draft {
    /// The words for Claude when you deny, or keep planning.
    pub feedback: String,
    /// The options picked, as (question, option) places.
    picked: HashSet<(usize, usize)>,
    /// The "Other" text of each question.
    other: HashMap<usize, String>,
    /// The questions whose "Something else" row is picked, which shows its text field.
    other_open: HashSet<usize>,
    /// The question shown, when several are asked one at a time.
    pub step: usize,
}

impl Draft {
    pub fn is_picked(&self, question: usize, option: usize) -> bool {
        self.picked.contains(&(question, option))
    }

    /// Whether the "Something else" row of the question is picked: opened, or holding text.
    pub fn other_picked(&self, question: usize) -> bool {
        self.other_open.contains(&question) || !self.other(question).trim().is_empty()
    }

    /// Picks the "Something else" row, or takes it back (and its text). A question that takes one answer drops its options.
    pub fn toggle_other(&mut self, question: usize, multi: bool) {
        if self.other_picked(question) {
            self.other_open.remove(&question);
            self.other.remove(&question);
            return;
        }
        if !multi {
            self.picked.retain(|(q, _)| *q != question);
        }
        self.other_open.insert(question);
    }

    pub fn other(&self, question: usize) -> &str {
        self.other.get(&question).map_or("", String::as_str)
    }

    /// Picks the option, or takes it back. A question that takes one answer holds one option, and not the other text.
    pub fn toggle(&mut self, question: usize, option: usize, multi: bool) {
        if self.picked.remove(&(question, option)) {
            return;
        }
        if !multi {
            self.picked.retain(|(q, _)| *q != question);
            self.other.remove(&question);
            self.other_open.remove(&question);
        }
        self.picked.insert((question, option));
    }

    pub fn set_other(&mut self, question: usize, text: String, multi: bool) {
        if !multi && !text.trim().is_empty() {
            self.picked.retain(|(q, _)| *q != question);
        }
        self.other.insert(question, text);
    }

    /// Whether question `question` has an option picked or other text.
    pub fn has_answer(&self, question: usize) -> bool {
        self.picked.iter().any(|(q, _)| *q == question) || !self.other(question).trim().is_empty()
    }

    /// Each question's answer, by its text: the labels picked, then the other text, joined by commas. None while a
    /// question has none.
    pub fn answers(&self, questions: &Value) -> Option<Map<String, Value>> {
        let mut answers = Map::new();
        for (q, question) in questions.as_array()?.iter().enumerate() {
            let mut said: Vec<String> = question["options"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .filter(|(o, _)| self.is_picked(q, *o))
                .filter_map(|(_, option)| option["label"].as_str().map(str::to_owned))
                .collect();
            said.extend(Some(self.other(q).trim().to_owned()).filter(|other| !other.is_empty()));
            if said.is_empty() {
                return None;
            }
            answers.insert(question["question"].as_str().unwrap_or_default().to_owned(), Value::String(said.join(", ")));
        }
        Some(answers)
    }
}

/// A model Claude Code offers, as `initialize` lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// What `set_model` takes, like `opus`.
    pub value: String,
    pub name: String,
    /// The effort levels it takes, none for a model without effort.
    pub efforts: Vec<String>,
    /// The model it stands for, like `claude-opus-5-5`.
    pub resolved: String,
    /// It has a fast mode.
    pub fast: bool,
    /// Claude Code's line about it, like "Most capable for complex work". Empty when it has none.
    pub description: String,
}

/// A slash command, as `initialize` lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub hint: String,
    /// Whether a plugin of Claude Code's gives it. Claude Code holds such a command until the turn ends.
    pub plugin: bool,
    /// One of Claude Code's own, which it lists from 2.1.330 on (`"builtin": true`), not a plugin's or a skill's.
    pub builtin: bool,
}

/// The permission modes Claude Code runs in, as `set_permission_mode` takes them, with what the chat calls them.
pub const MODES: [(&str, &str); 4] = [("default", "Ask first"), ("acceptEdits", "Accept edits"), ("plan", "Plan"), ("auto", "Auto")];

/// The ids of the requests the chat makes as Claude Code starts, which its answers come back under.
pub const INITIALIZE: &str = "initialize";
pub const CONTEXT: &str = "context";
pub const SETTINGS: &str = "settings";
pub const USAGE: &str = "usage";
/// The limits the chat shows, by Claude Code's names for them.
const LIMITS: [(&str, &str); 2] = [("5h", "five_hour"), ("week", "seven_day")];
/// The request that puts a new chat in the mode you set for them.
pub const START_MODE: &str = "start-mode";
/// The requests behind the usage and context views, apart from the opening ones: those ask for less.
pub const USAGE_VIEW: &str = "usage-view";
pub const CONTEXT_VIEW: &str = "context-view";
/// The opening request that has Claude Code keep the checkpoints a code rewind restores.
const CHECKPOINTS: &str = "checkpoints";
/// The opening request that has Claude Code send the text of its thinking.
const THINKING: &str = "thinking-display";
/// The rewind dialog's dry run, followed by the message's uuid, and the requests its choices make.
pub const REWIND_CHECK: &str = "rewind-check-";
pub const REWIND_FILES: &str = "rewind-files";
pub const REWIND_CONVERSATION: &str = "rewind-conversation";
/// A background task's output and its stop, each followed by the task's id.
pub const TASK_OUTPUT: &str = "task-output-";
pub const TASK_STOP: &str = "task-stop-";

#[derive(Debug, Default)]
pub struct Session {
    /// What auto mode blocked in this chat, for "Allow and retry" (`blocked.rs`).
    pub blocked: Vec<crate::blocked::Blocked>,
    /// Set when a block was kept, so the chat redraws once its time in Recently blocked is up (`chat.rs`).
    pub blocked_fresh: bool,
    /// Every call Claude Code refused on this session, for the side panel's Blocked (`refused.rs`).
    pub refusals: Vec<crate::refused::Refusal>,
    /// Whether this chat is on claude.ai and in the Claude app too.
    pub remote: crate::remote::Remote,
    pub items: Vec<Item>,
    /// The calls the transcript this start of Claude Code resumed holds without a result: the process that ran them is
    /// gone, so no result will come (`stop_replayed`).
    pub interrupted: HashSet<String>,
    /// The Skill calls whose skill an MCP server served, as their results say (`fromMcpServer`).
    pub mcp_skills: HashSet<String>,
    /// Claude's text of the message it is writing now, before the whole message comes.
    pub live: String,
    /// Claude is thinking before it writes.
    pub thinking: bool,
    pub id: Option<String>,
    /// The session id Claude Code names on its first lines, before its `init` and before anything is saved under it: only
    /// for the session sidebar to follow a new chat from the start, as `id` then can't resume or open it in a terminal.
    pub announced: Option<String>,
    /// The model the session runs on, as Claude Code names it (`claude-opus-5-5`) or as it was picked (`opus`).
    pub model: Option<String>,
    /// The effort picked in the chat.
    pub effort: Option<String>,
    /// The effort your settings give each model by its full name (`modelSettings`), and the one for the rest.
    model_efforts: Vec<(String, String)>,
    default_effort: Option<String>,
    /// How much of your plan's limits is used: ("5h", percent, when it resets in milliseconds) and the week's.
    pub limits: Vec<(&'static str, u64, Option<f64>)>,
    /// The windows the chat stopped Claude's turn for near the limit, each once (`plan_limit::stop_for`).
    pub limit_stops: Vec<crate::plan_limit::Stopped>,
    /// The permission mode, one of `MODES`' or another Claude Code has.
    pub mode: Option<String>,
    /// The mode the opening input asked for, which `mode` shows once Claude Code has accepted it.
    pub start_mode: Option<String>,
    pub models: Vec<Model>,
    pub commands: Vec<Command>,
    /// The tokens in the context window after the last message, and how many it holds.
    pub context: Option<(u64, u64)>,
    /// What the usage and context views show, once Claude Code has answered, or why it couldn't.
    pub usage_view: Option<Usage>,
    pub context_view: Option<ContextDetail>,
    pub view_error: Option<String>,
    /// What the plugins dialog shows, and the toggles in it.
    pub plugins: Plugins,
    /// The /hooks, /memory or /permissions dialog, while it is open, and what Claude Code answered its reads.
    pub setup: Setup,
    /// A turn is running: one is open (its first `requesting` status to its `result`), or a message Claude Code holds
    /// is not finished. Claude Code prints `session_state_changed` only when an environment variable asks for it, which
    /// Wings can't set, so this is worked out from the turn's own lines (`settle`). A turn can start with no message of
    /// yours, when a background task ends, and a message sent while one runs starts the next at once.
    pub working: bool,
    /// A call that can change files (an edit, a command, a subagent) ended since the chat last counted the changes.
    pub files_touched: bool,
    /// Between a turn's first `requesting` status and its `result`.
    turn_open: bool,
    /// The uuids of messages Claude Code has queued or started and not yet finished, from `command_lifecycle`.
    lifecycle: Vec<String>,
    /// Messages sent while Claude works, in the order Claude Code reads them, until it echoes each as read.
    pub queued: Vec<Queued>,
    /// What to tell you, as (title, body), for the chat to show when you aren't looking. At most one of each kind a turn.
    pub alerts: Vec<(String, String)>,
    /// Claude Code prints `session_state_changed`, so `working` follows it. It prints them only when asked to by an
    /// environment variable Wings can't set, so this is how the chat knows it may rely on them.
    reports_state: bool,
    asked_you: bool,
    /// When each call was made, by id (ms since the epoch), from its message's timestamp: the calls whose time is known so far.
    called: std::collections::HashMap<String, f64>,
    /// How long each finished call ran, in ms, by id. A call you were asked about, or a background command, is not here.
    pub ran: std::collections::HashMap<String, u64>,
    /// What the session has cost so far, in US dollars.
    pub cost: f64,
    /// The tokens of the turn under way, how far each subagent has got, and the prompt cache's clock (`spend.rs`).
    pub meter: Meter,
    /// Claude's checklist as it stands now.
    pub todos: Vec<Todo>,
    /// TaskCreate calls waiting for their result, which names the task's number: the call's id, its subject and its
    /// active form.
    creating: Vec<(String, String, String)>,
    /// You asked Claude to stop, and the turn hasn't ended yet.
    stopping: bool,
    /// What each subagent said and did, by the id of the tool call that started it, from the lines that carry it as their
    /// `parent_tool_use_id`. Each is a session of its own, so it is drawn with the same code as this one.
    pub subagents: HashMap<String, Session>,
    /// Claude Code's ids for the subagents, which their tabs and SendMessage name them by (`subagents.rs`).
    pub agents: crate::subagents::Agents,
    /// The time (ms) of the line being heard, which the page sets before each, as a session has no clock of its own.
    pub now: u64,
    /// When a subagent's first and last line came (ms), for how long it ran.
    pub started: u64,
    pub last: u64,
    /// The next message Claude Code predicts you will send, after a turn. Claude Code leaves it out when you turned
    /// prompt suggestions off in its settings.
    pub suggestion: Option<String>,
    /// The model named on the last message of Claude's heard, like `claude-haiku-5-5`.
    pub reported_model: Option<String>,
    /// Your messages Claude Code knows by a uuid, oldest first.
    pub prompts: Vec<Prompt>,
    /// The rewind dialog, while it is open.
    pub rewind: Option<Rewind>,
    /// What a rewound conversation gives back to the message box: the message it went back before, to edit and send.
    pub restored: Option<String>,
    /// The background tasks heard of, running or ended, in the order they started.
    pub tasks: Vec<Task>,
    /// The goal `/goal` set, while Claude Code works to meet it (`goal.rs`).
    pub goal: Option<crate::goal::Goal>,
    /// The scheduled tasks and the wakeup this chat's Claude set (`scheduled.rs`).
    pub scheduled: crate::scheduled::Scheduled,
    /// The plans Claude proposed with ExitPlanMode, which stay readable and commentable after the answer (`plan_doc.rs`).
    pub plans: crate::plan_doc::Plans,
    /// Fast mode and the output style.
    pub options: crate::composer::Options,
    /// What Claude Code says it is doing beyond a tool call, like compacting or retrying a request, for the status line.
    pub status: Option<String>,
    /// The session's name, as Claude Code last said it (`/rename`, or the one it gave).
    pub title: Option<String>,
    /// The title is one you gave the session, which the one Claude Code makes up doesn't replace.
    pub titled: bool,
    /// The conversation was cleared (`/clear`, `/new`, `/reset`), which the page reads once to drop what it kept of it.
    pub cleared: bool,
    /// The hand-backs already shown (their call, else their text), which stay known when a rewind or a clear takes their
    /// item away, so a transcript read again doesn't bring them back.
    pub reports_seen: Vec<String>,
    /// Claude Code stopped sending the effort with each turn (`per_turn_effort_changed`), so changing it rewrites the
    /// prompt cache. Its notice is shown once until an `init` turns it back on.
    pub effort_rewrites_cache: bool,
    /// When the thinking block being streamed began (ms), for how long the thought took.
    pub thought_since: u64,
    /// The dialogs and answers of `/status`, `/export` and `/btw`.
    pub cmds: Commands,
    /// Commands you moved to the background, and what MCP servers ask you (`actions.rs`, `elicitation.rs`).
    pub ops: Ops,
    /// What Claude Code's plugins show in the chat (`mods.rs`).
    pub mods: crate::mods::Mods,
    /// The files each turn changed, and Undo (`edits.rs`).
    pub edits: Edits,
    /// A hit usage limit, and waiting it out (`limit_wait.rs`).
    pub limit: crate::limit_wait::LimitWait,
    /// The rename dialog, and how the last turn ended (`titles.rs`).
    pub naming: crate::titles::Naming,
    /// How the background tasks nest, and the rows of workflow runs (`workflow.rs`).
    pub flows: crate::workflow::Flows,
    /// What the `!` commands printed that Claude hasn't read (`shell.rs`).
    pub shell: crate::shell::Shell,
    /// The `/skills` dialog (`skills.rs`).
    pub skills: crate::skills::Skills,
}

impl Session {
    /// Puts the notifications mods raised in the conversation, as notices (one that replaces a keyed one of the same mod
    /// takes its place), and returns the words to notify with.
    pub fn show_raised(&mut self) -> Vec<(String, String)> {
        let mut alerts = Vec::new();
        for raised in std::mem::take(&mut self.mods.raised) {
            let line = raised.line();
            let old = raised.replaces.and_then(|old| self.items.iter().rposition(|item| *item == Item::Notice(old.clone())));
            match old {
                Some(at) => self.items[at] = Item::Notice(line),
                None => self.items.push(Item::Notice(line)),
            }
            alerts.push((raised.head, raised.text));
        }
        alerts
    }

    /// Takes in one line Claude Code printed, or one transcript entry.
    pub fn hear(&mut self, line: &Value) {
        if let Some(id) = line["session_id"].as_str().filter(|id| self.announced.as_deref() != Some(*id)) {
            self.announced = Some(id.to_owned());
        }
        self.agents.learn(line);
        self.flows.heard(line);
        // A subagent's edits count for the turn that started it.
        self.edits.heard(line, self.items.len());
        // A subagent's own messages are kept apart, under the tool call that started it, and stay out of the conversation.
        if !line["parent_tool_use_id"].is_null() {
            if let Some(parent) = line["parent_tool_use_id"].as_str() {
                let now = self.now;
                let sub = self.subagents.entry(parent.to_owned()).or_default();
                (sub.now, sub.last) = (now, now);
                if sub.started == 0 {
                    sub.started = now;
                }
                sub.hear_own(line);
                let extra = sub.items.len().saturating_sub(SUBAGENT_KEPT);
                sub.items.drain(..extra);
            }
            self.list_subagent_blocks(line["parent_tool_use_id"].as_str());
            return;
        }
        self.hear_own(line);
        // The rows a mod places follow a message by its uuid.
        self.mods.note_message(line, self.items.len());
    }

    /// Lists the blocks the subagent that call `parent` started hit, read from its failed results, in Recently blocked
    /// (`blocked.rs`). The block's own `permission_denied` line does it first when Claude Code prints one; the same call is
    /// kept once.
    fn list_subagent_blocks(&mut self, parent: Option<&str>) {
        let Some((parent, sub)) = parent.and_then(|parent| Some((parent, self.subagents.get(parent)?))) else {
            return;
        };
        let name = self.items.iter().find_map(|item| match item {
            Item::Tool(tool) if tool.id == parent => Some(tool.summary.as_str()),
            _ => None,
        });
        let found: Vec<_> = sub
            .refusals
            .iter()
            .filter(|refusal| refusal.kind == crate::refused::Kind::Blocked && !self.blocked.iter().any(|kept| kept.tool_id == refusal.tool_id))
            .filter_map(|refusal| {
                let tool = sub.items.iter().find_map(|item| match item {
                    Item::Tool(tool) if tool.id == refusal.tool_id => Some(tool),
                    _ => None,
                })?;
                Some(crate::blocked::of_subagent(&format!("Auto mode blocked {}: {}", refusal.tool, refusal.reason), tool, name.unwrap_or_default(), self.now))
            })
            .collect();
        for blocked in found {
            crate::blocked::keep(self, blocked);
        }
    }

    fn hear_own(&mut self, line: &Value) {
        // Every turn starts with this status, also one a finished background task starts by itself, and Claude Code prints
        // none outside a turn. Its `init` line comes with it, but the chat's own scenes print that one to start a session.
        if line["type"] == "system" && line["subtype"] == "status" && line["status"] == "requesting" {
            self.turn_open = true;
            self.settle();
        }
        self.limit.heard(line);
        self.naming.heard(line);
        if notices::hear(self, line) {
            return;
        }
        let mut logged = None;
        if self.mods.hear(line, self.now, &mut logged) {
            // A plugin's log line is a notice in the conversation, which Claude doesn't read.
            self.items.extend(logged.map(Item::Notice));
            let said = self.show_raised();
            self.alerts.extend(said);
            return;
        }
        match line["type"].as_str().unwrap_or_default() {
            // The session's name as Claude Code keeps it in the transcript, as Wings' session lists name it: one you gave it
            // with /rename wins over the one Claude Code made up.
            "custom-title" => {
                if let Some(title) = line["customTitle"].as_str().map(str::trim).filter(|title| !title.is_empty()) {
                    self.title = Some(title.to_owned());
                    self.titled = true;
                }
            }
            "ai-title" if !self.titled => {
                if let Some(title) = line["aiTitle"].as_str().map(str::trim).filter(|title| !title.is_empty()) {
                    self.title = Some(title.to_owned());
                }
            }
            // A subagent's report to this chat. Claude Code adds it in the session's name (`isMeta` in the transcript), and it
            // is read before the arms that skip such lines.
            "user" if line["origin"]["handback"] == true => self.handed_back(&line["origin"]),
            // The same report when it came while Claude worked: the transcript keeps it as an attachment of that turn.
            "attachment" if line["attachment"]["type"] == "queued_command" && line["attachment"]["origin"]["handback"] == true => self.handed_back(&line["attachment"]["origin"]),
            // A message you sent while Claude worked, or a task's notice, as a session read back has it: the transcript
            // keeps it as an attachment of the turn that read it, not as a message. What another agent sent stays out.
            "attachment" if line["attachment"]["type"] == "queued_command" => {
                let queued = &line["attachment"];
                let yours = matches!(queued["origin"]["kind"].as_str(), None | Some("human"));
                match queued["commandMode"].as_str() {
                    Some("prompt") if yours => self.user(&queued["prompt"], None),
                    Some("task-notification") => self.user(&queued["prompt"], None),
                    _ => {}
                }
            }
            "system" if line["subtype"] == "init" => {
                self.id = line["session_id"].as_str().map(str::to_owned);
                // It names the model in full (`claude-opus-5-5`), which more than one choice stands for: the one picked, or
                // your settings', stays shown while it is that model.
                let reported = line["model"].as_str();
                if self.current_model().map(|model| model.resolved.as_str()) != reported {
                    self.model = reported.map(str::to_owned);
                }
                self.mode = line["permissionMode"].as_str().map(str::to_owned).or(self.mode.take());
                // Only an API key bills per token: a claude.ai login is "none", as is a cloud provider.
                self.meter.per_token = line["apiKeySource"].as_str().is_some_and(crate::spend::bills_per_token);
                if let Some(active) = line["per_turn_effort_active"].as_bool() {
                    self.effort_rewrites_cache = !active;
                }
            }
            "command_lifecycle" => {
                let uuid = line["command_uuid"].as_str().unwrap_or_default();
                match line["state"].as_str().unwrap_or_default() {
                    "queued" | "started" if !self.lifecycle.iter().any(|known| known == uuid) => self.lifecycle.push(uuid.to_owned()),
                    "completed" | "cancelled" => self.lifecycle.retain(|known| known != uuid),
                    _ => {}
                }
                self.settle();
            }
            "system" if line["subtype"] == "thinking_tokens" => self.meter.thinking(line["estimated_tokens"].as_u64().unwrap_or(0)),
            "system" if line["subtype"] == "task_progress" => {
                // A resumed agent's progress comes under the call that woke it, but its row is under the call that started it.
                let first = self.tasks.iter().find(|task| line["task_id"] == task.id.as_str()).map(|task| task.call.clone()).filter(|call| !call.is_empty());
                match first {
                    Some(call) => {
                        let mut line = line.clone();
                        line["tool_use_id"] = call.into();
                        self.meter.progress(&line);
                    }
                    None => self.meter.progress(line),
                }
            }
            "system" if line["subtype"] == "session_state_changed" => self.changed(line["state"].as_str().unwrap_or_default()),
            "system" if line["subtype"] == "background_tasks_changed" => self.tasks_now(&line["tasks"]),
            // A task's end, from its update and its notice, whichever comes first.
            "system" if line["subtype"] == "task_started" => {
                // Waking an agent with SendMessage starts it again under that message's call: the first call is its row's.
                // After a restart only the transcript knows that call.
                let id = line["task_id"].as_str().unwrap_or_default();
                let call = self.agents.call_of(id).or(line["tool_use_id"].as_str()).unwrap_or_default().to_owned();
                if let Some(task) = self.tasks.iter_mut().find(|task| task.id == id && task.call.is_empty()) {
                    task.call = call;
                }
            }
            "system" if line["subtype"] == "task_updated" || line["subtype"] == "task_notification" => {
                let status = line["patch"]["status"].as_str().or(line["status"].as_str()).filter(|status| ["completed", "failed", "stopped", "killed"].contains(status));
                if let (Some(task), Some(status)) = (self.tasks.iter_mut().find(|task| line["task_id"] == task.id.as_str()), status) {
                    task.ended = Some(status.to_owned());
                    task.ended_at = self.now;
                    task.stopping = false;
                }
                if line["subtype"] == "task_notification" {
                    self.task_finished(line);
                }
            }
            "control_response" => self.answered(&line["response"]),
            // Claude Code says how much is used when that changes, as when you hit a limit. The chat itself only asks when
            // you send something, so without this it showed what was used at your last message.
            "rate_limit_event" => {
                let windows = &line["rate_limit_info"]["unifiedWindows"];
                for (label, key) in LIMITS {
                    if let Some(used) = windows[key]["utilization"].as_f64() {
                        self.set_limit(label, used * 100.0, windows[key]["resetsAt"].as_f64().map(|at| at * 1000.0));
                    }
                }
            }
            "stream_event" => self.stream(&line["event"]),
            // It comes after the turn's result, and goes once anything else starts.
            "prompt_suggestion" => self.suggestion = line["suggestion"].as_str().filter(|text| !text.trim().is_empty()).map(str::to_owned),
            "assistant" => {
                self.suggestion = None;
                self.status = None;
                // A transcript says when each message was; a live line is the request being heard, which `message_start`
                // already timed.
                let usage = &line["message"]["usage"];
                match line["timestamp"].as_str().and_then(parse_time) {
                    Some(at) => self.meter.requested_at(at as u64, usage),
                    None if self.meter.requested == 0 => self.meter.requested_at(self.now, usage),
                    None => self.meter.note_cache(usage),
                }
                self.meter.message(line["message"]["id"].as_str(), usage);
                self.assistant(&line["message"], line["timestamp"].as_str().and_then(parse_time));
            }
            // After a rewind Claude Code leaves the dropped entries in the transcript and says what it cut after them.
            "last-prompt" if line["rewound"] == true => {
                let cut = line["cutUuid"].as_str().and_then(|uuid| self.prompts.iter().find(|prompt| prompt.uuid == uuid)).map(|prompt| prompt.at);
                if let Some(at) = cut {
                    self.cut(at);
                }
            }
            // Claude Code echoes each message of yours it reads when it runs with `--replay-user-messages`. The ones sent
            // while it was idle are on the page already.
            // What a `!` command printed, which waits for your next message and shows as one shell row.
            "user" if line["isReplay"] == true && line["message"]["content"].as_str().is_some_and(|text| self.shell.heard(text)) => {
                self.shell_row(line["message"]["content"].as_str().unwrap_or_default());
            }
            "user" if line["isReplay"] == true => self.echoed(line["uuid"].as_str().unwrap_or_default()),
            // What Claude Code adds in a turn on its own, like a skill's text once the Skill tool loads it (`isSynthetic` as it
            // prints it, `isMeta` in the transcript), isn't yours to see as a message.
            "user" if (line["isMeta"] != true && line["isSynthetic"] != true) || (line["isMeta"] == true && unmarked(&line["message"]["content"]).is_some()) => {
                let parent = line["parentUuid"].as_str();
                // In a transcript, an entry that follows what one of your messages followed took its place: the
                // conversation was rewound to before that message, which Claude Code leaves in the file.
                let replaced = parent.and_then(|parent| self.prompts.iter().find(|prompt| prompt.parent.as_deref() == Some(parent))).map(|prompt| prompt.at);
                if let Some(at) = replaced {
                    self.cut(at);
                }
                let first = self.items.len();
                self.mods.tool_answered(line);
                self.user(&unmarked(&line["message"]["content"]).unwrap_or_else(|| line["message"]["content"].clone()), line["timestamp"].as_str().and_then(parse_time));
                crate::scheduled::answered(self, line);
                self.served_skill(line);
                let said = (first..self.items.len()).find(|at| matches!(self.items[*at], Item::User(_) | Item::UserImages(..)));
                if let (Some(at), Some(uuid)) = (said, line["uuid"].as_str()) {
                    self.prompts.push(Prompt { at, uuid: uuid.to_owned(), parent: parent.map(str::to_owned) });
                }
            }
            "control_request" if line["request"]["subtype"] == "can_use_tool" => {
                let request = &line["request"];
                let tool = request["tool_name"].as_str().unwrap_or("a tool").to_owned();
                if !std::mem::replace(&mut self.asked_you, true) {
                    self.alerts.push((format!("Claude needs you: {tool}"), summary(&tool, &request["input"])));
                }
                // Claude Code says when a rule for this ask would be broader than the ask itself.
                let suggestions = match request["suppress_always_allow_rule"] == true {
                    true => Vec::new(),
                    false => request["permission_suggestions"].as_array().cloned().unwrap_or_default(),
                };
                let detail = ask_detail(&tool, &request["input"]);
                // The time it waited for you is not the time it ran.
                if let Some(call) = request["tool_use_id"].as_str() {
                    self.called.remove(call);
                }
                if tool == "ExitPlanMode" {
                    self.plans.asked(line["request_id"].as_str().unwrap_or_default(), request["tool_use_id"].as_str(), &request["input"]);
                }
                self.items.push(Item::Ask(Ask {
                    request: line["request_id"].as_str().unwrap_or_default().to_owned(),
                    summary: summary(&tool, &request["input"]),
                    detail: cut(&detail, ASK_KEPT),
                    cut_short: detail.len() > ASK_KEPT,
                    reason: reason(request),
                    suggestions,
                    tool,
                    input: request["input"].clone(),
                    preview: Default::default(),
                    answer: None,
                }));
            }
            // An MCP server asks you for details or to open a link, and Claude Code waits for the answer.
            "control_request" if line["request"]["subtype"] == "elicitation" => {
                if let Some(asked) = Elicitation::parse(line) {
                    if !std::mem::replace(&mut self.asked_you, true) {
                        self.alerts.push((format!("{} asks you", asked.server), cut(&asked.message, SUMMARY_KEPT)));
                    }
                    self.ops.elicitations.push(asked);
                }
            }
            // Claude Code no longer needs the answer: the turn was stopped, or another client answered.
            "control_cancel_request" => {
                if let Some(ask) = line["request_id"].as_str().and_then(|id| self.pending(id)) {
                    ask.answer = Some(false);
                }
                if let Some(id) = line["request_id"].as_str() {
                    self.withdraw_elicitation(id);
                }
            }
            "result" => {
                self.status = None;
                // With `session_state_changed` its `idle` ends the turn, else the result does. The messages it answered are
                // done, whether or not their `completed` line comes.
                self.turn_open = self.reports_state && self.turn_open;
                if let Some(answered) = line["user_message_uuids"].as_array() {
                    self.lifecycle.retain(|known| !answered.iter().any(|uuid| uuid == known.as_str()));
                }
                self.settle();
                self.live.clear();
                // The turn's own spend: its usage as the result says it, else what its messages added up to, and its share of
                // the session's running cost.
                let counted = self.meter.end_turn();
                let mut spend = Some(Spend::of(&line["usage"])).filter(|spend| spend.output > 0).unwrap_or(counted);
                spend.cost = line["total_cost_usd"].as_f64().map(|total| total - self.cost).filter(|cost| *cost > 0.0);
                self.cost = line["total_cost_usd"].as_f64().unwrap_or(self.cost);
                // A stopped turn ends as an error, with Claude Code's diagnostics as its text.
                if std::mem::take(&mut self.stopping) {
                    self.items.push(Item::Stopped);
                } else if line["is_error"] == true || line["subtype"].as_str().is_some_and(|s| s != "success") {
                    let said = line["result"].as_str().map(str::to_owned).or_else(|| line["errors"].as_array().map(|e| texts(e).join("\n")));
                    let said = said.filter(|s| !s.is_empty()).unwrap_or_else(|| "The turn ended with an error.".into());
                    // Claude Code also writes some errors, like a hit limit, as its reply: shown once, as the error.
                    if matches!(self.items.last(), Some(Item::Text(text)) if text.trim() == said.trim()) {
                        self.items.pop();
                    }
                    self.items.push(Item::Error(said));
                } else {
                    // As Claude Code timed the turn. A result of no turns, like one for a background task answered along
                    // with another, did no work of its own.
                    if let Some(ms) = line["duration_ms"].as_f64().filter(|_| line["num_turns"] != 0) {
                        self.items.push(Item::Worked(ms as u64, spend));
                    }
                }
                // A question in the next turn tells you again. "Finished" waits for the next turn's own output: a
                // turn can end with more than one result line.
                self.asked_you = false;
            }
            _ => {}
        }
    }

    /// Claude Code's `session_state_changed`: `idle` is its own word that the turn, and what follows it, is over.
    fn changed(&mut self, state: &str) {
        self.reports_state = true;
        match state {
            "idle" => {
                self.turn_open = false;
                self.lifecycle.clear();
            }
            "running" | "requires_action" => self.turn_open = true,
            _ => {}
        }
        self.settle();
    }

    /// Works `working` out: a turn is open, or a message Claude Code holds has not been finished.
    fn settle(&mut self) {
        self.working = self.turn_open || !self.lifecycle.is_empty();
        if !self.working {
            // Nothing will read what is still queued now, so it was read without an echo.
            for queued in std::mem::take(&mut self.queued) {
                self.read_queued(queued);
            }
        }
    }

    /// The Claude Code process ended: nothing runs any more.
    pub fn stopped_running(&mut self) {
        self.turn_open = false;
        self.lifecycle.clear();
        self.working = false;
    }

    /// Starts the conversation over as the new session `id`, as `/clear`, `/new` and `/reset` do: what was said, asked and
    /// planned is gone. What Claude Code reports about itself (models, commands, mode, limits) stays.
    pub fn reset(&mut self, id: &str) {
        self.items.clear();
        self.live.clear();
        self.thinking = false;
        self.prompts.clear();
        self.mods.cut(0);
        self.edits.clear();
        self.rewind = None;
        self.restored = None;
        self.todos.clear();
        self.creating.clear();
        self.subagents.clear();
        self.refusals.clear();
        self.agents.clear();
        self.tasks.clear();
        self.goal = None;
        self.scheduled = Default::default();
        self.suggestion = None;
        self.status = None;
        self.title = None;
        self.titled = false;
        self.stopping = false;
        self.cmds = Commands::default();
        self.id = Some(id.to_owned());
        self.cleared = true;
    }

    /// Claude Code read the message `uuid`: from the queue into the conversation.
    fn echoed(&mut self, uuid: &str) {
        if let Some(at) = self.queued.iter().position(|queued| queued.uuid == uuid) {
            let queued = self.queued.remove(at);
            self.read_queued(queued);
        }
    }

    /// A queued message, read: into the conversation, under its uuid.
    fn read_queued(&mut self, queued: Queued) {
        self.prompts.push(Prompt { at: self.items.len(), uuid: queued.uuid.clone(), parent: None });
        let ids = self.keep_pictures(&queued.images);
        self.items.push(sent_item(queued.text, ids));
    }

    /// The ids of pictures of yours (media type and base64), which Wings is to be given.
    fn keep_pictures(&mut self, pictures: &[(String, String)]) -> Vec<String> {
        pictures.iter().map(|(_, data)| images::keep(data)).collect()
    }

    /// Drops your message at `at` and everything after it.
    /// Says so at the top when a transcript read back came to the most entries the host gives (`chat::HISTORY_ALL`): a
    /// session longer than that has its earlier messages unread, which a conversation starting mid-way would hide.
    pub fn read_back(&mut self, entries: &[Value]) {
        if entries.len() >= HISTORY_ALL as usize {
            self.items.push(Item::Divider(EARLIER_NOT_LOADED.to_owned()));
        }
    }

    fn cut(&mut self, at: usize) {
        self.items.truncate(at);
        self.prompts.retain(|prompt| prompt.at < at);
        self.mods.cut(at);
        self.edits.cut(at);
    }

    /// The live set of background tasks, as `background_tasks_changed` gives it whole. One that left it has ended; one
    /// that ended and is back is a new run.
    fn tasks_now(&mut self, live: &Value) {
        let live: Vec<&Value> = live.as_array().into_iter().flatten().collect();
        for task in &mut self.tasks {
            let listed = live.iter().any(|entry| entry["task_id"] == task.id.as_str());
            match (listed, task.ended.is_some()) {
                (false, false) => (task.ended, task.ended_at) = (Some("ended".into()), self.now),
                (true, true) => (task.ended, task.ended_at, task.started, task.output) = (None, 0, self.now, None),
                _ => continue,
            }
            task.stopping = false;
        }
        for entry in live {
            let Some(id) = entry["task_id"].as_str().filter(|id| self.tasks.iter().all(|task| task.id != *id)) else {
                continue;
            };
            let text = |key: &str| entry[key].as_str().unwrap_or_default().to_owned();
            self.tasks.push(Task { id: id.to_owned(), kind: text("task_type"), description: text("description"), call: String::new(), started: self.now, ended: None, ended_at: 0, stopping: false, output: None, error: None });
        }
    }

    /// The line for a background task's notice in the stream, with how long it ran, and a note for you when you are
    /// elsewhere. Only tasks heard of in the live set count: a command that ran in the foreground sends a notice too.
    fn task_finished(&mut self, line: &Value) {
        let (Some(status), Some(task)) = (line["status"].as_str(), self.tasks.iter().find(|task| line["task_id"] == task.id.as_str())) else {
            return;
        };
        let summary = line["summary"].as_str().unwrap_or_default();
        let report = if line["summary_is_report"] == true || status != "completed" { summary } else { "" };
        let finished = Finished {
            task: task.id.clone(),
            call: line["tool_use_id"].as_str().unwrap_or(task.call.as_str()).to_owned(),
            kind: task_kind(&task.kind),
            description: task.description.clone(),
            status: status.to_owned(),
            took: Some(self.now.saturating_sub(task.started)).filter(|_| task.started > 0),
            report: cut(report, OUTPUT_KEPT),
        };
        self.items.push(Item::Finished(finished));
    }

    /// How long the call `id` ran (ms): from its line's time to its result's, or for a command in the background, which
    /// answers at once, what its notice said when it finished.
    pub fn ran_for(&self, id: &str) -> Option<u64> {
        self.ran.get(id).copied().or_else(|| {
            self.items.iter().rev().find_map(|item| match item {
                Item::Finished(done) if done.call == id => done.took,
                _ => None,
            })
        })
    }

    /// The hand-backs among transcript `entries`, which a chat reads at the end of a turn as Claude Code may print no line
    /// for one: each shows once, whatever number of times it is read.
    /// The names the transcript gives the session: Claude Code writes the one it makes up (`ai-title`) there and doesn't
    /// print it, so a live chat named its tab and its Agents row after the first message.
    pub fn hear_titles(&mut self, entries: &[Value]) {
        for entry in entries.iter().filter(|entry| matches!(entry["type"].as_str(), Some("ai-title" | "custom-title"))) {
            self.hear(entry);
        }
    }

    pub fn hear_handbacks(&mut self, entries: &[Value]) {
        for entry in entries.iter().filter(|entry| entry["origin"]["handback"] == true || entry["attachment"]["origin"]["handback"] == true) {
            self.hear(entry);
        }
    }

    /// A subagent's hand-back as an item of its own, once: the live line and the transcript's entry for the same call are one.
    fn handed_back(&mut self, origin: &Value) {
        let text = origin["handbackReportText"].as_str().map(str::trim).unwrap_or_default();
        if text.is_empty() {
            return;
        }
        let call = origin["handbackToolUseId"].as_str().unwrap_or_default();
        let key = if call.is_empty() { text } else { call };
        if self.reports_seen.iter().any(|seen| seen == key) {
            return;
        }
        self.reports_seen.push(key.to_owned());
        let note = origin["handbackNote"].as_str().map(str::trim).filter(|note| !note.is_empty()).map(str::to_owned);
        let text_of = |key: &str| origin[key].as_str().unwrap_or_default().to_owned();
        self.items.push(Item::Report(Report { from: text_of("from"), name: text_of("name"), text: text.to_owned(), note, call: call.to_owned() }));
    }

    /// Claude Code's answer to one of the chat's requests.
    fn answered(&mut self, response: &Value) {
        let answer = &response["response"];
        let id = response["request_id"].as_str().unwrap_or_default();
        // A reload brings the commands as they are now: a plugin's skills come and go with it.
        if id == plugins::RELOAD && response["subtype"] == "success" {
            self.commands = commands(&answer["commands"]);
        }
        if let Some(refused) = crate::remote::answered(&mut self.remote, id, response) {
            self.items.extend(refused.map(Item::Notice));
            return;
        }
        if self.plugins.answered(id, response)
            || self.ops.answered(id, response, &mut self.items)
            || self.answered_rewind(id, response)
            || self.edits.answered(id, response, &self.prompts, &mut self.items)
            || self.answered_task(id, response)
            || self.setup.answered(id, response)
            || self.skills.answered(id, response)
            || self.plans.answered(id, response)
            || self.cmds.answered(id, response)
            || self.mods.answered(id, response)
            || self.naming.answered(id, response, &mut self.title, &mut self.titled, &mut self.items)
        {
            return;
        }
        if response["subtype"] == "error" && (id == USAGE_VIEW || id == CONTEXT_VIEW) {
            self.view_error = Some(response["error"].as_str().unwrap_or("Claude Code didn't answer.").to_owned());
            return;
        }
        match id {
            USAGE_VIEW => {
                self.usage_view = Usage::parse(answer);
                self.view_error = self.usage_view.is_none().then(|| "Claude Code's answer had no usage in it.".to_owned());
                // The panel beside the dialog shows the same fresh numbers.
                self.take_limits(&answer["rate_limits"]);
            }
            CONTEXT_VIEW => {
                self.context_view = ContextDetail::parse(answer);
                self.view_error = self.context_view.is_none().then(|| "Claude Code's answer had no context in it.".to_owned());
            }
            INITIALIZE => {
                let text = |value: &Value, key: &str| value[key].as_str().unwrap_or_default().to_owned();
                self.models = answer["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|model| Model {
                        value: text(model, "value"),
                        name: text(model, "displayName"),
                        efforts: model["supportedEffortLevels"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
                        resolved: text(model, "resolvedModel"),
                        fast: model["supportsFastMode"] == true,
                        description: text(model, "description"),
                    })
                    .collect();
                self.options.initialized(answer);
                self.commands = commands(&answer["commands"]);
                self.mode = answer["current_permission_mode"].as_str().map(str::to_owned).or(self.mode.take());
            }
            // It is answered after `initialize` and the init line, which still say the mode Claude Code started in.
            START_MODE if response["subtype"] == "success" => self.mode = self.start_mode.clone().or(self.mode.take()),
            CONTEXT => {
                if let (Some(used), Some(most)) = (answer["totalTokens"].as_u64(), answer["maxTokens"].as_u64()) {
                    self.context = Some((used, most));
                }
            }
            // Your settings, merged as Claude Code reads them: the model and effort the chat shows until you pick
            // others, as a terminal's Claude starts on them.
            SETTINGS => {
                let settings = &answer["effective"];
                if self.model.is_none() {
                    self.model = settings["model"].as_str().map(str::to_owned);
                }
                self.default_effort = settings["effortLevel"].as_str().map(str::to_owned);
                self.options.applied(&answer["applied"]);
                self.model_efforts = settings["modelSettings"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(model, set)| Some((model.clone(), set["effortLevel"].as_str()?.to_owned())))
                    .collect();
            }
            USAGE => self.take_limits(&answer["rate_limits"]),
            _ => {}
        }
    }

    fn stream(&mut self, event: &Value) {
        match event["type"].as_str().unwrap_or_default() {
            "content_block_start" => {
                self.status = None;
                self.thinking = event["content_block"]["type"] == "thinking";
                if self.thinking {
                    self.thought_since = self.now;
                }
                if event["content_block"]["type"] == "text" {
                    self.live.clear();
                }
            }
            "content_block_stop" => self.thinking = false,
            // The request has gone out, which restarts the prompt cache's clock; the message's usage so far follows in
            // `message_delta`.
            "message_start" => {
                let message = &event["message"];
                self.meter.requested_at(self.now, &message["usage"]);
                self.meter.message(message["id"].as_str(), &message["usage"]);
            }
            "message_delta" => self.meter.update(&event["usage"]),
            "content_block_delta" if event["delta"]["type"] == "text_delta" => {
                self.live.push_str(event["delta"]["text"].as_str().unwrap_or_default());
            }
            _ => {}
        }
    }

    fn assistant(&mut self, message: &Value, at: Option<f64>) {
        if let Some(model) = message["model"].as_str() {
            self.reported_model = Some(model.to_owned());
        }
        // What the window holds now: everything sent in, cached or not, and what came out.
        let usage = &message["usage"];
        let used: u64 = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens", "output_tokens"].iter().filter_map(|key| usage[*key].as_u64()).sum();
        if let Some((_, most)) = self.context.filter(|_| used > 0) {
            self.context = Some((used, most));
        }
        for block in message["content"].as_array().into_iter().flatten() {
            match block["type"].as_str().unwrap_or_default() {
                "text" => {
                    self.live.clear();
                    let text = block["text"].as_str().unwrap_or_default().trim();
                    if !text.is_empty() {
                        self.items.push(Item::Text(text.to_owned()));
                    }
                }
                "thinking" => {
                    let text = block["thinking"].as_str().unwrap_or_default().trim();
                    if !text.is_empty() {
                        let took = if self.thought_since > 0 { self.now.saturating_sub(self.thought_since) } else { 0 };
                        self.items.push(Item::Thought(cut(text, OUTPUT_KEPT), took));
                    }
                    self.thought_since = 0;
                }
                "tool_use" => {
                    // A gateway can send a call with no name, which Claude Code answers with an error.
                    let name = block["name"].as_str().filter(|name| !name.trim().is_empty()).unwrap_or(UNNAMED).to_owned();
                    let id = block["id"].as_str().unwrap_or_default().to_owned();
                    self.track(&name, &id, &block["input"]);
                    self.mods.tool_called(&id, &block["input"]);
                    // A command in the background answers at once, so what it took is not when its answer came.
                    if let (Some(at), false) = (at, block["input"]["run_in_background"] == true) {
                        self.called.insert(id.clone(), at);
                    }
                    // A subagent's report to its caller is its last word: whole, as a reply, where a call's one-line
                    // summary cut it to 200 characters.
                    if name == HANDBACK
                        && let Some(report) = block["input"]["message"].as_str().map(str::trim).filter(|report| !report.is_empty())
                    {
                        self.items.push(Item::Text(report.to_owned()));
                        continue;
                    }
                    self.items.push(Item::Tool(Tool { id, summary: summary(&name, &block["input"]), detail: detail(&name, &block["input"]), input: input_of(&name, &block["input"]), name, output: None, images: Vec::new() }));
                }
                "image" => {
                    self.items.push(Item::Images(images::of_block(block).into_iter().collect()));
                }
                _ => {}
            }
        }
    }

    fn user(&mut self, content: &Value, at: Option<f64>) {
        if let Some(text) = content.as_str() {
            if self.shell_row(text) {
                return;
            }
            return self.items.extend(said_or_finished(text));
        }
        let first = self.items.len();
        let mut pictures: Vec<String> = Vec::new();
        for block in content.as_array().into_iter().flatten() {
            match block["type"].as_str().unwrap_or_default() {
                "image" => pictures.extend(images::of_block(block)),
                // What Claude Code writes in your name when a turn is stopped, which `Stopped` shows.
                "text" if block["text"].as_str().is_some_and(|text| text.starts_with("[Request interrupted by user")) => {}
                "text" => {
                    let text = block["text"].as_str().unwrap_or_default();
                    if !self.shell_row(text) {
                        self.items.extend(said_or_finished(text));
                    }
                }
                "tool_result" => {
                    let id = block["tool_use_id"].as_str().unwrap_or_default();
                    let output = match &block["content"] {
                        Value::String(text) => text.clone(),
                        Value::Array(parts) => texts(parts).join("\n"),
                        _ => String::new(),
                    };
                    let output = unwrapped(&output).to_owned();
                    // A Read of an image, a screenshot or an MCP tool gives pictures back as image blocks.
                    let shots: Vec<String> = block["content"].as_array().into_iter().flatten().filter(|part| part["type"] == "image").take(images::PER_RESULT).filter_map(images::of_block).collect();
                    let tool = self.items.iter_mut().rev().find_map(|item| match item {
                        Item::Tool(tool) if tool.id == id => Some(tool),
                        _ => None,
                    });
                    let mut refusal = None;
                    if let Some(tool) = tool {
                        tool.output = Some((cut(&output, OUTPUT_KEPT), block["is_error"] == true));
                        refusal = (block["is_error"] == true).then(|| crate::refused::of(tool, &output)).flatten();
                        if let (Some(end), Some(start)) = (at, self.called.remove(id)) {
                            self.ran.insert(id.to_owned(), (end - start).max(0.0) as u64);
                        }
                        tool.images = shots;
                        self.files_touched |= matches!(tool.name.as_str(), "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "Bash" | "Task" | "Agent");
                    }
                    if let Some(refusal) = refusal {
                        crate::refused::keep(self, refusal);
                    }
                    if let Some(at) = self.creating.iter().position(|(call, ..)| call == id) {
                        let (_, text, active) = self.creating.remove(at);
                        // "Task #3 created successfully: ..."
                        let number = output.strip_prefix("Task #").and_then(|rest| rest.split_whitespace().next());
                        if let (Some(number), false) = (number, block["is_error"] == true) {
                            self.todos.push(Todo { id: number.to_owned(), text, active, progress: Progress::Pending });
                        }
                    }
                }
                _ => {}
            }
        }
        if !pictures.is_empty() {
            // The pictures belong to the text of that message, else they are a message of their own.
            match self.items[first..].iter().position(|item| matches!(item, Item::User(_))) {
                Some(at) => {
                    if let Item::User(text) = &mut self.items[first + at] {
                        let text = std::mem::take(text);
                        self.items[first + at] = Item::UserImages(text, pictures);
                    }
                }
                None => self.items.insert(first, Item::UserImages(String::new(), pictures)),
            }
        }
    }

    /// Records one terminal row from a `!` command. Input starts its row and output completes it.
    fn shell_row(&mut self, text: &str) -> bool {
        let text = text.trim();
        if !(text.starts_with("<bash-input>") || text.starts_with("<bash-stdout>")) {
            return false;
        }
        let inner = |name: &str| text.split_once(&format!("<{name}>"))?.1.split_once(&format!("</{name}>")).map(|(inside, _)| inside.trim());
        if let Some(command) = inner("bash-input") {
            let id = format!("shell-{}", self.items.len());
            self.items.push(Item::Shell(Tool {
                id,
                name: "Shell".into(),
                summary: format!("$ {command}"),
                output: None,
                detail: Detail::Plain,
                input: Input::default(),
                images: Vec::new(),
            }));
            return true;
        }
        let Some(stdout) = inner("bash-stdout") else {
            return false;
        };
        let stderr = inner("bash-stderr").unwrap_or_default();
        let code = inner("bash-exit-code").and_then(|code| code.parse::<i32>().ok()).unwrap_or_default();
        let failed = code != 0;
        // As a Bash call's output reads: the exit code first when it failed, then what it printed, unlabelled.
        let exit = failed.then(|| format!("Exit code {code}")).unwrap_or_default();
        let output = [exit.as_str(), stdout, stderr].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n");
        if let Some(Item::Shell(shell)) = self.items.iter_mut().rev().find(|item| matches!(item, Item::Shell(Tool { output: None, .. }))) {
            shell.output = Some((cut(output.trim_end(), OUTPUT_KEPT), failed));
        }
        true
    }

    /// Keeps the checklist up with the calls that change it.
    fn track(&mut self, tool: &str, id: &str, input: &Value) {
        self.scheduled.called(tool, id, input);
        let text = |key: &str| input[key].as_str().map(str::to_owned);
        match tool {
            "TodoWrite" => {
                if let Some(todos) = input["todos"].as_array() {
                    self.todos = todos
                        .iter()
                        .enumerate()
                        .filter_map(|(n, todo)| {
                            let content = todo["content"].as_str()?.to_owned();
                            Some(Todo {
                                id: (n + 1).to_string(),
                                active: todo["activeForm"].as_str().unwrap_or(&content).to_owned(),
                                text: content,
                                progress: progress(todo["status"].as_str().unwrap_or_default()),
                            })
                        })
                        .collect();
                }
            }
            "TaskCreate" => {
                if let Some(subject) = text("subject") {
                    let active = text("activeForm").unwrap_or_else(|| subject.clone());
                    self.creating.push((id.to_owned(), subject, active));
                }
            }
            "TaskUpdate" => {
                let task = input["taskId"].as_str().map(str::to_owned).or_else(|| input["taskId"].as_u64().map(|n| n.to_string()));
                let Some(at) = task.and_then(|task| self.todos.iter().position(|todo| todo.id == task)) else {
                    return;
                };
                if input["status"] == "deleted" {
                    self.todos.remove(at);
                    return;
                }
                let todo = &mut self.todos[at];
                if let Some(status) = input["status"].as_str() {
                    todo.progress = progress(status);
                }
                if let Some(subject) = text("subject") {
                    todo.text = subject;
                }
                if let Some(active) = text("activeForm") {
                    todo.active = active;
                }
            }
            "ExitPlanMode" => self.plans.seen(id, input),
            _ => {}
        }
    }

    /// The line that sends `text` and the pictures `images` (media type and base64) as your message, and the message
    /// shown at once. It goes under a uuid made from `stamp` and `n`, as a queued one does, which a rewind names.
    pub fn send(&mut self, text: &str, images: &[(String, String)], stamp: u64, n: u32) -> String {
        self.send_as(text, images, stamp, n, false)
    }

    /// `send`, for text that isn't what you typed: another agent's message or a prompt a tool opened the chat with. Claude
    /// Code is told so (`relayed`), so it doesn't take it for your own words.
    pub fn send_as(&mut self, text: &str, images: &[(String, String)], stamp: u64, n: u32, relayed: bool) -> String {
        let uuid = uuid(stamp, n);
        self.prompts.push(Prompt { at: self.items.len(), uuid: uuid.clone(), parent: None });
        let ids = self.keep_pictures(images);
        self.items.push(sent_item(text.to_owned(), ids));
        self.turn_open = true;
        self.working = true;
        self.suggestion = None;
        self.limit.user_spoke();
        self.naming.user_spoke();
        // The limits are asked again with each message: the chat may only write when you send something.
        let content = crate::shell::with_context(content(text, images), &self.shell.take());
        let mut message = json!({ "type": "user", "message": { "role": "user", "content": content }, "parent_tool_use_id": null, "uuid": uuid });
        relay(&mut message, relayed);
        line(&message) + &usage()
    }

    /// Whether `text` is one of a Claude Code plugin's own slash commands (its entry in the commands list has a `plugin`
    /// key), which Claude Code holds until the turn ends, whatever the priority it was sent with.
    pub fn runs_after_turn(&self, text: &str) -> bool {
        let name = text.trim_start().strip_prefix('/').and_then(|rest| rest.split_whitespace().next());
        name.is_some_and(|name| self.commands.iter().any(|command| command.plugin && command.name == name))
    }

    /// The line that sends `text` while Claude works, and the message queued until Claude reads it. It goes in at
    /// Claude's next step, as a terminal's does: sent to wait for the turn's end, a message sat there for minutes while
    /// Claude ran command after command. `stamp` (the time in milliseconds) and `n` (this write's number) make its uuid:
    /// the plugin has no randomness, and the uuid must differ from the ones of this session's earlier starts, which share
    /// its transcript.
    pub fn queue(&mut self, text: &str, images: Vec<(String, String)>, stamp: u64, n: u32) -> String {
        self.queue_as(text, images, stamp, n, false)
    }

    /// `queue`, for text that isn't what you typed (`send_as`).
    pub fn queue_as(&mut self, text: &str, images: Vec<(String, String)>, stamp: u64, n: u32, relayed: bool) -> String {
        let uuid = uuid(stamp, n);
        self.suggestion = None;
        self.limit.user_spoke();
        self.naming.user_spoke();
        let sent = user_line(text, &images, "next", &uuid, &self.shell.take(), relayed);
        self.queued.push(Queued { uuid, text: text.to_owned(), images, relayed });
        sent
    }

    /// The line that takes the queued message `uuid` back, which is then gone from the queue. None when it isn't there.
    pub fn unqueue(&mut self, uuid: &str, n: u32) -> Option<String> {
        let at = self.queued.iter().position(|queued| queued.uuid == uuid)?;
        self.queued.remove(at);
        Some(cancel(uuid, n))
    }

    /// The lines that take the queued message `uuid` back and send it again as one that can't wait: Claude Code moves the
    /// command running to the background and reads it at once, as when you type in its terminal. It keeps its place in the
    /// queue, under a new uuid, since Claude Code may keep the cancelled one in mind.
    pub fn send_now(&mut self, uuid: &str, stamp: u64, n: u32) -> Option<String> {
        let queued = self.queued.iter_mut().find(|queued| queued.uuid == uuid)?;
        let fresh = self::uuid(stamp, n);
        let mut now: Value = serde_json::from_str(&user_line(&queued.text, &queued.images, "now", &fresh, "", queued.relayed)).unwrap_or_default();
        // Claude Code puts work aside for a message only when a person sent it.
        if !queued.relayed {
            now["origin"] = json!({ "kind": "human" });
        }
        let lines = [cancel(uuid, n), line(&now)].concat();
        queued.uuid = fresh;
        Some(lines)
    }

    /// The request line that fills the usage or context dialog: one read, for the gesture that opens it.
    pub fn open_view(&mut self, panel: Panel) -> String {
        // What the dialog showed last stays until the new answer replaces it.
        self.view_error = None;
        match panel {
            // Without `skip_behaviors`, so what fills the limits comes too.
            Panel::Usage => request(USAGE_VIEW, json!({ "subtype": "get_usage" })),
            Panel::Context => request(CONTEXT_VIEW, json!({ "subtype": "get_context_usage", "detail": "full" })),
        }
    }

    /// What the chat asks Claude Code as it starts: the models and commands it has, how full the context is, your
    /// settings and how much of your plan's limits is used, to keep file checkpoints, and, with a `mode`, to start in it.
    pub fn opening(mode: Option<&str>) -> String {
        let mode = mode.map(|mode| request(START_MODE, json!({ "subtype": "set_permission_mode", "mode": mode })));
        [
            request(INITIALIZE, json!({ "subtype": "initialize" })),
            request(CONTEXT, json!({ "subtype": "get_context_usage" })),
            request(SETTINGS, json!({ "subtype": "get_settings" })),
            usage(),
            // Claude Code leaves a thinking block's text empty unless it is asked for a summary of it. Without
            // `max_thinking_tokens` the thinking budget stays as it was.
            request(THINKING, json!({ "subtype": "set_max_thinking_tokens", "thinking_display": "summarized" })),
            // A terminal's Claude Code keeps them for /rewind by itself; with -p it answers "File rewinding is not
            // enabled." unless this variable is set, which its flag settings do as well as the environment.
            request(CHECKPOINTS, json!({ "subtype": "apply_flag_settings", "settings": { "env": { "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING": "1" } } })),
            // Claude Code's plugins show their status lines, toasts and panes to the chat as a surface of their own.
            crate::mods::attach(),
        ]
        .into_iter()
        .chain(mode)
        .collect()
    }

    /// The line that switches the session to `model`, which is then shown.
    pub fn set_model(&mut self, model: &str, n: u32) -> String {
        self.model = Some(model.to_owned());
        request(&format!("model-{n}"), json!({ "subtype": "set_model", "model": model }))
    }

    /// The line that sets how hard Claude thinks from the next message on.
    pub fn set_effort(&mut self, effort: &str, n: u32) -> String {
        self.effort = Some(effort.to_owned());
        request(&format!("effort-{n}"), json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": effort } }))
    }

    /// The line that switches the permission mode.
    pub fn set_mode(&mut self, mode: &str, n: u32) -> String {
        self.mode = Some(mode.to_owned());
        request(&format!("mode-{n}"), json!({ "subtype": "set_permission_mode", "mode": mode }))
    }

    /// The lines that switch to the model `model`, from the menu's Model group. The effort stays as it is when the model
    /// takes it, else it becomes the one the menu shows for that model (High, else its first). None when it is the one in
    /// use, or none is listed.
    pub fn pick_model(&mut self, model: &str, n: u32) -> Option<String> {
        let next = self.models.iter().find(|listed| listed.value == model)?.clone();
        if self.current_model().map(|current| current.value.as_str()) == Some(model) {
            return None;
        }
        let before = self.shown_effort();
        let mut lines = self.set_model(model, n);
        match before.filter(|effort| next.efforts.contains(effort)) {
            // The effort you had stays. One only your settings gave the old model is pinned, as they may give the new
            // model another.
            Some(_) if self.effort.is_some() => {}
            Some(effort) => lines.push_str(&self.set_effort(&effort, n)),
            None => {
                if let Some(effort) = next.efforts.iter().find(|effort| *effort == "high").or(next.efforts.first()) {
                    lines.push_str(&self.set_effort(effort, n));
                }
            }
        }
        Some(lines)
    }

    /// The line that sets the effort `effort`, from the menu's Effort group, for a model that takes it. None when it is
    /// the one shown, or the model doesn't list it.
    pub fn pick_effort(&mut self, effort: &str, n: u32) -> Option<String> {
        let listed = self.current_model().is_some_and(|model| model.efforts.iter().any(|level| level == effort));
        (listed && self.shown_effort().as_deref() != Some(effort)).then(|| self.set_effort(effort, n))
    }

    /// The effort a model starts on, which the Effort row says is recommended: High, else the model's first.
    pub fn recommended_effort(&self) -> Option<String> {
        let model = self.current_model().or_else(|| self.models.iter().find(|model| model.value == "default"))?;
        model.efforts.iter().find(|effort| *effort == "high").or(model.efforts.first()).cloned()
    }

    /// The effort the menu shows as chosen: the one in use when the model lists it, else High, else its first. None for
    /// a model without effort.
    pub fn shown_effort(&self) -> Option<String> {
        let model = self.current_model().or_else(|| self.models.iter().find(|model| model.value == "default"))?;
        self.current_effort().filter(|effort| model.efforts.contains(effort)).or_else(|| self.recommended_effort())
    }

    /// The effort Claude works at: the one picked here, else the one your settings give the model, else theirs for all.
    pub fn current_effort(&self) -> Option<String> {
        let model = self.current_model().map(|model| model.resolved.as_str());
        self.effort
            .clone()
            .or_else(|| self.model_efforts.iter().find(|(name, _)| Some(name.as_str()) == model).map(|(_, effort)| effort.clone()))
            .or_else(|| self.default_effort.clone())
    }

    /// The model the session runs on, among `models`: by its value, by the full name Claude Code reports, or by its family.
    pub fn current_model(&self) -> Option<&Model> {
        let model = self.model.as_deref()?;
        // "Default" stands for a model too, but it's the one you'd pick by its own name.
        let named = |m: &&Model| m.resolved == model && m.value != "default";
        self.models.iter().find(|m| m.value == model).or_else(|| self.models.iter().find(named)).or_else(|| self.models.iter().find(|m| m.resolved == model)).or_else(|| {
            let family = model.trim_start_matches("claude-").split('-').next()?;
            self.models.iter().find(|m| m.value == family)
        })
    }

    /// The line that asks Claude to stop the turn it is on, the `n`th time you have. Messages queued behind it stay
    /// queued, as Claude Code leaves them on a plain `interrupt` (they are listed under its `still_queued`): it reads them
    /// as the next turn once this one has stopped. `cancel_queued` would drop them, which is not what Stop is for here.
    pub fn interrupt(&mut self, n: u32) -> String {
        self.stopping = true;
        request(&format!("interrupt-{n}"), json!({ "subtype": "interrupt" }))
    }

    /// The question `request` while it waits for an answer.
    fn pending(&mut self, request: &str) -> Option<&mut Ask> {
        self.items.iter_mut().find_map(|item| match item {
            Item::Ask(ask) if ask.request == request && ask.answer.is_none() => Some(ask),
            _ => None,
        })
    }

    /// The line that allows what `request` asks, which is then shown answered. With `always` it also gives the rules
    /// Claude Code suggested, so it stops asking. None when there is no such question waiting.
    pub fn allow(&mut self, request: &str, always: bool) -> Option<String> {
        let ask = self.pending(request)?;
        ask.answer = Some(true);
        let mut response = json!({ "behavior": "allow", "updatedInput": ask.input });
        if always && !ask.suggestions.is_empty() {
            response["updatedPermissions"] = Value::Array(ask.suggestions.clone());
        }
        Some(answer_line(request, response))
    }

    /// The line that denies `request` with `message`, which Claude reads: what you wrote, or else a plain no.
    pub fn deny(&mut self, request: &str, message: &str) -> Option<String> {
        let ask = self.pending(request)?;
        ask.answer = Some(false);
        let message = Some(message.trim()).filter(|message| !message.is_empty()).unwrap_or("The user said no to this in the chat.");
        Some(answer_line(request, json!({ "behavior": "deny", "message": message })))
    }

    /// The line that answers the questions of `request` with what `draft` holds. None when one has no answer yet, or
    /// there is no such question waiting.
    pub fn submit(&mut self, request: &str, draft: &Draft) -> Option<String> {
        let ask = self.pending(request)?;
        let answers = draft.answers(&ask.input["questions"])?;
        ask.answer = Some(true);
        let mut input = ask.input.clone();
        input["answers"] = Value::Object(answers);
        Some(answer_line(request, json!({ "behavior": "allow", "updatedInput": input })))
    }

    /// The lines that approve the plan of `request` and switch to `mode`: both in one write, as one click makes them.
    pub fn approve_plan(&mut self, request: &str, mode: &str, n: u32) -> Option<String> {
        let allow = self.allow(request, false)?;
        Some(allow + &self.set_mode(mode, n))
    }

    /// Opens the rewind dialog for your message `uuid`, and the dry run that asks which files restoring the code to before
    /// it would change, which only reads. None for a message Claude Code knows by no uuid.
    pub fn open_rewind(&mut self, uuid: &str) -> Option<String> {
        let prompt = self.prompts.iter().find(|prompt| prompt.uuid == uuid)?;
        let text = match self.items.get(prompt.at) {
            Some(Item::User(said) | Item::UserImages(said, _)) => said.clone(),
            _ => String::new(),
        };
        self.rewind = Some(Rewind { uuid: uuid.to_owned(), text, check: None, waiting: Vec::new(), error: None });
        Some(request(&format!("{REWIND_CHECK}{uuid}"), json!({ "subtype": "rewind_files", "user_message_id": uuid, "dry_run": true })))
    }

    /// The requests a choice in the rewind dialog makes, in the one write its click allows: the code back to before the
    /// message, the conversation, or both, code first. None while an earlier choice waits, and for code until the dry run
    /// has listed the files it changes.
    pub fn restore(&mut self, how: Restore) -> Option<String> {
        // Claude Code refuses to cut a conversation with messages after the one named that it can't tell you saw.
        let seen = self.prompts.last()?.uuid.clone();
        let rewind = self.rewind.as_mut().filter(|rewind| rewind.waiting.is_empty())?;
        let code = how != Restore::Conversation;
        if code && !matches!(&rewind.check, Some(Ok(check)) if !check.files.is_empty()) {
            return None;
        }
        let uuid = rewind.uuid.clone();
        let mut lines = String::new();
        if code {
            lines.push_str(&request(REWIND_FILES, json!({ "subtype": "rewind_files", "user_message_id": uuid })));
            rewind.waiting.push(REWIND_FILES);
        }
        if how != Restore::Code {
            lines.push_str(&request(REWIND_CONVERSATION, json!({ "subtype": "rewind_conversation", "target_message_uuid": uuid, "last_seen_user_message_uuid": seen })));
            rewind.waiting.push(REWIND_CONVERSATION);
        }
        rewind.error = None;
        Some(lines)
    }

    /// Claude Code's answer to the rewind dialog's requests. False for any other.
    fn answered_rewind(&mut self, id: &str, response: &Value) -> bool {
        let answer = &response["response"];
        let why = || answer["error"].as_str().or(response["error"].as_str()).unwrap_or("Claude Code didn't say why.").to_owned();
        if let Some(uuid) = id.strip_prefix(REWIND_CHECK) {
            if let Some(rewind) = self.rewind.as_mut().filter(|rewind| rewind.uuid == uuid) {
                rewind.check = Some(match answer["canRewind"] == true {
                    true => Ok(Check {
                        files: answer["filesChanged"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
                        added: answer["insertions"].as_u64().unwrap_or(0),
                        removed: answer["deletions"].as_u64().unwrap_or(0),
                    }),
                    false => Err(why()),
                });
            }
            return true;
        }
        let error = match id {
            REWIND_FILES => (answer["canRewind"] != true).then(|| format!("Couldn't restore the code: {}", why())),
            REWIND_CONVERSATION => (answer["rewound"] != true).then(|| format!("Couldn't restore the conversation: {}", why())),
            _ => return false,
        };
        if id == REWIND_FILES && error.is_none() {
            if let Some(rewind) = &self.rewind {
                self.edits.mark_undone(&self.prompts, &rewind.uuid);
            }
        }
        if id == REWIND_CONVERSATION && error.is_none() {
            // Claude Code cut its conversation before the message, and gives its text back to send again.
            let target = answer["targetMessageUuid"].as_str().or(self.rewind.as_ref().map(|rewind| rewind.uuid.as_str()));
            let prompt = target.and_then(|target| self.prompts.iter().find(|prompt| prompt.uuid == target)).cloned();
            if let Some(prompt) = prompt {
                let said = match self.items.get(prompt.at) {
                    Some(Item::User(said) | Item::UserImages(said, _)) => said.clone(),
                    _ => String::new(),
                };
                self.restored = Some(answer["prefillText"].as_str().map_or(said, str::to_owned));
                self.cut(prompt.at);
            }
        }
        let done = match self.rewind.as_mut() {
            Some(rewind) => {
                rewind.waiting.retain(|waiting| *waiting != id);
                rewind.error = error.or(rewind.error.take());
                rewind.waiting.is_empty() && rewind.error.is_none()
            }
            // The dialog was closed before the answer came, so a failure shows in the conversation.
            None => {
                self.items.extend(error.map(Item::Error));
                false
            }
        };
        if done {
            self.rewind = None;
        }
        true
    }

    /// The request that reads the end of what the background task `id` printed, from your click on it. None for a task
    /// whose output Claude Code doesn't keep.
    pub fn read_task(&mut self, id: &str) -> Option<String> {
        let task = self.tasks.iter_mut().find(|task| task.id == id && task.has_output())?;
        task.error = None;
        Some(request(&format!("{TASK_OUTPUT}{id}"), json!({ "subtype": "get_task_output", "task_id": id })))
    }

    /// The request that stops the background task `id`, from your click on its Stop. None once it has ended, and while
    /// it is being stopped.
    pub fn stop_task(&mut self, id: &str) -> Option<String> {
        let task = self.tasks.iter_mut().find(|task| task.id == id && task.ended.is_none() && !task.stopping)?;
        task.stopping = true;
        task.error = None;
        Some(request(&format!("{TASK_STOP}{id}"), json!({ "subtype": "stop_task", "task_id": id })))
    }

    /// Claude Code's answer to a read or a stop of a background task. False for any other request.
    fn answered_task(&mut self, id: &str, response: &Value) -> bool {
        let (task, read) = match (id.strip_prefix(TASK_OUTPUT), id.strip_prefix(TASK_STOP)) {
            (Some(task), _) => (task, true),
            (_, Some(task)) => (task, false),
            _ => return false,
        };
        let Some(task) = self.tasks.iter_mut().find(|known| known.id == task) else {
            return true;
        };
        let answer = &response["response"];
        if response["subtype"] != "success" {
            task.error = Some(response["error"].as_str().unwrap_or("Claude Code didn't answer.").to_owned());
            if !read {
                task.stopping = false;
            }
        } else if read {
            task.output = Some(Output { text: answer["output"].as_str().unwrap_or_default().to_owned(), bytes: answer["total_bytes"].as_u64().unwrap_or(0), truncated: answer["truncated"] == true });
        }
        true
    }
}


/// Claude Code's answer to the question `request`.
fn answer_line(request: &str, response: Value) -> String {
    line(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": request, "response": response } }))
}

fn line(message: &Value) -> String {
    format!("{message}\n")
}

/// Your message `text` for Claude Code to read as `priority` says (`next`: at its next step, `now`: at once), stamped
/// `uuid`, which it echoes when it reads it.
fn user_line(text: &str, images: &[(String, String)], priority: &str, uuid: &str, shell: &str, relayed: bool) -> String {
    let mut message = json!({ "type": "user", "message": { "role": "user", "content": crate::shell::with_context(content(text, images), shell) }, "parent_tool_use_id": null, "priority": priority, "uuid": uuid });
    relay(&mut message, relayed);
    line(&message)
}

/// Marks the user message `message` as text you didn't write, when it is: `isSynthetic` has Claude Code count it as not
/// yours, and `client_composed` has it deliver text that starts with `/` as written instead of running it as a command
/// (Agent SDK docs, `SDKUserMessage`).
fn relay(message: &mut Value, relayed: bool) {
    if relayed {
        message["isSynthetic"] = json!(true);
        message["client_composed"] = json!(true);
    }
}

/// What Claude Code puts before text it was sent as `isSynthetic`, in the transcript, where it files the message as
/// `isMeta`: a resumed chat shows the message from what follows it.
const NOT_USER: &str = "[MESSAGE FROM NON-USER SOURCE - NOT USER INPUT]\n";

/// The `content` of a transcript entry that holds text another agent relayed, without Claude Code's marker. None for any
/// other entry.
fn unmarked(content: &Value) -> Option<Value> {
    let text = content.as_str()?.strip_prefix(NOT_USER)?;
    Some(json!(text))
}

/// A message's `content`: the text alone when there are no pictures, else a block for each picture and then the text,
/// which may be left out when there is none.
fn content(text: &str, images: &[(String, String)]) -> Value {
    if images.is_empty() {
        return json!(text);
    }
    let mut blocks: Vec<Value> = images.iter().map(|(media_type, data)| json!({ "type": "image", "source": { "type": "base64", "media_type": media_type, "data": data } })).collect();
    if !text.is_empty() {
        blocks.push(json!({ "type": "text", "text": text }));
    }
    Value::Array(blocks)
}

/// A message of yours as the conversation shows it.
fn sent_item(text: String, pictures: Vec<String>) -> Item {
    if let Some(note) = crate::actions::relayed_note(&text) {
        return Item::Notice(note);
    }
    if let Some((what, said)) = crate::blocked::allowed(&text) {
        return Item::Allowed(what, said);
    }
    if pictures.is_empty() { Item::User(text) } else { Item::UserImages(text, pictures) }
}

pub(crate) fn cancel(uuid: &str, n: u32) -> String {
    request(&format!("cancel-{n}"), json!({ "subtype": "cancel_async_message", "message_uuid": uuid }))
}

/// Whether a draft is `/exit`, which closes the chat.
pub fn exits(text: &str) -> bool {
    text.trim() == "/exit"
}

/// A uuid (version 4 in shape, which the protocol wants) made from the time in milliseconds and a number.
fn uuid(stamp: u64, n: u32) -> String {
    format!("{:08x}-{:04x}-4000-8000-{:012x}", (stamp >> 16) as u32, stamp & 0xffff, n)
}

/// A request of the chat's to Claude Code, answered under `id`.
pub(crate) fn request(id: &str, request: Value) -> String {
    line(&json!({ "type": "control_request", "request_id": id, "request": request }))
}

/// Whether the command in Claude Code's list has a `plugin` key that says which plugin it comes from.
fn is_plugin(command: &Value) -> bool {
    command["plugin"].as_str().map_or(!command["plugin"].is_null() && command["plugin"] != false, |name| !name.is_empty())
}

/// The slash commands `initialize` or a reload of the plugins lists.
pub(crate) fn commands(list: &Value) -> Vec<Command> {
    let text = |value: &Value, key: &str| value[key].as_str().unwrap_or_default().to_owned();
    list.as_array()
        .into_iter()
        .flatten()
        .map(|command| Command { name: text(command, "name"), description: text(command, "description"), hint: text(command, "argumentHint"), plugin: is_plugin(command), builtin: command["builtin"] == true })
        .collect()
}

/// The line that asks how much of your plan's limits is used.
fn usage() -> String {
    request(USAGE, json!({ "subtype": "get_usage" }))
}

/// A text in your name as the chat shows it: what you wrote, or a note for what Claude Code wrote in your name, which
/// starts with a tag of its own: a background task's notice, a reminder, a slash command or what it printed. The
/// caveat Claude Code puts before a command's output says nothing to you, and shows nothing.
fn said(text: &str) -> Option<Item> {
    let text = text.trim();
    let tag = text.strip_prefix('<').and_then(|rest| rest.split_once('>')).map(|(tag, _)| tag).filter(|tag| {
        tag.starts_with(|c: char| c.is_ascii_lowercase()) && tag.chars().all(|c| c.is_ascii_lowercase() || c == '-')
    });
    let Some(tag) = tag else {
        if let Some(note) = crate::actions::relayed_note(text) {
            return Some(Item::Notice(note));
        }
        if let Some((what, said)) = crate::blocked::allowed(text) {
            return Some(Item::Allowed(what, said));
        }
        return (!text.is_empty()).then(|| Item::User(text.to_owned()));
    };
    let inner = |name: &str| text.split_once(&format!("<{name}>"))?.1.split_once(&format!("</{name}>")).map(|(inside, _)| inside.trim());
    let title = match tag {
        "local-command-caveat" => return None,
        "task-notification" => match inner("summary") {
            Some(summary) => summary.to_owned(),
            None => "A background task finished".to_owned(),
        },
        "system-reminder" => "Reminder".to_owned(),
        "command-name" | "command-message" => format!("Ran {}", inner("command-name").unwrap_or("a command")),
        "local-command-stdout" => "Command output".to_owned(),
        "bash-input" => format!("Ran {}", inner("bash-input").unwrap_or("a shell command")),
        "bash-stdout" | "bash-stderr" => "Shell output".to_owned(),
        // A tag of your own, like a bit of HTML you pasted.
        _ => return Some(Item::User(text.to_owned())),
    };
    Some(Item::Note(cut(&title, SUMMARY_KEPT), cut(&untagged(text), OUTPUT_KEPT)))
}

/// What a background task is, as the line where it ended names it.
fn task_kind(kind: &str) -> &'static str {
    match kind {
        "local_agent" => "Agent",
        "local_bash" => "Command",
        _ if kind.starts_with("monitor") => "Monitor",
        _ => "Task",
    }
}

/// `said`, but a background task's notices, which a transcript has as one of your messages, each become the line where the
/// task ended. A notice with no status, like a monitor's event, stays a note.
fn said_or_finished(text: &str) -> Vec<Item> {
    if !text.trim_start().starts_with("<task-notification>") {
        return said(text).into_iter().collect();
    }
    text.split("<task-notification>")
        .skip(1)
        .filter_map(|block| {
            let inner = |name: &str| block.split_once(&format!("<{name}>"))?.1.split_once(&format!("</{name}>")).map(|(inside, _)| inside.trim());
            let (status, summary) = (inner("status"), inner("summary"));
            // `Agent "Survey" finished`, `Background command "Build" completed (exit code 0)`: the name is the quoted part.
            let described = summary.and_then(|summary| {
                let open = summary.find(['"', '“'])?;
                let rest = &summary[open..];
                let (quote, rest) = rest.split_at(rest.chars().next()?.len_utf8());
                let close = rest.find(if quote == "“" { '”' } else { '"' })?;
                Some((summary.starts_with("Agent "), summary.starts_with("Monitor"), rest[..close].to_owned()))
            });
            let (Some(status), Some((agent, monitor, description))) = (status, described) else {
                return said(&format!("<task-notification>{block}"));
            };
            let kind = if agent { "Agent" } else if monitor { "Monitor" } else { "Command" };
            // An agent's report reaches Claude as a message of its own, and the notice says it isn't repeated.
            let report = inner("result").filter(|report| !report.contains("not repeated here")).or(summary.filter(|_| status != "completed")).unwrap_or_default();
            Some(Item::Finished(Finished {
                task: inner("task-id").unwrap_or_default().to_owned(),
                call: inner("tool-use-id").unwrap_or_default().to_owned(),
                kind,
                description,
                status: status.to_owned(),
                took: inner("duration_ms").and_then(|ms| ms.parse().ok()),
                report: cut(report, OUTPUT_KEPT),
            }))
        })
        .collect()
}

/// `text` without its tags, and without the lines they leave empty.
fn untagged(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        match rest[open..].find('>') {
            Some(close) => rest = &rest[open + close + 1..],
            None => {
                rest = &rest[open..];
                break;
            }
        }
    }
    out.push_str(rest);
    out.lines().map(str::trim_end).filter(|line| !line.trim().is_empty()).collect::<Vec<_>>().join("\n")
}

/// A call's result without the `<tool_use_error>` tags Claude Code wraps a refused call's error in.
fn unwrapped(output: &str) -> &str {
    let trimmed = output.trim();
    trimmed.strip_prefix("<tool_use_error>").and_then(|rest| rest.strip_suffix("</tool_use_error>")).map_or(output, str::trim)
}

/// The field a tool's summary is made from, as (name, text): the command it runs, the file or the pattern, or else its
/// first text input.
fn summarized<'a>(tool: &str, input: &'a Value) -> Option<(&'a str, &'a str)> {
    let key = match tool {
        "Bash" => "command",
        "Read" | "Edit" | "Write" | "NotebookEdit" => "file_path",
        "Grep" | "Glob" => "pattern",
        "WebFetch" => "url",
        "WebSearch" => "query",
        "Task" | "Agent" => "description",
        _ => "",
    };
    let fields = input.as_object()?;
    let text = |(name, value): (&'a String, &'a Value)| Some((name.as_str(), value.as_str()?));
    fields.get_key_value(key).and_then(text).or_else(|| fields.iter().find_map(text))
}

/// A tool call's input whole, as `Input` keeps it, each part cut at the most a tool's output keeps.
fn input_of(tool: &str, input: &Value) -> Input {
    let summarized = summarized(tool, input);
    let whole = summarized.map(|(_, text)| text.trim()).filter(|text| *text != summary(tool, input)).map(|text| cut(text, OUTPUT_KEPT)).unwrap_or_default();
    let shown = matches!(tool, "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "Read" | "TodoWrite" | "TaskCreate" | "TaskUpdate");
    let fields = input.as_object().filter(|_| !shown).into_iter().flatten().filter(|(name, _)| Some(name.as_str()) != summarized.map(|(name, _)| name));
    let fields = fields.map(|(name, value)| (name.clone(), cut(&value.as_str().map_or_else(|| value.to_string(), str::to_owned), OUTPUT_KEPT))).collect();
    Input { whole, fields }
}

/// What a tool is doing, in one line: the command it runs, the file or the pattern, or else its first text input.
pub fn summary(tool: &str, input: &Value) -> String {
    if let Some(said) = crate::scheduled::summary(tool, input) {
        return said;
    }
    let text = summarized(tool, input).map(|(_, text)| text).unwrap_or_default();
    // A command's indented script or aligned text, said on one line, would show its runs of spaces as gaps.
    let line: Vec<&str> = text.lines().next().unwrap_or_default().split_whitespace().collect();
    cut(&line.join(" "), SUMMARY_KEPT)
}

impl Session {
    /// Shows `used` percent of the limit `label`, the 5-hour one first.
    /// The windows of a `get_usage` answer's `rate_limits` (percent used, ISO reset time). An answer can come without them:
    /// the limits shown stay.
    fn take_limits(&mut self, limits: &Value) {
        for (label, key) in LIMITS {
            if let Some(used) = limits[key]["utilization"].as_f64() {
                self.set_limit(label, used, limits[key]["resets_at"].as_str().and_then(parse_time));
            }
        }
    }

    fn set_limit(&mut self, label: &'static str, used: f64, resets: Option<f64>) {
        self.limits.retain(|(shown, ..)| *shown != label);
        self.limits.push((label, used.round() as u64, resets));
        self.limits.sort_by_key(|(shown, ..)| *shown != "5h");
    }

    /// Notes a Skill call whose result says an MCP server served the skill: `tool_use_result` as Claude Code prints it,
    /// `toolUseResult` in a transcript.
    fn served_skill(&mut self, line: &Value) {
        let result = if line["tool_use_result"].is_object() { &line["tool_use_result"] } else { &line["toolUseResult"] };
        if result["fromMcpServer"] != true {
            return;
        }
        let ids = line["message"]["content"].as_array().into_iter().flatten().filter_map(|block| block["tool_use_id"].as_str());
        let skills: Vec<String> = ids.filter(|id| self.items.iter().any(|item| matches!(item, Item::Tool(tool) if tool.id == *id && tool.name == "Skill"))).map(str::to_owned).collect();
        self.mcp_skills.extend(skills);
    }

    /// Whether `tool` still runs: it has no result and was not cut off by the death of a Claude Code process.
    pub fn runs(&self, tool: &Tool) -> bool {
        tool.output.is_none() && !self.interrupted.contains(&tool.id)
    }

    /// Whether `tool` was cut off: it has no result, and the Claude Code process that ran it ended before it answered.
    pub fn was_interrupted(&self, tool: &Tool) -> bool {
        tool.output.is_none() && self.interrupted.contains(&tool.id)
    }

    /// Called once the transcript of a resumed session is read, before the new Claude Code process says anything: a call
    /// the transcript holds without a result belonged to a process that is gone, since a process that ends writes none
    /// and Claude Code does not rerun it on resume (measured in real transcripts). Nothing here depends on timing: only on
    /// which process the call was heard from. A call this process makes is never in the set.
    pub fn stop_replayed(&mut self) {
        let cut: Vec<String> = self.items.iter().filter_map(|item| match item {
            Item::Tool(tool) if tool.output.is_none() => Some(tool.id.clone()),
            _ => None,
        }).collect();
        self.interrupted.extend(cut);
    }

    /// What Claude is doing now, as the line under the conversation says it: the tool it runs, else thinking or writing.
    pub fn activity(&self) -> String {
        if let Some(status) = &self.status {
            return status.clone();
        }
        let running = self.items.iter().rev().find_map(|item| match item {
            Item::Tool(tool) if self.runs(tool) => Some(tool),
            _ => None,
        });
        match running {
            _ if self.thinking => "Thinking".into(),
            Some(tool) => doing(&tool.name, &tool.summary),
            None if !self.live.is_empty() => "Writing".into(),
            None => "Working".into(),
        }
    }

    /// The items of the turn under way: those after your last message.
    fn turn(&self) -> &[Item] {
        let start = self.items.iter().rposition(|item| matches!(item, Item::User(_) | Item::UserImages(..))).map_or(0, |at| at + 1);
        &self.items[start..]
    }

    /// The subagents the turn under way started: how many, and how many of them still run.
    pub fn turn_subagents(&self) -> (usize, usize) {
        self.turn().iter().fold((0, 0), |(started, running), item| match item {
            Item::Tool(tool) if matches!(tool.name.as_str(), "Task" | "Agent") => (started + 1, running + usize::from(self.runs(tool))),
            _ => (started, running),
        })
    }

    /// How many subagents the turn waits on, when it waits on nothing else: every call still running is a subagent's, and
    /// Claude neither thinks nor writes. None otherwise.
    pub fn waiting_on_subagents(&self) -> Option<usize> {
        if self.thinking || !self.live.is_empty() {
            return None;
        }
        let running: Vec<&Tool> = self.turn().iter().filter_map(|item| match item {
            Item::Tool(tool) if self.runs(tool) => Some(tool),
            _ => None,
        }).collect();
        (!running.is_empty() && running.iter().all(|tool| matches!(tool.name.as_str(), "Task" | "Agent"))).then_some(running.len())
    }
}

/// A model as a person says it: `claude-haiku-5-5` is "Haiku 5.5", `claude-opus-4-1-20250805` is "Opus 4.1". A name that
/// isn't of that shape is shown as it is.
pub fn model_name(model: &str) -> String {
    let mut parts = model.strip_prefix("claude-").unwrap_or(model).split('-').filter(|part| !(part.len() == 8 && part.bytes().all(|b| b.is_ascii_digit())));
    let family = parts.next().unwrap_or_default();
    let version: Vec<&str> = parts.collect();
    if family.is_empty() || version.iter().any(|part| !part.bytes().all(|b| b.is_ascii_digit())) {
        return model.to_owned();
    }
    let mut name: String = family.chars().take(1).flat_map(char::to_uppercase).chain(family.chars().skip(1)).collect();
    if !version.is_empty() {
        name.push(' ');
        name.push_str(&version.join("."));
    }
    name
}

/// A running tool call in a few words, as Claude Code's own status line puts it: "Running pnpm test", "Calling wings".
fn doing(tool: &str, summary: &str) -> String {
    let file = summary.rsplit('/').next().unwrap_or(summary);
    match tool {
        "Bash" => format!("Running {}", cut(summary, 60)),
        "Read" => format!("Reading {file}"),
        "Edit" | "MultiEdit" | "NotebookEdit" => format!("Editing {file}"),
        "Write" => format!("Writing {file}"),
        "Grep" | "Glob" => format!("Searching for {}", cut(summary, 60)),
        "WebSearch" => "Searching the web".into(),
        "WebFetch" => format!("Fetching {}", cut(summary, 60)),
        "Task" | "Agent" => "Running a subagent".into(),
        UNNAMED => "Using a tool with no name".into(),
        // MCP tools are mcp__<server>__<tool>, and a plugin's server is plugin_<plugin>_<server>.
        _ => match tool.strip_prefix("mcp__").and_then(|rest| rest.split("__").next()) {
            Some(server) => format!("Calling {}", if server.starts_with("plugin_") { server.rsplit('_').next().unwrap_or(server) } else { server }),
            None => format!("Using {tool}"),
        },
    }
}

/// What a tool is asked to do, as a permission card shows it: the whole command, the change an edit makes, the file a
/// write leaves, else the input as Claude sent it. Whatever else the input holds follows as JSON, so nothing that runs is
/// left off the card.
fn ask_detail(tool: &str, input: &Value) -> String {
    ask_text(tool, input, true)
}

/// `ask_detail` without the lines of an edit or a write: its path, and what else the input holds. A card that draws the change
/// as a diff shows this above it.
pub(crate) fn ask_head(tool: &str, input: &Value) -> String {
    ask_text(tool, input, false)
}

fn ask_text(tool: &str, input: &Value, whole: bool) -> String {
    let field = |key: &str| shown(input[key].as_str().unwrap_or_default(), true);
    let change = |edit: &Value| {
        let lines = |key: &str, mark: &str| edit[key].as_str().unwrap_or_default().lines().map(|line| format!("{mark} {}", shown(line, true))).collect::<Vec<_>>();
        let every = if edit["replace_all"] == true { "Every match of:\n" } else { "" };
        format!("{every}{}\n{}", lines("old_string", "−").join("\n"), lines("new_string", "+").join("\n"))
    };
    let (said, keys): (String, &[&str]) = match tool {
        // A command's description is Claude's words about it, which don't run.
        "Bash" => (shown(input["command"].as_str().unwrap_or_default().trim(), false), &["command", "description"]),
        "Edit" if !whole => (field("file_path"), &["file_path", "old_string", "new_string", "replace_all"]),
        "MultiEdit" if !whole => (field("file_path"), &["file_path", "edits"]),
        "Write" if !whole => (field("file_path"), &["file_path", "content"]),
        "Edit" => (format!("{}\n\n{}", field("file_path"), change(input)), &["file_path", "old_string", "new_string", "replace_all"]),
        "MultiEdit" => {
            let edits = input["edits"].as_array().map(Vec::as_slice).unwrap_or_default();
            (format!("{}\n\n{}", field("file_path"), edits.iter().map(change).collect::<Vec<_>>().join("\n\n")), &["file_path", "edits"])
        }
        "Write" => (format!("{}\n\n{}", field("file_path"), shown(input["content"].as_str().unwrap_or_default(), false)), &["file_path", "content"]),
        _ => (String::new(), &[]),
    };
    let rest: serde_json::Map<String, Value> = input.as_object().into_iter().flatten().filter(|(key, _)| !keys.contains(&key.as_str())).map(|(key, value)| (key.clone(), value.clone())).collect();
    let rest = (!rest.is_empty()).then(|| shown(&serde_json::to_string_pretty(&rest).unwrap_or_default(), false));
    let outside = (input["dangerouslyDisableSandbox"] == true).then(|| "Runs outside the sandbox.".to_owned());
    [outside, Some(said).filter(|said| !said.is_empty()), rest].into_iter().flatten().collect::<Vec<_>>().join("\n\n")
}

/// `text` as a card can show it truthfully: printable ASCII, letters and digits stay, and every other character is
/// written out as `\u{…}`, so nothing invisible, nothing that turns text around and nothing that looks like something
/// else hides in what you allow. A field that is one line, like a path, writes out its line breaks and tabs too.
pub(crate) fn shown(text: &str, one_line: bool) -> String {
    let plain = |c: char| {
        matches!(c, ' '..='~')
            || (!one_line && matches!(c, '\n' | '\t'))
            // Hangul fillers are letters that draw nothing.
            || (!c.is_ascii() && c.is_alphanumeric() && !matches!(c, '\u{115f}' | '\u{1160}' | '\u{3164}' | '\u{ffa0}'))
    };
    text.chars().map(|c| if plain(c) { c.to_string() } else { format!("\\u{{{:x}}}", c as u32) }).collect()
}

/// Why Claude Code asks, in one line: its title, description and reason, without what two of them say alike.
fn reason(request: &Value) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for key in ["title", "description", "decision_reason"] {
        if let Some(said) = request[key].as_str().map(unescaped).filter(|said| !said.is_empty() && !parts.contains(said)) {
            parts.push(said);
        }
    }
    (!parts.is_empty()).then(|| cut(&parts.join(" · "), REASON_KEPT))
}

/// `text` as one line of plain words: Claude Code's reasons can carry terminal colour codes.
fn unescaped(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' if chars.peek() == Some(&'[') => {
                chars.next();
                for end in chars.by_ref() {
                    if end.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn progress(status: &str) -> Progress {
    match status {
        "in_progress" => Progress::Active,
        "completed" => Progress::Done,
        _ => Progress::Pending,
    }
}

/// What a tool call's input holds beyond its summary: the diff of an edit, the start of a file written, a whole
/// command.
fn detail(tool: &str, input: &Value) -> Detail {
    match tool {
        "Edit" | "MultiEdit" => {
            let one = [input.clone()];
            let edits = if tool == "MultiEdit" { input["edits"].as_array().map_or(&[][..], Vec::as_slice) } else { &one[..] };
            let mut diff = Diff::default();
            let mut total = 0;
            for edit in edits {
                let (old, new) = (edit["old_string"].as_str().unwrap_or_default(), edit["new_string"].as_str().unwrap_or_default());
                for (added, lines) in [(false, old), (true, new)] {
                    for line in lines.lines() {
                        total += 1;
                        match added {
                            true => diff.added += 1,
                            false => diff.removed += 1,
                        }
                        if diff.lines.len() < DIFF_KEPT {
                            diff.lines.push((added, cut(line, SUMMARY_KEPT)));
                        }
                    }
                }
            }
            diff.more = total - diff.lines.len();
            Detail::Diff(diff)
        }
        "Write" => {
            let content = input["content"].as_str().unwrap_or_default();
            Detail::Write { lines: content.lines().count(), head: content.lines().take(WRITE_KEPT).map(|line| cut(line, SUMMARY_KEPT)).collect() }
        }
        "Bash" => Detail::Command(cut(input["command"].as_str().unwrap_or_default(), OUTPUT_KEPT)),
        "Read" => {
            let (offset, limit) = (input["offset"].as_u64(), input["limit"].as_u64());
            Detail::Read(match (offset, limit) {
                (start, Some(limit)) if limit > 0 => Some(format!("lines {}–{}", start.unwrap_or(1), start.unwrap_or(1) + limit - 1)),
                (Some(start), None) => Some(format!("from line {start}")),
                _ => None,
            })
        }
        "Grep" | "Glob" => Detail::Search,
        _ => Detail::Plain,
    }
}

/// The first `cap` lines of an output, and how many lines are left out after them.
pub fn first_lines(output: &str, cap: usize) -> (&str, usize) {
    match output.match_indices('\n').nth(cap.saturating_sub(1)) {
        Some((end, _)) => (&output[..end], output[end + 1..].lines().count()),
        None => (output, 0),
    }
}

/// The exit code Claude Code puts first in a failed command's output: "Exit code 1".
pub fn exit_code(output: &str) -> Option<i32> {
    output.strip_prefix("Exit code ")?.split_whitespace().next()?.parse().ok()
}

/// How many files a Grep or Glob found, as what it printed says: "Found 12 files" and one path a line. A Grep that
/// printed matching lines has no count.
pub fn files_found(tool: &str, output: &str) -> Option<usize> {
    let first = output.lines().next().unwrap_or_default();
    if first.starts_with("No ") && first.ends_with(" found") {
        return Some(0);
    }
    match first.strip_prefix("Found ") {
        Some(rest) => rest.split_whitespace().next()?.parse().ok(),
        None if tool == "Glob" => Some(output.lines().filter(|line| !line.trim().is_empty()).count()),
        None => None,
    }
}

/// The `text` of each `{ type: "text", text }` part.
fn texts(parts: &[Value]) -> Vec<String> {
    parts.iter().filter_map(|part| part["text"].as_str().or(part.as_str()).map(str::to_owned)).collect()
}

/// At most `max` bytes of `text`, cut at a character, with `…` where it was cut.
fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let end = (0..=max).rev().find(|at| text.is_char_boundary(*at)).unwrap_or(0);
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests;
