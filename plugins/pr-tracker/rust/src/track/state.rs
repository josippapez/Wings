//! What the main script knows about the panes' and the lists' pull requests: every map its modules read and write.

use crate::types::{Diff, FailureDetail, Link, Status};
use serde_json::Value;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

/// A terminal pane, as `wings.onPanes` lists it: the parts the tracker reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    pub id: String,
    pub cwd: String,
    pub session: Option<Session>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// `claude` or `opencode`. Wings before 0.1.60 sends none, and every session was Claude Code's then.
    pub kind: Option<String>,
    pub id: String,
    pub state: String,
}

impl Pane {
    pub fn from_json(pane: &Value) -> Option<Self> {
        let text = |value: &Value| value.as_str().map(str::to_owned);
        Some(Self {
            id: text(&pane["paneId"])?,
            cwd: text(&pane["cwd"]).unwrap_or_default(),
            session: pane["session"].as_object().map(|_| Session { kind: text(&pane["session"]["kind"]), id: text(&pane["session"]["sessionId"]).unwrap_or_default(), state: text(&pane["session"]["state"]).unwrap_or_default() }),
        })
    }
}

/// What a status load gave: the status, or what went wrong.
pub type Entry = Result<Status, String>;

/// A loaded status and when it landed, so a tab that told main its status keeps the poll from loading it again.
#[derive(Clone)]
pub struct Loaded {
    pub entry: Entry,
    pub at: f64,
}

/// The details of a PR's failed checks, for the set of failures `key` names.
#[derive(Clone)]
pub struct Failures {
    pub key: String,
    pub list: Option<Vec<FailureDetail>>,
}

/// The newest diff viewers are kept track of, since Wings doesn't say when one is closed.
const MAX_VIEWERS: usize = 20;
/// PRs from the lists and tabs that no pane is on.
const MAX_KNOWN: usize = 300;

#[derive(Default)]
pub struct State {
    pub panes: Vec<Pane>,
    /// paneId → PR ref.
    pub links: HashMap<String, Link>,
    /// paneId → commits to push and pull on the pane's branch, as of the last fetch.
    pub sync: HashMap<String, (i64, i64)>,
    /// PR url → its status, or what went wrong loading it.
    pub status: HashMap<String, Loaded>,
    /// PRs refreshed because you clicked Refresh: background polls don't show a spinner.
    pub manual: Vec<String>,
    /// PR url → the patch and comments of the status `key` names, fetched ahead so "View changes" opens instantly.
    pub diffs: HashMap<String, (Option<String>, Rc<Diff>)>,
    /// Open diff viewer id → PR url, so a reply typed in it reaches the right PR.
    pub viewers: Vec<(String, String)>,
    pub failures: HashMap<String, Failures>,
    /// PR url → its ref, for PRs from the project lists and the PR page that no pane is on.
    pub known: Vec<(String, Link)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Reads or changes the state. Never hold the borrow across an `.await`.
pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| f(&mut state.borrow_mut()))
}

pub fn pane(id: &str) -> Option<Pane> {
    with(|s| s.panes.iter().find(|pane| pane.id == id).cloned())
}

pub fn fail_key(st: &Status) -> String {
    st.failing.iter().map(|f| f.id.as_str()).collect::<Vec<_>>().join(" ")
}

/// The ref of a PR a pane is on, else one a list or a tab found before.
pub fn link_for(url: &str) -> Option<Link> {
    with(|s| s.links.values().find(|link| link.url == url).cloned().or_else(|| s.known.iter().find(|(known, _)| known == url).map(|(_, link)| link.clone())))
}

pub fn remember(link: Link) {
    with(|s| {
        s.known.retain(|(url, _)| *url != link.url);
        if s.known.len() >= MAX_KNOWN {
            s.known.remove(0);
        }
        s.known.push((link.url.clone(), link));
    });
}

pub fn status_of(url: &str) -> Option<Loaded> {
    with(|s| s.status.get(url).cloned())
}

/// The pane that has an agent in a session on the PR `url`.
pub fn agent_on(url: &str) -> Option<(Pane, Session)> {
    with(|s| s.panes.iter().find_map(|pane| pane.session.clone().filter(|_| s.links.get(&pane.id).is_some_and(|link| link.url == url)).map(|session| (pane.clone(), session))))
}

pub fn add_viewer(id: String, url: String) {
    with(|s| {
        s.viewers.retain(|(known, _)| *known != id);
        s.viewers.push((id, url));
        if s.viewers.len() > MAX_VIEWERS {
            s.viewers.remove(0);
        }
    });
}

pub fn viewer_url(id: &str) -> Option<String> {
    with(|s| s.viewers.iter().find(|(known, _)| known == id).map(|(_, url)| url.clone()))
}

/// What is kept per PR (its status, diff and failed checks) goes once no pane, list, open diff viewer or tab shows the
/// PR. `listed` is the urls of every list, `tabs` those of the tabs that told main their status lately.
pub fn prune(listed: &[String]) {
    with(|s| {
        let mut shown: Vec<&str> = listed.iter().map(String::as_str).collect();
        shown.extend(s.links.values().map(|link| link.url.as_str()));
        shown.extend(s.viewers.iter().map(|(_, url)| url.as_str()));
        let shown: Vec<String> = shown.into_iter().map(str::to_owned).collect();
        let keep = |url: &String| shown.contains(url);
        s.status.retain(|url, _| keep(url));
        s.diffs.retain(|url, _| keep(url));
        s.failures.retain(|url, _| keep(url));
    });
}
