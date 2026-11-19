//! What the page reaches of Wings: calls the host answers later, broadcasts between the plugin's frames, focus, the
//! clock and timers.

use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[cfg(target_arch = "wasm32")]
pub(crate) mod ffi {
    #[link(wasm_import_module = "wings")]
    unsafe extern "C" {
        pub fn render(at: *const u8, len: usize);
        pub fn call(id: u32, method: *const u8, method_len: usize, params: *const u8, params_len: usize);
        pub fn storage(op: *const u8, op_len: usize, key: *const u8, key_len: usize, json: *const u8, json_len: usize) -> u64;
        pub fn broadcast(at: *const u8, len: usize);
        pub fn timer(id: u32, ms: u32);
        pub fn now() -> f64;
        pub fn utc_offset(ms: f64) -> i32;
        pub fn focused() -> i32;
        pub fn view_args() -> u64;
    }
}

/// Outside Wings there is no host: a plugin's own tests, built for this computer, run what doesn't reach for one.
/// Nothing is drawn or sent, a call is never answered, the clock stands at 0 (`testing::set_now` moves it) and the timezone is UTC.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::missing_safety_doc)]
pub(crate) mod ffi {
    pub unsafe fn render(_: *const u8, _: usize) {}
    pub unsafe fn call(_: u32, _: *const u8, _: usize, _: *const u8, _: usize) {}
    pub unsafe fn broadcast(_: *const u8, _: usize) {}
    pub unsafe fn timer(_: u32, _: u32) {}
    pub unsafe fn now() -> f64 {
        super::testing::NOW.with(std::cell::Cell::get)
    }
    pub unsafe fn utc_offset(_: f64) -> i32 {
        0
    }
    pub unsafe fn focused() -> i32 {
        1
    }
    pub unsafe fn view_args() -> u64 {
        0
    }
}

/// What a wait is for: the answer, once it's here, and who to wake.
struct Wait<T> {
    done: Option<T>,
    waker: Option<Waker>,
}

type Shared<T> = Rc<RefCell<Wait<T>>>;

fn finish<T>(wait: &Shared<T>, value: T) {
    let mut wait = wait.borrow_mut();
    wait.done = Some(value);
    if let Some(waker) = wait.waker.take() {
        waker.wake();
    }
}

/// A wait that ends with a `T`.
pub struct Pending<T>(Shared<T>);

impl<T> Future for Pending<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut wait = self.0.borrow_mut();
        match wait.done.take() {
            Some(value) => Poll::Ready(value),
            None => {
                wait.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

thread_local! {
    static CALLS: RefCell<HashMap<u32, Shared<Result<Value, String>>>> = RefCell::new(HashMap::new());
    static TIMERS: RefCell<HashMap<u32, Shared<()>>> = RefCell::new(HashMap::new());
    static NEXT: RefCell<u32> = const { RefCell::new(1) };
    static FOCUSED: RefCell<Option<bool>> = const { RefCell::new(None) };
    static ON_BROADCAST: RefCell<Vec<Rc<dyn Fn(&Value)>>> = const { RefCell::new(Vec::new()) };
    static ON_FOCUS: RefCell<Vec<Rc<dyn Fn(bool)>>> = const { RefCell::new(Vec::new()) };
    static ON_MESSAGE: RefCell<HashMap<&'static str, Vec<Rc<dyn Fn(u32, &Value)>>>> = RefCell::new(HashMap::new());
    /// The panes as last heard, once `watch_panes` listens.
    static PANES: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
    static WATCHING_PANES: RefCell<bool> = const { RefCell::new(false) };
    static ON_PANES: RefCell<Vec<Rc<dyn Fn(&[Value])>>> = const { RefCell::new(Vec::new()) };
    /// The badge last sent for each pane, as JSON, so one a pane already shows isn't sent again.
    static SENT_BADGES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// The same for each pane's status strip.
    static SENT_STATUSES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static ON_TRANSCRIPT: RefCell<Vec<Rc<dyn Fn(&Value)>>> = const { RefCell::new(Vec::new()) };
    static ON_ACTION: RefCell<Vec<Action>> = const { RefCell::new(Vec::new()) };
    /// Who hears the lines a call sends before its answer, by the call's id.
    static OUTPUTS: RefCell<HashMap<u32, Rc<dyn Fn(&Value)>>> = RefCell::new(HashMap::new());
}

/// A stand-in for Wings in a plugin's own tests, which run on this computer where nothing answers a call.
#[cfg(not(target_arch = "wasm32"))]
pub mod testing {
    use serde_json::Value;
    use std::{cell::RefCell, rc::Rc};

    type Answer = Rc<dyn Fn(&str, &Value) -> Result<Value, String>>;

    thread_local! {
        pub(super) static ANSWER: RefCell<Option<Answer>> = const { RefCell::new(None) };
        pub(super) static NOW: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    }

    /// From now on, on this thread, the clock stands at `ms`.
    pub fn set_now(ms: f64) {
        NOW.with(|now| now.set(ms));
    }

    /// From now on, on this thread, every `call` is answered at once with what `answer` gives for its method and
    /// params.
    pub fn answer(answer: impl Fn(&str, &Value) -> Result<Value, String> + 'static) {
        ANSWER.with(|slot| *slot.borrow_mut() = Some(Rc::new(answer)));
    }
}

type Action = Rc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<(), String>>>>>;

fn next_id() -> u32 {
    NEXT.with(|next| {
        let id = *next.borrow();
        *next.borrow_mut() = id.wrapping_add(1).max(1);
        id
    })
}

/// A call of `wings.<method>` with the params the JS frame's wrapper would send. It goes out now, so calls made one
/// after another and then awaited run side by side.
pub fn call(method: &str, params: Value) -> Pending<Result<Value, String>> {
    call_as(next_id(), method, params)
}

fn call_as(id: u32, method: &str, params: Value) -> Pending<Result<Value, String>> {
    let wait = Rc::new(RefCell::new(Wait { done: None, waker: None }));
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(answer) = testing::ANSWER.with(|slot| slot.borrow().clone()) {
        wait.borrow_mut().done = Some(answer(method, &params));
        return Pending(wait);
    }
    CALLS.with(|calls| calls.borrow_mut().insert(id, wait.clone()));
    let params = params.to_string();
    // SAFETY: both are live strings, which the host copies before this returns.
    unsafe { ffi::call(id, method.as_ptr(), method.len(), params.as_ptr(), params.len()) };
    Pending(wait)
}

/// Resolves after `ms`.
pub fn sleep(ms: u32) -> Pending<()> {
    let id = next_id();
    let wait = Rc::new(RefCell::new(Wait { done: None, waker: None }));
    TIMERS.with(|timers| timers.borrow_mut().insert(id, wait.clone()));
    // SAFETY: takes two numbers.
    unsafe { ffi::timer(id, ms) };
    Pending(wait)
}

pub(crate) fn fired(id: u32) {
    if let Some(wait) = TIMERS.with(|timers| timers.borrow_mut().remove(&id)) {
        finish(&wait, ());
    }
}

/// What the host tells the frame: an answer to a call, a broadcast, the window's focus, or one of the rest of what a
/// JS frame hears (panes, a tool call, an action, projects, git), which goes to who asked for it with `on`.
pub(crate) fn deliver(kind: &str, id: u32, text: String) {
    match kind {
        "resolve" | "reject" => {
            OUTPUTS.with(|outputs| outputs.borrow_mut().remove(&id));
            let Some(wait) = CALLS.with(|calls| calls.borrow_mut().remove(&id)) else {
                return;
            };
            finish(
                &wait,
                if kind == "resolve" { Ok(serde_json::from_str(&text).unwrap_or(Value::Null)) } else { Err(text) },
            );
        }
        "output" => {
            if let Some(listener) = OUTPUTS.with(|outputs| outputs.borrow().get(&id).cloned()) {
                // A line a program printed isn't JSON: it comes as the text it is.
                listener(&serde_json::from_str(&text).unwrap_or(Value::String(text)));
            }
        }
        "broadcast" => {
            let message = serde_json::from_str(&text).unwrap_or(Value::Null);
            for listener in ON_BROADCAST.with(|list| list.borrow().clone()) {
                listener(&message);
            }
        }
        "focus" => {
            let value = serde_json::from_str::<Value>(&text).is_ok_and(|v| v["focused"] == true);
            if FOCUSED.with(|focused| focused.borrow_mut().replace(value)) == Some(value) {
                return;
            }
            for listener in ON_FOCUS.with(|list| list.borrow().clone()) {
                listener(value);
            }
        }
        _ => {
            let Some(listeners) = ON_MESSAGE.with(|map| map.borrow().get(kind).cloned()) else {
                return;
            };
            let data = serde_json::from_str(&text).unwrap_or(Value::Null);
            for listener in listeners {
                listener(id, &data);
            }
        }
    }
}

/// Hears each message of `kind` the host sends this frame, as the host names it (`panes`, `tool`, `action`,
/// `projects`, `git`, ...), with the id it came with and its JSON. A `tool` is answered by calling `toolDone` with
/// that id as `token`, an `action` by `actionDone`.
pub fn on(kind: &'static str, listener: impl Fn(u32, &Value) + 'static) {
    ON_MESSAGE.with(|map| map.borrow_mut().entry(kind).or_default().push(Rc::new(listener)));
}

/// Keeps the pane list as the host sends it, and forgets the badge of a pane that closed, as the host does.
fn watch_panes() {
    if WATCHING_PANES.with(|watching| watching.replace(true)) {
        return;
    }
    on("panes", |_, list| {
        let list = list.as_array().cloned().unwrap_or_default();
        for sent in [&SENT_BADGES, &SENT_STATUSES] {
            sent.with(|sent| sent.borrow_mut().retain(|pane, _| list.iter().any(|open| open["paneId"] == pane.as_str())));
        }
        PANES.with(|panes| *panes.borrow_mut() = Some(list.clone()));
        for listener in ON_PANES.with(|listeners| listeners.borrow().clone()) {
            listener(&list);
        }
    });
}

/// `wings.onPanes`: every terminal pane, on every change, each with its `paneId`, `cwd`, `command` and project. Ask
/// for it when the frame starts: the first list comes right after.
pub fn on_panes(listener: impl Fn(&[Value]) + 'static) {
    watch_panes();
    let listener: Rc<dyn Fn(&[Value])> = Rc::new(listener);
    ON_PANES.with(|listeners| listeners.borrow_mut().push(listener.clone()));
    // Once more for a listener that comes after the first list.
    if let Some(list) = PANES.with(|panes| panes.borrow().clone()) {
        crate::task::spawn(async move { listener(&list) });
    }
}

/// The terminals the agent of this tab's page opened with Wings' `open_pane`, each `{ paneId, title, running }`, now and
/// on every change, for a page whose plugin declares `terminals` in `contributes.ui`. A `Terminal` node with a `pane`
/// from it draws that terminal live. Needs Wings 0.2.40.
pub fn on_terminals(listener: impl Fn(&[Value]) + 'static) {
    on("terminals", move |_, list| listener(list.as_array().map(Vec::as_slice).unwrap_or_default()));
}

/// `wings.setLinkWords`: the link `id` of the plugin's `contributes.links` finds only `words` from now on, like the issue
/// keys there are, or with `None` whatever its pattern matches. Needs Wings 0.2.40.
pub async fn set_link_words(id: &str, words: Option<&[String]>) -> Result<(), String> {
    call("setLinkWords", json!({ "id": id, "words": words })).await.map(drop)
}

type Sent = std::thread::LocalKey<RefCell<HashMap<String, String>>>;

/// Sends `value` for the pane only when it differs from the last one sent, which `sent` keeps. One the host refuses
/// is forgotten, so the next call sends it again.
async fn send_changed(sent: &'static Sent, method: &str, pane: &str, name: &str, value: &Value) -> Result<(), String> {
    watch_panes();
    let now = value.to_string();
    if sent.with(|sent| sent.borrow_mut().insert(pane.to_owned(), now.clone())).as_ref() == Some(&now) {
        return Ok(());
    }
    let answer = call(method, json!({ "paneId": pane, name: value })).await;
    if answer.is_err() {
        sent.with(|sent| {
            let mut sent = sent.borrow_mut();
            if sent.get(pane) == Some(&now) {
                sent.remove(pane);
            }
        });
    }
    answer.map(drop)
}

/// `wings.setBadge`: the badge the pane `pane` shows for this plugin, or none for null. One the pane already shows
/// isn't sent again, so a plugin can set every pane's badge on every look.
pub async fn set_badge(pane: &str, badge: &Value) -> Result<(), String> {
    send_changed(&SENT_BADGES, "setBadge", pane, "badge", badge).await
}

/// `wings.setPaneStatus`: the strip under the pane's terminal, with up to 4 figures and the lines of the card shown
/// when you point at it, or none for null. One the pane already shows isn't sent again.
pub async fn set_pane_status(pane: &str, status: &Value) -> Result<(), String> {
    send_changed(&SENT_STATUSES, "setPaneStatus", pane, "status", status).await
}

/// `wings.notify`: a notification with this title and body.
pub fn notify(title: &str, body: &str) -> Pending<Result<Value, String>> {
    call("notify", json!({ "title": title, "body": body }))
}

/// `wings.statusline`: what Claude Code last told `wings statusline`: each account's limits, and each session's
/// context.
pub fn statusline() -> Pending<Result<Value, String>> {
    call("statusline", json!({}))
}

/// `wings.exec`: runs a program the manifest allows and answers `{ code, stdout, stderr }`.
pub fn exec(program: &str, args: &[&str]) -> Pending<Result<Value, String>> {
    call("exec", json!({ "program": program, "args": args, "cwd": null, "timeoutMs": null, "stream": false }))
}

/// `wings.exec` in the folder `cwd`.
pub fn exec_in(cwd: &str, program: &str, args: &[&str]) -> Pending<Result<Value, String>> {
    call("exec", json!({ "program": program, "args": args, "cwd": cwd, "timeoutMs": null, "stream": false }))
}

/// `wings.exec` that Wings stops after `timeout_ms`, for a program that waits on you, like a sign-in in a browser.
pub fn exec_timeout(program: &str, args: &[&str], timeout_ms: u32) -> Pending<Result<Value, String>> {
    call("exec", json!({ "program": program, "args": args, "cwd": null, "timeoutMs": timeout_ms, "stream": false }))
}

/// `wings.exec` in the folder `cwd`, under a Claude Code account, that Wings stops after `timeout_ms` when it is given.
/// Without a session of its own the account is the project's that holds `cwd`. Needs Wings 0.2.39.
pub fn exec_as(account: &Account, cwd: &str, program: &str, args: &[&str], timeout_ms: Option<u32>) -> Pending<Result<Value, String>> {
    call("exec", json!({ "program": program, "args": args, "cwd": cwd, "timeoutMs": timeout_ms, "stream": false, "account": account.0 }))
}

/// `wings.projects`: every project, each with its `id`, `path` and `name`.
pub fn projects() -> Pending<Result<Value, String>> {
    call("projects", json!({}))
}

/// `wings.onProjects`: the projects, on every change. Ask for it when the frame starts: the first list comes right
/// after.
pub fn on_projects(listener: impl Fn(&[Value]) + 'static) {
    on("projects", move |_, list| listener(list.as_array().map_or(&[], Vec::as_slice)));
}

/// `wings.openPane`: a new terminal tab in the folder `cwd`, which has to be in one of your projects.
pub fn open_pane(cwd: &str) -> Pending<Result<Value, String>> {
    call("openPane", json!({ "command": null, "args": [], "cwd": cwd, "placement": "tab" }))
}

/// `wings.pastSessions`: the Claude Code sessions of the project on screen, most recent first, that match `query`, at
/// most `limit`: each `{ id, title, firstPrompt, cwd, gitBranch, lastActiveMs, messages }`. Needs `user` in
/// `permissions.transcript`.
pub fn past_sessions(query: &str, limit: u32) -> Pending<Result<Value, String>> {
    call("pastSessions", json!({ "query": query, "limit": limit }))
}

/// `wings.pastSessions` for the project `project_id` (an `id` of `projects`) instead of the one on screen. Needs
/// Wings 0.2.36.
pub fn past_sessions_in(project_id: &str, query: &str, limit: u32) -> Pending<Result<Value, String>> {
    call("pastSessions", json!({ "projectId": project_id, "query": query, "limit": limit }))
}

/// `wings.setAgent`: this tab's page runs an agent, which the sidebar's Agents lists with the terminals' agents: its
/// state (`working`, `blocked`, `done` or `idle`), its name and what it does now. `None` takes it off the list. Needs
/// `"agents"` in `contributes.ui` and Wings 0.2.34.
pub fn set_agent(agent: Option<(&str, &str, Option<&str>)>) -> Pending<Result<Value, String>> {
    let agent = agent.map(|(state, name, summary)| json!({ "state": state, "name": name, "summary": summary }));
    call("setAgent", json!({ "agent": agent }))
}

/// `wings.setTabTitle`: names the tab this page shows in, as a terminal's tab shows what runs in it: one line of at most
/// 80 characters, or `None` for its view's title again. Wings 0.2.37.
pub fn set_tab_title(title: Option<&str>) -> Pending<Result<Value, String>> {
    call("setTabTitle", json!({ "title": title }))
}

/// `wings.closeTab`: closes the tab this page shows in, and no other tab of its view. Wings 0.2.36.
pub fn close_tab() -> Pending<Result<Value, String>> {
    call("closeTab", json!({}))
}

/// `wings.closePane`: ends the program in a pane of the project on screen and closes the pane. Needs `panes`.
pub fn close_pane(pane: &str) -> Pending<Result<Value, String>> {
    call("closePane", json!({ "paneId": pane }))
}

/// `wings.disks`: the disks Finder shows, each with its `name`, `mount`, `total` and `free`.
pub fn disks() -> Pending<Result<Value, String>> {
    call("disks", json!({}))
}

/// A search of a disk that is running. Letting go of it doesn't stop the search.
pub struct Search {
    token: String,
    answer: Pending<Result<Value, String>>,
}

impl Search {
    /// What stops this search, to keep where a Stop button reaches it.
    pub fn stopper(&self) -> Stopper {
        Stopper(self.token.clone())
    }
}

impl Future for Search {
    type Output = Result<Value, String>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.answer).poll(cx)
    }
}

#[derive(Clone)]
pub struct Stopper(String);

impl Stopper {
    /// Stops the search, which then fails with "The scan was stopped".
    pub fn stop(&self) {
        drop(call("diskUsageCancel", json!({ "token": self.0 })));
    }
}

/// `wings.diskFind`: the files and folders under `path` called one of `names`, as `{ paths, entries }`, without going
/// into a folder called one of `skip`. `on_progress` hears how far it is, as `{ entries, path }`, while it runs.
pub fn disk_find(path: &str, names: &[&str], skip: &[&str], on_progress: impl Fn(&Value) + 'static) -> Search {
    let id = next_id();
    let token = format!("find-{id}");
    OUTPUTS.with(|outputs| outputs.borrow_mut().insert(id, Rc::new(on_progress)));
    let answer = call_as(id, "diskFind", json!({ "path": path, "names": names, "skip": skip, "token": token, "stream": true }));
    Search { token, answer }
}

/// `wings.transcript`: the entries of a Claude session's transcript of the `kinds` the manifest allows, oldest
/// first; with `last`, only that many of the newest.
pub fn transcript(session: &str, kinds: &[&str], last: Option<u32>) -> Pending<Result<Value, String>> {
    call("transcript", json!({ "sessionId": session, "types": kinds, "last": last }))
}

/// `transcript_with_images` with each string of an entry cut to `cut` characters, a long tool output or a whole file
/// that would be, which lets `last` ask for up to 10,000 entries: a long session's whole history reads in a few MB, about
/// what 400 whole entries take. Needs Wings 0.2.43; an older one reads the newest 1,000 whole.
pub fn transcript_cut(session: &str, kinds: &[&str], last: u32, cut: u32) -> Pending<Result<Value, String>> {
    call("transcript", json!({ "sessionId": session, "types": kinds, "last": last, "images": "register", "cut": cut }))
}

/// `wings.transcript` that keeps the pictures out of the page (Wings 0.2.40). Each image block of an entry, in a message
/// or in a tool result, comes back as `{ "type": "image", "source": { "type": "registered", "id": ... } }`, the id to
/// give `ui::image`, and as `{ "type": "unavailable" }` when Wings won't keep it. A page has 64 MB, and a transcript with
/// screenshots in it is more than that as base64.
pub fn transcript_with_images(session: &str, kinds: &[&str], last: Option<u32>) -> Pending<Result<Value, String>> {
    call("transcript", json!({ "sessionId": session, "types": kinds, "last": last, "images": "register" }))
}

/// `transcript_with_images` of the subagent `agent` (Claude Code's agent id) of the session `session`: its own transcript,
/// which Wings finds in the session's folder. Needs Wings 0.2.41.
pub fn subagent_transcript(session: &str, agent: &str, kinds: &[&str], last: Option<u32>) -> Pending<Result<Value, String>> {
    call("transcript", json!({ "sessionId": session, "agent": agent, "types": kinds, "last": last, "images": "register" }))
}

/// `wings.registerImage` (Wings 0.2.40): keeps a picture, a png, jpeg, gif or webp of up to 5 MB as base64, in Wings
/// under `id` (up to 64 letters, digits, `-` and `_`) for `ui::image` nodes to show. Send it once: a node names it by id,
/// so the bytes don't travel with each render. Wings keeps the most recently shown ones, so a node for one it let go of
/// shows a note. Nobody waits for it.
pub fn register_image(id: &str, data: &str) {
    drop(call("registerImage", json!({ "id": id, "data": data })));
}

/// `wings.onTranscript`: each entry Claude Code appends to the transcript of a session in a pane, as
/// `{ sessionId, entry }`.
pub fn on_transcript(listener: impl Fn(&Value) + 'static) {
    let first = ON_TRANSCRIPT.with(|listeners| {
        let mut listeners = listeners.borrow_mut();
        listeners.push(Rc::new(listener));
        listeners.len() == 1
    });
    if first {
        on("transcript", |_, entry| {
            for listener in ON_TRANSCRIPT.with(|listeners| listeners.borrow().clone()) {
                listener(entry);
            }
        });
    }
}

/// `wings.onAction`: a button of one of this plugin's badges was clicked. `listener` gets `{ paneId, actionId }`, and
/// the button shows progress until it is done. Its error shows by the button.
pub fn on_action<F, Fut>(listener: F)
where
    F: Fn(Value) -> Fut + 'static,
    Fut: Future<Output = Result<(), String>> + 'static,
{
    let first = ON_ACTION.with(|listeners| {
        let mut listeners = listeners.borrow_mut();
        listeners.push(Rc::new(move |action| Box::pin(listener(action))));
        listeners.len() == 1
    });
    if first {
        on("action", |token, action| {
            let action = action.clone();
            crate::task::spawn(async move {
                let mut error = None;
                for listener in ON_ACTION.with(|listeners| listeners.borrow().clone()) {
                    if let Err(message) = listener(action.clone()).await {
                        error = Some(message);
                    }
                }
                drop(call("actionDone", json!({ "token": token, "error": error })));
            });
        });
    }
}

/// `wings.ports`: the programs in terminal panes that listen on a local port, each with its `paneId`, `name`, `port`
/// and `startedAt`.
pub fn ports() -> Pending<Result<Value, String>> {
    call("ports", json!({}))
}

/// `wings.storage`: what the plugin keeps between runs, shared by its frames. Answered at once.
pub mod storage {
    #[cfg(target_arch = "wasm32")]
    use super::ffi;
    use serde_json::Value;

    /// Outside Wings nothing is kept: every key reads as null.
    #[cfg(not(target_arch = "wasm32"))]
    fn ask(_: &str, _: &str, _: &str) -> Result<Value, String> {
        Ok(Value::Null)
    }

    #[cfg(target_arch = "wasm32")]
    fn ask(op: &str, key: &str, json: &str) -> Result<Value, String> {
        // SAFETY: the three are live strings, which the host copies before this returns.
        let packed = unsafe { ffi::storage(op.as_ptr(), op.len(), key.as_ptr(), key.len(), json.as_ptr(), json.len()) };
        // SAFETY: the host wrote its answer into `wings_alloc` memory and returned where, and how long it is.
        let answer = unsafe { crate::taken((packed >> 32) as usize as *mut u8, (packed & 0xffff_ffff) as usize) };
        let mut answer: Value = serde_json::from_str(&answer).map_err(|error| error.to_string())?;
        match answer.get("error").and_then(Value::as_str) {
            Some(error) => Err(error.to_owned()),
            None => Ok(answer["ok"].take()),
        }
    }

    /// The value kept under `key`, or null.
    pub fn get(key: &str) -> Result<Value, String> {
        ask("get", key, "")
    }

    pub fn set(key: &str, value: &Value) -> Result<(), String> {
        ask("set", key, &value.to_string()).map(drop)
    }

    pub fn delete(key: &str) -> Result<(), String> {
        ask("delete", key, "").map(drop)
    }

    pub fn keys() -> Result<Value, String> {
        ask("keys", "", "")
    }

    /// The `[key, value]` pairs whose key starts with `prefix`.
    pub fn entries(prefix: &str) -> Result<Value, String> {
        ask("entries", prefix, "")
    }
}

/// Sends `message` to the plugin's other frames.
pub fn broadcast(message: &Value) {
    let json = message.to_string();
    // SAFETY: a live string, which the host copies before this returns.
    unsafe { ffi::broadcast(json.as_ptr(), json.len()) };
}

pub fn on_broadcast(listener: impl Fn(&Value) + 'static) {
    ON_BROADCAST.with(|list| list.borrow_mut().push(Rc::new(listener)));
}

/// Whether Wings is in front and this page is shown.
pub fn focused() -> bool {
    // SAFETY: takes nothing.
    FOCUSED.with(|focused| *focused.borrow()).unwrap_or_else(|| unsafe { ffi::focused() } != 0)
}

/// `wings.viewArgs`: what `open_view` opened this page's tab with, or none for a page opened without args. Needs
/// Wings 0.2.21: on an older one a module that calls this doesn't load.
pub fn view_args() -> Option<Value> {
    // SAFETY: takes nothing.
    let packed = unsafe { ffi::view_args() };
    let len = (packed & 0xffff_ffff) as usize;
    if len == 0 {
        return None;
    }
    // SAFETY: the host wrote the args into `wings_alloc` memory and returned where, and how long they are.
    serde_json::from_str(&unsafe { crate::taken((packed >> 32) as usize as *mut u8, len) }).ok()
}

/// Hears each change of `focused`.
pub fn on_focus(listener: impl Fn(bool) + 'static) {
    ON_FOCUS.with(|list| list.borrow_mut().push(Rc::new(listener)));
}

/// Milliseconds since the epoch.
pub fn now() -> f64 {
    // SAFETY: takes nothing.
    unsafe { ffi::now() }
}

/// How far this Mac's timezone is ahead of UTC at `ms`, in minutes.
pub fn utc_offset(ms: f64) -> i32 {
    // SAFETY: takes a number.
    unsafe { ffi::utc_offset(ms) }
}

/// What `wings.fetch` answers: the status and the body as text.
pub struct Response {
    pub status: u16,
    pub body: String,
}

/// `wings.fetch`, with the plugin's secret `bearer` as the token when given. The body is JSON.
pub async fn fetch(url: &str, method: &str, body: Option<&Value>, bearer: Option<&str>) -> Result<Response, String> {
    let mut params = json!({ "url": url, "method": method, "headers": {} });
    if let Some(body) = body {
        params["headers"] = json!({ "Content-Type": "application/json" });
        params["body"] = body.to_string().into();
    }
    if let Some(bearer) = bearer {
        params["bearer"] = bearer.into();
    }
    let answer = call("fetch", params).await?;
    Ok(Response {
        status: answer["status"].as_u64().unwrap_or(0) as u16,
        body: answer["body"].as_str().unwrap_or_default().to_owned(),
    })
}

pub async fn secret_has(name: &str) -> Result<bool, String> {
    Ok(call("secretHas", json!({ "name": name })).await? == true)
}

pub async fn secret_set(name: &str, value: &str) -> Result<(), String> {
    call("secretSet", json!({ "name": name, "value": value })).await.map(drop)
}

pub async fn secret_delete(name: &str) -> Result<(), String> {
    call("secretDelete", json!({ "name": name })).await.map(drop)
}

thread_local! {
    static SENT_LABELS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// `wings.setSidebarLabel`: the text by the title bar button of the plugin's sidebar or view `sidebar`, or none, and
/// the `(label, value)` rows shown when you point at it. A label that is the one last sent isn't sent again.
pub fn set_sidebar_label(sidebar: &str, label: Option<&str>, rows: &[(&str, &str)]) {
    let rows: Vec<Value> = rows.iter().map(|(label, value)| json!({ "label": label, "value": value })).collect();
    let sent = json!([label, rows]).to_string();
    if SENT_LABELS.with(|labels| labels.borrow_mut().insert(sidebar.to_owned(), sent.clone())).as_ref() == Some(&sent) {
        return;
    }
    let answer = call("setSidebarLabel", json!({ "sidebarId": sidebar, "label": label, "rows": rows }));
    let sidebar = sidebar.to_owned();
    crate::task::spawn(async move {
        // Refused: not shown, so the same label is sent again next time.
        if answer.await.is_err() {
            SENT_LABELS.with(|labels| {
                let mut labels = labels.borrow_mut();
                if labels.get(&sidebar) == Some(&sent) {
                    labels.remove(&sidebar);
                }
            });
        }
    });
}

/// `wings.openView`: shows the plugin's sidebar or view `id`. Nobody waits for it.
pub fn open_view(id: &str) {
    drop(call("openView", json!({ "id": id, "newTab": false })));
}

/// `wings.openView` with `as` ("tab", ...) and the `args` the view reads with `view_args`. A tab that is open with equal
/// args is shown rather than opened again.
pub fn open_view_with(id: &str, place: &str, args: &Value) -> Pending<Result<Value, String>> {
    call("openView", json!({ "id": id, "as": place, "args": args, "newTab": false }))
}

/// `open_view_with` as a tab, which, when it is a new one, goes right after the tab of this page in the tab bar rather than
/// at its end. Needs Wings 0.2.41.
pub fn open_tab_beside(id: &str, args: &Value) -> Pending<Result<Value, String>> {
    call("openView", json!({ "id": id, "as": "tab", "args": args, "newTab": false, "after": "self" }))
}

/// `wings.setProjectSection`: the rows of the plugin's section `section` under the project `project` in the sidebar, or
/// none for null to take the section away.
pub fn set_project_section(project: &str, section: &str, rows: &Value) -> Pending<Result<Value, String>> {
    call("setProjectSection", json!({ "projectId": project, "sectionId": section, "rows": rows }))
}

/// `wings.blockState`: what the block saved with `set_block_state`, or null.
pub fn block_state() -> Pending<Result<Value, String>> {
    call("blockState", json!({}))
}

/// `wings.setBlockState`: saves any JSON for the block.
pub fn set_block_state(state: &Value) -> Pending<Result<Value, String>> {
    call("setBlockState", json!({ "state": state }))
}

/// Opens `url` in the browser. Nobody waits for it.
pub fn open_url(url: &str) {
    drop(call("openUrl", json!({ "url": url })));
}

/// `wings.openFile`: `path`, the full path of a file in one of your Wings projects, in the editor chosen in Wings'
/// Settings, at `line` where the editor can. Wings opens it only right after your click or key in the page, once per
/// click. Needs Wings 0.2.42. The call goes out now.
pub fn open_file(path: &str, line: Option<u32>) -> Pending<Result<Value, String>> {
    call("openFile", json!({ "path": path, "line": line }))
}

/// `wings.pickFiles` for pictures: the system's file picker, `multiple` or one, and each png, jpeg, gif or webp picked as
/// its media type and base64, as a pasted one comes. Wings opens it only right after your click in the page. A file that
/// isn't one, or is over 3.75 MB, is left out, and so is what would take the pick past 7.5 MB. Cancelled, it gives none.
/// The call goes out now.
pub fn pick_images(multiple: bool) -> impl Future<Output = Result<Vec<(String, String)>, String>> {
    let answer = call("pickFiles", json!({ "kinds": ["image"], "multiple": multiple }));
    async move {
        let Value::Array(files) = answer.await? else {
            return Ok(Vec::new());
        };
        // Moved out of the answer, not copied: a picture is megabytes.
        Ok(files
            .into_iter()
            .filter_map(|file| match file {
                Value::Object(mut file) => match (file.remove("mediaType"), file.remove("data")) {
                    (Some(Value::String(media_type)), Some(Value::String(data))) => Some((media_type, data)),
                    _ => None,
                },
                _ => None,
            })
            .collect())
    }
}

/// `wings.diskUsage`: the tree of `path` as `{ root, floor, entries, restricted, ms }`, kept as the `options` say
/// (`minBytes`, `depth`, `maxEntries`, `reuse`). With `on_progress` it hears how far the scan is, as `{ entries, bytes,
/// path }`. The scan stops when its `stopper` is used, and then fails with "The scan was stopped".
pub fn disk_usage(path: &str, options: &Value, on_progress: Option<Rc<dyn Fn(&Value)>>) -> Search {
    let id = next_id();
    let token = format!("usage-{id}");
    let mut params = options.clone();
    params["path"] = path.into();
    params["token"] = token.clone().into();
    params["stream"] = on_progress.is_some().into();
    if let Some(on_progress) = on_progress {
        OUTPUTS.with(|outputs| outputs.borrow_mut().insert(id, on_progress));
    }
    Search { token, answer: call_as(id, "diskUsage", params) }
}

/// `wings.exec` with `onOutput`: `on_line` hears each line the program prints while it runs, and Wings stops it after
/// `timeout_ms`.
pub fn exec_lines(program: &str, args: &[&str], timeout_ms: u32, on_line: impl Fn(&str) + 'static) -> Pending<Result<Value, String>> {
    let id = next_id();
    OUTPUTS.with(|outputs| outputs.borrow_mut().insert(id, Rc::new(move |line| on_line(line.as_str().unwrap_or_default()))));
    call_as(id, "exec", json!({ "program": program, "args": args, "cwd": null, "timeoutMs": timeout_ms, "stream": true }))
}

/// A program the frame keeps running, from `spawn`. It resolves with `{ code, stderr }` once the program has ended
/// (`code` is null when a signal ended it, and `stderr` is the end of what it printed there). Letting go of it doesn't
/// stop the program.
pub struct Process {
    input: ProcessInput,
    answer: Pending<Result<Value, String>>,
}

impl Process {
    /// What writes to the program and stops it, to keep where a page's handlers reach it.
    pub fn input(&self) -> ProcessInput {
        self.input.clone()
    }
}

impl Future for Process {
    type Output = Result<Value, String>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.answer).poll(cx)
    }
}

#[derive(Clone)]
pub struct ProcessInput(String);

impl ProcessInput {
    /// Writes `text` to the program's input as it is, so a line needs its own `\n`. Writes reach the program in the
    /// order they were made.
    pub fn write(&self, text: &str) -> Pending<Result<Value, String>> {
        call("processWrite", json!({ "token": self.0, "text": text }))
    }

    /// Closes the program's input once the writes before it have gone, which tells a program like Claude to finish.
    /// Wings kills it if it is still running 3 s later.
    pub fn stop(&self) {
        drop(call("processStop", json!({ "token": self.0 })));
    }

    /// `wings.processNote`: keeps `note` (up to 1 MB of JSON) with the program, for the page that takes it over after an
    /// update or restart of the plugin (`processes`). With `heard`, the number of lines this page has heard of it, it has
    /// what it needs of those lines: Wings lets go of them, and the page that takes it over hears only the ones after.
    /// Until then Wings keeps what the program printed, up to 16 MB. Not a write. Needs Wings 0.2.42.
    pub fn note(&self, note: &Value, heard: Option<u64>) {
        drop(call("processNote", json!({ "token": self.0, "note": note, "heard": heard })));
    }

    /// `note`, saying which Claude Code session the program goes on with, once it has one: Wings starts no other program
    /// of the plugin on that session while this one runs. Needs Wings 0.2.47; an older Wings keeps the note alone.
    pub fn note_session(&self, note: &Value, heard: Option<u64>, session: Option<&str>) {
        drop(call("processNote", json!({ "token": self.0, "note": note, "heard": heard, "session": session })));
    }
}

/// A program the page before this one, in the same tab (or the main before this one), left running when the plugin was
/// updated or restarted, or Wings itself was.
pub struct Left {
    /// What `adopt` takes it over by.
    pub id: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    /// What that page kept with it (`ProcessInput::note`), or null.
    pub note: Value,
    /// How many lines it printed past that page's note, which the page that takes it over hears first.
    pub kept: u64,
    /// The Claude Code session it goes on with, when Wings knows it. Needs Wings 0.2.47.
    pub session: Option<String>,
    /// Another program of the plugin goes on with the session `processes_of` asked about: it can't be taken over, and
    /// the session mustn't be started again. Needs Wings 0.2.47.
    pub elsewhere: bool,
}

/// `wings.processes`: the programs this page's tab ran when the plugin was updated or restarted, or Wings was, which
/// Wings kept running for this page to take over with `adopt`. Wings stops each one no page takes over within 30 s (2
/// minutes after Wings restarts), and one the new version's manifest no longer allows. Empty for a page that didn't
/// load in place of another. Needs Wings 0.2.42.
pub async fn processes() -> Result<Vec<Left>, String> {
    processes_of(None).await
}

/// `processes`, and, after them, a `Left` that is `elsewhere` when another program of the plugin, in another tab or left
/// from before Wings restarted, goes on with the Claude Code session `session`. Needs Wings 0.2.47 for that; an older
/// Wings lists only this page's.
pub async fn processes_of(session: Option<&str>) -> Result<Vec<Left>, String> {
    let params = match session {
        Some(session) => json!({ "session": session }),
        None => json!({}),
    };
    let listed = call("processes", params).await?;
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    Ok(listed
        .as_array()
        .into_iter()
        .flatten()
        .map(|left| Left {
            id: text(&left["id"]),
            program: text(&left["program"]),
            args: left["args"].as_array().into_iter().flatten().map(text).collect(),
            cwd: text(&left["cwd"]),
            note: left["note"].clone(),
            kept: left["kept"].as_u64().unwrap_or(0),
            session: left["session"].as_str().map(str::to_owned),
            elsewhere: left["elsewhere"] == true,
        })
        .collect())
}

/// `wings.adopt`: takes over the program `left` (a `Left`'s id), which is then this page's as if it had spawned it.
/// `on_line` hears the lines `kept` names first, in order, then each new one; when Wings let go of some of them for
/// room, a line `{"wingsDropped": n}` comes before them. It resolves when the program ends. Taking it over writes
/// nothing to it. Needs Wings 0.2.42.
pub fn adopt(left: &str, on_line: impl Fn(&Value) + 'static) -> Process {
    let id = next_id();
    let token = format!("process-{id}");
    OUTPUTS.with(|outputs| outputs.borrow_mut().insert(id, Rc::new(on_line)));
    let answer = call_as(id, "adopt", json!({ "from": left, "token": token }));
    Process { input: ProcessInput(token), answer }
}

/// `wings.spawn`: runs a program the manifest's `processes` lists, in the folder `cwd`, which has to be in one of your
/// projects, and keeps it running until it ends, it is stopped, or the frame closes. `input` is what it reads first,
/// sent with its start (empty for nothing): later writes need your click or key. `on_line` hears each line it prints to
/// stdout, in order and none lost: as JSON when the line is JSON, and as a string when it isn't.
pub fn spawn(program: &str, args: &[&str], cwd: &str, input: &str, on_line: impl Fn(&Value) + 'static) -> Process {
    spawn_with(None, false, program, args, cwd, input, on_line)
}

/// The Claude Code account a `spawn` or `exec` runs under, set as `CLAUDE_CONFIG_DIR`. Wings picks the folder from the
/// accounts it knows: a plugin never names one. Needs Wings 0.2.39.
pub struct Account(Value);

impl Account {
    /// The account whose folder holds the transcript of `session`, so `claude --resume` finds it; for a new chat (`None`)
    /// and a session with no transcript yet, the account of the project that holds the program's folder, as a new
    /// terminal there gets. The default account leaves Wings' own environment as it is.
    pub fn of_session(session: Option<&str>) -> Self {
        Account(json!({ "session": session }))
    }

    /// `of_session`, except that a new chat (no `session`, or one with no transcript yet) runs under the account named
    /// `name` from `accounts`, `None` being the default one, instead of its project's. Needs the `claudeAccounts`
    /// permission and Wings 0.2.53.
    pub fn picked(session: Option<&str>, name: Option<&str>) -> Self {
        Account(json!({ "session": session, "pick": { "name": name } }))
    }
}

/// A Claude Code account a plugin may start Claude Code under (`Account::picked`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountChoice {
    /// Its name, `None` for the default account.
    pub name: Option<String>,
    /// The email of who signed in to it, when its config names one.
    pub email: Option<String>,
    /// Whether its config says someone signed in to it. The sign-in may be gone since.
    pub signed_in: bool,
}

/// `wings.claudeAccountChoices`: the Claude Code accounts there are, the default one first. Needs the `claudeAccounts`
/// permission and Wings 0.2.53.
pub async fn accounts() -> Result<Vec<AccountChoice>, String> {
    let listed = call("claudeAccountChoices", json!({})).await?;
    let text = |value: &Value| value.as_str().map(str::to_owned);
    Ok(listed
        .as_array()
        .into_iter()
        .flatten()
        .map(|account| AccountChoice { name: text(&account["name"]), email: text(&account["email"]), signed_in: account["signedIn"] == true })
        .collect())
}

/// `spawn` under a Claude Code account.
pub fn spawn_as(account: &Account, program: &str, args: &[&str], cwd: &str, input: &str, on_line: impl Fn(&Value) + 'static) -> Process {
    spawn_with(Some(account), false, program, args, cwd, input, on_line)
}

/// `spawn_as`, run by Wings' terminal server when it runs one, so the program goes on while Wings quits, restarts or
/// updates: the page that opens in this tab in Wings' next run takes it over (`processes`, `adopt`). Only a page of one
/// of Wings' own plugins shown as a tab keeps one; elsewhere, without the server, and in a Wings older than 0.2.47 it
/// runs as `spawn_as` does.
pub fn spawn_surviving(account: &Account, program: &str, args: &[&str], cwd: &str, input: &str, on_line: impl Fn(&Value) + 'static) -> Process {
    spawn_with(Some(account), true, program, args, cwd, input, on_line)
}

fn spawn_with(account: Option<&Account>, survive: bool, program: &str, args: &[&str], cwd: &str, input: &str, on_line: impl Fn(&Value) + 'static) -> Process {
    let id = next_id();
    let token = format!("process-{id}");
    OUTPUTS.with(|outputs| outputs.borrow_mut().insert(id, Rc::new(on_line)));
    let input = (!input.is_empty()).then_some(input);
    let account = account.map(|account| &account.0);
    let mut params = json!({ "program": program, "args": args, "cwd": cwd, "token": token, "input": input, "account": account });
    if survive {
        params["survive"] = true.into();
    }
    let answer = call_as(id, "spawn", params);
    Process { input: ProcessInput(token), answer }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{block_on, run_ready};

    /// Answers every call with `Ok(null)` and keeps what was called, in order. A method in `fail` is refused once.
    fn record(fail: &'static [&'static str]) -> Rc<RefCell<Vec<(String, Value)>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        let refused = RefCell::new(fail.to_vec());
        testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            let mut refused = refused.borrow_mut();
            match refused.iter().position(|name| *name == method) {
                Some(at) => {
                    refused.remove(at);
                    Err("no pane".into())
                }
                None => Ok(Value::Null),
            }
        });
        calls
    }

    #[test]
    fn a_pane_listener_that_comes_late_hears_the_last_list() {
        let heard = Rc::new(RefCell::new(Vec::new()));
        let listen = |name: &'static str| {
            let heard = heard.clone();
            on_panes(move |list| heard.borrow_mut().push((name, list.len())));
        };
        listen("first");
        deliver("panes", 0, r#"[{"paneId":"1"},{"paneId":"2"}]"#.into());
        listen("late");
        run_ready();
        assert_eq!(*heard.borrow(), [("first", 2), ("late", 2)]);
    }

    #[test]
    fn a_badge_a_pane_already_shows_is_not_sent_again_and_one_the_host_refused_is() {
        let calls = record(&["setBadge"]);
        let badge = json!({ "label": "x" });
        let set = |badge: Value| block_on(async move { set_badge("9", &badge).await });
        assert_eq!(set(badge.clone()), Err("no pane".into()));
        assert_eq!(set(badge.clone()), Ok(()));
        assert_eq!(set(badge.clone()), Ok(()));
        assert_eq!(set(Value::Null), Ok(()));
        assert_eq!(set(Value::Null), Ok(()));
        let sent: Vec<Value> = calls.borrow().iter().map(|(_, params)| params["badge"].clone()).collect();
        assert_eq!(sent, [badge.clone(), badge, Value::Null]);
    }

    #[test]
    fn a_program_asks_for_an_account_only_when_the_page_does() {
        let calls = record(&[]);
        let _plain = spawn("claude", &["-p"], "/repo", "", |_| {});
        let _resumed = spawn_as(&Account::of_session(Some("s1")), "claude", &["-p"], "/repo", "", |_| {});
        drop(exec_as(&Account::of_session(None), "/repo", "claude", &["plugin", "list"], None));
        let sent: Vec<Value> = calls.borrow().iter().map(|(_, params)| params["account"].clone()).collect();
        assert_eq!(sent, [Value::Null, json!({ "session": "s1" }), json!({ "session": null })]);
    }

    #[test]
    fn a_new_chat_picks_its_account_by_name_and_lists_them() {
        let calls = record(&[]);
        let _work = spawn_as(&Account::picked(None, Some("work")), "claude", &["-p"], "/repo", "", |_| {});
        let _default = spawn_as(&Account::picked(Some("s1"), None), "claude", &["-p"], "/repo", "", |_| {});
        let sent: Vec<Value> = calls.borrow().iter().map(|(_, params)| params["account"].clone()).collect();
        assert_eq!(sent, [json!({ "session": null, "pick": { "name": "work" } }), json!({ "session": "s1", "pick": { "name": null } })]);
        testing::answer(|method, _| match method {
            "claudeAccountChoices" => Ok(json!([{ "name": null, "email": "me@home", "signedIn": true }, { "name": "work", "email": null, "signedIn": false }])),
            _ => Ok(Value::Null),
        });
        let listed = block_on(accounts()).unwrap();
        assert_eq!(listed[0], AccountChoice { name: None, email: Some("me@home".into()), signed_in: true });
        assert_eq!(listed[1].name.as_deref(), Some("work"));
    }

    #[test]
    fn a_page_takes_over_a_program_it_was_left_by_its_id_under_a_token_of_its_own() {
        testing::answer(|method, params| match method {
            "processes" => Ok(json!([{ "id": "left-0", "token": "process-3", "program": "claude", "args": ["-p"], "cwd": "/repo", "note": { "cut": "u1" }, "kept": 2 }])),
            _ => Ok(params.clone()),
        });
        let left = block_on(processes()).unwrap();
        assert_eq!((left[0].id.as_str(), left[0].args.as_slice(), left[0].kept), ("left-0", &["-p".to_owned()][..], 2));
        assert_eq!(left[0].note, json!({ "cut": "u1" }));
        let calls = record(&[]);
        let taken = adopt(&left[0].id, |_| {});
        taken.input().note(&json!({ "cut": "u2" }), Some(5));
        let sent: Vec<(String, Value)> = calls.borrow().clone();
        let token = sent[0].1["token"].clone();
        assert_eq!(sent[0], ("adopt".to_owned(), json!({ "from": "left-0", "token": token })));
        assert_eq!(sent[1], ("processNote".to_owned(), json!({ "token": token, "note": { "cut": "u2" }, "heard": 5 })));
    }

    #[test]
    fn a_program_that_survives_asks_for_it_and_a_page_asks_whether_a_session_runs() {
        let calls = record(&[]);
        let _plain = spawn_as(&Account::of_session(None), "claude", &["-p"], "/repo", "", |_| {});
        let kept = spawn_surviving(&Account::of_session(Some("s1")), "claude", &["-p", "--resume=s1"], "/repo", "", |_| {});
        kept.input().note_session(&json!({}), None, Some("s1"));
        let sent: Vec<(String, Value)> = calls.borrow().clone();
        assert_eq!(sent[0].1.get("survive"), None, "a spawn that needn't survive is sent as before");
        assert_eq!(sent[1].1["survive"], true);
        assert_eq!(sent[2], ("processNote".to_owned(), json!({ "token": sent[1].1["token"], "note": {}, "heard": null, "session": "s1" })));
        testing::answer(|method, params| match method {
            "processes" => Ok(json!([{ "elsewhere": true, "session": params["session"] }])),
            _ => Ok(Value::Null),
        });
        let left = block_on(processes_of(Some("s1"))).unwrap();
        assert!(left[0].elsewhere);
        assert_eq!(left[0].session.as_deref(), Some("s1"));
    }

    #[test]
    fn a_sidebar_label_that_is_the_one_last_sent_is_not_sent_again() {
        let calls = record(&[]);
        set_sidebar_label("tracker", Some("same"), &[]);
        set_sidebar_label("tracker", Some("same"), &[]);
        set_sidebar_label("tracker", None, &[]);
        run_ready();
        let sent: Vec<Value> = calls.borrow().iter().map(|(_, params)| params["label"].clone()).collect();
        assert_eq!(sent, [json!("same"), Value::Null]);
    }

    #[test]
    fn picked_pictures_come_as_their_media_type_and_base64() {
        testing::answer(|method, params| {
            assert_eq!((method, params), ("pickFiles", &json!({ "kinds": ["image"], "multiple": true })));
            Ok(json!([{ "mediaType": "image/png", "data": "AAAA" }, { "mediaType": "image/gif", "data": "BBBB" }]))
        });
        let picked = block_on(pick_images(true)).unwrap();
        assert_eq!(picked, [("image/png".to_owned(), "AAAA".to_owned()), ("image/gif".to_owned(), "BBBB".to_owned())]);
    }

    #[test]
    fn a_click_settles_when_every_action_listener_has_with_the_last_error() {
        let calls = record(&[]);
        on_action(|_| async { Err("first".into()) });
        on_action(|_| async { Err("second".into()) });
        on_action(|_| async { Ok(()) });
        deliver("action", 7, r#"{"paneId":"1","actionId":"open"}"#.into());
        run_ready();
        assert_eq!(*calls.borrow(), [("actionDone".to_owned(), json!({ "token": 7, "error": "second" }))]);
    }
}
