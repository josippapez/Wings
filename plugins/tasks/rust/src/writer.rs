//! The only writer of the issues. Pages ask for operations and tools run the same ones, each against what storage holds at
//! that moment. Storage answers at once, so a read, the change and its write never interleave with another operation's.
//! After a change it tells the pages and refreshes the project's section in the sidebar.

use crate::{
    domain::{Actor, Issue, Op, Store},
    ops::apply_op,
    sidebar::push_section,
    storage::{changed_keys, migrate_legacy, read_store, write_store},
};
use serde_json::json;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
use wings_plugin::{messages, task::spawn};
#[cfg(not(test))]
use wings_plugin::host;

thread_local! {
    /// The projects whose older entries are saved as one entry per issue in this run, so a write only adds to that.
    static MIGRATED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// The key prefix of each project that has issues, as main last saw it.
    static PREFIXES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// The issue keys of each project, as main last saw them.
    /// Each project's store as main last read or wrote it. Main is the only writer, so it stays right without reading again.
    static STORES: RefCell<HashMap<String, Rc<Store>>> = RefCell::new(HashMap::new());
    static KEYS: RefCell<HashMap<String, Vec<String>>> = RefCell::new(HashMap::new());
}

#[cfg(test)]
thread_local! {
    /// Set by a test to read every store from storage, as main did before it kept them.
    pub static NO_STORE_CACHE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn cache_on() -> bool {
    #[cfg(test)]
    return !NO_STORE_CACHE.with(std::cell::Cell::get);
    #[cfg(not(test))]
    true
}

#[cfg(test)]
thread_local! {
    pub static CLOCK: std::cell::Cell<f64> = const { std::cell::Cell::new(1.0) };
}

/// Milliseconds since the epoch. A test sets its own clock.
pub fn now() -> f64 {
    #[cfg(test)]
    return CLOCK.with(std::cell::Cell::get);
    #[cfg(not(test))]
    host::now()
}

pub fn remember_prefix(project_id: &str, prefix: &str, issues: usize) {
    PREFIXES.with(|prefixes| {
        let mut prefixes = prefixes.borrow_mut();
        if issues > 0 {
            prefixes.insert(project_id.to_owned(), prefix.to_owned());
        } else {
            prefixes.remove(project_id);
        }
    });
}

/// Keeps the keys of a project's issues and, when they changed, tells Wings: a key in a chat's message is a link to its
/// issue only while there is one, so "UTF-8" isn't.
fn remember_keys(project_id: &str, issues: &[Issue]) {
    let keys: Vec<String> = issues.iter().map(|issue| issue.key.clone()).collect();
    let changed = KEYS.with(|known| known.borrow_mut().insert(project_id.to_owned(), keys.clone()) != Some(keys));
    if changed {
        let mut all: Vec<String> = KEYS.with(|known| known.borrow().values().flatten().cloned().collect());
        all.sort();
        all.dedup();
        spawn(async move {
            let _ = wings_plugin::host::set_link_words("issue", Some(&all)).await;
        });
    }
}

pub fn known_prefix(project_id: &str) -> bool {
    PREFIXES.with(|prefixes| prefixes.borrow().contains_key(project_id))
}

pub fn prefixes() -> Vec<String> {
    let mut all: Vec<String> = PREFIXES.with(|prefixes| prefixes.borrow().values().cloned().collect());
    all.sort();
    all.dedup();
    all
}

fn migrate_once(project_id: &str, project_name: &str) -> Result<(), String> {
    if MIGRATED.with(|migrated| migrated.borrow().contains(project_id)) {
        return Ok(());
    }
    migrate_legacy(project_id, project_name)?;
    MIGRATED.with(|migrated| migrated.borrow_mut().insert(project_id.to_owned()));
    Ok(())
}

/// A project's store: read from storage the first time and kept after that, since only `write` changes it.
pub fn stored(project_id: &str, project_name: &str) -> Result<Rc<Store>, String> {
    if let Some(store) = STORES.with(|stores| stores.borrow().get(project_id).cloned()).filter(|_| cache_on()) {
        return Ok(store);
    }
    let store = Rc::new(read_store(project_id, project_name)?);
    if cache_on() {
        STORES.with(|stores| stores.borrow_mut().insert(project_id.to_owned(), store.clone()));
    }
    Ok(store)
}

/// Drops what was kept, so the next read is from storage.
pub fn forget_stores() {
    STORES.with(|stores| stores.borrow_mut().clear());
}

pub struct Written {
    pub issue: Issue,
    pub changed: bool,
}

/// Applies `op` as `by` to a project's issues, saves it, and tells the pages. An op that changes nothing saves and sends
/// nothing.
pub fn write(project_id: &str, project_name: &str, op: &Op, by: &Actor) -> Result<Written, String> {
    migrate_once(project_id, project_name)?;
    let before = stored(project_id, project_name)?;
    let applied = apply_op(&before, op, by, now())?;
    remember_prefix(project_id, &applied.store.prefix, applied.store.issues.len());
    remember_keys(project_id, &applied.store.issues);
    if applied.changed {
        if let Err(error) = write_store(project_id, &before, &applied.store) {
            // Some of it may have been saved: read it from storage next time.
            STORES.with(|stores| stores.borrow_mut().remove(project_id));
            return Err(error);
        }
        let keys = changed_keys(&before, &applied.store);
        // The pages patch these issues and take the counter from here; one from before this build has no keys and reads it all.
        messages::tell("issues", &json!({ "projectId": project_id, "keys": keys, "prefix": applied.store.prefix, "next": applied.store.next }));
        let issues = applied.store.issues.clone();
        if cache_on() {
            STORES.with(|stores| stores.borrow_mut().insert(project_id.to_owned(), Rc::new(applied.store)));
        }
        spawn(push_section(project_id.to_owned(), issues));
    }
    Ok(Written { issue: applied.issue, changed: applied.changed })
}

/// Picks up a project Wings listed: saves older data in the current format, then fills its section from storage.
pub async fn load_project(project_id: String, project_name: String) {
    // A project whose older data can't be moved still shows what it can read.
    let _ = migrate_once(&project_id, &project_name);
    if let Ok(store) = stored(&project_id, &project_name) {
        remember_prefix(&project_id, &store.prefix, store.issues.len());
        remember_keys(&project_id, &store.issues);
        push_section(project_id, store.issues.clone()).await;
    }
}
