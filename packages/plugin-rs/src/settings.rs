//! A setting kept in `host::storage`, that every frame of the plugin hears change.

use crate::{host, messages};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

thread_local! {
    /// This frame's listeners by key. Kept apart from any one `Setting`, as code makes a new one each time it reaches a
    /// setting (`prefs::x()`), and a `set` on one has to reach the listener another registered.
    static HERE: RefCell<HashMap<&'static str, Vec<Rc<dyn Fn(&Value)>>>> = RefCell::new(HashMap::new());
}

struct Inner<T> {
    key: &'static str,
    default: T,
    read: fn(&Value) -> Option<T>,
    write: fn(&T) -> Value,
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
        Self(Rc::new(Inner { key, default, read, write }))
    }

    fn name(&self) -> String {
        format!("setting:{}", self.0.key)
    }

    pub fn get(&self) -> T {
        let stored = host::storage::get(self.0.key).unwrap_or(Value::Null);
        if stored.is_null() { None } else { (self.0.read)(&stored) }.unwrap_or_else(|| self.0.default.clone())
    }

    pub fn set(&self, value: T) -> Result<(), String> {
        let written = (self.0.write)(&value);
        host::storage::set(self.0.key, &written)?;
        let here = HERE.with(|here| here.borrow().get(self.0.key).cloned().unwrap_or_default());
        for listener in here {
            listener(&written);
        }
        messages::tell(&self.name(), &Value::Null);
        Ok(())
    }

    /// Hears this frame's own `set` and the other frames' ones.
    pub fn on_change(&self, listener: impl Fn(&T) + 'static) {
        let this = self.clone();
        let read = self.clone();
        let here: Rc<dyn Fn(&Value)> = Rc::new(move |written| listener(&(read.0.read)(written).unwrap_or_else(|| read.get())));
        HERE.with(|all| all.borrow_mut().entry(self.0.key).or_default().push(here.clone()));
        messages::on(&self.name(), move |_| here(&(this.0.write)(&this.get())));
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
    fn a_set_through_another_handle_of_the_same_setting_reaches_the_listener() {
        let make = || Setting::boolean("split", false);
        let heard = Rc::new(RefCell::new(0));
        let seen = heard.clone();
        make().on_change(move |_| *seen.borrow_mut() += 1);
        make().set(true).unwrap();
        assert_eq!(*heard.borrow(), 1);
    }

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
