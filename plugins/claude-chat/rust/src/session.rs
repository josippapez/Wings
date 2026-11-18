//! A Claude Code session as the chat shows it, built from the lines `claude -p --output-format stream-json` prints, and
//! from the entries of its transcript when an earlier session is resumed. The two share their `user` and `assistant`
//! shapes.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};
use wings_plugin::format::parse_time;

use crate::usage::{ContextDetail, Panel, Usage};

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
/// calls, and only the end of them is read in a side panel. Older ones go, as the page couldn't draw them all anyway.
pub const SUBAGENT_KEPT: usize = 200;

#[derive(Debug, PartialEq)]
pub enum Item {
    /// What you sent.
    User(String),
    /// What you sent with pictures: the text, which may be empty, and how many pictures. Their data stays out of the
    /// item, as it is large and only Claude Code needs it.
    UserImages(String, usize),
    /// What Claude wrote, as Markdown.
    Text(String),
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

impl Queued {
    /// The message as the conversation shows it once it is read.
    fn read(self) -> Item {
        sent_item(self.text, self.images.len())
    }
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
    /// A turn is running. Claude Code's own `session_state_changed` says so when it prints them, else it is from your
    /// message to its result. The result alone ends a turn, not the work: with a message of yours queued the next turn
    /// starts at once, and a background task can run on, while `idle` waits for all of that.
    pub working: bool,
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
}

impl Session {
    /// Takes in one line Claude Code printed, or one transcript entry.
    pub fn hear(&mut self, line: &Value) {
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
            "system" if line["subtype"] == "session_state_changed" => self.changed(line["state"].as_str().unwrap_or_default()),
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
                self.told_you = false;
                self.assistant(&line["message"]);
            }
            // Claude Code echoes each message of yours it reads when it runs with `--replay-user-messages`. The ones sent
            // while it was idle are on the page already.
            "user" if line["isReplay"] == true => self.echoed(line["uuid"].as_str().unwrap_or_default()),
            // What Claude Code adds in a turn on its own, like a skill's text once the Skill tool loads it (`isSynthetic` as it
            // prints it, `isMeta` in the transcript), isn't yours to see as a message.
            "user" if line["isMeta"] != true && line["isSynthetic"] != true => self.user(&line["message"]["content"]),
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
            // Claude Code no longer needs the answer: the turn was stopped, or another client answered.
            "control_cancel_request" => {
                if let Some(ask) = line["request_id"].as_str().and_then(|id| self.pending(id)) {
                    ask.answer = Some(false);
                }
            }
            "result" => {
                self.working = self.reports_state && self.working;
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
                } else if !std::mem::replace(&mut self.told_you, true) {
                    let last = self.items.iter().rev().find_map(|item| if let Item::Text(text) = item { Some(text) } else { None });
                    let first = last.and_then(|text| text.lines().find(|line| !line.trim().is_empty())).unwrap_or_default();
                    self.alerts.push(("Claude finished".into(), cut(first.trim(), SUMMARY_KEPT)));
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
                self.working = false;
                // Nothing will read what is still queued now, so it was read without an echo.
                let left = std::mem::take(&mut self.queued);
                self.items.extend(left.into_iter().map(Queued::read));
            }
            "running" | "requires_action" => self.working = true,
            _ => {}
        }
    }

    /// Claude Code read the message `uuid`: from the queue into the conversation.
    fn echoed(&mut self, uuid: &str) {
        if let Some(at) = self.queued.iter().position(|queued| queued.uuid == uuid) {
            self.items.push(self.queued.remove(at).read());
        }
    }

    /// Claude Code's answer to one of the chat's requests.
    fn answered(&mut self, response: &Value) {
        let answer = &response["response"];
        let id = response["request_id"].as_str().unwrap_or_default();
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
                    })
                    .collect();
                self.commands = answer["commands"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|command| Command { name: text(command, "name"), description: text(command, "description"), hint: text(command, "argumentHint") })
                    .collect();
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
                self.thinking = event["content_block"]["type"] == "thinking";
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
                "tool_use" => {
                    let name = block["name"].as_str().unwrap_or("Tool").to_owned();
                    let id = block["id"].as_str().unwrap_or_default().to_owned();
                    self.track(&name, &id, &block["input"]);
                    self.items.push(Item::Tool(Tool { id, summary: summary(&name, &block["input"]), detail: detail(&name, &block["input"]), name, output: None }));
                }
                _ => {}
            }
        }
    }

    fn user(&mut self, content: &Value) {
        if let Some(text) = content.as_str() {
            return self.items.extend(said(text));
        }
        let first = self.items.len();
        let mut images = 0;
        for block in content.as_array().into_iter().flatten() {
            match block["type"].as_str().unwrap_or_default() {
                "image" => images += 1,
                // What Claude Code writes in your name when a turn is stopped, which `Stopped` shows.
                "text" if block["text"].as_str().is_some_and(|text| text.starts_with("[Request interrupted by user")) => {}
                "text" => self.items.extend(said(block["text"].as_str().unwrap_or_default())),
                "tool_result" => {
                    let id = block["tool_use_id"].as_str().unwrap_or_default();
                    let output = match &block["content"] {
                        Value::String(text) => text.clone(),
                        Value::Array(parts) => texts(parts).join("\n"),
                        _ => String::new(),
                    };
                    let tool = self.items.iter_mut().rev().find_map(|item| match item {
                        Item::Tool(tool) if tool.id == id => Some(tool),
                        _ => None,
                    });
                    if let Some(tool) = tool {
                        tool.output = Some((cut(&output, OUTPUT_KEPT), block["is_error"] == true));
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
        if images > 0 {
            // The pictures belong to the text of that message, else they are a message of their own.
            match self.items[first..].iter().position(|item| matches!(item, Item::User(_))) {
                Some(at) => {
                    if let Item::User(text) = &mut self.items[first + at] {
                        let text = std::mem::take(text);
                        self.items[first + at] = Item::UserImages(text, images);
                    }
                }
                None => self.items.insert(first, Item::UserImages(String::new(), images)),
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
    /// shown at once.
    pub fn send(&mut self, text: &str, images: &[(String, String)]) -> String {
        self.items.push(sent_item(text.to_owned(), images.len()));
        self.working = true;
        self.suggestion = None;
        // The limits are asked again with each message: the chat may only write when you send something.
        line(&json!({ "type": "user", "message": { "role": "user", "content": content(text, images) } })) + &usage()
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

    /// A draft that is a command with a view of the chat's own (`/usage`, `/cost`, `/context`): which view, and the
    /// request line that fills it. It is never sent as a message. Claude Code answers a control request at once, even
    /// while a turn runs, so the view doesn't wait for the turn.
    pub fn view_command(&mut self, text: &str) -> Option<(Panel, String)> {
        let (panel, line) = match view_of(text)? {
            // Without `skip_behaviors`, so what fills the limits comes too.
            Panel::Usage => (Panel::Usage, request(USAGE_VIEW, json!({ "subtype": "get_usage" }))),
            Panel::Context => (Panel::Context, request(CONTEXT_VIEW, json!({ "subtype": "get_context_usage", "detail": "full" }))),
        };
        // What the view showed last stays until the new answer replaces it.
        self.view_error = None;
        Some((panel, line))
    }

    /// What the chat asks Claude Code as it starts: the models and commands it has, how full the context is, your
    /// settings and how much of your plan's limits is used, and, with a `mode`, to start in it.
    pub fn opening(mode: Option<&str>) -> String {
        let mode = mode.map(|mode| request(START_MODE, json!({ "subtype": "set_permission_mode", "mode": mode })));
        [
            request(INITIALIZE, json!({ "subtype": "initialize" })),
            request(CONTEXT, json!({ "subtype": "get_context_usage" })),
            request(SETTINGS, json!({ "subtype": "get_settings" })),
            usage(),
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
fn sent_item(text: String, images: usize) -> Item {
    if images == 0 { Item::User(text) } else { Item::UserImages(text, images) }
}

fn cancel(uuid: &str, n: u32) -> String {
    request(&format!("cancel-{n}"), json!({ "subtype": "cancel_async_message", "message_uuid": uuid }))
}

/// The view a draft opens, when the draft is exactly that command.
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
fn request(id: &str, request: Value) -> String {
    line(&json!({ "type": "control_request", "request_id": id, "request": request }))
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
        session.send("How many files?", &[]);
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

        session.send("run it", &[]);
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
    fn pictures_go_as_image_blocks_before_the_text_and_show_as_a_count() {
        let pictures = [("image/png".to_owned(), "AAAA".to_owned()), ("image/jpeg".to_owned(), "BBBB".to_owned())];
        let mut session = Session::default();
        let sent: Value = serde_json::from_str(session.send("what is this?", &pictures).lines().next().unwrap()).unwrap();
        assert_eq!(
            sent["message"]["content"],
            json!([
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } },
                { "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": "BBBB" } },
                { "type": "text", "text": "what is this?" },
            ])
        );
        // Without text there is no empty text block, which the API refuses.
        let alone: Value = serde_json::from_str(session.send("", &pictures[..1]).lines().next().unwrap()).unwrap();
        assert_eq!(alone["message"]["content"], json!([{ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } }]));
        assert_eq!(session.items, [Item::UserImages("what is this?".into(), 2), Item::UserImages(String::new(), 1)]);
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
    fn a_transcript_message_with_pictures_shows_how_many() {
        let session = heard(concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}},{"type":"text","text":"what is this?"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}}]}}"#,
        ));
        assert_eq!(session.items, [Item::UserImages("what is this?".into(), 1), Item::UserImages(String::new(), 1)]);
    }

    #[test]
    fn a_failed_turn_says_why_and_ends() {
        let mut session = Session::default();
        session.send("hi", &[]);
        session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "errors": ["Credit balance is too low"] }));
        assert!(!session.working);
        assert_eq!(session.items[1], Item::Error("Credit balance is too low".into()));
    }

    #[test]
    fn a_hit_limit_shows_at_once_in_the_limits_and_once_in_the_conversation() {
        let answer = |response: Value| json!({ "type": "control_response", "response": { "subtype": "success", "request_id": USAGE, "response": response } });
        let mut session = Session::default();
        session.send("go on", &[]);
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
        session.send("count to 400", &[]);
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
        session.send("count to 400", &[]);
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
        session.send("count to 400", &[]);
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
        session.send("count to 400", &[]);
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
        plain.send("hi", &[]);
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
        session.send("clean up", &[]);
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
        assert_eq!(asked.len(), 5);
        assert_eq!(asked[4]["request_id"], START_MODE);
        assert_eq!(asked[4]["request"], json!({ "subtype": "set_permission_mode", "mode": "auto" }));
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
        assert!(session.send("hi", &[]).contains(r#""request_id":"usage""#));
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
        session.send("go", &[]);
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
        session.send("go", &[]);
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
}
