//! CEF's message loop work, run from a host that owns the main run loop (GPUI in Wings), with
//! `external_message_pump` on instead of `cef_run_message_loop`.
//!
//! CEF asks for work through `OnScheduleMessagePumpWork(delay)`, from any thread. [`schedule`] moves the request to the
//! main thread, which moves a run loop timer's next fire date; the timer runs `cef_do_message_loop_work`. The timer is in
//! the run loop's common modes, so CEF keeps working while AppKit tracks a window resize or a menu. `fallback` makes
//! the timer also fire on its own at that interval, as cefclient's external pump does (1/30 s), in case a request is
//! missed.

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
static INTERVAL_MS: AtomicU64 = AtomicU64::new(0);
static IN_WORK: AtomicBool = AtomicBool::new(false);
static WORK_CALLS: AtomicU64 = AtomicU64::new(0);
static SCHEDULE_CALLS: AtomicU64 = AtomicU64::new(0);

/// Never on its own: a timer with this interval only fires when [`schedule`] moves it.
const NEVER: f64 = 1.0e9;

/// Starts the timer. Call it on the main thread before `cef::initialize`.
pub fn start(fallback: Option<Duration>) {
    let interval = fallback.map_or(NEVER, |interval| interval.as_secs_f64());
    INTERVAL_MS.store(fallback.map_or(u64::MAX, |interval| interval.as_millis() as u64), Ordering::Relaxed);
    unsafe {
        let timer = CFRunLoopTimerCreate(
            std::ptr::null(),
            CFAbsoluteTimeGetCurrent() + interval,
            interval,
            0,
            0,
            fire,
            std::ptr::null_mut(),
        );
        CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopCommonModes);
        TIMER.store(timer, Ordering::Release);
    }
}

/// `OnScheduleMessagePumpWork`: run CEF's work in `delay_ms` (now when 0 or less). Any thread.
pub fn schedule(delay_ms: i64) {
    SCHEDULE_CALLS.fetch_add(1, Ordering::Relaxed);
    let delay = delay_ms.max(0) as usize;
    unsafe { dispatch_async_f(&_dispatch_main_q, delay as *mut c_void, move_timer) };
}

/// Stops the timer and runs CEF's remaining work, after the host's run loop has ended and before `cef::shutdown`:
/// closing browsers leaves work behind, and `cef::shutdown` without it hangs until Chromium's teardown watchdog kills
/// the process. CEF can't say when it's idle, so this runs it "for a while", as cefclient's external pump does.
pub fn drain() {
    let timer = TIMER.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !timer.is_null() {
        unsafe { CFRunLoopTimerInvalidate(timer) };
    }
    for _ in 0..10 {
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.001, 1) };
        cef::do_message_loop_work();
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// How many times CEF's work ran and CEF asked for work, so far.
pub fn counts() -> (u64, u64) {
    (WORK_CALLS.load(Ordering::Relaxed), SCHEDULE_CALLS.load(Ordering::Relaxed))
}

extern "C" fn move_timer(context: *mut c_void) {
    let delay_ms = (context as usize as u64).min(INTERVAL_MS.load(Ordering::Relaxed));
    let timer = TIMER.load(Ordering::Acquire);
    if !timer.is_null() {
        unsafe { CFRunLoopTimerSetNextFireDate(timer, CFAbsoluteTimeGetCurrent() + delay_ms as f64 / 1000.0) };
    }
}

extern "C" fn fire(_timer: *mut c_void, _info: *mut c_void) {
    // CEF's work can spin a nested run loop (a menu, a dialog) that fires the timer again inside it.
    if IN_WORK.swap(true, Ordering::Relaxed) {
        return;
    }
    WORK_CALLS.fetch_add(1, Ordering::Relaxed);
    cef::do_message_loop_work();
    IN_WORK.store(false, Ordering::Relaxed);
}
