//! Loading a PR's status, the details of its failed checks, and its diff, from whichever forge it's on.

use super::{badges, state::{self, Failures, Loaded}};
use crate::{forges, forges::shared::no_log, types::{Diff, FailureDetail}};
use std::rc::Rc;
use wings_plugin::{cache::Cached, host, task::spawn};

thread_local! {
    /// PR url → its running status refresh: asking again while one runs joins it.
    static REFRESHES: Cached<String, ()> = Cached::new(0.0, 0.0, |url: String| async move {
        load_status(&url).await;
        Ok(())
    });
    static DIFF_LOADS: Cached<String, Rc<Diff>> = Cached::new(0.0, 0.0, |url: String| async move { read_diff(&url).await });
    static FAILURE_LOADS: Cached<(String, String), Vec<FailureDetail>> = Cached::new(0.0, 0.0, |(url, key): (String, String)| async move { read_failures(&url, &key).await });
}

pub async fn refresh_status(url: &str, by_user: bool) {
    if state::link_for(url).is_none() {
        return;
    }
    if by_user {
        state::with(|s| s.manual.push(url.to_owned()));
    }
    if !badges::in_flight(url) {
        badges::set_in_flight(url, true);
        badges::render();
    }
    REFRESHES.with(Cached::clone).get(url.to_owned()).await.ok();
}

async fn load_status(url: &str) {
    let Some(link) = state::link_for(url) else {
        badges::set_in_flight(url, false);
        return;
    };
    let fresh = state::with(|s| s.manual.iter().any(|manual| manual == url));
    let entry = forges::status(&link, fresh).await.map_err(|error| error.to_string());
    state::with(|s| {
        s.status.insert(url.to_owned(), Loaded { entry, at: host::now() });
        s.manual.retain(|manual| manual != url);
    });
    badges::set_in_flight(url, false);
    badges::render();
    prefetch(url);
}

/// After a status loads: the diff, so "View changes" opens at once, and the details of checks that newly failed.
pub fn prefetch(url: &str) {
    let Some(Loaded { entry: Ok(st), .. }) = state::status_of(url) else { return };
    let has_diff = state::with(|s| s.diffs.get(url).is_some_and(|(key, _)| key.as_deref() == Some(st.key.as_str())));
    if !has_diff {
        let url = url.to_owned();
        spawn(async move {
            drop(load_diff(&url).await);
        });
    }
    let has_failures = state::with(|s| s.failures.get(url).is_some_and(|f| f.key == state::fail_key(&st)));
    if !st.failing.is_empty() && !has_failures {
        let url = url.to_owned();
        spawn(async move {
            load_failures(&url).await;
        });
    }
}

async fn read_failures(url: &str, key: &str) -> Result<Vec<FailureDetail>, String> {
    let (Some(link), Some(Loaded { entry: Ok(st), .. })) = (state::link_for(url), state::status_of(url)) else {
        // The pane moved off this PR while its status loaded.
        return Ok(Vec::new());
    };
    state::with(|s| s.failures.insert(url.to_owned(), Failures { key: key.to_owned(), list: None }));
    let loads = st.failing.iter().map(|f| {
        let (link, f) = (link.clone(), f.clone());
        Box::pin(async move {
            forges::failure(&link, &f).await.unwrap_or_else(|error| no_log(f.name.as_deref(), &format!("{} (no log: {error})", f.summary.as_deref().unwrap_or("Failed")), f.url.as_deref()))
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = Vec<FailureDetail>>>>
    });
    let list: Vec<FailureDetail> = wings_plugin::task::join_all(loads.collect()).await.into_iter().flatten().collect();
    state::with(|s| {
        if let Some(entry) = s.failures.get_mut(url).filter(|entry| entry.key == key) {
            entry.list = Some(list.clone());
        }
    });
    badges::render();
    Ok(list)
}

/// Details of the PR's failed checks, fetched once per set of failures rather than on every poll.
pub async fn load_failures(url: &str) -> Vec<FailureDetail> {
    let Some(Loaded { entry: Ok(st), .. }) = state::status_of(url) else { return Vec::new() };
    let key = state::fail_key(&st);
    if let Some(list) = state::with(|s| s.failures.get(url).filter(|f| f.key == key).and_then(|f| f.list.clone())) {
        return list;
    }
    FAILURE_LOADS.with(Cached::clone).get((url.to_owned(), key)).await.unwrap_or_default()
}

async fn read_diff(url: &str) -> Result<Rc<Diff>, String> {
    let Some(link) = state::link_for(url) else {
        return Err("This pull request isn't open in a pane any more.".into());
    };
    let st = match state::status_of(url).map(|loaded| loaded.entry) {
        Some(Ok(st)) => st,
        Some(Err(error)) => return Err(error),
        None => return Err("The pull request's status hasn't loaded.".into()),
    };
    let (patch, comments) = forges::diff(&link, &st).await.map_err(|error| error.to_string())?;
    let diff = Rc::new(Diff { key: Some(st.key.clone()), patch, comments });
    state::with(|s| s.diffs.insert(url.to_owned(), (Some(st.key), diff.clone())));
    Ok(diff)
}

/// The PR's diff and review comments, cached per PR version; concurrent callers share one fetch.
pub async fn load_diff(url: &str) -> Result<Rc<Diff>, String> {
    let key = match state::status_of(url).map(|loaded| loaded.entry) {
        Some(Ok(st)) => Some(st.key),
        _ => None,
    };
    if let Some((cached, diff)) = state::with(|s| s.diffs.get(url).cloned())
        && cached.is_some()
        && cached == key
    {
        return Ok(diff);
    }
    DIFF_LOADS.with(Cached::clone).get(url.to_owned()).await
}

pub fn forget_all() {
    state::with(|s| {
        s.status.clear();
        s.diffs.clear();
        s.failures.clear();
    });
    REFRESHES.with(Cached::clear);
    DIFF_LOADS.with(Cached::clear);
    FAILURE_LOADS.with(Cached::clear);
}
