//! The chat's session in Wings' right sidebar (`contributes.sidebars`, `--features session`): what the folder's working
//! tree changed, how full the context is and how much of your limits is used, the background tasks, the folders you
//! added, the pull requests and issues the conversation names, the panes a Claude Code plugin opened, the subagents and
//! the terminals Claude opened. It used to be a card floating over the chat's right side, which covered the conversation
//! in a narrow tab.
//!
//! The chat tab in front draws the sidebar's sections with its own code (`view::side`) and tells the sidebar them as JSON
//! (`ui::export`). The sidebar draws that JSON (`ui::import`) and hands each click back to the chat with your click, so it
//! runs there as a click in the chat would: a task's dialog opens over the chat, Stop writes to its Claude Code.
//!
//! This is one of the two places the panel can be (`prefs::session_panel`): the other is a card over the chat's right
//! side (`view::side`), which is the default. While it is there, none of this runs.
//!
//! The sidebar takes the name `session-side` while it follows no chat, and `session-side:<session>` while it shows that
//! session's chat. A chat tab that comes to the front (Wings tells a tab's page it is shown with its focus) asks the
//! sidebar to follow it, so with several chats open the sidebar shows the one you look at.

use std::{cell::RefCell, rc::Rc};

use serde_json::{Value, json};
use wings_plugin::{
    host, pages,
    ui::{self, Handlers, Node, empty, loading, muted, page, toolbar},
};

use crate::{chat, prefs, subagents};

/// The id of the plugin's sidebar for the session (`contributes.sidebars`).
pub const SIDEBAR: &str = "session";
/// The name the sidebar takes while it follows no chat, and the start of the names it takes for one.
pub const IDLE: &str = "session-side";

/// The name the sidebar takes while it shows `session`'s chat.
pub fn page_name(session: &str) -> String {
    format!("{IDLE}:{session}")
}

/// The session the sidebar follows for this chat: a new chat's from Claude Code's first lines, so its meters show before
/// you write.
fn followed(c: &chat::Chat) -> Option<String> {
    c.session.id.clone().or_else(|| c.session.announced.clone())
}

/// The sidebar's name among the names the plugin's pages have, whatever it shows.
fn sidebar_of(names: &[String]) -> Option<&str> {
    names.iter().map(String::as_str).find(|name| *name == IDLE || name.starts_with(&format!("{IDLE}:")))
}

/// What a chat tells the sidebar as it comes to the front: to follow its session, or, for a chat that hasn't started one,
/// to say so.
pub fn show_message(session: Option<&str>) -> Value {
    json!({ "kind": "session-show", "session": session })
}

/// What a subagent's sidebar tells the chat to have the session back in the sidebar.
pub fn reopen_message() -> Value {
    json!({ "kind": "session-reopen" })
}

// ---------- the chat's side ----------

#[derive(Default)]
struct Relay {
    /// The names the plugin's pages have now.
    names: Vec<String>,
    /// What the sidebar is to be told, until its page is there.
    show: Option<Value>,
    /// The session this tab last asked the sidebar to follow while in front, none while it is behind.
    wanted: Option<Option<String>>,
    /// What the sidebar was last told of this chat, so it is told only what changed.
    told: Option<Value>,
    /// Whether the sidebar shows, as it said when it last followed this chat.
    open: bool,
    /// The sidebar was opened for this tab once, as your setting asks.
    opened: bool,
}

thread_local! {
    static RELAY: RefCell<Relay> = RefCell::new(Relay::default());
    /// The handlers of what the sidebar was last told, which a click there runs.
    static HANDLERS: RefCell<Rc<Handlers>> = RefCell::new(Rc::new(Handlers::default()));
}

/// Hears which pages are open, what the sidebar hands back, and when the tab comes to the front. Called as the chat starts.
pub fn start() {
    pages::on_names(names_changed);
    pages::on_told(told);
    host::on_focus(focused);
    if host::focused() {
        focused(true);
    }
}

fn names_changed(names: &[String]) {
    let gone = RELAY.with(|relay| {
        let mut relay = relay.borrow_mut();
        relay.names = names.to_vec();
        // The sidebar went to another chat, or closed: it is told all again when it follows this one again.
        let followed = chat::with(|c| followed(c)).is_some_and(|session| names.contains(&page_name(&session)));
        if !followed {
            relay.told = None;
        }
        sidebar_of(names).is_none()
    });
    if gone {
        RELAY.with(|relay| relay.borrow_mut().open = false);
    }
    send_show();
}

/// The tab came to the front, or went behind another, or Wings did.
fn focused(focused: bool) {
    if focused {
        claim();
    } else {
        RELAY.with(|relay| {
            let mut relay = relay.borrow_mut();
            relay.wanted = None;
            relay.show = None;
        });
    }
}

/// Whether the panel is in Wings' sidebar, as you set for every chat.
fn in_wings() -> bool {
    chat::with(|c| c.side_in_wings)
}

/// Has the sidebar follow this chat. The first time, with your setting on, it opens it too.
fn claim() {
    if !in_wings() {
        return;
    }
    let (session, on) = chat::with(|c| (followed(c), c.side_open));
    let open = RELAY.with(|relay| {
        let mut relay = relay.borrow_mut();
        relay.wanted = Some(session.clone());
        relay.show = Some(show_message(session.as_deref()));
        let open = on && !relay.opened;
        relay.opened |= open;
        open
    });
    if open {
        drop(host::open_view_with(SIDEBAR, "sidebar", &Value::Null));
    }
    send_show();
}

/// Tells the sidebar what to follow, once its page is there.
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

/// After each line Claude Code printed: a chat in front that has its session now, as a new chat does after its first
/// message, or a new one after /clear, has the sidebar follow that.
pub fn sync() {
    let session = chat::with(|c| followed(c));
    let stale = RELAY.with(|relay| relay.borrow().wanted.as_ref().is_some_and(|wanted| *wanted != session));
    if stale && host::focused() {
        claim();
    }
}

/// Whether the sidebar shows this chat's session, for the header's button.
pub fn shown() -> bool {
    RELAY.with(|relay| relay.borrow().open)
}

/// The header's button: opens the sidebar on this chat unless it is the sidebar on screen, in which case it closes it, and
/// keeps the choice for every chat as the one that opens the sidebar with a chat.
pub fn toggle() {
    let open = !shown();
    chat::with(|c| c.side_open = open);
    drop(prefs::side_panel().set(open));
    if open {
        open_here();
    } else {
        RELAY.with(|relay| relay.borrow_mut().open = false);
        drop(host::close_view(SIDEBAR));
    }
}

/// Opens the sidebar on this chat, where another sidebar may have taken its place. Wings leaves one already on screen be.
fn open_here() {
    let session = chat::with(|c| followed(c));
    RELAY.with(|relay| {
        let mut relay = relay.borrow_mut();
        relay.opened = true;
        relay.show = Some(show_message(session.as_deref()));
        relay.wanted = Some(session);
        relay.open = true;
    });
    drop(host::open_view_with(SIDEBAR, "sidebar", &Value::Null));
    send_show();
}

/// This chat opened another sidebar (a subagent's, a plugin's pane), which takes the session's place in a lone sidebar:
/// the button is then to bring the session back, not to close what is gone. Wings tells the sidebar's page it isn't shown
/// too, but that comes after, and not when the other sidebar joins it as a split.
pub fn covered() {
    if in_wings() {
        RELAY.with(|relay| relay.borrow_mut().open = false);
    }
}

/// The setting moved the panel: into Wings' sidebar, where a chat in front opens it as one does when it starts; or into the
/// chat, where the sidebar this chat had open is closed and nothing is published to it.
pub fn moved(sidebar: bool) {
    if sidebar {
        RELAY.with(|relay| relay.borrow_mut().opened = false);
        if host::focused() {
            claim();
        }
        return;
    }
    let open = RELAY.with(|relay| {
        let mut relay = relay.borrow_mut();
        relay.wanted = None;
        relay.show = None;
        relay.told = None;
        relay.opened = false;
        std::mem::take(&mut relay.open)
    });
    if open {
        drop(host::close_view(SIDEBAR));
    }
}

/// What the chat tells the sidebar of itself while it follows this chat: its name, its folder and branch, its sections
/// with the handlers their clicks run, and the terminals Claude opened. Called as the chat's page is built, so it goes
/// with every change; it is sent only when it changed.
pub fn publish(c: &chat::Chat) {
    if !c.side_in_wings {
        return;
    }
    let Some(session) = followed(c) else {
        return;
    };
    let name = page_name(&session);
    if !RELAY.with(|relay| relay.borrow().names.contains(&name)) {
        return;
    }
    let (sections, handlers) = ui::export(crate::view::side_sections(c, false));
    HANDLERS.with(|slot| *slot.borrow_mut() = Rc::new(handlers));
    let message = state_message(c, sections);
    let changed = RELAY.with(|relay| {
        let mut relay = relay.borrow_mut();
        let changed = relay.told.as_ref() != Some(&message);
        if changed {
            relay.told = Some(message.clone());
        }
        changed
    });
    if changed {
        drop(pages::tell(&name, &message, false));
    }
}

fn state_message(c: &chat::Chat, sections: Value) -> Value {
    let place = match (c.folder.is_empty(), &c.branch) {
        (true, _) => None,
        (false, Some(branch)) => Some(format!("{} · {branch}", c.folder)),
        (false, None) => Some(c.folder.clone()),
    };
    let terminals: Vec<&str> = c.terminals.iter().rev().take(crate::view::TERMINALS_SHOWN).map(|terminal| terminal.pane.as_str()).collect();
    json!({ "kind": "session-state", "title": chat::name(c).unwrap_or_else(|| "Claude chat".into()), "place": place, "sections": sections, "terminals": terminals })
}

/// What the sidebar hands the chat: a click on what the chat drew there, or whether it shows.
fn told(message: &Value, from: Option<&str>, _write: bool) {
    let Some(session) = chat::with(|c| followed(c)) else {
        return;
    };
    // A subagent's sidebar asks for the session back; it isn't the session sidebar, so it comes before the check below.
    if message["kind"] == "session-reopen" {
        if in_wings() {
            open_here();
        }
        return;
    }
    if from != Some(page_name(&session).as_str()) {
        return;
    }
    match message["kind"].as_str() {
        Some("session-act") => {
            let Some(handler) = message["handler"].as_str() else {
                return;
            };
            let handlers = HANDLERS.with(|slot| slot.borrow().clone());
            handlers.call(handler, &message["payload"]);
        }
        // A sidebar loses its focus with the window too: only one put away while this chat is in front is closed.
        Some("session-open") if message["open"] == true || host::focused() => RELAY.with(|relay| relay.borrow_mut().open = message["open"] == true),
        _ => {}
    }
}

// ---------- the sidebar's side ----------

/// What the sidebar shows.
#[derive(Debug, Default, PartialEq)]
pub enum Followed {
    /// No chat has asked it yet.
    #[default]
    Nothing,
    /// A chat that hasn't started its session.
    New,
    Session(String),
}

#[derive(Debug, Default)]
pub struct Side {
    pub followed: Followed,
    /// What the chat last said of itself (`state_message`), null until it has.
    pub state: Value,
    /// The terminals Wings hands this page, those of the chat tab in front, by pane.
    pub terminals: Vec<String>,
    /// Whether the chat it follows had its page open since, so its closing sends the sidebar back to nothing.
    seen: bool,
}

thread_local! {
    static SIDE: RefCell<Option<Side>> = const { RefCell::new(None) };
}

/// Runs the page as Wings' sidebar for the session: empty until a chat says which.
pub fn start_sidebar() {
    SIDE.with(|side| *side.borrow_mut() = Some(Side::default()));
    wings_plugin::run(view);
    drop(pages::name(Some(IDLE)));
    pages::on_told(told_side);
    pages::on_names(side_names);
    host::on_focus(|_| report_open());
    host::on_terminals(|listed| {
        let panes = listed.iter().filter_map(|terminal| terminal["paneId"].as_str().map(str::to_owned)).collect();
        with_side(|side| side.terminals = panes);
    });
}

fn with_side<T>(change: impl FnOnce(&mut Side) -> T) -> Option<T> {
    SIDE.with(|side| side.borrow_mut().as_mut().map(change))
}

/// What a chat tells the sidebar: which chat to follow, and what that one shows.
fn told_side(message: &Value, from: Option<&str>, _write: bool) {
    match message["kind"].as_str() {
        Some("session-show") => {
            let followed = match message["session"].as_str() {
                Some(session) if !session.is_empty() => Followed::Session(session.to_owned()),
                _ => Followed::New,
            };
            let changed = with_side(|side| {
                if side.followed == followed {
                    return false;
                }
                *side = Side { followed, terminals: std::mem::take(&mut side.terminals), ..Side::default() };
                true
            });
            if changed == Some(true) {
                let name = with_side(|side| match &side.followed {
                    Followed::Session(session) => page_name(session),
                    _ => IDLE.to_owned(),
                });
                drop(pages::name(name.as_deref()));
                report_open();
            }
        }
        Some("session-state") => {
            with_side(|side| {
                if let Followed::Session(session) = &side.followed
                    && from == Some(subagents::chat_page(session).as_str())
                {
                    side.state = message.clone();
                }
            });
        }
        _ => {}
    }
}

/// The chat it follows closed: nothing to show until another says.
fn side_names(names: &[String]) {
    let closed = with_side(|side| {
        let Followed::Session(session) = &side.followed else {
            return false;
        };
        let open = names.contains(&subagents::chat_page(session));
        side.seen |= open;
        side.seen && !open
    });
    if closed == Some(true) {
        with_side(|side| *side = Side { terminals: std::mem::take(&mut side.terminals), ..Side::default() });
        drop(pages::name(Some(IDLE)));
    }
}

/// The session of the chat the sidebar follows.
fn followed_session() -> Option<String> {
    with_side(|side| match &side.followed {
        Followed::Session(session) => Some(session.clone()),
        _ => None,
    })
    .flatten()
}

/// Tells the chat it follows whether the sidebar shows, for its header's button. Wings tells a sidebar's page it is
/// focused while it is in the panel and the window is in front.
fn report_open() {
    if let Some(session) = followed_session() {
        drop(pages::tell(&subagents::chat_page(&session), &json!({ "kind": "session-open", "open": host::focused() }), false));
    }
}

/// A click on what the chat drew: the chat runs it, handed your click so it may write to its Claude Code.
fn act(handler: &str, payload: &Value) {
    if let Some(session) = followed_session() {
        drop(pages::tell(&subagents::chat_page(&session), &json!({ "kind": "session-act", "handler": handler, "payload": payload }), true));
    }
}

/// The sidebar's page: the chat's name with its folder and branch, and what the chat drew under it.
pub fn view() -> Vec<Node> {
    SIDE.with(|side| side.borrow().as_ref().map(body)).into_iter().collect()
}

fn body(side: &Side) -> Node {
    match &side.followed {
        Followed::Nothing => page(toolbar("Session", None), [empty("No chat in front", "Open a Claude chat, and what it changes, runs and starts shows here.")]),
        Followed::New => page(toolbar("New chat", None), [empty("Nothing yet", "Send the first message, and what Claude changes, runs and starts shows here.")]),
        Followed::Session(_) if side.state.is_null() => page(toolbar("Session", None), [loading()]),
        Followed::Session(_) => {
            let state = &side.state;
            let title = state["title"].as_str().unwrap_or("Claude chat");
            // The app cuts a toolbar's title to 40% of its width, so the chat's name and its folder · branch are lines of
            // the body, where each has the room of the panel.
            let head = [Some(crate::view::inset(ui::text(title).prop("weight", "medium").prop("truncate", true).prop("title", title))), state["place"].as_str().map(|place| crate::view::inset(muted(place).prop("size", "small").prop("truncate", true).prop("title", place)))];
            let on: Rc<dyn Fn(&str, &Value)> = Rc::new(act);
            let sections = ui::import(&state["sections"], &on);
            // Only a terminal Wings handed this page draws: the chat's, while it is the tab in front.
            let terminals: Vec<Node> = state["terminals"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|pane| side.terminals.iter().any(|shown| shown == pane))
                .map(|pane| Node::new("Terminal").prop("pane", pane).prop("height", crate::view::TERMINAL_HEIGHT).key(format!("terminal-{pane}")))
                .collect();
            let shown: Vec<Node> = if sections.is_empty() && terminals.is_empty() {
                vec![empty("Nothing yet", "What Claude changes, runs and starts in this chat shows here.")]
            } else {
                sections.into_iter().chain(terminals).collect()
            };
            // The title is a row-inset line, as the body's rows are: a toolbar's own title sits 7 points left of them.
            let bar = Node::new("Toolbar").child(Node::new("Slot").prop("name", "children").child(crate::view::inset(ui::title("Session"))));
            page(bar, head.into_iter().flatten().chain(shown))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every call the page makes of Wings, answered at once.
    fn record() -> Rc<RefCell<Vec<(String, Value)>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        calls
    }

    fn called(calls: &Rc<RefCell<Vec<(String, Value)>>>, method: &str) -> Vec<Value> {
        calls.borrow().iter().filter(|(called, _)| called == method).map(|(_, params)| params.clone()).collect()
    }

    /// A chat whose session `s1` changed the working tree and has its context measured.
    fn chat_with_changes() {
        chat::with(|c| {
            c.side_in_wings = true;
            c.session.id = Some("s1".into());
            c.folder = "shop".into();
            c.branch = Some("main".into());
            c.changes = Some((12, 3));
            c.session.context = Some((41_000, 200_000));
        });
    }

    /// What the chat last told the sidebar of itself.
    fn last_state(calls: &Rc<RefCell<Vec<(String, Value)>>>) -> Option<Value> {
        called(calls, "tellPage").into_iter().filter(|told| told["message"]["kind"] == "session-state").map(|told| told["message"].clone()).last()
    }

    /// The handler name the exported JSON gives the node with `title`.
    fn handler_titled(tree: &Value, title: &str) -> Option<String> {
        if tree["props"]["title"] == title {
            return tree["props"]["onClick"].as_str().map(str::to_owned);
        }
        let children = tree.as_array().or_else(|| tree["children"].as_array())?;
        children.iter().find_map(|child| handler_titled(child, title))
    }

    #[test]
    fn a_chat_that_comes_to_the_front_opens_the_sidebar_once_and_has_it_follow_its_session() {
        let calls = record();
        chat_with_changes();
        focused(true);
        assert_eq!(called(&calls, "openView"), [json!({ "id": "session", "as": "sidebar", "args": null, "newTab": false })]);
        // Told once its page is there.
        names_changed(&[IDLE.to_owned(), "chat:s1".to_owned()]);
        let shows: Vec<Value> = called(&calls, "tellPage").into_iter().filter(|told| told["message"]["kind"] == "session-show").collect();
        assert_eq!(shows, [json!({ "name": IDLE, "message": show_message(Some("s1")), "write": false })]);
        // Behind another tab and back: it follows this chat again, and isn't opened again, as you may have closed it.
        focused(false);
        focused(true);
        assert_eq!(called(&calls, "openView").len(), 1);
        assert_eq!(called(&calls, "tellPage").into_iter().filter(|told| told["message"]["kind"] == "session-show").count(), 2);
    }

    #[test]
    fn a_new_chat_has_the_sidebar_follow_it_once_its_session_starts() {
        let calls = record();
        names_changed(&[IDLE.to_owned()]);
        chat::with(|c| c.side_in_wings = true);
        focused(true);
        chat::with(|c| c.session.id = Some("s2".into()));
        sync();
        let shows: Vec<Value> = called(&calls, "tellPage").into_iter().filter(|told| told["message"]["kind"] == "session-show").map(|told| told["message"].clone()).collect();
        assert_eq!(shows, [show_message(None), show_message(Some("s2"))]);
    }

    #[test]
    fn a_new_chat_has_the_sidebar_follow_it_before_its_first_message() {
        let calls = record();
        names_changed(&[IDLE.to_owned()]);
        chat::with(|c| c.side_in_wings = true);
        focused(true);
        // Claude Code names the session on the lines it prints as it starts, long before the `init` a first message brings.
        chat::with(|c| c.session.hear(&json!({ "type": "system", "subtype": "ui_status", "session_id": "s3" })));
        sync();
        let shows: Vec<Value> = called(&calls, "tellPage").into_iter().filter(|told| told["message"]["kind"] == "session-show").map(|told| told["message"].clone()).collect();
        assert_eq!(shows, [show_message(None), show_message(Some("s3"))]);
        assert_eq!(chat::with(|c| c.session.id.clone()), None, "nothing to resume yet");
    }

    #[test]
    fn the_sidebar_is_told_the_chats_sections_while_it_follows_this_chat_and_only_when_they_change() {
        let calls = record();
        chat_with_changes();
        // Following another chat: nothing is drawn for it.
        names_changed(&[page_name("s9")]);
        chat::with(|c| publish(c));
        assert!(last_state(&calls).is_none());
        names_changed(&[page_name("s1")]);
        chat::with(|c| publish(c));
        chat::with(|c| publish(c));
        let state = last_state(&calls).expect("told what the chat has");
        assert_eq!(called(&calls, "tellPage").len(), 1, "told once while nothing changed");
        assert_eq!((&state["title"], &state["place"]), (&json!("Claude chat"), &json!("shop · main")));
        assert!(handler_titled(&state["sections"], "Open Changes").is_some());
        chat::with(|c| c.changes = Some((13, 3)));
        chat::with(|c| publish(c));
        assert_eq!(called(&calls, "tellPage").len(), 2);
    }

    #[test]
    fn a_click_in_the_sidebar_runs_in_the_chat_and_only_from_the_sidebar_that_follows_it() {
        let calls = record();
        chat_with_changes();
        names_changed(&[page_name("s1")]);
        chat::with(|c| publish(c));
        let open = handler_titled(&last_state(&calls).unwrap()["sections"], "Open Changes").unwrap();
        let click = json!({ "kind": "session-act", "handler": open, "payload": {} });
        told(&click, Some("session-side:s9"), true);
        assert!(called(&calls, "emit").is_empty(), "not from a page that follows another chat");
        told(&click, Some(&page_name("s1")), true);
        assert_eq!(called(&calls, "emit"), [json!({ "name": "open-changes", "data": null })]);
        // The sidebar says whether it shows, for the header's button.
        told(&json!({ "kind": "session-open", "open": true }), Some(&page_name("s1")), false);
        assert!(shown());
    }

    #[test]
    fn the_header_button_opens_the_sidebar_on_this_chat_and_closes_it() {
        let calls = record();
        chat_with_changes();
        names_changed(&[IDLE.to_owned()]);
        toggle();
        assert!(shown());
        assert_eq!(called(&calls, "openView").len(), 1);
        assert!(called(&calls, "tellPage").iter().any(|told| told["message"] == show_message(Some("s1"))));
        assert!(chat::with(|c| c.side_open));
        toggle();
        assert!(!shown());
        assert_eq!(called(&calls, "closeView"), [json!({ "id": "session" })]);
        assert!(!chat::with(|c| c.side_open), "your choice for every chat");
    }

    #[test]
    fn after_a_subagents_sidebar_took_its_place_the_button_brings_the_session_sidebar_back() {
        let calls = record();
        chat_with_changes();
        names_changed(&[IDLE.to_owned()]);
        toggle();
        assert!(shown());
        // The chat opens a subagent in the sidebar, which replaces the session's.
        covered();
        assert!(!shown(), "the button offers to show it");
        toggle();
        assert!(shown());
        assert_eq!(called(&calls, "openView").len(), 2, "opened again");
        assert!(called(&calls, "closeView").is_empty(), "not closed as if it were on screen");
        assert!(chat::with(|c| c.side_open));
        // Now it is the one on screen, and the button closes it.
        toggle();
        assert_eq!(called(&calls, "closeView"), [json!({ "id": "session" })]);
    }

    #[test]
    fn the_sidebar_that_replaced_it_reports_the_session_sidebar_away_and_the_button_opens_it() {
        let calls = record();
        chat_with_changes();
        told(&json!({ "kind": "session-open", "open": true }), Some(&page_name("s1")), false);
        assert!(shown());
        told(&json!({ "kind": "session-open", "open": false }), Some(&page_name("s1")), false);
        assert!(!shown());
        toggle();
        assert_eq!(called(&calls, "openView").len(), 1);
    }

    #[test]
    fn a_subagent_sidebars_session_button_has_the_chat_open_the_session_sidebar() {
        let calls = record();
        chat_with_changes();
        told(&reopen_message(), Some("subagent-side:s1:a1"), false);
        assert_eq!(called(&calls, "openView"), [json!({ "id": "session", "as": "sidebar", "args": null, "newTab": false })]);
        assert!(shown());
        // In the chat there is no session sidebar to open.
        let calls = record();
        chat::with(|c| c.side_in_wings = false);
        told(&reopen_message(), Some("subagent-side:s1:a1"), false);
        assert!(called(&calls, "openView").is_empty());
    }

    #[test]
    fn with_the_panel_in_the_chat_the_sidebar_is_neither_opened_nor_told_anything() {
        let calls = record();
        chat_with_changes();
        chat::with(|c| c.side_in_wings = false);
        focused(true);
        names_changed(&[page_name("s1")]);
        chat::with(|c| publish(c));
        assert!(called(&calls, "openView").is_empty());
        assert!(called(&calls, "tellPage").iter().all(|told| told["message"]["kind"] != "session-state"));
    }

    #[test]
    fn the_setting_moves_the_panel_between_the_chat_and_the_sidebar_in_an_open_chat() {
        let calls = record();
        chat_with_changes();
        chat::with(|c| c.side_in_wings = false);
        // Into the sidebar: a chat in front opens it, as it does when it starts.
        chat::with(|c| c.side_in_wings = true);
        moved(true);
        assert_eq!(called(&calls, "openView").len(), 1);
        names_changed(&[page_name("s1")]);
        told(&json!({ "kind": "session-open", "open": true }), Some(&page_name("s1")), false);
        chat::with(|c| publish(c));
        assert!(last_state(&calls).is_some());
        // Back into the chat: the sidebar it had open is closed.
        chat::with(|c| c.side_in_wings = false);
        moved(false);
        assert_eq!(called(&calls, "closeView"), [json!({ "id": "session" })]);
        assert!(!shown());
    }

    fn sidebar_text() -> String {
        view().into_iter().map(|node| ui::tree_json(node).to_string()).collect()
    }

    #[test]
    fn the_sidebar_draws_the_chat_it_follows_with_only_the_terminals_wings_handed_it() {
        let calls = record();
        SIDE.with(|side| *side.borrow_mut() = Some(Side::default()));
        assert!(sidebar_text().contains("No chat in front"));
        told_side(&show_message(Some("s1")), Some("chat:s1"), false);
        assert_eq!(called(&calls, "namePage"), [json!({ "name": page_name("s1") })]);
        assert!(called(&calls, "tellPage").iter().any(|told| told["name"] == "chat:s1" && told["message"]["kind"] == "session-open"));
        // What another chat says isn't shown.
        chat_with_changes();
        chat::with(|c| c.terminals = vec![chat::Terminal { pane: "4".into() }, chat::Terminal { pane: "5".into() }]);
        let state = chat::with(|c| state_message(c, ui::export(crate::view::side_sections(c, false)).0));
        told_side(&state, Some("chat:s9"), false);
        assert!(!sidebar_text().contains("Open Changes"));
        told_side(&state, Some("chat:s1"), false);
        with_side(|side| side.terminals = vec!["5".into()]);
        let drawn = sidebar_text();
        assert!(drawn.contains("Open Changes") && drawn.contains("shop · main"), "{drawn}");
        // The chat's name and its folder · branch are body lines under the toolbar "Session", which the app would cut.
        assert!(drawn.contains("\"Session\"") && drawn.contains("Claude chat"), "{drawn}");
        assert!(drawn.contains("\"pane\":\"5\"") && !drawn.contains("\"pane\":\"4\""));
        // A click there goes to the chat, with the click.
        act("7:onClick", &json!({}));
        let handed = called(&calls, "tellPage").pop().unwrap();
        assert_eq!(handed, json!({ "name": "chat:s1", "message": { "kind": "session-act", "handler": "7:onClick", "payload": {} }, "write": true }));
        // The chat closed: back to nothing.
        side_names(&["chat:s1".to_owned(), page_name("s1")]);
        side_names(&[page_name("s1")]);
        assert!(sidebar_text().contains("No chat in front"));
        SIDE.with(|side| *side.borrow_mut() = None);
    }

    #[test]
    fn a_busy_sessions_sections_fit_in_one_message_to_the_sidebar() {
        record();
        chat_with_changes();
        chat::with(|c| {
            let long = "Survey every caller of applyDiscount across the checkout, the cart and the order history, and say which round".repeat(2);
            for n in 0..20 {
                let (call, agent) = (format!("toolu_{n}"), format!("{n:017x}"));
                c.session.hear(&json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": call, "name": "Agent", "input": { "description": long, "prompt": long } }] } }));
                c.session.hear(&json!({ "type": "system", "subtype": "task_started", "task_id": agent, "tool_use_id": call, "description": long, "task_type": "local_agent" }));
            }
            let tasks: Vec<Value> = (0..10).map(|n| json!({ "task_id": format!("b{n:08}"), "run_id": format!("run-{n}"), "task_type": "local_bash", "description": long })).collect();
            c.session.hear(&json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": tasks }));
            let state = state_message(c, ui::export(crate::view::side_sections(c, false)).0);
            // What `pages::tell` takes at most.
            assert!(state.to_string().len() < 64_000, "{} bytes", state.to_string().len());
        });
    }
}
