//! Pages of one plugin that reach each other by name, like a chat tab and the tabs it opened for its subagents. A page
//! names itself (`namePage`), the plugin's frames hear which names there are (`pages`), a page tells another one a
//! message (`tellPage`), and a page brings another one's tab to the front (`showPage`). A new tab can open right after
//! the tab of the page that opens it (`openView`'s `after: "self"`).
//!
//! `tellPage` with `write: true` carries your click or key. The page you clicked in gives it up, as it would for a write
//! to a program of its own, and the page it names may then make one `processWrite` in the next few seconds. So a tab that
//! doesn't run Claude Code can have the one that does pass on what you typed, one write for each click or key, and
//! nothing else lets that page write.

use std::{
    collections::HashMap,
    time::Instant,
};

use serde_json::{Value, json};

use super::{FrameLink, FrameState, MAX_BROADCAST_CHARS, Role, Shared, USER_ACTION};
use crate::page::runtime::{Delivery, Message};

/// The most writes a page may be handed and not have made yet.
const GRANTS_KEPT: usize = 8;

/// The names the plugin's pages took, and the writes handed to them.
#[derive(Default)]
pub(super) struct Pages {
    /// Each named page: its name, its frame, and its tab's state when it shows as a tab.
    named: Vec<(String, u64, Option<FrameState>)>,
    /// When each write was handed on, by the frame it was handed to, oldest first.
    granted: HashMap<u64, Vec<Instant>>,
}

/// A page's name: up to 128 letters, digits and `-_.:`, so it can hold a session's id and a subagent's.
fn page_name(value: Option<&Value>) -> Result<String, String> {
    let name = value.and_then(Value::as_str).unwrap_or_default();
    let fits = (1..=128).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b));
    if fits {
        Ok(name.to_owned())
    } else {
        Err("A page's name is 1 to 128 letters, digits, -, _, . and :".into())
    }
}

impl Shared {
    fn page_names(&self) -> String {
        let pages = self.pages.lock().unwrap();
        json!(pages.named.iter().map(|(name, ..)| name).collect::<Vec<_>>()).to_string()
    }

    /// `namePage`: this page answers to `name` from now on, in place of a name it had, and takes it from a page that had
    /// it. `null` drops its name. Every frame of the plugin hears the names when they change.
    fn name_page(&self, frame: u64, state: Option<FrameState>, params: &Value) -> Result<String, String> {
        let name = match params.get("name") {
            None | Some(Value::Null) => None,
            name => Some(page_name(name)?),
        };
        let changed = {
            let mut pages = self.pages.lock().unwrap();
            let before: Vec<String> = pages.named.iter().map(|(name, ..)| name.clone()).collect();
            pages.named.retain(|(taken, owner, _)| *owner != frame && Some(taken) != name.as_ref());
            if let Some(name) = name {
                pages.named.push((name, frame, state));
            }
            pages.named.iter().map(|(name, ..)| name).ne(before.iter())
        };
        if changed {
            self.tell_all(Message::Pages, &self.page_names());
        }
        Ok("null".into())
    }

    /// A frame closed: its name goes, and the writes handed to it.
    pub(super) fn forget_page(&self, frame: u64) {
        let named = {
            let mut pages = self.pages.lock().unwrap();
            pages.granted.remove(&frame);
            let before = pages.named.len();
            pages.named.retain(|(_, owner, _)| *owner != frame);
            pages.named.len() != before
        };
        if named {
            self.tell_all(Message::Pages, &self.page_names());
        }
    }

    /// `tellPage`: `message` for the page named `name`, as `{ from, message, write }`, `from` being the sender's name. With
    /// `write`, `acted` says the sender just had your click or key, which it has given up; the page told may then make one
    /// write.
    fn tell_page(&self, from: u64, acted: bool, params: &Value) -> Result<String, String> {
        let name = page_name(params.get("name"))?;
        let write = match params.get("write") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(write)) => *write,
            _ => return Err("tellPage write must be true or false".into()),
        };
        if write && !acted {
            return Err("A page can hand on a write only right after you click or type in it".into());
        }
        let message = params.get("message").cloned().unwrap_or(Value::Null);
        let (to, sender) = {
            let pages = self.pages.lock().unwrap();
            let to = pages.named.iter().find(|(taken, ..)| *taken == name).map(|(_, frame, _)| *frame);
            let sender = pages.named.iter().find(|(_, frame, _)| *frame == from).map(|(name, ..)| name.clone());
            (to.ok_or_else(|| format!("No page is named {name}"))?, sender)
        };
        let text = json!({ "from": sender, "message": message, "write": write }).to_string();
        if text.encode_utf16().count() > MAX_BROADCAST_CHARS {
            return Err("A page's message is limited to 64 KB".into());
        }
        if write {
            let mut pages = self.pages.lock().unwrap();
            let granted = pages.granted.entry(to).or_default();
            granted.retain(|at| at.elapsed() < USER_ACTION);
            if granted.len() >= GRANTS_KEPT {
                return Err(format!("{name} hasn't made the writes it was handed yet"));
            }
            granted.push(Instant::now());
        }
        let inbox = self.frames.lock().unwrap().iter().find(|frame| frame.id == to).map(|frame| frame.inbox.clone());
        if inbox.is_some_and(|inbox| inbox.send(Message::PageMessage, 0, text) == Delivery::Queued) {
            return Ok("null".into());
        }
        // Not delivered, so its write goes too.
        if write && let Some(granted) = self.pages.lock().unwrap().granted.get_mut(&to) {
            granted.pop();
        }
        Err(format!("{name} didn't get the message"))
    }

    /// Lets `frame` make one write in the next few seconds, as `tellPage` with `write` does: for another agent's message
    /// the user lets agents send (`PluginHost::send_agent_message`).
    pub(super) fn grant_write(&self, frame: u64) {
        let mut pages = self.pages.lock().unwrap();
        let granted = pages.granted.entry(frame).or_default();
        granted.retain(|at| at.elapsed() < USER_ACTION);
        granted.push(Instant::now());
    }

    /// Takes the oldest write handed to `frame` that is still fresh, if there is one.
    pub(super) fn take_grant(&self, frame: u64) -> bool {
        let mut pages = self.pages.lock().unwrap();
        let Some(granted) = pages.granted.get_mut(&frame) else {
            return false;
        };
        granted.retain(|at| at.elapsed() < USER_ACTION);
        let taken = !granted.is_empty();
        if taken {
            granted.remove(0);
        }
        taken
    }

    /// `showPage`: brings the tab of the page named `name` to the front.
    fn show_page(&self, params: &Value) -> Result<String, String> {
        let name = page_name(params.get("name"))?;
        let state = {
            let pages = self.pages.lock().unwrap();
            let (_, _, state) = pages.named.iter().find(|(taken, ..)| *taken == name).ok_or_else(|| format!("No page is named {name}"))?;
            state.clone()
        };
        state.ok_or_else(|| format!("{name} isn't shown as a tab"))?.show()?;
        Ok("null".into())
    }
}

/// The calls of this module, which `FrameLink::call` hands here, answered at once. None for any other.
pub(super) fn call(link: &FrameLink, method: &str, params: &Value) -> Option<Result<String, String>> {
    let shared = &link.shared;
    Some(match method {
        "namePage" => shared.name_page(link.id, link.state.clone(), params),
        "pages" => Ok(shared.page_names()),
        "tellPage" => {
            // Your click or key goes with the message only when it asks for it: a plain message leaves it to the page.
            let write = params.get("write") == Some(&Value::Bool(true));
            let acted = write && link.role == Role::Page && link.acted.lock().unwrap().take().is_some_and(|at| at.elapsed() < USER_ACTION);
            shared.tell_page(link.id, acted, params)
        }
        "showPage" => shared.show_page(params),
        _ => return None,
    })
}

/// `openView`'s `after`: `"self"` opens a new tab right after the tab of the page that asks, as the frame it runs in.
pub(super) fn after(link: &FrameLink, params: &Value) -> Result<Option<u64>, String> {
    match params.get("after") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(after)) if after == "self" => match &link.state {
            Some(state) if state.is_tab() => Ok(Some(link.id)),
            _ => Err("openView after: \"self\" works only from a page shown as a tab".into()),
        },
        _ => Err("openView after is \"self\" or null".into()),
    }
}
