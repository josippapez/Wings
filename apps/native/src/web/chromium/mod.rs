//! The web views as Chromium draws them, for a profile that chose it (`web::engine`): each page is a frameless
//! Chrome-style CEF window over its place in the Wings window, made, placed and hidden by `views` as a WebKit view is.
//! Chrome extensions only run in Chrome style, which CEF can't put inside another view on macOS.
//!
//! - [`page`]: one web view, its browser and what it tells its page.
//! - [`devtools`] and [`agent`]: the page's own DevTools channel, and a plugin's scripts, pictures and logs over it.
//! - [`runtime`]: starting CEF for the first page, its work, and closing it as Wings quits.
//! - [`extensions`]: the profile's extensions, their buttons and popups.
//! - [`cookies`]: another browser's sign-ins imported into the profile.
//! - [`focus`] and [`keys`]: the keyboard between Wings and a page.

mod agent;
pub mod cookies;
#[cfg(debug_assertions)]
mod debug;
mod devtools;
pub mod extensions;
mod focus;
pub mod keys;
pub mod page;
pub mod runtime;

pub use page::Chromium;

/// Runs `then` on the main thread's next turn, from the main thread: for what AppKit has to settle first (which window is
/// key) and for objects that never leave the main thread, which `DispatchQueue::exec_async` won't carry.
pub(crate) fn next_turn(then: impl FnOnce() + 'static) {
    extern "C" fn run(context: *mut std::ffi::c_void) {
        // SAFETY: the box `next_turn` leaked, run once on the main queue.
        let then = unsafe { Box::from_raw(context.cast::<Box<dyn FnOnce()>>()) };
        then();
    }
    unsafe extern "C" {
        static _dispatch_main_q: std::ffi::c_void;
        fn dispatch_async_f(queue: *const std::ffi::c_void, context: *mut std::ffi::c_void, work: extern "C" fn(*mut std::ffi::c_void));
    }
    debug_assert!(objc2::MainThreadMarker::new().is_some(), "next_turn is for the main thread's own work");
    let then: Box<Box<dyn FnOnce()>> = Box::new(Box::new(then));
    // SAFETY: the main queue, and a context `run` frees.
    unsafe { dispatch_async_f(&raw const _dispatch_main_q, Box::into_raw(then).cast(), run) };
}

#[cfg(test)]
mod check {
    /// Readies a test copy of Wings, whose `HOME` this test runs with, by the same code the Browser's settings run: the
    /// engine from `WINGS_CHROMIUM_ARCHIVE` (copied, checked against the pinned size and hash, unpacked, its signature
    /// checked), Chromium chosen for the profile `WINGS_CHECK_PROFILE` (`<plugin>.<scope>`, its web folder
    /// `WINGS_CHECK_WEB`), and the extension folder `WINGS_CHECK_EXTENSION` added to it as if allowed. Run with
    /// `--ignored ready_a_test_copy`.
    #[test]
    #[ignore]
    fn ready_a_test_copy() {
        use crate::web::{Profile, engine, extensions};
        let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
        assert!(var("HOME").starts_with("/tmp/"), "a test copy's own HOME, never yours");
        engine::download().unwrap();
        engine::verify().unwrap();
        engine::install().unwrap();
        let bundle = engine::bundle().expect("installed");
        engine::verify_signature(&bundle).unwrap();
        let named = var("WINGS_CHECK_PROFILE");
        let (plugin, scope) = named.split_once('.').unwrap();
        let profile = Profile { plugin: plugin.into(), scope: scope.into(), dir: std::path::PathBuf::from(var("WINGS_CHECK_WEB")) };
        engine::set_choice(&profile, engine::Kind::Chromium).unwrap();
        assert_eq!(engine::choice(&profile), engine::Kind::Chromium);
        if let Ok(folder) = std::env::var("WINGS_CHECK_EXTENSION") {
            let staged = extensions::stage_path(&profile, std::path::Path::new(&folder)).unwrap();
            let (added, _) = extensions::prepare(&profile, &staged.stage).unwrap();
            extensions::keep(&profile, &added, &|| extensions::key(&profile)).unwrap();
            extensions::verified(&profile, &added.id, &|| extensions::key(&profile)).unwrap();
            eprintln!("added {} as {}", added.name, added.id);
        }
        eprintln!("installed {}", bundle.display());
    }
}
