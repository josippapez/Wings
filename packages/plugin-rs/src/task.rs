//! Tasks: futures that run on the page's one thread, between the host's turns.

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

type Task = Pin<Box<dyn Future<Output = ()>>>;

thread_local! {
    static TASKS: RefCell<HashMap<u32, Task>> = RefCell::new(HashMap::new());
    static READY: RefCell<VecDeque<u32>> = const { RefCell::new(VecDeque::new()) };
    static NEXT: RefCell<u32> = const { RefCell::new(1) };
}

struct Ready(u32);

impl Wake for Ready {
    fn wake(self: Arc<Self>) {
        READY.with(|ready| ready.borrow_mut().push_back(self.0));
    }
}

/// Starts `future`. It runs up to its first wait before the turn ends.
pub fn spawn(future: impl Future<Output = ()> + 'static) {
    let id = NEXT.with(|next| {
        let id = *next.borrow();
        *next.borrow_mut() = id.wrapping_add(1);
        id
    });
    TASKS.with(|tasks| tasks.borrow_mut().insert(id, Box::pin(future)));
    READY.with(|ready| ready.borrow_mut().push_back(id));
}

pub(crate) fn run_ready() {
    while let Some(id) = READY.with(|ready| ready.borrow_mut().pop_front()) {
        // Out of the table while it runs, so it can spawn.
        let Some(mut task) = TASKS.with(|tasks| tasks.borrow_mut().remove(&id)) else {
            continue;
        };
        let waker = Waker::from(Arc::new(Ready(id)));
        if task.as_mut().poll(&mut Context::from_waker(&waker)) == Poll::Pending {
            TASKS.with(|tasks| tasks.borrow_mut().insert(id, task));
        }
    }
}

/// Runs `futures` side by side and gives their results in order.
pub async fn join_all<T>(futures: Vec<Pin<Box<dyn Future<Output = T>>>>) -> Vec<T> {
    let mut futures: Vec<Option<Pin<Box<dyn Future<Output = T>>>>> = futures.into_iter().map(Some).collect();
    let mut results: Vec<Option<T>> = futures.iter().map(|_| None).collect();
    std::future::poll_fn(move |cx| {
        for (slot, result) in futures.iter_mut().zip(&mut results) {
            if let Some(future) = slot
                && let Poll::Ready(value) = future.as_mut().poll(cx)
            {
                *result = Some(value);
                *slot = None;
            }
        }
        if futures.iter().all(Option::is_none) { Poll::Ready(results.drain(..).flatten().collect()) } else { Poll::Pending }
    })
    .await
}
