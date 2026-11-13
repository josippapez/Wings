//! The plugin's storage, `wings.storage`. A test of this plugin's own, built for this computer, gets the same four calls
//! against a map in memory that records each write: the SDK's storage is empty there.

#[cfg(not(test))]
pub use wings_plugin::host::storage::{delete, entries, get, set};

#[cfg(test)]
pub use memory::{delete, entries, get, set};

#[cfg(test)]
pub mod memory {
    use serde_json::{Value, json};
    use std::{cell::RefCell, collections::BTreeMap};

    thread_local! {
        static STORED: RefCell<BTreeMap<String, Value>> = const { RefCell::new(BTreeMap::new()) };
        static WRITES: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    }

    pub fn get(key: &str) -> Result<Value, String> {
        Ok(peek(key).unwrap_or(Value::Null))
    }

    pub fn set(key: &str, value: &Value) -> Result<(), String> {
        WRITES.with(|w| w.borrow_mut().push(("set".into(), key.to_owned())));
        // Stored as JSON, like the host does: what is read back is a copy.
        STORED.with(|s| s.borrow_mut().insert(key.to_owned(), serde_json::from_str(&value.to_string()).unwrap()));
        Ok(())
    }

    pub fn delete(key: &str) -> Result<(), String> {
        WRITES.with(|w| w.borrow_mut().push(("delete".into(), key.to_owned())));
        STORED.with(|s| s.borrow_mut().remove(key));
        Ok(())
    }

    pub fn entries(prefix: &str) -> Result<Value, String> {
        Ok(STORED.with(|s| Value::Array(s.borrow().iter().filter(|(k, _)| k.starts_with(prefix)).map(|(k, v)| json!([k, v])).collect())))
    }

    /// What is stored under `key`, without recording a read.
    pub fn peek(key: &str) -> Option<Value> {
        STORED.with(|s| s.borrow().get(key).cloned())
    }

    pub fn keys() -> Vec<String> {
        STORED.with(|s| s.borrow().keys().cloned().collect())
    }

    /// Empties storage and the record of writes, and puts `initial` in.
    pub fn reset(initial: &[(&str, Value)]) {
        STORED.with(|s| *s.borrow_mut() = initial.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect());
        clear_writes();
    }

    pub fn writes() -> Vec<(String, String)> {
        WRITES.with(|w| w.borrow().clone())
    }

    pub fn clear_writes() {
        WRITES.with(|w| w.borrow_mut().clear());
    }
}
