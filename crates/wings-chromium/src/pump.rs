//! CEF's message loop work, run from a host that owns the main run loop (GPUI in Wings), with
//! `external_message_pump` on instead of `cef_run_message_loop`.
//!
//! CEF asks for work through `OnScheduleMessagePumpWork(delay)`, from any thread. [`schedule`] moves the request to the
//! main thread, which moves a run loop timer's next fire date; the timer runs `cef_do_message_loop_work`. The timer is in
//! the run loop's common modes, so CEF keeps working while AppKit tracks a window resize or a menu.
//!
//! CEF also needs its work run on its own every so often while it has pages: without that it stalled after one round
//! in the spike, and cefclient's external pump does the same at 1/30 s. That fallback ([`set_fallback`]) runs only while
//! the host has pages or popups open, so an idle CEF with none costs nothing but the work it asks for itself.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::time::Duration;

#[repr(C)]
struct TimerContext {
    version: isize,
    info: *mut c_void,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopCommonModes: *const c_void;
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source_handled: u8) -> i32;
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFAbsoluteTimeGetCurrent() -> f64;
    fn CFRunLoopTimerCreate(
        allocator: *const c_void,
        fire_date: f64,
        interval: f64,
        flags: u64,
        order: isize,
        callout: extern "C" fn(*mut c_void, *mut c_void),
        context: *mut TimerContext,
    ) -> *mut c_void;
    fn CFRunLoopAddTimer(run_loop: *mut c_void, timer: *mut c_void, mode: *const c_void);
    fn CFRunLoopTimerSetNextFireDate(timer: *mut c_void, fire_date: f64);
    fn CFRunLoopTimerInvalidate(timer: *mut c_void);
}

extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

static TIMER: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static FALLBACK: AtomicBool = AtomicBool::new(false);
static IN_WORK: AtomicBool = AtomicBool::new(false);
static WORK_CALLS: AtomicU64 = AtomicU64::new(0);
static SCHEDULE_CALLS: AtomicU64 = AtomicU64::new(0);

/// How often CEF's work runs on its own while the fallback is on, and the longest a request waits then: cefclient's
/// `kMaxTimerDelay`.
pub const FALLBACK_EVERY: Duration = Duration::from_millis(1000 / 30);

/// Never on its own: a timer with this interval only fires when it is moved.
const NEVER: f64 = 1.0e9;

/// Makes the timer, which fires only when CEF asks for work until [`set_fallback`] turns the fallback on. Call it on the
/// main thread before `cef::initialize`.
pub fn start() {
    if !TIMER.load(Ordering::Acquire).is_null() {
        return;
    }
    unsafe {
        let timer = CFRunLoopTimerCreate(std::ptr::null(), CFAbsoluteTimeGetCurrent() + NEVER, NEVER, 0, 0, fire, std::ptr::null_mut());
        CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopCommonModes);
        TIMER.store(timer, Ordering::Release);
    }
}

/// Whether CEF's work also runs every [`FALLBACK_EVERY`] on its own: on while the host shows a page or a popup, off
/// otherwise. Main thread.
pub fn set_fallback(on: bool) {
    if FALLBACK.swap(on, Ordering::Relaxed) == on {
        return;
    }
    if on {
        move_to(FALLBACK_EVERY.as_secs_f64());
    }
}

/// Whether the fallback is on.
pub fn fallback() -> bool {
    FALLBACK.load(Ordering::Relaxed)
}

/// `OnScheduleMessagePumpWork`: run CEF's work in `delay_ms` (now when 0 or less). Any thread.
pub fn schedule(delay_ms: i64) {
    SCHEDULE_CALLS.fetch_add(1, Ordering::Relaxed);
    let delay = delay_ms.max(0) as usize;
    unsafe { dispatch_async_f(&_dispatch_main_q, delay as *mut c_void, move_timer) };
}

/// Stops the timer and runs CEF's remaining work for `rounds` rounds of about 50 ms, after the browsers have closed
/// and before `cef::shutdown`: closing browsers leaves work behind, and `cef::shutdown` without it hangs until
/// Chromium's teardown watchdog kills the process. CEF can't say when it's idle, so this runs it "for a while", as
/// cefclient's external pump does.
pub fn drain(rounds: u32) {
    let timer = TIMER.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !timer.is_null() {
        unsafe { CFRunLoopTimerInvalidate(timer) };
    }
    for _ in 0..rounds {
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.001, 1) };
        cef::do_message_loop_work();
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Runs CEF's work once now, from the host, while it waits for something CEF does (browsers closing at quit).
pub fn work_now() {
    if !IN_WORK.swap(true, Ordering::Relaxed) {
        WORK_CALLS.fetch_add(1, Ordering::Relaxed);
        cef::do_message_loop_work();
        IN_WORK.store(false, Ordering::Relaxed);
    }
}

/// How many times CEF's work ran and CEF asked for work, so far.
pub fn counts() -> (u64, u64) {
    (WORK_CALLS.load(Ordering::Relaxed), SCHEDULE_CALLS.load(Ordering::Relaxed))
}

/// When the timer fires next after a request for work in `delay_ms`: then, but no later than the fallback's next round
/// while it is on.
pub fn next_delay_ms(delay_ms: u64, fallback: bool) -> u64 {
    if fallback {
        delay_ms.min(FALLBACK_EVERY.as_millis() as u64)
    } else {
        delay_ms
    }
}

fn move_to(seconds_from_now: f64) {
    let timer = TIMER.load(Ordering::Acquire);
    if !timer.is_null() {
        unsafe { CFRunLoopTimerSetNextFireDate(timer, CFAbsoluteTimeGetCurrent() + seconds_from_now) };
    }
}

extern "C" fn move_timer(context: *mut c_void) {
    let delay_ms = next_delay_ms(context as usize as u64, fallback());
    move_to(delay_ms as f64 / 1000.0);
}

extern "C" fn fire(_timer: *mut c_void, _info: *mut c_void) {
    // CEF's work can spin a nested run loop (a menu, a dialog) that fires the timer again inside it.
    if IN_WORK.swap(true, Ordering::Relaxed) {
        return;
    }
    WORK_CALLS.fetch_add(1, Ordering::Relaxed);
    // Before the work: a request it makes moves the timer again, earlier.
    move_to(if fallback() { FALLBACK_EVERY.as_secs_f64() } else { NEVER });
    cef::do_message_loop_work();
    IN_WORK.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With a page open CEF's request waits no longer than the fallback's round; with none it waits as long as it asks,
    /// so an idle CEF wakes Wings only when it has work.
    #[test]
    fn a_request_waits_at_most_a_fallback_round_only_while_the_fallback_is_on() {
        assert_eq!(next_delay_ms(5, true), 5);
        assert_eq!(next_delay_ms(15_000, true), 33);
        assert_eq!(next_delay_ms(15_000, false), 15_000);
        assert_eq!(next_delay_ms(0, false), 0);
    }
}
