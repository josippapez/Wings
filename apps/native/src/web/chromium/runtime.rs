//! Starting Chromium when the first page needs it, running its work while it has pages, and closing it as Wings quits.
//!
//! Nothing of CEF loads at launch: the first Chromium page asks (`ensure`), shows its loading state, and CEF starts on
//! the next turn of the main loop (`cef_initialize` holds the main thread about 0.8 s). CEF can start only once in a
//! process, so it stays up until Wings quits, its work's own 30 Hz round running only while a page or a popup is open
//! (`opened`/`gone`, `wings_chromium::pump`).

use std::{cell::RefCell, collections::HashMap};

use cef::*;
use objc2::{MainThreadMarker, define_class, msg_send, rc::Retained, runtime::NSObject, sel, MainThreadOnly};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSString};
use wings_chromium::{app_protocol, engine::Engine, pipe, pump, sandbox_bundle};

use crate::web::{Profile, engine};

/// Why a Chromium page didn't open.
pub const NOT_INSTALLED: &str = "Chromium isn't installed: choose it again in Settings, Browser";

enum Phase {
    Off,
    Starting,
    Ready,
    Failed(String),
}

type Then = Box<dyn FnOnce(Result<(), String>)>;

struct Runtime {
    phase: Phase,
    waiting: Vec<Then>,
    /// The profile CEF's global context keeps, the first page's: extensions load into it (`extensions`).
    global: Option<Profile>,
    /// The other profiles' contexts, by profile and whether they keep their data.
    contexts: HashMap<(Profile, bool), RequestContext>,
    /// Every browser alive, the ones Chrome opens itself included: all close before `cef::shutdown`.
    browsers: Vec<Browser>,
    /// Pages and popups open now.
    open: usize,
    quitting: bool,
    /// The browser target's DevTools pipe, once CEF started with it.
    pipe: Option<pipe::Pipe>,
    observer: Option<Retained<Quitting>>,
}

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime {
        phase: Phase::Off,
        waiting: Vec::new(),
        global: None,
        contexts: HashMap::new(),
        browsers: Vec::new(),
        open: 0,
        quitting: false,
        pipe: None,
        observer: None,
    });
}

/// Whether CEF runs in this process now.
pub fn running() -> bool {
    RUNTIME.with_borrow(|runtime| matches!(runtime.phase, Phase::Ready))
}

/// The profile CEF's global context keeps, once it started.
pub fn global() -> Option<Profile> {
    RUNTIME.with_borrow(|runtime| runtime.global.clone().filter(|_| matches!(runtime.phase, Phase::Ready)))
}

/// Runs `then` once CEF is up, starting it for `profile`'s page if nothing did yet, or with why it can't start.
pub fn ensure(profile: &Profile, then: impl FnOnce(Result<(), String>) + 'static) {
    let now = RUNTIME.with_borrow(|runtime| match &runtime.phase {
        Phase::Ready => Some(Ok(())),
        Phase::Failed(why) => Some(Err(why.clone())),
        Phase::Starting | Phase::Off => None,
    });
    if let Some(ready) = now {
        return then(ready);
    }
    let first = RUNTIME.with_borrow_mut(|runtime| {
        runtime.waiting.push(Box::new(then));
        let first = matches!(runtime.phase, Phase::Off);
        if first {
            runtime.phase = Phase::Starting;
            runtime.global = Some(profile.clone());
        }
        first
    });
    if !first {
        return;
    }
    // Checked off the main thread, then started on it: the page's loading state draws first.
    std::thread::spawn(|| {
        let checked = engine::bundle().ok_or_else(|| NOT_INSTALLED.to_owned()).and_then(|bundle| {
            engine::available().map_err(str::to_owned)?;
            engine::verify_signature(&bundle).map(|()| bundle)
        });
        dispatch2::DispatchQueue::main().exec_async(move || {
            if let Err(why) = checked.and_then(|bundle| start(&bundle)) {
                eprintln!("[chromium] couldn't start: {why}");
                finish(Err(why));
            }
        });
    });
}

/// Starts CEF from the engine at `bundle`. Its global context opens `global`'s profile.
fn start(bundle: &std::path::Path) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    let engine = Engine::new(bundle);
    engine.load()?;
    app_protocol::install()?;
    // Without it Chromium's child processes can't read the engine, the GPU process dies at start and Chromium ends
    // Wings with it ("GPU process isn't usable").
    sandbox_bundle::install(&engine)?;
    pump::start();
    let pipe = match pipe::open(|message| dispatch2::DispatchQueue::main().exec_async(move || super::extensions::heard(&message))) {
        Ok(pipe) => Some(pipe),
        Err(why) => {
            eprintln!("[chromium] no DevTools pipe, so no extensions: {why}");
            None
        }
    };
    let global = RUNTIME.with_borrow(|runtime| runtime.global.clone()).ok_or("no profile")?;
    let root = profiles_root().ok_or("no data folder")?;
    let profile = wings_chromium::engine::Profile { root: root.clone(), path: root.join(dir_name(&global)) };
    let mut settings = engine.settings(&profile);
    // Wings' own arguments aren't Chromium's.
    settings.command_line_args_disabled = 1;
    settings.log_severity = LogSeverity::ERROR;
    settings.log_file = CefString::from(root.join("chromium.log").to_string_lossy().as_ref());
    let piped = pipe.is_some();
    RUNTIME.with_borrow_mut(|runtime| runtime.pipe = pipe);
    let args = cef::args::Args::new();
    let mut app = WingsApp::new(piped);
    if cef::initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()) != 1 {
        return Err("Chromium didn't start".into());
    }
    // Closes Chromium before Wings ends: shutting it down with a browser still open crashed.
    let observer = Quitting::new(mtm);
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &observer,
            sel!(quitting:),
            Some(&NSString::from_str("NSApplicationWillTerminateNotification")),
            None,
        )
    };
    RUNTIME.with_borrow_mut(|runtime| runtime.observer = Some(observer));
    #[cfg(debug_assertions)]
    super::debug::listen();
    Ok(())
}

/// The folder Chromium keeps its profiles in, as CEF reads it: with every link resolved, as CEF refuses a profile that
/// isn't inside it as written (`/tmp/...` under `/private/tmp/...`), and opened in memory then.
pub(super) fn profiles_root() -> Option<std::path::PathBuf> {
    let root = engine::profiles()?;
    std::fs::create_dir_all(&root).ok()?;
    std::fs::canonicalize(root).ok()
}

/// The folder of `profile` under the profiles' root.
pub fn dir_name(profile: &Profile) -> String {
    format!("{}.{}", profile.plugin, profile.scope)
}

/// CEF is up or couldn't start: the pages waiting hear which.
fn finish(ready: Result<(), String>) {
    let waiting = RUNTIME.with_borrow_mut(|runtime| {
        runtime.phase = match &ready {
            Ok(()) => Phase::Ready,
            Err(why) => Phase::Failed(why.clone()),
        };
        std::mem::take(&mut runtime.waiting)
    });
    for then in waiting {
        then(ready.clone());
    }
}

/// The request context of `profile`'s pages: none for the global context's own kept pages, an in-memory one for its
/// pages that keep nothing, and one on disk of its own for another plugin's.
pub fn context(profile: &Profile, keep_data: bool) -> Option<RequestContext> {
    if keep_data && RUNTIME.with_borrow(|runtime| runtime.global.as_ref() == Some(profile)) {
        return None;
    }
    RUNTIME.with_borrow_mut(|runtime| {
        let root = profiles_root()?;
        runtime
            .contexts
            .entry((profile.clone(), keep_data))
            .or_insert_with(|| {
                let cache_path = if keep_data { root.join(dir_name(profile)).to_string_lossy().into_owned() } else { String::new() };
                let settings = RequestContextSettings { cache_path: CefString::from(cache_path.as_str()), ..Default::default() };
                request_context_create_context(Some(&settings), None).expect("CEF makes a request context")
            })
            .clone()
            .into()
    })
}

/// A page or a popup opened: CEF's work runs on its own too while one is.
pub fn opened() {
    let open = RUNTIME.with_borrow_mut(|runtime| {
        runtime.open += 1;
        runtime.open
    });
    pump::set_fallback(open > 0);
}

/// A page or a popup closed.
pub fn gone() {
    let open = RUNTIME.with_borrow_mut(|runtime| {
        runtime.open = runtime.open.saturating_sub(1);
        runtime.open
    });
    pump::set_fallback(open > 0);
}

/// A browser exists now (`CefLifeSpanHandler::OnAfterCreated`).
pub fn created(browser: &Browser) {
    RUNTIME.with_borrow_mut(|runtime| runtime.browsers.push(browser.clone()));
}

/// A browser is gone (`OnBeforeClose`).
pub fn closed(browser: &Browser) {
    let id = browser.identifier();
    RUNTIME.with_borrow_mut(|runtime| runtime.browsers.retain(|kept| kept.identifier() != id));
}

/// Sends a DevTools request to the browser target: false when CEF runs without the pipe.
pub fn send(message: &[u8]) -> bool {
    RUNTIME.with_borrow_mut(|runtime| runtime.pipe.as_mut().is_some_and(|pipe| pipe.send(message).is_ok()))
}

/// Closes every browser, Chrome's own included, waits up to 3 s for them to go, runs CEF's last work and shuts it
/// down, on the main thread as AppKit ends the app.
fn shutdown() {
    let browsers = RUNTIME.with_borrow_mut(|runtime| {
        runtime.quitting = true;
        runtime.browsers.clone()
    });
    super::page::close_all();
    super::extensions::close_popup();
    for browser in &browsers {
        if let Some(host) = browser.host() {
            host.close_browser(1);
        }
    }
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while RUNTIME.with_borrow(|runtime| !runtime.browsers.is_empty()) && std::time::Instant::now() < until {
        pump::work_now();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let left = RUNTIME.with_borrow(|runtime| runtime.browsers.len());
    if left > 0 {
        eprintln!("[chromium] {left} browsers didn't close in time");
    }
    RUNTIME.with_borrow_mut(|runtime| runtime.contexts.clear());
    pump::drain(10);
    cef::shutdown();
    // Removed in Settings while it ran, it wrote to its profiles until now: they go too.
    if engine::bundle().is_none()
        && let Some(profiles) = engine::profiles()
    {
        let _ = std::fs::remove_dir_all(profiles);
    }
}

define_class!(
    // SAFETY: NSObject has no rules for subclasses, and this has no ivars.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Quitting;

    impl Quitting {
        #[unsafe(method(quitting:))]
        fn quitting(&self, _notification: &NSNotification) {
            if RUNTIME.with_borrow(|runtime| matches!(runtime.phase, Phase::Ready) && !runtime.quitting) {
                shutdown();
            }
        }
    }
);

impl Quitting {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's `init`.
        unsafe { msg_send![super(this), init] }
    }
}

/// The browser process's switches. `use-mock-keychain`: Chromium keeps the key it encrypts its cookies with in a
/// Keychain item, which each build of Wings would be asked about anew, and every navigation waited on that prompt. With
/// it the cookies are kept unencrypted, as WebKit keeps Wings' own (Cookies.binarycookies is plain), so this is parity,
/// not a step down.
pub fn switches(piped: bool) -> Vec<String> {
    let mut switches: Vec<String> = [
        "use-mock-keychain",
        // A GPU process that crashes again and again falls back to software and, out of fallbacks, ends the browser
        // process, which is Wings: it restarts instead.
        "disable-gpu-process-crash-limit",
        "no-first-run",
        "no-default-browser-check",
    ]
    .map(String::from)
    .to_vec();
    if piped {
        switches.push("remote-debugging-pipe".into());
    }
    // A debug build checked by hand or by a script opens a DevTools port on this Mac's loopback when asked; a release
    // build never does.
    if cfg!(debug_assertions)
        && let Some(port) = std::env::var("WINGS_CHROMIUM_DEBUG_PORT").ok().filter(|port| port.parse::<u16>().is_ok())
    {
        switches.push(format!("remote-debugging-port={port}"));
    }
    // And on a locked Mac, where every window is hidden, it keeps drawing and running its pages for the script.
    if cfg!(debug_assertions) && std::env::var_os("WINGS_DRAW_WHILE_HIDDEN").is_some() {
        switches.extend(["disable-backgrounding-occluded-windows", "disable-renderer-backgrounding"].map(String::from));
    }
    switches
}

wrap_app! {
    struct WingsApp {
        piped: bool,
    }

    impl App {
        fn on_before_command_line_processing(&self, process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
            let browser = process_type.is_none_or(|kind| kind.to_string().is_empty());
            if let (true, Some(command_line)) = (browser, command_line) {
                for switch in switches(self.piped) {
                    match switch.split_once('=') {
                        Some((name, value)) => command_line.append_switch_with_value(Some(&CefString::from(name)), Some(&CefString::from(value))),
                        None => command_line.append_switch(Some(&CefString::from(switch.as_str()))),
                    }
                }
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(WingsBrowserProcess::new())
        }
    }
}

wrap_browser_process_handler! {
    struct WingsBrowserProcess {}

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            finish(Ok(()));
            super::extensions::started();
        }

        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            pump::schedule(delay_ms);
        }

        // What a browser Chrome opens itself gets (an extension's `tabs.create`): it closes as it opens, as WebKit
        // refuses an extension's new tabs.
        fn default_client(&self) -> Option<Client> {
            Some(StrayClient::new())
        }
    }
}

wrap_client! {
    struct StrayClient {}

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(StrayLifeSpan::new())
        }
    }
}

wrap_life_span_handler! {
    struct StrayLifeSpan {}

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser else { return };
            created(browser);
            let url = browser.main_frame().map(|frame| CefString::from(&frame.url()).to_string()).unwrap_or_default();
            eprintln!("[chromium] closed a window Chrome opened by itself: {}", crate::web::origin(&url).unwrap_or_default());
            if let Some(host) = browser.host() {
                hide_window_of(&host);
                host.close_browser(1);
            }
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                closed(browser);
            }
        }
    }
}

/// Hides the window a stray browser opened in before it closes, so it doesn't flash on screen.
fn hide_window_of(host: &BrowserHost) {
    let view = host.window_handle() as *mut objc2::runtime::AnyObject;
    if view.is_null() {
        return;
    }
    unsafe {
        let window: *mut objc2::runtime::AnyObject = msg_send![view, window];
        if !window.is_null() {
            let _: () = msg_send![window, orderOut: std::ptr::null::<objc2::runtime::AnyObject>()];
        }
    }
}

#[cfg(test)]
mod tests {
    /// Chromium's cookie key stays out of the Keychain, its GPU crashes never end Wings, and the pipe is on only when
    /// Wings holds it.
    #[test]
    fn chromium_starts_with_the_mock_keychain_no_gpu_crash_limit_and_the_pipe_only_when_held() {
        let switches = super::switches(true);
        for wanted in ["use-mock-keychain", "disable-gpu-process-crash-limit", "remote-debugging-pipe"] {
            assert!(switches.iter().any(|switch| switch == wanted), "{wanted}");
        }
        assert!(!super::switches(false).iter().any(|switch| switch == "remote-debugging-pipe"));
        assert!(!switches.iter().any(|switch| switch.contains("no-sandbox")), "the sandbox stays on");
    }
}
