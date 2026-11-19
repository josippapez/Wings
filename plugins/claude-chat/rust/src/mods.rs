//! Claude Code's mods in the chat, as Claude Code Desktop shows them: the chat attaches as a remote surface
//! (`ui_attach`), and Claude Code tells it what its plugins show. Status lines (`ui_status`), toasts (`ui_toast`) and log
//! lines (`ui_log`) come by themselves; the panes the plugins open come as a roster (`ui_panes`), and the chat asks for
//! each pane's tree, the band above the prompt, the hint under it and the mode labels (`ui_render`), again when Claude
//! Code says they changed (`ui_invalidate`) and after a press, a submit or a pick in one of them (`ui_press`, `ui_input`,
//! `ui_select`). Measured on Claude Code 2.1.328, which marks these messages internal: a Claude Code without them answers
//! the attach with an error, and the chat then asks nothing. What it doesn't send a host like the chat yet: a mod's
//! notifications, which it raises only for its own remote apps, and a rewrite of the spinner, which came back unchanged.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Value, json};

use crate::session::request;

/// The surface the chat draws as: Claude Code Desktop's elements, but `Client`, which runs a plugin's own code.
pub const SURFACE: &str = "desktop";
/// The chat's name among the clients attached to the session.
pub const CLIENT: &str = "wings";
/// The band over the message box, as Claude Code names its instance.
pub const BAND: &str = "above-prompt";
/// The hint line under the message box, and the mode labels of its footer: one instance each, named by the chat.
pub const HINT: &str = "prompt-hint";
pub const MODES: &str = "session-mode";
/// A mod's look, as Claude Code names its instance: its colours and what the conversation lies on.
pub const THEME: &str = "theme";
/// What the chat asks for once attached, beside the panes.
const SITES: [&str; 4] = [BAND, HINT, MODES, THEME];
/// The right-click menu's instance: its rows are asked for when it opens.
pub const MENU: &str = "context-menu";
/// The keys of a tool call's row and of its result among the trees: the call's id after these.
pub const USE: &str = "use:";
pub const RESULT: &str = "result:";
/// The keys of your messages and Claude's among the trees: the item's place and a hash of its text after these, so
/// another message at that place after a rewind is another key.
pub const YOURS: &str = "yours:";
pub const CLAUDES: &str = "claudes:";
/// The line under the conversation while Claude works, which a mod may reword or draw.
pub const SPINNER: &str = "spinner";
/// The keys of Claude's questions (its permission request's id after it) and of a folded run of tool calls (the first
/// call's place and how many calls it has, so a run that grows is asked for again).
pub const QUESTION: &str = "question:";
pub const GROUP: &str = "group:";
/// The longest text of a tool call's input or result a mod gets: a file's whole content isn't what a row reads.
pub const KEPT: usize = 4_000;
/// The attach's request, whose answer a page that takes the chat's Claude Code over hears again (`reattach.rs`).
pub const ATTACH: &str = "ui-attach";
const RENDER: &str = "ui-render-";
const PRESS: &str = "ui-press-";
const INPUT: &str = "ui-input-";
const SELECT: &str = "ui-select-";

#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    pub id: String,
    pub title: String,
    pub plugin: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub plugin: String,
    pub text: String,
    /// When it goes, in milliseconds since the epoch.
    pub until: u64,
}

/// What the plugins of Claude Code show in this chat.
#[derive(Debug, Default)]
pub struct Mods {
    /// Each plugin's status line, by plugin.
    pub status: BTreeMap<String, String>,
    pub toasts: Vec<Toast>,
    /// The panes open, in Claude Code's order.
    pub panes: Vec<Pane>,
    /// What each pane, by its id, and the band were last drawn as: a tree of elements.
    pub trees: HashMap<String, Value>,
    /// The hint's and the mode labels' props as the mods left them, where they kept Claude Code's own drawing.
    pub props: HashMap<String, Value>,
    /// What you typed in a mod's field and haven't submitted, by its instance and key.
    pub drafts: HashMap<String, String>,
    /// Each tool call's input and result as Claude Code printed them, long text cut, for the mods that draw its row.
    pub tool_inputs: HashMap<String, Value>,
    pub tool_results: HashMap<String, Value>,
    /// Claude Code's requests the chat answers, by request id: a plugin's read or fill of the message box.
    pub asks: Vec<(String, Value)>,
    /// What was asked of each tool row, by key, and whether its call had ended then.
    asked: HashMap<String, bool>,
    /// The right-click menu open, as Wings said where and on what (`x`, `y`, `link`, `code`).
    pub menu: Option<Value>,
    /// The look a mod set (`$.theme.set`): its `colors` and `backdrop`.
    pub theme: Option<Value>,
    /// Whether Claude Code took the attach, so it may be asked to draw.
    pub attached: bool,
    /// A pane Claude Code asked to show or focus that the chat hasn't opened in Wings' sidebar yet (`mod_pane.rs`).
    pub to_open: Option<String>,
    /// The last pane Claude Code said it shows and asked to focus, so each ask opens the sidebar once.
    shown: (Option<String>, Option<String>),
    /// What is to be asked for again.
    stale: BTreeSet<String>,
}

/// The request that attaches the chat as a surface, among those it opens with. A docked layout, so a pane a plugin opens
/// unasked is a sidebar, as the chat's side panel is. The chat answers a plugin's read and fill of the message box.
pub fn attach() -> String {
    let viewport = json!({ "columns": 100, "rows": 40, "isFullscreen": true });
    request(ATTACH, json!({ "subtype": "ui_attach", "surface": SURFACE, "client_id": CLIENT, "viewport": viewport, "answers": ["ui_prompt_read", "ui_prompt_fill"] }))
}

/// The request for a tool call's row (`ToolUse`) or its result (`ToolResult`), kept under `key`, with the props Claude
/// Code's own row reads.
pub fn render_row(key: &str, component: &str, call: &str, props: Value, n: u32) -> String {
    request(
        &format!("{RENDER}{key}-{n}"),
        json!({ "subtype": "ui_render", "surface": SURFACE, "client_id": CLIENT, "component": component, "instance_id": call, "props": props }),
    )
}

/// The request for the rows mods add to a right-click menu on what `props` names: a `link` or `code`.
pub fn render_menu(props: Value, n: u32) -> String {
    request(&format!("{RENDER}{MENU}-{n}"), json!({ "subtype": "ui_render", "surface": SURFACE, "client_id": CLIENT, "component": "ContextMenu", "instance_id": MENU, "props": props }))
}

/// The answer to Claude Code's request `id`.
pub fn answer(id: &str, response: Value) -> String {
    format!("{}\n", json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } }))
}

/// `value` with each text longer than `KEPT` cut to it.
pub fn trimmed(value: &Value) -> Value {
    match value {
        Value::String(text) if text.len() > KEPT => Value::String(text.chars().take(KEPT).collect()),
        Value::Array(list) => Value::Array(list.iter().map(trimmed).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(key, value)| (key.clone(), trimmed(value))).collect()),
        other => other.clone(),
    }
}

/// The request for the tree of `instance`, a pane's id or one of `SITES`, numbered `n`. The hint and the mode labels go
/// with Claude Code's own props for them, which the chat draws none of, for a mod to add to.
pub fn render(instance: &str, n: u32) -> String {
    let (component, props) = match instance {
        BAND => ("AbovePrompt", json!({})),
        HINT => ("PromptHint", json!({ "isDraft": false, "isWorking": false, "hint": "" })),
        MODES => ("SessionMode", json!({ "modes": [] })),
        THEME => ("Theme", json!({})),
        _ => ("Pane", json!({})),
    };
    request(
        &format!("{RENDER}{instance}-{n}"),
        json!({ "subtype": "ui_render", "surface": SURFACE, "client_id": CLIENT, "component": component, "instance_id": instance, "props": props }),
    )
}

/// The request that submits what you typed in a mod's field, numbered `n`, from its `press` and its key.
pub fn input(instance: &str, press: &Value, key: &str, value: &str, n: u32) -> Option<String> {
    let (plugin, handle) = (press["plugin"].as_str()?, press["handle"].as_u64()?);
    let body = json!({ "subtype": "ui_input", "plugin": plugin, "handle": handle, "kind": "submit", "value": value, "key": key, "surface": SURFACE, "client_id": CLIENT });
    Some(request(&format!("{INPUT}{instance}-{n}"), body))
}

/// The request that picks `value` in a mod's dropdown, numbered `n`.
pub fn select(instance: &str, press: &Value, key: &str, value: &str, n: u32) -> Option<String> {
    let (plugin, handle) = (press["plugin"].as_str()?, press["handle"].as_u64()?);
    let body = json!({ "subtype": "ui_select", "plugin": plugin, "handle": handle, "value": value, "key": key, "surface": SURFACE, "client_id": CLIENT });
    Some(request(&format!("{SELECT}{instance}-{n}"), body))
}

/// The request that presses a button of `instance`'s tree, numbered `n`, from its `press` and its key.
pub fn press(instance: &str, press: &Value, key: Option<&str>, n: u32) -> Option<String> {
    let (plugin, handle) = (press["plugin"].as_str()?, press["handle"].as_u64()?);
    let mut body = json!({ "subtype": "ui_press", "plugin": plugin, "handle": handle, "surface": SURFACE, "client_id": CLIENT, "press_id": format!("p{n}") });
    if let Some(key) = key {
        body["key"] = json!(key);
    }
    Some(request(&format!("{PRESS}{instance}-{n}"), body))
}

impl Mods {
    /// Takes in a line Claude Code printed about its plugins' UI, and says whether it was one. A log line becomes `logged`,
    /// for the conversation to show.
    pub fn hear(&mut self, line: &Value, now: u64, logged: &mut Option<String>) -> bool {
        if line["type"] == "control_request" && matches!(line["request"]["subtype"].as_str(), Some("ui_prompt_read" | "ui_prompt_fill")) {
            self.asks.push((line["request_id"].as_str().unwrap_or_default().to_owned(), line["request"].clone()));
            return true;
        }
        if line["type"] != "system" {
            return false;
        }
        let plugin = line["plugin"].as_str().unwrap_or_default().to_owned();
        match line["subtype"].as_str().unwrap_or_default() {
            "ui_status" => match line["text"].as_str().filter(|text| !text.trim().is_empty()) {
                Some(text) => drop(self.status.insert(plugin, text.to_owned())),
                None => drop(self.status.remove(&plugin)),
            },
            "ui_toast" => {
                let shown = line["timeout_ms"].as_u64().unwrap_or(4_000).min(60_000);
                self.toasts.retain(|toast| toast.until > now);
                self.toasts.push(Toast { plugin, text: line["text"].as_str().unwrap_or_default().to_owned(), until: now + shown });
            }
            "ui_log" => *logged = line["text"].as_str().map(|text| format!("{plugin}: {text}")),
            "ui_panes" => {
                self.panes = line["panes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|pane| {
                        let id = pane["id"].as_str()?.to_owned();
                        let title = pane["title"].as_str().filter(|title| !title.is_empty()).unwrap_or(&id).to_owned();
                        Some(Pane { id, title, plugin: pane["plugin"].as_str().unwrap_or_default().to_owned() })
                    })
                    .collect();
                let open: Vec<String> = self.panes.iter().map(|pane| pane.id.clone()).collect();
                // A pane newly shown or asked to be focused, as `/feedback` opens its pane, opens once; the same ask
                // repeated with the next roster doesn't open it again.
                let id = |key: &str| line[key].as_str().filter(|id| open.iter().any(|open| open == id)).map(str::to_owned);
                let (shown, focus) = (id("shown_id"), id("focus_requested_id"));
                let asked = focus.clone().filter(|_| focus != self.shown.1).or_else(|| shown.clone().filter(|_| shown != self.shown.0));
                if asked.is_some() {
                    self.to_open = asked;
                }
                self.shown = (shown, focus);
                self.trees.retain(|instance, _| instance == BAND || open.contains(instance));
                self.stale.extend(open.into_iter().filter(|id| !self.trees.contains_key(id)));
            }
            "ui_invalidate" => match line["instances"].as_array() {
                Some(instances) => self.stale.extend(instances.iter().filter_map(|instance| {
                    let id = instance["instance_id"].as_str()?;
                    Some(match instance["component"].as_str() {
                        Some("ToolUse") => format!("{USE}{id}"),
                        Some("ToolResult") => format!("{RESULT}{id}"),
                        // Its scene changing (`backdrop`) comes with the look, which carries the backdrop too.
                        Some("Theme") => THEME.to_owned(),
                        _ => id.to_owned(),
                    })
                })),
                None => self.stale.extend(self.panes.iter().map(|pane| pane.id.clone()).chain(SITES.map(str::to_owned))),
            },
            _ => return false,
        }
        true
    }

    /// Takes in Claude Code's answer to a request of the mods', and says whether it was one.
    pub fn answered(&mut self, id: &str, response: &Value) -> bool {
        let ok = response["subtype"] == "success";
        if id == ATTACH {
            self.attached = ok;
            if ok {
                self.stale.extend(SITES.map(str::to_owned));
                self.stale.extend(self.panes.iter().map(|pane| pane.id.clone()));
            }
            return true;
        }
        if let Some(instance) = id.strip_prefix(RENDER).and_then(|rest| Some(rest.rsplit_once('-')?.0)) {
            if instance == THEME {
                // The look is in the props, with no rewrite: Claude Code draws no theme of its own.
                let props = &response["response"]["props"];
                self.theme = (ok && props["colors"].is_object()).then(|| props.clone());
                return true;
            }
            let tree = &response["response"]["tree"];
            // Claude Code's own component, which a hook left as it is (`{ type: "engine" }`), is nothing of a mod's but
            // the props a mod changed for it.
            if ok && tree.is_object() && tree["type"] != "engine" {
                self.trees.insert(instance.to_owned(), tree.clone());
            } else {
                self.trees.remove(instance);
            }
            match &response["response"]["props"] {
                props if ok && props.is_object() && response["response"]["rewritten"] == true => drop(self.props.insert(instance.to_owned(), props.clone())),
                _ => drop(self.props.remove(instance)),
            }
            return true;
        }
        if let Some(instance) = [PRESS, INPUT, SELECT].iter().find_map(|prefix| Some(id.strip_prefix(prefix)?.rsplit_once('-')?.0)) {
            // What the press changed shows once it is drawn again.
            self.stale.insert(instance.to_owned());
            return true;
        }
        false
    }

    /// The instances to ask Claude Code to draw again, once it took the attach.
    pub fn take_stale(&mut self) -> Vec<String> {
        if !self.attached {
            return Vec::new();
        }
        std::mem::take(&mut self.stale).into_iter().collect()
    }

    /// Whether the row under `key` is to be asked for: not asked yet, or asked while its call still ran and `ended` now.
    pub fn want_row(&mut self, key: &str, ended: bool) -> bool {
        if !self.attached || self.asked.get(key) == Some(&ended) || self.asked.get(key) == Some(&true) {
            return false;
        }
        self.asked.insert(key.to_owned(), ended);
        true
    }

    /// Keeps what a mod gets of a tool call: its input when it is called, its result when it ends.
    pub fn tool_called(&mut self, call: &str, input: &Value) {
        self.tool_inputs.insert(call.to_owned(), trimmed(input));
    }

    /// Keeps a tool call's result from the line that carries it (`tool_use_result`).
    pub fn tool_answered(&mut self, line: &Value) {
        if !line["tool_use_result"].is_object() {
            return;
        }
        let call = line["message"]["content"].as_array().into_iter().flatten().find_map(|block| block["tool_use_id"].as_str());
        if let Some(call) = call {
            self.tool_results.insert(call.to_owned(), trimmed(&line["tool_use_result"]));
        }
    }

    /// Asks for `key` again once attached, as when what Claude Code's own drawing of it reads changed.
    pub fn redraw(&mut self, key: &str) {
        self.stale.insert(key.to_owned());
    }

    /// Lets the toasts whose time is up go.
    pub fn expire(&mut self, now: u64) {
        self.toasts.retain(|toast| toast.until > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(subtype: &str, more: Value) -> Value {
        let mut line = json!({ "type": "system", "subtype": subtype, "uuid": "u", "session_id": "s" });
        line.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
        line
    }

    #[test]
    fn status_lines_toasts_and_log_lines_are_taken_in_as_claude_code_prints_them() {
        let mut mods = Mods::default();
        let mut logged = None;
        assert!(mods.hear(&system("ui_status", json!({ "plugin": "repo-docs", "text": "indexing docs 30% (3/10)" })), 0, &mut logged));
        assert_eq!(mods.status.get("repo-docs").map(String::as_str), Some("indexing docs 30% (3/10)"));
        assert!(mods.hear(&system("ui_status", json!({ "plugin": "repo-docs", "text": null })), 0, &mut logged));
        assert!(mods.status.is_empty());
        assert!(mods.hear(&system("ui_toast", json!({ "plugin": "repo-docs", "text": "repo-docs: doc index updated", "timeout_ms": 4000 })), 1_000, &mut logged));
        assert_eq!(mods.toasts[0].until, 5_000);
        mods.expire(5_000);
        assert!(mods.toasts.is_empty());
        assert!(mods.hear(&system("ui_log", json!({ "plugin": "probe", "text": "pressed" })), 0, &mut logged));
        assert_eq!(logged.as_deref(), Some("probe: pressed"));
        // Anything else is the conversation's.
        assert!(!mods.hear(&system("init", json!({})), 0, &mut logged));
        assert!(!mods.hear(&json!({ "type": "assistant" }), 0, &mut logged));
    }

    #[test]
    fn panes_and_the_band_are_asked_for_once_attached_and_again_when_they_change() {
        let mut mods = Mods::default();
        let mut logged = None;
        mods.hear(&system("ui_panes", json!({ "panes": [{ "id": "probe", "title": "Probe pane", "plugin": "wings-probe" }], "shown_id": "probe" })), 0, &mut logged);
        assert_eq!(mods.panes, [Pane { id: "probe".into(), title: "Probe pane".into(), plugin: "wings-probe".into() }]);
        // Nothing is asked of a Claude Code that hasn't taken the attach.
        assert!(mods.take_stale().is_empty());
        assert!(mods.answered(ATTACH, &json!({ "subtype": "success", "request_id": ATTACH, "response": { "surfaces": ["desktop"] } })));
        assert_eq!(mods.take_stale(), [BAND, "probe", HINT, MODES, THEME]);
        assert!(mods.take_stale().is_empty());
        let drawn = json!({ "type": "Box", "props": { "flexDirection": "column" }, "children": [{ "type": "Text", "children": ["Probe pane"] }] });
        let id = "ui-render-probe-3";
        assert!(mods.answered(id, &json!({ "subtype": "success", "request_id": id, "response": { "tree": drawn, "hooked": true } })));
        assert_eq!(mods.trees.get("probe"), Some(&drawn));
        // The band Claude Code draws itself is nothing to show.
        let band = "ui-render-above-prompt-4";
        assert!(mods.answered(band, &json!({ "subtype": "success", "request_id": band, "response": { "tree": { "type": "engine", "ref": 3 } } })));
        assert!(!mods.trees.contains_key(BAND));
        // Told one changed, the chat asks for that one; told nothing more, for all.
        mods.hear(&system("ui_invalidate", json!({ "event": "ui.render", "instances": [{ "surface": "desktop", "component": "Pane", "instance_id": "probe" }] })), 0, &mut logged);
        assert_eq!(mods.take_stale(), ["probe"]);
        mods.hear(&system("ui_invalidate", json!({ "event": "ui.render" })), 0, &mut logged);
        assert_eq!(mods.take_stale(), [BAND, "probe", HINT, MODES, THEME]);
        // A look's scene changing asks for the look, which carries it.
        mods.hear(&system("ui_invalidate", json!({ "event": "ui.render", "instances": [{ "surface": "desktop", "component": "Theme", "instance_id": "backdrop" }] })), 0, &mut logged);
        assert_eq!(mods.take_stale(), [THEME]);
        // A mod that adds to the hint or the mode labels keeps Claude Code's drawing and changes its props, as measured.
        let hint = "ui-render-prompt-hint-5";
        let rewritten = json!({ "isDraft": false, "isWorking": false, "hint": "", "tail": "probe tail" });
        mods.answered(hint, &json!({ "subtype": "success", "request_id": hint, "response": { "tree": { "type": "engine", "ref": 1 }, "props": rewritten, "rewritten": true } }));
        assert_eq!(mods.props.get(HINT), Some(&rewritten));
        mods.answered(hint, &json!({ "subtype": "success", "request_id": hint, "response": { "tree": { "type": "engine", "ref": 1 }, "props": {}, "rewritten": false } }));
        assert!(!mods.props.contains_key(HINT));
        // A pane that closed goes with its tree.
        mods.hear(&system("ui_panes", json!({ "panes": [] })), 0, &mut logged);
        assert!(mods.panes.is_empty() && mods.trees.is_empty());
    }

    #[test]
    fn a_pane_claude_code_shows_or_focuses_is_to_open_once_per_ask() {
        let mut mods = Mods::default();
        let mut logged = None;
        let panes = json!([{ "id": "probe", "title": "Deploys", "plugin": "deploy-watch" }, { "id": "feedback", "title": "Feedback", "plugin": "feedback" }]);
        mods.hear(&system("ui_panes", json!({ "panes": panes, "shown_id": "probe", "focused_id": null, "focus_requested_id": null })), 0, &mut logged);
        assert_eq!(mods.to_open.take().as_deref(), Some("probe"));
        // The same roster again asks nothing.
        mods.hear(&system("ui_panes", json!({ "panes": panes, "shown_id": "probe", "focused_id": null, "focus_requested_id": null })), 0, &mut logged);
        assert_eq!(mods.to_open, None);
        // `/feedback` asks to focus its pane: once, though the ask stays in the rosters after.
        for _ in 0..2 {
            mods.hear(&system("ui_panes", json!({ "panes": panes, "shown_id": "probe", "focused_id": null, "focus_requested_id": "feedback" })), 0, &mut logged);
        }
        assert_eq!(mods.to_open.take().as_deref(), Some("feedback"));
        mods.hear(&system("ui_panes", json!({ "panes": panes, "shown_id": "probe", "focused_id": null, "focus_requested_id": "feedback" })), 0, &mut logged);
        assert_eq!(mods.to_open, None);
        // A pane not in the roster isn't opened.
        mods.hear(&system("ui_panes", json!({ "panes": [], "shown_id": "gone", "focus_requested_id": "gone" })), 0, &mut logged);
        assert_eq!(mods.to_open, None);
    }

    #[test]
    fn tool_rows_are_asked_for_once_and_again_when_the_call_ends_and_a_menu_and_box_requests_are_heard() {
        let mut mods = Mods::default();
        // Not before Claude Code took the attach.
        assert!(!mods.want_row("use:t1", false));
        mods.attached = true;
        assert!(mods.want_row("use:t1", false));
        assert!(!mods.want_row("use:t1", false));
        assert!(mods.want_row("use:t1", true));
        assert!(!mods.want_row("use:t1", true) && !mods.want_row("use:t1", false));
        // Told a tool row changed, the chat asks for it under its own key.
        let mut logged = None;
        mods.hear(&system("ui_invalidate", json!({ "event": "ui.render", "instances": [{ "surface": "desktop", "component": "ToolResult", "instance_id": "t1" }] })), 0, &mut logged);
        assert_eq!(mods.take_stale(), ["result:t1"]);
        // A plugin's fill of the message box waits for the chat to answer it.
        let fill = json!({ "type": "control_request", "request_id": "q1", "request": { "subtype": "ui_prompt_fill", "surface": "desktop", "client_id": "wings", "text": "Summarize https://example.com", "mode": "insert" } });
        assert!(mods.hear(&fill, 0, &mut logged));
        assert_eq!(mods.asks[0].0, "q1");
        assert_eq!(serde_json::from_str::<Value>(answer("q1", json!({ "filled": true })).trim()).unwrap()["response"]["response"], json!({ "filled": true }));
        // What a mod reads of a call is cut short where it is long.
        mods.tool_called("t1", &json!({ "command": "ls", "content": "x".repeat(10_000) }));
        assert_eq!(mods.tool_inputs["t1"]["content"].as_str().unwrap().len(), KEPT);
        mods.tool_answered(&json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "t1" }] }, "tool_use_result": { "stdout": "ok" } }));
        assert_eq!(mods.tool_results["t1"], json!({ "stdout": "ok" }));
    }

    #[test]
    fn the_requests_are_the_ones_claude_code_takes() {
        let line: Value = serde_json::from_str(attach().trim()).unwrap();
        assert_eq!((&line["request"]["subtype"], &line["request"]["surface"], &line["request"]["client_id"]), (&json!("ui_attach"), &json!("desktop"), &json!("wings")));
        assert_eq!(line["request"]["answers"], json!(["ui_prompt_read", "ui_prompt_fill"]));
        let line: Value = serde_json::from_str(render_row("use:t1", "ToolUse", "t1", json!({ "tool": "Bash" }), 3).trim()).unwrap();
        assert_eq!((&line["request_id"], &line["request"]["instance_id"], &line["request"]["component"]), (&json!("ui-render-use:t1-3"), &json!("t1"), &json!("ToolUse")));
        let line: Value = serde_json::from_str(render(BAND, 7).trim()).unwrap();
        assert_eq!((&line["request_id"], &line["request"]["component"]), (&json!("ui-render-above-prompt-7"), &json!("AbovePrompt")));
        let line: Value = serde_json::from_str(render("probe", 8).trim()).unwrap();
        assert_eq!(line["request"]["component"], "Pane");
        let pressed = press("probe", &json!({ "plugin": "wings-probe", "handle": 100025600 }), Some("ping"), 9).unwrap();
        let line: Value = serde_json::from_str(pressed.trim()).unwrap();
        assert_eq!((&line["request"]["handle"], &line["request"]["key"], &line["request_id"]), (&json!(100025600), &json!("ping"), &json!("ui-press-probe-9")));
        // As Claude Code took them from a probe mod's field and dropdown.
        let typed: Value = serde_json::from_str(input("form", &json!({ "plugin": "probe-a", "handle": 41726026 }), "name", "Ana", 10).unwrap().trim()).unwrap();
        assert_eq!(typed["request"], json!({ "subtype": "ui_input", "plugin": "probe-a", "handle": 41726026, "kind": "submit", "value": "Ana", "key": "name", "surface": "desktop", "client_id": "wings" }));
        let picked: Value = serde_json::from_str(select("form", &json!({ "plugin": "probe-a", "handle": 41726027 }), "pick", "2", 11).unwrap().trim()).unwrap();
        assert_eq!(picked["request"], json!({ "subtype": "ui_select", "plugin": "probe-a", "handle": 41726027, "value": "2", "key": "pick", "surface": "desktop", "client_id": "wings" }));
        let line: Value = serde_json::from_str(render(MODES, 12).trim()).unwrap();
        assert_eq!((&line["request"]["component"], &line["request"]["props"]), (&json!("SessionMode"), &json!({ "modes": [] })));
        // A submit or a pick is drawn again, as a press is.
        let mut mods = Mods { attached: true, ..Mods::default() };
        assert!(mods.answered("ui-select-form-11", &json!({ "subtype": "success", "response": { "handled": true } })));
        assert_eq!(mods.take_stale(), ["form"]);
    }
}
