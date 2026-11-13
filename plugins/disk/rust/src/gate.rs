//! Something to wait for that happens once, like a scan showing as scanning or ending.

use std::{cell::RefCell, future::Future, rc::Rc, task::{Poll, Waker}};

#[derive(Clone, Default)]
pub struct Gate(Rc<RefCell<(bool, Vec<Waker>)>>);

impl Gate {
    pub fn open(&self) {
        let waiting = {
            let mut gate = self.0.borrow_mut();
            gate.0 = true;
            std::mem::take(&mut gate.1)
        };
        waiting.into_iter().for_each(Waker::wake);
    }

    /// Ready once the gate is open, now if it is.
    pub fn wait(&self) -> impl Future<Output = ()> + use<> {
        let gate = self.0.clone();
        std::future::poll_fn(move |cx| {
            let mut gate = gate.borrow_mut();
            if gate.0 {
                Poll::Ready(())
            } else {
                gate.1.push(cx.waker().clone());
                Poll::Pending
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wings_plugin::task::{block_on, spawn};

    #[test]
    fn what_waits_for_a_gate_goes_on_once_it_opens_and_never_waits_for_one_that_is_open() {
        let gate = Gate::default();
        let seen = Rc::new(RefCell::new(Vec::new()));
        for n in 1..=2 {
            let (gate, seen) = (gate.clone(), seen.clone());
            spawn(async move {
                gate.wait().await;
                seen.borrow_mut().push(n);
            });
        }
        block_on(async {});
        assert!(seen.borrow().is_empty());
        let opening = gate.clone();
        block_on(async move { opening.open() });
        assert_eq!(*seen.borrow(), [1, 2]);
        block_on(gate.wait());
    }
}
