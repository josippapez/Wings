//! Tasks: futures that run on the page's one thread, between the host's turns.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    rc::Rc,
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

type Run = Box<dyn Fn() -> Pin<Box<dyn Future<Output = ()>>>>;

struct Flight {
    run: Run,
    running: Cell<bool>,
    again: Cell<bool>,
    waiting: RefCell<Vec<Waker>>,
}

/// `wings-plugin-sdk/async`'s `singleFlight`: a function made to run once at a time. Calling it while it runs doesn't
/// start a second run: it makes it run once more when this one is done, however many calls came meanwhile.
#[derive(Clone)]
pub struct SingleFlight(Rc<Flight>);

impl SingleFlight {
    pub fn new<F, Fut>(run: F) -> Self
    where
        F: Fn() -> Fut + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        Self(Rc::new(Flight { run: Box::new(move || Box::pin(run())), running: Cell::new(false), again: Cell::new(false), waiting: RefCell::new(Vec::new()) }))
    }

    /// Starts a run now, or has the one that is running go once more. What it returns is done when that run, and any
    /// rerun, is, and can be let go of by a caller that doesn't wait.
    pub fn call(&self) -> impl Future<Output = ()> + use<> {
        let flight = self.0.clone();
        if flight.running.replace(true) {
            flight.again.set(true);
        } else {
            let flight = flight.clone();
            spawn(async move {
                loop {
                    flight.again.set(false);
                    (flight.run)().await;
                    if !flight.again.get() {
                        break;
                    }
                }
                flight.running.set(false);
                for waker in flight.waiting.borrow_mut().drain(..) {
                    waker.wake();
                }
            });
        }
        std::future::poll_fn(move |cx| {
            if flight.running.get() {
                flight.waiting.borrow_mut().push(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
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
