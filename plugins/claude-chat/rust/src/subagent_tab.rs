//! A subagent's tab (`subagents.rs`): the page of the chat view opened with a subagent in its args. It shows the
//! subagent's conversation, read from its transcript as the tab opens and then from the lines its chat passes on, and its
//! message box has the chat's Claude pass what you send on to the subagent. What it shows is `subagent_view.rs`.

use std::cell::RefCell;

use serde_json::Value;
use wings_plugin::{host, pages, task};

use crate::{
    chat, prefs,
    session::{Item, Session},
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
fn hear(hear: impl FnOnce(&mut Heard, &mut Session)) {
    TAB.with(|tab| {
        if let Some(tab) = tab.borrow_mut().as_mut() {
            chat::with(|c| {
                c.session.now = host::now() as u64;
                hear(&mut tab.heard, &mut c.session);
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
    wings_plugin::run_filled(crate::subagent_view::view);
    chat::with(|c| {
        c.detailed = prefs::detailed().get();
        c.text_scale = prefs::scale_of(&prefs::text_size().get());
    });
    prefs::text_size().on_change(|size| chat::with(|c| c.text_scale = prefs::scale_of(size)));
    drop(host::set_tab_title(Some(&title)));
    drop(pages::name(Some(&subagents::tab_page(session, agent))));
    pages::on_names(|names| change(|tab| tab.chat_open = Some(names.contains(&subagents::chat_page(&tab.session)))));
    pages::on_told(|message, _, _| told(message));
    load();
    true
}

/// Reads the subagent's transcript, then what the chat passed on while it was read.
fn load() {
    let Some((session, agent)) = with(|tab| (tab.session.clone(), tab.agent.clone())) else {
        return;
    };
    task::spawn(async move {
        let read = host::subagent_transcript(&session, &agent, &["user", "assistant"], Some(HISTORY)).await;
        // A subagent that has only just started may have no transcript yet: what the chat passes on still shows.
        let entries = match read {
            Ok(entries) => entries.as_array().cloned().unwrap_or_default(),
            Err(error) => {
                change(|tab| tab.error = Some(error));
                Vec::new()
            }
        };
        hear(|heard, session| heard.load(&entries, session));
    });
}

/// What the chat tells the tab: a line of the subagent's, how it stands, or that a line was too long to pass on.
fn told(message: &Value) {
    match message["kind"].as_str() {
        Some("line") => {
            let line = message["line"].clone();
            hear(|heard, session| heard.line(line, session));
        }
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
            Ok(_) => chat::with(|c| c.session.items.push(Item::User(said))),
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

/// The header's way back: the chat's tab, in front.
pub fn back() {
    if let Some(session) = with(|tab| tab.session.clone()) {
        drop(pages::show(&subagents::chat_page(&session)));
    }
}
