//! A subagent's page (`subagents.rs`): a tab of the chat view opened with a subagent in its args, or Wings' right sidebar,
//! which the chat tells which subagent to show (`show`). It shows the subagent's conversation, read from its transcript as
//! it opens and then from the lines its chat passes on, and its message box has the chat's Claude pass what you send on to
//! the subagent. What it shows is `subagent_view.rs`.

use std::cell::RefCell;

use serde_json::Value;
use wings_plugin::{host, pages, task};

use crate::{
    chat, prefs,
    session::Session,
    subagents::{self, Heard},
};

/// How many of the transcript's newest entries the tab reads as it opens.
const HISTORY: u32 = 400;

#[derive(Debug, Default)]
pub struct Tab {
    pub session: String,
    pub agent: String,
    /// What the subagent is for, as its call said: the tab's name.
    pub title: String,
    pub heard: Heard,
    /// Why its transcript couldn't be read.
    pub error: Option<String>,
    /// Whether the chat this subagent belongs to is open, once the page has heard which pages are.
    pub chat_open: Option<bool>,
    /// What the chat last said of the subagent and of itself (`subagents::state_of`), null until it has.
    pub state: Value,
    /// Why the last message didn't go.
    pub refused: Option<String>,
    /// What you sent from here that hasn't reached the subagent yet, shown at the end as on its way: it leaves once the chat
    /// passes it on, or the transcript shows it (`Heard`).
    pub sent: Vec<String>,
    /// This is the sidebar, which shows one subagent at a time and none (an empty `session`) until the chat says which.
    pub sidebar: bool,
    /// The names the plugin's pages have, to tell whether the chat is open when the subagent changes.
    pub names: Vec<String>,
}

impl Tab {
    /// Why the message box can't send now (`subagents::closed_because`).
    pub fn closed(&self) -> Option<Option<&'static str>> {
        subagents::closed_because(self.chat_open, self.state["ready"].as_bool())
    }
}

thread_local! {
    static TAB: RefCell<Option<Tab>> = const { RefCell::new(None) };
}

/// What `read` makes of the tab, while the page is one.
pub fn with<T>(read: impl FnOnce(&Tab) -> T) -> Option<T> {
    TAB.with(|tab| tab.borrow().as_ref().map(read))
}

fn change(change: impl FnOnce(&mut Tab)) {
    TAB.with(|tab| {
        if let Some(tab) = tab.borrow_mut().as_mut() {
            change(tab);
        }
    });
}

/// Hears into the subagent's conversation, which is the page's session.
fn hear(hear: impl FnOnce(&mut Tab, &mut Session)) {
    TAB.with(|tab| {
        if let Some(tab) = tab.borrow_mut().as_mut() {
            chat::with(|c| {
                c.session.now = host::now() as u64;
                hear(tab, &mut c.session);
            });
        }
    });
}

/// Runs the page as a subagent's tab when its tab was opened for one, and says whether it was.
pub fn start() -> bool {
    let Some(args) = host::view_args() else {
        return false;
    };
    let opened = &args["subagent"];
    let (Some(session), Some(agent)) = (opened["session"].as_str(), opened["agent"].as_str().filter(|agent| subagents::is_agent_id(agent))) else {
        return false;
    };
    let title = opened["title"].as_str().filter(|title| !title.trim().is_empty()).unwrap_or("Subagent").to_owned();
    TAB.with(|tab| *tab.borrow_mut() = Some(Tab { session: session.to_owned(), agent: agent.to_owned(), title: title.clone(), ..Tab::default() }));
    begin();
    drop(host::set_tab_title(Some(&title)));
    drop(pages::name(Some(&subagents::tab_page(session, agent))));
    load();
    true
}

/// Runs the page as Wings' sidebar for a subagent: empty until the chat's `show` says which subagent.
pub fn start_sidebar() {
    TAB.with(|tab| *tab.borrow_mut() = Some(Tab { title: "Subagent".into(), sidebar: true, ..Tab::default() }));
    begin();
    drop(pages::name(Some(subagents::SIDE_IDLE)));
}

/// What a subagent's tab and the sidebar both do as they start.
fn begin() {
    wings_plugin::run_filled(crate::subagent_view::view);
    chat::with(|c| {
        c.detailed = prefs::detailed().get();
        c.show_thinking = prefs::thinking().get();
        c.text_scale = prefs::scale_of(&prefs::text_size().get());
    });
    prefs::text_size().on_change(|size| chat::with(|c| c.text_scale = prefs::scale_of(size)));
    crate::folds::follow();
    pages::on_names(|names| {
        change(|tab| {
            tab.names = names.to_vec();
            tab.chat_open = (!tab.session.is_empty()).then(|| names.contains(&subagents::chat_page(&tab.session)));
        })
    });
    pages::on_told(|message, _, _| told(message));
}

/// The sidebar is told to show a subagent: its conversation in place of the last one's, read from its transcript, under the
/// name the chat feeds it by. A message that isn't a subagent of a session is ignored.
fn show(message: &Value) {
    let (Some(session), Some(agent)) = (message["session"].as_str().filter(|session| !session.is_empty()), message["agent"].as_str().filter(|agent| subagents::is_agent_id(agent))) else {
        return;
    };
    let title = message["title"].as_str().filter(|title| !title.trim().is_empty()).unwrap_or("Subagent").to_owned();
    let same = with(|tab| tab.sidebar && tab.session == session && tab.agent == agent).unwrap_or(false);
    if same {
        return;
    }
    switch(session, agent, &title);
    drop(pages::name(Some(&subagents::side_page(session, agent))));
    load();
}

/// The page shows `agent` of `session` from the start: nothing heard, the draft kept.
fn switch(session: &str, agent: &str, title: &str) {
    chat::with(|c| c.session = Session::default());
    TAB.with(|tab| {
        if let Some(tab) = tab.borrow_mut().as_mut() {
            let names = std::mem::take(&mut tab.names);
            let chat_open = Some(names.contains(&subagents::chat_page(session)));
            *tab = Tab { session: session.to_owned(), agent: agent.to_owned(), title: title.to_owned(), sidebar: true, names, chat_open, ..Tab::default() };
        }
    });
}

/// The sidebar's pop-out button: the chat opens the subagent as a tab beside its own, and the sidebar is left empty.
pub fn pop_out() {
    let Some((session, agent, title)) = with(|tab| (tab.session.clone(), tab.agent.clone(), tab.title.clone())).filter(|(session, ..)| !session.is_empty()) else {
        return;
    };
    drop(pages::tell(&subagents::chat_page(&session), &subagents::pop_message(&agent, &title), false));
    chat::with(|c| c.session = Session::default());
    TAB.with(|tab| {
        if let Some(tab) = tab.borrow_mut().as_mut() {
            let names = std::mem::take(&mut tab.names);
            *tab = Tab { title: "Subagent".into(), sidebar: true, names, ..Tab::default() };
        }
    });
    drop(pages::name(Some(subagents::SIDE_IDLE)));
}

/// Reads the subagent's transcript, then what the chat passed on while it was read.
fn load() {
    let Some((session, agent)) = with(|tab| (tab.session.clone(), tab.agent.clone())) else {
        return;
    };
    task::spawn(async move {
        let read = host::subagent_transcript(&session, &agent, &subagents::KINDS, Some(HISTORY)).await;
        // A subagent that has only just started may have no transcript yet: what the chat passes on still shows.
        let entries = match read {
            Ok(entries) => entries.as_array().cloned().unwrap_or_default(),
            Err(error) => {
                change(|tab| tab.error = Some(error));
                Vec::new()
            }
        };
        hear(|tab, session| tab.heard.load(&entries, session, &mut tab.sent));
    });
}

/// What the chat tells the tab: a line of the subagent's, how it stands, or that a line was too long to pass on.
fn told(message: &Value) {
    match message["kind"].as_str() {
        Some("line") => {
            let line = message["line"].clone();
            hear(|tab, session| tab.heard.line(line, session, &mut tab.sent));
        }
        Some("show") => show(message),
        Some("state") => change(|tab| tab.state = message.clone()),
        Some("reload") => {
            chat::with(|c| c.session = Session::default());
            change(|tab| {
                tab.heard = Heard::default();
                tab.error = None;
            });
            load();
        }
        _ => {}
    }
}

/// Enter in the message box, or its Send button: the chat's Claude passes what you typed on to the subagent. Your key or
/// click goes along to the chat, which may then write once.
pub fn send() {
    let Some((session, agent, title, open)) = with(|tab| (tab.session.clone(), tab.agent.clone(), tab.title.clone(), tab.closed().is_none())) else {
        return;
    };
    let said = chat::with(|c| c.draft.trim().to_owned());
    if said.is_empty() || !open {
        return;
    }
    let sent = pages::tell(&subagents::chat_page(&session), &subagents::send_message(&agent, &title, &said), true);
    chat::with(|c| c.draft.clear());
    change(|tab| tab.refused = None);
    task::spawn(async move {
        match sent.await {
            Ok(_) => change(|tab| tab.sent.push(said)),
            Err(error) => {
                // Back in the box, to send again.
                chat::with(|c| {
                    if c.draft.is_empty() {
                        c.draft = said;
                    }
                });
                change(|tab| tab.refused = Some(error));
            }
        }
    });
}

/// The sidebar's Session button: the chat shows its session in the sidebar again, in place of this subagent.
pub fn show_session() {
    if let Some(session) = with(|tab| tab.session.clone()).filter(|session| !session.is_empty()) {
        drop(pages::tell(&subagents::chat_page(&session), &crate::session_side::reopen_message(), false));
    }
}

/// The header's way back: the chat's tab, in front.
pub fn back() {
    if let Some(session) = with(|tab| tab.session.clone()) {
        drop(pages::show(&subagents::chat_page(&session)));
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;

    use super::*;

    fn record() -> Rc<RefCell<Vec<(String, Value)>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        calls
    }

    fn named(calls: &Rc<RefCell<Vec<(String, Value)>>>) -> Option<String> {
        calls.borrow().iter().rev().find(|(method, _)| method == "namePage").and_then(|(_, params)| params["name"].as_str().map(str::to_owned))
    }

    fn tells(calls: &Rc<RefCell<Vec<(String, Value)>>>) -> Vec<Value> {
        calls.borrow().iter().filter(|(method, _)| method == "tellPage").map(|(_, params)| params.clone()).collect()
    }

    #[test]
    fn the_sidebar_is_empty_until_told_shows_the_subagent_it_is_told_and_switches_to_the_next() {
        let calls = record();
        start_sidebar();
        assert_eq!(named(&calls).as_deref(), Some("subagent-side"));
        assert_eq!(with(|tab| tab.session.is_empty() && tab.sidebar), Some(true));
        told(&subagents::show_message("s1", "a1b2c3d4e5f60718a", "count lines"));
        assert_eq!(with(|tab| (tab.agent.clone(), tab.title.clone())), Some(("a1b2c3d4e5f60718a".into(), "count lines".into())));
        assert_eq!(named(&calls).as_deref(), Some("subagent-side:s1:a1b2c3d4e5f60718a"));
        // What the first printed is in the page's conversation; the second subagent's starts again without it.
        chat::with(|c| c.session.items.push(crate::session::Item::Text("one".into())));
        told(&subagents::show_message("s1", "b2c3d4e5f60718a9b", "count words"));
        assert_eq!(with(|tab| tab.agent.clone()).as_deref(), Some("b2c3d4e5f60718a9b"));
        assert_eq!(named(&calls).as_deref(), Some("subagent-side:s1:b2c3d4e5f60718a9b"));
        assert!(chat::with(|c| c.session.items.is_empty()));
        // What isn't a subagent of a session changes nothing.
        told(&subagents::show_message("s1", "../x", "x"));
        told(&subagents::show_message("", "a1b2c3d4e5f60718a", "x"));
        assert_eq!(with(|tab| tab.agent.clone()).as_deref(), Some("b2c3d4e5f60718a9b"));
    }

    #[test]
    fn the_sidebars_pop_out_asks_the_chat_for_a_tab_and_leaves_the_sidebar_empty() {
        let calls = record();
        start_sidebar();
        pop_out();
        assert!(tells(&calls).is_empty(), "nothing to pop out while it is empty");
        told(&subagents::show_message("s1", "a1b2c3d4e5f60718a", "count lines"));
        pop_out();
        assert_eq!(tells(&calls), [json!({ "name": "chat:s1", "message": subagents::pop_message("a1b2c3d4e5f60718a", "count lines"), "write": false })]);
        assert_eq!(with(|tab| tab.session.is_empty()), Some(true));
        assert_eq!(named(&calls).as_deref(), Some("subagent-side"));
    }

    #[test]
    fn a_message_from_the_sidebar_hands_the_chat_exactly_one_write() {
        let calls = record();
        start_sidebar();
        told(&subagents::show_message("s1", "a1b2c3d4e5f60718a", "count lines"));
        change(|tab| {
            tab.chat_open = Some(true);
            tab.state = json!({ "kind": "state", "ready": true });
        });
        // Nothing to send from an empty box.
        send();
        assert!(tells(&calls).is_empty());
        chat::with(|c| c.draft = " also count the words ".into());
        send();
        assert_eq!(tells(&calls), [json!({ "name": "chat:s1", "message": subagents::send_message("a1b2c3d4e5f60718a", "count lines", "also count the words"), "write": true })]);
        // The box is empty now, so another Enter sends nothing more.
        send();
        assert_eq!(tells(&calls).len(), 1);
    }
}
