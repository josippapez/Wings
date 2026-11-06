//! `setTimeout`, `setInterval` and their clears, on real deadlines from the frame thread's loop.

use super::message;
use rquickjs::{
    Ctx, Exception, Function, Object, Persistent, Value,
    function::{Opt, Rest},
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

const MAX_TIMERS: usize = 1024;
/// A day: a longer delay is the same to a page that is never left open that long, and it keeps the sum in range.
const MAX_DELAY_MS: f64 = 86_400_000.0;
/// An interval can't be tighter than this, so one that asks for 0 is a busy loop of turns no more.
const MIN_INTERVAL_MS: f64 = 1.0;

struct Timer {
    id: u32,
    due: Instant,
    /// Set for an interval: it is due again this long after each run, until it is cleared.
    every: Option<Duration>,
    callback: Persistent<Function<'static>>,
    args: Vec<Persistent<Value<'static>>>,
}

/// A timer that is due: what to call, and with what.
pub(super) struct Due {
    pub callback: Persistent<Function<'static>>,
    pub args: Vec<Persistent<Value<'static>>>,
}

#[derive(Default)]
pub(super) struct Timers {
    last_id: u32,
    pending: Vec<Timer>,
}

impl Timers {
    pub fn next_due(&self) -> Option<Instant> {
        self.pending.iter().map(|timer| timer.due).min()
    }

    /// The timer that is due first, if it is due at `now`. A timeout is taken out; an interval stays, due again one
    /// period after `now`, so a callback that clears it still can.
    pub fn take_due(&mut self, now: Instant) -> Option<Due> {
        let at = self
            .pending
            .iter()
            .enumerate()
            .filter(|(_, timer)| timer.due <= now)
            .min_by_key(|(_, timer)| (timer.due, timer.id))?
            .0;
        let Some(every) = self.pending[at].every else {
            let timer = self.pending.remove(at);
            return Some(Due {
                callback: timer.callback,
                args: timer.args,
            });
        };
        let timer = &mut self.pending[at];
        timer.due = now + every;
        Some(Due {
            callback: timer.callback.clone(),
            args: timer.args.clone(),
        })
    }

    /// Persistent values left in a timer would keep the engine from freeing what they hold.
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

/// Adds the timer functions to `native`, for the setup script to hand to the frame: `setTimeout` and `clearTimeout`,
/// and with `intervals` `setInterval` and `clearInterval` too.
pub(super) fn install<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    timers: &Rc<RefCell<Timers>>,
    intervals: bool,
) -> Result<(), String> {
    native
        .set("setTimeout", setter(ctx, timers, false).map_err(message)?)
        .map_err(message)?;
    let clear = Function::new(ctx.clone(), {
        let timers = timers.clone();
        move |id: Opt<Value<'js>>| {
            if let Some(id) = id.0.and_then(|id| id.as_number()) {
                timers
                    .borrow_mut()
                    .pending
                    .retain(|timer| f64::from(timer.id) != id);
            }
        }
    })
    .map_err(message)?;
    native.set("clearTimeout", clear.clone()).map_err(message)?;
    if intervals {
        native
            .set("setInterval", setter(ctx, timers, true).map_err(message)?)
            .map_err(message)?;
        native.set("clearInterval", clear).map_err(message)?;
    }
    Ok(())
}

fn setter<'js>(
    ctx: &Ctx<'js>,
    timers: &Rc<RefCell<Timers>>,
    repeat: bool,
) -> rquickjs::Result<Function<'js>> {
    let timers = timers.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>,
              callback: Function<'js>,
              delay: Opt<Value<'js>>,
              args: Rest<Value<'js>>| {
            let delay = delay
                .0
                .and_then(|delay| delay.as_number())
                .filter(|delay| delay.is_finite())
                .unwrap_or(0.0)
                .clamp(if repeat { MIN_INTERVAL_MS } else { 0.0 }, MAX_DELAY_MS);
            let delay = Duration::from_secs_f64(delay / 1000.0);
            let mut timers = timers.borrow_mut();
            if timers.pending.len() >= MAX_TIMERS {
                return Err(Exception::throw_range(&ctx, "too many timers"));
            }
            timers.last_id += 1;
            let id = timers.last_id;
            timers.pending.push(Timer {
                id,
                due: Instant::now() + delay,
                every: repeat.then_some(delay),
                callback: Persistent::save(&ctx, callback),
                args: args
                    .0
                    .into_iter()
                    .map(|arg| Persistent::save(&ctx, arg))
                    .collect(),
            });
            Ok(id)
        },
    )
}
