//! The only writer of the issues. Pages ask for operations and tools run the same ones, each against what storage holds at
//! that moment. Storage answers at once, so a read, the change and its write never interleave with another operation's.
//! After a change it tells the pages and refreshes the project's section in the sidebar.

use crate::{
    domain::{Actor, Issue, Op},
    ops::apply_op,
    sidebar::push_section,
    storage::{migrate_legacy, read_store, write_store},
};
use serde_json::json;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};
use wings_plugin::{messages, task::spawn};
#[cfg(not(test))]
use wings_plugin::host;

thread_local! {
    /// The projects whose older entries are saved as one entry per issue in this run, so a write only adds to that.
    static MIGRATED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// The key prefix of each project that has issues, as main last saw it.
    static PREFIXES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
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

pub struct Written {
    pub issue: Issue,
    pub changed: bool,
}

/// Applies `op` as `by` to a project's issues, saves it, and tells the pages. An op that changes nothing saves and sends
/// nothing.
pub fn write(project_id: &str, project_name: &str, op: &Op, by: &Actor) -> Result<Written, String> {
    migrate_once(project_id, project_name)?;
    let before = read_store(project_id, project_name)?;
    let applied = apply_op(&before, op, by, now())?;
    remember_prefix(project_id, &applied.store.prefix, applied.store.issues.len());
    if applied.changed {
        write_store(project_id, &before, &applied.store)?;
        messages::tell("issues", &json!({ "projectId": project_id }));
        spawn(push_section(project_id.to_owned(), applied.store.issues));
    }
    Ok(Written { issue: applied.issue, changed: applied.changed })
}

/// Picks up a project Wings listed: saves older data in the current format, then fills its section from storage.
pub async fn load_project(project_id: String, project_name: String) {
    // A project whose older data can't be moved still shows what it can read.
    let _ = migrate_once(&project_id, &project_name);
    if let Ok(store) = read_store(&project_id, &project_name) {
        remember_prefix(&project_id, &store.prefix, store.issues.len());
        push_section(project_id, store.issues).await;
    }
}
