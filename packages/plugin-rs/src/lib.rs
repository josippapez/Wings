//! A Wings plugin written in Rust and built to `wasm32-unknown-unknown`: its main, which only runs, and its pages. The
//! native app runs each as a module, and draws the tree a page renders with its own controls
//! (`apps/native/src/page/runtime/wasm.rs` has the imports and exports this file is the other side of).
//!
//! A main defines `start`, and there sets up what it listens to (`host`, `messages`, `tools`, `poll`). A page does
//! the same and calls `run`. It keeps its state where it likes, gives `run` a function that builds the page from it, and changes the state
//! in handlers and tasks. The page is built again after every turn (an event, an answer, a timer), and sent to the
//! app when it differs from the last one.

pub mod format;
pub mod host;
pub mod messages;
pub mod poll;
pub mod sessions;
pub mod task;
pub mod tools;
pub mod ui;

use serde_json::Value;
use std::cell::RefCell;

thread_local! {
    static VIEW: RefCell<Option<fn() -> Vec<ui::Node>>> = const { RefCell::new(None) };
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Shows what `view` builds as the page. Call it once, from `start`.
pub fn run(view: fn() -> Vec<ui::Node>) {
    VIEW.with(|slot| *slot.borrow_mut() = Some(view));
}

/// Runs the tasks that can go on, then builds the page and sends it if it changed.
fn settle() {
    task::run_ready();
    let Some(view) = VIEW.with(|slot| *slot.borrow()) else {
        return;
    };
    let json = ui::render(view());
    let changed = LAST.with(|last| {
        let changed = *last.borrow() != json;
        if changed {
            last.borrow_mut().clone_from(&json);
        }
        changed
    });
    if changed {
        // SAFETY: the pointer and length are of a live `String`, which the host copies before this returns.
        unsafe { host::ffi::render(json.as_ptr(), json.len()) };
    }
}

/// A string the host wrote into memory `wings_alloc` gave it.
pub(crate) unsafe fn taken(at: *mut u8, len: usize) -> String {
    // SAFETY: `at` came from `wings_alloc(len)`, a `Vec` of that capacity, and the host filled all of it with UTF-8.
    unsafe { String::from_utf8_unchecked(Vec::from_raw_parts(at, len, len)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn wings_alloc(len: usize) -> *mut u8 {
    let mut bytes = Vec::<u8>::with_capacity(len);
    let at = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    at
}

unsafe extern "Rust" {
    /// The plugin's entry: it sets its state up and calls `run`.
    fn start();
}

#[unsafe(no_mangle)]
pub extern "C" fn wings_start() {
    // SAFETY: the plugin crate defines `start` with `#[unsafe(no_mangle)]`, taking and returning nothing.
    unsafe { start() };
    settle();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wings_event(handler: *mut u8, handler_len: usize, payload: *mut u8, payload_len: usize) {
    // SAFETY: both are strings the host wrote into `wings_alloc` memory.
    let (handler, payload) = unsafe { (taken(handler, handler_len), taken(payload, payload_len)) };
    ui::dispatch(&handler, serde_json::from_str(&payload).unwrap_or(Value::Null));
    settle();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wings_deliver(kind: *mut u8, kind_len: usize, id: u32, text: *mut u8, text_len: usize) {
    // SAFETY: both are strings the host wrote into `wings_alloc` memory.
    let (kind, text) = unsafe { (taken(kind, kind_len), taken(text, text_len)) };
    host::deliver(&kind, id, text);
    settle();
}

#[unsafe(no_mangle)]
pub extern "C" fn wings_timer(id: u32) {
    host::fired(id);
    settle();
}
