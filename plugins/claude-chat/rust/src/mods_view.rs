//! What Claude Code's mods show in the chat (`mods.rs`): over the message box the band a plugin draws above the prompt,
//! each plugin's status line and its toasts; under it what mods add to the hint line and the mode labels; in the side
//! panel each pane a plugin opened, under its title. A mod's tree of elements (`Box`, `Text`, `Button`, `Link`, `Code`,
//! `Markdown`, `Input`, `Select`) becomes the chat's own rows, texts, buttons and fields, and a press, a submit or a pick
//! goes back to Claude Code. What runs a plugin's own code (`Client`) shows as a line that says it is Claude Code's.

use std::collections::HashMap;

use serde_json::Value;
use wings_plugin::{
    host,
    markdown::{highlighted_code, markdown},
    ui::{Node, button, muted, row, select, stack, text, text_input},
};

use std::cell::RefCell;

use serde_json::json;

use crate::{
    chat::{self, Chat},
    mods::{self, BAND, CLAUDES, GROUP, HINT, MENU, MODES, QUESTION, RESULT, SPINNER, USE, YOURS},
    session::{Item, Tool},
};

thread_local! {
    /// The tool rows the page drew in its last build, by key, with whether each call had ended, to ask the mods for.
    static SEEN: RefCell<Vec<(String, bool)>> = const { RefCell::new(Vec::new()) };
}

/// Where a tree is drawn: the instance its presses and fields go back to, and what you typed in its fields.
struct At<'a> {
    instance: &'a str,
    drafts: &'a HashMap<String, String>,
}

/// The most nodes one mod's tree makes: the page's budget is the conversation's.
const MOST: usize = 300;

/// The band, the status lines and the toasts, over the message box. None with nothing to show.
pub fn above_prompt(c: &Chat) -> Option<Node> {
    let mods = &c.session.mods;
    let now = host::now() as u64;
    let band = mods.trees.get(BAND).and_then(|tree| drawn(tree, &At { instance: BAND, drafts: &mods.drafts }));
    let status: Vec<String> = mods.status.iter().map(|(plugin, said)| format!("{plugin} · {said}")).collect();
    let toasts = mods.toasts.iter().filter(|toast| toast.until > now).map(|toast| {
        // A toast often starts with its plugin's name already.
        let said = if toast.text.starts_with(&toast.plugin) { toast.text.clone() } else { format!("{}: {}", toast.plugin, toast.text) };
        text(said).prop("size", "small").prop("tone", "info")
    });
    let lines: Vec<Node> = band
        .into_iter()
        .chain((!status.is_empty()).then(|| muted(status.join("   ")).prop("size", "small").prop("truncate", true).prop("title", status.join("\n"))))
        .chain(toasts)
        .collect();
    (!lines.is_empty()).then(|| stack().prop("gap", "xs").children(lines).key("mods"))
}

/// What mods add to the hint line under the message box and to its mode labels, in one quiet line under it.
pub fn under_prompt(c: &Chat) -> Option<Node> {
    let mods = &c.session.mods;
    let said = |instance: &str, keys: &[&str]| -> Vec<String> {
        let props = mods.props.get(instance);
        keys.iter().filter_map(|key| props?[*key].as_str().map(str::trim).filter(|said| !said.is_empty()).map(str::to_owned)).collect()
    };
    let hint = said(HINT, &["hint", "tail"]).join(" · ");
    let modes: Vec<&str> = mods.props.get(MODES).and_then(|props| props["modes"].as_array()).into_iter().flatten().filter_map(Value::as_str).collect();
    if hint.is_empty() && modes.is_empty() {
        return None;
    }
    Some(
        row()
            .prop("gap", "s")
            .child(muted(hint).prop("size", "small").prop("grow", true).prop("truncate", true))
            .maybe((!modes.is_empty()).then(|| muted(modes.join(" & ")).prop("size", "small")))
            .key("mods-under"),
    )
}

/// A tool call's row as a mod draws it, in place of `head`, the chat's own; `head` where none does. The row is asked for
/// once the page is built (`ask_rows`), and again when the call ends.
pub fn tool_head(c: &Chat, tool: &Tool, head: Node) -> Node {
    let ended = tool.output.is_some();
    SEEN.with(|seen| {
        let mut seen = seen.borrow_mut();
        seen.push((format!("{USE}{}", tool.id), ended));
        if ended {
            seen.push((format!("{RESULT}{}", tool.id), true));
        }
    });
    let key = format!("{USE}{}", tool.id);
    let mods = &c.session.mods;
    mods.trees.get(&key).and_then(|tree| drawn(tree, &At { instance: &key, drafts: &mods.drafts })).unwrap_or(head)
}

/// What a mod draws under a tool call's row for its result, when one does.
pub fn tool_result(c: &Chat, tool: &Tool) -> Option<Node> {
    let key = format!("{RESULT}{}", tool.id);
    let mods = &c.session.mods;
    drawn(mods.trees.get(&key)?, &At { instance: &key, drafts: &mods.drafts })
}

/// The most messages asked of the mods at a time, the newest the page drew: a long session's whole history would be a
/// request each.
const MESSAGES_ASKED: usize = 40;

/// The key a message is asked for under: its place and a hash of its text.
fn message_key(prefix: &str, at: usize, said: &str) -> String {
    let hash = said.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    format!("{prefix}{at}:{hash:x}")
}

/// What mods made of a message the page draws: a tree of their own in its place, or its text reworded. Asked for once
/// the page is built (`ask_rows`).
pub fn message(c: &Chat, at: usize, item: &Item) -> (Option<Node>, Option<String>) {
    let (prefix, said) = match item {
        Item::User(said) => (YOURS, said),
        Item::Text(said) => (CLAUDES, said),
        _ => return (None, None),
    };
    let key = message_key(prefix, at, said);
    SEEN.with(|seen| seen.borrow_mut().push((key.clone(), true)));
    let mods = &c.session.mods;
    let tree = mods.trees.get(&key).and_then(|tree| drawn(tree, &At { instance: &key, drafts: &mods.drafts }));
    // A rewording of a text cut short for the mod would cut the message.
    let reworded = mods.props.get(&key).and_then(|props| props["text"].as_str()).filter(|_| said.len() <= mods::KEPT).map(str::to_owned);
    (tree, reworded)
}

/// The chat's palette from a mod's look: its dark colours, as Wings is dark, by the design system's names Claude Code
/// Desktop paints them under, and its still backdrop as the page's ground. None without a look, or one for light alone.
pub fn palette(c: &Chat) -> Option<Value> {
    let theme = c.session.mods.theme.as_ref()?;
    let dark = theme["colors"]["inputs"]["dark"].as_object()?;
    let hex = |value: &Value| value.as_str().filter(|hex| hex.len() == 7 && hex.starts_with('#')).map(str::to_owned);
    // The surfaces and text are its grays: `surface-0` the darkest, `text-primary` the lightest. A role's text tone on
    // dark is its light shade.
    const ROLES: [(&str, &str); 10] = [
        ("base", "gray-900"),
        ("page", "gray-900"),
        ("dialog", "gray-830"),
        ("popover", "gray-800"),
        ("text", "gray-50"),
        ("muted", "gray-400"),
        ("info", "role-accent-250"),
        ("danger", "role-danger-250"),
        ("done", "role-success-250"),
        ("working", "role-warning-250"),
    ];
    let mut palette = serde_json::Map::new();
    for (role, token) in ROLES {
        if let Some(color) = dark.get(token).and_then(hex) {
            palette.insert(role.to_owned(), json!(color));
        }
    }
    // A ground's colour is a hex or a surface's name. A gradient lies from its first stop to its last.
    let named = |value: &Value| -> Option<String> {
        let name = value.as_str().or_else(|| value["color"].as_str())?;
        let token = match name {
            "surface-0" => "gray-900",
            "surface-1" => "gray-850",
            "surface-2" => "gray-830",
            "surface-3" => "gray-800",
            _ => return hex(&json!(name)),
        };
        dark.get(token).and_then(hex)
    };
    let backdrop = &theme["backdrop"];
    let ground = match (&backdrop["color"], backdrop["gradient"]["stops"].as_array()) {
        (color, _) if !color.is_null() => named(color).map(|color| json!({ "from": color, "to": color })),
        (_, Some(stops)) if backdrop["gradient"]["shape"] != "radial" => {
            let (from, to) = (named(stops.first()?)?, named(stops.last()?)?);
            Some(json!({ "from": from, "to": to, "angle": backdrop["gradient"]["angle"].as_f64().unwrap_or(180.0) }))
        }
        _ => None,
    };
    if let Some(ground) = ground {
        palette.insert("ground".to_owned(), ground);
    }
    (!palette.is_empty()).then_some(Value::Object(palette))
}

/// What mods add above Claude's question: the elements of their tree for the dialog, which holds Claude Code's own
/// question once, drawn here as the chat's card.
pub fn question(c: &Chat, request: &str) -> Option<Node> {
    let key = format!("{QUESTION}{request}");
    SEEN.with(|seen| seen.borrow_mut().push((key.clone(), true)));
    let mods = &c.session.mods;
    drawn(mods.trees.get(&key)?, &At { instance: &key, drafts: &mods.drafts })
}

/// What a mod draws for a folded run of tool calls in place of its count line ("Ran 2 commands"), when one does.
pub fn group(c: &Chat, first: usize, tools: &[&Tool]) -> Option<Node> {
    let key = format!("{GROUP}{first}:{}", tools.len());
    let ended = tools.iter().all(|tool| tool.output.is_some());
    SEEN.with(|seen| seen.borrow_mut().push((key.clone(), ended)));
    let mods = &c.session.mods;
    drawn(mods.trees.get(&key)?, &At { instance: &key, drafts: &mods.drafts })
}

/// What a mod made of the line under the conversation while Claude works: a tree of its own, or the words it shows.
pub fn spinner(c: &Chat) -> (Option<Node>, Option<String>) {
    let mods = &c.session.mods;
    let tree = mods.trees.get(SPINNER).and_then(|tree| drawn(tree, &At { instance: SPINNER, drafts: &mods.drafts }));
    let said = mods.props.get(SPINNER).map(|props| {
        let word = props["message"].as_str().or(props["word"].as_str()).unwrap_or_default();
        format!("{word}{}", props["suffix"].as_str().unwrap_or_default())
    });
    (tree, said.filter(|said| !said.trim().is_empty()))
}

/// Asks Claude Code for the tool rows and messages the page just drew that it hasn't been asked for, or asked while they
/// ran: of the messages, the newest few.
pub fn ask_rows() {
    let mut seen = SEEN.with(|seen| std::mem::take(&mut *seen.borrow_mut()));
    if seen.is_empty() {
        return;
    }
    let place = |key: &str| key.strip_prefix(YOURS).or(key.strip_prefix(CLAUDES)).and_then(|rest| rest.split(':').next()?.parse::<usize>().ok());
    let mut messages: Vec<usize> = seen.iter().filter_map(|(key, _)| place(key)).collect();
    messages.sort_unstable_by(|a, b| b.cmp(a));
    let oldest = messages.get(MESSAGES_ASKED - 1).copied().unwrap_or(0);
    seen.retain(|(key, _)| place(key).is_none_or(|at| at >= oldest));
    let wanted: Vec<String> = chat::with(|c| seen.into_iter().filter(|(key, ended)| c.session.mods.want_row(key, *ended)).map(|(key, _)| key).collect());
    for key in wanted {
        chat::write(move |c, n| request_for(c, &key, n));
    }
}

/// The request that asks Claude Code to draw `key`: a pane, one of the sites around the message box, or a tool call's
/// row or result with the props Claude Code's own row reads. None for one the chat can't ask for now, as the menu once
/// closed or a call no longer in the conversation.
pub fn request_for(c: &Chat, key: &str, n: u32) -> Option<String> {
    if key == MENU {
        return None;
    }
    if key == SPINNER {
        let mode = if c.session.thinking { "thinking" } else if c.session.items.iter().rev().any(|item| matches!(item, Item::Tool(tool) if tool.output.is_none())) { "tool-use" } else { "responding" };
        let word = if c.doing.0.is_empty() { "Working" } else { c.doing.0.as_str() };
        let props = json!({ "word": word, "message": null, "suffix": "", "mode": mode });
        return Some(mods::render_row(key, "Spinner", key, props, n));
    }
    if let Some(request) = key.strip_prefix(QUESTION) {
        let ask = c.session.items.iter().rev().find_map(|item| match item {
            Item::Ask(ask) if ask.request == request => Some(ask),
            _ => None,
        })?;
        let props = json!({ "tool": ask.tool, "questions": mods::trimmed(&ask.input["questions"]) });
        return Some(mods::render_row(key, "AskUserQuestion", request, props, n));
    }
    if let Some(run) = key.strip_prefix(GROUP) {
        let (first, count) = run.split_once(':')?;
        let (first, count): (usize, usize) = (first.parse().ok()?, count.parse().ok()?);
        let calls: Vec<Value> = c.session.items.iter().skip(first).filter_map(|item| match item {
            Item::Tool(tool) => Some(tool),
            _ => None,
        }).take(count).map(|tool| {
            let input = c.session.mods.tool_inputs.get(&tool.id).cloned().unwrap_or(json!({}));
            json!({ "tool": tool.name, "input": input, "isRunning": tool.output.is_none(), "isErrored": matches!(tool.output, Some((_, true))), "isInterrupted": false })
        }).collect();
        if calls.len() != count {
            return None;
        }
        let active = calls.iter().any(|call| call["isRunning"] == true);
        let props = json!({ "calls": calls, "isActive": active, "isExpanded": false });
        return Some(mods::render_row(key, "ToolGroup", key, props, n));
    }
    if let Some((component, at)) = key.strip_prefix(YOURS).map(|rest| ("UserMessage", rest)).or_else(|| key.strip_prefix(CLAUDES).map(|rest| ("AssistantMessage", rest))) {
        let at: usize = at.split(':').next()?.parse().ok()?;
        let item = c.session.items.get(at)?;
        let (Item::User(said) | Item::Text(said)) = item else {
            return None;
        };
        // The message must still be the one asked for: a rewind puts another at its place.
        if message_key(if component == "UserMessage" { YOURS } else { CLAUDES }, at, said) != key {
            return None;
        }
        let text = mods::trimmed(&json!(said));
        let props = match component {
            "UserMessage" => json!({ "text": text, "origin": { "kind": "human" }, "isExpanded": false }),
            _ => {
                let first = at == 0 || !matches!(c.session.items.get(at - 1), Some(Item::Text(_) | Item::Tool(_) | Item::Thought(..)));
                json!({ "text": text, "isFirstOfReply": first })
            }
        };
        return Some(mods::render_row(key, component, key, props, n));
    }
    let (component, call) = match (key.strip_prefix(USE), key.strip_prefix(RESULT)) {
        (Some(call), _) => ("ToolUse", call),
        (_, Some(call)) => ("ToolResult", call),
        _ => return Some(mods::render(key, n)),
    };
    let tool = c.session.items.iter().rev().find_map(|item| match item {
        Item::Tool(tool) if tool.id == call => Some(tool),
        _ => None,
    })?;
    let mods = &c.session.mods;
    let output = mods.tool_results.get(call).cloned().or_else(|| tool.output.as_ref().map(|(said, _)| mods::trimmed(&json!(said))));
    let errored = matches!(tool.output, Some((_, true)));
    let mut props = match component {
        "ToolUse" => json!({ "tool": tool.name, "input": mods.tool_inputs.get(call).cloned().unwrap_or(json!({})), "isRunning": tool.output.is_none(), "isErrored": errored, "isInterrupted": false }),
        _ => json!({ "tool": tool.name, "isErrored": errored }),
    };
    props["output"] = output.unwrap_or(Value::Null);
    if props["output"].is_null() {
        props.as_object_mut()?.remove("output");
    }
    Some(mods::render_row(key, component, call, props, n))
}

/// The chat's answer to Claude Code's request `id`: what the message box holds, or that a plugin's text went into it.
pub fn answer_ask(c: &mut Chat, id: &str, asked: &Value) -> String {
    if asked["subtype"] == "ui_prompt_fill" {
        let text = asked["text"].as_str().unwrap_or_default();
        // The chat doesn't keep the caret: inserted text goes at the end, as appended text does.
        match asked["mode"].as_str() {
            Some("replace") => c.draft = text.to_owned(),
            _ => c.draft.push_str(text),
        }
        return mods::answer(id, json!({ "filled": true }));
    }
    let cursor = c.draft.encode_utf16().count();
    mods::answer(id, json!({ "text": c.draft, "cursor": cursor }))
}

/// Wings says the conversation was right-clicked: at `x`, `y`, on a `link`, a `code` block or a `selection` if one. The
/// menu opens with the chat's own rows, and Claude Code is asked for the rows its mods add.
pub fn open_menu(payload: &Value) {
    let mut on = json!({});
    for part in ["link", "code", "selection"] {
        if payload[part].is_object() {
            on[part] = payload[part].clone();
        }
    }
    let attached = chat::with(|c| {
        c.session.mods.trees.remove(MENU);
        c.session.mods.menu = Some(payload.clone());
        c.session.mods.attached
    });
    if attached {
        chat::write(move |_, n| Some(mods::render_menu(on.clone(), n)));
    }
}

fn close_menu() {
    chat::with(|c| c.session.mods.menu = None);
}

/// The right-click menu, while it is open: copying the selected words, the link or the code under the pointer, and the rows mods add, each a
/// `Button` (its label, with a dim line under it from a dim `Text` inside) or a `Link` to an `https:` address. None when
/// it has no rows.
pub fn menu(c: &Chat) -> Option<Node> {
    let mods = &c.session.mods;
    let at = mods.menu.as_ref()?;
    let mut rows: Vec<Node> = Vec::new();
    if let Some(selected) = at["selection"]["text"].as_str().filter(|text| !text.is_empty()) {
        let copied = selected.to_owned();
        rows.push(button("Copy", "ghost", false, move || copy(&copied)).key("copy-selection"));
    }
    if let Some(href) = at["link"]["href"].as_str() {
        let (copied, opened) = (href.to_owned(), href.to_owned());
        rows.push(button("Copy link", "ghost", false, move || copy(&copied)).key("copy-link"));
        if href.starts_with("https://") {
            rows.push(button("Open link", "ghost", false, move || {
                host::open_url(&opened);
                close_menu();
            }).key("open-link"));
        }
    }
    if let Some(code) = at["code"]["text"].as_str() {
        let copied = code.to_owned();
        rows.push(button("Copy code", "ghost", false, move || copy(&copied)).key("copy-code"));
    }
    rows.extend(mods.trees.get(MENU).map(menu_rows).unwrap_or_default());
    if rows.is_empty() {
        return None;
    }
    Some(
        Node::new("PopupMenu")
            .prop("x", at["x"].as_f64().unwrap_or_default())
            .prop("y", at["y"].as_f64().unwrap_or_default())
            .on("onClose", |_| close_menu())
            .children(rows)
            .key("context-menu"),
    )
}

fn copy(text: &str) {
    drop(host::call("copyText", json!({ "text": text })));
    close_menu();
}

/// The rows a menu tree holds: its buttons and its `https:` links, wherever they sit in it.
fn menu_rows(tree: &Value) -> Vec<Node> {
    let mut found = Vec::new();
    collect(tree, &mut found);
    found
        .into_iter()
        .enumerate()
        .filter_map(|(at, el)| {
            let props = &el["props"];
            let children = flat(&el["children"]);
            match el["type"].as_str()? {
                "Button" => {
                    let said: Vec<&Value> = children.iter().copied().filter(|child| child.is_string()).collect();
                    let label = props["label"].as_str().map(str::to_owned).unwrap_or_else(|| words(&said));
                    let under = children.iter().find(|child| child["type"] == "Text" && child["props"]["dimColor"] == true).map(|child| words(&flat(&child["children"])));
                    let (press, key) = (el["press"].clone(), props["key"].as_str().map(str::to_owned));
                    let row = button(label, "ghost", false, move || {
                        let (press, key) = (press.clone(), key.clone());
                        chat::write(move |_, n| mods::press(MENU, &press, key.as_deref(), n));
                        close_menu();
                    });
                    Some(match under {
                        Some(under) => row.prop("description", under),
                        None => row,
                    }.key(format!("mod-{at}")))
                }
                "Link" => {
                    let href = props["href"].as_str().filter(|href| href.starts_with("https://"))?.to_owned();
                    let label = props["label"].as_str().map(str::to_owned).unwrap_or_else(|| Some(words(&children)).filter(|said| !said.is_empty()).unwrap_or_else(|| href.clone()));
                    Some(button(label, "ghost", false, move || {
                        host::open_url(&href);
                        close_menu();
                    }).key(format!("mod-{at}")))
                }
                _ => None,
            }
        })
        .collect()
}

/// The buttons and links in a tree, in order.
fn collect<'a>(el: &'a Value, found: &mut Vec<&'a Value>) {
    match el["type"].as_str() {
        Some("Button" | "Link") => found.push(el),
        _ => flat(&el["children"]).into_iter().for_each(|child| collect(child, found)),
    }
}

/// Each pane a plugin opened, as a section of the side panel.
pub fn panes(c: &Chat) -> Vec<Node> {
    let mods = &c.session.mods;
    mods.panes
        .iter()
        .filter_map(|pane| {
            let tree = drawn(mods.trees.get(&pane.id)?, &At { instance: &pane.id, drafts: &mods.drafts })?;
            Some(stack().prop("gap", "xs").child(muted(pane.title.as_str()).prop("size", "small")).child(tree).key(format!("pane-{}", pane.id)))
        })
        .collect()
}

/// A mod's tree as the chat's nodes, for `at`, whose buttons it presses. None for one that shows nothing.
fn drawn(tree: &Value, at: &At) -> Option<Node> {
    let mut budget = MOST;
    node(tree, at, &mut budget)
}

/// The elements and strings among `children`, as a list may hold them nested.
fn flat(children: &Value) -> Vec<&Value> {
    match children {
        Value::Array(list) => list.iter().flat_map(flat).collect(),
        Value::Null | Value::Bool(_) => Vec::new(),
        one => vec![one],
    }
}

fn node(el: &Value, at: &At, budget: &mut usize) -> Option<Node> {
    let instance = at.instance;
    if *budget == 0 {
        return None;
    }
    *budget -= 1;
    if let Some(said) = el.as_str() {
        return Some(text(said));
    }
    if el.is_number() {
        return Some(text(el.to_string()));
    }
    let props = &el["props"];
    let children = flat(if el["children"].is_null() { &props["children"] } else { &el["children"] });
    let inner = |budget: &mut usize| children.iter().filter_map(|child| node(child, at, budget)).collect::<Vec<Node>>();
    match el["type"].as_str()? {
        "engine" => None,
        "Box" | "div" => {
            let column = props["flexDirection"].as_str().map_or(el["type"] == "div", |direction| direction.starts_with("column"));
            let gap = match props["gap"].as_f64().or(props[if column { "rowGap" } else { "columnGap" }].as_f64()) {
                Some(cells) if cells >= 2.0 => "m",
                Some(cells) if cells >= 1.0 => "s",
                _ => "xs",
            };
            let mut node = if column { stack() } else { row().prop("wrap", props["flexWrap"] == "wrap") }.prop("gap", gap);
            if props["alignItems"] == "center" {
                node = node.prop("align", "center");
            }
            if props["justifyContent"] == "space-between" {
                node = node.prop("justify", "between");
            }
            Some(node.children(inner(budget)))
        }
        "Text" | "span" | "b" => {
            let styled = |node: Node| {
                let node = if el["type"] == "b" || props["bold"] == true { node.prop("weight", "strong") } else { node };
                match tone(props) {
                    Some(tone) => node.prop("tone", tone),
                    None => node,
                }
            };
            if children.iter().all(|child| child.is_string() || child.is_number()) {
                let said: String = children.iter().map(|child| child.as_str().map_or_else(|| child.to_string(), str::to_owned)).collect();
                return Some(styled(text(said)));
            }
            Some(row().prop("gap", "none").prop("wrap", true).children(inner(budget).into_iter().map(styled)))
        }
        "Button" => {
            let label = props["label"].as_str().map(str::to_owned).unwrap_or_else(|| words(&children));
            let press = el["press"].clone();
            let key = props["key"].as_str().map(str::to_owned);
            let instance = instance.to_owned();
            let variant = if props["variant"] == "primary" { "default" } else { "outline" };
            let pressed = button(label, variant, false, move || {
                let (instance, press, key) = (instance.clone(), press.clone(), key.clone());
                chat::write(move |_, n| crate::mods::press(&instance, &press, key.as_deref(), n));
            });
            // In a row, so it is as wide as its label, as in Claude Code, rather than the whole column.
            Some(row().child(pressed.prop("size", "sm")))
        }
        "Link" => {
            let href = props["href"].as_str()?.to_owned();
            let label = props["label"].as_str().map(str::to_owned).unwrap_or_else(|| Some(words(&children)).filter(|said| !said.is_empty()).unwrap_or_else(|| href.clone()));
            Some(text(label).prop("tone", "info").prop("title", href.as_str()).on("onClick", move |_| host::open_url(&href)))
        }
        // A field sends what you typed when you press Enter, as a mod's `onSubmit` hears it.
        "Input" => {
            let key = props["key"].as_str()?.to_owned();
            let draft = format!("{instance}/{key}");
            let typed = at.drafts.get(&draft).cloned().unwrap_or_default();
            let label = props["placeholder"].as_str().unwrap_or(&key).to_owned();
            let (kept, sent, press, instance) = (draft.clone(), draft, el["press"].clone(), instance.to_owned());
            let field = text_input(&label, &typed, move |said| chat::with(|c| drop(c.session.mods.drafts.insert(kept.clone(), said))))
                .prop("placeholder", label.as_str())
                .on("onKeyDown", move |event| {
                    if event["key"] != "Enter" {
                        return;
                    }
                    let said = chat::with(|c| c.session.mods.drafts.remove(&sent)).unwrap_or_default();
                    let (instance, press, key) = (instance.clone(), press.clone(), key.clone());
                    chat::write(move |_, n| crate::mods::input(&instance, &press, &key, &said, n));
                });
            Some(field)
        }
        "Select" => {
            let key = props["key"].as_str()?.to_owned();
            let options: Vec<(String, String)> = props["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    let value = option["value"].as_str()?.to_owned();
                    let label = option["label"].as_str().unwrap_or(&value).to_owned();
                    Some((value, label))
                })
                .collect();
            let (press, instance) = (el["press"].clone(), instance.to_owned());
            let label = props["label"].as_str().unwrap_or(&key).to_owned();
            Some(select(&label, &options, props["value"].as_str().unwrap_or_default(), move |value| {
                let (instance, press, key, value) = (instance.clone(), press.clone(), key.clone(), value.to_owned());
                chat::write(move |_, n| crate::mods::select(&instance, &press, &key, &value, n));
            }))
        }
        "Code" => Some(highlighted_code(props["source"].as_str()?, props["language"].as_str().unwrap_or_default())),
        "Markdown" => {
            let shown = markdown(props["text"].as_str()?);
            Some(if props["dimColor"] == true { shown.prop("tone", "muted") } else { shown })
        }
        _ => Some(muted("This part shows only in Claude Code.").prop("size", "small")),
    }
}

/// The words of what an element holds.
fn words(children: &[&Value]) -> String {
    children.iter().filter_map(|child| child.as_str()).collect()
}

/// A text's tone from its terminal colours.
fn tone(props: &Value) -> Option<&'static str> {
    if props["dimColor"] == true {
        return Some("muted");
    }
    match props["color"].as_str()?.trim_end_matches("Bright").to_ascii_lowercase().as_str() {
        "green" | "success" => Some("success"),
        "red" | "error" => Some("danger"),
        "yellow" | "warning" => Some("warn"),
        "blue" | "cyan" | "magenta" | "suggestion" => Some("info"),
        "gray" | "grey" | "subtle" => Some("muted"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn at(instance: &str) -> At<'_> {
        static NONE: std::sync::LazyLock<HashMap<String, String>> = std::sync::LazyLock::new(HashMap::new);
        At { instance, drafts: &NONE }
    }

    #[test]
    fn a_mods_tree_becomes_the_chats_rows_texts_and_buttons() {
        // What the repo-docs plugin draws for its reindex call, and a pane with a button, as Claude Code sends them.
        let row_tree = json!({ "type": "Box", "props": { "marginX": 1, "gap": 1 }, "children": [
            { "type": "Text", "props": { "color": "gray" }, "children": ["●"] },
            { "type": "Text", "props": { "bold": true }, "children": ["Reindex repo docs"] },
            { "type": "Text", "props": { "dimColor": true }, "children": ["embedding changed docs…"] }] });
        let drawn = drawn(&row_tree, &at("x")).unwrap();
        assert_eq!(drawn.count(), 7, "a row of three texts, each with its words");
        let pane = json!({ "type": "Box", "props": { "flexDirection": "column" }, "children": [
            { "type": "Text", "children": ["Probe pane on ", "desktop"] },
            { "type": "Button", "props": { "key": "ping", "label": "Ping" }, "press": { "plugin": "wings-probe", "handle": 100025600 } },
            { "type": "Client", "props": { "module": "./frame.js" } },
            { "type": "engine", "ref": 2 }] });
        let drawn = super::drawn(&pane, &at("probe")).unwrap();
        // A stack of a text, a button with its label in a row and a line for the frame; Claude Code's own part is left out.
        assert_eq!(drawn.count(), 1 + 2 + 3 + 2);
        assert!(super::drawn(&json!({ "type": "engine", "ref": 0 }), &at("x")).is_none());
        // A tree too big for the page is cut off.
        let many = json!({ "type": "Box", "children": (0..1000).map(|n| json!({ "type": "Text", "children": [n.to_string()] })).collect::<Vec<_>>() });
        assert!(super::drawn(&many, &at("x")).unwrap().count() <= 2 * MOST);
    }

    #[test]
    fn a_mods_field_and_dropdown_become_the_chats() {
        // As a probe mod's pane came from Claude Code.
        let pane = json!({ "type": "Box", "props": { "flexDirection": "column" }, "children": [
            { "type": "Input", "props": { "key": "name", "placeholder": "Name" }, "press": { "plugin": "probe-a", "handle": 41726026 } },
            { "type": "Select", "props": { "key": "pick", "options": [{ "value": "1", "label": "One" }, { "value": "2", "label": "Two" }] }, "press": { "plugin": "probe-a", "handle": 41726027 } }] });
        let drafts = HashMap::from([("form/name".to_owned(), "An".to_owned())]);
        let drawn = super::drawn(&pane, &At { instance: "form", drafts: &drafts }).unwrap();
        assert_eq!(drawn.count(), 3, "a stack of a field and a dropdown, none of them Claude Code's line");
    }

    #[test]
    fn a_menus_rows_are_its_buttons_and_https_links_as_claude_code_sent_them() {
        // What a probe mod answered a right-click on a link with.
        let tree = json!({ "type": "Box", "props": { "flexDirection": "column" }, "children": [
            { "type": "Button", "props": { "key": "sum", "label": "Summarize this page" }, "press": { "plugin": "probe-a", "handle": 723257875 },
              "children": [{ "type": "Text", "props": { "dimColor": true }, "children": ["into the message box"] }] },
            { "type": "Link", "props": { "href": "https://example.com/docs", "label": "Docs" } },
            { "type": "Link", "props": { "href": "http://plain.example.com" } }] });
        let rows = menu_rows(&tree);
        assert_eq!(rows.len(), 2, "a button and the https link; a plain http one is left out");
    }

    #[test]
    fn a_right_click_on_selected_words_offers_copy() {
        chat::with(|c| {
            c.session.mods.menu = Some(json!({ "x": 10, "y": 20, "selection": { "text": "picked words" } }));
            assert!(menu(c).is_some(), "Copy for the selection");
            c.session.mods.menu = Some(json!({ "x": 10, "y": 20, "selection": { "text": "" } }));
            assert!(menu(c).is_none(), "nothing selected, nothing to copy");
        });
    }

    #[test]
    fn a_tool_row_is_asked_for_with_what_claude_codes_own_row_reads() {
        chat::with(|c| {
            c.session.items = vec![Item::Tool(Tool {
                id: "t1".into(),
                name: "Bash".into(),
                summary: "ls".into(),
                output: Some(("a\nb".into(), false)),
                detail: crate::session::Detail::Plain,
                images: Vec::new(),
            })];
            c.session.mods.tool_called("t1", &json!({ "command": "ls" }));
            let asked: Value = serde_json::from_str(request_for(c, "use:t1", 4).unwrap().trim()).unwrap();
            assert_eq!(asked["request"]["props"], json!({ "tool": "Bash", "input": { "command": "ls" }, "isRunning": false, "isErrored": false, "isInterrupted": false, "output": "a\nb" }));
            let asked: Value = serde_json::from_str(request_for(c, "result:t1", 5).unwrap().trim()).unwrap();
            assert_eq!((&asked["request"]["component"], &asked["request"]["instance_id"]), (&json!("ToolResult"), &json!("t1")));
            // A call no longer in the conversation isn't asked for.
            assert!(request_for(c, "use:gone", 6).is_none());
        });
    }

    #[test]
    fn a_plugins_fill_goes_into_the_message_box_and_a_read_gets_it() {
        chat::with(|c| {
            c.draft = "Hi ".into();
            let filled: Value = serde_json::from_str(answer_ask(c, "q1", &json!({ "subtype": "ui_prompt_fill", "text": "there", "mode": "insert" })).trim()).unwrap();
            assert_eq!((c.draft.as_str(), &filled["response"]["response"]), ("Hi there", &json!({ "filled": true })));
            answer_ask(c, "q2", &json!({ "subtype": "ui_prompt_fill", "text": "New", "mode": "replace" }));
            let read: Value = serde_json::from_str(answer_ask(c, "q3", &json!({ "subtype": "ui_prompt_read" })).trim()).unwrap();
            assert_eq!(read["response"]["response"], json!({ "text": "New", "cursor": 3 }));
        });
    }

    #[test]
    fn messages_and_the_working_line_are_asked_for_and_take_what_a_mod_made_of_them() {
        chat::with(|c| {
            c.session.items = vec![Item::User("Fix the build".into()), Item::Text("Done.".into())];
            let yours = message_key(YOURS, 0, "Fix the build");
            let asked: Value = serde_json::from_str(request_for(c, &yours, 1).unwrap().trim()).unwrap();
            assert_eq!((&asked["request"]["component"], &asked["request"]["props"]), (&json!("UserMessage"), &json!({ "text": "Fix the build", "origin": { "kind": "human" }, "isExpanded": false })));
            let claudes = message_key(CLAUDES, 1, "Done.");
            let asked: Value = serde_json::from_str(request_for(c, &claudes, 2).unwrap().trim()).unwrap();
            assert_eq!(asked["request"]["props"], json!({ "text": "Done.", "isFirstOfReply": true }));
            // After a rewind another message stands there: the old key asks for nothing.
            c.session.items[1] = Item::Text("Something else".into());
            assert!(request_for(c, &claudes, 3).is_none());
            // As Claude Code answered a probe mod that reworded Claude's message and drew yours.
            let reworded = message_key(CLAUDES, 1, "Something else");
            c.session.mods.answered(&format!("ui-render-{reworded}-4"), &json!({ "subtype": "success", "response": { "tree": { "type": "engine", "ref": 1 }, "props": { "text": "✨ Something else", "isFirstOfReply": true }, "rewritten": true } }));
            assert_eq!(message(c, 1, &c.session.items[1]).1.as_deref(), Some("✨ Something else"));
            c.session.mods.answered(&format!("ui-render-{yours}-5"), &json!({ "subtype": "success", "response": { "tree": { "type": "Text", "props": { "color": "cyan" }, "children": ["you said: ", "Fix the build"] } } }));
            assert!(message(c, 0, &c.session.items[0]).0.is_some());
            // The working line goes with what the chat says it does, and takes a mod's suffix.
            c.doing = ("Running tests".into(), 0);
            let asked: Value = serde_json::from_str(request_for(c, SPINNER, 6).unwrap().trim()).unwrap();
            assert_eq!(asked["request"]["props"], json!({ "word": "Running tests", "message": null, "suffix": "", "mode": "responding" }));
            c.session.mods.answered("ui-render-spinner-6", &json!({ "subtype": "success", "response": { "tree": { "type": "engine", "ref": 1 }, "props": { "word": "Running tests", "message": null, "suffix": " · 3 tool calls", "mode": "responding" }, "rewritten": true } }));
            assert_eq!(spinner(c).1.as_deref(), Some("Running tests · 3 tool calls"));
        });
    }

    #[test]
    fn a_question_and_a_run_of_calls_are_asked_for_with_claude_codes_props() {
        chat::with(|c| {
            let tool = |id: &str, output: Option<&str>| Item::Tool(Tool { id: id.into(), name: "Bash".into(), summary: String::new(), output: output.map(|said| (said.to_owned(), false)), detail: crate::session::Detail::Plain, images: Vec::new() });
            c.session.items = vec![Item::User("Go".into()), tool("t1", Some("ok")), tool("t2", None)];
            c.session.mods.tool_called("t1", &json!({ "command": "ls" }));
            let asked: Value = serde_json::from_str(request_for(c, "group:1:2", 1).unwrap().trim()).unwrap();
            assert_eq!(asked["request"]["component"], "ToolGroup");
            assert_eq!(asked["request"]["props"]["calls"][0], json!({ "tool": "Bash", "input": { "command": "ls" }, "isRunning": false, "isErrored": false, "isInterrupted": false }));
            assert_eq!(asked["request"]["props"]["isActive"], true);
            // A run that isn't there as it was asked for asks for nothing.
            assert!(request_for(c, "group:1:3", 2).is_none());
            c.session.items.push(Item::Ask(crate::session::Ask {
                request: "r1".into(),
                tool: "AskUserQuestion".into(),
                summary: String::new(),
                detail: String::new(),
                cut_short: false,
                reason: None,
                suggestions: Vec::new(),
                input: json!({ "questions": [{ "question": "Which?", "options": [{ "label": "A" }] }] }),
                answer: None,
            }));
            let asked: Value = serde_json::from_str(request_for(c, "question:r1", 3).unwrap().trim()).unwrap();
            assert_eq!((&asked["request"]["component"], &asked["request"]["instance_id"]), (&json!("AskUserQuestion"), &json!("r1")));
            assert_eq!(asked["request"]["props"]["questions"][0]["question"], "Which?");
            // What a mod adds above the question, without Claude Code's own part.
            c.session.mods.answered("ui-render-question:r1-3", &json!({ "subtype": "success", "response": { "tree": { "type": "Box", "props": { "flexDirection": "column" }, "children": [{ "type": "Text", "children": ["Pick carefully"] }, { "type": "engine", "ref": 1 }] } } }));
            assert_eq!(question(c, "r1").map(|node| node.count()), Some(3), "a stack of the mod's text");
        });
    }

    #[test]
    fn a_mods_look_paints_the_chat_in_its_dark_colours_on_its_ground() {
        chat::with(|c| {
            assert!(palette(c).is_none(), "no look, the app's colours");
            // As Claude Code answered for a look made with `makeColors`, cut to the colours read here.
            let dark = json!({ "gray-900": "#1f1a15", "gray-850": "#28221b", "gray-830": "#312a22", "gray-800": "#3a3229", "gray-50": "#f0e7d7", "gray-400": "#aca08d", "role-accent-250": "#93b7d9", "role-danger-250": "#e89a92", "role-success-250": "#9cc58a", "role-warning-250": "#f6cf7e" });
            c.session.mods.answered("ui-render-theme-4", &json!({ "subtype": "success", "response": { "tree": { "type": "engine", "ref": 0 }, "props": { "plugin": "paper", "colors": { "v": 1, "label": "Paper", "inputs": { "dark": dark } }, "backdrop": { "gradient": { "stops": ["surface-0", "#000000"], "angle": 90 } } }, "rewritten": false } }));
            let painted = palette(c).unwrap();
            assert_eq!((&painted["page"], &painted["text"], &painted["muted"], &painted["info"]), (&json!("#1f1a15"), &json!("#f0e7d7"), &json!("#aca08d"), &json!("#93b7d9")));
            assert_eq!(painted["ground"], json!({ "from": "#1f1a15", "to": "#000000", "angle": 90.0 }));
            // A look for light alone leaves the dark chat as it is.
            c.session.mods.answered("ui-render-theme-5", &json!({ "subtype": "success", "response": { "props": { "colors": { "v": 1, "label": "Day", "inputs": { "light": { "gray-0": "#ffffff" } } } } } }));
            assert!(palette(c).is_none());
            c.session.mods.theme = None;
        });
    }

    #[test]
    fn colours_read_as_tones() {
        assert_eq!(tone(&json!({ "color": "greenBright" })), Some("success"));
        assert_eq!(tone(&json!({ "color": "red", "dimColor": true })), Some("muted"));
        assert_eq!(tone(&json!({ "color": "#ff8800" })), None);
    }
}
