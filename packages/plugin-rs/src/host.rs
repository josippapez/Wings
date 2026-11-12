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
}

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
    let id = next_id();
    let wait = Rc::new(RefCell::new(Wait { done: None, waker: None }));
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
            let Some(wait) = CALLS.with(|calls| calls.borrow_mut().remove(&id)) else {
                return;
            };
            finish(
                &wait,
                if kind == "resolve" { Ok(serde_json::from_str(&text).unwrap_or(Value::Null)) } else { Err(text) },
            );
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

/// `wings.storage`: what the plugin keeps between runs, shared by its frames. Answered at once.
pub mod storage {
    use super::ffi;
    use serde_json::Value;

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

/// Opens `url` in the browser. Nobody waits for it.
pub fn open_url(url: &str) {
    drop(call("openUrl", json!({ "url": url })));
}
