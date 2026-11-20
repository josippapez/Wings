//! A pane a Claude Code plugin opened (`mods.rs`), shown in Wings' sidebar as a page of its own (`contributes.sidebars`,
//! `--features pane`), where it has the room a pane needs: in the chat's side panel the Feedback pane's header and filters
//! were squeezed a word or two a line. The chat keeps the pane's tree, as Claude Code draws it, and tells the sidebar it
//! whenever it changes; the sidebar draws it with the chat's own translation (`mods_view.rs`) and hands what you press,
//! submit or pick back to the chat with your click or key, as the one write the chat then makes.
//!
//! The sidebar takes the name `mod-pane` while it shows nothing, and `mod-pane:<session>:<pane's hash>` for a pane of a
//! session, so the chat of that session finds it among the plugin's pages.

use std::{
    cell::RefCell,
    collections::HashMap,
};

use serde_json::{Value, json};
use wings_plugin::{
    host, pages,
    ui::{Node, empty, loading, page, toolbar},
};

use crate::{chat, mods, subagents};

/// The id of the plugin's sidebar for a pane (`contributes.sidebars`).
pub const SIDEBAR: &str = "pane";
/// The name the sidebar takes while it shows no pane, and the start of the names it takes for one.
pub const IDLE: &str = "mod-pane";

/// The name the sidebar takes while it shows `pane` of `session`. A pane's id is Claude Code's, which may hold what a
/// page's name can't, so the name carries its hash.
pub fn page_name(session: &str, pane: &str) -> String {
    let hash = pane.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    format!("{IDLE}:{session}:{hash:x}")
}

/// The sidebar's name among the names the plugin's pages have, whatever it shows.
fn sidebar_of(names: &[String]) -> Option<&str> {
    names.iter().map(String::as_str).find(|name| *name == IDLE || name.starts_with(&format!("{IDLE}:")))
}

/// What the chat tells the sidebar to show: `pane` of `session`.
pub fn show_message(session: &str, pane: &str) -> Value {
    json!({ "kind": "mod-show", "session": session, "pane": pane })
}

/// What the chat tells the sidebar of the pane it shows: its title and tree, or that it closed.
fn pane_message(c: &chat::Chat, pane: &str) -> Value {
    match c.session.mods.panes.iter().find(|open| open.id == pane) {
        Some(open) => json!({ "kind": "mod-pane", "pane": pane, "open": true, "title": open.title, "tree": c.session.mods.trees.get(pane).map_or(Value::Null, crate::mods_view::fit) }),
        None => json!({ "kind": "mod-pane", "pane": pane, "open": false }),
    }
}

/// What a press, a submit, an edit or a pick in a mod's tree is: `act` (`press`, `input`, `change`, `select`, `link`) of
/// `instance`, with the `press` handle and `key` the element came with, and the `value` typed, picked or linked to.
pub fn act_message(instance: &str, act: &str, press: &Value, key: Option<&str>, value: Option<&str>) -> Value {
    json!({ "kind": "mod-act", "pane": instance, "act": act, "press": press, "key": key, "value": value })
}

/// The request that tells Claude Code of an `act_message`, numbered `n`.
pub fn request(message: &Value, n: u32) -> Option<String> {
    let (instance, press, key) = (message["pane"].as_str()?, &message["press"], message["key"].as_str());
    match message["act"].as_str()? {
        "press" => mods::press(instance, press, key, n),
        "input" => mods::input(instance, press, key?, message["value"].as_str()?, n),
        "select" => mods::select(instance, press, key?, message["value"].as_str()?, n),
        "change" => mods::input_change(instance, press, key?, message["value"].as_str()?, n),
        "link" => mods::press_link(instance, press, key, message["value"].as_str()?, n),
        _ => None,
    }
}

/// A press, a submit or a pick in a mod's tree: in the chat, Claude Code hears it now; in the sidebar, the chat is handed
/// it with your click or key.
pub fn act(message: Value) {
    match with_side(|side| subagents::chat_page(&side.session)) {
        Some(chat_page) => drop(pages::tell(&chat_page, &message, true)),
        None => chat::write(move |_, n| request(&message, n)),
    }
}

// ---------- the chat's side ----------

#[derive(Default)]
struct Relay {
    /// The names the plugin's pages have now.
    names: Vec<String>,
    /// What each page that shows a pane was last told of it, by page name, so it is told only what changed.
    told: HashMap<String, Value>,
    /// What the sidebar is to show, until its page is there to be told.
    show: Option<Value>,
    /// The pane the sidebar shows, with its page's name and whether that page has been among the names yet: its page
    /// going away after is you closing it.
    watch: Option<(String, String, bool)>,
}

thread_local! {
    static RELAY: RefCell<Relay> = RefCell::new(Relay::default());
}

/// Hears which pages are open and what the sidebar hands back. Called as the chat starts.
pub fn start() {
    pages::on_names(names_changed);
    pages::on_told(told);
}

fn names_changed(names: &[String]) {
    let closed = RELAY.with(|relay| {
        let mut relay = relay.borrow_mut();
        relay.names = names.to_vec();
        let (pane, name, seen) = relay.watch.as_mut()?;
        if names.contains(name) {
            *seen = true;
            return None;
        }
        let closed = seen.then(|| pane.clone());
        if closed.is_some() {
            relay.watch = None;
        }
        closed
    });
    // Claude Code hears you closed it, if it is still open there.
    if let Some(pane) = closed {
        chat::write(move |c, n| c.session.mods.panes.iter().any(|open| open.id == pane).then(|| mods::close(&pane, n)));
    }
    send_show();
    sync();
}

/// A click on a pane's row in the side panel, or Claude Code asking to show it: Wings' sidebar shows it, in place of the
/// pane it showed. The sidebar is told once its page is there.
pub fn open(pane: &str) {
    let shown = chat::with(|c| {
        let session = c.session.id.as_deref()?;
        c.session.mods.panes.iter().any(|open| open.id == pane).then(|| show_message(session, pane))
    });
    let Some(message) = shown else {
        return;
    };
    let name = chat::with(|c| c.session.id.as_deref().map(|session| page_name(session, pane))).unwrap_or_default();
    RELAY.with(|relay| {
        let mut relay = relay.borrow_mut();
        relay.show = Some(message);
        // The sidebar changing to this pane isn't a close of the last one.
        relay.watch = Some((pane.to_owned(), name, false));
    });
    // Claude Code hears which pane is shown.
    let shown = pane.to_owned();
    chat::write(move |_, n| Some(mods::pane_show(&shown, n)));
    crate::session_side::covered();
    drop(host::open_view_with(SIDEBAR, "sidebar", &Value::Null));
    send_show();
}

fn send_show() {
    let sent = RELAY.with(|relay| {
        let mut relay = relay.borrow_mut();
        let name = sidebar_of(&relay.names)?.to_owned();
        Some((name, relay.show.take()?))
    });
    if let Some((name, message)) = sent {
        drop(pages::tell(&name, &message, false));
    }
}

/// Tells each page that shows a pane of the chat's session what the pane shows now, when that changed.
pub fn sync() {
    let Some(session) = chat::with(|c| c.session.id.clone()) else {
        return;
    };
    let names = RELAY.with(|relay| relay.borrow().names.clone());
    let messages: Vec<(String, Value)> = chat::with(|c| {
        names
            .iter()
            .filter(|name| name.starts_with(&format!("{IDLE}:{session}:")))
            .filter_map(|name| {
                // The pane the page shows, by its hash; one closed since is told so.
                let pane = c.session.mods.panes.iter().find(|open| page_name(&session, &open.id) == *name);
                Some((name.clone(), match pane {
                    Some(open) => pane_message(c, &open.id),
                    None => json!({ "kind": "mod-pane", "open": false }),
                }))
            })
            .collect()
    });
    for (page, message) in &messages {
        let changed = RELAY.with(|relay| {
            let mut relay = relay.borrow_mut();
            let changed = relay.told.get(page) != Some(message);
            if changed {
                relay.told.insert(page.clone(), message.clone());
            }
            changed
        });
        if changed {
            drop(pages::tell(page, message, false));
        }
    }
    // A page that closed, or went on to another pane, is told again if it shows this one again.
    RELAY.with(|relay| relay.borrow_mut().told.retain(|page, _| messages.iter().any(|(open, _)| open == page)));
}

/// What the sidebar hands the chat: a press, a submit or a pick in a pane that is still open, which Claude Code hears.
fn told(message: &Value, _from: Option<&str>, write: bool) {
    if let Some(message) = taken(message, write) {
        chat::write(move |_, n| request(&message, n));
    }
}

/// The act the chat writes of what the sidebar handed it: only with `write`, the click or key in the sidebar that Wings
/// handed on with it, and only for a pane still open.
fn taken(message: &Value, write: bool) -> Option<Value> {
    if !write || message["kind"] != "mod-act" {
        return None;
    }
    let pane = message["pane"].as_str()?;
    chat::with(|c| c.session.mods.panes.iter().any(|open| open.id == pane)).then(|| message.clone())
}

// ---------- the sidebar's side ----------

/// What the sidebar shows: a pane of a session, empty (an empty `session`) until the chat says which.
#[derive(Debug, Default)]
pub struct Side {
    pub session: String,
    pub pane: String,
    pub title: String,
    /// The pane's tree, null until the chat has told it.
    pub tree: Value,
    /// Whether the chat said the pane closed.
    pub closed: bool,
}

thread_local! {
    static SIDE: RefCell<Option<Side>> = const { RefCell::new(None) };
}

fn with_side<T>(read: impl FnOnce(&Side) -> T) -> Option<T> {
    SIDE.with(|side| side.borrow().as_ref().filter(|side| !side.session.is_empty()).map(read))
}

/// Runs the page as Wings' sidebar for a pane: empty until the chat's `mod-show` says which.
pub fn start_sidebar() {
    SIDE.with(|side| *side.borrow_mut() = Some(Side::default()));
    wings_plugin::run(view);
    drop(pages::name(Some(IDLE)));
    pages::on_told(|message, _, _| told_side(message));
}

/// What the chat tells the sidebar: which pane to show, and what it shows.
fn told_side(message: &Value) {
    match message["kind"].as_str() {
        Some("mod-show") => {
            let (Some(session), Some(pane)) = (message["session"].as_str().filter(|id| !id.is_empty()), message["pane"].as_str().filter(|id| !id.is_empty())) else {
                return;
            };
            let same = SIDE.with(|side| side.borrow().as_ref().is_some_and(|side| side.session == session && side.pane == pane));
            if same {
                return;
            }
            SIDE.with(|side| *side.borrow_mut() = Some(Side { session: session.to_owned(), pane: pane.to_owned(), title: pane.to_owned(), ..Side::default() }));
            // What you typed in the last pane's fields isn't this one's.
            chat::with(|c| c.session.mods.drafts.clear());
            drop(pages::name(Some(&page_name(session, pane))));
        }
        Some("mod-pane") => SIDE.with(|side| {
            let mut side = side.borrow_mut();
            let Some(side) = side.as_mut().filter(|side| !side.session.is_empty()) else {
                return;
            };
            if message["open"] != true {
                side.closed = true;
                return;
            }
            if message["pane"].as_str() != Some(side.pane.as_str()) {
                return;
            }
            side.closed = false;
            side.title = message["title"].as_str().unwrap_or(&side.pane).to_owned();
            side.tree = message["tree"].clone();
        }),
        _ => {}
    }
}

/// The sidebar's page: the pane's tree under a toolbar with its title, scrolling in its frame.
pub fn view() -> Vec<Node> {
    let shown = SIDE.with(|side| side.borrow().as_ref().map(|side| (side.session.clone(), side.pane.clone(), side.title.clone(), side.tree.clone(), side.closed)));
    let Some((session, pane, title, tree, closed)) = shown else {
        return Vec::new();
    };
    if session.is_empty() {
        return vec![page(toolbar("Pane", None), [empty("No pane open", "Open one from a Claude chat's side panel.")])];
    }
    let body = if closed {
        empty("This pane was closed", "The plugin that opened it closed it.")
    } else if tree.is_null() {
        loading()
    } else {
        chat::with(|c| crate::mods_view::pane_body(&pane, &title, &tree, &c.session.mods.drafts)).unwrap_or_else(|| empty("Nothing to show", "The plugin draws nothing in this pane now."))
    };
    vec![page(toolbar(&title, None), [body])]
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::mods::Pane;

    fn record() -> Rc<RefCell<Vec<(String, Value)>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        calls
    }

    fn tells(calls: &Rc<RefCell<Vec<(String, Value)>>>) -> Vec<(String, Value, bool)> {
        calls
            .borrow()
            .iter()
            .filter(|(method, _)| method == "tellPage")
            .map(|(_, params)| (params["name"].as_str().unwrap().to_owned(), params["message"].clone(), params["write"] == true))
            .collect()
    }

    /// The Feedback pane's header, cut down, as Claude Code draws it.
    fn feedback_tree() -> Value {
        json!({ "type": "Box", "props": { "flexDirection": "column" }, "children": [
            { "type": "Box", "props": { "gap": 2 }, "children": [
                { "type": "Text", "props": { "bold": true }, "children": ["Feedback"] },
                { "type": "Button", "props": { "key": "close-pane", "plain": true, "label": "✕ Close" }, "press": { "plugin": "feedback", "handle": 21 } }] },
            { "type": "Text", "props": { "dimColor": true }, "children": ["88 open"] }] })
    }

    /// A chat whose session `s1` has the Feedback pane open with its tree.
    fn chat_with_feedback() {
        chat::with(|c| {
            c.session.id = Some("s1".into());
            c.session.mods.panes = vec![Pane { id: "feedback".into(), title: "Feedback".into(), plugin: "feedback".into() }];
            c.session.mods.trees.insert("feedback".into(), feedback_tree());
        });
    }

    #[test]
    fn a_pane_opens_in_the_sidebar_which_is_fed_its_tree_and_told_when_it_closes() {
        let calls = record();
        chat_with_feedback();
        open("feedback");
        let opened: Vec<Value> = calls.borrow().iter().filter(|(method, _)| method == "openView").map(|(_, params)| params.clone()).collect();
        assert_eq!(opened, [json!({ "id": "pane", "as": "sidebar", "args": null, "newTab": false })]);
        // Told what to show once its page is there, and once only.
        names_changed(&[IDLE.to_owned()]);
        names_changed(&[IDLE.to_owned(), "chat:s1".to_owned()]);
        let shows: Vec<_> = tells(&calls).into_iter().filter(|(_, message, _)| message["kind"] == "mod-show").collect();
        assert_eq!(shows, [(IDLE.to_owned(), show_message("s1", "feedback"), false)]);
        // Named for the pane, it is fed the tree, and fed again only when it changes.
        let named = page_name("s1", "feedback");
        names_changed(&[named.clone()]);
        sync();
        let fed: Vec<_> = tells(&calls).into_iter().filter(|(_, message, _)| message["kind"] == "mod-pane").collect();
        assert_eq!(fed.len(), 1);
        assert_eq!((fed[0].0.as_str(), &fed[0].1["tree"], &fed[0].1["title"]), (named.as_str(), &feedback_tree(), &json!("Feedback")));
        // The plugin closed it: the sidebar is told so.
        chat::with(|c| c.session.mods.panes.clear());
        sync();
        let last = tells(&calls).into_iter().filter(|(_, message, _)| message["kind"] == "mod-pane").last().unwrap();
        assert_eq!(last.1["open"], false);
        // A pane that isn't open opens nothing.
        let before = calls.borrow().len();
        open("feedback");
        assert_eq!(calls.borrow().len(), before);
    }

    #[test]
    fn a_press_in_the_sidebar_comes_back_to_the_chat_as_claude_codes_press() {
        let calls = record();
        // The sidebar, told to show the pane and fed its tree.
        SIDE.with(|side| *side.borrow_mut() = Some(Side::default()));
        told_side(&show_message("s1", "feedback"));
        told_side(&json!({ "kind": "mod-pane", "pane": "feedback", "open": true, "title": "Feedback", "tree": feedback_tree() }));
        assert_eq!(with_side(|side| (side.title.clone(), side.tree == feedback_tree())), Some(("Feedback".into(), true)));
        // Its Close button hands the chat the press, with the click as the chat's one write.
        let message = act_message("feedback", "press", &json!({ "plugin": "feedback", "handle": 21 }), Some("close-pane"), None);
        act(message.clone());
        assert_eq!(tells(&calls), [("chat:s1".to_owned(), message.clone(), true)]);
        // The chat makes it Claude Code's `ui_press` for that pane.
        let line: Value = serde_json::from_str(request(&message, 7).unwrap().trim()).unwrap();
        assert_eq!(line["request"]["subtype"], "ui_press");
        assert_eq!((&line["request"]["plugin"], &line["request"]["handle"], &line["request"]["key"]), (&json!("feedback"), &json!(21), &json!("close-pane")));
        assert_eq!(line["request_id"], "ui-press-feedback-7");
        // A submit and a pick are Claude Code's too; anything else is nothing.
        let typed = act_message("feedback", "input", &json!({ "plugin": "feedback", "handle": 9 }), Some("note"), Some("hi"));
        assert!(request(&typed, 8).unwrap().contains("ui_input"));
        assert!(request(&json!({ "kind": "mod-act", "pane": "feedback", "act": "drop" }), 9).is_none());
        // Closed, the sidebar keeps no stale tree on screen.
        told_side(&json!({ "kind": "mod-pane", "open": false }));
        assert_eq!(with_side(|side| side.closed), Some(true));
        SIDE.with(|side| *side.borrow_mut() = None);
    }

    #[test]
    fn the_chat_takes_a_handed_press_only_with_its_write_and_for_an_open_pane() {
        record();
        chat_with_feedback();
        let message = act_message("feedback", "press", &json!({ "plugin": "feedback", "handle": 21 }), Some("close-pane"), None);
        assert_eq!(taken(&message, true), Some(message.clone()));
        assert_eq!(taken(&message, false), None, "not without the click handed on with it");
        assert_eq!(taken(&act_message("gone", "press", &json!({ "plugin": "feedback", "handle": 21 }), None, None), true), None, "not for a pane that closed");
        assert_eq!(taken(&show_message("s1", "feedback"), true), None);
    }

    /// The subtypes and ids of the requests the chat wrote to Claude Code.
    fn written(calls: &Rc<RefCell<Vec<(String, Value)>>>) -> Vec<(String, Value)> {
        wings_plugin::task::block_on(async {});
        calls
            .borrow()
            .iter()
            .filter(|(method, _)| method == "processWrite")
            .filter_map(|(_, params)| serde_json::from_str::<Value>(params["text"].as_str()?.trim()).ok())
            .map(|line| (line["request"]["subtype"].as_str().unwrap_or_default().to_owned(), line["request"]["id"].clone()))
            .collect()
    }

    #[test]
    fn showing_a_pane_and_closing_its_sidebar_are_told_to_claude_code_once() {
        let calls = record();
        chat_with_feedback();
        let process = host::spawn("claude", &[], "/work/shop", "", |_| {});
        chat::with(|c| c.input = Some(process.input()));
        open("feedback");
        assert_eq!(written(&calls), [("ui_pane_show".to_owned(), json!("feedback"))]);
        // The sidebar's page comes, then goes: you closed it.
        let named = page_name("s1", "feedback");
        names_changed(&[IDLE.to_owned()]);
        names_changed(&[named.clone()]);
        assert_eq!(written(&calls).len(), 1, "not before its page was there and went");
        names_changed(&[]);
        names_changed(&[]);
        assert_eq!(written(&calls)[1..], [("ui_close".to_owned(), json!("feedback"))]);
        // A pane Claude Code closed itself is not closed again.
        open("feedback");
        names_changed(&[named]);
        chat::with(|c| c.session.mods.panes.clear());
        names_changed(&[]);
        assert_eq!(written(&calls).iter().filter(|(subtype, _)| subtype == "ui_close").count(), 1);
    }
}
