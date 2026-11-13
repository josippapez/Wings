//! Loads that are kept (`cached` of `wings-plugin-sdk/async`) and loads that are batched (`batched`).

use crate::{host, task::{spawn, wait_once}};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    hash::Hash,
    pin::Pin,
    rc::Rc,
    task::{Poll, Waker},
};

type Load<K, V> = Rc<dyn Fn(K) -> Pin<Box<dyn Future<Output = Result<V, String>>>>>;

/// A load that is running: its answer once it has one, and who waits for it.
struct Slot<V> {
    done: RefCell<Option<Result<V, String>>>,
    waiting: RefCell<Vec<Waker>>,
}

impl<V: Clone> Slot<V> {
    fn new() -> Rc<Self> {
        Rc::new(Self { done: RefCell::new(None), waiting: RefCell::new(Vec::new()) })
    }

    fn finish(&self, answer: Result<V, String>) {
        *self.done.borrow_mut() = Some(answer);
        self.waiting.borrow_mut().drain(..).for_each(Waker::wake);
    }

    async fn wait(&self) -> Result<V, String> {
        std::future::poll_fn(|cx| match &*self.done.borrow() {
            Some(answer) => Poll::Ready(answer.clone()),
            None => {
                wait_once(&mut self.waiting.borrow_mut(), cx.waker());
                Poll::Pending
            }
        })
        .await
    }
}

enum Entry<V> {
    Loading(Rc<Slot<V>>),
    /// The load finished at this time, with this answer.
    Done(f64, Result<V, String>),
}

struct Kept<K, V> {
    load: Load<K, V>,
    ttl_ms: f64,
    fail_ttl_ms: f64,
    entries: RefCell<HashMap<K, Entry<V>>>,
    /// Goes up when keys are forgotten, so a load that was running then doesn't keep its answer.
    forgotten: Cell<u64>,
}

/// A value loaded per key and kept, so the load runs once however many ask. A value is kept for `ttl_ms` after its
/// load finished. A failure isn't kept, unless `fail_ttl_ms` is set: then `get` fails the same way for that long without
/// loading. Asking while a load runs joins it.
pub struct Cached<K, V>(Rc<Kept<K, V>>);

impl<K, V> Clone for Cached<K, V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<K: Hash + Eq + Clone + 'static, V: Clone + 'static> Cached<K, V> {
    pub fn new<Fut>(ttl_ms: f64, fail_ttl_ms: f64, load: impl Fn(K) -> Fut + 'static) -> Self
    where
        Fut: Future<Output = Result<V, String>> + 'static,
    {
        Self(Rc::new(Kept {
            load: Rc::new(move |key| Box::pin(load(key))),
            ttl_ms,
            fail_ttl_ms,
            entries: RefCell::new(HashMap::new()),
            forgotten: Cell::new(0),
        }))
    }

    pub fn get(&self, key: K) -> impl Future<Output = Result<V, String>> + use<K, V> {
        let kept = self.0.clone();
        async move {
            let hit = match kept.entries.borrow().get(&key) {
                Some(Entry::Loading(slot)) => Some(Err(slot.clone())),
                Some(Entry::Done(at, answer)) if host::now() - at < if answer.is_err() { kept.fail_ttl_ms } else { kept.ttl_ms } => Some(Ok(answer.clone())),
                _ => None,
            };
            match hit {
                Some(Ok(answer)) => return answer,
                Some(Err(slot)) => return slot.wait().await,
                None => {}
            }
            let (slot, started) = (Slot::new(), kept.forgotten.get());
            kept.entries.borrow_mut().insert(key.clone(), Entry::Loading(slot.clone()));
            let answer = (kept.load)(key.clone()).await;
            if kept.forgotten.get() == started {
                let mut entries = kept.entries.borrow_mut();
                if answer.is_ok() || kept.fail_ttl_ms > 0.0 {
                    entries.insert(key, Entry::Done(host::now(), answer.clone()));
                } else {
                    entries.remove(&key);
                }
            }
            slot.finish(answer.clone());
            answer
        }
    }

    /// Forgets every key, loads that are running included.
    pub fn clear(&self) {
        self.0.forgotten.set(self.0.forgotten.get() + 1);
        self.0.entries.borrow_mut().clear();
    }
}

type LoadMany<K, V> = Rc<dyn Fn(Vec<K>) -> Pin<Box<dyn Future<Output = Result<Vec<V>, String>>>>>;

struct Waiting<K, V> {
    load_many: LoadMany<K, V>,
    max: usize,
    /// The `get`s that haven't gone out yet, one slot per key.
    queue: RefCell<Vec<(K, Rc<Slot<V>>)>>,
}

/// Turns many loads of one thing into one call of `load_many`: every `get` made before the tasks that are ready have
/// run goes out together, in calls of at most `max` keys. `load_many` gives one value per key, in the same order. One
/// that fails fails every `get` of its call. It keeps nothing: put a `Cached` over it for a TTL.
pub struct Batch<K, V>(Rc<Waiting<K, V>>);

impl<K, V> Clone for Batch<K, V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<K: Eq + Clone + 'static, V: Clone + 'static> Batch<K, V> {
    pub fn new<Fut>(max: usize, load_many: impl Fn(Vec<K>) -> Fut + 'static) -> Self
    where
        Fut: Future<Output = Result<Vec<V>, String>> + 'static,
    {
        Self(Rc::new(Waiting { load_many: Rc::new(move |keys| Box::pin(load_many(keys))), max: max.max(1), queue: RefCell::new(Vec::new()) }))
    }

    pub fn get(&self, key: K) -> impl Future<Output = Result<V, String>> + use<K, V> {
        let waiting = self.0.clone();
        let existing = waiting.queue.borrow().iter().find(|(queued, _)| *queued == key).map(|(_, slot)| slot.clone());
        let slot = existing.unwrap_or_else(|| {
            let slot = Slot::new();
            let first = waiting.queue.borrow().is_empty();
            waiting.queue.borrow_mut().push((key, slot.clone()));
            if first {
                let waiting = waiting.clone();
                spawn(async move {
                    let queued = std::mem::take(&mut *waiting.queue.borrow_mut());
                    for chunk in queued.chunks(waiting.max) {
                        let (keys, slots): (Vec<K>, Vec<Rc<Slot<V>>>) = chunk.iter().cloned().unzip();
                        let waiting = waiting.clone();
                        spawn(async move {
                            match (waiting.load_many)(keys).await {
                                Ok(values) if values.len() == slots.len() => slots.iter().zip(values).for_each(|(slot, value)| slot.finish(Ok(value))),
                                Ok(values) => slots.iter().for_each(|slot| slot.finish(Err(format!("The load gave {} values for {} keys", values.len(), slots.len())))),
                                Err(error) => slots.iter().for_each(|slot| slot.finish(Err(error.clone()))),
                            }
                        });
                    }
                });
            }
            slot
        });
        async move { slot.wait().await }
    }
}

impl<K: Hash + Eq + Clone + 'static, V: Clone + 'static> Cached<K, V> {
    /// What was kept for `key` whatever its age, or none when it was never loaded, is loading or was forgotten.
    pub fn peek(&self, key: &K) -> Option<Result<V, String>> {
        match self.0.entries.borrow().get(key) {
            Some(Entry::Done(_, answer)) => Some(answer.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{block_on, join_all};

    #[test]
    fn a_kept_load_runs_once_a_failure_is_asked_again_and_clear_forgets() {
        let runs = Rc::new(Cell::new(0));
        let counted = runs.clone();
        let cached: Cached<String, usize> = Cached::new(f64::INFINITY, 0.0, move |key: String| {
            counted.set(counted.get() + 1);
            async move { if key == "bad" { Err("no".to_owned()) } else { Ok(key.len()) } }
        });
        assert_eq!(block_on(cached.get("abc".into())), Ok(3));
        assert_eq!(block_on(cached.get("abc".into())), Ok(3));
        assert_eq!(runs.get(), 1);
        assert_eq!(block_on(cached.get("bad".into())), Err("no".into()));
        assert_eq!(block_on(cached.get("bad".into())), Err("no".into()));
        assert_eq!(runs.get(), 3);
        cached.clear();
        assert_eq!(block_on(cached.get("abc".into())), Ok(3));
        assert_eq!(runs.get(), 4);
    }

    #[test]
    fn a_failure_is_kept_for_the_fail_ttl() {
        let runs = Rc::new(Cell::new(0));
        let counted = runs.clone();
        let cached: Cached<u8, u8> = Cached::new(f64::INFINITY, 1000.0, move |_| {
            counted.set(counted.get() + 1);
            async { Err("down".to_owned()) }
        });
        // The test clock stands still, so the failure is still kept the second time.
        assert_eq!(block_on(cached.get(1)), Err("down".into()));
        assert_eq!(block_on(cached.get(1)), Err("down".into()));
        assert_eq!(runs.get(), 1);
    }

    #[test]
    fn gets_made_together_are_one_load_split_at_the_max_and_equal_keys_share_a_slot() {
        let calls: Rc<RefCell<Vec<Vec<u32>>>> = Rc::default();
        let seen = calls.clone();
        let batch: Batch<u32, u32> = Batch::new(2, move |keys: Vec<u32>| {
            seen.borrow_mut().push(keys.clone());
            async move { if keys.contains(&9) { Err("nine".to_owned()) } else { Ok(keys.iter().map(|key| key * 10).collect()) } }
        });
        let asked: Vec<Pin<Box<dyn Future<Output = Result<u32, String>>>>> = [1, 2, 2, 3].map(|key| Box::pin(batch.get(key)) as Pin<Box<dyn Future<Output = _>>>).into();
        assert_eq!(block_on(join_all(asked)), [Ok(10), Ok(20), Ok(20), Ok(30)]);
        assert_eq!(*calls.borrow(), [vec![1, 2], vec![3]]);
        assert_eq!(block_on(batch.get(9)), Err("nine".into()));
    }
}

impl<K: Hash + Eq + Clone + 'static, V: Clone + 'static> Cached<K, V> {
    /// Forgets one key, so the next `get` of it loads again.
    pub fn forget(&self, key: &K) {
        self.0.entries.borrow_mut().remove(key);
    }
}
