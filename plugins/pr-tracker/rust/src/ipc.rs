//! What the pages and the main script say to each other (`lib/messages.ts`'s channel, over `messages::tell`): a page
//! asks for what only main can do or what main already holds, and main answers. A message over what one broadcast can
//! carry goes in parts.

use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, future::Future, rc::Rc, task::{Poll, Waker}};
use wings_plugin::{host, messages, task::spawn};

/// A tab's or a block's ask of main for a project's open pull requests: `{ path }`, answered with the block's rows.
pub const PULLS: &str = "pulls";
/// The PR's status, which a tab tells main it loaded (`{ url, status }`), so main keeps the pane's badge on it fresh
/// without loading it again.
pub const PR_STATUS: &str = "pr-status";
/// A tab asks for what only main can do: `{ op: "diff" | "agent" | "seen" | "commit", args, focus?, commit? }`.
pub const PR: &str = "pr";
/// A block's or a tab's button: what the pane badge's Sign in, Add token or Install does for this project's host (`{ path }`).
pub const PULLS_ACTION: &str = "pulls-action";
/// Settings saved or removed a token, which main asks again with.
pub const TOKENS_CHANGED: &str = "tokens-changed";
/// Main loaded the hosts again after a sign-in or Try again: the pages load what they show again.
pub const REFRESH: &str = "refresh";

/// Wings refuses a broadcast whose JSON is longer than 64,000 characters, so a part holds well under that.
const PART_CHARS: usize = 20_000;
const PART: &str = "\u{1}part";

type Slot = Rc<RefCell<(Option<Result<Value, String>>, Option<Waker>)>>;

thread_local! {
    static WAITING: RefCell<HashMap<String, Slot>> = RefCell::new(HashMap::new());
    static PARTS: RefCell<HashMap<String, Vec<Option<String>>>> = RefCell::new(HashMap::new());
    static ANSWERED: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    static COUNT: RefCell<u64> = const { RefCell::new(0) };
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
        format!("{}:{}", host::now() as u64, count.borrow())
    })
}

/// Tells the plugin's other frames `name` with `payload`, in parts when its JSON is long.
pub fn send(name: &str, payload: &Value) {
    let text = payload.to_string();
    if text.chars().count() <= PART_CHARS {
        return messages::tell(name, payload);
    }
    let part_name = format!("{name}{PART}");
    for part in parts_of(&next_id(), &text) {
        messages::tell(&part_name, &part);
    }
}

/// Hears each `name` another frame sends, whole or in parts.
pub fn hear(name: &str, listener: impl Fn(&Value) + 'static) {
    let listener = Rc::new(listener);
    let whole = listener.clone();
    messages::on(name, move |payload| whole(payload));
    messages::on(&format!("{name}{PART}"), move |part| {
        if let Some(message) = assemble(part) {
            listener(&message);
        }
    });
}

fn answer_name(name: &str) -> String {
    format!("{name}:answer")
}

/// Asks main `name` with `payload` and waits for its answer, which only a main that `answer`s `name` gives.
pub fn ask(name: &'static str, payload: Value) -> impl Future<Output = Result<Value, String>> {
    let id = next_id();
    let slot: Slot = Rc::new(RefCell::new((None, None)));
    WAITING.with(|waiting| waiting.borrow_mut().insert(id.clone(), slot.clone()));
    if !ANSWERED.with(|names| names.borrow().contains(&name)) {
        ANSWERED.with(|names| names.borrow_mut().push(name));
        hear(&answer_name(name), |message| {
            let Some(id) = message["id"].as_str() else { return };
            let Some(slot) = WAITING.with(|waiting| waiting.borrow_mut().remove(id)) else { return };
            let answer = match message["error"].as_str() {
                Some(error) => Err(error.to_owned()),
                None => Ok(message["result"].clone()),
            };
            let waker = {
                let mut slot = slot.borrow_mut();
                slot.0 = Some(answer);
                slot.1.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        });
    }
    send(name, &json!({ "id": id, "payload": payload }));
    std::future::poll_fn(move |cx| {
        let mut slot = slot.borrow_mut();
        match slot.0.take() {
            Some(answer) => Poll::Ready(answer),
            None => {
                slot.1 = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    })
}

/// Answers each `name` a page asks with what `handler` gives for its payload.
pub fn answer<F, Fut>(name: &'static str, handler: F)
where
    F: Fn(Value) -> Fut + 'static,
    Fut: Future<Output = Result<Value, String>> + 'static,
{
    let handler = Rc::new(handler);
    hear(name, move |message| {
        let (Some(id), payload) = (message["id"].as_str().map(str::to_owned), message["payload"].clone()) else { return };
        let handler = handler.clone();
        spawn(async move {
            let reply = match handler(payload).await {
                Ok(result) => json!({ "id": id, "result": result }),
                Err(error) => json!({ "id": id, "error": error }),
            };
            send(&answer_name(name), &reply);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_message_goes_in_parts_that_put_back_what_was_sent_in_any_order() {
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
}
