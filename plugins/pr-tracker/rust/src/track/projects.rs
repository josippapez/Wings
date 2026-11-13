//! The Pull requests section under each project in the sidebar: its open PRs, yours first, then the ones waiting for your
//! review, then, when Settings says so, the rest. The Pull requests block and the tab's picker read the same lists.

use super::{inbox, problems, state::{self, Loaded}};
use crate::{
    args::args_of,
    finished::{self, Tracked, event_for, final_state},
    forges,
    providers::link_ref,
    pulls::{Agent, BLOCK_ROWS, PullRow, PullsAnswer, RowBadge, Scope, order_pulls},
    remote::remote_for,
    types::{ListedPr, Status},
    watch::{NoteItem, new_pulls, new_reviews, notes_for},
};
use serde_json::{Value, json};
use std::{cell::{Cell, RefCell}, collections::{HashMap, HashSet}, hash::{Hash, Hasher}, rc::Rc};
use wings_plugin::{cache::Cached, format::plural, host, remote::Remote, task::{SingleFlight, join_all, spawn}};

const SECTION: &str = "pulls";
/// The host's limit on a section's rows.
const MAX_ROWS: usize = 50;
/// A little under the 60 s poll, so each poll fetches fresh while a burst of pane changes shares one list.
const LIST_TTL_MS: f64 = 55_000.0;

/// A project, as `wings.projects` lists it.
#[derive(Clone, Debug)]
pub struct Project {
    pub id: String,
    pub path: String,
    pub name: String,
}

impl Project {
    fn from_json(project: &Value) -> Option<Self> {
        let text = |name: &str| project[name].as_str().map(str::to_owned);
        Some(Self { id: text("id")?, path: text("path")?, name: text("name").unwrap_or_default() })
    }
}

/// What a repo's list is asked by: the repo, whatever folder or form of its remote the project has.
#[derive(Clone)]
struct ListKey {
    key: String,
    cwd: String,
    remote: Remote,
}

impl PartialEq for ListKey {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for ListKey {}
impl Hash for ListKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

thread_local! {
    /// repo → its open PRs. A failed list is kept for as long, so a signed-out CLI isn't asked on every pane change.
    static LISTS: Cached<ListKey, Vec<ListedPr>> = Cached::new(LIST_TTL_MS, LIST_TTL_MS, |list: ListKey| async move {
        match forges::list(&list.cwd, &list.remote).await {
            Ok(items) => {
                problems::answered(&list.remote);
                Ok(items)
            }
            Err(error) => {
                problems::explains(&list.remote, &error);
                Err(error.to_wire())
            }
        }
    });
    /// PR url → comments and reviews counted when you last looked, to tell what's new.
    static SEEN: RefCell<Option<HashMap<String, u64>>> = const { RefCell::new(None) };
    /// PR url → comments and reviews counted in the latest list.
    static ACTIVITY: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
    /// repo → the urls of its open PRs on the last check, to tell which are new.
    static OPENED: RefCell<HashMap<String, HashSet<String>>> = RefCell::new(HashMap::new());
    /// repo → the urls of its open PRs that asked for your review on the last check, to tell which ask now.
    static ASKED: RefCell<HashMap<String, HashSet<String>>> = RefCell::new(HashMap::new());
    /// repo → its open PRs on the last check, as `settle` keeps them, to tell which merged or closed.
    static TRACKED: RefCell<HashMap<String, Vec<Tracked>>> = RefCell::new(HashMap::new());
    /// What Settings says to notify about: `off`, `review` or `all`.
    static NOTIFY: Cell<&'static str> = const { Cell::new("off") };
    /// What Settings says the sidebar lists: `mine` or `all`.
    static SIDEBAR: Cell<&'static str> = const { Cell::new("mine") };
    /// `SEEN` as last saved.
    static PERSISTED: RefCell<String> = const { RefCell::new(String::new()) };
    /// The `fresh` of the next run: set by every call that asks for it, read by the run that starts.
    static WANT_FRESH: Cell<bool> = const { Cell::new(false) };
    /// One run at a time: a run asked for during another one goes again after it.
    static RUN: SingleFlight = SingleFlight::new(|| async { run_projects().await });
}

/// What was seen before this run, read once.
fn with_seen<R>(f: impl FnOnce(&mut HashMap<String, u64>) -> R) -> R {
    SEEN.with(|slot| {
        let mut slot = slot.borrow_mut();
        let seen = slot.get_or_insert_with(|| {
            let saved = host::storage::get("seen").unwrap_or(Value::Null);
            let seen: HashMap<String, u64> = saved.as_object().map(|map| map.iter().filter_map(|(url, count)| Some((url.clone(), count.as_u64()?))).collect()).unwrap_or_default();
            PERSISTED.with(|persisted| *persisted.borrow_mut() = sorted_json(&seen));
            seen
        });
        f(seen)
    })
}

fn sorted_json(seen: &HashMap<String, u64>) -> String {
    json!(seen.iter().collect::<std::collections::BTreeMap<_, _>>()).to_string()
}

/// A repo never listed is fetched. A listed one is served at any age when `any_age` (a redraw from pane changes), else
/// only while younger than the TTL.
async fn list_for(cwd: &str, remote: &Remote, any_age: bool) -> Result<Vec<ListedPr>, String> {
    let key = ListKey { key: forges::repo_key(remote), cwd: cwd.to_owned(), remote: remote.clone() };
    let lists = LISTS.with(Cached::clone);
    if any_age && let Some(kept) = lists.peek(&key) {
        return kept;
    }
    lists.get(key).await
}

/// Keeps every listed PR's ref for the tab and the diff, and the full status of a list that carries it for the rows.
fn remember(items: &[ListedPr]) {
    for item in items {
        state::remember(item.link.clone());
        if item.loaded {
            let url = item.link.url.clone();
            state::with(|s| s.status.insert(url, Loaded { entry: Ok(item.status.clone()), at: host::now() }));
        }
        if let Some(count) = item.activity {
            ACTIVITY.with(|activity| activity.borrow_mut().insert(item.link.url.clone(), count));
        }
    }
}

/// The full status, once a pane or the tab has loaded it, knows about checks that the list can't see.
fn status_of(item: &ListedPr) -> Status {
    state::status_of(&item.link.url).and_then(|loaded| loaded.entry.ok()).unwrap_or_else(|| item.status.clone())
}

fn unread(url: &str) -> u32 {
    let count = ACTIVITY.with(|activity| activity.borrow().get(url).copied());
    let seen = with_seen(|seen| seen.get(url).copied());
    match (count, seen) {
        (Some(count), Some(seen)) => count.saturating_sub(seen) as u32,
        _ => 0,
    }
}

fn row(item: &ListedPr, cwd: &str) -> Value {
    let link = &item.link;
    let st = status_of(item);
    let (failed, running) = (st.checks.failed, st.checks.running);
    let unread = unread(&link.url);
    let agent = state::agent_on(&link.url).map(|(_, session)| if session.state == "working" { "agent working" } else { "agent here" });
    let mut parts = vec![link_ref(link)];
    if !item.mine {
        parts.extend(if item.reviewing { Some("needs your review".to_owned()) } else { item.author.clone() });
    }
    if failed > 0 {
        parts.push(format!("{} failing", plural(failed as usize, "check")));
    } else if running > 0 {
        parts.push("checks running".into());
    }
    if unread > 0 {
        parts.push(format!("{unread} unread"));
    }
    parts.extend(agent.map(str::to_owned));
    let detail: String = parts.join(" · ").chars().take(120).collect();
    // The host caps an id at 64 characters, and a URL can be longer.
    json!({
        "id": format!("{}:{}", provider_name(link.provider), link.number),
        "label": item.title.chars().take(120).collect::<String>(),
        "detail": detail,
        "badge": { "text": st.label, "tone": st.tone },
        "icon": st.icon,
        "open": { "view": "pr", "args": args_of(link, &item.title, cwd) },
    })
}

fn provider_name(provider: crate::types::Provider) -> String {
    serde_json::to_value(provider).ok().and_then(|name| name.as_str().map(str::to_owned)).unwrap_or_default()
}

async fn run_project(project: Project, remote: Remote, fresh: bool, sidebar: &'static str, live: Rc<RefCell<HashSet<String>>>) -> bool {
    let known = forges::is_known(&remote);
    let items = if known {
        match list_for(&project.path, &remote, !fresh).await {
            Ok(items) => items,
            Err(_) => return false,
        }
    } else {
        Vec::new()
    };
    remember(&items);
    if known {
        let repo = forges::repo_key(&remote);
        announce(&project, &repo, &items);
        let (finished_project, finished_repo, finished_items) = (project.clone(), repo, items.clone());
        spawn(async move { announce_finished(&finished_project, &finished_repo, &finished_items).await });
    }
    live.borrow_mut().extend(items.iter().map(|item| item.link.url.clone()));
    // A PR seen for the first time starts from what it has, so only what comes after counts as unread.
    for item in &items {
        if let Some(count) = item.activity {
            with_seen(|seen| {
                seen.entry(item.link.url.clone()).or_insert(count);
            });
        }
    }
    // Yours, then the ones asked of you, then the rest when Settings says the sidebar lists every open PR.
    let shown = items.iter().filter(|i| i.mine).chain(items.iter().filter(|i| !i.mine && i.reviewing)).chain(items.iter().filter(|i| !i.mine && !i.reviewing && sidebar == "all"));
    let rows: Vec<Value> = shown.take(MAX_ROWS).map(|item| row(item, &project.path)).collect();
    // The native app has no Project page and refuses sections: that must not skip the mentions below.
    drop(host::set_project_section(&project.id, SECTION, &if rows.is_empty() { Value::Null } else { json!(rows) }).await);
    if known {
        drop(inbox::check_mentions(&project, &remote, &items).await);
    }
    true
}

async fn run_projects() {
    let fresh = WANT_FRESH.replace(false);
    let sidebar = SIDEBAR.get();
    let projects: Vec<Project> = host::projects().await.ok().and_then(|list| list.as_array().map(|list| list.iter().filter_map(Project::from_json).collect())).unwrap_or_default();
    let live = Rc::new(RefCell::new(HashSet::new()));
    // Every remote is read before any list is asked for, so the lists of all the projects are asked in one turn and a forge that batches reads them in one call.
    let remotes = join_all(projects.iter().map(|project| {
        let path = project.path.clone();
        Box::pin(async move { remote_for(&path).await }) as std::pin::Pin<Box<dyn std::future::Future<Output = Remote>>>
    }).collect()).await;
    let runs = projects.iter().cloned().zip(remotes).map(|(project, remote)| Box::pin(run_project(project, remote, fresh, sidebar, live.clone())) as std::pin::Pin<Box<dyn std::future::Future<Output = bool>>>);
    let complete = join_all(runs.collect()).await.into_iter().all(|done| done);
    if complete {
        // Forget what you've seen of PRs that are no longer open, but only when every list came back.
        let live = live.borrow().clone();
        with_seen(|seen| seen.retain(|url, _| live.contains(url)));
        ACTIVITY.with(|activity| activity.borrow_mut().retain(|url, _| live.contains(url)));
        let urls: Vec<String> = live.iter().cloned().collect();
        state::prune(&urls);
        inbox::prune_mentions(&live);
    }
    let json = with_seen(|seen| sorted_json(seen));
    if PERSISTED.with(|persisted| *persisted.borrow() != json) {
        drop(host::storage::set("seen", &serde_json::from_str(&json).unwrap_or_default()));
    }
    PERSISTED.with(|persisted| *persisted.borrow_mut() = json);
}

/// Sets each project's section from its repo's list (one call per repo). Only `fresh`, the focused poll, goes back to the
/// network for a repo it already listed; pane and project changes redraw the rows, with the agent marks, from the cache.
pub async fn refresh_projects(fresh: bool) {
    if fresh {
        WANT_FRESH.set(true);
    }
    RUN.with(SingleFlight::clone).call().await;
}

pub fn set_notify(to: &'static str) {
    NOTIFY.set(to);
}

pub fn set_sidebar(to: &'static str) {
    SIDEBAR.set(to);
    spawn(async { refresh_projects(false).await });
}

fn notify(project: &Project, note: crate::watch::Notification) {
    drop(host::call("notify", json!({ "title": note.title, "body": note.body, "actions": note.actions, "projectId": project.id })));
}

/// Notifies about the PRs that ask for your review and didn't on the last check, and with "all" also the PRs that
/// weren't open. Tracked with notifications off too, so turning them on doesn't announce every open PR.
fn announce(project: &Project, repo: &str, items: &[ListedPr]) {
    let before_opened = OPENED.with(|opened| opened.borrow().get(repo).cloned());
    let before_asked = ASKED.with(|asked| asked.borrow().get(repo).cloned());
    let reviews = new_reviews(before_asked.as_ref(), items);
    let fresh: Vec<&ListedPr> = new_pulls(before_opened.as_ref(), items).into_iter().filter(|i| !reviews.iter().any(|r| r.link.url == i.link.url)).collect();
    OPENED.with(|opened| opened.borrow_mut().insert(repo.to_owned(), items.iter().map(|i| i.link.url.clone()).collect()));
    ASKED.with(|asked| asked.borrow_mut().insert(repo.to_owned(), items.iter().filter(|i| i.reviewing).map(|i| i.link.url.clone()).collect()));
    let notify_about = NOTIFY.get();
    if notify_about == "off" {
        return;
    }
    let notes = |list: &[&ListedPr], review: bool| {
        let items: Vec<NoteItem> = list.iter().map(|i| NoteItem { title: i.title.clone(), author: i.author.clone(), url: i.link.url.clone(), args: serde_json::to_value(args_of(&i.link, &i.title, &project.path)).unwrap_or_default() }).collect();
        notes_for(&project.name, &items, review)
    };
    for note in notes(&reviews, true).into_iter().chain(if notify_about == "all" { notes(&fresh, false) } else { Vec::new() }) {
        notify(project, note);
    }
}

/// Tells the other built-in plugins (Issues) about a PR that left the repo's open list because it merged or closed. The
/// list can't say which, so each gone PR's own status is read once; one that can't be read is tried again on the next lists.
async fn announce_finished(project: &Project, repo: &str, items: &[ListedPr]) {
    let before = TRACKED.with(|tracked| tracked.borrow().get(repo).cloned());
    let (next, gone) = finished::settle(before.as_deref(), items, &project.id, &project.path);
    TRACKED.with(|tracked| tracked.borrow_mut().insert(repo.to_owned(), next));
    for t in gone {
        let read = async {
            let link = forges::link_of(&args_of(&t.link, &t.title, &t.cwd)).await.map_err(|error| error.to_string())?;
            forges::status(&link, false).await.map_err(|error| error.to_string())
        };
        match read.await {
            Ok(st) => {
                if let Some(ended) = final_state(&st) {
                    drop(host::call("emit", json!({ "name": "pr-state", "data": event_for(&t, ended) })).await);
                }
            }
            Err(_) => TRACKED.with(|tracked| {
                if let Some(next) = tracked.borrow_mut().get_mut(repo) {
                    finished::retry(next, &t);
                }
            }),
        }
    }
}

/// Sign-in or Try again: the lists that failed, or came back empty for lack of it, are fetched again.
pub fn forget_lists() {
    LISTS.with(Cached::clear);
}

/// The tab for a PR is open: everything up to its latest comment counts as read.
pub fn mark_seen(url: &str) {
    inbox::mark_read(url);
    let Some(count) = ACTIVITY.with(|activity| activity.borrow().get(url).copied()) else { return };
    with_seen(|seen| seen.insert(url.to_owned(), count));
    spawn(async { refresh_projects(false).await });
}

/// The open pull requests of one project's repo for a Pull requests block. It shares the cache of the sidebar section, so
/// a block and a section cost one list per repo. A block asks only while visible and focused, so it never serves a list
/// older than the TTL.
pub async fn pulls_for(path: &str) -> Result<Value, String> {
    let answer = |answer: PullsAnswer| serde_json::to_value(answer).map_err(|error| error.to_string());
    let remote = remote_for(path).await;
    if !forges::is_known(&remote) {
        return answer(PullsAnswer { none: Some(true), host: match &remote { Remote::None { host } => host.clone(), _ => None }, ..PullsAnswer::default() });
    }
    let items = match list_for(path, &remote, false).await {
        Ok(items) => items,
        Err(error) => {
            // A host that needs signing in, a token or a CLI gets a short message and a button, not the error.
            if let Some(problem) = problems::shown_problem(Some(&remote)) {
                return answer(PullsAnswer { problem: Some(problem), ..PullsAnswer::default() });
            }
            return Err(error.chars().take(300).collect());
        }
    };
    remember(&items);
    let rows: Vec<PullRow> = items
        .iter()
        .map(|item| {
            let link = &item.link;
            let st = status_of(item);
            PullRow {
                id: format!("{}:{}", provider_name(link.provider), link.number),
                title: item.title.chars().take(160).collect(),
                number: link_ref(link),
                author: item.author.clone(),
                branch: st.branch.clone(),
                scope: if item.mine { Scope::Mine } else if item.reviewing { Scope::Review } else { Scope::Other },
                draft: st.label == "Draft",
                badge: RowBadge { text: st.label.clone(), tone: st.tone.clone() },
                review: st.review.clone(),
                checks: st.checks,
                updated: st.updated.clone(),
                unread: unread(&link.url),
                agent: state::agent_on(&link.url).map(|(_, session)| if session.state == "working" { Agent::Working } else { Agent::Here }),
                args: args_of(link, &item.title, path),
            }
        })
        .collect();
    answer(PullsAnswer { rows: Some(order_pulls(&rows).into_iter().take(BLOCK_ROWS).collect()), ..PullsAnswer::default() })
}
