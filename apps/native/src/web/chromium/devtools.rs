//! A Chromium page's own DevTools channel, in this process: requests go in with `CefBrowserHost::SendDevToolsMessage`,
//! and answers and events come back to an observer CEF calls on the main thread (`AddDevToolsMessageObserver`). No port
//! and no pipe: nothing outside Wings reaches it. A cross-site frame runs in a process of its own, a target of its own
//! that the page's session attaches to (`Target.setAutoAttach` with `flatten`), and its messages go over this channel
//! too, named by their `sessionId`.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Weak,
};

use cef::*;
use serde_json::{Value, json};

use super::page::Shared;

/// What hears the answer to one request: its `result`, or the protocol's error message.
pub type Answered = Box<dyn FnOnce(Result<Value, String>)>;

/// One page's channel, alive as long as its page: the observer goes with `registration`.
pub struct Channel {
    host: BrowserHost,
    _registration: Registration,
    next: Cell<u64>,
    asked: RefCell<HashMap<u64, Answered>>,
}

impl Channel {
    /// Opens `browser`'s channel; what it hears goes to `page` (`Shared::devtools`).
    pub fn open(browser: &Browser, page: Weak<Shared>) -> Option<Self> {
        let host = browser.host()?;
        let registration = host.add_dev_tools_message_observer(Some(&mut PageDevTools::new(page)))?;
        Some(Self { host, _registration: registration, next: Cell::new(1), asked: RefCell::default() })
    }

    /// Asks `method` with `params` of the page, or of the frame target `session`, and `done` hears the answer.
    pub fn ask(&self, session: Option<&str>, method: &str, params: Value, done: impl FnOnce(Result<Value, String>) + 'static) {
        let id = self.next.replace(self.next.get() + 1);
        // In before it is sent: Chromium answers some requests (the `Target` domain's) while it is being sent.
        self.asked.borrow_mut().insert(id, Box::new(done));
        if self.host.send_dev_tools_message(Some(request(id, method, params, session).as_bytes())) == 0
            && let Some(done) = self.asked.borrow_mut().remove(&id)
        {
            done(Err("Chromium's DevTools didn't take the request".into()));
        }
    }

    /// The answer to the request `id` came.
    pub fn answered(&self, id: u64, result: Result<Value, String>) {
        let done = self.asked.borrow_mut().remove(&id);
        if let Some(done) = done {
            done(result);
        }
    }
}

thread_local! {
    /// The one-off requests (`once`) waiting for their answer, with the observer that hears it.
    static ONCE: RefCell<HashMap<u64, (Registration, Answered)>> = RefCell::default();
    /// Their ids, far above any a page's channel reaches, as every observer of a browser hears every answer, and below
    /// 2^31: the protocol reads an id as a 32-bit int, and answers none above.
    static NEXT_ONCE: Cell<u64> = const { Cell::new(1 << 30) };
}

/// Asks `method` with `params` of `browser`'s page once, on its own observer, whether or not its page has a channel
/// (`Channel`): `done` hears the answer, on a turn of its own.
pub fn once(browser: &Browser, method: &str, params: Value, done: impl FnOnce(Result<Value, String>) + 'static) {
    let Some(host) = browser.host() else { return done(Err("The page has no browser".into())) };
    let id = NEXT_ONCE.replace(NEXT_ONCE.get() + 1);
    let Some(registration) = host.add_dev_tools_message_observer(Some(&mut Once::new(id))) else {
        return done(Err("Chromium's DevTools can't be heard".into()));
    };
    ONCE.with_borrow_mut(|once| once.insert(id, (registration, Box::new(done))));
    if host.send_dev_tools_message(Some(request(id, method, params, None).as_bytes())) == 0
        && let Some((_, done)) = ONCE.with_borrow_mut(|once| once.remove(&id))
    {
        done(Err("Chromium's DevTools didn't take the request".into()));
    }
}

/// A request as the protocol writes it, sent to the target of `session` (the page's own for none).
pub fn request(id: u64, method: &str, params: Value, session: Option<&str>) -> String {
    let mut message = json!({ "id": id, "method": method, "params": params });
    if let Some(session) = session {
        message["sessionId"] = json!(session);
    }
    message.to_string()
}

/// What came over the channel.
#[derive(Debug, PartialEq)]
pub enum Incoming {
    /// The answer to the request `id`.
    Answer { id: u64, result: Result<Value, String> },
    /// An event, of the page's own session or of the frame target `session`.
    Event { method: String, params: Value, session: Option<String> },
}

/// The events a page follows (`Shared::devtools`): any other is left unread, as a busy page's console sends one for each
/// line while `Runtime` is on.
const FOLLOWED: [&str; 7] = [
    "Runtime.executionContextCreated",
    "Runtime.executionContextDestroyed",
    "Runtime.executionContextsCleared",
    "Runtime.bindingCalled",
    "Target.attachedToTarget",
    "Target.detachedFromTarget",
    "Page.screencastFrame",
];

/// The name of the event `message` is, read from its first bytes, before any of it is parsed: none for an answer, or for
/// one written another way than Chromium writes it.
pub fn event_name(message: &[u8]) -> Option<&[u8]> {
    let rest = message.strip_prefix(b"{\"method\":\"")?;
    Some(&rest[..rest.iter().position(|byte| *byte == b'"')?])
}

/// Reads one message, or none for an event no page follows (by its first bytes, before any is parsed) or what isn't a
/// message of the protocol.
pub fn parse(message: &[u8]) -> Option<Incoming> {
    if event_name(message).is_some_and(|name| !FOLLOWED.iter().any(|followed| followed.as_bytes() == name)) {
        return None;
    }
    let mut message: Value = serde_json::from_slice(message).ok()?;
    if let Some(id) = message["id"].as_u64() {
        let result = match message.get("error") {
            Some(error) => Err(error["message"].as_str().unwrap_or("Chromium refused").to_owned()),
            None => Ok(message["result"].take()),
        };
        return Some(Incoming::Answer { id, result });
    }
    let method = message["method"].as_str()?.to_owned();
    if !FOLLOWED.contains(&method.as_str()) {
        return None;
    }
    let session = message["sessionId"].as_str().map(str::to_owned);
    Some(Incoming::Event { method, params: message["params"].take(), session })
}

wrap_dev_tools_message_observer! {
    struct PageDevTools {
        page: Weak<Shared>,
    }

    impl DevToolsMessageObserver {
        /// Every message is read here, so CEF parses none of them again.
        fn on_dev_tools_message(&self, _browser: Option<&mut Browser>, message: Option<&[u8]>) -> ::std::os::raw::c_int {
            if let (Some(page), Some(incoming)) = (self.page.upgrade(), message.and_then(parse)) {
                page.devtools(incoming);
            }
            1
        }
    }
}

wrap_dev_tools_message_observer! {
    struct Once {
        id: u64,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_message(&self, _browser: Option<&mut Browser>, message: Option<&[u8]>) -> ::std::os::raw::c_int {
            if let Some(Incoming::Answer { id, result }) = message.and_then(parse).filter(|incoming| matches!(incoming, Incoming::Answer { id, .. } if *id == self.id))
                && let Some((registration, done)) = ONCE.with_borrow_mut(|once| once.remove(&id))
            {
                // Its observer goes after CEF is done calling it.
                super::next_turn(move || {
                    drop(registration);
                    done(result);
                });
            }
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_request_names_its_frames_session_only_when_it_has_one() {
        let page: Value = serde_json::from_str(&request(3, "Runtime.evaluate", json!({ "expression": "1" }), None)).unwrap();
        assert_eq!(page, json!({ "id": 3, "method": "Runtime.evaluate", "params": { "expression": "1" } }));
        let frame: Value = serde_json::from_str(&request(4, "Runtime.runIfWaitingForDebugger", json!({}), Some("F1"))).unwrap();
        assert_eq!(frame["sessionId"], "F1");
    }

    #[test]
    fn answers_and_followed_events_are_read_and_a_busy_consoles_lines_are_left_unread() {
        assert_eq!(parse(br#"{"id":7,"result":{"data":"x"}}"#), Some(Incoming::Answer { id: 7, result: Ok(json!({ "data": "x" })) }));
        assert_eq!(
            parse(br#"{"id":8,"error":{"code":-32000,"message":"Cannot find context with specified id"}}"#),
            Some(Incoming::Answer { id: 8, result: Err("Cannot find context with specified id".into()) })
        );
        assert_eq!(
            parse(br#"{"method":"Runtime.bindingCalled","params":{"name":"wingsGuard"},"sessionId":"F1"}"#),
            Some(Incoming::Event { method: "Runtime.bindingCalled".into(), params: json!({ "name": "wingsGuard" }), session: Some("F1".into()) })
        );
        assert_eq!(parse(br#"{"method":"Runtime.consoleAPICalled","params":{"args":[]}}"#), None);
        // Written another way, it is still left unread once parsed.
        assert_eq!(parse(br#"{ "method": "Page.lifecycleEvent", "params": {} }"#), None);
        assert_eq!(parse(b"not json"), None);
    }
}
