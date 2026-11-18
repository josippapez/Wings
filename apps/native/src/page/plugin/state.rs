//! State belongs to a host-created tab or block instance, never an id supplied by page JavaScript.

use std::sync::{Arc, Mutex};

use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatePlace {
    Tab,
    Block,
}

struct State {
    value: Value,
    save: Box<dyn Fn(Value) -> Result<(), String> + Send + Sync>,
    /// What a tab's page says of the agent it runs, for the sidebar's Agents.
    agent: Option<TabAgent>,
    agent_changed: Option<Box<dyn Fn() + Send + Sync>>,
    /// Closes the tab that holds this state, when its page asks.
    close: Option<Box<dyn Fn(&FrameState) + Send + Sync>>,
}

/// An agent a tab's page runs, as the sidebar lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct TabAgent {
    /// `working`, `blocked`, `done` or `idle`.
    pub state: String,
    pub name: String,
    /// What it does now, or what it waits for.
    pub summary: Option<String>,
}

#[derive(Clone)]
pub struct FrameState {
    place: StatePlace,
    state: Arc<Mutex<State>>,
}

impl FrameState {
    pub fn new(
        place: StatePlace,
        initial: Option<Value>,
        save: impl Fn(Value) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            place,
            state: Arc::new(Mutex::new(State {
                value: initial.unwrap_or(Value::Null),
                save: Box::new(save),
                agent: None,
                agent_changed: None,
                close: None,
            })),
        }
    }

    /// Calls `changed` each time the page says what its agent does.
    pub fn on_agent(self, changed: impl Fn() + Send + Sync + 'static) -> Self {
        self.state.lock().unwrap().agent_changed = Some(Box::new(changed));
        self
    }

    /// Calls `close` with this state when its tab's page asks to close the tab.
    pub fn on_close(self, close: impl Fn(&FrameState) + Send + Sync + 'static) -> Self {
        self.state.lock().unwrap().close = Some(Box::new(close));
        self
    }

    /// Whether `other` is this same tab's or block's state.
    pub fn same(&self, other: &FrameState) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    pub(super) fn close(&self) -> Result<(), String> {
        match (&self.state.lock().unwrap().close, self.place) {
            (Some(close), StatePlace::Tab) => Ok(close(self)),
            _ => Err("Only a page shown as a tab can close its tab".into()),
        }
    }

    pub fn agent(&self) -> Option<TabAgent> {
        self.state.lock().unwrap().agent.clone()
    }

    pub(super) fn set_agent(&self, agent: Option<TabAgent>) -> Result<(), String> {
        if self.place != StatePlace::Tab {
            return Err("Only a page shown as a tab can show as an agent".into());
        }
        let mut state = self.state.lock().unwrap();
        if state.agent != agent {
            state.agent = agent;
            if let Some(changed) = &state.agent_changed {
                changed();
            }
        }
        Ok(())
    }

    pub fn value(&self) -> Value {
        self.state.lock().unwrap().value.clone()
    }

    pub(super) fn read(&self, place: StatePlace) -> Value {
        if place == self.place {
            self.value()
        } else {
            Value::Null
        }
    }

    pub(super) fn write(&self, place: StatePlace, value: Value) -> Result<(), String> {
        if place != self.place {
            return Err("The page isn't shown in that placement".into());
        }
        let limit = if place == StatePlace::Tab {
            64_000
        } else {
            16_000
        };
        let text = value.to_string();
        if text.encode_utf16().count() > limit {
            return Err(format!(
                "{} state is limited to {} KB",
                if place == StatePlace::Tab {
                    "Tab"
                } else {
                    "Block"
                },
                limit / 1000
            ));
        }
        let mut state = self.state.lock().unwrap();
        (state.save)(value.clone())?;
        state.value = value;
        Ok(())
    }
}
