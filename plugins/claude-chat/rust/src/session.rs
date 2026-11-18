//! A Claude Code session as the chat shows it, built from the lines `claude -p --output-format stream-json` prints, and
//! from the entries of its transcript when an earlier session is resumed. The two share their `user` and `assistant`
//! shapes.

use serde_json::{Value, json};

/// The most of a tool's output the chat keeps. A whole file read or a long build log is more than anyone reads in a card.
const OUTPUT_KEPT: usize = 4000;
/// The most of a tool's input shown as its one-line summary.
const SUMMARY_KEPT: usize = 200;

#[derive(Debug, PartialEq)]
pub enum Item {
    /// What you sent.
    User(String),
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
}

#[derive(Debug, PartialEq)]
pub struct Ask {
    pub request: String,
    pub tool: String,
    pub summary: String,
    pub input: Value,
    /// Allowed or denied, once you have said.
    pub answer: Option<bool>,
}

/// A model Claude Code offers, as `initialize` lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// What `set_model` takes, like `opus`.
    pub value: String,
    pub name: String,
    /// The effort levels it takes, none for a model without effort.
    pub efforts: Vec<String>,
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
    pub effort: Option<String>,
    /// The permission mode, one of `MODES`' or another Claude Code has.
    pub mode: Option<String>,
    pub models: Vec<Model>,
    pub commands: Vec<Command>,
    /// The tokens in the context window after the last message, and how many it holds.
    pub context: Option<(u64, u64)>,
    /// A turn is running: from your message to its result.
    pub working: bool,
    /// What the session has cost so far, in US dollars.
    pub cost: f64,
    /// You asked Claude to stop, and the turn hasn't ended yet.
    stopping: bool,
}

impl Session {
    /// Takes in one line Claude Code printed, or one transcript entry.
    pub fn hear(&mut self, line: &Value) {
        // A subagent's own messages stay inside its tool call.
        if !line["parent_tool_use_id"].is_null() {
            return;
        }
        match line["type"].as_str().unwrap_or_default() {
            "system" if line["subtype"] == "init" => {
                self.id = line["session_id"].as_str().map(str::to_owned);
                self.model = line["model"].as_str().map(str::to_owned);
                self.mode = line["permissionMode"].as_str().map(str::to_owned).or(self.mode.take());
            }
            "control_response" => self.answered(&line["response"]),
            "stream_event" => self.stream(&line["event"]),
            "assistant" => self.assistant(&line["message"]),
            "user" if line["isMeta"] != true => self.user(&line["message"]["content"]),
            "control_request" if line["request"]["subtype"] == "can_use_tool" => {
                let request = &line["request"];
                let tool = request["tool_name"].as_str().unwrap_or("a tool").to_owned();
                self.items.push(Item::Ask(Ask {
                    request: line["request_id"].as_str().unwrap_or_default().to_owned(),
                    summary: summary(&tool, &request["input"]),
                    tool,
                    input: request["input"].clone(),
                    answer: None,
                }));
            }
            "result" => {
                self.working = false;
                self.live.clear();
                self.cost = line["total_cost_usd"].as_f64().unwrap_or(self.cost);
                // A stopped turn ends as an error, with Claude Code's diagnostics as its text.
                if std::mem::take(&mut self.stopping) {
                    self.items.push(Item::Stopped);
                } else if line["is_error"] == true || line["subtype"].as_str().is_some_and(|s| s != "success") {
                    let said = line["result"].as_str().map(str::to_owned).or_else(|| line["errors"].as_array().map(|e| texts(e).join("\n")));
                    self.items.push(Item::Error(said.filter(|s| !s.is_empty()).unwrap_or_else(|| "The turn ended with an error.".into())));
                }
            }
            _ => {}
        }
    }

    /// Claude Code's answer to one of the chat's requests.
    fn answered(&mut self, response: &Value) {
        let answer = &response["response"];
        match response["request_id"].as_str().unwrap_or_default() {
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
            CONTEXT => {
                if let (Some(used), Some(most)) = (answer["totalTokens"].as_u64(), answer["maxTokens"].as_u64()) {
                    self.context = Some((used, most));
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
                    self.items.push(Item::Tool(Tool {
                        id: block["id"].as_str().unwrap_or_default().to_owned(),
                        summary: summary(&name, &block["input"]),
                        name,
                        output: None,
                    }));
                }
                _ => {}
            }
        }
    }

    fn user(&mut self, content: &Value) {
        if let Some(text) = content.as_str() {
            return self.items.extend(said(text));
        }
        for block in content.as_array().into_iter().flatten() {
            match block["type"].as_str().unwrap_or_default() {
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
                }
                _ => {}
            }
        }
    }

    /// The line that sends `text` as your message, and the message shown at once.
    pub fn send(&mut self, text: &str) -> String {
        self.items.push(Item::User(text.to_owned()));
        self.working = true;
        line(&json!({ "type": "user", "message": { "role": "user", "content": text } }))
    }

    /// What the chat asks Claude Code as it starts: the models and commands it has, and how full the context is.
    pub fn opening() -> String {
        [request(INITIALIZE, json!({ "subtype": "initialize" })), request(CONTEXT, json!({ "subtype": "get_context_usage" }))].concat()
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

    /// The model the session runs on, among `models`: by its value, or by the full name Claude Code reports.
    pub fn current_model(&self) -> Option<&Model> {
        let model = self.model.as_deref()?;
        self.models.iter().find(|m| m.value == model).or_else(|| {
            let family = model.trim_start_matches("claude-").split('-').next()?;
            self.models.iter().find(|m| m.value == family)
        })
    }

    /// The line that asks Claude to stop the turn it is on, the `n`th time you have.
    pub fn interrupt(&mut self, n: u32) -> String {
        self.stopping = true;
        request(&format!("interrupt-{n}"), json!({ "subtype": "interrupt" }))
    }

    /// The line that answers the question `request`, which is then shown answered. None when there is no such question
    /// waiting.
    pub fn answer(&mut self, request: &str, allow: bool) -> Option<String> {
        let ask = self.items.iter_mut().find_map(|item| match item {
            Item::Ask(ask) if ask.request == request && ask.answer.is_none() => Some(ask),
            _ => None,
        })?;
        ask.answer = Some(allow);
        let response = if allow {
            json!({ "behavior": "allow", "updatedInput": ask.input })
        } else {
            json!({ "behavior": "deny", "message": "The user said no to this in the chat." })
        };
        Some(line(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": request, "response": response } })))
    }
}


fn line(message: &Value) -> String {
    format!("{message}\n")
}

/// A request of the chat's to Claude Code, answered under `id`.
fn request(id: &str, request: Value) -> String {
    line(&json!({ "type": "control_request", "request_id": id, "request": request }))
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
        session.send("How many files?");
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
                }),
                Item::Text("There are 3 files:\n\n- `err.txt`\n- `out.jsonl`\n- `probe-file.txt`".into()),
            ]
        );
        assert!(!session.working);
        assert!(session.live.is_empty());
        assert_eq!(session.cost, 0.00596854);
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
        let allow: Value = serde_json::from_str(&session.answer("r1", true).unwrap()).unwrap();
        assert_eq!(
            allow,
            json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "r1", "response": { "behavior": "allow", "updatedInput": { "command": "touch a\nb", "description": "Make a" } } } })
        );
        assert_eq!(session.answer("r1", false), None, "answered already");
        assert_eq!(session.answer("r2", true), None, "never asked");
        let mut denied = heard(r#"{"type":"control_request","request_id":"r3","request":{"subtype":"can_use_tool","tool_name":"Write","input":{}}}"#);
        let deny: Value = serde_json::from_str(&denied.answer("r3", false).unwrap()).unwrap();
        assert_eq!(deny["response"]["response"]["behavior"], "deny");
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
    fn a_failed_turn_says_why_and_ends() {
        let mut session = Session::default();
        session.send("hi");
        session.hear(&json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "errors": ["Credit balance is too low"] }));
        assert!(!session.working);
        assert_eq!(session.items[1], Item::Error("Credit balance is too low".into()));
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
        session.send("count to 400");
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
}
