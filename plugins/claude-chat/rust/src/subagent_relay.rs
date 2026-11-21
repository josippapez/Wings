//! The chat's side of its subagents' pages (`subagents.rs`): the chat's page takes the name the pages of its session reach
//! it by, opens a subagent in Wings' sidebar (telling that page which one to show) or as a tab right after its own, passes
//! each line a subagent prints on to the pages that show it, tells each how its subagent and the chat stand, and writes
//! what you send from either, with the write your Enter there handed on.

use std::{cell::RefCell, collections::HashMap};

use serde_json::Value;
use wings_plugin::{host, pages};

use crate::{
    chat::{self, Chat, State},
    subagents,
};

/// The id of the plugin's sidebar for a subagent (`contributes.sidebars`).
const SIDEBAR: &str = "subagent";

#[derive(Default)]
struct Relay {
    /// The name the chat's page took, for its session.
    named: Option<String>,
    /// The names the plugin's pages have now.
    names: Vec<String>,
    /// What each page that shows a subagent was last told of it, by page name, so it is told only what changed.
    told: HashMap<String, Value>,
    /// What the sidebar is to show, until its page is there to be told.
    show: Option<Value>,
}

thread_local! {
    static RELAY: RefCell<Relay> = RefCell::new(Relay::default());
}

/// Hears which pages are open and what the pages send. Called as the chat starts.
pub fn start() {
    pages::on_names(names_changed);
    pages::on_told(told);
}

fn names_changed(names: &[String]) {
    RELAY.with(|relay| relay.borrow_mut().names = names.to_vec());
    send_show();
    sync();
}

/// What a page of a subagent tells the chat: to open its subagent as a tab, or to pass a message on, which only your key or
/// click in that page, handed on with it as this page's one write, makes the chat write.
fn told(message: &Value, _from: Option<&str>, write: bool) {
    if message["kind"] == "pop" {
        return pop(message);
    }
    let message = message.clone();
    chat::write(move |c, n| {
        let stamp = host::now() as u64;
        let line = subagents::relay_write(&mut c.session, &message, write, stamp, n)?;
        c.doing = (c.session.activity(), stamp);
        Some(line)
    });
    chat::report_agent();
}

/// Whether a click on `id`, a call that started a subagent or a background agent's own id, has a page to open.
pub fn can_open(c: &Chat, id: &str) -> bool {
    c.session.id.is_some() && subagents::target(&c.session, id).is_some()
}

/// A click on a subagent: Wings' right sidebar shows it, and shows it in place of the one it showed. The sidebar is told
/// once its page is there, which may be a moment after it was asked to open.
pub fn open_in_sidebar(id: &str) {
    let shown = chat::with(|c| {
        let (agent, title) = subagents::target(&c.session, id)?;
        Some(subagents::show_message(c.session.id.as_deref()?, &agent, &title))
    });
    let Some(message) = shown else {
        return;
    };
    RELAY.with(|relay| relay.borrow_mut().show = Some(message));
    crate::session_side::covered();
    drop(host::open_view_with(SIDEBAR, "sidebar", &Value::Null));
    send_show();
}

/// Tells the sidebar what to show, when it is asked to show something and its page has taken its name.
fn send_show() {
    let sent = RELAY.with(|relay| {
        let mut relay = relay.borrow_mut();
        let name = subagents::side_name(&relay.names)?.to_owned();
        Some((name, relay.show.take()?))
    });
    if let Some((name, message)) = sent {
        drop(pages::tell(&name, &message, false));
    }
}

/// The sidebar's pop-out: the subagent's tab, right after the chat's, or the one already open for it.
fn pop(message: &Value) {
    let opened = chat::with(|c| {
        let session = c.session.id.as_deref()?;
        let agent = message["agent"].as_str().filter(|agent| subagents::is_agent_id(agent))?;
        let title = message["name"].as_str().unwrap_or_default();
        Some(subagents::tab_args(session, agent, title))
    });
    if let Some(args) = opened {
        drop(host::open_tab_beside("chat", &args));
    }
}

/// A line Claude Code printed, which goes on to the pages that show its subagent while they are open, as does what a
/// SendMessage call in it tells a subagent. Nothing for any other line, and nothing is copied unless a page takes it.
pub fn heard(line: &Value) {
    let sent = subagents::sent_with(line);
    if !sent.is_empty()
        && let Some(session) = chat::with(|c| c.session.id.clone())
    {
        for (agent, message) in sent {
            for name in RELAY.with(|relay| subagents::pages_of(&relay.borrow().names, &session, &agent)) {
                drop(pages::tell(&name, &message, false));
            }
        }
    }
    let Some(agent) = line["agent_id"].as_str() else {
        return;
    };
    let Some(session) = chat::with(|c| c.session.id.clone()) else {
        return;
    };
    let shown = RELAY.with(|relay| subagents::pages_of(&relay.borrow().names, &session, agent));
    if shown.is_empty() {
        return;
    }
    if let Some((_, message)) = subagents::passed_line(line) {
        for name in shown {
            drop(pages::tell(&name, &message, false));
        }
    }
}

/// Takes the name for the chat's session, and tells each page that shows one of its subagents how they stand when that
/// changed.
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
    let open = RELAY.with(|relay| subagents::watching(&relay.borrow().names, &session));
    let states: Vec<(String, Value)> = chat::with(|c| open.iter().map(|(page, agent)| (page.clone(), subagents::state_of(&c.session, agent, title.as_deref(), ready))).collect());
    for (page, state) in states {
        let changed = RELAY.with(|relay| {
            let mut relay = relay.borrow_mut();
            let changed = relay.told.get(&page) != Some(&state);
            if changed {
                relay.told.insert(page.clone(), state.clone());
            }
            changed
        });
        if changed {
            drop(pages::tell(&page, &state, false));
        }
    }
    // A page that closed, or went on to another subagent, is told again if it shows this one again.
    RELAY.with(|relay| relay.borrow_mut().told.retain(|page, _| open.iter().any(|(open, _)| open == page)));
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use serde_json::json;

    use super::*;

    /// Every call the chat makes of Wings, answered at once.
    fn record() -> Rc<RefCell<Vec<(String, Value)>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        calls
    }

    /// A chat whose session `s1` started two subagents, as Claude Code 2.1.327 prints them, cut down.
    fn two_subagents() {
        chat::with(|c| {
            c.session.id = Some("s1".into());
            for (call, agent, what) in [("toolu_1", "a1b2c3d4e5f60718a", "count lines"), ("toolu_2", "b2c3d4e5f60718a9b", "count words")] {
                c.session.hear(&json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": call, "name": "Agent", "input": { "description": what, "prompt": what } }] } }));
                c.session.hear(&json!({ "type": "system", "subtype": "task_started", "task_id": agent, "tool_use_id": call, "description": what, "task_type": "local_agent" }));
            }
        });
    }

    fn told_pages(calls: &Rc<RefCell<Vec<(String, Value)>>>) -> Vec<(String, Value)> {
        calls.borrow().iter().filter(|(method, _)| method == "tellPage").map(|(_, params)| (params["name"].as_str().unwrap().to_owned(), params["message"].clone())).collect()
    }

    #[test]
    fn a_click_on_a_subagent_opens_the_sidebar_and_shows_that_subagent_and_a_second_click_switches_it() {
        let calls = record();
        two_subagents();
        // The sidebar isn't there yet: it is asked to open, and told what to show once its page names itself.
        open_in_sidebar("toolu_1");
        let opened: Vec<Value> = calls.borrow().iter().filter(|(method, _)| method == "openView").map(|(_, params)| params.clone()).collect();
        assert_eq!(opened, [json!({ "id": "subagent", "as": "sidebar", "args": null, "newTab": false })]);
        assert!(told_pages(&calls).is_empty());
        names_changed(&[subagents::SIDE_IDLE.to_owned()]);
        assert_eq!(told_pages(&calls), [("subagent-side".to_owned(), subagents::show_message("s1", "a1b2c3d4e5f60718a", "count lines"))]);
        // Told once: more names don't tell it again.
        names_changed(&[subagents::SIDE_IDLE.to_owned(), "chat:s1".to_owned()]);
        assert_eq!(told_pages(&calls).iter().filter(|(_, message)| message["kind"] == "show").count(), 1);
        // The sidebar shows the first now and answers to its name; a click on the second switches it, at once.
        names_changed(&[subagents::side_page("s1", "a1b2c3d4e5f60718a"), subagents::chat_page("s1")]);
        open_in_sidebar("toolu_2");
        let shows: Vec<(String, Value)> = told_pages(&calls).into_iter().filter(|(_, message)| message["kind"] == "show").collect();
        assert_eq!(shows.last().unwrap(), &("subagent-side:s1:a1b2c3d4e5f60718a".to_owned(), subagents::show_message("s1", "b2c3d4e5f60718a9b", "count words")));
        // A call whose subagent has no id yet opens nothing.
        let before = calls.borrow().len();
        open_in_sidebar("toolu_unknown");
        assert_eq!(calls.borrow().len(), before);
    }

    #[test]
    fn the_sidebar_is_fed_the_lines_and_the_state_of_the_subagent_it_shows() {
        let calls = record();
        two_subagents();
        names_changed(&[subagents::side_page("s1", "b2c3d4e5f60718a9b")]);
        calls.borrow_mut().clear();
        heard(&json!({ "type": "assistant", "parent_tool_use_id": "toolu_2", "agent_id": "b2c3d4e5f60718a9b", "uuid": "u9", "message": { "role": "assistant", "content": [{ "type": "text", "text": "hi" }] } }));
        // Another subagent's line goes to no one.
        heard(&json!({ "type": "assistant", "parent_tool_use_id": "toolu_1", "agent_id": "a1b2c3d4e5f60718a", "uuid": "u8", "message": { "role": "assistant", "content": [{ "type": "text", "text": "no" }] } }));
        let lines = told_pages(&calls);
        assert_eq!(lines.len(), 1);
        assert_eq!((lines[0].0.as_str(), lines[0].1["kind"].as_str()), ("subagent-side:s1:b2c3d4e5f60718a9b", Some("line")));
    }

    /// Claude Code 2.1.330 prints no line of a subagent's for the message SendMessage wakes it with: what a tab open on it
    /// hears is the call, then the subagent's answer.
    #[test]
    fn a_tab_open_on_a_subagent_shows_the_message_its_chat_sent_to_wake_it() {
        let calls = record();
        two_subagents();
        names_changed(&[subagents::tab_page("s1", "a1b2c3d4e5f60718a")]);
        calls.borrow_mut().clear();
        heard(&json!({ "type": "assistant", "parent_tool_use_id": null, "uuid": "u4", "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": "toolu_s", "name": "SendMessage", "input": { "to": "a1b2c3d4e5f60718a", "message": "Now reply with only the word hello.", "summary": "Ask it to say hello" } }] } }));
        heard(&json!({ "type": "assistant", "parent_tool_use_id": "toolu_1", "agent_id": "a1b2c3d4e5f60718a", "uuid": "u5", "message": { "role": "assistant", "content": [{ "type": "text", "text": "hello" }] } }));
        let (mut session, mut heard) = (crate::session::Session::default(), subagents::Heard::default());
        heard.load(&[], &mut session, &mut Vec::new());
        for (name, message) in told_pages(&calls) {
            assert_eq!(name, "subagent:s1:a1b2c3d4e5f60718a");
            heard.line(message["line"].clone(), &mut session, &mut Vec::new());
        }
        assert_eq!(session.items, [crate::session::Item::User("Now reply with only the word hello.".into()), crate::session::Item::Text("hello".into())]);
    }

    #[test]
    fn the_sidebars_pop_out_opens_a_tab_right_after_the_chat() {
        let calls = record();
        two_subagents();
        told(&subagents::pop_message("a1b2c3d4e5f60718a", "count lines"), None, false);
        let opened: Vec<Value> = calls.borrow().iter().filter(|(method, _)| method == "openView").map(|(_, params)| params.clone()).collect();
        assert_eq!(
            opened,
            [json!({ "id": "chat", "as": "tab", "args": subagents::tab_args("s1", "a1b2c3d4e5f60718a", "count lines"), "newTab": false, "after": "self" })]
        );
        // An id that isn't a subagent's opens nothing.
        told(&subagents::pop_message("../x", "x"), None, false);
        assert_eq!(calls.borrow().iter().filter(|(method, _)| method == "openView").count(), 1);
    }
}
