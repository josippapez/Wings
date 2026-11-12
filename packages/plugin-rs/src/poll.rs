//! A timer that runs only while the user is in Wings: `wings-plugin-sdk/poll`.

use crate::{host, task};
use std::{cell::Cell, rc::Rc};

struct Poll {
    tick: Box<dyn Fn()>,
    ms: u32,
    /// When `tick` last ran, in ms since the epoch.
    last: Cell<f64>,
    focused: Cell<bool>,
    /// Which run of the timer is the live one: a run that finds another number here has been stopped.
    run: Cell<u64>,
}

impl Poll {
    fn tick(&self) {
        self.last.set(host::now());
        (self.tick)();
    }

    /// Ticks after `wait` ms and then every `ms`, until the timer is stopped or started again.
    fn start(self: &Rc<Self>, wait: u32) {
        let run = self.run.get() + 1;
        self.run.set(run);
        let poll = self.clone();
        task::spawn(async move {
            host::sleep(wait).await;
            while poll.run.get() == run {
                poll.tick();
                host::sleep(poll.ms).await;
            }
        });
    }
}

/// Calls `tick` every `ms` while Wings is focused. When Wings loses focus the timer stops. When it regains focus,
/// `tick` runs at once if `ms` has passed since the last one, so flipping between apps doesn't fetch on every flip;
/// otherwise the timer resumes for the time that was left. `tick` is not called for the state Wings starts in, so a
/// plugin keeps doing its own first check, unless `immediate` is set: then it also runs once now, when Wings starts
/// focused.
pub fn while_focused(ms: u32, immediate: bool, tick: impl Fn() + 'static) {
    let focused = host::focused();
    let poll = Rc::new(Poll {
        tick: Box::new(tick),
        ms,
        // An unfocused start has not ticked, so its first focus ticks.
        last: Cell::new(if focused { host::now() } else { f64::NEG_INFINITY }),
        focused: Cell::new(focused),
        run: Cell::new(0),
    });
    if focused {
        poll.start(ms);
        if immediate {
            (poll.tick)();
        }
    }
    host::on_focus(move |focused| {
        if poll.focused.replace(focused) == focused {
            return;
        }
        if !focused {
            // Stops the run that is waiting.
            return poll.run.set(poll.run.get() + 1);
        }
        let left = f64::from(poll.ms) - (host::now() - poll.last.get());
        if left <= 0.0 {
            poll.tick();
            poll.start(poll.ms);
        } else {
            poll.start(left as u32);
        }
    });
}
