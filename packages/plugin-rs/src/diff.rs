//! The diff viewer, from the main script: `wings.openDiff` and `wings.updateDiff`, and `wings-plugin-sdk/async`'s
//! `openDiffWhile` and `openLiveDiff` over them.

use crate::{host, poll, task};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::Rc,
};

/// What a viewer shows: `{ patch }` or `{ error }`, with `comments` and `focus` if it has them.
pub type Loaded = Pin<Box<dyn Future<Output = Result<Value, String>>>>;

/// How often an open live viewer loads again while Wings is in front.
const LIVE_EVERY: u32 = 15_000;

/// `wings.openDiff`: opens the viewer `diff` describes and gives its id.
pub async fn open(diff: Value) -> Result<Value, String> {
    Ok(host::call("openDiff", diff).await?["id"].take())
}

/// `wings.updateDiff`: what the viewer `id` shows from now on.
pub async fn update(id: &Value, shown: &Value) -> Result<(), String> {
    let mut params = shown.clone();
    params["id"] = id.clone();
    host::call("updateDiff", params).await.map(drop)
}

/// What a load gave, as the viewer takes it: its error shows in the viewer.
fn shown(loaded: Result<Value, String>) -> Value {
    loaded.unwrap_or_else(|error| json!({ "error": error }))
}

/// Opens the viewer at once, loading, then fills it with what `load` gives.
pub async fn open_while(diff: Value, load: impl Future<Output = Result<Value, String>>) -> Result<(), String> {
    let id = open(diff).await?;
    update(&id, &shown(load.await)).await
}

struct Viewer {
    load: Box<dyn Fn(bool) -> Loaded>,
    key: String,
    /// What it shows, as JSON, so an update it already shows isn't sent again.
    shown: RefCell<Option<String>>,
    /// One load at a time: a reload asked for meanwhile runs when this one is done, `fresh` if any of them was.
    running: Cell<bool>,
    again: Cell<Option<bool>>,
}

thread_local! {
    static LIVE: RefCell<HashMap<String, Rc<Viewer>>> = RefCell::new(HashMap::new());
    static HOOKED: Cell<bool> = const { Cell::new(false) };
}

async fn reload(id: Value, viewer: Rc<Viewer>, fresh: bool) {
    if viewer.running.replace(true) {
        viewer.again.set(Some(fresh || viewer.again.get() == Some(true)));
        return;
    }
    let mut fresh = fresh;
    loop {
        let now = shown((viewer.load)(fresh).await);
        let text = now.to_string();
        let open = LIVE.with(|live| live.borrow().contains_key(&id.to_string()));
        if open && viewer.shown.borrow().as_deref() != Some(&text) {
            *viewer.shown.borrow_mut() = Some(text);
            drop(update(&id, &now).await);
        }
        match viewer.again.take() {
            Some(next) => fresh = next,
            None => break,
        }
    }
    viewer.running.set(false);
}

fn id_of(key: &str) -> Value {
    serde_json::from_str(key).unwrap_or(Value::Null)
}

/// Like `open_while`, for a diff that can change while it's open, like the working tree. The viewer gets a Refresh
/// button, and `load` runs again every 15 s while Wings is in front. An update the viewer already shows isn't sent
/// again, and nothing runs once it's closed. `load` gets whether to skip a cache, which the Refresh button asks for.
/// `key` names what it shows, like a repo folder, for `refresh_live`.
pub async fn open_live_id<F, Fut>(mut diff: Value, key: &str, load: F) -> Result<Value, String>
where
    F: Fn(bool) -> Fut + 'static,
    Fut: Future<Output = Result<Value, String>> + 'static,
{
    if !HOOKED.replace(true) {
        host::on("diffRefresh", |token, asked| {
            let id = asked["diffId"].clone();
            task::spawn(async move {
                if let Some(viewer) = LIVE.with(|live| live.borrow().get(&id.to_string()).cloned()) {
                    reload(id, viewer, true).await;
                }
                drop(host::call("actionDone", json!({ "token": token, "error": null })));
            });
        });
        host::on("diffClosed", |_, closed| {
            LIVE.with(|live| live.borrow_mut().remove(&closed["diffId"].to_string()));
        });
        poll::while_focused(LIVE_EVERY, false, || refresh_live(None));
    }
    diff["refresh"] = true.into();
    let id = open(diff).await?;
    let viewer = Rc::new(Viewer { load: Box::new(move |fresh| Box::pin(load(fresh))), key: key.to_owned(), shown: RefCell::new(None), running: Cell::new(false), again: Cell::new(None) });
    LIVE.with(|live| live.borrow_mut().insert(id.to_string(), viewer.clone()));
    reload(id.clone(), viewer, false).await;
    Ok(id)
}

/// `open_live_id`, without the id.
pub async fn open_live<F, Fut>(diff: Value, key: &str, load: F) -> Result<(), String>
where
    F: Fn(bool) -> Fut + 'static,
    Fut: Future<Output = Result<Value, String>> + 'static,
{
    open_live_id(diff, key, load).await.map(drop)
}

/// Loads the open live viewers again now, or only those whose key is in `keys`, like after a git change.
pub fn refresh_live(keys: Option<&[String]>) {
    let viewers: Vec<(String, Rc<Viewer>)> = LIVE.with(|live| live.borrow().iter().filter(|(_, viewer)| keys.is_none_or(|keys| keys.contains(&viewer.key))).map(|(id, viewer)| (id.clone(), viewer.clone())).collect());
    for (id, viewer) in viewers {
        task::spawn(reload(id_of(&id), viewer, false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::block_on;

    #[test]
    fn a_live_viewer_opens_with_refresh_shows_what_load_gives_and_isnt_sent_the_same_again() {
        let calls: Rc<RefCell<Vec<(String, Value)>>> = Rc::default();
        let seen = calls.clone();
        host::testing::answer(move |method, params| {
            seen.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(json!({ "id": 7 }))
        });
        let loads = Rc::new(Cell::new(0));
        let count = loads.clone();
        block_on(open_live(json!({ "title": "Changes" }), "/repo", move |_| {
            count.set(count.get() + 1);
            let n = count.get();
            async move { if n < 3 { Ok(json!({ "patch": "one" })) } else { Err("git failed".to_owned()) } }
        }))
        .unwrap();
        refresh_live(Some(&["/other".to_owned()]));
        refresh_live(Some(&["/repo".to_owned()]));
        refresh_live(None);
        block_on(async {});
        let sent: Vec<(String, Value)> = calls.borrow().iter().filter(|call| call.0 != "focused").cloned().collect();
        assert_eq!(
            sent,
            [
                ("openDiff".to_owned(), json!({ "title": "Changes", "refresh": true })),
                ("updateDiff".to_owned(), json!({ "patch": "one", "id": 7 })),
                // The second load gave the same, so nothing was sent. The third failed, which the viewer shows.
                ("updateDiff".to_owned(), json!({ "error": "git failed", "id": 7 })),
            ]
        );
        assert_eq!(loads.get(), 3);
    }
}
