//! Claude Code's mods in the chat, as Claude Code Desktop shows them: the chat attaches as a remote surface
//! (`ui_attach`), and Claude Code tells it what its plugins show. Status lines (`ui_status`), toasts (`ui_toast`) and log
//! lines (`ui_log`) come by themselves; the panes the plugins open come as a roster (`ui_panes`), and the chat asks for
//! each pane's tree, the band above the prompt, the hint under it and the mode labels (`ui_render`), again when Claude
//! Code says they changed (`ui_invalidate`) and after a press, a submit or a pick in one of them (`ui_press`, `ui_input`,
//! `ui_select`). Measured on Claude Code 2.1.328, which marks these messages internal: a Claude Code without them answers
//! the attach with an error, and the chat then asks nothing. A mod's notifications (`ui_notify`, `ui_notify_cancel`) and
//! the rows it places in the transcript (`ui_placed`) are heard too. Measured on 2.1.329: Claude Code sends the
//! notifications only when `CLAUDE_CODE_NOTIFICATIONS_HOST` is in its environment (a mod's `$.ui.notify` answers
//! `no-surface` without it), and neither `--settings` `env` nor `apply_flag_settings` sets it, so they come once the host
//! can give a program an environment.

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
/// The key of a row a mod placed in the transcript (`ui_placed`): its `request_id` after this.
pub const PLACED: &str = "placed:";
/// The longest a notification may wait, as Claude Code caps it: seven days.
const NOTIFY_MOST: u64 = 604_800_000;
/// The longest text of a tool call's input or result a mod gets: a file's whole content isn't what a row reads.
pub const KEPT: usize = 4_000;
/// The attach's request, whose answer a page that takes the chat's Claude Code over hears again (`reattach.rs`).
pub const ATTACH: &str = "ui-attach";
const RENDER: &str = "ui-render-";
const PRESS: &str = "ui-press-";
const INPUT: &str = "ui-input-";
const SELECT: &str = "ui-select-";
const CLOSE: &str = "ui-close-";
const PANE: &str = "ui-pane-";

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

/// A row a mod placed in the transcript: asked for as `Placed` under `request`, drawn after the message `after`.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub plugin: String,
    pub id: String,
    pub request: String,
    pub after: Option<String>,
}

/// A notification a mod sent, held until its time (`delay_ms`, `at`).
#[derive(Clone, Debug, PartialEq)]
struct Notify {
    plugin: String,
    source: String,
    title: String,
    text: String,
    key: Option<String>,
    due: u64,
    until_back: bool,
}

/// A notification that is due: the line the conversation shows, what Wings notifies with, and the line it replaces when it
/// has a key a notification of the same mod already had.
#[derive(Clone, Debug, PartialEq)]
pub struct Raised {
    pub head: String,
    pub text: String,
    pub replaces: Option<String>,
}

impl Raised {
    pub fn line(&self) -> String {
        format!("{}: {}", self.head, self.text)
    }
}

/// `said` without control characters (a newline stays where `lines`), cut to `most` characters.
fn clean(said: &str, most: usize, lines: bool) -> String {
    let kept = said.chars().filter(|c| !c.is_control() || (lines && *c == '\n'));
    kept.take(most).collect::<String>().trim().to_owned()
}

/// How long after `now` a `ui_notify` line is to wait before it is raised, from its `delay_ms` or `at`; None to raise it
/// now.
pub fn notify_wait(line: &Value, now: u64) -> Option<u64> {
    if line["type"] != "system" || line["subtype"] != "ui_notify" {
        return None;
    }
    let wait = line["delay_ms"].as_u64().or_else(|| line["at"].as_u64().map(|at| at.saturating_sub(now)));
    wait.filter(|wait| *wait > 0).map(|wait| wait.min(NOTIFY_MOST))
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
    /// A pane Claude Code asked to focus (`focus_requested_id`) that the chat hasn't settled yet, with `ui_pane_focus`.
    pub to_focus: Option<String>,
    /// The last pane Claude Code said it shows and asked to focus, so each ask opens the sidebar once.
    shown: (Option<String>, Option<String>),
    /// What is to be asked for again.
    stale: BTreeSet<String>,
    /// The rows mods placed in the transcript, in Claude Code's order, and the item each message's uuid is (the last item
    /// the message made), for the rows after it.
    pub placed: Vec<Placed>,
    uuids: HashMap<String, usize>,
    /// Notifications waiting for their time, those due that the conversation has yet to show, and the line each keyed one
    /// last showed, to replace.
    pending: Vec<Notify>,
    pub raised: Vec<Raised>,
    noted: HashMap<(String, String), String>,
}

/// The request that attaches the chat as a surface, among those it opens with. A docked layout, so a pane a plugin opens
/// unasked is a sidebar, as the chat's side panel is. The chat answers a plugin's read and fill of the message box.
pub fn attach() -> String {
    let viewport = json!({ "columns": 100, "rows": 40, "isFullscreen": true });
    request(ATTACH, json!({ "subtype": "ui_attach", "surface": SURFACE, "client_id": CLIENT, "viewport": viewport, "answers": ["ui_prompt_read", "ui_prompt_fill"], "draws": ["Progress"] }))
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

/// The request that tells a mod what is in its field as you type (`kind` `change`), numbered `n`. Claude Code runs the
/// mod's `onInput` and answers `{ handled, element, value }`.
pub fn input_change(instance: &str, press: &Value, key: &str, value: &str, n: u32) -> Option<String> {
    let (plugin, handle) = (press["plugin"].as_str()?, press["handle"].as_u64()?);
    let mut body = json!({ "subtype": "ui_input", "plugin": plugin, "handle": handle, "kind": "change", "value": value, "key": key, "surface": SURFACE, "client_id": CLIENT });
    // A redraw after each edit retires the field's handle: with its component and instance Claude Code finds the field again.
    if let Some(component) = component_of(instance) {
        body["component"] = json!(component);
        body["instance_id"] = json!(instance.strip_prefix(PLACED).unwrap_or(instance));
    }
    Some(request(&format!("{INPUT}{instance}-{n}"), body))
}

/// The component a tree of the chat's key was asked for as, for the fields of a pane, the band and a placed row.
fn component_of(instance: &str) -> Option<&'static str> {
    match instance {
        BAND => Some("AbovePrompt"),
        HINT | MODES | THEME | SPINNER | MENU => None,
        _ if instance.starts_with(PLACED) => Some("Placed"),
        _ if instance.contains(':') => None,
        _ => Some("Pane"),
    }
}

/// The request that picks `value` in a mod's dropdown, numbered `n`.
pub fn select(instance: &str, press: &Value, key: &str, value: &str, n: u32) -> Option<String> {
    let (plugin, handle) = (press["plugin"].as_str()?, press["handle"].as_u64()?);
    let body = json!({ "subtype": "ui_select", "plugin": plugin, "handle": handle, "value": value, "key": key, "surface": SURFACE, "client_id": CLIENT });
    Some(request(&format!("{SELECT}{instance}-{n}"), body))
}

/// The request that tells Claude Code you closed pane `id` in Wings, numbered `n`: it runs the plugin's close, which may
/// keep the pane open, and the roster that follows says whether it closed.
pub fn close(id: &str, n: u32) -> String {
    request(&format!("{CLOSE}{n}"), json!({ "subtype": "ui_close", "id": id, "client_id": CLIENT }))
}

/// The request that tells Claude Code you picked pane `id` and Wings shows it, numbered `n`.
pub fn pane_show(id: &str, n: u32) -> String {
    request(&format!("{PANE}show-{n}"), json!({ "subtype": "ui_pane_show", "id": id, "surface": SURFACE, "client_id": CLIENT }))
}

/// The request that tells Claude Code which pane holds the keyboard, numbered `n`: `id` once Wings focused it, which also
/// settles a standing focus request, and null where it refused one.
pub fn pane_focus(id: Option<&str>, n: u32) -> String {
    request(&format!("{PANE}focus-{n}"), json!({ "subtype": "ui_pane_focus", "id": id, "surface": SURFACE, "client_id": CLIENT }))
}

/// The request that presses a link of a mod's `Markdown` (`href` as the text wrote it), numbered `n`.
pub fn press_link(instance: &str, press: &Value, key: Option<&str>, href: &str, n: u32) -> Option<String> {
    let (plugin, handle) = (press["plugin"].as_str()?, press["handle"].as_u64()?);
    let mut body = json!({ "subtype": "ui_press", "plugin": plugin, "handle": handle, "href": href, "surface": SURFACE, "client_id": CLIENT, "press_id": format!("p{n}") });
    if let Some(key) = key {
        body["key"] = json!(key);
    }
    Some(request(&format!("{PRESS}{instance}-{n}"), body))
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
                let before: Vec<String> = self.panes.iter().map(|pane| pane.id.clone()).collect();
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
                    self.to_focus = focus.clone().filter(|_| focus != self.shown.1);
                    self.to_open = asked;
                }
                self.shown = (shown, focus);
                // A closed pane's tree goes; the rows of the transcript (a call's, a message's, a placed row's) stay.
                let closed: Vec<String> = before.into_iter().filter(|id| !open.contains(id)).collect();
                self.trees.retain(|instance, _| !closed.contains(instance));
                self.stale.extend(open.into_iter().filter(|id| !self.trees.contains_key(id)));
            }
            "ui_invalidate" => match line["instances"].as_array() {
                Some(instances) => self.stale.extend(instances.iter().filter_map(|instance| {
                    let id = instance["instance_id"].as_str()?;
                    Some(match instance["component"].as_str() {
                        Some("ToolUse") => format!("{USE}{id}"),
                        Some("ToolResult") => format!("{RESULT}{id}"),
                        Some("Placed") => format!("{PLACED}{id}"),
                        // Its scene changing (`backdrop`) comes with the look, which carries the backdrop too.
                        Some("Theme") => THEME.to_owned(),
                        _ => id.to_owned(),
                    })
                })),
                None => self.stale.extend(self.panes.iter().map(|pane| pane.id.clone()).chain(SITES.map(str::to_owned))),
            },
            "ui_notify" => {
                let (title, source) = (clean(line["title"].as_str().unwrap_or_default(), 120, false), clean(line["source"].as_str().unwrap_or_default(), 128, false));
                let plugin = clean(&plugin, 64, false);
                let key = line["key"].as_str().filter(|key| !key.is_empty()).map(str::to_owned);
                let wait = notify_wait(line, now);
                let notify = Notify {
                    title: if title.is_empty() { plugin.clone() } else { title },
                    plugin,
                    text: clean(line["text"].as_str().unwrap_or_default(), 1_000, true),
                    due: now + wait.unwrap_or(0),
                    until_back: line["until_back"] == true,
                    source,
                    key,
                };
                // One with the same source and key replaces the one waiting.
                self.pending.retain(|held| notify.key.is_none() || held.source != notify.source || held.key != notify.key);
                match wait {
                    Some(_) => self.pending.push(notify),
                    None => self.raise(notify),
                }
            }
            "ui_notify_cancel" => {
                let (source, key) = (line["source"].as_str().unwrap_or_default(), line["key"].as_str());
                self.pending.retain(|held| held.source != source || key.is_some_and(|key| held.key.as_deref() != Some(key)));
                self.noted.retain(|(noted, kept), _| noted != source || key.is_some_and(|key| kept != key));
            }
            "ui_placed" => {
                self.placed = line["rows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|row| {
                        Some(Placed {
                            plugin: row["plugin"].as_str().unwrap_or_default().to_owned(),
                            id: row["id"].as_str().unwrap_or_default().to_owned(),
                            request: row["request_id"].as_str().filter(|request| !request.is_empty())?.to_owned(),
                            after: row["after_uuid"].as_str().map(str::to_owned),
                        })
                    })
                    .collect();
                let live: Vec<String> = self.placed.iter().map(|row| format!("{PLACED}{}", row.request)).collect();
                self.trees.retain(|key, _| !key.starts_with(PLACED) || live.contains(key));
                self.stale.extend(live.into_iter().filter(|key| !self.trees.contains_key(key)));
            }
            _ => return false,
        }
        true
    }

    /// Shows a notification: what the conversation says and Wings notifies with. A mod's own words follow the chat's, as
    /// "Mod name", so neither its name nor its title can pass for the app's own.
    fn raise(&mut self, notify: Notify) {
        let head = if notify.title == notify.plugin { format!("Mod {}", notify.plugin) } else { format!("Mod {} \u{b7} {}", notify.plugin, notify.title) };
        let raised = Raised { head, text: notify.text, replaces: None };
        let replaces = notify.key.and_then(|key| self.noted.insert((notify.source, key), raised.line()));
        self.raised.push(Raised { replaces, ..raised });
    }

    /// Raises the notifications whose time came. One with `until_back` is thrown away when `back`: you are at the session.
    pub fn due(&mut self, now: u64, back: bool) {
        let (due, waiting): (Vec<Notify>, Vec<Notify>) = std::mem::take(&mut self.pending).into_iter().partition(|held| held.due <= now);
        self.pending = waiting;
        for held in due.into_iter().filter(|held| !(held.until_back && back)) {
            self.raise(held);
        }
    }

    /// Keeps which item the message `line` carries (its `uuid`) made, `items` long now, for the rows placed after it.
    pub fn note_message(&mut self, line: &Value, items: usize) {
        if matches!(line["type"].as_str(), Some("user" | "assistant")) && line["isReplay"] != true && items > 0 {
            if let Some(uuid) = line["uuid"].as_str() {
                self.uuids.insert(uuid.to_owned(), items - 1);
            }
        }
    }

    /// The item a message by `uuid` made, among the first `items`.
    pub fn item_of(&self, uuid: &str, items: usize) -> Option<usize> {
        self.uuids.get(uuid).copied().filter(|at| *at < items)
    }

    /// Forgets the messages from item `at` on (a rewind cut them), or all of them.
    pub fn cut(&mut self, at: usize) {
        self.uuids.retain(|_, item| *item < at);
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
        // Its answers to a pane's close, show and focus say nothing the roster that follows doesn't.
        id.starts_with(CLOSE) || id.starts_with(PANE)
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
        // The focus it asked for is to be settled, once.
        assert_eq!(mods.to_focus.take().as_deref(), Some("feedback"));
        mods.hear(&system("ui_panes", json!({ "panes": panes, "shown_id": "probe", "focused_id": null, "focus_requested_id": "feedback" })), 0, &mut logged);
        assert_eq!((mods.to_open.as_deref(), mods.to_focus.as_deref()), (None, None));
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
        assert_eq!(line["request"]["draws"], json!(["Progress"]));
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

    #[test]
    fn closing_showing_and_focusing_a_pane_are_the_requests_claude_code_takes() {
        let line: Value = serde_json::from_str(close("feedback", 4).trim()).unwrap();
        assert_eq!(line["request_id"], "ui-close-4");
        assert_eq!(line["request"], json!({ "subtype": "ui_close", "id": "feedback", "client_id": "wings" }));
        let line: Value = serde_json::from_str(pane_show("feedback", 5).trim()).unwrap();
        assert_eq!(line["request"], json!({ "subtype": "ui_pane_show", "id": "feedback", "surface": "desktop", "client_id": "wings" }));
        let line: Value = serde_json::from_str(pane_focus(Some("feedback"), 6).trim()).unwrap();
        assert_eq!(line["request"], json!({ "subtype": "ui_pane_focus", "id": "feedback", "surface": "desktop", "client_id": "wings" }));
        let line: Value = serde_json::from_str(pane_focus(None, 7).trim()).unwrap();
        assert_eq!(line["request"]["id"], Value::Null);
        // Its answers are the roster's to show, not the conversation's.
        let mut mods = Mods::default();
        assert!(mods.answered("ui-close-4", &json!({ "subtype": "success", "response": { "closed": true } })));
        assert!(mods.answered("ui-pane-show-5", &json!({ "subtype": "success", "response": {} })));
    }

    /// What Claude Code 2.1.329 printed to a client attached as `desktop` for a probe mod that notified, placed a row after
    /// the turn and answered its `Placed` row's field and link, trimmed (`CLAUDE_CODE_NOTIFICATIONS_HOST=1` was set: a
    /// mod's notifications reach a client only then).
    fn probe() -> Vec<Value> {
        include_str!("../tests/mods_probe.jsonl").lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }

    fn probe_line(subtype: &str, nth: usize) -> Value {
        probe().into_iter().filter(|line| line["subtype"] == subtype).nth(nth).unwrap()
    }

    #[test]
    fn a_mods_notifications_are_raised_at_once_or_when_they_are_due_and_a_keyed_one_replaces_the_last() {
        let mut mods = Mods::default();
        let mut logged = None;
        let (ready, later, at, gone, cancel) = (probe_line("ui_notify", 0), probe_line("ui_notify", 1), probe_line("ui_notify", 2), probe_line("ui_notify", 3), probe_line("ui_notify_cancel", 0));
        let now = at["at"].as_u64().unwrap() - 5_000;
        for line in [&ready, &later, &at, &gone] {
            assert!(mods.hear(line, now, &mut logged));
        }
        // With no delay it is raised now, under the mod's name, with the title the mod gave.
        assert_eq!(mods.raised, [Raised { head: "Mod wings-probe2 \u{b7} Probe".into(), text: "probe ready".into(), replaces: None }]);
        assert_eq!(mods.raised[0].line(), "Mod wings-probe2 \u{b7} Probe: probe ready");
        // The others wait for their time: `delay_ms` after it came, `at` on the clock.
        assert_eq!((notify_wait(&ready, now), notify_wait(&later, now), notify_wait(&at, now), notify_wait(&gone, now)), (None, Some(3_000), Some(5_000), Some(60_000)));
        mods.raised.clear();
        mods.due(now + 2_999, false);
        assert!(mods.raised.is_empty());
        // The mod took one back by its key.
        assert!(mods.hear(&cancel, now, &mut logged));
        // One that asked to go if you came back is thrown away when you are at the session; the one with no such wish is
        // raised, and its title is the mod's name, which isn't said twice.
        mods.due(now + 5_000, true);
        assert_eq!(mods.raised, [Raised { head: "Mod wings-probe2".into(), text: "at a time".into(), replaces: None }]);
        mods.raised.clear();
        mods.due(now + 120_000, false);
        assert!(mods.raised.is_empty(), "the one it took back, and the one thrown away, don't come");
        // The same key again takes the place of the last one it raised.
        mods.hear(&ready, now, &mut logged);
        assert_eq!(mods.raised[0].replaces.as_deref(), Some("Mod wings-probe2 \u{b7} Probe: probe ready"));
        // A delay of a day, which Claude Code caps at a week, waits; nothing it says is trusted to be tidy.
        let mut odd = ready.clone();
        odd["text"] = json!("a\u{1b}[31mred\nline");
        odd["plugin"] = json!("evil\nname");
        odd["key"] = Value::Null;
        odd["delay_ms"] = json!(u64::MAX);
        assert_eq!(notify_wait(&odd, 0), Some(NOTIFY_MOST));
        odd["delay_ms"] = json!(0);
        mods.raised.clear();
        mods.hear(&odd, 0, &mut logged);
        assert_eq!(mods.raised[0].text, "a[31mred\nline");
        assert!(mods.raised[0].head.starts_with("Mod evilname"));
    }

    #[test]
    fn notifications_become_notices_in_the_conversation_and_words_to_notify_with() {
        let mut session = crate::session::Session::default();
        let now = probe_line("ui_notify", 2)["at"].as_u64().unwrap() - 5_000;
        session.now = now;
        session.hear(&probe_line("ui_notify", 0));
        assert_eq!(session.items, [crate::session::Item::Notice("Mod wings-probe2 \u{b7} Probe: probe ready".into())]);
        assert_eq!(session.alerts, [("Mod wings-probe2 \u{b7} Probe".to_owned(), "probe ready".to_owned())]);
        // The same key again changes the notice rather than adding another.
        session.hear(&probe_line("ui_notify", 0));
        assert_eq!(session.items.len(), 1);
        // One that comes due later is shown as it is raised.
        session.hear(&probe_line("ui_notify", 1));
        assert_eq!(session.items.len(), 1);
        session.mods.due(now + 3_000, false);
        session.show_raised();
        assert_eq!(session.items.len(), 2);
    }

    #[test]
    fn a_row_a_mod_places_is_asked_for_as_placed_and_follows_the_message_it_names() {
        let mut session = crate::session::Session::default();
        session.mods.attached = true;
        // The turn as it was heard: your message, Claude's answer, then the push naming Claude's answer.
        for line in probe().iter().filter(|line| matches!(line["type"].as_str(), Some("user" | "assistant"))) {
            session.hear(line);
        }
        let assistant = probe().into_iter().find(|line| line["type"] == "assistant").unwrap();
        let placed = probe_line("ui_placed", 0);
        let after = placed["rows"][0]["after_uuid"].as_str().unwrap();
        assert_eq!(assistant["uuid"], after, "the row follows the last message of the turn, by its uuid");
        session.hear(&placed);
        let request = placed["rows"][0]["request_id"].as_str().unwrap();
        let key = format!("{PLACED}{request}");
        assert_eq!(session.mods.placed, [Placed { plugin: "wings-probe2".into(), id: "row1".into(), request: request.into(), after: Some(after.into()) }]);
        assert_eq!(session.mods.item_of(after, session.items.len()), Some(session.items.len() - 1));
        assert_eq!(session.mods.take_stale(), [key.clone()]);
        // A rewind that cut the message takes it from the rows' places.
        session.mods.cut(0);
        assert_eq!(session.mods.item_of(after, 9), None);
        // Claude Code's answer is the row's tree, kept under its key; its change is asked for again.
        let drawn = probe().into_iter().find(|line| line["response"]["request_id"].as_str().is_some_and(|id| id.starts_with("ui-render-placed:"))).unwrap();
        let id = drawn["response"]["request_id"].as_str().unwrap();
        assert!(session.mods.answered(id, &drawn["response"]));
        assert_eq!(session.mods.trees.get(&key), Some(&drawn["response"]["response"]["tree"]));
        let mut logged = None;
        session.mods.hear(&system("ui_invalidate", json!({ "event": "ui.render", "instances": [{ "surface": "desktop", "component": "Placed", "instance_id": request }] })), 0, &mut logged);
        assert_eq!(session.mods.take_stale(), [key.clone()]);
        // A roster of panes doesn't take the row's tree along, and the row's leaving does.
        session.mods.hear(&system("ui_panes", json!({ "panes": [] })), 0, &mut logged);
        assert!(session.mods.trees.contains_key(&key));
        session.mods.hear(&system("ui_placed", json!({ "rows": [] })), 0, &mut logged);
        assert!(session.mods.trees.is_empty() && session.mods.placed.is_empty());
        // The ask is `Placed` under the id Claude Code gave.
        let line: Value = serde_json::from_str(render_row(&key, "Placed", request, json!({}), 4).trim()).unwrap();
        assert_eq!((&line["request"]["component"], &line["request"]["instance_id"], &line["request"]["props"]), (&json!("Placed"), &json!(request), &json!({})));
    }

    #[test]
    fn what_you_type_in_a_field_and_a_link_you_press_are_the_requests_claude_code_answered() {
        let (input_answer, press_answer) = (probe_line_response("ui-input-1"), probe_line_response("ui-press-1"));
        assert_eq!(input_answer["response"]["response"], json!({ "handled": true, "element": "q", "value": "abc" }));
        assert_eq!(press_answer["response"]["response"], json!({ "handled": true, "element": "md" }));
        let tree = &probe().into_iter().find(|line| line["response"]["request_id"].as_str().is_some_and(|id| id.starts_with("ui-render-placed:"))).unwrap()["response"]["response"]["tree"];
        let field = &tree["children"][1];
        let key = format!("{PLACED}row");
        let line: Value = serde_json::from_str(input_change(&key, &field["press"], "q", "abc", 5).unwrap().trim()).unwrap();
        // As the probe's own: the field's whole text, with the component and instance Claude Code finds the field again by.
        assert_eq!(
            line["request"],
            json!({ "subtype": "ui_input", "plugin": "wings-probe2", "handle": field["press"]["handle"], "kind": "change", "value": "abc", "key": "q", "component": "Placed", "instance_id": "row", "surface": "desktop", "client_id": "wings" })
        );
        assert_eq!(line["request_id"], "ui-input-placed:row-5");
        let pane: Value = serde_json::from_str(input_change("probe", &field["press"], "q", "abc", 6).unwrap().trim()).unwrap();
        assert_eq!((&pane["request"]["component"], &pane["request"]["instance_id"]), (&json!("Pane"), &json!("probe")));
        let tool: Value = serde_json::from_str(input_change("use:t1", &field["press"], "q", "abc", 6).unwrap().trim()).unwrap();
        assert!(tool["request"]["component"].is_null());
        let markdown = &tree["children"][4];
        let link: Value = serde_json::from_str(press_link(&key, &markdown["press"], Some("md"), "https://example.com/a", 7).unwrap().trim()).unwrap();
        assert_eq!(
            link["request"],
            json!({ "subtype": "ui_press", "plugin": "wings-probe2", "handle": markdown["press"]["handle"], "href": "https://example.com/a", "surface": "desktop", "client_id": "wings", "press_id": "p7", "key": "md" })
        );
        // Their answers draw the row again.
        let mut mods = Mods { attached: true, ..Mods::default() };
        assert!(mods.answered("ui-input-placed:row-5", &input_answer["response"]) && mods.answered("ui-press-placed:row-7", &press_answer["response"]));
        assert_eq!(mods.take_stale(), ["placed:row"]);
    }

    fn probe_line_response(request: &str) -> Value {
        probe().into_iter().find(|line| line["response"]["request_id"] == request).unwrap()
    }
}
