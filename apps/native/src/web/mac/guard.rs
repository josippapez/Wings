//! The password guard in WebKit (its rules are `web::guard`'s): the check runs in Wings' own content world of the main
//! frame (`check`), and every frame runs the watch there as it finishes loading (`WATCH`), telling a handler only that
//! world reaches (`HANDLER`).

use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::{MainThreadMarker, runtime::AnyObject};
use objc2_foundation::{NSError, NSString};
use objc2_web_kit::{WKContentWorld, WKWebView};
use serde_json::Value;

pub(super) use crate::web::guard::{CHECK, Seen, WORLD, checked_again, masked, may_run, rects};

/// The script each frame runs as it finishes loading, in `WORLD`, which tells `HANDLER` once it holds a secret field.
pub(super) const WATCH: &str = concat!(include_str!("../guard.js"), include_str!("../watch.js"));
/// The handler only Wings' world reaches.
pub(super) const HANDLER: &str = "wingsGuard";

/// Runs the check in Wings' own world of `view`'s main frame, then `then` with what it found, or why it couldn't, and
/// how long it took.
pub(super) fn check(view: &WKWebView, mtm: MainThreadMarker, then: impl Fn(Result<Value, String>, Duration) + 'static) {
    let started = Instant::now();
    let done = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        // SAFETY: WebKit hands one of the two, alive for the call, on the main thread.
        let found = match unsafe { (value.as_ref(), error.as_ref()) } {
            (_, Some(error)) => Err(error.localizedDescription().to_string()),
            (Some(value), None) => super::json_of(value),
            (None, None) => Err("The check answered nothing".into()),
        };
        then(found, started.elapsed());
    });
    // SAFETY: Wings' own script in a world of its own, and a block WebKit calls once on the main thread.
    unsafe {
        let world = WKContentWorld::worldWithName(&NSString::from_str(WORLD), mtm);
        view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(&NSString::from_str(CHECK), None, None, &world, Some(&done));
    }
}

#[cfg(test)]
mod tests {
    /// WebKit's watch is the shared one, whole: the functions, then the watch.
    #[test]
    fn webkits_watch_is_the_shared_functions_and_watch() {
        assert_eq!(super::WATCH, format!("{}{}", crate::web::guard::FUNCTIONS, crate::web::guard::WATCH_FUNCTION));
    }
}
