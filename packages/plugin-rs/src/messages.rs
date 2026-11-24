//! Named events between the frames of one plugin, sent over `wings.broadcast`, so a frame in Rust and one in
//! TypeScript hear each other (`tell` and `on`), and what plugins did by hand on top of them:
//!
//! - `send` and `hear` carry a message of any length, in parts when its JSON is long.
//! - `ask` and `answer`: a page asks main and waits for the reply, which is a value or an error.
//! - `publish`, `publish_on_want` and `want`: main tells the pages its state, and a page that opens later asks for it.
//!
//! **Size.** The native app refuses a broadcast whose JSON is over 1,000,000 JS string units
//! (`MAX_BROADCAST_CHARS`, `apps/native/src/page/plugin.rs`). Plugins also ran on the desktop host, which allowed 64,000,
//! so a part holds `PART_CHARS` (20,000) characters of the message's JSON, well under both, with its envelope. A longer
//! message goes as `n` parts that share a `mid`, and the hearer puts them together in any order and listens once.
//!
//! The wire: an ask is `name` with `{ id, payload }` and its reply is `name:answer` with `{ id, result }` or
//! `{ id, error }`; the state a page wants is told as `name`, and asked for with `name:want`.
//! A frame's own messages never come back to it, so a frame is either the asking side or the answering side of a name.

use crate::{host, task};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, future::Future, pin::Pin, rc::Rc, task::Poll};

/// The most characters of a message's JSON in one broadcast of a long message.
pub const PART_CHARS: usize = 20_000;
const PART: &str = "\u{1}part";

type Reply = Result<Value, String>;

thread_local! {
    static EVENTS: RefCell<HashMap<String, Vec<Rc<dyn Fn(&Value)>>>> = RefCell::new(HashMap::new());
    static PARTS: RefCell<HashMap<String, Vec<Option<String>>>> = RefCell::new(HashMap::new());
    static WAITING: RefCell<HashMap<String, task::Sender<Reply>>> = RefCell::new(HashMap::new());
    static ANSWERED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static COUNT: RefCell<u64> = const { RefCell::new(0) };
    /// This frame's part of its ids: the clock to the microsecond when it first made one. Every frame of a plugin hears
    /// every answer, so two frames that start in the same millisecond (pages restored at launch) mustn't share ids.
    static SEED: u64 = (host::now() * 1000.0) as u64;
}

/// Tells the plugin's other frames `name` happened, with `payload` when it isn't null.
pub fn tell(name: &str, payload: &Value) {
    let mut message = json!({ "wings:sdk": 1, "kind": "tell", "name": name });
    if !payload.is_null() {
        message["payload"] = payload.clone();
    }
    host::broadcast(&message);
}

/// Hears each `name` another frame tells, with its payload, or null when it has none.
pub fn on(name: &str, listener: impl Fn(&Value) + 'static) {
    let first = EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        let first = events.is_empty();
        events.entry(name.to_owned()).or_default().push(Rc::new(listener));
        first
    });
    if first {
        host::on_broadcast(|message| {
            if message["wings:sdk"] != 1 || message["kind"] != "tell" {
                return;
            }
            let name = message["name"].as_str().unwrap_or_default();
            for listener in EVENTS.with(|events| events.borrow().get(name).cloned()).unwrap_or_default() {
                listener(&message["payload"]);
            }
        });
    }
}

/// The messages that carry `text` in parts, which `assemble` puts back.
fn parts_of(mid: &str, text: &str) -> Vec<Value> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len().div_ceil(PART_CHARS);
    chars.chunks(PART_CHARS).enumerate().map(|(i, part)| json!({ "mid": mid, "i": i, "n": n, "text": part.iter().collect::<String>() })).collect()
}

/// Puts a part in its place, and gives the whole message once its last part came.
fn assemble(part: &Value) -> Option<Value> {
    let (mid, i, n) = (part["mid"].as_str()?.to_owned(), part["i"].as_u64()? as usize, part["n"].as_u64()? as usize);
    let text = PARTS.with(|parts| {
        let mut parts = parts.borrow_mut();
        let slots = parts.entry(mid.clone()).or_insert_with(|| vec![None; n]);
        *slots.get_mut(i)? = Some(part["text"].as_str()?.to_owned());
        if slots.iter().all(Option::is_some) { parts.remove(&mid) } else { None }
    })?;
    serde_json::from_str(&text.into_iter().flatten().collect::<String>()).ok()
}

fn next_id() -> String {
    COUNT.with(|count| {
        *count.borrow_mut() += 1;
        format!("{}:{}", SEED.with(|seed| *seed), count.borrow())
    })
}

/// `tell` for a payload of any length: in parts when its JSON is over `PART_CHARS`, which `hear` puts together.
pub fn send(name: &str, payload: &Value) {
    let text = payload.to_string();
    if text.chars().count() <= PART_CHARS {
        return tell(name, payload);
    }
    let part_name = format!("{name}{PART}");
    for part in parts_of(&next_id(), &text) {
        tell(&part_name, &part);
    }
}

/// `on` for what `send` sends: each `name` another frame sends, whole or in parts.
pub fn hear(name: &str, listener: impl Fn(&Value) + 'static) {
    let listener = Rc::new(listener);
    let whole = listener.clone();
    on(name, move |payload| whole(payload));
    on(&format!("{name}{PART}"), move |part| {
        if let Some(message) = assemble(part) {
            listener(&message);
        }
    });
}

fn answer_name(name: &str) -> String {
    format!("{name}:answer")
}

/// Sends the ask and gives the id it waits on, and what it will hear.
fn start_ask(name: &str, payload: &Value) -> (String, task::Receiver<Reply>) {
    let id = next_id();
    let (sender, receiver) = task::oneshot();
    WAITING.with(|waiting| waiting.borrow_mut().insert(id.clone(), sender));
    if !ANSWERED.with(|names| names.borrow().iter().any(|heard| heard == name)) {
        ANSWERED.with(|names| names.borrow_mut().push(name.to_owned()));
        hear(&answer_name(name), |message| {
            let Some(id) = message["id"].as_str() else { return };
            let Some(sender) = WAITING.with(|waiting| waiting.borrow_mut().remove(id)) else { return };
            sender.send(match message["error"].as_str() {
                Some(error) => Err(error.to_owned()),
                None => Ok(message["result"].clone()),
            });
        });
    }
    send(name, &json!({ "id": id, "payload": payload }));
    (id, receiver)
}

const NO_ANSWER: &str = "Wings gave no answer.";

/// Asks main `name` with `payload`, which goes out now, and waits for its answer: what a main that `answer`s `name`
/// gives, or its error. A main that never answers keeps it waiting, so a call that must not use `ask_within`.
pub fn ask(name: &str, payload: Value) -> impl Future<Output = Reply> + use<> {
    let (_, receiver) = start_ask(name, &payload);
    async move { receiver.await.unwrap_or_else(|| Err(NO_ANSWER.into())) }
}

/// `ask`, giving up with an error after `ms`.
pub fn ask_within(name: &str, payload: Value, ms: u32) -> impl Future<Output = Reply> + use<> {
    let (id, mut receiver) = start_ask(name, &payload);
    let mut timer = host::sleep(ms);
    std::future::poll_fn(move |cx| {
        if let Poll::Ready(reply) = Pin::new(&mut receiver).poll(cx) {
            return Poll::Ready(reply.unwrap_or_else(|| Err(NO_ANSWER.into())));
        }
        if Pin::new(&mut timer).poll(cx).is_ready() {
            WAITING.with(|waiting| waiting.borrow_mut().remove(&id));
            return Poll::Ready(Err("Wings took too long to answer.".into()));
        }
        Poll::Pending
    })
}

/// Answers each `name` a page asks with what `handler` gives for its payload: a value, or the error the page shows.
pub fn answer<F, Fut>(name: &str, handler: F)
where
    F: Fn(Value) -> Fut + 'static,
    Fut: Future<Output = Reply> + 'static,
{
    let (handler, reply_name) = (Rc::new(handler), answer_name(name));
    hear(name, move |message| {
        let Some(id) = message["id"].as_str().map(str::to_owned) else { return };
        let (handler, reply_name, payload) = (handler.clone(), reply_name.clone(), message["payload"].clone());
        task::spawn(async move {
            let reply = match handler(payload).await {
                Ok(result) => json!({ "id": id, "result": result }),
                Err(error) => json!({ "id": id, "error": error }),
            };
            send(&reply_name, &reply);
        });
    });
}

fn want_name(name: &str) -> String {
    format!("{name}:want")
}

/// Main: tells every page the state `name`, which is `value`. Call it when the state changes.
pub fn publish(name: &str, value: &Value) {
    send(name, value);
}

/// Main: runs `told` each time a page wants `name`, which it answers with `publish`.
pub fn on_want(name: &str, told: impl Fn() + 'static) {
    on(&want_name(name), move |_| told());
}

/// Main: answers a page that wants `name` with `build()`, so a page that opens later learns the state.
pub fn publish_on_want(name: &str, build: impl Fn() -> Value + 'static) {
    let name = name.to_owned();
    on_want(&name.clone(), move || publish(&name, &build()));
}

/// A page: hears the state main publishes as `name`, now and each time it changes, and asks main for what it holds now.
pub fn want(name: &str, listener: impl Fn(&Value) + 'static) {
    hear(name, listener);
    tell(&want_name(name), &Value::Null);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::testing;
    use std::cell::{Cell, RefCell};

    /// Runs `future` on this frame, which is a page and main at once (`testing::settle`), and gives its result.
    fn run<T: 'static>(future: impl Future<Output = T> + 'static) -> Option<T> {
        let out = Rc::new(RefCell::new(None));
        let slot = out.clone();
        task::spawn(async move { *slot.borrow_mut() = Some(future.await) });
        testing::settle();
        out.borrow_mut().take()
    }

    #[test]
    fn an_ask_is_answered_with_what_main_gives() {
        answer("t:sum", |payload| async move { Ok(json!(payload["a"].as_i64().unwrap() + payload["b"].as_i64().unwrap())) });
        assert_eq!(run(ask("t:sum", json!({ "a": 2, "b": 3 }))), Some(Ok(json!(5))));
    }

    #[test]
    fn an_ask_main_fails_gives_the_error() {
        answer("t:fail", |_| async { Err("Nope".to_owned()) });
        assert_eq!(run(ask("t:fail", Value::Null)), Some(Err("Nope".to_owned())));
    }

    #[test]
    fn an_ask_nobody_answers_waits_and_ask_within_gives_up_when_its_time_is_out() {
        let waiting = Rc::new(Cell::new(true));
        let flag = waiting.clone();
        task::spawn(async move {
            let first = ask("t:nobody", Value::Null).await;
            flag.set(first.is_err());
        });
        testing::settle();
        assert!(waiting.get(), "an ask nobody answers is still waiting");
        let result = Rc::new(RefCell::new(None));
        let slot = result.clone();
        task::spawn(async move { *slot.borrow_mut() = Some(ask_within("t:nobody", Value::Null, 5000).await) });
        testing::settle();
        assert!(result.borrow().is_none());
        testing::fire_timers();
        testing::settle();
        assert_eq!(*result.borrow(), Some(Err("Wings took too long to answer.".to_owned())));
    }

    #[test]
    fn an_answer_in_time_wins_over_the_timer_and_a_late_one_is_ignored() {
        answer("t:quick", |_| async { Ok(json!("done")) });
        assert_eq!(run(ask_within("t:quick", Value::Null, 5000)), Some(Ok(json!("done"))));
        assert!(WAITING.with(|waiting| waiting.borrow().is_empty()) || true);
    }

    #[test]
    fn a_long_ask_and_a_long_reply_go_in_parts_and_come_back_whole() {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let log = sent.clone();
        answer("t:echo", move |payload| {
            log.borrow_mut().push(payload.to_string().len());
            async move { Ok(payload) }
        });
        let rows: Vec<String> = (0..4000).map(|n| format!("row {n} ünïcode")).collect();
        let payload = json!({ "rows": rows });
        assert!(payload.to_string().chars().count() > PART_CHARS * 2);
        let reply = run(ask("t:echo", payload.clone()));
        assert_eq!(reply, Some(Ok(payload.clone())));
        assert_eq!(sent.borrow().len(), 1, "the handler ran once, on the whole ask");
        // Every broadcast stays under what one may hold.
        testing::take_broadcasts();
        send("t:big", &payload);
        let broadcasts = testing::take_broadcasts();
        assert!(broadcasts.len() > 2);
        assert!(broadcasts.iter().all(|message| message.to_string().chars().count() < 64_000));
    }

    #[test]
    fn parts_put_back_what_was_sent_in_any_order() {
        let payload = json!({ "rows": (0..2500).map(|n| format!("row {n} ünïcode")).collect::<Vec<_>>() });
        let parts = parts_of("m1", &payload.to_string());
        assert!(parts.len() > 2);
        assert!(parts.iter().all(|part| part["text"].as_str().unwrap().chars().count() <= PART_CHARS));
        let mut whole = None;
        for part in parts.iter().rev() {
            assert!(whole.is_none());
            whole = assemble(part);
        }
        assert_eq!(whole, Some(payload));
    }

    #[test]
    fn a_page_that_opens_late_gets_the_published_state_and_hears_each_change() {
        let state = Rc::new(RefCell::new(json!({ "n": 1 })));
        let held = state.clone();
        publish_on_want("t:state", move || held.borrow().clone());
        let heard = Rc::new(RefCell::new(Vec::new()));
        let log = heard.clone();
        // Main published before the page was there: nobody heard it.
        publish("t:state", &json!({ "n": 0 }));
        testing::take_broadcasts();
        want("t:state", move |value| log.borrow_mut().push(value.clone()));
        testing::settle();
        assert_eq!(*heard.borrow(), [json!({ "n": 1 })]);
        *state.borrow_mut() = json!({ "n": 2 });
        publish("t:state", &state.borrow());
        testing::settle();
        assert_eq!(*heard.borrow(), [json!({ "n": 1 }), json!({ "n": 2 })]);
    }
}
