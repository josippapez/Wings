//! The chat's side of its subagents' tabs (`subagents.rs`): the chat's page takes the name the tabs of its session reach
//! it by, opens a subagent's tab right after its own, passes each line a subagent prints on to the subagent's tab while
//! that is open, tells each tab how its subagent and the chat stand, and writes what you send from a tab, with the write
//! your Enter there handed on.

use std::{cell::RefCell, collections::HashMap};

use serde_json::Value;
use wings_plugin::{host, pages};

use crate::{
    chat::{self, Chat, State},
    subagents,
};

#[derive(Default)]
struct Relay {
    /// The name the chat's page took, for its session.
    named: Option<String>,
    /// The names the plugin's pages have now.
    names: Vec<String>,
    /// What each open tab was last told of its subagent, by agent, so it is told only what changed.
    told: HashMap<String, Value>,
}

thread_local! {
    static RELAY: RefCell<Relay> = RefCell::new(Relay::default());
}

/// Hears which pages are open and what the tabs send. Called as the chat starts.
pub fn start() {
    pages::on_names(|names| {
        RELAY.with(|relay| relay.borrow_mut().names = names.to_vec());
        sync();
    });
    // Only with your Enter in the tab, which Wings hands on as this page's one write.
    pages::on_told(|message, _, write| {
        let message = message.clone();
        chat::write(move |c, n| {
            let stamp = host::now() as u64;
            let line = subagents::relay_write(&mut c.session, &message, write, stamp, n)?;
            c.doing = (c.session.activity(), stamp);
            Some(line)
        });
        chat::report_agent();
    });
}

/// Whether a click on `id`, a call that started a subagent or a background agent's own id, has a tab to open.
pub fn can_open(c: &Chat, id: &str) -> bool {
    c.session.id.is_some() && subagents::target(&c.session, id).is_some()
}

/// A click on a subagent: its tab, right after the chat's, or the one already open for it.
pub fn open(id: &str) {
    let opened = chat::with(|c| {
        let (agent, title) = subagents::target(&c.session, id)?;
        Some(subagents::tab_args(c.session.id.as_deref()?, &agent, &title))
    });
    if let Some(args) = opened {
        drop(host::open_tab_beside("chat", &args));
    }
}

/// A line Claude Code printed, which goes on to its subagent's tab while that is open. Nothing for any other line, and
/// nothing is copied unless a tab takes it.
pub fn heard(line: &Value) {
    let Some(agent) = line["agent_id"].as_str() else {
        return;
    };
    let Some(name) = chat::with(|c| c.session.id.as_deref().map(|session| subagents::tab_page(session, agent))) else {
        return;
    };
    if !RELAY.with(|relay| relay.borrow().names.contains(&name)) {
        return;
    }
    if let Some((_, message)) = subagents::passed_line(line) {
        drop(pages::tell(&name, &message, false));
    }
}

/// Takes the name for the chat's session, and tells each open tab of its subagents how they stand when that changed.
pub fn sync() {
    let Some((session, title, ready)) = chat::with(|c| Some((c.session.id.clone()?, chat::name(c), c.state == State::Running))) else {
        return;
    };
    let name = subagents::chat_page(&session);
    let renamed = RELAY.with(|relay| {
        let mut relay = relay.borrow_mut();
        let renamed = relay.named.as_ref() != Some(&name);
        if renamed {
            relay.named = Some(name.clone());
            relay.told.clear();
        }
        renamed
    });
    if renamed {
        drop(pages::name(Some(&name)));
    }
    let open = RELAY.with(|relay| subagents::open_tabs(&relay.borrow().names, &session));
    let states: Vec<(String, Value)> = chat::with(|c| open.iter().map(|agent| (agent.clone(), subagents::state_of(&c.session, agent, title.as_deref(), ready))).collect());
    for (agent, state) in states {
        let changed = RELAY.with(|relay| {
            let mut relay = relay.borrow_mut();
            let changed = relay.told.get(&agent) != Some(&state);
            if changed {
                relay.told.insert(agent.clone(), state.clone());
            }
            changed
        });
        if changed {
            drop(pages::tell(&subagents::tab_page(&session, &agent), &state, false));
        }
    }
    // A tab that closed is told again if it opens again.
    RELAY.with(|relay| relay.borrow_mut().told.retain(|agent, _| open.contains(agent)));
}
