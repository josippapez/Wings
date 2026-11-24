//! What the sidebar and the popover hold: the project on screen, the accounts, and what each control is doing.

use crate::accounts::{Loaded, Project};
use serde_json::Value;
use std::{cell::RefCell, collections::HashMap};

/// What the card says after the project's account changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// The account was added rather than switched to.
    pub added: bool,
    pub label: String,
}

/// A running action and why the last one failed, by the name its control gives it.
#[derive(Clone, Default)]
pub struct Action {
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone, Default)]
pub struct State {
    /// `None` until Wings says, then the project, or none when no project is open.
    pub project: Option<Option<Project>>,
    pub data: Option<Loaded>,
    pub error: Option<String>,
    pub note: Option<Note>,
    pub adding: bool,
    pub name: String,
    pub submitted: bool,
    /// The picker shows the account just picked (`Some(None)` is the default) while Wings saves it.
    pub picked: Option<Option<String>>,
    /// The same for each account's "use my default setup" box.
    pub shares: HashMap<String, bool>,
    /// The account a "Start Claude as" button was pressed for.
    pub starting: Option<String>,
    pub actions: HashMap<String, Action>,
}

impl State {
    pub fn action(&self, key: &str) -> Action {
        self.actions.get(key).cloned().unwrap_or_default()
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn with<T>(change: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|state| change(&mut state.borrow_mut()))
}

/// The project Wings says is on screen: another one starts without the last one's message or half-done choices.
pub fn set_project(value: &Value) {
    let next = (!value.is_null()).then(|| Project::from_json(value));
    with(|s| {
        if s.project.as_ref().and_then(|p| p.as_ref()).map(|p| &p.id) != next.as_ref().map(|p| &p.id) {
            s.note = None;
            s.picked = None;
            s.starting = None;
            for key in ["swap", "open", "start"] {
                s.actions.remove(key);
            }
        }
        s.project = Some(next);
    });
}
