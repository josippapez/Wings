//! Tells the agent on a PR's branch when its PR gets news (see `news.rs`). It polls only the PRs that have an agent pane,
//! every 2 minutes, in the background too: the forge can't call a local app, so the news has to be asked for.

use super::{state::{self, Loaded, Pane}, status::{load_failures, refresh_status}};
use crate::{
    finished::final_state,
    forges,
    news::{FORGET_MS, PrInfo, PrNews, Snapshot, comments_due, diff_news, failures_of, has_news, message_for, settled, under_cap},
    prefs,
    remote::remote_for,
    types::{Link, Me},
};
use serde_json::{Value, json};
use std::{cell::{Cell, RefCell}, collections::BTreeMap};
use wings_plugin::{host, task::{join, map_limit, spawn}};

const KEY: &str = "agent-news";
pub const NEWS_MS: u32 = 120_000;
/// Watched PRs read at once: a forge CLI call takes a second or two.
const PARALLEL: usize = 3;

thread_local! {
    static MEMORY: RefCell<Option<BTreeMap<String, PrNews>>> = const { RefCell::new(None) };
    static PERSISTED: RefCell<String> = const { RefCell::new(String::new()) };
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

fn with_memory<R>(f: impl FnOnce(&mut BTreeMap<String, PrNews>) -> R) -> R {
    MEMORY.with(|slot| {
        let mut slot = slot.borrow_mut();
        let memory = slot.get_or_insert_with(|| {
            let saved = host::storage::get(KEY).unwrap_or(Value::Null);
            let memory: BTreeMap<String, PrNews> = saved.as_object().map(|map| map.iter().filter_map(|(url, news)| Some((url.clone(), serde_json::from_value(news.clone()).ok()?))).collect()).unwrap_or_default();
            PERSISTED.with(|persisted| *persisted.borrow_mut() = json!(memory).to_string());
            memory
        });
        f(memory)
    })
}

fn save() {
    let json = with_memory(|memory| json!(memory));
    if PERSISTED.with(|persisted| *persisted.borrow() != json.to_string()) {
        drop(host::storage::set(KEY, &json));
        PERSISTED.with(|persisted| *persisted.borrow_mut() = json.to_string());
    }
}

/// The PRs with an agent in a pane on their branch, each with those panes.
fn watched() -> Vec<(String, Link, Vec<Pane>)> {
    state::with(|s| {
        let mut by_url: Vec<(String, Link, Vec<Pane>)> = Vec::new();
        for pane in &s.panes {
            let Some(link) = s.links.get(&pane.id).filter(|_| pane.session.is_some()) else { continue };
            match by_url.iter_mut().find(|(url, _, _)| *url == link.url) {
                Some((_, _, panes)) => panes.push(pane.clone()),
                None => by_url.push((link.url.clone(), link.clone(), vec![pane.clone()])),
            }
        }
        by_url
    })
}

/// Who you are on the PR's forge, which tells your comments from the others.
async fn me_of(link: &Link, cwd: &str) -> Result<Option<Me>, String> {
    let remote = match &link.remote {
        Some(remote) => remote.clone(),
        None => remote_for(cwd).await,
    };
    if !forges::is_known(&remote) {
        return Ok(None);
    }
    forges::me(&remote).await.map(Some).map_err(|error| error.to_string())
}

async fn look(url: String, link: Link, panes: Vec<Pane>) {
    // Focused, the pane poll keeps the status fresh; in the background nothing does, so it is read here.
    if !host::focused() {
        refresh_status(&url, false).await;
    }
    let Some(Loaded { entry: Ok(st), .. }) = state::status_of(&url) else { return };
    if final_state(&st).is_some() {
        return;
    }
    let now = host::now();
    let known = with_memory(|memory| memory.get(&url).filter(|prev| now - prev.watched <= FORGET_MS).cloned());
    let stamp = format!("{}|{}", st.key, st.updated);
    let (mut comments, mut me) = (None, None);
    if comments_due(known.as_ref(), &link, &stamp) {
        let (found_me, found_comments) = join(me_of(&link, &panes[0].cwd), async { forges::comments(&link).await.map_err(|error| error.to_string()) }).await;
        if let (Ok(found_me), Ok(found_comments)) = (found_me, found_comments) {
            (me, comments) = (found_me, Some(found_comments));
        }
    }
    // Without its comments a first look would count every old one as new on the next.
    if known.is_none() && !(comments.is_some() && me.is_some()) {
        return;
    }
    let snapshot = Snapshot { failing: st.failing.clone(), conflict: st.label == "Conflicts", comments, stamp };
    let (next, found, _) = diff_news(known.as_ref(), &snapshot, me.as_ref(), now);
    if !has_news(&found) {
        return with_memory(|memory| drop(memory.insert(url, next)));
    }
    if !under_cap(&next) {
        return with_memory(|memory| drop(memory.insert(url, settled(&next, false, now))));
    }
    let details = if found.checks.is_empty() { Vec::new() } else { load_failures(&url).await };
    let base = st.base.clone().or_else(|| st.branch.split(" → ").nth(1).map(str::to_owned));
    let text = message_for(&PrInfo { link: &link, title: &st.title, base: base.as_deref() }, &found, &failures_of(&found.checks, &details));
    for pane in &panes {
        if host::call("sendToAgent", json!({ "paneId": pane.id, "text": text })).await.is_ok() {
            return with_memory(|memory| drop(memory.insert(url, settled(&next, true, now))));
        }
        // No agent there any more, on a permission prompt, or text typed and unsent: the next look tries again, and the news is still new then.
    }
}

async fn tick() {
    if RUNNING.get() || !prefs::tell_agents().get() {
        return;
    }
    RUNNING.set(true);
    let now = host::now();
    let open = watched();
    map_limit(open.clone(), PARALLEL, |(url, link, panes)| look(url, link, panes)).await;
    with_memory(|memory| memory.retain(|url, news| open.iter().any(|(open, _, _)| open == url) || now - news.watched <= FORGET_MS));
    save();
    RUNNING.set(false);
}

thread_local! {
    /// Which run of the timer is the live one: a run that finds another number here has been stopped.
    static RUN: Cell<u64> = const { Cell::new(0) };
}

fn arm(on: bool) {
    let run = RUN.get() + 1;
    RUN.set(run);
    if !on {
        return;
    }
    spawn(async move {
        tick().await;
        loop {
            host::sleep(NEWS_MS).await;
            if RUN.get() != run {
                break;
            }
            tick().await;
        }
    });
}

/// Starts and stops the checks with the setting.
pub fn start_news() {
    let tell = prefs::tell_agents();
    arm(tell.get());
    tell.on_change(|on| arm(*on));
}
