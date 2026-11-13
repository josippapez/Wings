//! A page's view of one project's issues: read from storage, and read again whenever main says they changed. Changes go to
//! main as operations. A page in a worktree Wings made shows its main project's issues, so the project here has the main
//! project's id and name; its path stays the worktree's, where terminals open.

use crate::{ask::ask, domain::{Op, Store}, home::home_of, storage::read_store};
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
        if id.is_some_and(|id| changed["projectId"] == id.as_str()) {
            reload();
        }
    });
}

/// Asks main to apply an operation, and says why when it failed.
pub fn apply(op: Op) -> impl Future<Output = Result<(), String>> {
    let (id, name) = with(|feed| {
        let project = feed.project.clone().flatten();
        (feed.only.clone().or_else(|| project.clone().map(|p| p.id)).unwrap_or_default(), project.map(|p| p.name).unwrap_or_default())
    });
    async move { ask("op", json!({ "projectId": id, "projectName": name, "op": op.to_value() })).await }
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
