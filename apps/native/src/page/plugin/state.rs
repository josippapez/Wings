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
            })),
        }
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
