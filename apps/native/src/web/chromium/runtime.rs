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

#[derive(Debug)]
enum Phase {
    Off,
    Starting,
    Ready,
    /// Why it didn't start, and, when that was its files' check (nothing of CEF loaded yet), the engine's
    /// `engine::generation` then: a removal and a new install since let the next page try again in this run.
    Failed(String, Option<u64>),
}

impl Phase {
    /// What a page asking for CEF now finds, with the engine at `generation`: a check that failed for an engine since
    /// removed or installed again counts as never tried, as CEF never loaded.
    fn now(&self, generation: u64) -> &Self {
        match self {
            Phase::Failed(_, Some(then)) if *then != generation => &Phase::Off,
            phase => phase,
        }
    }
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
    /// Counts the times the last page or popup closed, so only the latest wind-down stops CEF's own work (`gone`).
    wind_down: u64,
    quitting: bool,
    /// The browser target's DevTools pipe, once CEF started with it.
    pipe: Option<pipe::Pipe>,
    /// A context sharing the global one's profile, kept until CEF shuts down, which turned Chrome's password manager off
    /// in it once it was ready (`quiet`).
    quieted: Option<RequestContext>,
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
        wind_down: 0,
        quitting: false,
        pipe: None,
        quieted: None,
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
    let generation = engine::generation();
    let now = RUNTIME.with_borrow(|runtime| match runtime.phase.now(generation) {
        Phase::Ready => Some(Ok(())),
        Phase::Failed(why, _) => Some(Err(why.clone())),
        Phase::Starting | Phase::Off => None,
    });
    if let Some(ready) = now {
        return then(ready);
    }
    let first = RUNTIME.with_borrow_mut(|runtime| {
        runtime.waiting.push(Box::new(then));
        let first = matches!(runtime.phase.now(generation), Phase::Off);
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
    std::thread::spawn(move || {
        let checked = engine::bundle().ok_or_else(|| NOT_INSTALLED.to_owned()).and_then(|bundle| {
            engine::available().map_err(str::to_owned)?;
            engine::verify_bundle(&bundle).map(|()| bundle)
        });
        dispatch2::DispatchQueue::main().exec_async(move || {
            let started = match checked {
                // Nothing of CEF loaded: another install may start it in this run.
                Err(why) => Err((why, Some(generation))),
                Ok(bundle) => start(&bundle).map_err(|why| (why, None)),
            };
            if let Err((why, retry)) = started {
                eprintln!("[chromium] couldn't start: {why}");
                finish(Err(why), retry);
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
    let pipe = match pipe::open(|message| {
        if super::extensions::follows(&message) {
            dispatch2::DispatchQueue::main().exec_async(move || super::extensions::heard(&message));
        }
    }) {
        Ok(pipe) => Some(pipe),
        Err(why) => {
            eprintln!("[chromium] no DevTools pipe, so no extensions: {why}");
            None
        }
    };
    let global = RUNTIME.with_borrow(|runtime| runtime.global.clone()).ok_or("no profile")?;
    let root = profiles_root().ok_or("no data folder")?;
    // Its global context is `Default`, whatever the settings name: the starting profile's own data goes there first.
    super::homes::make_global(&root, &dir_name(&global))?;
    let profile = wings_chromium::engine::Profile { root: root.clone(), path: root.join("Default") };
    let mut settings = engine.settings(&profile);
    // Wings' own arguments aren't Chromium's.
    settings.command_line_args_disabled = 1;
    let (severity, log_file) = logging(&root, cfg!(debug_assertions));
    settings.log_severity = severity;
    if let Some(file) = log_file {
        settings.log_file = CefString::from(file.to_string_lossy().as_ref());
    }
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

/// How much Chromium writes of its own log, and where: its errors in `chromium.log` in a debug build, nothing in a release
/// one, whose log a person never reads and which would keep the addresses of the pages that failed.
fn logging(root: &std::path::Path, debug: bool) -> (LogSeverity, Option<std::path::PathBuf>) {
    match debug {
        true => (LogSeverity::ERROR, Some(root.join("chromium.log"))),
        false => (LogSeverity::DISABLE, None),
    }
}

/// Chrome's own password manager and autofill, off in every profile: they'd offer to save what is typed in a page, and
/// with the mock keychain (`switches`) what Chrome saved would be kept as good as unencrypted. Wings has its own
/// extensions for passwords. (`password_manager_pref_names.h`, `autofill_prefs.h` at 158.)
pub const OFF: [&str; 4] = ["credentials_enable_service", "credentials_enable_autosignin", "autofill.profile_enabled", "autofill.credit_card_enabled"];

/// The choosers of devices a page may ask for, blocked by default, so a page's request is refused at once: Chrome's own
/// chooser would be a window over Wings that Wings doesn't place, and WebKit in Wings has none of them.
const CHOOSERS: [ContentSettingTypes; 5] = [
    ContentSettingTypes::USB_GUARD,
    ContentSettingTypes::HID_GUARD,
    ContentSettingTypes::SERIAL_GUARD,
    ContentSettingTypes::BLUETOOTH_GUARD,
    ContentSettingTypes::BLUETOOTH_SCANNING,
];

/// Turns Chrome's password manager and autofill off in `context` (`OFF`) and blocks its device choosers (`CHOOSERS`),
/// once its profile is ready (`Quiet`): CEF refuses preferences of a context whose profile isn't, as the global one's
/// isn't yet in `OnContextInitialized`. On CEF's UI thread.
fn quiet(context: &RequestContext) {
    for name in OFF {
        let Some(mut off) = value_create() else { continue };
        off.set_bool(0);
        // CEF refuses the call, saying nothing, for an error string with no storage of its own (`CefString::default`).
        let mut error = CefString::from(" ");
        if context.set_preference(Some(&CefString::from(name)), Some(&mut off), Some(&mut error)) == 0 {
            eprintln!("[chromium] couldn't turn off {name}: {}", error.to_string().trim());
        }
    }
    for kind in CHOOSERS {
        context.set_content_setting(None, None, kind, ContentSettingValues::BLOCK);
    }
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

/// CEF is up or couldn't start: the pages waiting hear which. `retry`: the engine's generation when its files failed
/// their check before CEF loaded.
fn finish(ready: Result<(), String>, retry: Option<u64>) {
    let waiting = RUNTIME.with_borrow_mut(|runtime| {
        runtime.phase = match &ready {
            Ok(()) => Phase::Ready,
            Err(why) => Phase::Failed(why.clone(), retry),
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
                request_context_create_context(Some(&settings), Some(&mut Quiet::new())).expect("CEF makes a request context")
            })
            .clone()
            .into()
    })
}

/// The request contexts of `profile` CEF holds now, each with whether it keeps its data: the global one for the profile
/// that started Chromium, and those its pages made (`context`). None is made here.
pub fn live(profile: &Profile) -> Vec<(bool, RequestContext)> {
    RUNTIME.with_borrow(|runtime| {
        let global = (matches!(runtime.phase, Phase::Ready) && runtime.global.as_ref() == Some(profile)).then(request_context_get_global_context).flatten();
        let made = runtime.contexts.iter().filter(|((of, _), _)| of == profile).map(|((_, keep_data), context)| (*keep_data, context.clone()));
        global.map(|global| (true, global)).into_iter().chain(made).collect()
    })
}

/// A page or a popup opened: CEF's work runs on its own too while one is.
pub fn opened() {
    let open = RUNTIME.with_borrow_mut(|runtime| {
        runtime.open += 1;
        runtime.open
    });
    pump::set_fallback(open > 0);
    super::extensions::pages_open(true);
}

/// How many pages and popups are open, and how many browsers CEF holds, Chrome's own included: for the debug control.
pub(super) fn counts() -> (usize, usize) {
    RUNTIME.with_borrow(|runtime| (runtime.open, runtime.browsers.len()))
}

/// How long CEF's work goes on running on its own once the last page or popup closed. Chromium ends a closed page's
/// process and lets the extensions' workers go idle and stop only while its work runs: stopped at once, the closed page's
/// process (194 MB) and Bitwarden's (109 MB) stayed with every Browser tab closed.
const WIND_DOWN: std::time::Duration = std::time::Duration::from_secs(60);

/// A page or a popup closed. After the last, CEF's work runs on its own for `WIND_DOWN`, then only when CEF asks.
pub fn gone() {
    let (open, round) = RUNTIME.with_borrow_mut(|runtime| {
        runtime.open = runtime.open.saturating_sub(1);
        if runtime.open == 0 {
            runtime.wind_down += 1;
        }
        (runtime.open, runtime.wind_down)
    });
    super::extensions::pages_open(open > 0);
    if open > 0 {
        return;
    }
    let when = dispatch2::DispatchTime::try_from(WIND_DOWN).unwrap_or(dispatch2::DispatchTime::NOW);
    let _ = dispatch2::DispatchQueue::main().after(when, move || {
        if RUNTIME.with_borrow(|runtime| runtime.open == 0 && runtime.wind_down == round) {
            pump::set_fallback(false);
        }
    });
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

/// Closes every browser, Chrome's own included, waits up to 3 s for them to go, then, if they all went, runs CEF's last
/// work and shuts it down (`end`), on the main thread as AppKit ends the app.
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
    RUNTIME.with_borrow_mut(|runtime| {
        runtime.contexts.clear();
        runtime.quieted = None;
    });
    end(left, || pump::drain(10), cef::shutdown);
    // Removed in Settings while it ran, it wrote to its profiles until now: they go too.
    if engine::bundle().is_none()
        && let Some(profiles) = engine::profiles()
    {
        let _ = std::fs::remove_dir_all(profiles);
    }
}

/// Runs CEF's last work (`drain`) and shuts it down, once every browser closed. With `left` still open it doesn't: in
/// Chrome's style a browser whose page's script doesn't answer (busy, or slow in its `beforeunload`) stays open however
/// it is closed, and `CefShutdown` then destroys its Chrome window during the profile's teardown, which crashed (a null
/// `ProfileManager` with one left, a freed `BrowserWindowFeatures` with two). CEF ends with the process instead.
fn end(left: usize, drain: impl FnOnce(), shut_down: impl FnOnce()) {
    if left > 0 {
        eprintln!("[chromium] {left} browsers didn't close in time, so Chromium ends with Wings without shutting down");
        return;
    }
    drain();
    shut_down();
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
        // Nothing of Chromium's own phones home: no component downloads (Safe Browsing lists, models, hints), no
        // background requests (variations, translate, update checks), no reliability reports, no crash reports.
        "disable-component-update",
        "disable-background-networking",
        "disable-domain-reliability",
        "disable-breakpad",
        // No window Wings doesn't place: no Document Picture-in-Picture window over everything, and a site's HTTP
        // sign-in asks CEF (`GetAuthCredentials`, which refuses it, as WebKit in Wings shows no such prompt) rather
        // than showing Chrome's own login dialog.
        "disable-blink-features=DocumentPictureInPictureAPI",
        "disable-chrome-login-prompt",
        // A window a page opens with no click of yours is refused by Wings, which tells the page's plugin of it
        // (`page::PageLifeSpan::on_before_popup`): Chrome's own blocker refused it before Wings heard of it.
        "disable-popup-blocking",
        // Nor do two features the switches above leave on: the omnibox's AI Mode asking Google whether this Chromium
        // may use it (`aim_eligibility_fetch`, www.google.com/async/folae), and connecting ahead of time to the default
        // search engine, Google (`SearchEnginePreconnector`). Nor a spare page process started ahead of the next page
        // (`kSpareRendererForSitePerProcess`), which stayed, 74 MB, with every Browser tab closed. Chromium 158's names;
        // CEF's own list is kept (`merged`).
        "disable-features=AimEnabled,PreconnectToSearch,SpareRendererForSitePerProcess",
        // Nor Chrome's on-device AI model asking update.googleapis.com for its manifest (`update_client`, component
        // ceofaddefefcbblgcgnibnonglccbfja), which it does on demand past disable-component-update: an override that
        // names no manifest leaves it none to fetch (`CreateManifestDelegate` in Chromium 158). Since 158 the switch
        // also turns the on-device AI setting's default on (`ShouldEnableOnDeviceAiByDefault`); the override still
        // installs no model.
        "optimization-guide-manifest-override=/dev/null",
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
    // And records every request Chromium makes, its own included, to the file named, for a check of what it reaches.
    if cfg!(debug_assertions)
        && let Some(file) = std::env::var("WINGS_CHROMIUM_NETLOG").ok().filter(|file| file.starts_with("/tmp/"))
    {
        switches.push(format!("log-net-log={file}"));
    }
    // And on a locked Mac, where every window is hidden, it keeps drawing and running its pages for the script.
    if cfg!(debug_assertions) && std::env::var_os("WINGS_DRAW_WHILE_HIDDEN").is_some() {
        switches.extend(["disable-backgrounding-occluded-windows", "disable-renderer-backgrounding"].map(String::from));
    }
    switches
}

/// The switches whose value is a list Wings adds to.
const LISTS: [&str; 2] = ["disable-features", "disable-blink-features"];

/// A switch's value `had` with the items of `ours` it lacks added after its own, comma-separated: Chromium reads the last
/// switch of a name only, so a list Wings adds to (`disable-features`) has to carry the one CEF set.
fn merged(had: &str, ours: &str) -> String {
    let mut items: Vec<&str> = had.split(',').filter(|item| !item.is_empty()).collect();
    for item in ours.split(',').filter(|item| !item.is_empty()) {
        if !items.contains(&item) {
            items.push(item);
        }
    }
    items.join(",")
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
                        Some((name, value)) => {
                            let named = CefString::from(name);
                            // CEF wrote its own list of features to turn off before this; a second switch would replace it.
                            let value = match LISTS.contains(&name) {
                                true => merged(&CefString::from(&command_line.switch_value(Some(&named))).to_string(), value),
                                false => value.to_owned(),
                            };
                            command_line.append_switch_with_value(Some(&named), Some(&CefString::from(value.as_str())));
                        }
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
            // A context sharing the global one's profile, which hears when that profile is ready (`Quiet`).
            let mut global = request_context_get_global_context();
            let shared = request_context_cef_create_context_shared(global.as_mut(), Some(&mut Quiet::new()));
            RUNTIME.with_borrow_mut(|runtime| runtime.quieted = shared);
            finish(Ok(()), None);
            super::extensions::started();
        }

        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            pump::schedule(delay_ms);
        }

        // What a browser Chrome opens itself gets (an extension's `windows.create` or `tabs.create`): a page of an
        // extension Wings loaded shows (`extensions::opened_window_may_go`), anything else closes as it goes there.
        fn default_client(&self) -> Option<Client> {
            Some(StrayClient::new())
        }
    }
}

wrap_request_context_handler! {
    struct Quiet {}

    impl RequestContextHandler {
        fn on_request_context_initialized(&self, request_context: Option<&mut RequestContext>) {
            if let Some(context) = request_context {
                quiet(context);
            }
        }
    }
}

// A window Chrome opened by itself: it refuses what a page refuses (permissions, downloads), and opens nothing more.
wrap_client! {
    struct StrayClient {}

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(StrayLifeSpan::new())
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(StrayRequests::new())
        }
        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(super::page::Refuse::new())
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(super::page::NoDownloads::new())
        }
    }
}

// The inspector Wings opens for a page (`page::Shared`, Inspect): a window of its own that refuses what a page refuses and
// opens nothing more. It has no `StrayRequests`, which close a window that goes to anything but an extension's page.
wrap_client! {
    struct InspectorClient {}

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(StrayLifeSpan::new())
        }
        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(super::page::Refuse::new())
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(super::page::NoDownloads::new())
        }
    }
}

/// The client of the inspector window of a page.
pub(super) fn inspector() -> Client {
    InspectorClient::new()
}

wrap_request_handler! {
    struct StrayRequests {}

    impl RequestHandler {
        /// Its address isn't known as it opens, so it closes as it goes where it may not.
        fn on_before_browse(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, _user_gesture: ::std::os::raw::c_int, _is_redirect: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            let url = request.map(|request| CefString::from(&request.url()).to_string()).unwrap_or_default();
            let main_frame = frame.is_none_or(|frame| frame.is_main() != 0);
            if super::extensions::opened_window_may_go(&url, main_frame) {
                return 0;
            }
            if main_frame && let Some(host) = browser.and_then(|browser| browser.host()) {
                eprintln!("[chromium] closed a window Chrome opened by itself: {}", crate::web::origin(&url).unwrap_or_default());
                hide_window_of(&host);
                host.close_browser(1);
            }
            1
        }

        /// A link it opens in a new tab goes nowhere.
        fn on_open_urlfrom_tab(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _target_url: Option<&CefString>, _target_disposition: WindowOpenDisposition, _user_gesture: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            1
        }
    }
}

wrap_life_span_handler! {
    struct StrayLifeSpan {}

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                created(browser);
                opened();
            }
        }

        /// A link it opens in a new window goes nowhere.
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            _target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            1
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                closed(browser);
                gone();
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

    /// Nothing of Chromium's own phones home, no window it opens by itself draws over Wings, and only a debug build
    /// keeps Chromium's log.
    #[test]
    fn chromium_starts_quiet_and_with_no_windows_of_its_own() {
        let switches = super::switches(false);
        for wanted in [
            "disable-component-update",
            "disable-background-networking",
            "disable-domain-reliability",
            "disable-breakpad",
            "disable-blink-features=DocumentPictureInPictureAPI",
            "disable-chrome-login-prompt",
            "disable-popup-blocking",
            "disable-features=AimEnabled,PreconnectToSearch,SpareRendererForSitePerProcess",
            "optimization-guide-manifest-override=/dev/null",
        ] {
            assert!(switches.iter().any(|switch| switch == wanted), "{wanted}");
        }
        let root = std::path::Path::new("/profiles");
        assert_eq!(super::logging(root, false), (cef::LogSeverity::DISABLE, None), "a release build");
        assert_eq!(super::logging(root, true), (cef::LogSeverity::ERROR, Some(root.join("chromium.log"))));
        assert_eq!(super::OFF, ["credentials_enable_service", "credentials_enable_autosignin", "autofill.profile_enabled", "autofill.credit_card_enabled"]);
    }

    /// The features Wings turns off join those CEF turned off on the same switch, which Chromium reads once: none of
    /// CEF's is lost, and none is named twice.
    #[test]
    fn wings_features_to_turn_off_join_cefs_own() {
        let cef = "AutofillActorMode,BackForwardCache,GlicActorUi";
        assert_eq!(super::merged(cef, "AimEnabled,PreconnectToSearch"), "AutofillActorMode,BackForwardCache,GlicActorUi,AimEnabled,PreconnectToSearch");
        assert_eq!(super::merged("", "AimEnabled"), "AimEnabled", "CEF set none");
        assert_eq!(super::merged("AimEnabled,LensOverlay", "AimEnabled"), "AimEnabled,LensOverlay");
        assert_eq!(super::merged("DocumentPictureInPictureAPI", ""), "DocumentPictureInPictureAPI");
    }

    /// Quitting shuts CEF down only once every browser closed, after its last work: with a browser still open (its page's
    /// script busy as it was asked to unload) CEF's shutdown destroyed that browser's Chrome window with its profile and
    /// crashed Wings.
    #[test]
    fn chromium_shuts_down_only_once_every_browser_closed() {
        let steps = std::cell::RefCell::new(Vec::new());
        super::end(1, || steps.borrow_mut().push("work"), || steps.borrow_mut().push("shutdown"));
        assert!(steps.borrow().is_empty(), "a browser left open: {:?}", steps.borrow());
        super::end(0, || steps.borrow_mut().push("work"), || steps.borrow_mut().push("shutdown"));
        assert_eq!(*steps.borrow(), ["work", "shutdown"]);
    }

    /// Files that failed their check before CEF loaded keep Chromium off only until the engine is removed or installed
    /// again: the next page then starts it, in the same run. A start that failed once CEF loaded stays failed.
    #[test]
    fn a_failed_check_lasts_only_until_the_engine_changes() {
        use super::Phase;
        let checked = Phase::Failed("changed".into(), Some(4));
        assert!(matches!(checked.now(4), Phase::Failed(..)), "the same engine");
        assert!(matches!(checked.now(5), Phase::Off), "removed and downloaded again");
        let started = Phase::Failed("Chromium didn't start".into(), None);
        assert!(matches!(started.now(5), Phase::Failed(..)));
        assert!(matches!(Phase::Ready.now(5), Phase::Ready));
    }
}
