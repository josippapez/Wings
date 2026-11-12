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
    /// Each task with its waker, which is one for the task's life: a wait that is polled again can tell it already
    /// holds it (`Waker::will_wake`).
    static TASKS: RefCell<HashMap<u32, (Task, Waker)>> = RefCell::new(HashMap::new());
    /// The tasks to run, each once however many times it was woken.
    static READY: RefCell<VecDeque<u32>> = const { RefCell::new(VecDeque::new()) };
    static NEXT: RefCell<u32> = const { RefCell::new(1) };
}

struct Ready(u32);

impl Wake for Ready {
    fn wake(self: Arc<Self>) {
        READY.with(|ready| {
            let mut ready = ready.borrow_mut();
            if !ready.contains(&self.0) {
                ready.push_back(self.0);
            }
        });
    }
}

/// Keeps `waker` among `waiting` unless it is there already: a wait is polled every time its task runs, whatever woke
/// the task, and a waker kept each time made the list, and the wakes when it ended, grow with every poll.
pub(crate) fn wait_once(waiting: &mut Vec<Waker>, waker: &Waker) {
    if !waiting.iter().any(|kept| kept.will_wake(waker)) {
        waiting.push(waker.clone());
    }
}

/// Starts `future`. It runs up to its first wait before the turn ends.
pub fn spawn(future: impl Future<Output = ()> + 'static) {
    let id = NEXT.with(|next| {
        let id = *next.borrow();
        *next.borrow_mut() = id.wrapping_add(1);
        id
    });
    TASKS.with(|tasks| tasks.borrow_mut().insert(id, (Box::pin(future), Waker::from(Arc::new(Ready(id))))));
    READY.with(|ready| ready.borrow_mut().push_back(id));
}

pub(crate) fn run_ready() {
    while let Some(id) = READY.with(|ready| ready.borrow_mut().pop_front()) {
        // Out of the table while it runs, so it can spawn.
        let Some((mut task, waker)) = TASKS.with(|tasks| tasks.borrow_mut().remove(&id)) else {
            continue;
        };
        if task.as_mut().poll(&mut Context::from_waker(&waker)) == Poll::Pending {
            TASKS.with(|tasks| tasks.borrow_mut().insert(id, (task, waker)));
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
                wait_once(&mut flight.waiting.borrow_mut(), cx.waker());
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

/// Runs `a` and `b` side by side and gives both results.
pub async fn join<A, B>(a: impl Future<Output = A>, b: impl Future<Output = B>) -> (A, B) {
    let (mut a, mut b) = (Box::pin(a), Box::pin(b));
    let (mut left, mut right) = (None, None);
    std::future::poll_fn(move |cx| {
        if left.is_none()
            && let Poll::Ready(value) = a.as_mut().poll(cx)
        {
            left = Some(value);
        }
        if right.is_none()
            && let Poll::Ready(value) = b.as_mut().poll(cx)
        {
            right = Some(value);
        }
        if left.is_some() && right.is_some() { Poll::Ready((left.take().unwrap(), right.take().unwrap())) } else { Poll::Pending }
    })
    .await
}

/// Runs `run` over `items`, at most `limit` at a time, and gives the results in the order of `items`.
pub async fn map_limit<I, T, Fut>(items: Vec<I>, limit: usize, run: impl Fn(I) -> Fut) -> Vec<T>
where
    Fut: Future<Output = T>,
{
    let mut results: Vec<Option<T>> = items.iter().map(|_| None).collect();
    let mut waiting = items.into_iter().enumerate();
    let mut running: Vec<(usize, Pin<Box<Fut>>)> = Vec::new();
    std::future::poll_fn(move |cx| {
        loop {
            while running.len() < limit.max(1)
                && let Some((index, item)) = waiting.next()
            {
                running.push((index, Box::pin(run(item))));
            }
            let before = running.len();
            running.retain_mut(|(index, future)| match future.as_mut().poll(cx) {
                Poll::Ready(value) => {
                    results[*index] = Some(value);
                    false
                }
                Poll::Pending => true,
            });
            if running.is_empty() && waiting.len() == 0 {
                return Poll::Ready(results.drain(..).flatten().collect());
            }
            // One that finished made room: start the next before waiting.
            if running.len() == before || waiting.len() == 0 {
                return Poll::Pending;
            }
        }
    })
    .await
}

/// Runs `future` to its end, for a plugin's own tests. Panics when it waits for something that never comes.
#[cfg(not(target_arch = "wasm32"))]
pub fn block_on<T: 'static>(future: impl Future<Output = T> + 'static) -> T {
    let done = Rc::new(RefCell::new(None));
    let slot = done.clone();
    spawn(async move { *slot.borrow_mut() = Some(future.await) });
    run_ready();
    done.borrow_mut().take().expect("the future waits for something that never comes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_woken_many_times_runs_once_and_a_wait_keeps_its_waker_once() {
        let (polls, waiting) = (Rc::new(Cell::new(0)), Rc::new(RefCell::new(Vec::<Waker>::new())));
        let (count, held) = (polls.clone(), waiting.clone());
        spawn(std::future::poll_fn(move |cx| {
            count.set(count.get() + 1);
            wait_once(&mut held.borrow_mut(), cx.waker());
            if count.get() < 3 { Poll::Pending } else { Poll::Ready(()) }
        }));
        run_ready();
        for _ in 0..2 {
            let wakers = waiting.borrow().clone();
            assert_eq!(wakers.len(), 1);
            // Woken three times before its turn: it runs once.
            wakers.iter().chain(&wakers).chain(&wakers).for_each(Waker::wake_by_ref);
            run_ready();
        }
        assert_eq!(polls.get(), 3);
    }

    #[test]
    fn map_limit_keeps_the_order_and_never_runs_more_than_the_limit() {
        let (running, most) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let (r, m) = (running.clone(), most.clone());
        let out = block_on(map_limit((0..7).collect(), 3, move |n: u32| {
            let (r, m) = (r.clone(), m.clone());
            async move {
                r.set(r.get() + 1);
                m.set(m.get().max(r.get()));
                // One turn of waiting, so the others get to start.
                let mut waited = false;
                std::future::poll_fn(|cx| {
                    if std::mem::replace(&mut waited, true) {
                        Poll::Ready(())
                    } else {
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                })
                .await;
                r.set(r.get() - 1);
                n * 2
            }
        }));
        assert_eq!(out, [0, 2, 4, 6, 8, 10, 12]);
        assert_eq!(most.get(), 3);
        assert!(block_on(map_limit(Vec::<u32>::new(), 3, |n| async move { n })).is_empty());
    }
}
