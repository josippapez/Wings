//! A page asking main to do something, and being told once it is done. The SDK's messages are tells, so the question goes
//! out as `ask` with an id and main answers with `answer` and the same id, and the error when it failed.

use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, future::Future, pin::Pin, rc::Rc, task::{Poll, Waker}};
use wings_plugin::{host, messages, task::spawn};

/// A broadcast takes 64 KB; what goes in one with its envelope is kept under that.
const MOST_BYTES: usize = 60_000;

struct Wait {
    answer: Option<Result<(), String>>,
    waker: Option<Waker>,
}

thread_local! {
    static PENDING: RefCell<HashMap<String, Rc<RefCell<Wait>>>> = RefCell::new(HashMap::new());
    static NEXT: RefCell<u32> = const { RefCell::new(1) };
}

/// Asks main to do `what` with `payload`, and waits for its answer.
pub async fn ask(what: &str, payload: Value) -> Result<(), String> {
    let id = NEXT.with(|next| {
        let id = *next.borrow();
        *next.borrow_mut() += 1;
        format!("{}-{id}", host::now() as u64)
    });
    let message = json!({ "id": id, "what": what, "payload": payload });
    if message.to_string().len() > MOST_BYTES {
        return Err("That is too long to send at once. Cut it down and try again.".into());
    }
    let wait = Rc::new(RefCell::new(Wait { answer: None, waker: None }));
    let first = PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        pending.insert(id.clone(), wait.clone());
        pending.len() == 1
    });
    if first {
        listen();
    }
    messages::tell("ask", &message);
    std::future::poll_fn(|cx| {
        let mut wait = wait.borrow_mut();
        match wait.answer.take() {
            Some(answer) => Poll::Ready(answer),
            None => {
                wait.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    })
    .await
}

/// Hears main's answers. Registered once: a second listener would only hear the same answers twice.
fn listen() {
    thread_local! {
        static LISTENING: RefCell<bool> = const { RefCell::new(false) };
    }
    if LISTENING.with(|listening| listening.replace(true)) {
        return;
    }
    messages::on("answer", |message| {
        let Some(wait) = PENDING.with(|pending| pending.borrow_mut().remove(message["id"].as_str().unwrap_or_default())) else {
            return;
        };
        let answer = match message["error"].as_str() {
            Some(error) => Err(error.to_owned()),
            None => Ok(()),
        };
        let mut wait = wait.borrow_mut();
        wait.answer = Some(answer);
        if let Some(waker) = wait.waker.take() {
            waker.wake();
        }
    });
}

type Handled = Pin<Box<dyn Future<Output = Result<(), String>>>>;

/// Main: runs `handler` for each question a page asks, and answers with what it says.
pub fn serve(handler: impl Fn(String, Value) -> Handled + 'static) {
    messages::on("ask", move |message| {
        let (id, what, payload) = (message["id"].clone(), message["what"].as_str().unwrap_or_default().to_owned(), message["payload"].clone());
        let done = handler(what, payload);
        spawn(async move {
            let answer = match done.await {
                Ok(()) => json!({ "id": id }),
                Err(error) => json!({ "id": id, "error": error }),
            };
            messages::tell("answer", &answer);
        });
    });
}
