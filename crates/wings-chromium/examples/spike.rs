//! Feasibility spike for a Chromium engine in Wings' Browser tab.
//!
//! A plain AppKit window stands in for Wings: a text field for a terminal, a button for an extension's toolbar button,
//! and a region below them where the browser goes. The browser is a frameless Chrome-style CEF window glued over that
//! region as a child window; the button opens the extension's popup page in a second one under it.
//!
//! Like Wings, the host owns `NSApp` (an `NSApplication` subclass made at run time with nothing CEF needs, the way GPUI
//! makes its own) and the run loop (`[NSApp run]`, CEF's work runs from a timer). CEF loads from a folder outside the
//! app bundle. Lines on stdout starting with `SPIKE` say what happened.
//!
//! Options (Chromium's own switches pass straight through):
//!   --wcs-engine=<app>      the engine's bundle (framework and helpers in its Contents/Frameworks)
//!   --wcs-profile=<dir>     the profile root (CEF's root_cache_path); the profile is <dir>/Default
//!   --wcs-url=<url>         the first page
//!   --wcs-popup-url=<url>   what the button opens (Bitwarden's popup by default)
//!   --wcs-open-popup        open the popup at start, as if the button was clicked
//!   --wcs-control=<file>    on SIGUSR1, run the commands in this file (see `run_command`)
//!   --wcs-pump-fallback-ms=<ms>  also run CEF's work this often on its own (33 by default, 0 never)
//!   --wcs-no-sandbox        run the child processes without the macOS sandbox

use std::cell::RefCell;
use std::ffi::c_void;
use std::io::Write as _;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cef::*;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSBackingStoreType, NSButton, NSEvent,
    NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem, NSScreen, NSTextField, NSView, NSWindow, NSWindowDelegate,
    NSWindowNumberListOptions, NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_foundation::{ns_string, NSNotification, NSNotificationCenter, NSPoint, NSRect, NSSize, NSString};
use wings_chromium::engine::{Engine, Profile};
use wings_chromium::{app_protocol, pump, sandbox_bundle, window as frameless};

const BITWARDEN: &str = "nngceckbapebfimnlniiiahkandclblb";
const POPUP_SIZE: NSSize = NSSize { width: 380.0, height: 600.0 };
const TOOLBAR_HEIGHT: f64 = 52.0;

struct Config {
    engine: PathBuf,
    profile: PathBuf,
    url: String,
    popup_url: String,
    open_popup: bool,
    control: Option<PathBuf>,
    pump_fallback: Option<Duration>,
    no_sandbox: bool,
}

impl Config {
    fn from_args() -> Self {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let spike = home.join("Library/Caches/wings-chromium-spike");
        let mut config = Config {
            engine: spike.join("156.0.31/Wings Chromium.app"),
            profile: spike.join("profile"),
            url: "data:text/html,<title>Wings%20Chromium%20spike</title><h1>Wings%20Chromium%20spike</h1>".into(),
            popup_url: format!("chrome-extension://{BITWARDEN}/popup/index.html"),
            open_popup: false,
            control: None,
            pump_fallback: Some(Duration::from_millis(33)),
            no_sandbox: false,
        };
        for arg in std::env::args().skip(1) {
            let (name, value) = arg.split_once('=').unwrap_or((&arg, ""));
            match name {
                "--wcs-engine" => config.engine = value.into(),
                "--wcs-profile" => config.profile = value.into(),
                "--wcs-url" => config.url = value.into(),
                "--wcs-popup-url" => config.popup_url = value.into(),
                "--wcs-open-popup" => config.open_popup = true,
                "--wcs-control" => config.control = Some(value.into()),
                "--wcs-pump-fallback-ms" => {
                    config.pump_fallback = value.parse().ok().filter(|ms| *ms > 0).map(Duration::from_millis)
                }
                "--wcs-no-sandbox" => config.no_sandbox = true,
                _ => {}
            }
        }
        config
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();
static LAUNCHED_MS: OnceLock<u128> = OnceLock::new();

fn config() -> &'static Config {
    CONFIG.get().expect("config")
}

fn epoch_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_millis())
}

fn log(event: &str, detail: impl std::fmt::Display) {
    let since = epoch_ms() - LAUNCHED_MS.get().copied().unwrap_or_default();
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "SPIKE {since:>6}ms {event} {detail}");
    let _ = out.flush();
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    Main,
    Popup,
}

/// A CEF window glued over the host.
struct Child {
    window: cef::Window,
    ns: Retained<NSWindow>,
}

#[derive(Default)]
struct State {
    host: Option<Retained<NSWindow>>,
    field: Option<Retained<NSTextField>>,
    button: Option<Retained<NSButton>>,
    region: Option<Retained<NSView>>,
    controller: Option<Retained<Controller>>,
    main: Option<Child>,
    popup: Option<Child>,
    popup_opening: bool,
    /// Every browser, the ones Chrome opens on its own (an extension's new tab) included: all must close before
    /// `cef::shutdown`.
    browsers: Vec<Browser>,
    quitting: bool,
    first_load_logged: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|state| f(&mut state.borrow_mut()))
}

fn main() {
    LAUNCHED_MS.get_or_init(epoch_ms);
    let config = CONFIG.get_or_init(Config::from_args);
    log("launch", format!("epoch_ms={} pid={}", LAUNCHED_MS.get().unwrap(), std::process::id()));
    let mtm = MainThreadMarker::new().expect("main thread");

    // NSApp from a subclass the host owns, made the way GPUI makes GPUIApplication: CEF's methods aren't on it.
    let app = host_application(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let engine = Engine::new(&config.engine);
    if let Err(error) = engine.load() {
        log("error", error);
        std::process::exit(1);
    }
    log("cef_loaded", engine.framework().display());

    match app_protocol::install() {
        Ok(installed) => log("app_protocol", format!("{installed:?}")),
        Err(error) => {
            log("error", error);
            std::process::exit(1);
        }
    }

    if !config.no_sandbox {
        match sandbox_bundle::install(&engine) {
            Ok(()) => log("sandbox_bundle", format!("CEF's +[NSBundle mainBundle] is {}", engine.bundle().display())),
            Err(error) => {
                log("error", error);
                std::process::exit(1);
            }
        }
    }

    pump::start(config.pump_fallback);
    build_host(mtm);

    let profile = Profile { root: config.profile.clone(), path: config.profile.join("Default") };
    let mut settings = engine.settings(&profile);
    settings.no_sandbox = config.no_sandbox as i32;
    log(
        "settings",
        format!(
            "framework_dir_path={:?} browser_subprocess_path={:?} main_bundle_path={:?} root_cache_path={:?} cache_path={:?} external_message_pump={} no_sandbox={}",
            engine.framework(),
            engine.helper(),
            engine.bundle(),
            profile.root,
            profile.path,
            settings.external_message_pump,
            settings.no_sandbox
        ),
    );

    let args = cef::args::Args::new();
    let mut cef_app = SpikeApp::new();
    let initialized = cef::initialize(Some(args.as_main_args()), Some(&settings), Some(&mut cef_app), std::ptr::null_mut());
    log("cef_initialized", initialized);
    if initialized != 1 {
        std::process::exit(1);
    }

    listen_for_control();
    app.run();

    let (work, scheduled) = pump::counts();
    log("run_loop_returned", format!("pump_work={work} pump_schedule={scheduled}"));
    pump::drain();
    cef::shutdown();
    log("shutdown", "done");
}

fn host_application(mtm: MainThreadMarker) -> Retained<NSApplication> {
    let class: &AnyClass = ClassBuilder::new(c"SpikeHostApplication", NSApplication::class())
        .expect("SpikeHostApplication")
        .register();
    let _: *mut AnyObject = unsafe { msg_send![class, sharedApplication] };
    NSApplication::sharedApplication(mtm)
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SpikeController"]
    struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSWindowDelegate for Controller {
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            layout_children();
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            quit();
        }
    }

    impl Controller {
        #[unsafe(method(togglePopup:))]
        fn toggle_popup_action(&self, _sender: Option<&AnyObject>) {
            toggle_popup();
        }

        #[unsafe(method(quitSpike:))]
        fn quit_action(&self, _sender: Option<&AnyObject>) {
            quit();
        }

        /// Every window's key and main changes, CEF's included.
        #[unsafe(method(windowStateChanged:))]
        fn window_state_changed(&self, notification: &NSNotification) {
            let name = notification.name().to_string();
            let window = notification.object().and_then(|object| object.downcast::<NSWindow>().ok());
            log("window_event", format!("{name} window={}", window.map_or("?".into(), |window| describe(&window))));
        }
    }
);

impl Controller {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn build_host(mtm: MainThreadMarker) {
    let size = NSSize::new(1000.0, 700.0);
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    let host = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(160.0, 160.0), size),
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { host.setReleasedWhenClosed(false) };
    host.setTitle(ns_string!("Wings Chromium spike (host)"));
    let content = host.contentView().expect("content view");
    let controller = Controller::new(mtm);

    let field = NSTextField::textFieldWithString(ns_string!(""), mtm);
    field.setPlaceholderString(Some(ns_string!("Terminal stand-in: type here")));
    field.setFrame(NSRect::new(NSPoint::new(12.0, size.height - 40.0), NSSize::new(size.width - 152.0, 28.0)));
    field.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin);
    content.addSubview(&field);

    let button = unsafe {
        NSButton::buttonWithTitle_target_action(ns_string!("Bitwarden"), Some(&controller), Some(sel!(togglePopup:)), mtm)
    };
    button.setFrame(NSRect::new(NSPoint::new(size.width - 132.0, size.height - 42.0), NSSize::new(120.0, 32.0)));
    button.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMinYMargin);
    content.addSubview(&button);

    let region = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size.width, size.height - TOOLBAR_HEIGHT)),
    );
    region.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    content.addSubview(&region);

    host.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
    let center = NSNotificationCenter::defaultCenter();
    for name in [
        ns_string!("NSWindowDidBecomeKeyNotification"),
        ns_string!("NSWindowDidResignKeyNotification"),
        ns_string!("NSWindowDidBecomeMainNotification"),
        ns_string!("NSWindowDidResignMainNotification"),
    ] {
        unsafe { center.addObserver_selector_name_object(&controller, sel!(windowStateChanged:), Some(name), None) };
    }
    build_menu(mtm, &controller);

    host.makeKeyAndOrderFront(None);
    host.makeFirstResponder(Some(&field));
    log("host_window", describe(&host));
    with_state(|state| {
        state.host = Some(host);
        state.field = Some(field);
        state.button = Some(button);
        state.region = Some(region);
        state.controller = Some(controller);
    });
}

fn build_menu(mtm: MainThreadMarker, controller: &Controller) {
    let item = |title: &NSString, action, key: &NSString| unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), title, Some(action), key)
    };
    let bar = NSMenu::new(mtm);
    let app_menu = NSMenu::new(mtm);
    let quit = item(ns_string!("Quit spike"), sel!(quitSpike:), ns_string!("q"));
    unsafe { quit.setTarget(Some(controller)) };
    app_menu.addItem(&quit);
    // Edit items go to the first responder, so copy and paste work in the text field and in the page alike.
    let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Edit"));
    edit_menu.addItem(&item(ns_string!("Cut"), sel!(cut:), ns_string!("x")));
    edit_menu.addItem(&item(ns_string!("Copy"), sel!(copy:), ns_string!("c")));
    edit_menu.addItem(&item(ns_string!("Paste"), sel!(paste:), ns_string!("v")));
    edit_menu.addItem(&item(ns_string!("Select All"), sel!(selectAll:), ns_string!("a")));
    for submenu in [app_menu, edit_menu] {
        let holder = NSMenuItem::new(mtm);
        holder.setSubmenu(Some(&submenu));
        bar.addItem(&holder);
    }
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&bar));
}

/// Where a child window goes, in screen coordinates: the browser over the region, the popup under the button.
fn frame_for(role: Role, state: &State) -> Option<NSRect> {
    let host = state.host.as_ref()?;
    match role {
        Role::Main => {
            let region = state.region.as_ref()?;
            Some(host.convertRectToScreen(region.convertRect_toView(region.bounds(), None)))
        }
        Role::Popup => {
            let button = state.button.as_ref()?;
            let anchor = host.convertRectToScreen(button.convertRect_toView(button.bounds(), None));
            let x = anchor.origin.x + anchor.size.width - POPUP_SIZE.width;
            let y = anchor.origin.y - 4.0 - POPUP_SIZE.height;
            Some(NSRect::new(NSPoint::new(x, y), POPUP_SIZE))
        }
    }
}

/// AppKit's screen rect (bottom-left origin) as CEF Views takes it (top-left origin of the main screen).
fn cef_rect(frame: NSRect) -> Rect {
    let mtm = MainThreadMarker::new().expect("main thread");
    let screen_height = NSScreen::screens(mtm).firstObject().map_or(0.0, |screen| screen.frame().size.height);
    Rect {
        x: frame.origin.x as i32,
        y: (screen_height - frame.origin.y - frame.size.height) as i32,
        width: frame.size.width as i32,
        height: frame.size.height as i32,
    }
}

fn layout_children() {
    with_state(|state| {
        for (role, child) in [(Role::Main, &state.main), (Role::Popup, &state.popup)] {
            if let (Some(child), Some(frame)) = (child, frame_for(role, state)) {
                child.ns.setFrame_display(frame, true);
            }
        }
    });
}

fn open_child(role: Role) {
    let url = match role {
        Role::Main => config().url.clone(),
        Role::Popup => config().popup_url.clone(),
    };
    let Some(frame) = with_state(|state| frame_for(role, state)) else {
        return;
    };
    log("open_child", format!("{role:?} url={url}"));
    frameless::open(
        &url,
        cef_rect(frame),
        Some(SpikeClient::new()),
        Rc::new(move |window, _view| on_child_created(role, window)),
        Rc::new(move |active| log("cef_window_activation", format!("{role:?} active={active}"))),
    );
}

fn on_child_created(role: Role, window: &mut cef::Window) {
    let Some(ns) = frameless::ns_window(window) else {
        log("error", format!("{role:?}: no NSWindow for the CEF window"));
        return;
    };
    log("child_created", format!("{role:?} before_attach {}", describe(&ns)));
    window.show();
    let Some((host, frame)) = with_state(|state| Some((state.host.clone()?, frame_for(role, state)?))) else {
        return;
    };
    frameless::attach(&ns, &host, frame);
    if role == Role::Main {
        // Flush with the host's content: no shadow around the page.
        ns.setHasShadow(false);
    }
    log("child_attached", format!("{role:?} {} region={}", describe(&ns), rect(frame)));
    let child = Child { window: window.clone(), ns };
    with_state(|state| match role {
        Role::Main => state.main = Some(child),
        Role::Popup => {
            state.popup = Some(child);
            state.popup_opening = false;
        }
    });
    log("window_order", window_order());
}

fn toggle_popup() {
    let open = with_state(|state| state.popup.take());
    match open {
        Some(popup) => {
            log("popup_close", "");
            popup.window.close();
        }
        None if !with_state(|state| std::mem::replace(&mut state.popup_opening, true)) => open_child(Role::Popup),
        None => {}
    }
}

fn quit() {
    if with_state(|state| std::mem::replace(&mut state.quitting, true)) {
        return;
    }
    log("quit", "closing browsers");
    let (children, browsers) = with_state(|state| ([state.popup.take(), state.main.take()], state.browsers.clone()));
    let own: Vec<i32> = children.iter().flatten().filter_map(|child| child.window.child_view_at(0)?.as_browser_view()?.browser()).map(|browser| browser.identifier()).collect();
    for child in children.into_iter().flatten() {
        child.window.close();
    }
    for browser in browsers.iter().filter(|browser| !own.contains(&browser.identifier())) {
        log("close_browser", format!("id={} (opened by Chrome)", browser.identifier()));
        if let Some(host) = browser.host() {
            host.close_browser(1);
        }
    }
    // Shutting down with a browser still open crashes CEF; give them a moment, then stop anyway.
    after(3000, || stop_run_loop("timeout"));
    if with_state(|state| state.browsers.is_empty()) {
        stop_run_loop("no browsers");
    }
}

fn stop_run_loop(reason: &str) {
    log("stop_run_loop", reason);
    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.stop(None);
    // `stop:` takes effect after the next event.
    if let Some(event) = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::ApplicationDefined,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        0,
        0,
        0,
    ) {
        app.postEvent_atStart(&event, true);
    }
}

// --- Control: SIGUSR1 runs the commands in the control file, so a script can drive the spike without real input. ---

extern "C" {
    static _dispatch_main_q: c_void;
    static _dispatch_source_type_signal: c_void;
    fn dispatch_source_create(kind: *const c_void, handle: usize, mask: usize, queue: *const c_void) -> *mut c_void;
    fn dispatch_source_set_event_handler_f(source: *mut c_void, handler: extern "C" fn(*mut c_void));
    fn dispatch_resume(object: *mut c_void);
    fn dispatch_after_f(when: u64, queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_time(when: u64, delta: i64) -> u64;
    fn signal(signal: i32, handler: usize) -> usize;
}

const SIGUSR1: i32 = 30;
const SIG_IGN: usize = 1;

fn listen_for_control() {
    if config().control.is_none() {
        return;
    }
    extern "C" fn on_signal(_context: *mut c_void) {
        let Some(path) = config().control.as_ref() else { return };
        let commands = std::fs::read_to_string(path).unwrap_or_default();
        for command in commands.lines().filter(|line| !line.trim().is_empty()) {
            run_command(command.trim());
        }
    }
    unsafe {
        signal(SIGUSR1, SIG_IGN);
        let source = dispatch_source_create(&_dispatch_source_type_signal, SIGUSR1 as usize, 0, &_dispatch_main_q);
        dispatch_source_set_event_handler_f(source, on_signal);
        dispatch_resume(source);
    }
    log("control", format!("SIGUSR1 runs {}", config().control.as_ref().unwrap().display()));
}

/// Runs `f` on the main thread after `ms`.
fn after(ms: u64, f: impl FnOnce() + 'static) {
    extern "C" fn run(context: *mut c_void) {
        let f = unsafe { Box::from_raw(context.cast::<Box<dyn FnOnce()>>()) };
        f();
    }
    let f: Box<Box<dyn FnOnce()>> = Box::new(Box::new(f));
    unsafe {
        let when = dispatch_time(0, (ms * 1_000_000) as i64);
        dispatch_after_f(when, &_dispatch_main_q, Box::into_raw(f).cast(), run);
    }
}

fn run_command(command: &str) {
    log("command", command);
    let mut words = command.split_whitespace();
    match words.next().unwrap_or_default() {
        "popup" => toggle_popup(),
        "state" => log("state", focus_state()),
        // What clicking into the page does: AppKit makes the browser's window key, Views gives the page focus.
        "focus-browser" => {
            if let Some((window, ns)) = with_state(|state| state.main.as_ref().map(|main| (main.window.clone(), main.ns.clone()))) {
                ns.makeKeyWindow();
                if let Some(view) = window.child_view_at(0) {
                    view.request_focus();
                }
            }
            after(300, || log("state_after_focus_browser", focus_state()));
        }
        // Handing typing back to the terminal: the host's window key again, the field its first responder.
        "focus-host" => {
            with_state(|state| {
                if let (Some(host), Some(field)) = (&state.host, &state.field) {
                    host.makeKeyWindow();
                    host.makeFirstResponder(Some(field));
                }
            });
            after(300, || log("state_after_focus_host", focus_state()));
        }
        "resize" | "move" => {
            let numbers: Vec<f64> = words.filter_map(|word| word.parse().ok()).collect();
            // Out of the state: the resize calls back into `windowDidResize:`, which lays the children out.
            let Some(host) = with_state(|state| state.host.clone()) else { return };
            let mut frame = host.frame();
            if command.starts_with("resize") {
                frame.size = NSSize::new(numbers.first().copied().unwrap_or(1000.0), numbers.get(1).copied().unwrap_or(700.0));
            } else {
                frame.origin.x += numbers.first().copied().unwrap_or(0.0);
                frame.origin.y += numbers.get(1).copied().unwrap_or(0.0);
            }
            host.setFrame_display(frame, true);
            log("geometry", geometry());
        }
        // Whether the browser's window may become main: off, the host stays main (and its title bar lit) while the
        // page has the keyboard.
        "never-main" => {
            if let Some(main) = with_state(|state| state.main.as_ref().map(|main| main.ns.clone())) {
                log("never_main", frameless::never_main(&main));
            }
        }
        "geometry" => log("geometry", geometry()),
        // Wings hides the web view while one of its own menus or dialogs covers it.
        "hide-browser" => with_state(|state| {
            if let (Some(host), Some(main)) = (&state.host, &state.main) {
                host.removeChildWindow(&main.ns);
                main.ns.orderOut(None);
            }
        }),
        "show-browser" => with_state(|state| {
            if let (Some(host), Some(main), Some(frame)) = (&state.host, &state.main, frame_for(Role::Main, state)) {
                frameless::attach(&main.ns, host, frame);
            }
        }),
        "order" => log("window_order", window_order()),
        // CEF's frameless window keeps a titled style mask, which macOS draws with rounded corners.
        "borderless" => with_state(|state| {
            if let (Some(main), Some(frame)) = (&state.main, frame_for(Role::Main, state)) {
                main.ns.setStyleMask(NSWindowStyleMask::Borderless);
                main.ns.setFrame_display(frame, true);
            }
        }),
        "describe" => with_state(|state| {
            for window in [state.host.clone(), state.main.as_ref().map(|main| main.ns.clone()), state.popup.as_ref().map(|popup| popup.ns.clone())].into_iter().flatten() {
                log("describe", describe(&window));
            }
        }),
        "pump" => {
            let (work, scheduled) = pump::counts();
            log("pump", format!("work={work} schedule={scheduled}"));
        }
        "quit" => quit(),
        other => log("error", format!("unknown command {other}")),
    }
}

// --- Reporting ---

fn rect(frame: NSRect) -> String {
    format!("({:.0},{:.0} {:.0}x{:.0})", frame.origin.x, frame.origin.y, frame.size.width, frame.size.height)
}

fn describe(window: &NSWindow) -> String {
    let class: String = unsafe {
        let class: *const AnyClass = msg_send![window, class];
        (*class).name().to_string_lossy().into_owned()
    };
    format!(
        "#{} class={class} frame={} style={:#x} key={} main={} can_key={} can_main={} visible={} on_screen={} shadow={} opaque={} level={} collection={:#x} parent={}",
        window.windowNumber(),
        rect(window.frame()),
        window.styleMask().0,
        window.isKeyWindow(),
        window.isMainWindow(),
        window.canBecomeKeyWindow(),
        window.canBecomeMainWindow(),
        window.isVisible(),
        window.occlusionState().contains(NSWindowOcclusionState::Visible),
        window.hasShadow(),
        window.isOpaque(),
        window.level(),
        window.collectionBehavior().0,
        window.parentWindow().map_or(0, |parent| parent.windowNumber()),
    )
}

fn focus_state() -> String {
    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    with_state(|state| {
        let mut out = format!(
            "app_active={} app_key_window={} app_main_window={}",
            app.isActive(),
            app.keyWindow().map_or(0, |window| window.windowNumber()),
            app.mainWindow().map_or(0, |window| window.windowNumber()),
        );
        if let Some(host) = &state.host {
            let responder = host.firstResponder().map_or("none".to_string(), |responder| {
                let class: *const AnyClass = unsafe { msg_send![&*responder, class] };
                unsafe { (*class).name().to_string_lossy().into_owned() }
            });
            // An editing NSTextField hands typing to the window's field editor, whose delegate is the field.
            let field_editing = state.field.as_ref().is_some_and(|field| unsafe {
                let editor: Option<Retained<AnyObject>> = msg_send![&**field, currentEditor];
                editor.is_some()
            });
            out += &format!(
                " host=#{} host_key={} host_main={} host_first_responder={responder} field_editing={field_editing}",
                host.windowNumber(),
                host.isKeyWindow(),
                host.isMainWindow()
            );
        }
        for (name, child) in [("browser", &state.main), ("popup", &state.popup)] {
            if let Some(child) = child {
                out += &format!(
                    " {name}=#{} {name}_key={} {name}_main={} {name}_cef_active={}",
                    child.ns.windowNumber(),
                    child.ns.isKeyWindow(),
                    child.ns.isMainWindow(),
                    child.window.is_active()
                );
            }
        }
        out
    })
}

fn geometry() -> String {
    with_state(|state| {
        let mut out = String::new();
        if let (Some(main), Some(frame)) = (&state.main, frame_for(Role::Main, state)) {
            out += &format!("region={} browser={} ", rect(frame), rect(main.ns.frame()));
            let bounds = main.window.bounds_in_screen();
            out += &format!("browser_cef_bounds=({},{} {}x{}) ", bounds.x, bounds.y, bounds.width, bounds.height);
        }
        if let (Some(popup), Some(frame)) = (&state.popup, frame_for(Role::Popup, state)) {
            out += &format!("popup_anchor={} popup={}", rect(frame), rect(popup.ns.frame()));
        }
        out
    })
}

/// The front-to-back order of the windows on screen around the host's, all apps' included.
fn window_order() -> String {
    let mtm = MainThreadMarker::new().expect("main thread");
    let numbers = NSWindow::windowNumbersWithOptions(NSWindowNumberListOptions::AllApplications, mtm);
    let names = with_state(|state| {
        let mut names = Vec::new();
        if let Some(host) = &state.host {
            names.push((host.windowNumber(), "host"));
        }
        if let Some(main) = &state.main {
            names.push((main.ns.windowNumber(), "browser"));
        }
        if let Some(popup) = &state.popup {
            names.push((popup.ns.windowNumber(), "popup"));
        }
        names
    });
    let Some(numbers) = numbers else { return "unknown".into() };
    let order: Vec<String> = numbers
        .iter()
        .take(12)
        .map(|number| {
            let number = number.integerValue();
            names.iter().find(|(own, _)| *own == number).map_or(format!("other#{number}"), |(_, name)| format!("{name}#{number}"))
        })
        .collect();
    order.join(" > ")
}

// --- CEF ---

wrap_app! {
    struct SpikeApp {}

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(SpikeBrowserProcess::new())
        }
    }
}

wrap_browser_process_handler! {
    struct SpikeBrowserProcess {}

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            log("context_initialized", "");
            open_child(Role::Main);
            if config().open_popup {
                toggle_popup();
            }
        }

        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            pump::schedule(delay_ms);
        }

        // The client of the browsers Chrome opens itself, like a tab an extension opens.
        fn default_client(&self) -> Option<Client> {
            Some(SpikeClient::new())
        }
    }
}

wrap_client! {
    struct SpikeClient {}

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(SpikeLifeSpan::new())
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(SpikeLoad::new())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(SpikeDisplay::new())
        }
    }
}

wrap_life_span_handler! {
    struct SpikeLifeSpan {}

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser else { return };
            let url = browser.main_frame().map(|frame| CefString::from(&frame.url()).to_string()).unwrap_or_default();
            let open = with_state(|state| {
                state.browsers.push(browser.clone());
                state.browsers.len()
            });
            log("browser_created", format!("id={} open={open} url={url}", browser.identifier()));
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            let id = browser.map_or(0, |browser| browser.identifier());
            let (open, quitting) = with_state(|state| {
                state.browsers.retain(|browser| browser.identifier() != id);
                (state.browsers.len(), state.quitting)
            });
            log("browser_closed", format!("id={id} open={open}"));
            if quitting && open == 0 {
                // Not from inside CEF's work: a `stop:` sent there didn't end the run loop.
                after(0, || stop_run_loop("all browsers closed"));
            }
        }
    }
}

wrap_load_handler! {
    struct SpikeLoad {}

    impl LoadHandler {
        fn on_load_end(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, http_status_code: i32) {
            let Some(frame) = frame.filter(|frame| frame.is_main() != 0) else { return };
            let url = CefString::from(&frame.url()).to_string();
            let first = !with_state(|state| std::mem::replace(&mut state.first_load_logged, true));
            log(
                "load_end",
                format!("id={} status={http_status_code} first={first} url={url}", browser.map_or(0, |browser| browser.identifier())),
            );
        }
    }
}

wrap_display_handler! {
    struct SpikeDisplay {}

    impl DisplayHandler {
        fn on_console_message(
            &self,
            browser: Option<&mut Browser>,
            level: LogSeverity,
            message: Option<&CefString>,
            source: Option<&CefString>,
            line: i32,
        ) -> i32 {
            let message: String = message.map(CefString::to_string).unwrap_or_default().chars().take(300).collect();
            let source = source.map(CefString::to_string).unwrap_or_default();
            log(
                "console",
                format!("id={} level={} {message} ({source}:{line})", browser.map_or(0, |browser| browser.identifier()), level.get_raw()),
            );
            0
        }
    }
}
