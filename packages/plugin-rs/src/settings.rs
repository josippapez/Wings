//! A setting kept in `host::storage`, that every frame of the plugin hears change: `wings-plugin-sdk/settings`.

use crate::{host, messages};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

struct Inner<T> {
    key: &'static str,
    default: T,
    read: fn(&Value) -> Option<T>,
    write: fn(&T) -> Value,
    here: RefCell<Vec<Rc<dyn Fn(&T)>>>,
}

/// A setting stored under `key`, read as `default` when it isn't set or `read` doesn't accept what is stored. Make it
/// where main, the pages and the settings page can all reach it. `set` tells the other frames.
pub struct Setting<T>(Rc<Inner<T>>);

impl<T> Clone for Setting<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Clone + 'static> Setting<T> {
    pub fn new(key: &'static str, default: T, read: fn(&Value) -> Option<T>, write: fn(&T) -> Value) -> Self {
        Self(Rc::new(Inner { key, default, read, write, here: RefCell::new(Vec::new()) }))
    }

    fn name(&self) -> String {
        format!("setting:{}", self.0.key)
    }

    pub fn get(&self) -> T {
        let stored = host::storage::get(self.0.key).unwrap_or(Value::Null);
        if stored.is_null() { None } else { (self.0.read)(&stored) }.unwrap_or_else(|| self.0.default.clone())
    }

    pub fn set(&self, value: T) -> Result<(), String> {
        host::storage::set(self.0.key, &(self.0.write)(&value))?;
        for listener in self.0.here.borrow().clone() {
            listener(&value);
        }
        messages::tell(&self.name(), &Value::Null);
        Ok(())
    }

    /// Hears this frame's own `set` and the other frames' ones.
    pub fn on_change(&self, listener: impl Fn(&T) + 'static) {
        let listener: Rc<dyn Fn(&T)> = Rc::new(listener);
        self.0.here.borrow_mut().push(listener.clone());
        let this = self.clone();
        messages::on(&self.name(), move |_| listener(&this.get()));
    }
}

impl Setting<bool> {
    pub fn boolean(key: &'static str, default: bool) -> Self {
        Self::new(key, default, |stored| stored.as_bool(), |value| json!(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_setting_reads_as_its_default_and_a_listener_hears_a_set_here() {
        let setting = Setting::<u32>::new("minutes", 5, |stored| stored.as_u64().map(|n| n as u32).filter(|n| [5, 10].contains(n)), |n| json!(n));
        // Outside Wings nothing is kept: every key reads as null.
        assert_eq!(setting.get(), 5);
        let heard = Rc::new(RefCell::new(Vec::new()));
        let seen = heard.clone();
        setting.on_change(move |value| seen.borrow_mut().push(*value));
        setting.set(10).unwrap();
        assert_eq!(*heard.borrow(), [10]);
        assert!(!Setting::boolean("on", false).get());
    }

    #[test]
    fn what_the_check_refuses_reads_as_the_default() {
        let read: fn(&Value) -> Option<u32> = |stored| stored.as_u64().map(|n| n as u32).filter(|n| *n == 5);
        assert_eq!(read(&json!(7)), None);
        assert_eq!(read(&json!(5)), Some(5));
    }
}
