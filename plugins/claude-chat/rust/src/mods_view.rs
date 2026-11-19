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

use crate::{
    chat::{self, Chat},
    mods::{BAND, HINT, MODES},
};

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
    fn colours_read_as_tones() {
        assert_eq!(tone(&json!({ "color": "greenBright" })), Some("success"));
        assert_eq!(tone(&json!({ "color": "red", "dimColor": true })), Some("muted"));
        assert_eq!(tone(&json!({ "color": "#ff8800" })), None);
    }
}
