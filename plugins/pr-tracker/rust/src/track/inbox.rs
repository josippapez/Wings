//! Tells you when a comment on a listed PR mentions you or replies in a thread you wrote in. It rides on the lists the
//! sidebar already loads: only a PR whose list entry changed has its comments fetched.

use super::projects::Project;
use crate::{
    args::args_of,
    checklist::iso_to_ms,
    forges,
    inbox::{Hit, Inbox, MAX_FETCH, MAX_SEEN, Note, hits_in, notes_for},
    prefs,
    types::{Comment, ListedPr, Me},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::{HashMap, HashSet}, future::Future};
use wings_plugin::{format::iso_of, host, remote::Remote};

const KEY: &str = "inbox";

thread_local! {
    static INBOX: RefCell<Option<Inbox>> = const { RefCell::new(None) };
    /// The inbox as last saved.
    static PERSISTED: RefCell<String> = const { RefCell::new(String::new()) };
    /// PR url → its `updatedAt` in the latest list, which opening the PR's tab marks as read.
    static LATEST: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// What a check does with the PRs of a list. `Track` only moves each PR's mark, for when notifications are off, so turning
/// them on doesn't announce what happened meanwhile. `Hold` leaves a changed PR's mark alone, for while Wings is focused:
/// the next check in the background looks at what changed. `Notify` fetches the comments of each changed PR.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Track,
    Hold,
    Notify,
}

/// A PR of a list, as a check looks at it.
pub struct Item {
    pub url: String,
    pub updated_at: Option<String>,
}

/// What a check does with the PRs of one list. A PR seen for the first time is only marked, in every mode: its first
/// check is where counting starts.
pub async fn scan<M, MF, L, LF>(inbox: &mut Inbox, items: &[Item], mode: Mode, now: &str, me: M, load: L) -> Result<Vec<Hit>, String>
where
    M: FnOnce() -> MF,
    MF: Future<Output = Result<Me, String>>,
    L: Fn(&str) -> LF,
    LF: Future<Output = Result<Vec<Comment>, String>>,
{
    let mut due: Vec<&Item> = Vec::new();
    for item in items {
        match inbox.marks.get(&item.url) {
            None => {
                inbox.marks.insert(item.url.clone(), item.updated_at.clone().unwrap_or_else(|| now.to_owned()));
            }
            Some(_) if mode == Mode::Track => {
                inbox.marks.insert(item.url.clone(), item.updated_at.clone().unwrap_or_else(|| now.to_owned()));
            }
            // A list with no time for a PR can't say it is unchanged, so it is looked at on every check.
            Some(mark) if item.updated_at.as_deref() != Some(mark.as_str()) => due.push(item),
            Some(_) => {}
        }
    }
    if mode != Mode::Notify || due.is_empty() {
        return Ok(Vec::new());
    }
    let me = me().await?;
    let mut seen: HashSet<String> = inbox.seen.iter().cloned().collect();
    let mark_ms = |inbox: &Inbox, item: &Item| iso_to_ms(&inbox.marks[&item.url]).unwrap_or(f64::NAN);
    due.sort_by(|a, b| mark_ms(inbox, a).partial_cmp(&mark_ms(inbox, b)).unwrap_or(std::cmp::Ordering::Equal));
    let mut hits = Vec::new();
    for item in due.into_iter().take(MAX_FETCH) {
        let after = mark_ms(inbox, item);
        let Ok(comments) = load(&item.url).await else { continue };
        for hit in hits_in(&comments, &me, after, &item.url) {
            if seen.insert(hit.key.clone()) {
                inbox.seen.push(hit.key.clone());
                hits.push(hit);
            }
        }
        inbox.marks.insert(item.url.clone(), item.updated_at.clone().unwrap_or_else(|| now.to_owned()));
    }
    let keep_from = inbox.seen.len().saturating_sub(MAX_SEEN);
    inbox.seen.drain(..keep_from);
    Ok(hits)
}

/// What was kept before this run, read once.
fn with_inbox<R>(f: impl FnOnce(&mut Inbox) -> R) -> R {
    INBOX.with(|slot| {
        let mut slot = slot.borrow_mut();
        let inbox = slot.get_or_insert_with(|| {
            let saved = host::storage::get(KEY).unwrap_or(Value::Null);
            let inbox = Inbox {
                marks: saved["marks"].as_object().map(|marks| marks.iter().filter_map(|(url, at)| Some((url.clone(), at.as_str()?.to_owned()))).collect()).unwrap_or_default(),
                seen: saved["seen"].as_array().map(|seen| seen.iter().filter_map(|key| key.as_str().map(str::to_owned)).collect()).unwrap_or_default(),
            };
            PERSISTED.with(|persisted| *persisted.borrow_mut() = json!(inbox).to_string());
            inbox
        });
        f(inbox)
    })
}

fn save() {
    with_inbox(|inbox| {
        let json = json!(inbox);
        if PERSISTED.with(|persisted| *persisted.borrow() != json.to_string()) {
            drop(host::storage::set(KEY, &json));
            PERSISTED.with(|persisted| *persisted.borrow_mut() = json.to_string());
        }
    });
}

/// Looks at a project's listed PRs. While Wings is focused nothing is fetched or sent: a PR that changed is looked at by
/// the next check in the background.
pub async fn check_mentions(project: &Project, remote: &Remote, items: &[ListedPr]) -> Result<(), String> {
    let on = prefs::mentions_on().get();
    for item in items {
        if let Some(updated) = &item.updated_at {
            LATEST.with(|latest| latest.borrow_mut().insert(item.link.url.clone(), updated.clone()));
        }
    }
    let mode = if !on { Mode::Track } else if host::focused() { Mode::Hold } else { Mode::Notify };
    let list: Vec<Item> = items.iter().map(|item| Item { url: item.link.url.clone(), updated_at: item.updated_at.clone() }).collect();
    let mut inbox = with_inbox(|inbox| inbox.clone());
    let remote = remote.clone();
    let links: HashMap<String, ListedPr> = items.iter().map(|item| (item.link.url.clone(), item.clone())).collect();
    let hits = scan(&mut inbox, &list, mode, &iso_of(host::now()), || async move { forges::me(&remote).await.map_err(|error| error.to_string()) }, |url| {
        let link = links[url].link.clone();
        async move { forges::comments(&link).await.map_err(|error| error.to_string()) }
    })
    .await?;
    with_inbox(|kept| *kept = inbox);
    save();
    let by_url: HashMap<&str, &ListedPr> = items.iter().map(|item| (item.link.url.as_str(), item)).collect();
    let notes: Vec<Note> = hits.into_iter().map(|hit| {
        let pr = by_url[hit.url.as_str()];
        Note { args: serde_json::to_value(args_of(&pr.link, &pr.title, &project.path)).unwrap_or_default(), title: pr.title.clone(), hit }
    }).collect();
    for note in notes_for(&project.name, &notes) {
        drop(host::call("notify", json!({ "title": note.title, "body": note.body, "actions": note.actions, "projectId": project.id })));
    }
    Ok(())
}

/// Forgets what is kept of PRs that are no longer open, once every list came back.
pub fn prune_mentions(live: &HashSet<String>) {
    with_inbox(|inbox| inbox.marks.retain(|url, _| live.contains(url)));
    LATEST.with(|latest| latest.borrow_mut().retain(|url, _| live.contains(url)));
    save();
}

/// The PR's tab is open: what it has up to now counts as read.
pub fn mark_read(url: &str) {
    let at = LATEST.with(|latest| latest.borrow().get(url).cloned()).unwrap_or_else(|| iso_of(host::now()));
    with_inbox(|inbox| inbox.marks.insert(url.to_owned(), at));
    save();
}
