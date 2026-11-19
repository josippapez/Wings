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
    plugins::{self, Plugins, Show},
    setup::Setup,
    usage::{ContextDetail, Panel, Usage},
};

/// The most of a tool's output the chat keeps. A whole file read or a long build log is more than anyone reads in a card.
const OUTPUT_KEPT: usize = 4000;
/// The most of a tool's input shown as its one-line summary.
const SUMMARY_KEPT: usize = 200;
/// The most of what a tool would do that a permission card shows, and of the reason. Past the first, the card can't be
/// allowed: you would approve what you can't read.
const ASK_KEPT: usize = 16_000;
const REASON_KEPT: usize = 300;
/// The most lines of an edit's diff the chat keeps, and the most of a written file's start.
const DIFF_KEPT: usize = 40;
const WRITE_KEPT: usize = 20;
/// The most items kept of each subagent's own transcript: a subagent that searches a repository makes hundreds of tool
/// calls, and its own tab reads them from its transcript. Older ones go.
pub const SUBAGENT_KEPT: usize = 200;

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
    /// You stopped the turn.
    Stopped,
    /// What Claude Code added to the conversation in your name, like a background task's notice: what it is, and
    /// what it said.
    Note(String, String),
    /// A background agent, command or monitor ended.
    Finished(Finished),
    /// A turn ended as it should, after this many milliseconds of work.
    Worked(u64),
    /// A line across the conversation, like the one where it was compacted.
    Divider(String),
    /// A short note from Claude Code, like a retry that failed or a model it switched to.
    Notice(String),
    /// What Claude thought before it answered, as its summary, and how long it took (ms, 0 when unknown).
    Thought(String, u64),
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
    /// The pictures it gave back (a Read of an image, a screenshot, an MCP tool), by id.
    pub images: Vec<String>,
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
}

impl Draft {
    pub fn is_picked(&self, question: usize, option: usize) -> bool {
        self.picked.contains(&(question, option))
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
        }
        self.picked.insert((question, option));
    }

    pub fn set_other(&mut self, question: usize, text: String, multi: bool) {
        if !multi && !text.trim().is_empty() {
            self.picked.retain(|(q, _)| *q != question);
        }
        self.other.insert(question, text);
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
}

/// A slash command, as `initialize` lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub hint: String,
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
    pub items: Vec<Item>,
    /// Claude's text of the message it is writing now, before the whole message comes.
    pub live: String,
    /// Claude is thinking before it writes.
    pub thinking: bool,
    pub id: Option<String>,
    /// The model the session runs on, as Claude Code names it (`claude-opus-5-5`) or as it was picked (`opus`).
    pub model: Option<String>,
    /// The effort picked in the chat.
    pub effort: Option<String>,
    /// The effort your settings give each model by its full name (`modelSettings`), and the one for the rest.
    model_efforts: Vec<(String, String)>,
    default_effort: Option<String>,
    /// How much of your plan's limits is used: ("5h", percent, when it resets in milliseconds) and the week's.
    pub limits: Vec<(&'static str, u64, Option<f64>)>,
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
    told_you: bool,
    /// What the session has cost so far, in US dollars.
    pub cost: f64,
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
    /// Fast mode and the output style.
    pub options: crate::composer::Options,
    /// What Claude Code says it is doing beyond a tool call, like compacting or retrying a request, for the status line.
    pub status: Option<String>,
    /// The session's name, as Claude Code last said it (`/rename`, or the one it gave).
    pub title: Option<String>,
    /// The conversation was cleared (`/clear`, `/new`, `/reset`), which the page reads once to drop what it kept of it.
    pub cleared: bool,
    /// When the thinking block being streamed began (ms), for how long the thought took.
    pub thought_since: u64,
    /// The dialogs and answers of `/status`, `/export` and `/btw`.
    pub cmds: Commands,
    /// Commands you moved to the background, and what MCP servers ask you (`actions.rs`, `elicitation.rs`).
    pub ops: Ops,
    /// The files each turn changed, and Undo (`edits.rs`).
    pub edits: Edits,
}

impl Session {
    /// Takes in one line Claude Code printed, or one transcript entry.
    pub fn hear(&mut self, line: &Value) {
        self.agents.learn(line);
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
            return;
        }
        self.hear_own(line);
    }

    fn hear_own(&mut self, line: &Value) {
        // Every turn starts with this status, also one a finished background task starts by itself, and Claude Code prints
        // none outside a turn. Its `init` line comes with it, but the chat's own scenes print that one to start a session.
        if line["type"] == "system" && line["subtype"] == "status" && line["status"] == "requesting" {
            self.turn_open = true;
            self.settle();
        }
        if notices::hear(self, line) {
            return;
        }
        match line["type"].as_str().unwrap_or_default() {
            "system" if line["subtype"] == "init" => {
                self.id = line["session_id"].as_str().map(str::to_owned);
                // It names the model in full (`claude-opus-5-5`), which more than one choice stands for: the one picked, or
                // your settings', stays shown while it is that model.
                let reported = line["model"].as_str();
                if self.current_model().map(|model| model.resolved.as_str()) != reported {
                    self.model = reported.map(str::to_owned);
                }
                self.mode = line["permissionMode"].as_str().map(str::to_owned).or(self.mode.take());
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
            "system" if line["subtype"] == "session_state_changed" => self.changed(line["state"].as_str().unwrap_or_default()),
            "system" if line["subtype"] == "background_tasks_changed" => self.tasks_now(&line["tasks"]),
            // A task's end, from its update and its notice, whichever comes first.
            "system" if line["subtype"] == "task_started" => {
                if let Some(task) = self.tasks.iter_mut().find(|task| line["task_id"] == task.id.as_str()) {
                    task.call = line["tool_use_id"].as_str().unwrap_or_default().to_owned();
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
                self.told_you = false;
                self.assistant(&line["message"]);
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
            "user" if line["isReplay"] == true => self.echoed(line["uuid"].as_str().unwrap_or_default()),
            // What Claude Code adds in a turn on its own, like a skill's text once the Skill tool loads it (`isSynthetic` as it
            // prints it, `isMeta` in the transcript), isn't yours to see as a message.
            "user" if line["isMeta"] != true && line["isSynthetic"] != true => {
                let parent = line["parentUuid"].as_str();
                // In a transcript, an entry that follows what one of your messages followed took its place: the
                // conversation was rewound to before that message, which Claude Code leaves in the file.
                let replaced = parent.and_then(|parent| self.prompts.iter().find(|prompt| prompt.parent.as_deref() == Some(parent))).map(|prompt| prompt.at);
                if let Some(at) = replaced {
                    self.cut(at);
                }
                let first = self.items.len();
                self.user(&line["message"]["content"]);
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
                self.items.push(Item::Ask(Ask {
                    request: line["request_id"].as_str().unwrap_or_default().to_owned(),
                    summary: summary(&tool, &request["input"]),
                    detail: cut(&detail, ASK_KEPT),
                    cut_short: detail.len() > ASK_KEPT,
                    reason: reason(request),
                    suggestions,
                    tool,
                    input: request["input"].clone(),
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
                    if !std::mem::replace(&mut self.told_you, true) {
                        let last = self.items.iter().rev().find_map(|item| if let Item::Text(text) = item { Some(text) } else { None });
                        let first = last.and_then(|text| text.lines().find(|line| !line.trim().is_empty())).unwrap_or_default();
                        self.alerts.push(("Claude finished".into(), cut(first.trim(), SUMMARY_KEPT)));
                    }
                    // As Claude Code timed the turn. A result of no turns, like one for a background task answered along
                    // with another, did no work of its own.
                    if let Some(ms) = line["duration_ms"].as_f64().filter(|_| line["num_turns"] != 0) {
                        self.items.push(Item::Worked(ms as u64));
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
        self.edits.clear();
        self.rewind = None;
        self.restored = None;
        self.todos.clear();
        self.creating.clear();
        self.subagents.clear();
        self.agents.clear();
        self.tasks.clear();
        self.suggestion = None;
        self.status = None;
        self.title = None;
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
    fn cut(&mut self, at: usize) {
        self.items.truncate(at);
        self.prompts.retain(|prompt| prompt.at < at);
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
        self.alerts.push((finished.title(), cut(report.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim(), SUMMARY_KEPT)));
        self.items.push(Item::Finished(finished));
    }

    /// Claude Code's answer to one of the chat's requests.
    fn answered(&mut self, response: &Value) {
        let answer = &response["response"];
        let id = response["request_id"].as_str().unwrap_or_default();
        // A reload brings the commands as they are now: a plugin's skills come and go with it.
        if id == plugins::RELOAD && response["subtype"] == "success" {
            self.commands = commands(&answer["commands"]);
        }
        if self.plugins.answered(id, response)
            || self.ops.answered(id, response, &mut self.items)
            || self.answered_rewind(id, response)
            || self.edits.answered(id, response, &self.prompts, &mut self.items)
            || self.answered_task(id, response)
            || self.setup.answered(id, response)
            || self.cmds.answered(id, response)
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
                self.model_efforts = settings["modelSettings"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(model, set)| Some((model.clone(), set["effortLevel"].as_str()?.to_owned())))
                    .collect();
            }
            // An answer can come without `rate_limits`: the limits shown stay.
            USAGE => {
                let limits = &answer["rate_limits"];
                for (label, key) in LIMITS {
                    if let Some(used) = limits[key]["utilization"].as_f64() {
                        self.set_limit(label, used, limits[key]["resets_at"].as_str().and_then(parse_time));
                    }
                }
            }
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
            "content_block_delta" if event["delta"]["type"] == "text_delta" => {
                self.live.push_str(event["delta"]["text"].as_str().unwrap_or_default());
            }
            _ => {}
        }
    }

    fn assistant(&mut self, message: &Value) {
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
                    let name = block["name"].as_str().unwrap_or("Tool").to_owned();
                    let id = block["id"].as_str().unwrap_or_default().to_owned();
                    self.track(&name, &id, &block["input"]);
                    self.items.push(Item::Tool(Tool { id, summary: summary(&name, &block["input"]), detail: detail(&name, &block["input"]), name, output: None, images: Vec::new() }));
                }
                "image" => {
                    self.items.push(Item::Images(images::of_block(block).into_iter().collect()));
                }
                _ => {}
            }
        }
    }

    fn user(&mut self, content: &Value) {
        if let Some(text) = content.as_str() {
            return self.items.extend(said_or_finished(text));
        }
        let first = self.items.len();
        let mut pictures: Vec<String> = Vec::new();
        for block in content.as_array().into_iter().flatten() {
            match block["type"].as_str().unwrap_or_default() {
                "image" => pictures.extend(images::of_block(block)),
                // What Claude Code writes in your name when a turn is stopped, which `Stopped` shows.
                "text" if block["text"].as_str().is_some_and(|text| text.starts_with("[Request interrupted by user")) => {}
                "text" => self.items.extend(said_or_finished(block["text"].as_str().unwrap_or_default())),
                "tool_result" => {
                    let id = block["tool_use_id"].as_str().unwrap_or_default();
                    let output = match &block["content"] {
                        Value::String(text) => text.clone(),
                        Value::Array(parts) => texts(parts).join("\n"),
                        _ => String::new(),
                    };
                    // A Read of an image, a screenshot or an MCP tool gives pictures back as image blocks.
                    let shots: Vec<String> = block["content"].as_array().into_iter().flatten().filter(|part| part["type"] == "image").take(images::PER_RESULT).filter_map(images::of_block).collect();
                    let tool = self.items.iter_mut().rev().find_map(|item| match item {
                        Item::Tool(tool) if tool.id == id => Some(tool),
                        _ => None,
                    });
                    if let Some(tool) = tool {
                        tool.output = Some((cut(&output, OUTPUT_KEPT), block["is_error"] == true));
                        tool.images = shots;
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

    /// Keeps the checklist up with the calls that change it.
    fn track(&mut self, tool: &str, id: &str, input: &Value) {
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
            _ => {}
        }
    }

    /// The line that sends `text` and the pictures `images` (media type and base64) as your message, and the message
    /// shown at once. It goes under a uuid made from `stamp` and `n`, as a queued one does, which a rewind names.
    pub fn send(&mut self, text: &str, images: &[(String, String)], stamp: u64, n: u32) -> String {
        let uuid = uuid(stamp, n);
        self.prompts.push(Prompt { at: self.items.len(), uuid: uuid.clone(), parent: None });
        let ids = self.keep_pictures(images);
        self.items.push(sent_item(text.to_owned(), ids));
        self.turn_open = true;
        self.working = true;
        self.suggestion = None;
        // The limits are asked again with each message: the chat may only write when you send something.
        line(&json!({ "type": "user", "message": { "role": "user", "content": content(text, images) }, "parent_tool_use_id": null, "uuid": uuid })) + &usage()
    }

    /// The line that sends `text` while Claude works, and the message queued until Claude reads it. It goes in at
    /// Claude's next step, as a terminal's does: sent to wait for the turn's end, a message sat there for minutes while
    /// Claude ran command after command. `stamp` (the time in milliseconds) and `n` (this write's number) make its uuid:
    /// the plugin has no randomness, and the uuid must differ from the ones of this session's earlier starts, which share
    /// its transcript.
    pub fn queue(&mut self, text: &str, images: Vec<(String, String)>, stamp: u64, n: u32) -> String {
        let uuid = uuid(stamp, n);
        self.suggestion = None;
        let sent = user_line(text, &images, "next", &uuid);
        self.queued.push(Queued { uuid, text: text.to_owned(), images });
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
        let mut now: Value = serde_json::from_str(&user_line(&queued.text, &queued.images, "now", &fresh)).unwrap_or_default();
        // Claude Code puts work aside for a message only when a person sent it.
        now["origin"] = json!({ "kind": "human" });
        let lines = [cancel(uuid, n), line(&now)].concat();
        queued.uuid = fresh;
        Some(lines)
    }

    /// A draft that is a command with a dialog of the chat's own (`/usage`, `/cost`, `/context`): which dialog, and the
    /// request line that fills it. It is never sent as a message. Claude Code answers a control request at once, even
    /// while a turn runs, so the dialog doesn't wait for the turn.
    pub fn view_command(&mut self, text: &str) -> Option<(Panel, String)> {
        let panel = view_of(text)?;
        Some((panel, self.open_view(panel)))
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

    /// A draft that is `/plugins`, `/plugin` or `/mcp`, which open the plugins dialog over the chat: which list it leads
    /// with, and the requests that fill it, which only read. Never sent as a message either.
    pub fn dialog_command(&mut self, text: &str) -> Option<(Show, String)> {
        let show = plugins::command(text)?;
        Some((show, self.plugins.ask()))
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

    /// The lines that switch to a model and effort picked together, `<model>@<effort>`, the effort left out for a model
    /// that takes none: only what differs from now. None when nothing does.
    pub fn pick(&mut self, choice: &str, n: u32) -> Option<String> {
        let (model, effort) = choice.rsplit_once('@')?;
        let switch = self.current_model().map(|current| current.value.as_str()) != Some(model);
        let mut lines = String::new();
        if switch {
            lines.push_str(&self.set_model(model, n));
        }
        // A new model gets the effort picked with it, whatever your settings give that model.
        if !effort.is_empty() && (switch || self.current_effort().as_deref() != Some(effort)) {
            lines.push_str(&self.set_effort(effort, n));
        }
        (!lines.is_empty()).then_some(lines)
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

    /// The line that asks Claude to stop the turn it is on, the `n`th time you have, and to drop the messages queued
    /// behind it, which are then gone from the chat. Without that, Claude Code reads them as soon as it has stopped.
    pub fn interrupt(&mut self, n: u32) -> String {
        self.stopping = true;
        let cancel_queued = !std::mem::take(&mut self.queued).is_empty();
        let what = if cancel_queued { json!({ "subtype": "interrupt", "cancel_queued": true }) } else { json!({ "subtype": "interrupt" }) };
        request(&format!("interrupt-{n}"), what)
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
fn user_line(text: &str, images: &[(String, String)], priority: &str, uuid: &str) -> String {
    line(&json!({ "type": "user", "message": { "role": "user", "content": content(text, images) }, "parent_tool_use_id": null, "priority": priority, "uuid": uuid }))
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
    if pictures.is_empty() { Item::User(text) } else { Item::UserImages(text, pictures) }
}

fn cancel(uuid: &str, n: u32) -> String {
    request(&format!("cancel-{n}"), json!({ "subtype": "cancel_async_message", "message_uuid": uuid }))
}

/// The view a draft opens, when the draft is exactly that command.
/// Whether a draft is `/exit`, which closes the chat.
pub fn exits(text: &str) -> bool {
    text.trim() == "/exit"
}

pub fn view_of(text: &str) -> Option<Panel> {
    match text.trim() {
        "/usage" | "/cost" => Some(Panel::Usage),
        "/context" => Some(Panel::Context),
        _ => None,
    }
}

/// A uuid (version 4 in shape, which the protocol wants) made from the time in milliseconds and a number.
fn uuid(stamp: u64, n: u32) -> String {
    format!("{:08x}-{:04x}-4000-8000-{:012x}", (stamp >> 16) as u32, stamp & 0xffff, n)
}

/// A request of the chat's to Claude Code, answered under `id`.
pub(crate) fn request(id: &str, request: Value) -> String {
    line(&json!({ "type": "control_request", "request_id": id, "request": request }))
}

/// The slash commands `initialize` or a reload of the plugins lists.
fn commands(list: &Value) -> Vec<Command> {
    let text = |value: &Value, key: &str| value[key].as_str().unwrap_or_default().to_owned();
    list.as_array()
        .into_iter()
        .flatten()
        .map(|command| Command { name: text(command, "name"), description: text(command, "description"), hint: text(command, "argumentHint") })
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

/// What a tool is doing, in one line: the command it runs, the file or the pattern, or else its first text input.
pub fn summary(tool: &str, input: &Value) -> String {
    let key = match tool {
        "Bash" => "command",
        "Read" | "Edit" | "Write" | "NotebookEdit" => "file_path",
        "Grep" | "Glob" => "pattern",
        "WebFetch" => "url",
        "WebSearch" => "query",
        "Task" | "Agent" => "description",
        _ => "",
    };
    let text = input[key].as_str().or_else(|| input.as_object()?.values().find_map(Value::as_str)).unwrap_or_default();
    cut(text.lines().next().unwrap_or_default(), SUMMARY_KEPT)
}

impl Session {
    /// Shows `used` percent of the limit `label`, the 5-hour one first.
    fn set_limit(&mut self, label: &'static str, used: f64, resets: Option<f64>) {
        self.limits.retain(|(shown, ..)| *shown != label);
        self.limits.push((label, used.round() as u64, resets));
        self.limits.sort_by_key(|(shown, ..)| *shown != "5h");
    }

    /// What Claude is doing now, as the line under the conversation says it: the tool it runs, else thinking or writing.
    pub fn activity(&self) -> String {
        if let Some(status) = &self.status {
            return status.clone();
        }
        let running = self.items.iter().rev().find_map(|item| match item {
            Item::Tool(tool) if tool.output.is_none() => Some(tool),
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
            Item::Tool(tool) if matches!(tool.name.as_str(), "Task" | "Agent") => (started + 1, running + usize::from(tool.output.is_none())),
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
            Item::Tool(tool) if tool.output.is_none() => Some(tool),
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
    let field = |key: &str| shown(input[key].as_str().unwrap_or_default(), true);
    let change = |edit: &Value| {
        let lines = |key: &str, mark: &str| edit[key].as_str().unwrap_or_default().lines().map(|line| format!("{mark} {}", shown(line, true))).collect::<Vec<_>>();
        let every = if edit["replace_all"] == true { "Every match of:\n" } else { "" };
        format!("{every}{}\n{}", lines("old_string", "−").join("\n"), lines("new_string", "+").join("\n"))
    };
    let (said, keys): (String, &[&str]) = match tool {
        // A command's description is Claude's words about it, which don't run.
        "Bash" => (shown(input["command"].as_str().unwrap_or_default().trim(), false), &["command", "description"]),
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
fn shown(text: &str, one_line: bool) -> String {
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
mod tests {
    use super::*;

    /// A turn as Claude Code 2.1.327 printed it, trimmed to the lines the chat reads and a few it skips.
    const TURN: &str = include_str!("../tests/turn.jsonl");

    fn heard(lines: &str) -> Session {
        let mut session = Session::default();
        for line in lines.lines().filter(|line| !line.trim().is_empty()) {
            session.hear(&serde_json::from_str(line).unwrap());
        }
        session
    }

    #[test]
    fn a_turn_becomes_the_tool_call_with_its_output_then_the_answer() {
        let mut session = Session::default();
        session.send("How many files?", &[], 1, 1);
        for line in TURN.lines().filter(|line| !line.trim().is_empty()) {
            session.hear(&serde_json::from_str(line).unwrap());
        }
        assert_eq!(session.id.as_deref(), Some("1fccd556-e115-4d47-9a08-f291a83a5457"));
        assert_eq!(session.model.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(
            session.items,
            [
                Item::User("How many files?".into()),
                Item::Tool(Tool {
                    id: "toolu_01PQFGJCuZRyDiWjCATE1ktf".into(),
                    name: "Bash".into(),
                    summary: "ls".into(),
                    output: Some(("err.txt\nout.jsonl\nprobe-file.txt".into(), false)),
                    detail: Detail::Command("ls".into()),
                    images: Vec::new(),
                }),
                Item::Text("There are 3 files:\n\n- `err.txt`\n- `out.jsonl`\n- `probe-file.txt`".into()),
            ]
        );
        assert!(!session.working);
        assert!(session.live.is_empty());
        assert_eq!(session.cost, 0.00596854);
    }

    /// The line Claude Code 2.1.327 prints after a turn's result with `--prompt-suggestions`.
    const SUGGESTION: &str = r#"{"type":"prompt_suggestion","suggestion":"run it","uuid":"cdab3e06-e878-423a-add5-9ed9174c610e","session_id":"4414f6b3-7ed9-4059-bbb1-974fbcaf0b9b"}"#;

    #[test]
    fn the_suggested_next_message_comes_after_the_turn_and_goes_with_the_next_one() {
        let mut session = heard(TURN);
        assert_eq!(session.suggestion, None);
        session.hear(&serde_json::from_str(SUGGESTION).unwrap());
        assert_eq!(session.suggestion.as_deref(), Some("run it"));
        assert!(session.items.iter().all(|item| !matches!(item, Item::User(_))));

        session.send("run it", &[], 1, 1);
        assert_eq!(session.suggestion, None);

        // A turn that starts on its own, or a message queued while Claude works, clears it as well.
        session.hear(&serde_json::from_str(SUGGESTION).unwrap());
        session.queue("and lint", Vec::new(), 1, 1);
        assert_eq!(session.suggestion, None);
        session.hear(&serde_json::from_str(SUGGESTION).unwrap());
        session.hear(&serde_json::from_str(r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Running."}]}}"#).unwrap());
        assert_eq!(session.suggestion, None);

        session.hear(&serde_json::from_str(r#"{"type":"prompt_suggestion","suggestion":"  "}"#).unwrap());
        assert_eq!(session.suggestion, None);
    }

    #[test]
    fn text_shows_as_it_streams_in_before_the_message_is_whole() {
        // Up to the delta that writes the third file's name.
        let partial = TURN.lines().take_while(|line| !line.contains("l`\\n- `probe-")).collect::<Vec<_>>().join("\n");
        assert_eq!(heard(&partial).live, "There are 3 files:\n\n- `err.txt`\n- `out.json");
    }

    #[test]
    fn a_permission_question_is_answered_once_with_the_input_it_asked_about() {
        let mut session = heard(
            r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"touch a\nb","description":"Make a"}}}"#,
        );
        let Item::Ask(ask) = &session.items[0] else { panic!("{:?}", session.items) };
        assert_eq!((ask.tool.as_str(), ask.summary.as_str()), ("Bash", "touch a"));
        let allow: Value = serde_json::from_str(&session.allow("r1", false).unwrap()).unwrap();
        assert_eq!(
            allow,
            json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "r1", "response": { "behavior": "allow", "updatedInput": { "command": "touch a\nb", "description": "Make a" } } } })
        );
        assert_eq!(session.deny("r1", ""), None, "answered already");
        assert_eq!(session.allow("r2", false), None, "never asked");
        let mut denied = heard(r#"{"type":"control_request","request_id":"r3","request":{"subtype":"can_use_tool","tool_name":"Write","input":{}}}"#);
        let deny: Value = serde_json::from_str(&denied.deny("r3", "  ").unwrap()).unwrap();
        assert_eq!(deny["response"]["response"], json!({ "behavior": "deny", "message": "The user said no to this in the chat." }));
    }

    #[test]
    fn a_subagents_lines_are_kept_under_its_call_and_stay_out_of_the_conversation() {
        let mut session = Session::default();
        session.now = 1_000;
        for line in [
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Task","input":{"description":"Survey"}}]}}"#,
            r#"{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"model":"claude-haiku-5-5","content":[{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"user","parent_tool_use_id":"toolu_1","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_2","content":"a.txt"}]}}"#,
            r#"{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"content":[{"type":"text","text":"Found a.txt"}]}}"#,
        ] {
            session.hear(&serde_json::from_str(line).unwrap());
            session.now += 500;
        }
        assert_eq!(session.items.len(), 1);
        let sub = &session.subagents["toolu_1"];
        assert_eq!(sub.items.len(), 2);
        assert!(matches!(&sub.items[0], Item::Tool(Tool { output: Some((out, false)), .. }) if out == "a.txt"));
        assert_eq!(sub.items[1], Item::Text("Found a.txt".into()));
        assert_eq!((sub.started, sub.last), (1_500, 2_500));
        assert_eq!(sub.reported_model.as_deref(), Some("claude-haiku-5-5"));
        // Its words never reach the conversation's own state.
        assert!(session.live.is_empty() && session.reported_model.is_none());
    }

    #[test]
    fn a_subagents_list_keeps_only_its_newest_items() {
        let mut session = Session::default();
        for n in 0..SUBAGENT_KEPT + 30 {
            let line = json!({ "type": "assistant", "parent_tool_use_id": "toolu_1", "message": { "content": [{ "type": "text", "text": format!("step {n}") }] } });
            session.hear(&line);
        }
        let sub = &session.subagents["toolu_1"];
        assert_eq!(sub.items.len(), SUBAGENT_KEPT);
        assert_eq!(sub.items[0], Item::Text("step 30".into()));
        assert_eq!(sub.items.last(), Some(&Item::Text(format!("step {}", SUBAGENT_KEPT + 29))));
    }

    #[test]
    fn a_model_is_named_as_a_person_says_it() {
        assert_eq!(model_name("claude-haiku-5-5"), "Haiku 5.5");
        assert_eq!(model_name("claude-opus-4-1-20250805"), "Opus 4.1");
        assert_eq!(model_name("claude-sonnet-5"), "Sonnet 5");
        assert_eq!(model_name("gpt-4o"), "gpt-4o");
    }

    #[test]
    fn a_resumed_sessions_transcript_reads_like_the_stream() {
        let session = heard(concat!(
            r#"{"type":"user","message":{"role":"user","content":"Fix the build"}}"#,
            "\n",
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Done."}]}}"#,
            "\n",
            r#"{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"content":[{"type":"text","text":"a subagent's"}]}}"#,
        ));
        assert_eq!(session.items, [Item::User("Fix the build".into()), Item::Text("Done.".into())]);
        // As this very session's transcript has them: what Claude Code wrote in your name is a note, not yours.
        let added = heard(concat!(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b1</task-id>\n<summary>Survey stopped</summary>\n</task-notification>"}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<local-command-caveat>The command below was run</local-command-caveat>"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/reload-plugins</command-name>\n<command-args></command-args>"}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"<b>bold</b> is mine"}}"#,
        ));
        assert_eq!(
            added.items,
            [
                Item::Note("Survey stopped".into(), "b1\nSurvey stopped".into()),
                Item::Note("Ran /reload-plugins".into(), "/reload-plugins".into()),
                Item::User("<b>bold</b> is mine".into()),
            ]
        );
    }

    #[test]
    fn pictures_go_as_image_blocks_before_the_text_and_show_by_their_ids() {
        images::reset();
        let pictures = [("image/png".to_owned(), "AAAA".to_owned()), ("image/jpeg".to_owned(), "BBBB".to_owned())];
        let mut session = Session::default();
        let sent: Value = serde_json::from_str(session.send("what is this?", &pictures, 1, 1).lines().next().unwrap()).unwrap();
        assert_eq!(
            sent["message"]["content"],
            json!([
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } },
                { "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": "BBBB" } },
                { "type": "text", "text": "what is this?" },
            ])
        );
        // Without text there is no empty text block, which the API refuses.
        let alone: Value = serde_json::from_str(session.send("", &pictures[..1], 1, 1).lines().next().unwrap()).unwrap();
        assert_eq!(alone["message"]["content"], json!([{ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } }]));
        let [Item::UserImages(text, two), Item::UserImages(none, one)] = &session.items[..] else {
            panic!("{:?}", session.items)
        };
        assert_eq!((text.as_str(), two.len(), none.as_str(), one.len()), ("what is this?", 2, "", 1));
        // Each picture is kept for Wings under its id, once, and the id is what the item holds.
        let kept = images::pending();
        assert_eq!(kept.iter().map(|(_, data)| data.as_str()).collect::<Vec<_>>(), ["AAAA", "BBBB", "AAAA"]);
        assert_eq!(kept.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(), [two.clone(), one.clone()].concat());
    }

    #[test]
    fn a_queued_message_keeps_its_pictures_for_send_now() {
        let mut session = Session::default();
        let sent = parsed(&session.queue("look", vec![("image/gif".into(), "CCCC".into())], 1, 1));
        assert_eq!(sent[0]["message"]["content"][0]["source"]["media_type"], "image/gif");
        assert_eq!(sent[0]["message"]["content"][1], json!({ "type": "text", "text": "look" }));
        let uuid = session.queued[0].uuid.clone();
        let again = parsed(&session.send_now(&uuid, 2, 2).unwrap());
        assert_eq!(again[1]["message"]["content"][0]["source"]["data"], "CCCC");
    }

    #[test]
    fn a_transcript_message_with_pictures_shows_them() {
        images::reset();
        let session = heard(concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}},{"type":"text","text":"what is this?"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}}]}}"#,
        ));
        let [Item::UserImages(text, first), Item::UserImages(none, second)] = &session.items[..] else {
            panic!("{:?}", session.items)
        };
        assert_eq!((text.as_str(), first.len(), none.as_str(), second.len()), ("what is this?", 1, "", 1));
        assert_ne!(first, second);
        assert!(images::is_shown(&first[0]) && images::is_shown(&second[0]));
    }

    #[test]
    fn a_failed_turn_says_why_and_ends() {
        let mut session = Session::default();
        session.send("hi", &[], 1, 1);
        session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "errors": ["Credit balance is too low"] }));
        assert!(!session.working);
        assert_eq!(session.items[1], Item::Error("Credit balance is too low".into()));
    }

    #[test]
    fn a_hit_limit_shows_at_once_in_the_limits_and_once_in_the_conversation() {
        let answer = |response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": USAGE, "response": response } });
        let mut session = Session::default();
        session.send("go on", &[], 1, 1);
        session.hear(&answer(json!({ "rate_limits": {
            "five_hour": { "utilization": 67, "resets_at": "2026-11-18T19:30:00.262659+00:00" },
            "seven_day": { "utilization": 97, "resets_at": "2026-11-19T10:59:59+00:00" } } })));
        // As Claude Code 2.1.327 says it once the 5-hour limit is hit, past the last message's answer.
        let limited = "You've hit your 5-hour limit · resets 8:30pm (Europe/Zagreb)";
        session.hear(&json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "rejected", "resetsAt": 1795030200,
            "rateLimitType": "five_hour", "unifiedWindows": {
                "five_hour": { "utilization": 1, "resetsAt": 1795030200, "status": "rejected", "surpassedThreshold": 1 },
                "seven_day": { "utilization": 0.98, "resetsAt": 1795086000, "status": "allowed_warning", "surpassedThreshold": 0.75 } } } }));
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": limited }] }, "error": "rate_limit" }));
        session.hear(&json!({ "type": "result", "subtype": "success", "is_error": true, "result": limited }));
        assert_eq!(session.limits, [("5h", 100, Some(1_795_030_200_000.0)), ("week", 98, Some(1_795_086_000_000.0))]);
        assert_eq!(session.items, [Item::User("go on".into()), Item::Error(limited.into())]);
        // An answer that comes without them keeps the limits shown.
        session.hear(&answer(json!({ "rate_limits": null })));
        assert_eq!(session.limits.len(), 2);
    }

    #[test]
    fn long_output_is_cut_at_a_character() {
        assert_eq!(cut("ééé", 3), "é…");
        assert_eq!(cut("abc", 3), "abc");
        assert_eq!(summary("Mystery", &json!({ "n": 1, "path": "/a/b" })), "/a/b");
    }

    #[test]
    fn a_stopped_turn_shows_as_stopped_not_as_an_error() {
        let mut session = Session::default();
        session.send("count to 400", &[], 1, 1);
        let stop: Value = serde_json::from_str(&session.interrupt(4)).unwrap();
        assert_eq!(stop, json!({ "type": "control_request", "request_id": "interrupt-4", "request": { "subtype": "interrupt" } }));
        // As Claude Code 2.1.327 ended a stopped turn.
        session.hear(&json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": "[Request interrupted by user]" }] } }));
        session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true,
            "errors": ["[ede_diagnostic] result_type=user last_content_type=n/a stop_reason=null"] }));
        assert_eq!(session.items, [Item::User("count to 400".into()), Item::Stopped]);
        assert!(!session.working);
        // The next turn's error is an error again.
        session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "errors": ["Overloaded"] }));
        assert_eq!(session.items[2], Item::Error("Overloaded".into()));
    }

    fn parsed(lines: &str) -> Vec<Value> {
        lines.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }

    #[test]
    fn a_message_sent_while_claude_works_is_queued_for_claudes_next_step_with_a_uuid() {
        let mut session = Session::default();
        session.send("count to 400", &[], 1, 1);
        let sent = parsed(&session.queue("and then stop", Vec::new(), 1_700_000_000_123, 5));
        assert_eq!(sent.len(), 1);
        let uuid = sent[0]["uuid"].as_str().unwrap().to_owned();
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.matches('-').count(), 4);
        assert_eq!(
            sent[0],
            json!({ "type": "user", "message": { "role": "user", "content": "and then stop" }, "parent_tool_use_id": null, "priority": "next", "uuid": uuid })
        );
        assert_eq!(session.queued, [Queued { uuid: uuid.clone(), text: "and then stop".into(), images: Vec::new() }]);
        // Another write, even in the same millisecond, has another uuid.
        session.queue("more", Vec::new(), 1_700_000_000_123, 6);
        assert_ne!(session.queued[0].uuid, session.queued[1].uuid);
    }

    #[test]
    fn taking_a_queued_message_back_cancels_it_by_uuid() {
        let mut session = Session::default();
        session.queue("one", Vec::new(), 1, 1);
        let uuid = session.queued[0].uuid.clone();
        let cancelled: Value = serde_json::from_str(&session.unqueue(&uuid, 2).unwrap()).unwrap();
        assert_eq!(cancelled, json!({ "type": "control_request", "request_id": "cancel-2", "request": { "subtype": "cancel_async_message", "message_uuid": uuid } }));
        assert!(session.queued.is_empty());
        assert_eq!(session.unqueue(&uuid, 3), None, "gone already");
    }

    #[test]
    fn send_now_cancels_and_sends_again_to_be_read_at_once_in_one_write() {
        let mut session = Session::default();
        session.queue("first", Vec::new(), 1, 1);
        session.queue("second", Vec::new(), 1, 2);
        let old = session.queued[1].uuid.clone();
        let written = session.send_now(&old, 2, 3).unwrap();
        let lines = parsed(&written);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["request"], json!({ "subtype": "cancel_async_message", "message_uuid": old }));
        assert_eq!((lines[1]["priority"].as_str(), &lines[1]["origin"]), (Some("now"), &json!({ "kind": "human" })));
        assert_eq!(lines[1]["message"]["content"], "second");
        let fresh = lines[1]["uuid"].as_str().unwrap();
        assert_ne!(fresh, old);
        // Still queued, in its place, under the new uuid.
        assert_eq!((session.queued[1].uuid.as_str(), session.queued[1].text.as_str()), (fresh, "second"));
        assert_eq!(session.send_now("nope", 2, 4), None);
    }

    #[test]
    fn a_queued_message_moves_into_the_conversation_when_claude_echoes_it_and_only_once() {
        let mut session = Session::default();
        session.send("count to 400", &[], 1, 1);
        session.queue("then say done", Vec::new(), 1, 1);
        let uuid = session.queued[0].uuid.clone();
        // The echo of the first message, sent while Claude was idle, is on the page already.
        session.hear(&json!({ "type": "user", "isReplay": true, "uuid": "x", "message": { "role": "user", "content": "count to 400" } }));
        assert_eq!(session.items, [Item::User("count to 400".into())]);
        let echo = json!({ "type": "user", "isReplay": true, "uuid": uuid, "session_id": "s", "message": { "role": "user", "content": "then say done" } });
        session.hear(&echo);
        session.hear(&echo);
        assert_eq!(session.items, [Item::User("count to 400".into()), Item::User("then say done".into())]);
        assert!(session.queued.is_empty());
    }

    #[test]
    fn stop_with_messages_queued_cancels_them_too() {
        let mut session = Session::default();
        session.send("count to 400", &[], 1, 1);
        session.queue("one", Vec::new(), 1, 1);
        session.queue("two", Vec::new(), 1, 2);
        let stop: Value = serde_json::from_str(&session.interrupt(3)).unwrap();
        assert_eq!(stop, json!({ "type": "control_request", "request_id": "interrupt-3", "request": { "subtype": "interrupt", "cancel_queued": true } }));
        assert!(session.queued.is_empty());
    }

    #[test]
    fn the_working_state_follows_session_state_changed_once_claude_code_prints_it() {
        let state = |state: &str| json!({ "type": "system", "subtype": "session_state_changed", "state": state });
        let result = json!({ "type": "result", "subtype": "success", "is_error": false });
        // Without the events, the result ends the turn.
        let mut plain = Session::default();
        plain.send("hi", &[], 1, 1);
        plain.hear(&result);
        assert!(!plain.working);
        // With them, a turn's result isn't the end: the next queued turn goes on, and `idle` ends it.
        let mut session = Session::default();
        session.hear(&state("running"));
        session.queue("next", Vec::new(), 1, 1);
        let uuid = session.queued[0].uuid.clone();
        session.hear(&result);
        assert!(session.working);
        session.hear(&json!({ "type": "user", "isReplay": true, "uuid": uuid, "message": { "role": "user", "content": "next" } }));
        session.hear(&state("requires_action"));
        assert!(session.working);
        session.hear(&result);
        session.hear(&state("idle"));
        assert!(!session.working);
        // A message nothing echoed is shown once Claude is idle, not left in the queue.
        session.hear(&state("running"));
        session.queue("lost", Vec::new(), 1, 2);
        session.hear(&state("idle"));
        assert!(session.queued.is_empty());
        assert_eq!(session.items.last(), Some(&Item::User("lost".into())));
    }

    #[test]
    fn you_are_told_once_a_turn_when_claude_needs_you_and_once_when_it_finishes() {
        let ask = |id: &str| json!({ "type": "control_request", "request_id": id, "request": { "subtype": "can_use_tool", "tool_name": "Bash", "input": { "command": "rm x" } } });
        let mut session = Session::default();
        session.send("clean up", &[], 1, 1);
        session.hear(&ask("r1"));
        session.hear(&ask("r2"));
        assert_eq!(session.alerts, [("Claude needs you: Bash".to_owned(), "rm x".to_owned())]);
        session.alerts.clear();
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "\nDone, x is gone.\nAnything else?" }] } }));
        session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false }));
        session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false }));
        assert_eq!(session.alerts, [("Claude finished".to_owned(), "Done, x is gone.".to_owned())]);
        // The next turn tells you again.
        session.alerts.clear();
        session.hear(&ask("r3"));
        assert_eq!(session.alerts.len(), 1);
    }

    fn asked(extra: Value) -> Session {
        let mut request = json!({ "subtype": "can_use_tool", "tool_name": "Bash", "input": { "command": "npm test" } });
        request.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        heard(&json!({ "type": "control_request", "request_id": "r1", "request": request }).to_string())
    }

    fn sent(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn the_opening_input_asks_for_a_start_mode_only_when_given_one() {
        let lines = |mode| Session::opening(mode).lines().map(sent).collect::<Vec<_>>();
        assert!(lines(None).iter().all(|line| line["request"]["subtype"] != "set_permission_mode"));
        let asked = lines(Some("auto"));
        assert_eq!(asked.len(), 7);
        let last = asked.last().unwrap();
        assert_eq!(last["request_id"], START_MODE);
        assert_eq!(last["request"], json!({ "subtype": "set_permission_mode", "mode": "auto" }));
        // Once accepted the picker shows it, though Claude Code's own answers before it say it started in `default`.
        let answer = |id: &str, response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
        let mut session = Session { start_mode: Some("auto".into()), ..Session::default() };
        session.hear(&answer(INITIALIZE, json!({ "current_permission_mode": "default" })));
        assert_eq!(session.mode.as_deref(), Some("default"));
        session.hear(&answer(START_MODE, json!({})));
        assert_eq!(session.mode.as_deref(), Some("auto"));
        // Refused, it stays as Claude Code has it.
        let mut refused = Session { start_mode: Some("auto".into()), ..Session::default() };
        refused.hear(&answer(INITIALIZE, json!({ "current_permission_mode": "default" })));
        refused.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": START_MODE, "error": "no" } }));
        assert_eq!(refused.mode.as_deref(), Some("default"));
    }

    #[test]
    fn an_output_shows_its_first_lines_and_counts_the_rest() {
        assert_eq!(first_lines("a\nb\nc", 2), ("a\nb", 1));
        assert_eq!(first_lines("a\nb\nc", 3), ("a\nb\nc", 0));
        assert_eq!(first_lines("a\nb", 5), ("a\nb", 0));
        assert_eq!(first_lines("a\nb\nc\nd\n", 1), ("a", 3));
    }

    #[test]
    fn always_allow_gives_the_suggested_rules_back() {
        let rules = json!([{ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": "npm test:*" }], "behavior": "allow", "destination": "localSettings" }]);
        let mut session = asked(json!({ "permission_suggestions": rules, "title": "Run the tests", "decision_reason": "\u{1b}[31mNot allowed\u{1b}[0m yet" }));
        let Item::Ask(ask) = &session.items[0] else { panic!() };
        assert_eq!(ask.reason.as_deref(), Some("Run the tests · Not allowed yet"));
        assert_eq!(ask.suggestions.len(), 1);
        let line = sent(&session.allow("r1", true).unwrap());
        assert_eq!(line["response"]["response"], json!({ "behavior": "allow", "updatedInput": { "command": "npm test" }, "updatedPermissions": rules }));
        // Allow once carries no rules, and a request that suppresses them offers none.
        let mut once = asked(json!({ "permission_suggestions": rules }));
        assert!(sent(&once.allow("r1", false).unwrap())["response"]["response"].get("updatedPermissions").is_none());
        let Item::Ask(ask) = &asked(json!({ "permission_suggestions": rules, "suppress_always_allow_rule": true })).items[0] else { panic!() };
        assert!(ask.suggestions.is_empty());
    }

    #[test]
    fn deny_tells_claude_what_you_wrote() {
        let mut session = asked(json!({}));
        let line = sent(&session.deny("r1", " use pnpm instead ").unwrap());
        assert_eq!(line["response"]["response"], json!({ "behavior": "deny", "message": "use pnpm instead" }));
        let Item::Ask(ask) = &session.items[0] else { panic!() };
        assert_eq!(ask.answer, Some(false));
    }

    #[test]
    fn the_chat_starts_on_your_settings_model_and_effort_and_shows_your_limits() {
        let answer = |id: &str, response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
        let efforts = json!(["low", "medium", "high", "xhigh", "max"]);
        let mut session = Session::default();
        session.hear(&answer(INITIALIZE, json!({ "models": [
            { "value": "default", "displayName": "Default", "supportedEffortLevels": efforts, "resolvedModel": "claude-opus-5-5" },
            { "value": "opus", "displayName": "Opus 5.5", "supportedEffortLevels": efforts, "resolvedModel": "claude-opus-5-5" },
            { "value": "sonnet", "displayName": "Sonnet 5.5", "supportedEffortLevels": efforts, "resolvedModel": "claude-sonnet-5-5" },
        ] })));
        // As a terminal's Claude: `model` and `effortLevel`, and `modelSettings` over them for one model.
        session.hear(&answer(SETTINGS, json!({ "effective": { "model": "opus", "effortLevel": "high",
            "modelSettings": { "claude-opus-5-5": { "effortLevel": "xhigh" } } } })));
        assert_eq!(session.current_model().map(|model| model.name.as_str()), Some("Opus 5.5"));
        assert_eq!(session.current_effort().as_deref(), Some("xhigh"));
        session.set_model("sonnet", 1);
        assert_eq!(session.current_effort().as_deref(), Some("high"));
        session.set_effort("low", 2);
        assert_eq!(session.current_effort().as_deref(), Some("low"));
        session.hear(&answer(USAGE, json!({ "rate_limits": {
            "five_hour": { "utilization": 96.4, "resets_at": "2026-11-18T14:29:59.562698+00:00" },
            "seven_day": { "utilization": 91, "resets_at": "2026-11-19T10:59:59+00:00" }, "seven_day_opus": null } })));
        assert_eq!(session.limits.iter().map(|(label, used, _)| (*label, *used)).collect::<Vec<_>>(), [("5h", 96), ("week", 91)]);
        // Each message asks for them again.
        assert!(session.send("hi", &[], 1, 1).contains(r#""request_id":"usage""#));
    }

    #[test]
    fn the_model_stays_as_picked_once_claude_code_names_it_in_full_and_a_skills_text_isnt_yours() {
        let answer = |id: &str, response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
        let mut session = Session::default();
        session.hear(&answer(INITIALIZE, json!({ "models": [
            { "value": "default", "displayName": "Default (recommended)", "resolvedModel": "claude-opus-5-5" },
            { "value": "opus", "displayName": "Opus 5.5", "resolvedModel": "claude-opus-5-5" },
            { "value": "sonnet", "displayName": "Sonnet 5.5", "resolvedModel": "claude-sonnet-5-5" },
        ] })));
        session.hear(&answer(SETTINGS, json!({ "effective": { "model": "opus" } })));
        session.send("go", &[], 1, 1);
        session.hear(&json!({ "type": "system", "subtype": "init", "session_id": "s", "model": "claude-opus-5-5" }));
        assert_eq!(session.current_model().map(|model| model.name.as_str()), Some("Opus 5.5"));
        // Started with no model of its own: the full name reads as the model, not as "Default".
        let mut fresh = Session::default();
        fresh.hear(&json!({ "type": "system", "subtype": "init", "session_id": "s", "model": "claude-opus-5-5" }));
        fresh.hear(&answer(INITIALIZE, json!({ "models": [
            { "value": "default", "displayName": "Default (recommended)", "resolvedModel": "claude-opus-5-5" },
            { "value": "opus", "displayName": "Opus 5.5", "resolvedModel": "claude-opus-5-5" } ] })));
        assert_eq!(fresh.current_model().map(|model| model.name.as_str()), Some("Opus 5.5"));
        // A skill's text, as Claude Code prints it once the Skill tool loaded it.
        let shown = session.items.len();
        session.hear(&json!({ "type": "user", "isSynthetic": true, "message": { "role": "user", "content": [{ "type": "text", "text": "# Keybindings Skill" }] } }));
        assert_eq!(session.items.len(), shown);
    }

    #[test]
    fn the_status_line_names_the_tool_that_runs() {
        let mut session = Session::default();
        session.send("go", &[], 1, 1);
        assert_eq!(session.activity(), "Working");
        tool_call(&mut session, "a", "mcp__wings__tasks__list", json!({}));
        assert_eq!(session.activity(), "Calling wings");
        tool_result(&mut session, "a", "[]", false);
        tool_call(&mut session, "b", "mcp__plugin_repo-docs_repo-docs__find_docs", json!({ "query": "x" }));
        assert_eq!(session.activity(), "Calling repo-docs");
        tool_result(&mut session, "b", "", false);
        tool_call(&mut session, "c", "Read", json!({ "file_path": "/work/shop/src/checkout.ts" }));
        assert_eq!(session.activity(), "Reading checkout.ts");
        tool_result(&mut session, "c", "", false);
        tool_call(&mut session, "d", "Bash", json!({ "command": "pnpm test\necho done" }));
        assert_eq!(session.activity(), "Running pnpm test");
    }

    #[test]
    fn the_card_shows_all_a_tool_would_do_and_the_characters_that_hide() {
        let detail = |extra: Value| {
            let Item::Ask(ask) = asked(extra).items.remove(0) else { panic!() };
            ask
        };
        let ask = detail(json!({ "input": { "command": "a\nb\nc\nd\ne\nf\ng\nh" } }));
        assert_eq!((ask.summary.as_str(), ask.detail.as_str()), ("a", "a\nb\nc\nd\ne\nf\ng\nh"));
        // A right-to-left mark, a zero-width space, a Hangul filler or a tag would make the command read other than it
        // runs. Letters stay as they are.
        let tricky = detail(json!({ "input": { "command": "ls \u{202e}fdp.exe\u{200b} \u{3164} \u{e0041} café" } })).detail;
        assert_eq!(tricky, "ls \\u{202e}fdp.exe\\u{200b} \\u{3164} \\u{e0041} café");
        // A path is one line: a line break in it is written out, not drawn.
        let edit = json!({ "file_path": "/a.rs\n/b.rs", "old_string": "one\ntwo", "new_string": "one\n2" });
        assert_eq!(detail(json!({ "tool_name": "Edit", "input": edit })).detail, "/a.rs\\u{a}/b.rs\n\n− one\n− two\n+ one\n+ 2");
        assert_eq!(detail(json!({ "tool_name": "Write", "input": { "file_path": "/b", "content": "x\ny" } })).detail, "/b\n\nx\ny");
        assert_eq!(detail(json!({ "tool_name": "mcp__db__drop", "input": { "table": "users" } })).detail, "{\n  \"table\": \"users\"\n}");
        // What else the input holds shows too, and leaving the sandbox says so first.
        let outside = detail(json!({ "input": { "command": "make", "description": "Build", "dangerouslyDisableSandbox": true, "timeout": 600000 } })).detail;
        assert_eq!(outside, "Runs outside the sandbox.\n\nmake\n\n{\n  \"dangerouslyDisableSandbox\": true,\n  \"timeout\": 600000\n}");
        // Past what a card can show, it says so.
        let long = detail(json!({ "input": { "command": "x".repeat(ASK_KEPT + 1) } }));
        assert!(long.cut_short);
        assert!(!ask.cut_short);
    }

    #[test]
    fn the_questions_are_answered_by_text_with_labels_or_other_words() {
        let questions = json!([
            { "question": "Which library?", "header": "Library", "multiSelect": false, "options": [{ "label": "date-fns", "description": "" }, { "label": "dayjs", "description": "" }] },
            { "question": "Which features?", "header": "Features", "multiSelect": true, "options": [{ "label": "Parse", "description": "" }, { "label": "Format", "description": "" }] },
        ]);
        let mut session = heard(&json!({ "type": "control_request", "request_id": "q1", "request": { "subtype": "can_use_tool", "tool_name": "AskUserQuestion", "input": { "questions": questions } } }).to_string());
        let mut draft = Draft::default();
        assert_eq!(session.submit("q1", &draft), None, "nothing picked");
        draft.toggle(0, 0, false);
        draft.toggle(0, 1, false);
        draft.toggle(1, 0, true);
        draft.toggle(1, 1, true);
        draft.set_other(1, " and Diff ".into(), true);
        let line = sent(&session.submit("q1", &draft).unwrap());
        assert_eq!(
            line["response"]["response"],
            json!({ "behavior": "allow", "updatedInput": { "questions": questions, "answers": { "Which library?": "dayjs", "Which features?": "Parse, Format, and Diff" } } })
        );
    }

    #[test]
    fn approving_a_plan_allows_it_and_switches_the_mode_in_one_write() {
        let mut session = heard(r##"{"type":"control_request","request_id":"p1","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"# Plan"}}}"##);
        let written = session.approve_plan("p1", "acceptEdits", 7).unwrap();
        let lines: Vec<Value> = written.lines().map(sent).collect();
        assert_eq!(lines.len(), 2);
        assert!(written.ends_with('\n'));
        assert_eq!(lines[0]["response"]["response"], json!({ "behavior": "allow", "updatedInput": { "plan": "# Plan" } }));
        assert_eq!(lines[1], json!({ "type": "control_request", "request_id": "mode-7", "request": { "subtype": "set_permission_mode", "mode": "acceptEdits" } }));
        assert_eq!(session.mode.as_deref(), Some("acceptEdits"));
        assert_eq!(session.approve_plan("p1", "default", 8), None, "answered already");
    }

    #[test]
    fn a_cancel_request_withdraws_the_card() {
        let mut session = asked(json!({}));
        session.hear(&json!({ "type": "control_cancel_request", "request_id": "other" }));
        assert_eq!(session.pending("r1").is_some(), true);
        session.hear(&json!({ "type": "control_cancel_request", "request_id": "r1" }));
        let Item::Ask(ask) = &session.items[0] else { panic!() };
        assert_eq!(ask.answer, Some(false));
        assert_eq!(session.allow("r1", false), None, "nothing left to answer");
    }

    fn tool_call(session: &mut Session, id: &str, name: &str, input: Value) {
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] } }));
    }

    fn tool_result(session: &mut Session, id: &str, content: &str, error: bool) {
        session.hear(&json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "content": content, "is_error": error }] } }));
    }

    #[test]
    fn an_edit_counts_the_lines_it_takes_out_and_puts_in() {
        let mut session = Session::default();
        tool_call(&mut session, "e1", "Edit", json!({ "file_path": "/a/b.rs", "old_string": "one\ntwo", "new_string": "one\n2\nthree" }));
        let Item::Tool(tool) = &session.items[0] else { panic!("{:?}", session.items) };
        assert_eq!(
            tool.detail,
            Detail::Diff(Diff {
                added: 3,
                removed: 2,
                lines: vec![(false, "one".into()), (false, "two".into()), (true, "one".into()), (true, "2".into()), (true, "three".into())],
                more: 0,
            })
        );
        // A multi-edit adds its edits up, and a long one keeps only its first lines.
        let many = "x\n".repeat(30);
        tool_call(&mut session, "e2", "MultiEdit", json!({ "file_path": "/a", "edits": [{ "old_string": many, "new_string": "y" }, { "old_string": "", "new_string": many }] }));
        let Item::Tool(tool) = &session.items[1] else { panic!() };
        let Detail::Diff(diff) = &tool.detail else { panic!("{:?}", tool.detail) };
        assert_eq!((diff.added, diff.removed, diff.lines.len(), diff.more), (31, 30, 40, 21));
    }

    #[test]
    fn a_write_a_read_and_a_command_keep_what_the_view_draws() {
        let mut session = Session::default();
        tool_call(&mut session, "w", "Write", json!({ "file_path": "/a", "content": "a\nb\nc\n" }));
        tool_call(&mut session, "r", "Read", json!({ "file_path": "/a", "offset": 10, "limit": 50 }));
        tool_call(&mut session, "r2", "Read", json!({ "file_path": "/a" }));
        let details: Vec<&Detail> = session.items.iter().filter_map(|item| if let Item::Tool(tool) = item { Some(&tool.detail) } else { None }).collect();
        assert_eq!(details[0], &Detail::Write { lines: 3, head: vec!["a".into(), "b".into(), "c".into()] });
        assert_eq!(details[1], &Detail::Read(Some("lines 10–59".into())));
        assert_eq!(details[2], &Detail::Read(None));
    }

    #[test]
    fn failed_commands_and_searches_say_what_they_printed() {
        assert_eq!(exit_code("Exit code 127\nzsh: command not found"), Some(127));
        assert_eq!(exit_code("fine"), None);
        assert_eq!(files_found("Grep", "Found 12 files\na\nb"), Some(12));
        assert_eq!(files_found("Grep", "No files found"), Some(0));
        assert_eq!(files_found("Grep", "12:let x = 1"), None);
        assert_eq!(files_found("Glob", "a.rs\nb.rs"), Some(2));
    }

    #[test]
    fn todo_write_replaces_the_checklist() {
        let mut session = Session::default();
        tool_call(
            &mut session,
            "t1",
            "TodoWrite",
            json!({ "todos": [
                { "content": "Write it", "status": "completed", "activeForm": "Writing it" },
                { "content": "Test it", "status": "in_progress", "activeForm": "Testing it" },
                { "content": "Ship it", "status": "pending" },
            ] }),
        );
        let got: Vec<(&str, &str, Progress)> = session.todos.iter().map(|t| (t.text.as_str(), t.active.as_str(), t.progress)).collect();
        assert_eq!(got, [("Write it", "Writing it", Progress::Done), ("Test it", "Testing it", Progress::Active), ("Ship it", "Ship it", Progress::Pending)]);
        tool_call(&mut session, "t2", "TodoWrite", json!({ "todos": [{ "content": "Only", "status": "pending" }] }));
        assert_eq!(session.todos.len(), 1);
    }

    #[test]
    fn task_tools_build_the_checklist_from_their_results() {
        let mut session = Session::default();
        tool_call(&mut session, "c1", "TaskCreate", json!({ "subject": "Entities", "description": "d", "activeForm": "Building entities" }));
        assert!(session.todos.is_empty(), "the number comes with the result");
        tool_result(&mut session, "c1", "Task #1 created successfully: Entities", false);
        tool_call(&mut session, "c2", "TaskCreate", json!({ "subject": "Maps" }));
        tool_result(&mut session, "c2", "Task #2 created successfully: Maps", false);
        tool_call(&mut session, "c3", "TaskCreate", json!({ "subject": "Refused" }));
        tool_result(&mut session, "c3", "no", true);
        tool_call(&mut session, "u1", "TaskUpdate", json!({ "taskId": "1", "status": "in_progress" }));
        tool_call(&mut session, "u2", "TaskUpdate", json!({ "taskId": "2", "status": "completed", "subject": "EF maps" }));
        let got: Vec<(&str, &str, Progress)> = session.todos.iter().map(|t| (t.id.as_str(), t.text.as_str(), t.progress)).collect();
        assert_eq!(got, [("1", "Entities", Progress::Active), ("2", "EF maps", Progress::Done)]);
        assert_eq!(session.todos[0].active, "Building entities");
        tool_call(&mut session, "u3", "TaskUpdate", json!({ "taskId": "1", "status": "deleted" }));
        assert_eq!(session.todos.len(), 1);
    }

    #[test]
    fn the_usage_answer_becomes_the_windows_credits_contributors_and_totals_the_view_shows() {
        let mut session = Session::default();
        session.hear(&serde_json::from_str(include_str!("../tests/usage.json")).unwrap());
        let usage = session.usage_view.expect("the answer to the usage view's request");
        let windows: Vec<(&str, u64)> = usage.windows.iter().map(|window| (window.name.as_str(), window.used)).collect();
        assert_eq!(windows, [("Current session", 3), ("Current week (all models)", 13), ("Current week (Fable)", 0)]);
        assert_eq!(usage.windows[1].resets, "2026-11-22T15:59:59.562716+00:00");
        assert_eq!(usage.plan.as_deref(), Some("max"));
        let credits = usage.credits.unwrap();
        assert_eq!((credits.spent, credits.limit, credits.currency.as_str(), credits.used), (11.74, Some(200.0), "EUR", Some(6)));
        let day = usage.day.unwrap();
        assert_eq!(day.tips[0], (70, "See what fills your context".into(), "run /context".into()));
        assert_eq!((day.skills, day.plugins, day.mcp_servers), (vec![("release-notes".into(), 4)], vec![("toolbox".into(), 11)], vec![("files".into(), 6)]));
        assert_eq!(usage.week.unwrap().skills, [("release-notes".to_owned(), 7)]);
        let totals = usage.totals;
        assert_eq!((totals.api_ms, totals.wall_ms, totals.added, totals.removed), (84_200, 612_000, 120, 31));
        // Summed over the models the session used.
        assert_eq!((totals.input, totals.output, totals.cache_read, totals.cache_write), (1500, 10_500, 410_000, 57_000));
        assert!((totals.cost - 1.2345).abs() < 1e-9);
    }

    #[test]
    fn a_usage_answer_without_credits_or_a_scan_shows_neither_and_an_error_says_why() {
        let answer = |id: &str, response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
        let mut session = Session::default();
        session.hear(&answer(USAGE_VIEW, json!({ "session": {}, "rate_limits": {
            "five_hour": { "utilization": 40, "resets_at": "2026-11-18T14:29:59+00:00" },
            "seven_day_opus": { "utilization": 7, "resets_at": "2026-11-19T10:59:59+00:00" },
            "extra_usage": { "is_enabled": false } }, "behaviors": null })));
        let usage = session.usage_view.take().unwrap();
        // No server-listed per-model windows: the fixed Opus one stands in.
        assert_eq!(usage.windows.iter().map(|window| window.name.as_str()).collect::<Vec<_>>(), ["Current session", "Current week (Opus)"]);
        assert!(usage.credits.is_none() && usage.day.is_none() && usage.week.is_none());
        session.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": USAGE_VIEW, "error": "Not signed in" } }));
        assert_eq!(session.view_error.as_deref(), Some("Not signed in"));
    }

    #[test]
    fn the_context_answer_becomes_its_categories_without_the_deferred_ones() {
        let answer = json!({ "type": "control_response", "response": { "subtype": "success", "request_id": CONTEXT_VIEW, "response": {
            "model": "claude-opus-5-5", "totalTokens": 41_000, "maxTokens": 200_000,
            "categories": [
                { "name": "System prompt", "tokens": 6000, "color": "x", "kind": "used" },
                { "name": "Messages", "tokens": 35_000, "color": "x", "kind": "used" },
                { "name": "Deferred tools", "tokens": 9000, "color": "x", "kind": "deferred", "isDeferred": true },
                { "name": "Free space", "tokens": 120_000, "color": "x", "kind": "free" },
                { "name": "Autocompact buffer", "tokens": 39_000, "color": "x", "kind": "buffer" } ] } } });
        let mut session = Session::default();
        session.hear(&answer);
        let context = session.context_view.unwrap();
        assert_eq!((context.used, context.most, context.model.as_str()), (41_000, 200_000, "claude-opus-5-5"));
        let names: Vec<&str> = context.categories.iter().map(|(name, ..)| name.as_str()).collect();
        assert_eq!(names, ["System prompt", "Messages", "Free space", "Autocompact buffer"]);
    }

    #[test]
    fn usage_cost_and_context_open_a_view_and_are_not_sent_as_messages() {
        for (text, panel, subtype) in [("/usage", Panel::Usage, "get_usage"), (" /cost\n", Panel::Usage, "get_usage"), ("/context", Panel::Context, "get_context_usage")] {
            let mut session = Session::default();
            let (opened, line) = session.view_command(text).expect(text);
            assert_eq!(opened, panel);
            // One control request, no message, and nothing in the conversation.
            assert_eq!(line.lines().count(), 1);
            let sent: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(sent["type"], "control_request");
            assert_eq!(sent["request"]["subtype"], subtype);
            assert!(sent["request"].get("skip_behaviors").is_none());
            assert!(session.items.is_empty() && !session.working);
        }
        let mut session = Session::default();
        assert!(session.view_command("/usage now").is_none());
        assert!(session.view_command("tell me about /usage").is_none());
    }

    #[test]
    fn a_click_on_a_side_panel_row_opens_the_dialog_with_one_read() {
        for (panel, subtype) in [(Panel::Usage, "get_usage"), (Panel::Context, "get_context_usage")] {
            let mut session = Session::default();
            session.view_error = Some("Not signed in".into());
            let line = session.open_view(panel);
            assert_eq!(line.lines().count(), 1);
            let sent: Value = serde_json::from_str(&line).unwrap();
            assert_eq!((sent["type"].as_str(), sent["request"]["subtype"].as_str()), (Some("control_request"), Some(subtype)));
            assert_eq!(session.view_error, None, "the last answer's error goes with the new read");
            assert!(session.items.is_empty() && !session.working);
        }
    }

    // As a rewound session's transcript has them, ids made up: what the rewind dropped stays in the file, and a
    // `last-prompt` line with `rewound` comes after it.
    const FIRST_USER: &str = r#"{"type":"user","uuid":"11111111-1111-4111-8111-111111111111","parentUuid":"b70db500-0000-4000-8000-000000000001","message":{"role":"user","content":"Say only: one"}}"#;
    const FIRST_REPLY: &str = r#"{"type":"assistant","uuid":"33469fd0-0000-4000-8000-000000000002","parentUuid":"bbc4a9ba-0000-4000-8000-000000000003","message":{"role":"assistant","content":[{"type":"text","text":"one"}]}}"#;
    const SECOND_USER: &str = r#"{"type":"user","uuid":"22222222-2222-4222-8222-222222222222","parentUuid":"9936c70a-0000-4000-8000-000000000004","message":{"role":"user","content":"Say only: two"}}"#;
    const SECOND_REPLY: &str = r#"{"type":"assistant","uuid":"f66a3fb3-0000-4000-8000-000000000005","parentUuid":"9aeba6c8-0000-4000-8000-000000000006","message":{"role":"assistant","content":[{"type":"text","text":"two"}]}}"#;
    const PLAIN_LAST_PROMPT: &str = r#"{"type":"last-prompt","lastPrompt":"Say only: two","leafUuid":"9aeba6c8-0000-4000-8000-000000000006","sessionId":"c05c0eb5-0000-4000-8000-000000000007"}"#;

    #[test]
    fn a_transcript_leaves_out_what_a_rewind_dropped() {
        let rewound = r#"{"type":"last-prompt","lastPrompt":"Say only: two","leafUuid":"33469fd0-0000-4000-8000-000000000002","explicit":true,"rewound":true,"cutUuid":"22222222-2222-4222-8222-222222222222","sessionId":"c05c0eb5-0000-4000-8000-000000000007"}"#;
        let mut session = heard(&[FIRST_USER, FIRST_REPLY, PLAIN_LAST_PROMPT, SECOND_USER, SECOND_REPLY, rewound, PLAIN_LAST_PROMPT].join("\n"));
        assert_eq!(session.items, [Item::User("Say only: one".into()), Item::Text("one".into())]);
        assert_eq!(session.prompts.len(), 1);
        // What is said after the rewind goes on from the reply it kept.
        let next = r#"{"type":"user","uuid":"44444444-4444-4444-8444-444444444444","parentUuid":"33469fd0-0000-4000-8000-000000000002","message":{"role":"user","content":"Say only: three"}}"#;
        session.hear(&serde_json::from_str(next).unwrap());
        assert_eq!(session.items, [Item::User("Say only: one".into()), Item::Text("one".into()), Item::User("Say only: three".into())]);
    }

    #[test]
    fn a_transcript_rewound_to_its_first_message_is_empty() {
        // Claude Code writes a null `leafUuid` when nothing is left.
        let rewound = r#"{"type":"last-prompt","lastPrompt":"Say only: two","leafUuid":null,"explicit":true,"rewound":true,"cutUuid":"11111111-1111-4111-8111-111111111111","sessionId":"c05c0eb5-0000-4000-8000-000000000007"}"#;
        let bare = r#"{"type":"last-prompt","lastPrompt":"Say only: two","sessionId":"c05c0eb5-0000-4000-8000-000000000007"}"#;
        let session = heard(&[FIRST_USER, FIRST_REPLY, SECOND_USER, SECOND_REPLY, rewound, bare].join("\n"));
        assert!(session.items.is_empty() && session.prompts.is_empty());
        // A last-prompt line that isn't a rewind drops nothing.
        assert_eq!(heard(&[FIRST_USER, FIRST_REPLY, PLAIN_LAST_PROMPT].join("\n")).items.len(), 2);
    }

    #[test]
    fn a_finished_turn_says_how_long_it_worked() {
        let mut session = Session::default();
        session.send("go", &[], 1, 1);
        // Fields as the Agent SDK's SDKResultMessage names them.
        session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 20_400, "num_turns": 3 }));
        // One that answers a background task along with another did no turns of its own.
        session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false, "duration_ms": 12, "num_turns": 0 }));
        // A failed turn says so instead.
        session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "duration_ms": 900, "num_turns": 1, "errors": ["Overloaded"] }));
        assert_eq!(session.items, [Item::User("go".into()), Item::Worked(20_400), Item::Error("Overloaded".into())]);
    }

    #[test]
    fn the_turn_counts_its_subagents_and_waits_on_them_once_nothing_else_runs() {
        let mut session = Session::default();
        session.send("survey", &[], 1, 1);
        tool_call(&mut session, "s1", "Task", json!({ "description": "Callers" }));
        tool_call(&mut session, "s2", "Agent", json!({ "description": "Rounding" }));
        tool_call(&mut session, "b1", "Bash", json!({ "command": "ls" }));
        assert_eq!((session.turn_subagents(), session.waiting_on_subagents()), ((2, 2), None), "it runs a command too");
        tool_result(&mut session, "b1", "a.txt", false);
        tool_result(&mut session, "s1", "Three callers", false);
        assert_eq!((session.turn_subagents(), session.waiting_on_subagents()), ((2, 1), Some(1)));
        session.thinking = true;
        assert_eq!(session.waiting_on_subagents(), None, "Claude thinks");
        // The next message starts a turn with none, even with one left running.
        session.thinking = false;
        session.send("next", &[], 1, 1);
        assert_eq!((session.turn_subagents(), session.waiting_on_subagents()), ((0, 0), None));
    }

    #[test]
    fn a_model_and_effort_picked_together_switch_only_what_differs() {
        let answer = |id: &str, response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } });
        let mut session = Session::default();
        session.hear(&answer(INITIALIZE, json!({ "models": [
            { "value": "opus", "displayName": "Opus 5.5", "supportedEffortLevels": ["high", "max"], "resolvedModel": "claude-opus-5-5" },
            { "value": "sonnet", "displayName": "Sonnet 5.5", "supportedEffortLevels": ["low", "high"], "resolvedModel": "claude-sonnet-5-5" },
            { "value": "haiku", "displayName": "Haiku 5.5", "resolvedModel": "claude-haiku-5-5" },
        ] })));
        session.hear(&answer(SETTINGS, json!({ "effective": { "model": "opus", "effortLevel": "high" } })));
        assert_eq!(session.pick("opus@high", 1), None);
        let requests = |lines: String| parsed(&lines).iter().map(|line| line["request"].clone()).collect::<Vec<_>>();
        let effort = |level: &str| json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": level } });
        assert_eq!(requests(session.pick("opus@max", 2).unwrap()), [effort("max")]);
        // Another model is switched to with the effort picked, even the one in use.
        session.pick("opus@high", 3);
        assert_eq!(requests(session.pick("sonnet@high", 4).unwrap()), [json!({ "subtype": "set_model", "model": "sonnet" }), effort("high")]);
        assert_eq!(requests(session.pick("haiku@", 5).unwrap()), [json!({ "subtype": "set_model", "model": "haiku" })]);
        assert_eq!(session.current_model().map(|model| model.name.as_str()), Some("Haiku 5.5"));
        assert_eq!(session.pick("haiku", 6), None, "not a pick");
    }

    /// Claude Code 2.1.327's answers to `get_settings` and `mcp_status`, with made-up plugins and servers.
    const PLUGINS: &str = include_str!("../tests/plugins.jsonl");

    fn with_plugins() -> Session {
        heard(PLUGINS)
    }

    #[test]
    fn plugins_plugin_and_mcp_open_the_dialog_ask_only_to_read_and_are_not_sent_as_messages() {
        for (text, show) in [("/plugins", Show::Plugins), (" /plugin\n", Show::Plugins), ("/mcp", Show::Mcp)] {
            let mut session = Session::default();
            let (opened, line) = session.dialog_command(text).expect(text);
            assert_eq!(opened, show);
            // One write of two requests that read, no message, and nothing in the conversation.
            let subtypes: Vec<Value> = parsed(&line).iter().map(|sent| sent["request"]["subtype"].clone()).collect();
            assert_eq!(subtypes, [json!("get_settings"), json!("mcp_status")]);
            assert!(parsed(&line).iter().all(|sent| sent["type"] == "control_request"));
            assert!(session.items.is_empty() && !session.working);
        }
        let mut session = Session::default();
        assert!(session.dialog_command("/plugins install toolbox").is_none());
        assert!(session.dialog_command("what does /mcp do").is_none());
    }

    #[test]
    fn the_settings_answer_lists_each_plugin_on_or_off_where_your_settings_decide_it() {
        let session = with_plugins();
        let list = session.plugins.list.as_ref().expect("the plugins");
        let shown: Vec<(&str, &str, bool, bool, &str)> = list.iter().map(|p| (p.name.as_str(), p.marketplace.as_str(), p.on, p.saved, p.scope)).collect();
        assert_eq!(
            shown,
            [
                // Only your organization's settings name it, so the chat can't turn it off.
                ("audit-log", "company-policy", true, true, "Managed"),
                // The project's settings come after yours and turn it on.
                ("code-search", "claude-plugins-official", true, true, "Project"),
                ("lint-guard", "team-tools", false, false, "User"),
                // On in your settings, off in this session's flags.
                ("pixel-perfect", "claude-plugins-official", false, true, "User"),
                ("release-notes", "team-tools", true, true, "User"),
                ("toolbox", "team-tools", true, true, "User"),
            ]
        );
        assert!(list[0].locked() && !list[1].locked());
    }

    #[test]
    fn the_status_answer_lists_each_server_with_its_status_and_a_plugins_under_it() {
        let session = with_plugins();
        let servers = session.plugins.servers.as_ref().expect("the servers");
        let files = &servers[0];
        assert_eq!((files.of_plugin(), files.short_name(), files.status.as_str(), files.tools), (Some(("toolbox", "files")), "files", "connected", Some(3)));
        let index = servers.iter().find(|server| server.short_name() == "index").unwrap();
        assert_eq!((index.status.as_str(), index.error.as_deref(), index.tools), ("failed", Some("Connection closed"), None));
        let own: Vec<(&str, &str, &str)> = servers.iter().filter(|server| server.of_plugin().is_none()).map(|s| (s.name.as_str(), s.status.as_str(), s.scope.as_str())).collect();
        assert_eq!(
            own,
            [
                ("notes", "connected", "user"),
                ("design-files", "disabled", "user"),
                ("tickets", "connected", "project"),
                ("claude.ai Calendar", "needs-auth", "claudeai"),
                ("claude.ai Drive", "pending", "claudeai"),
            ]
        );
    }

    #[test]
    fn a_plugins_skills_are_the_commands_named_after_it() {
        let commands = |names: &[&str]| names.iter().map(|name| Command { name: (*name).into(), description: String::new(), hint: String::new() }).collect::<Vec<_>>();
        let all = commands(&["toolbox:lint", "toolbox:format", "toolbox-extra:x", "review", "release-notes:write"]);
        assert_eq!([plugins::skills(&all, "toolbox"), plugins::skills(&all, "release-notes"), plugins::skills(&all, "lint-guard")], [2, 1, 0]);
    }

    #[test]
    fn a_plugin_toggle_is_one_flag_settings_request_with_every_plugin_this_chat_set_and_never_a_settings_file() {
        let mut session = with_plugins();
        let line = session.plugins.set_plugin("toolbox@team-tools", false, 7).expect("a toggle");
        assert_eq!(line.lines().count(), 1);
        let sent = sent(&line);
        assert_eq!(sent["request_id"], "plugin-toggle-7");
        // The flag settings take a second `enabledPlugins` in place of the first, so the one already there goes too.
        assert_eq!(
            sent["request"],
            json!({ "subtype": "apply_flag_settings", "settings": { "enabledPlugins": { "pixel-perfect@claude-plugins-official": false, "toolbox@team-tools": false } } })
        );
        let toolbox = session.plugins.list.as_ref().unwrap().iter().find(|p| p.name == "toolbox").unwrap();
        assert!(!toolbox.on && toolbox.saved && session.plugins.unapplied);
        // Off already, so nothing to send; and your organization's plugin can't be turned off here.
        assert_eq!(session.plugins.set_plugin("toolbox@team-tools", false, 8), None);
        assert_eq!(session.plugins.set_plugin("audit-log@company-policy", false, 9), None);
        assert_eq!(session.plugins.set_plugin("unknown@nowhere", true, 10), None);
    }

    #[test]
    fn a_server_toggle_is_one_mcp_toggle_request_by_its_full_name() {
        let mut session = with_plugins();
        let line = session.plugins.set_server("plugin:toolbox:files", false, 3).expect("a toggle");
        assert_eq!(line.lines().count(), 1);
        assert_eq!(sent(&line)["request"], json!({ "subtype": "mcp_toggle", "serverName": "plugin:toolbox:files", "enabled": false }));
        assert_eq!(session.plugins.servers.as_ref().unwrap()[0].status, "disabled");
        assert_eq!(session.plugins.set_server("plugin:toolbox:files", false, 4), None, "off already");
        // Turned on, it connects anew, which only a new read shows.
        let on = session.plugins.set_server("design-files", true, 5).expect("a toggle");
        assert_eq!(sent(&on)["request"]["enabled"], true);
        assert!(!session.plugins.stale);
        session.hear(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "server-toggle-5" } }));
        assert!(session.plugins.stale);
    }

    #[test]
    fn a_refused_toggle_is_put_back_and_says_why() {
        let mut session = with_plugins();
        session.plugins.set_plugin("toolbox@team-tools", false, 1).unwrap();
        session.plugins.set_server("notes", false, 2).unwrap();
        let refused = |id: &str, why: &str| json!({ "type": "control_response", "response": { "subtype": "error", "request_id": id, "error": why } });
        session.hear(&refused("plugin-toggle-1", "apply_flag_settings failed"));
        let toolbox = session.plugins.list.as_ref().unwrap().iter().find(|p| p.name == "toolbox").unwrap();
        assert!(toolbox.on && !session.plugins.unapplied);
        session.hear(&refused("server-toggle-2", "Server not found: notes"));
        assert_eq!(session.plugins.servers.as_ref().unwrap()[3].status, "connected");
        assert_eq!(session.plugins.error.as_deref(), Some("Server not found: notes"));
        // The next toggle sends only what this chat set and Claude Code kept.
        let line = session.plugins.set_plugin("lint-guard@team-tools", true, 3).unwrap();
        assert_eq!(sent(&line)["request"]["settings"]["enabledPlugins"], json!({ "pixel-perfect@claude-plugins-official": false, "lint-guard@team-tools": true }));
    }

    #[test]
    fn a_reload_applies_the_toggles_and_brings_the_servers_and_commands_as_they_are_now() {
        let mut session = with_plugins();
        session.plugins.set_plugin("toolbox@team-tools", false, 1).unwrap();
        let reload = session.plugins.reload();
        assert_eq!(reload.lines().count(), 1);
        assert_eq!(sent(&reload)["request"], json!({ "subtype": "reload_plugins", "refuse_if_busy": true }));
        session.hear(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "plugins-reload", "response": {
            "commands": [{ "name": "release-notes:write", "description": "Write the release notes", "argumentHint": "" }],
            "agents": [], "plugins": [{ "name": "release-notes", "path": "/plugins/release-notes", "source": "release-notes@team-tools" }],
            "mcpServers": [{ "name": "notes", "status": "connected", "scope": "user", "source": "user" }], "error_count": 0 } } }));
        assert!(!session.plugins.unapplied);
        assert_eq!(session.commands.iter().map(|command| command.name.as_str()).collect::<Vec<_>>(), ["release-notes:write"]);
        assert_eq!(session.plugins.servers.as_ref().unwrap().len(), 1);
        // A busy session reloads nothing, and says so.
        session.plugins.reload();
        session.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": "plugins-reload", "error": "The session is busy" } }));
        assert_eq!(session.plugins.error.as_deref(), Some("The session is busy"));
        assert_eq!(session.commands.len(), 1);
    }

    /// What Claude Code 2.1.327 printed while two commands ran with `run_in_background`, one finishing on its own, with
    /// made-up ids and descriptions.
    fn live(tasks: &[(&str, &str)]) -> Value {
        let tasks: Vec<Value> = tasks.iter().map(|(id, description)| json!({ "task_id": id, "run_id": format!("run-{id}"), "task_type": "local_bash", "description": description })).collect();
        json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": tasks, "uuid": "7a0e5b52-0c1f-4d39-8f0b-2b3c4d5e6f70", "session_id": "s" })
    }

    fn started(id: &str, description: &str) -> Value {
        json!({ "type": "system", "subtype": "task_started", "task_id": id, "run_id": format!("run-{id}"), "tool_use_id": format!("toolu_{id}"),
            "description": description, "is_backgrounded": true, "task_type": "local_bash", "uuid": "8b1f6c63-1d20-4e4a-9a1c-3c4d5e6f7081", "session_id": "s" })
    }

    fn task_end(id: &str, updated: &str, notified: &str) -> [Value; 2] {
        [
            json!({ "type": "system", "subtype": "task_updated", "task_id": id, "run_id": format!("run-{id}"), "patch": { "status": updated, "end_time": 1_795_027_323_607u64 }, "uuid": "9c2a7d74-2e31-4f5b-8b2d-4d5e6f708192", "session_id": "s" }),
            json!({ "type": "system", "subtype": "task_notification", "task_id": id, "run_id": format!("run-{id}"), "tool_use_id": format!("toolu_{id}"), "status": notified,
                "output_file": format!("/tmp/claude/tasks/{id}.output"), "summary": "Background command \"Run the tests\" completed (exit code 0)", "uuid": "ad3b8e85-3f42-405c-9c3e-5e6f708192a3", "session_id": "s" }),
        ]
    }

    fn answered_with(id: &str, response: Value) -> Value {
        json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } })
    }

    #[test]
    fn background_tasks_follow_the_live_set_and_say_how_each_ended() {
        let mut session = Session::default();
        session.now = 1_000;
        session.hear(&live(&[("bk1", "Run the tests")]));
        session.hear(&started("bk1", "Run the tests"));
        session.now = 1_070;
        session.hear(&live(&[("bk1", "Run the tests"), ("bk2", "Watch the build")]));
        session.hear(&started("bk2", "Watch the build"));
        let running: Vec<(&str, &str, &str, u64)> = session.tasks.iter().map(|t| (t.id.as_str(), t.kind.as_str(), t.description.as_str(), t.started)).collect();
        assert_eq!(running, [("bk1", "local_bash", "Run the tests", 1_000), ("bk2", "local_bash", "Watch the build", 1_070)]);
        assert!(session.tasks.iter().all(|task| task.ended.is_none() && task.has_output()));
        // The first ends on its own: its update and notice come before the set it leaves.
        session.now = 6_000;
        for line in task_end("bk1", "completed", "completed") {
            session.hear(&line);
        }
        session.hear(&live(&[("bk2", "Watch the build")]));
        assert_eq!(session.tasks[0].ended.as_deref(), Some("completed"));
        assert_eq!((session.tasks[1].ended.as_deref(), session.tasks[1].started), (None, 1_070));
        // One that leaves the set with no notice has ended all the same.
        session.hear(&live(&[]));
        assert_eq!(session.tasks[1].ended.as_deref(), Some("ended"));
        // Listed again, it is a new run, timed from now.
        session.hear(&live(&[("bk2", "Watch the build")]));
        assert_eq!((session.tasks[1].ended.as_deref(), session.tasks[1].started), (None, 6_000));
        assert_eq!(session.tasks.len(), 2);
    }

    #[test]
    fn a_stop_click_is_one_stop_task_request_and_the_task_ends_as_claude_code_says() {
        let mut session = heard(&live(&[("bk1", "Sleep then say done")]).to_string());
        let line = session.stop_task("bk1").expect("a stop");
        assert_eq!(parsed(&line), [json!({ "type": "control_request", "request_id": "task-stop-bk1", "request": { "subtype": "stop_task", "task_id": "bk1" } })]);
        assert!(session.tasks[0].stopping);
        assert_eq!(session.stop_task("bk1"), None, "stopping already");
        assert_eq!(session.stop_task("nope"), None);
        // As Claude Code 2.1.327 answered and ended it.
        session.hear(&answered_with("task-stop-bk1", json!({})));
        for line in task_end("bk1", "killed", "stopped") {
            session.hear(&line);
        }
        session.hear(&live(&[]));
        assert_eq!((session.tasks[0].ended.as_deref(), session.tasks[0].stopping), (Some("stopped"), false));
        assert_eq!(session.stop_task("bk1"), None, "ended");
        // A refused stop lets you try again, and says why.
        let mut refused = heard(&live(&[("bk2", "Watch the build")]).to_string());
        refused.stop_task("bk2").unwrap();
        refused.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": "task-stop-bk2", "error": "No task found with ID: bk2" } }));
        assert_eq!((refused.tasks[0].stopping, refused.tasks[0].error.as_deref()), (false, Some("No task found with ID: bk2")));
        assert!(refused.stop_task("bk2").is_some());
    }

    #[test]
    fn opening_a_tasks_output_is_one_read_and_its_answer_shows_the_end_of_what_it_printed() {
        let mut session = heard(&live(&[("bk1", "Watch the build")]).to_string());
        let line = session.read_task("bk1").expect("a read");
        assert_eq!(parsed(&line), [json!({ "type": "control_request", "request_id": "task-output-bk1", "request": { "subtype": "get_task_output", "task_id": "bk1" } })]);
        // As Claude Code 2.1.327 answered while it ran, and once it had ended.
        session.hear(&answered_with("task-output-bk1", json!({ "output": "tock 1\ntock 2\ntock 3\n", "total_bytes": 21, "truncated": false })));
        assert_eq!(session.tasks[0].output, Some(Output { text: "tock 1\ntock 2\ntock 3\n".into(), bytes: 21, truncated: false }));
        session.hear(&answered_with("task-output-bk1", json!({ "output": "\n[killed]\n", "total_bytes": 10, "truncated": false })));
        assert_eq!(session.tasks[0].output.as_ref().map(|output| output.text.as_str()), Some("\n[killed]\n"));
        // A subagent sent to the background has no output Claude Code reads.
        let mut agent = heard(&json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": "a1", "task_type": "local_agent", "description": "Survey" }] }).to_string());
        assert_eq!(agent.read_task("a1"), None);
        assert_eq!(agent.read_task("nope"), None);
    }

    const ONE: &str = "11111111-1111-4111-8111-111111111111";

    /// Two messages of yours sent from the chat, the second at `stamp` 2, each answered.
    fn two_turns() -> (Session, String, String) {
        let mut session = Session::default();
        let first: Value = serde_json::from_str(session.send("Write notes.txt with one", &[], 1, 1).lines().next().unwrap()).unwrap();
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "OK" }] } }));
        session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false }));
        let second: Value = serde_json::from_str(session.send("Change one to two", &[], 2, 3).lines().next().unwrap()).unwrap();
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "Done" }] } }));
        session.hear(&json!({ "type": "result", "subtype": "success", "is_error": false }));
        let uuid = |sent: &Value| sent["uuid"].as_str().unwrap().to_owned();
        (session, uuid(&first), uuid(&second))
    }

    #[test]
    fn a_message_sent_while_claude_is_idle_goes_under_a_uuid_a_rewind_names() {
        let (session, first, second) = two_turns();
        assert_ne!(first, second);
        assert_eq!(first.len(), 36);
        let prompts: Vec<(usize, &str)> = session.prompts.iter().map(|prompt| (prompt.at, prompt.uuid.as_str())).collect();
        assert_eq!(prompts, [(0, first.as_str()), (2, second.as_str())]);
        // A queued one keeps its own once it is read.
        let mut queued = Session::default();
        queued.hear(&json!({ "type": "system", "subtype": "session_state_changed", "state": "running" }));
        queued.queue("and lint", Vec::new(), 5, 6);
        let uuid = queued.queued[0].uuid.clone();
        queued.hear(&json!({ "type": "user", "isReplay": true, "uuid": uuid, "message": { "role": "user", "content": "and lint" } }));
        assert_eq!(queued.prompts, [Prompt { at: 0, uuid, parent: None }]);
    }

    #[test]
    fn the_rewind_dialog_opens_with_a_dry_run_that_lists_what_restoring_the_code_changes() {
        let (mut session, first, second) = two_turns();
        let line = session.open_rewind(&second).expect("a dry run");
        assert_eq!(
            parsed(&line),
            [json!({ "type": "control_request", "request_id": format!("rewind-check-{second}"), "request": { "subtype": "rewind_files", "user_message_id": second, "dry_run": true } })]
        );
        let rewind = session.rewind.as_ref().unwrap();
        assert_eq!((rewind.text.as_str(), &rewind.check), ("Change one to two", &None));
        // Another message's dry run doesn't fill this one.
        session.hear(&answered_with(&format!("rewind-check-{first}"), json!({ "canRewind": false, "error": "No file checkpoint found for this message." })));
        assert_eq!(session.rewind.as_ref().unwrap().check, None);
        // As Claude Code 2.1.327 answered it, with a made-up path.
        session.hear(&answered_with(&format!("rewind-check-{second}"), json!({ "canRewind": true, "filesChanged": ["/work/shop/notes.txt"], "insertions": 1, "deletions": 1 })));
        assert_eq!(session.rewind.as_ref().unwrap().check, Some(Ok(Check { files: vec!["/work/shop/notes.txt".into()], added: 1, removed: 1 })));
        // Without checkpoints it says why, as Claude Code does.
        session.open_rewind(&first).unwrap();
        session.hear(&answered_with(&format!("rewind-check-{first}"), json!({ "canRewind": false, "error": "File rewinding is not enabled." })));
        assert_eq!(session.rewind.as_ref().unwrap().check, Some(Err("File rewinding is not enabled.".into())));
        assert_eq!(session.open_rewind(ONE), None, "not a message of this chat");
    }

    #[test]
    fn each_rewind_choice_is_one_write_of_what_it_restores() {
        let (mut session, first, second) = two_turns();
        session.open_rewind(&first).unwrap();
        // Code isn't restored before the dry run has listed what it changes.
        assert_eq!(session.restore(Restore::Code), None);
        assert_eq!(session.restore(Restore::Both), None);
        session.hear(&answered_with(&format!("rewind-check-{first}"), json!({ "canRewind": true, "filesChanged": ["/work/shop/notes.txt"], "insertions": 0, "deletions": 1 })));
        let files = json!({ "type": "control_request", "request_id": "rewind-files", "request": { "subtype": "rewind_files", "user_message_id": first } });
        let conversation = json!({ "type": "control_request", "request_id": "rewind-conversation",
            "request": { "subtype": "rewind_conversation", "target_message_uuid": first, "last_seen_user_message_uuid": second } });
        assert_eq!(parsed(&session.restore(Restore::Both).unwrap()), [files.clone(), conversation.clone()]);
        assert_eq!(session.restore(Restore::Conversation), None, "waiting on the first choice");
        for (how, sent) in [(Restore::Code, vec![files]), (Restore::Conversation, vec![conversation])] {
            session.rewind.as_mut().unwrap().waiting.clear();
            assert_eq!(parsed(&session.restore(how).unwrap()), sent);
        }
        // With no files changed there is no code to restore; the conversation still can be.
        let (mut quiet, first, _) = two_turns();
        quiet.open_rewind(&first).unwrap();
        quiet.hear(&answered_with(&format!("rewind-check-{first}"), json!({ "canRewind": true, "filesChanged": [], "insertions": 0, "deletions": 0 })));
        assert_eq!(quiet.restore(Restore::Code), None);
        assert!(quiet.restore(Restore::Conversation).is_some());
    }

    #[test]
    fn a_restored_conversation_drops_the_message_and_after_and_gives_it_back_to_send_again() {
        let (mut session, _, second) = two_turns();
        session.open_rewind(&second).unwrap();
        session.hear(&answered_with(&format!("rewind-check-{second}"), json!({ "canRewind": true, "filesChanged": ["/work/shop/notes.txt"], "insertions": 1, "deletions": 1 })));
        session.restore(Restore::Both).unwrap();
        // As Claude Code 2.1.327 answered the two, in the order sent.
        session.hear(&answered_with("rewind-files", json!({ "canRewind": true, "skippedLinks": 0 })));
        assert_eq!(session.rewind.as_ref().unwrap().waiting, [REWIND_CONVERSATION]);
        session.hear(&answered_with("rewind-conversation", json!({ "rewound": true, "targetMessageUuid": second,
            "prefillText": "Change one to two", "precedingAssistantUuid": "5cc4bf2b-6279-4085-9a7b-75fa1de128b9" })));
        assert_eq!(session.items, [Item::User("Write notes.txt with one".into()), Item::Text("OK".into())]);
        assert_eq!(session.prompts.len(), 1);
        assert_eq!(session.restored.as_deref(), Some("Change one to two"));
        assert!(session.rewind.is_none(), "done, so closed");
    }

    #[test]
    fn a_refused_restore_says_why_and_keeps_the_dialog_open() {
        let (mut session, first, _) = two_turns();
        session.open_rewind(&first).unwrap();
        session.restore(Restore::Conversation).unwrap();
        // As Claude Code 2.1.327 refused one that named no message as seen.
        session.hear(&answered_with("rewind-conversation", json!({ "rewound": false, "prefillText": null, "precedingAssistantUuid": null, "error": "stale target", "reason": "stale_target" })));
        let rewind = session.rewind.as_ref().unwrap();
        assert_eq!((rewind.error.as_deref(), rewind.waiting.is_empty()), (Some("Couldn't restore the conversation: stale target"), true));
        assert_eq!(session.items.len(), 4);
        assert_eq!(session.restored, None);
        // Closed before the answer, a failure shows in the conversation.
        session.rewind = None;
        session.hear(&json!({ "type": "control_response", "response": { "subtype": "error", "request_id": "rewind-files", "error": "Failed to rewind: EACCES" } }));
        assert_eq!(session.items.last(), Some(&Item::Error("Couldn't restore the code: Failed to rewind: EACCES".into())));
    }

    #[test]
    fn a_transcript_read_again_leaves_out_what_a_rewind_dropped() {
        // Entries as Claude Code 2.1.327 wrote them, with made-up uuids: the second message was rewound, and the next one
        // follows the entry it followed, which the chat can't read.
        let entry = |kind: &str, uuid: &str, parent: &str, content: Value| {
            json!({ "parentUuid": parent, "isSidechain": false, "type": kind, "message": { "role": kind, "content": content }, "uuid": uuid, "timestamp": "2026-11-18T18:42:42.764Z" })
        };
        let said = |text: &str| json!([{ "type": "text", "text": text }]);
        let mut session = Session::default();
        for line in [
            entry("user", "u1", "a0", json!("Write notes.txt with one")),
            entry("assistant", "r1", "a1", said("OK")),
            entry("user", "u2", "s1", json!("Start two commands in the background")),
            entry("assistant", "r2", "a2", said("Both run")),
            entry("user", "u3", "s1", json!("List my messages")),
            entry("assistant", "r3", "a3", said("One, and this one")),
        ] {
            session.hear(&line);
        }
        assert_eq!(
            session.items,
            [Item::User("Write notes.txt with one".into()), Item::Text("OK".into()), Item::User("List my messages".into()), Item::Text("One, and this one".into())]
        );
        let prompts: Vec<(usize, &str)> = session.prompts.iter().map(|prompt| (prompt.at, prompt.uuid.as_str())).collect();
        assert_eq!(prompts, [(0, "u1"), (2, "u3")]);
    }

    #[test]
    fn the_opening_input_has_claude_code_keep_the_checkpoints_a_code_rewind_restores() {
        let lines = parsed(&Session::opening(None));
        assert!(lines.contains(&json!({ "type": "control_request", "request_id": "checkpoints",
            "request": { "subtype": "apply_flag_settings", "settings": { "env": { "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING": "1" } } } })));
    }

    /// A Read of a small generated png as Claude Code 2.1.327 printed it: the tool result is an image block, and the line
    /// carries the picture a second time as `tool_use_result`.
    const IMAGE_TURN: &str = include_str!("../tests/image_turn.jsonl");

    fn tool_images(session: &Session) -> Vec<Vec<String>> {
        session.items.iter().filter_map(|item| if let Item::Tool(tool) = item { Some(tool.images.clone()) } else { None }).collect()
    }

    #[test]
    fn a_picture_a_tool_gives_back_shows_in_its_step_and_goes_to_wings_once() {
        images::reset();
        let session = heard(IMAGE_TURN);
        let shown = tool_images(&session);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].len(), 1);
        let Some(Item::Tool(read)) = session.items.first() else {
            panic!("{:?}", session.items)
        };
        // It printed no text, and the picture is the id of what Wings was given.
        assert_eq!(read.output, Some((String::new(), false)));
        let kept = images::pending();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, shown[0][0]);
        assert!(kept[0].1.starts_with("iVBORw0KGgo"));
        assert!(images::is_shown(&kept[0].0));
    }

    #[test]
    fn a_transcripts_pictures_come_as_ids_wings_registered() {
        images::reset();
        let session = heard(concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"registered","id":"tabc"}},{"type":"text","text":"look"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"c1","name":"Read","input":{"file_path":"/tmp/a.png"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"c1","type":"tool_result","content":[{"type":"image","source":{"type":"registered","id":"tdef"}},{"type":"image","source":{"type":"unavailable"}}]}]}}"#,
        ));
        assert_eq!(session.items[0], Item::UserImages("look".into(), vec!["tabc".into()]));
        assert_eq!(tool_images(&session), [["tdef".to_owned(), "gone".to_owned()]]);
        // Nothing is sent to Wings: it kept them as it read the transcript.
        assert!(images::pending().is_empty());
        assert!(images::is_shown("tabc") && images::is_shown("tdef") && !images::is_shown("gone"));
    }

    #[test]
    fn only_the_newest_pictures_show_and_an_oversize_one_is_not_sent() {
        images::reset();
        let mut session = Session::default();
        let pictures: Vec<(String, String)> = (0..images::SHOWN + 3).map(|_| ("image/png".to_owned(), "AAAA".to_owned())).collect();
        session.send("many", &pictures, 1, 1);
        let Some(Item::UserImages(_, ids)) = session.items.first() else {
            panic!("{:?}", session.items)
        };
        assert_eq!(ids.len(), images::SHOWN + 3);
        let showing: Vec<bool> = ids.iter().map(|id| images::is_shown(id)).collect();
        assert_eq!(showing.iter().filter(|shown| **shown).count(), images::SHOWN);
        assert!(!showing[0] && showing[ids.len() - 1]);
        // Beyond what Wings keeps, the picture isn't copied into the page's queue at all.
        images::reset();
        let big = "A".repeat(5 * 1024 * 1024 / 3 * 4 + 8);
        assert_eq!(images::keep(&big), "gone");
        assert!(images::pending().is_empty());
    }

    #[test]
    fn a_queued_message_shows_its_pictures_once_claude_code_reads_it() {
        images::reset();
        let mut session = Session::default();
        session.queue("look", vec![("image/gif".into(), "CCCC".into())], 1, 1);
        assert!(images::pending().is_empty());
        let uuid = session.queued[0].uuid.clone();
        session.hear(&json!({ "type": "user", "isReplay": true, "uuid": uuid, "message": { "role": "user", "content": "look" } }));
        let Some(Item::UserImages(text, ids)) = session.items.first() else {
            panic!("{:?}", session.items)
        };
        assert_eq!((text.as_str(), ids.len()), ("look", 1));
        assert_eq!(images::pending(), [(ids[0].clone(), "CCCC".to_owned())]);
    }

    #[test]
    fn a_tool_result_shows_at_most_a_few_pictures() {
        images::reset();
        let shot = json!({ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } });
        let session = heard(&[
            json!({ "type": "assistant", "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": "c1", "name": "mcp__shots__all", "input": {} }] } }),
            json!({ "type": "user", "message": { "role": "user", "content": [{ "tool_use_id": "c1", "type": "tool_result", "content": vec![shot; 10] }] } }),
        ].map(|line| line.to_string()).join("\n"));
        assert_eq!(tool_images(&session)[0].len(), images::PER_RESULT);
        assert_eq!(images::pending().len(), images::PER_RESULT);
    }

    #[test]
    fn the_opening_input_asks_for_the_text_of_thinking_and_leaves_the_budget_alone() {
        let lines = parsed(&Session::opening(None));
        // Probed with Claude Code 2.1.327: with this alone the thinking blocks carry a summary, and the request is accepted.
        let asked = lines.iter().find(|line| line["request"]["subtype"] == "set_max_thinking_tokens").expect("a thinking request");
        assert_eq!(asked["request"], json!({ "subtype": "set_max_thinking_tokens", "thinking_display": "summarized" }));
    }

    #[test]
    fn a_thought_is_kept_as_its_summary_with_how_long_it_took_and_one_without_text_is_left_out() {
        let mut session = Session::default();
        session.now = 1_000;
        session.hear(&json!({ "type": "stream_event", "event": { "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "", "signature": "" } } }));
        assert!(session.thinking);
        session.now = 13_000;
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "thinking", "thinking": "I'm checking the riddle.\n\n", "signature": "sig" }] } }));
        assert_eq!(session.items, [Item::Thought("I'm checking the riddle.".into(), 12_000)]);
        // Without a summary asked for, Claude Code sends the block with an empty text.
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "thinking", "thinking": "", "signature": "sig" }] } }));
        assert_eq!(session.items.len(), 1);
    }

    // What Claude Code 2.1.327 printed, which has no `session_state_changed`: a turn starts with a `status` of `requesting` and ends
    // with its `result`, and `command_lifecycle` follows each message of yours.
    fn turn_start() -> Value {
        json!({ "type": "system", "subtype": "status", "status": "requesting", "session_id": "s" })
    }

    fn turn_end(answered: &[&String], origin: Option<&str>) -> Value {
        json!({ "type": "result", "subtype": "success", "is_error": false, "num_turns": 1, "user_message_uuids": answered, "origin": origin.map(|kind| json!({ "kind": kind })) })
    }

    fn lifecycle(uuid: &str, state: &str) -> Value {
        json!({ "type": "command_lifecycle", "command_uuid": uuid, "state": state, "session_id": "s" })
    }

    fn thinks() -> Value {
        json!({ "type": "assistant", "message": { "content": [{ "type": "thinking", "thinking": "Reading the request.", "signature": "sig" }] } })
    }

    #[test]
    fn a_turn_a_finished_task_starts_on_its_own_is_working_from_its_first_line_to_its_result() {
        // No message of yours began it, as when a background command ends: its notice, then `init`, thinking, one command.
        let mut session = Session::default();
        session.now = 1_000;
        session.hear(&live(&[("bk1", "Run the tests")]));
        session.hear(&started("bk1", "Run the tests"));
        for line in task_end("bk1", "completed", "completed") {
            session.hear(&line);
        }
        session.hear(&live(&[]));
        assert!(!session.working);
        session.hear(&turn_start());
        assert!(session.working);
        session.hear(&thinks());
        session.hear(&json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "toolu_1", "name": "Bash", "input": { "command": "ls" } }] } }));
        assert!(session.working, "while it thinks and runs a command");
        session.hear(&json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "a" }] } }));
        assert!(session.working);
        session.hear(&turn_end(&[], Some("task-notification")));
        assert!(!session.working);
    }

    #[test]
    fn a_long_command_that_goes_on_in_the_background_leaves_the_turn_ended_and_its_end_starts_another() {
        let mut session = Session::default();
        session.send("Run the slow job", &[], 1, 1);
        let sent = uuid(1, 1);
        session.hear(&lifecycle(&sent, "queued"));
        session.hear(&lifecycle(&sent, "started"));
        session.hear(&turn_start());
        session.hear(&live(&[("bk1", "Slow job")]));
        session.hear(&started("bk1", "Slow job"));
        assert!(session.working);
        session.hear(&turn_end(&[&sent], None));
        session.hear(&lifecycle(&sent, "completed"));
        assert!(!session.working, "the turn is over while the job runs on");
        for line in task_end("bk1", "completed", "completed") {
            session.hear(&line);
        }
        session.hear(&turn_start());
        assert!(session.working);
        session.hear(&turn_end(&[], Some("task-notification")));
        assert!(!session.working);
    }

    #[test]
    fn a_message_sent_while_claude_works_keeps_it_working_across_the_turn_that_read_it() {
        let mut session = Session::default();
        session.send("Run sleep 8", &[], 1, 1);
        let (first, next) = (uuid(1, 1), uuid(2, 2));
        session.hear(&lifecycle(&first, "queued"));
        session.hear(&lifecycle(&first, "started"));
        session.hear(&turn_start());
        session.queue("Also say banana", Vec::new(), 2, 2);
        session.hear(&lifecycle(&next, "queued"));
        // Claude Code stops the command for it: the first turn ends with its result, and the next starts at once.
        session.hear(&turn_end(&[&first], None));
        assert!(session.working, "its message is next");
        session.hear(&lifecycle(&first, "cancelled"));
        session.hear(&lifecycle(&next, "started"));
        session.hear(&turn_start());
        session.hear(&json!({ "type": "user", "isReplay": true, "uuid": next, "message": { "role": "user", "content": "Also say banana" } }));
        assert!(session.working);
        assert_eq!(session.items.last(), Some(&Item::User("Also say banana".into())));
        session.hear(&thinks());
        assert!(session.working);
        session.hear(&turn_end(&[&next], None));
        session.hear(&lifecycle(&next, "completed"));
        assert!(!session.working);
    }

    #[test]
    fn a_finished_message_is_not_waited_for_when_its_completed_line_never_comes() {
        let mut session = Session::default();
        session.send("hi", &[], 1, 1);
        let sent = uuid(1, 1);
        session.hear(&lifecycle(&sent, "queued"));
        session.hear(&turn_start());
        session.hear(&turn_end(&[&sent], None));
        assert!(!session.working);
        // The process ending ends it too.
        session.hear(&turn_start());
        session.stopped_running();
        assert!(!session.working);
    }

    fn agent_live(id: &str, description: &str) -> [Value; 2] {
        [
            json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [{ "task_id": id, "run_id": format!("run-{id}"), "task_type": "local_agent", "subagent_type": "general-purpose", "description": description }] }),
            json!({ "type": "system", "subtype": "task_started", "task_id": id, "run_id": format!("run-{id}"), "tool_use_id": format!("toolu_{id}"), "description": description,
                "subagent_type": "general-purpose", "is_backgrounded": true, "task_type": "local_agent" }),
        ]
    }

    #[test]
    fn a_background_agent_or_command_that_ends_makes_a_line_with_its_time_and_a_note_for_when_you_are_away() {
        let mut session = Session::default();
        session.now = 10_000;
        for line in agent_live("ag1", "Survey the parser") {
            session.hear(&line);
        }
        // The live set holds both now.
        let mut both = agent_live("ag1", "Survey the parser")[0].clone();
        both["tasks"].as_array_mut().unwrap().push(live(&[("bk1", "Run the tests")])["tasks"][0].clone());
        session.hear(&both);
        session.hear(&started("bk1", "Run the tests"));
        assert_eq!(session.tasks[0].call, "toolu_ag1");
        session.now = 10_000 + 452_000;
        session.hear(&json!({ "type": "system", "subtype": "task_notification", "task_id": "ag1", "run_id": "run-ag1", "tool_use_id": "toolu_ag1", "status": "completed",
            "output_file": "/tmp/claude/tasks/ag1.output", "summary": "Found two callers.\nBoth are tests.", "summary_is_report": true }));
        let Some(Item::Finished(done)) = session.items.last() else { panic!("a finished line") };
        assert_eq!((done.kind, done.description.as_str(), done.took, done.call.as_str()), ("Agent", "Survey the parser", Some(452_000), "toolu_ag1"));
        assert_eq!(done.title(), "Agent “Survey the parser” finished");
        assert_eq!(done.report, "Found two callers.\nBoth are tests.");
        assert_eq!(session.alerts, [("Agent “Survey the parser” finished".to_owned(), "Found two callers.".to_owned())]);
        // A command that fails says so, with Claude Code's words under it.
        session.now = 20_000;
        session.hear(&json!({ "type": "system", "subtype": "task_notification", "task_id": "bk1", "tool_use_id": "toolu_bk1", "status": "failed",
            "summary": "Background command \"Run the tests\" failed (exit code 1)" }));
        let Some(Item::Finished(done)) = session.items.last() else { panic!("a finished line") };
        assert_eq!((done.kind, done.failed(), done.title().as_str()), ("Command", true, "Command “Run the tests” failed"));
        assert_eq!(done.report, "Background command \"Run the tests\" failed (exit code 1)");
        assert_eq!(session.tasks[1].ended_at, 20_000);
        // A command that ran in the foreground sends a notice too, but was never one of the background tasks.
        let before = session.items.len();
        session.hear(&json!({ "type": "system", "subtype": "task_notification", "task_id": "fg1", "tool_use_id": "toolu_fg1", "status": "completed", "summary": "Ran it" }));
        assert_eq!(session.items.len(), before);
    }

    #[test]
    fn a_resumed_sessions_task_notices_are_the_lines_where_tasks_ended_and_never_your_messages() {
        let text = |body: &str| json!({ "type": "user", "origin": { "kind": "task-notification" }, "message": { "role": "user", "content": body } }).to_string();
        let agent = "<task-notification>\n<task-id>ag1</task-id>\n<tool-use-id>toolu_ag1</tool-use-id>\n<output-file>/tmp/claude/tasks/ag1.output</output-file>\n<status>completed</status>\n<summary>Agent \"Survey the parser\" finished</summary>\n<note>A task-notification fires each time this agent stops.</note>\n<result>Found two callers.</result>\n<usage><subagent_tokens>89885</subagent_tokens><tool_uses>10</tool_uses><duration_ms>75250</duration_ms></usage>\n</task-notification>";
        let command = "<task-notification>\n<task-id>bk1</task-id>\n<tool-use-id>toolu_bk1</tool-use-id>\n<output-file>/tmp/claude/tasks/bk1.output</output-file>\n<status>completed</status>\n<summary>Background command \"Run the tests\" completed (exit code 0)</summary>\n</task-notification>";
        let event = "<task-notification>\n<task-id>mn1</task-id>\n<summary>Monitor event: \"Watch the build\"</summary>\n<event>build ended</event>\n</task-notification>";
        let session = heard(&[text(agent), text(command), text(event), text(&format!("{command}{agent}"))].join("\n"));
        let Item::Finished(first) = &session.items[0] else { panic!("an agent line") };
        assert_eq!((first.kind, first.description.as_str(), first.status.as_str(), first.took, first.report.as_str()), ("Agent", "Survey the parser", "completed", Some(75_250), "Found two callers."));
        let Item::Finished(second) = &session.items[1] else { panic!("a command line") };
        assert_eq!((second.kind, second.title().as_str(), second.took, second.report.as_str()), ("Command", "Command “Run the tests” finished", None, ""));
        // A notice with no status is a monitor's event, which stays a quiet note.
        assert!(matches!(&session.items[2], Item::Note(title, _) if title == "Monitor event: \"Watch the build\""));
        // One entry can hold several notices.
        assert_eq!(session.items.len(), 5);
        assert!(session.items.iter().all(|item| !matches!(item, Item::User(_))));
    }
}
