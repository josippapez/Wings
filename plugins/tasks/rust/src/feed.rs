//! A page's view of one project's issues: read from storage, and read again whenever main says they changed. Changes go to
//! main as operations. A page in a worktree Wings made shows its main project's issues, so the project here has the main
//! project's id and name; its path stays the worktree's, where terminals open.

use crate::{domain::{Op, Store, key_number}, home::home_of, storage::{read_issue, read_store}};
use serde_json::{Value, json};
use std::{cell::RefCell, future::Future, rc::Rc};
use wings_plugin::{host, messages, task::spawn};

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    /// The id and name the issues are kept under.
    pub id: String,
    pub name: String,
    /// The folder terminals open in.
    pub path: String,
    pub branch: Option<String>,
}

#[derive(Default)]
pub struct Feed {
    /// Until the app says which project the page is about, nothing; then the project, or none when none is open.
    pub project: Option<Option<Project>>,
    /// The issues' project when a tab was opened for one other than the project on screen.
    pub only: Option<String>,
    pub store: Option<Rc<Store>>,
    /// Why the issues couldn't be read.
    pub error: Option<String>,
    /// Counts each read, so what is worked out from the issues is made again only when they changed.
    pub version: u64,
}

thread_local! {
    static FEED: RefCell<Feed> = RefCell::new(Feed::default());
}

pub fn with<T>(f: impl FnOnce(&mut Feed) -> T) -> T {
    FEED.with(|feed| f(&mut feed.borrow_mut()))
}

pub fn project() -> Option<Option<Project>> {
    with(|feed| feed.project.clone())
}

/// Reads the project's issues again.
pub fn reload() {
    let Some(Some(project)) = project() else {
        return;
    };
    let id = with(|feed| feed.only.clone()).unwrap_or_else(|| project.id.clone());
    match read_store(&id, &project.name) {
        Ok(store) => with(|feed| {
            feed.store = Some(Rc::new(store));
            feed.error = None;
            feed.version += 1;
        }),
        Err(error) => with(|feed| feed.error = Some(error)),
    }
}

/// Takes main's change into the page's copy: reads only the issues it names, and the counter it sends. False when the page
/// has to read everything instead: it has no copy yet, main sent no keys (an older main), or an issue can't be read.
fn patch(id: &str, changed: &Value) -> bool {
    let (Some(keys), Some(mut store)) = (changed["keys"].as_array(), with(|feed| feed.store.clone())) else {
        return false;
    };
    let mut read = Vec::new();
    for key in keys {
        let Some(key) = key.as_str() else {
            return false;
        };
        match read_issue(id, key) {
            Ok(issue) => read.push((key, issue)),
            Err(_) => return false,
        }
    }
    let store_mut = Rc::make_mut(&mut store);
    for (key, issue) in read {
        store_mut.issues.retain(|i| i.key != key);
        store_mut.issues.extend(issue);
    }
    store_mut.issues.sort_by_key(|i| key_number(&i.key));
    if let Some(prefix) = changed["prefix"].as_str() {
        store_mut.prefix = prefix.to_owned();
    }
    if let Some(next) = changed["next"].as_f64() {
        store_mut.next = next as u64;
    }
    with(|feed| {
        feed.store = Some(store);
        feed.error = None;
        feed.version += 1;
    });
    true
}

/// Follows the project on screen: its issues are read when it is known, and again when main says they changed. `only` is
/// the id of the project a tab was opened for, when it names one.
pub fn watch(only: Option<String>) {
    with(|feed| feed.only = only);
    host::on("project", |_, current| {
        if current.is_null() {
            with(|feed| feed.project = Some(None));
            return;
        }
        let current = current.clone();
        spawn(async move {
            let home = home_of(&current).await;
            let project = Project { id: home.id, name: home.name, path: current["path"].as_str().unwrap_or_default().to_owned(), branch: current["branch"].as_str().filter(|b| !b.is_empty()).map(str::to_owned) };
            let same = with(|feed| feed.project.as_ref() == Some(&Some(project.clone())));
            if !same {
                with(|feed| feed.project = Some(Some(project)));
                reload();
            }
        });
    });
    messages::on("issues", |changed| {
        let id = with(|feed| feed.only.clone().or_else(|| feed.project.clone().flatten().map(|p| p.id)));
        if let Some(id) = id.filter(|id| changed["projectId"] == id.as_str()) {
            if !patch(&id, changed) {
                reload();
            }
        }
    });
}

/// Asks main to apply an operation, and says why when it failed.
pub fn apply(op: Op) -> impl Future<Output = Result<(), String>> {
    let (id, name) = with(|feed| {
        let project = feed.project.clone().flatten();
        (feed.only.clone().or_else(|| project.clone().map(|p| p.id)).unwrap_or_default(), project.map(|p| p.name).unwrap_or_default())
    });
    async move { messages::ask("op", json!({ "projectId": id, "projectName": name, "op": op.to_value() })).await.map(drop) }
}

/// Opens an issue's tab.
pub fn open_issue(key: &str, project_id: &str, trail: Option<Vec<String>>, new_tab: bool) {
    let mut args = json!({ "key": key, "projectId": project_id });
    if let Some(trail) = trail {
        args["trail"] = json!(trail);
    }
    drop(host::call("openView", json!({ "id": "issue", "as": "tab", "args": args, "newTab": new_tab })));
}

pub fn open_board() {
    drop(host::call("openView", json!({ "id": "board", "as": "tab", "args": Value::Null, "newTab": false })));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::Actor, kv::memory, writer::{NO_STORE_CACHE, write}};
    use serde_json::json;
    use wings_plugin::host::testing::{deliver_broadcast, take_broadcasts};

    /// Starts following the project, once: each `watch` adds its listeners.
    fn listen() {
        thread_local! { static LISTENING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
        if !LISTENING.with(|l| l.replace(true)) {
            watch(None);
        }
    }

    fn note(key: &str) -> Op {
        Op::Note { key: key.to_owned(), text: "progress".to_owned() }
    }

    /// One log note with a board page open: what main and the page read from storage for it.
    fn log_with_a_page_open(cached: bool) -> ((usize, usize), (usize, usize)) {
        memory::reset(&[]);
        NO_STORE_CACHE.with(|c| c.set(false));
        for n in 0..40 {
            write("proj1", "web", &Op::Create(Op::create(&format!("Issue {n}"))), &Actor::You).unwrap();
        }
        with(|feed| *feed = Feed::default());
        listen();
        with(|feed| feed.project = Some(Some(Project { id: "proj1".into(), name: "web".into(), path: "/work/web".into(), branch: None })));
        reload();
        NO_STORE_CACHE.with(|c| c.set(!cached));
        take_broadcasts();
        memory::clear_reads();
        write("proj1", "web", &note("WEB-7"), &Actor::You).unwrap();
        let main = memory::reads();
        memory::clear_reads();
        for mut message in take_broadcasts() {
            if !cached {
                // Main before this change named no issues.
                message["payload"] = json!({ "projectId": "proj1" });
            }
            deliver_broadcast(&message);
        }
        let page = memory::reads();
        NO_STORE_CACHE.with(|c| c.set(false));
        let shown = with(|feed| feed.store.clone()).unwrap();
        assert_eq!(*shown, read_store("proj1", "web").unwrap(), "the page shows what is stored");
        assert_eq!(shown.issues[6].log.last().unwrap().text, "progress");
        (main, page)
    }

    #[test]
    fn a_log_note_with_a_page_open_reads_no_entries_in_main_and_one_issue_in_the_page() {
        let (main, page) = log_with_a_page_open(false);
        eprintln!("before: main (entries, gets) {main:?}, page {page:?}");
        assert!(main.0 >= 1 && page.0 >= 1);
        let (main, page) = log_with_a_page_open(true);
        eprintln!("after: main (entries, gets) {main:?}, page {page:?}");
        assert_eq!(main, (0, 0));
        assert_eq!(page, (0, 1));
    }

    #[test]
    fn a_page_patches_creates_and_removals_and_reads_everything_when_the_message_has_no_keys() {
        memory::reset(&[]);
        for n in 0..3 {
            write("proj1", "web", &Op::Create(Op::create(&format!("Issue {n}"))), &Actor::You).unwrap();
        }
        with(|feed| *feed = Feed::default());
        listen();
        with(|feed| feed.project = Some(Some(Project { id: "proj1".into(), name: "web".into(), path: "/work/web".into(), branch: None })));
        reload();
        take_broadcasts();
        let heard = || take_broadcasts().into_iter().for_each(|m| deliver_broadcast(&m));
        write("proj1", "web", &Op::Create(Op::create("Fourth")), &Actor::You).unwrap();
        heard();
        write("proj1", "web", &Op::Remove { key: "WEB-2".into(), children: false }, &Actor::You).unwrap();
        heard();
        memory::clear_reads();
        let patched = with(|feed| feed.store.clone()).unwrap();
        assert_eq!(*patched, read_store("proj1", "web").unwrap());
        assert_eq!(patched.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["WEB-1", "WEB-3", "WEB-4"]);
        assert_eq!(patched.next, 5);
        // An older main sends no keys: everything is read.
        write("proj1", "web", &note("WEB-1"), &Actor::You).unwrap();
        take_broadcasts();
        memory::clear_reads();
        deliver_broadcast(&json!({ "wings:sdk": 1, "kind": "tell", "name": "issues", "payload": { "projectId": "proj1" } }));
        assert_eq!(memory::reads().0, 1);
        // Another project's change is ignored.
        memory::clear_reads();
        deliver_broadcast(&json!({ "wings:sdk": 1, "kind": "tell", "name": "issues", "payload": { "projectId": "other", "keys": ["X-1"] } }));
        assert_eq!(memory::reads(), (0, 0));
    }
}
