//! The web views as WebKit draws them: a `WKWebView` over GPUI's view in the window's content view, at its page's place.
//! Each plugin's pages share a store of cookies and site data of its own (`WKWebsiteDataStore(forIdentifier:)`, macOS
//! 14), or one kept only while Wings runs, and on macOS 15.4 a controller of the extensions added to it
//! (`WKWebExtensionController`).

use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::c_void,
    rc::Rc,
};

use block2::RcBlock;
use gpui::{Bounds, Pixels, Window};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_app_kit::{NSAlert, NSBitmapImageFileType, NSEvent, NSEventMask, NSResponder, NSBitmapImageRep, NSImage, NSModalResponse, NSOpenPanel, NSTextField, NSView, NSWindowOrderingMode};
use objc2_foundation::{
    NSJSONSerialization, NSJSONWritingOptions,
    NSArray, NSDate, NSDictionary, NSError, NSHTTPCookie, NSHTTPCookieDomain, NSHTTPCookieExpires, NSHTTPCookieName,
    NSHTTPCookiePath, NSHTTPCookieSameSitePolicy, NSHTTPCookieSameSiteLax, NSHTTPCookieSameSiteStrict,
    NSHTTPCookieSecure, NSHTTPCookieValue, NSKeyValueChangeKey, NSKeyValueObservingOptions,
    NSNumber, NSObjectNSKeyValueObserverRegistration, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect, NSSize, NSString, NSURL, NSURLRequest, NSUUID,
};
use objc2_web_kit::{
    WKContentWorld, WKFrameInfo, WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKOpenPanelParameters, WKScriptMessage,
    WKScriptMessageHandler, WKUserContentController, WKUserScript, WKUserScriptInjectionTime, WKNavigationDelegate, WKUIDelegate, WKWebExtension, WKWebExtensionContext,
    WKWebExtensionContextPermissionStatus, WKWebExtensionController, WKWebExtensionControllerConfiguration, WKWebView,
    WKWebViewConfiguration, WKWebsiteDataStore, WKWindowFeatures,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{
    Profile,
    views::{Heard, Maker, Native, State, Tell, make_with},
};
use crate::page::tree::WebGo;

mod actions;
mod data;
mod grant;
mod guard;

pub use data::{forget_profile, remove_extension};
use grant::granted;

/// What the web views say they are, which a change of any of them tells the page.
const OBSERVED: [&str; 5] = ["URL", "title", "loading", "canGoBack", "canGoForward"];

/// Has WebKit draw the web views of the pages in `window` from now on.
pub fn host_natively(window: &Window) {
    make_with(window.window_handle().window_id(), Rc::new(WebKit));
    keys_follow_presses();
}

/// A press anywhere in a window but a web view gives the keyboard back to what was pressed. A `WKWebView` keeps the
/// window's first responder after a press in it, and GPUI doesn't take it back, so keys typed into a terminal or a chat
/// after a press on it went to the web page, or nowhere once the page was hidden.
fn keys_follow_presses() {
    thread_local!(static ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    if ON.replace(true) {
        return;
    }
    let block = RcBlock::new(|event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        if let Some(mtm) = MainThreadMarker::new() {
            // SAFETY: AppKit hands the event it is about to send, alive for this call.
            give_keys_to_press(unsafe { event.as_ref() }, mtm);
        }
        event.as_ptr()
    });
    let mask = NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
    // SAFETY: the block returns the event it was given. The monitor lasts as long as the app, so it is never removed.
    std::mem::forget(unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) });
}

fn give_keys_to_press(event: &NSEvent, mtm: MainThreadMarker) {
    let Some(window) = event.window(mtm) else { return };
    LAST_PRESS.set(Some((std::ptr::from_ref(&*window).addr(), event.locationInWindow(), std::time::Instant::now())));
    let Some(content) = window.contentView() else { return };
    // SAFETY: AppKit's views, on the main thread.
    let at = match unsafe { content.superview() } {
        Some(frame) => frame.convertPoint_fromView(event.locationInWindow(), None),
        None => event.locationInWindow(),
    };
    // Hiding a web view that had the keyboard leaves it with the window, not GPUI's view, so whatever has it now, a
    // press outside the web views hands it to the view pressed.
    let pressed = content.hitTest(at);
    PRESSED_WEB_VIEW.set(pressed.as_deref().and_then(web_view_of).map_or(0, |view| std::ptr::from_ref(&*view).addr()));
    if let Some(pressed) = pressed
        && !in_web_view(&pressed)
        && window.firstResponder().is_none_or(|focused| !std::ptr::eq(&*focused, &**pressed))
    {
        window.makeFirstResponder(Some(&pressed));
    }
}

/// Whether `view` is a web view or inside one.
fn in_web_view(view: &NSView) -> bool {
    web_view_of(view).is_some()
}

/// The web view `view` is or is inside.
fn web_view_of(view: &NSView) -> Option<Retained<NSView>> {
    let mut at = Some(view.retain());
    while let Some(view) = at {
        if view.isKindOfClass(WKWebView::class()) {
            return Some(view);
        }
        // SAFETY: AppKit's views, on the main thread.
        at = unsafe { view.superview() };
    }
    None
}

thread_local! {
    /// The web view the last press of the mouse was in, by its address, 0 when it was in none.
    static PRESSED_WEB_VIEW: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The window of the last press, by its address, where in it, and when: an extension's popup opens under the button
    /// pressed (`actions::anchor`).
    static LAST_PRESS: std::cell::Cell<Option<(usize, NSPoint, std::time::Instant)>> = const { std::cell::Cell::new(None) };
}

impl WebView {
    fn may_take_keys(&self) -> bool {
        !self.isHidden() && PRESSED_WEB_VIEW.get() == std::ptr::from_ref(self).addr()
    }
}

/// GPUI's view among the views of the window's `content` view. The vibrancy and the backdrop are views there too, under
/// it, and handing them the keyboard swallowed every key, so it's picked by its class, not its place.
fn gpui_among(content: &NSView) -> Option<Retained<NSView>> {
    let class = objc2::runtime::AnyClass::get(c"GPUIView")?;
    content.subviews().iter().find(|view| view.isKindOfClass(class))
}

define_class!(
    // SAFETY: WKWebView may be subclassed, and this has no ivars and no `Drop`.
    #[unsafe(super(WKWebView, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct WebView;

    impl WebView {
        /// A page takes the keyboard only once it was pressed and while it shows. WebKit takes it for a page that
        /// focuses a field of its own as it loads, so a search page loading in a hidden tab took the keys typed into a
        /// terminal or a chat, and they went nowhere.
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> Bool {
            if !self.may_take_keys() {
                return Bool::NO;
            }
            // SAFETY: NSResponder's own method, on the main thread.
            unsafe { msg_send![super(self), acceptsFirstResponder] }
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> Bool {
            if !self.may_take_keys() {
                // What had the keyboard has given it up by now, which leaves it with the window: once this call is over,
                // GPUI's view takes it back.
                // SAFETY: AppKit's views, on the main thread.
                if let (Some(window), Some(gpui)) = (self.window(), unsafe { self.superview() }.as_deref().and_then(gpui_among)) {
                    // SAFETY: `makeFirstResponder:` takes one object, and the window keeps the view alive until then.
                    let _: () = unsafe { msg_send![&*window, performSelector: objc2::sel!(makeFirstResponder:), withObject: &*gpui, afterDelay: 0.0f64] };
                }
                return Bool::NO;
            }
            // SAFETY: NSResponder's own method, on the main thread.
            unsafe { msg_send![super(self), becomeFirstResponder] }
        }
    }
);

/// Hands the keyboard to `to` when `view` has it, before `view` is hidden or taken away, so typing doesn't go nowhere.
fn give_keys_away(view: &WKWebView, to: &NSView) {
    let Some(window) = view.window() else { return };
    if let Some(focused) = window.firstResponder().and_then(|responder| responder.downcast::<NSView>().ok())
        && focused.isDescendantOf(view)
    {
        window.makeFirstResponder(Some(to));
    }
}

/// What the plugins' web views share, by profile and whether they keep their data.
struct Shared {
    store: Retained<WKWebsiteDataStore>,
    /// Its extensions' controller, on a Mac that has them (15.4).
    extensions: Option<Retained<WKWebExtensionController>>,
    /// The controller's delegate, which the controller holds weakly: its window of the pages, and their buttons' popups.
    tabs: Option<Retained<actions::Delegate>>,
}

thread_local! {
    static SHARED: RefCell<HashMap<(Profile, bool), Rc<Shared>>> = RefCell::default();
    /// Each profile's extensions loaded now, by their id, to unload one turned off or removed.
    static LOADED: RefCell<HashMap<(Profile, bool, String), Retained<WKWebExtensionContext>>> = RefCell::default();
    /// The extensions WebKit is reading now, and how many times each was unloaded, so a read that ends after an unload
    /// loads nothing.
    static LOADING: RefCell<std::collections::HashSet<(Profile, bool, String)>> = RefCell::default();
    static UNLOADS: RefCell<HashMap<(Profile, String), u64>> = RefCell::default();
}

/// Whether this Mac runs macOS `major.minor` or newer. By the system's version, not by WebKit's classes: those are
/// only there once WebKit is loaded, which a process that hasn't made a web view yet (the screenshots, the tests) said
/// "no" to on a Mac that has them.
fn macos_at_least(major: isize, minor: isize) -> bool {
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion { majorVersion: major, minorVersion: minor, patchVersion: 0 })
}

/// Whether this Mac has WebKit's extensions, which came with macOS 15.4.
pub fn extensions_supported() -> bool {
    macos_at_least(15, 4)
}

/// Whether WebKit keeps a store for each plugin (`dataStoreForIdentifier:`, macOS 14).
pub fn keeps_per_plugin() -> bool {
    macos_at_least(14, 0)
}

fn shared(profile: &Profile, keep_data: bool, mtm: MainThreadMarker) -> Rc<Shared> {
    if let Some(shared) = SHARED.with_borrow(|all| all.get(&(profile.clone(), keep_data)).cloned()) {
        return shared;
    }
    let uuid = NSUUID::from_string(&NSString::from_str(&profile.uuid_text())).expect("a profile's UUID is valid");
    // SAFETY: WebKit's constructors, on the main thread; the identifier is a valid UUID (`Profile::uuid`).
    let store = unsafe {
        match keep_data && keeps_per_plugin() {
            true => WKWebsiteDataStore::dataStoreForIdentifier(&uuid, mtm),
            // Before macOS 14 the only kept store is the app's default one, which every web view in the app could read:
            // a plugin's cookies are kept only while Wings runs instead (`keeps_refusal`).
            false => WKWebsiteDataStore::nonPersistentDataStore(mtm),
        }
    };
    let extensions = extensions_supported().then(|| unsafe {
        let configuration = if keep_data {
            WKWebExtensionControllerConfiguration::configurationWithIdentifier(&uuid, mtm)
        } else {
            WKWebExtensionControllerConfiguration::nonPersistentConfiguration(mtm)
        };
        configuration.setDefaultWebsiteDataStore(Some(&store));
        // Its extensions' own pages name a browser (`EXTENSION_PAGES_AGENT`). Read after the store is set: WebKit puts that
        // store in the pages' configuration as it makes it.
        let pages = configuration.webViewConfiguration();
        pages.setApplicationNameForUserAgent(Some(&NSString::from_str(EXTENSION_PAGES_AGENT)));
        configuration.setWebViewConfiguration(Some(&pages));
        WKWebExtensionController::initWithConfiguration(WKWebExtensionController::alloc(mtm), &configuration)
    });
    // Before any extension loads, which asks it for the pages.
    let tabs = extensions.as_ref().map(|controller| {
        let delegate = actions::Delegate::new(mtm);
        // SAFETY: the controller's delegate, kept in `Shared` as long as the controller.
        unsafe { controller.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        delegate
    });
    let shared = Rc::new(Shared { store, extensions, tabs });
    SHARED.with_borrow_mut(|all| all.insert((profile.clone(), keep_data), shared.clone()));
    if shared.extensions.is_some() {
        load_verified(profile, keep_data);
    }
    shared
}

/// What an extension's own pages (its background, popup and options) add to WebKit's user agent: Chrome's words, as most
/// extensions added come from Chromium browsers and are built for Chrome. WebKit's own names no browser, and an extension
/// that tells the browser by it found none: Bitwarden's background and popup both stopped at `getDevice().toString()`,
/// which left its popup on a spinner. Safari's words send a Chrome build down the paths meant for its Safari build, where
/// Bitwarden's background looped for good.
// debt: Chrome's stable version in November 2026, fixed; an extension that wants a newer Chrome by it needs this moved up.
const EXTENSION_PAGES_AGENT: &str = "Chrome/159.0.0.0 Safari/537.36";

/// Loads the extensions of `profile` that are on and pass `extensions::verify`, checked off the main thread: hashing
/// a big one's folder (Bitwarden's 92 MB) takes 60 ms in a release build and a second in a debug one, and the Keychain
/// may wait for the person. One turned off or removed while it was checked doesn't load.
fn load_verified(profile: &Profile, keep_data: bool) {
    let unloads: HashMap<String, u64> = UNLOADS.with_borrow(|unloads| {
        unloads.iter().filter(|((of, _), _)| of == profile).map(|((_, id), count)| (id.clone(), *count)).collect()
    });
    let profile = profile.clone();
    std::thread::spawn(move || {
        super::extensions::sweep_staging(&profile);
        let verified = super::extensions::loadable(&profile, &|| super::extensions::key(&profile));
        dispatch2::DispatchQueue::main().exec_async(move || {
            for extension in verified {
                let count = UNLOADS.with_borrow(|now| now.get(&(profile.clone(), extension.id.clone())).copied().unwrap_or(0));
                if count == unloads.get(&extension.id).copied().unwrap_or(0) {
                    let again = profile.clone();
                    load(&profile, keep_data, &extension, move |_| refresh_buttons(&again));
                }
            }
        });
    });
}

/// Tells each page of `profile` its extensions' buttons again: one was loaded or unloaded.
pub fn refresh_buttons(profile: &Profile) {
    for keep_data in [true, false] {
        let delegate = SHARED.with_borrow(|all| all.get(&(profile.clone(), keep_data)).and_then(|shared| shared.tabs.clone()));
        for tab in delegate.iter().flat_map(|delegate| delegate.tabs()) {
            actions::refresh(&tab);
        }
    }
}

/// Loads `extension`, which passed `extensions::verify`, into `profile`'s controller, granting of what it asks for only
/// what Wings' dialog showed the person as they added it; `done` hears whether it loaded. Nothing for a profile no web
/// view has used yet: its controller loads its extensions as it is made.
pub fn load(profile: &Profile, keep_data: bool, extension: &super::extensions::Verified, done: impl Fn(Result<(), String>) + 'static) {
    let (id, dir) = (extension.id.as_str(), extension.dir.as_path());
    let allowed = (extension.permissions.clone(), extension.hosts.clone());
    let Some(controller) =
        SHARED.with_borrow(|all| all.get(&(profile.clone(), keep_data)).and_then(|shared| shared.extensions.clone()))
    else {
        return done(Ok(()));
    };
    let Some(mtm) = MainThreadMarker::new() else { return };
    let key = (profile.clone(), keep_data, id.to_owned());
    // Once: a second load ran a second copy of it, which turning it off left running.
    if LOADED.with_borrow(|loaded| loaded.contains_key(&key)) || !LOADING.with_borrow_mut(|loading| loading.insert(key.clone())) {
        return done(Ok(()));
    }
    let unloads = UNLOADS.with_borrow(|unloads| unloads.get(&(profile.clone(), id.to_owned())).copied().unwrap_or(0));
    let url = NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy()));
    let (profile, id) = (profile.clone(), id.to_owned());
    let loaded = RcBlock::new(move |extension: *mut WKWebExtension, error: *mut NSError| {
        LOADING.with_borrow_mut(|loading| loading.remove(&(profile.clone(), keep_data, id.clone())));
        // Turned off or removed while WebKit read it: it doesn't load after all.
        if UNLOADS.with_borrow(|unloads| unloads.get(&(profile.clone(), id.clone())).copied().unwrap_or(0)) != unloads {
            return done(Err("It was turned off before it loaded".into()));
        }
        // SAFETY: WebKit hands one of the two, alive for the call.
        let Some(extension) = (unsafe { extension.as_ref() }) else {
            let why = unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string());
            return done(Err(why.unwrap_or_else(|| "WebKit couldn't read the extension".into())));
        };
        // SAFETY: WebKit's own objects on the main thread, where its completion handlers run.
        let result = unsafe {
            let context = WKWebExtensionContext::contextForExtension(extension);
            // Its storage stays its own across runs, by its id here.
            context.setUniqueIdentifier(&NSString::from_str(&id));
            let (permissions, hosts) = granted(&extension.requestedPermissions(), &extension.requestedPermissionMatchPatterns(), &allowed, mtm);
            for permission in permissions {
                context.setPermissionStatus_forPermission(WKWebExtensionContextPermissionStatus::GrantedExplicitly, &permission);
            }
            for pattern in hosts {
                context.setPermissionStatus_forMatchPattern(WKWebExtensionContextPermissionStatus::GrantedExplicitly, &pattern);
            }
            controller.loadExtensionContext_error(&context).map(|()| context)
        };
        match result {
            Ok(context) => {
                LOADED.with_borrow_mut(|loaded| loaded.insert((profile.clone(), keep_data, id.clone()), context));
                done(Ok(()))
            }
            Err(error) => done(Err(error.localizedDescription().to_string())),
        }
    });
    // SAFETY: a file URL of a folder, and a block WebKit calls once on the main thread.
    unsafe { WKWebExtension::extensionWithResourceBaseURL_completionHandler(&url, &loaded, mtm) };
}

/// Unloads the extension `id` from every web view of `profile`.
pub fn unload(profile: &Profile, id: &str) {
    UNLOADS.with_borrow_mut(|unloads| *unloads.entry((profile.clone(), id.to_owned())).or_default() += 1);
    for keep_data in [true, false] {
        let Some(context) = LOADED.with_borrow_mut(|loaded| loaded.remove(&(profile.clone(), keep_data, id.to_owned()))) else {
            continue;
        };
        // SAFETY: the context's own controller, on the main thread.
        if let Some(controller) = unsafe { context.webExtensionController() } {
            let _ = unsafe { controller.unloadExtensionContext_error(&context) };
        }
    }
}

/// Loads `extension` into each controller `profile` has now.
pub fn load_everywhere(profile: &Profile, extension: &super::extensions::Verified, done: impl Fn(Result<(), String>) + Clone + 'static) {
    for keep_data in [true, false] {
        load(profile, keep_data, extension, done.clone());
    }
}

/// Removes every cookie and all site data `profile`'s web views kept, in both its stores, then calls `done`.
pub fn clear_data(profile: &Profile, done: impl Fn() + 'static) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let done = Rc::new(done);
    for keep_data in [true, false] {
        let store = shared(profile, keep_data, mtm).store.clone();
        let done = done.clone();
        let block = RcBlock::new(move || done());
        // SAFETY: all of the store's kinds of data, since the start of time; WebKit calls the block on the main thread.
        unsafe {
            store.removeDataOfTypes_modifiedSince_completionHandler(
                &WKWebsiteDataStore::allWebsiteDataTypes(mtm),
                &NSDate::distantPast(),
                &block,
            )
        };
    }
}

/// A cookie to put in a store, from another browser (`wings_browser_cookies`).
pub struct Cookie<'a> {
    pub domain: &'a str,
    pub name: &'a str,
    pub value: &'a str,
    pub path: &'a str,
    pub expires: Option<f64>,
    pub secure: bool,
    pub http_only: bool,
    /// "lax", "strict" or none.
    pub same_site: Option<&'a str>,
}

/// Puts `cookies` in `profile`'s store, kept or not, then calls `done` with how many it took. A cookie WebKit can't
/// make (a name it refuses) is left out.
pub fn set_cookies(profile: &Profile, keep_data: bool, cookies: &[Cookie], done: impl Fn(usize) + 'static) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    // SAFETY: the store's cookie store, on the main thread.
    let jar = unsafe { shared(profile, keep_data, mtm).store.httpCookieStore() };
    let made: Vec<Retained<NSHTTPCookie>> = cookies.iter().filter_map(cookie).collect();
    let left = Rc::new(std::cell::Cell::new(made.len()));
    let done = Rc::new(done);
    if made.is_empty() {
        return done(0);
    }
    // What the store holds after, not what was handed to it: it may refuse one (a domain it won't take).
    let wanted: Rc<std::collections::HashSet<(String, String, String)>> = Rc::new(made.iter().map(|made| key_of(made)).collect());
    for made in made {
        let (left, done, store, wanted) = (left.clone(), done.clone(), jar.clone(), wanted.clone());
        let block = RcBlock::new(move || {
            left.set(left.get() - 1);
            if left.get() == 0 {
                let (done, wanted) = (done.clone(), wanted.clone());
                let counted = RcBlock::new(move |all: std::ptr::NonNull<NSArray<NSHTTPCookie>>| {
                    // SAFETY: the store's cookies, which WebKit hands over for the call.
                    let all = unsafe { all.as_ref() };
                    done(all.iter().filter(|kept| wanted.contains(&key_of(kept))).count());
                });
                // SAFETY: a block WebKit calls once on the main thread.
                unsafe { store.getAllCookies(&counted) };
            }
        });
        // SAFETY: a cookie made just now, and a block WebKit calls once on the main thread.
        unsafe { jar.setCookie_completionHandler(&made, Some(&block)) };
    }
}

/// A cookie by its domain, name and path, which is what makes one cookie in a store.
fn key_of(cookie: &NSHTTPCookie) -> (String, String, String) {
    (cookie.domain().to_string(), cookie.name().to_string(), cookie.path().to_string())
}

fn cookie(cookie: &Cookie) -> Option<Retained<NSHTTPCookie>> {
    let string = |text: &str| Retained::into_super(Retained::into_super(NSString::from_str(text)));
    // SAFETY: Foundation's own property keys, which are statics.
    let mut keys = unsafe {
        vec![NSHTTPCookieDomain, NSHTTPCookieName, NSHTTPCookieValue, NSHTTPCookiePath]
    };
    let mut values: Vec<Retained<AnyObject>> =
        vec![string(cookie.domain), string(cookie.name), string(cookie.value), string(if cookie.path.is_empty() { "/" } else { cookie.path })];
    if let Some(expires) = cookie.expires {
        keys.push(unsafe { NSHTTPCookieExpires });
        values.push(Retained::into_super(Retained::into_super(NSDate::dateWithTimeIntervalSince1970(expires))));
    }
    if cookie.secure {
        keys.push(unsafe { NSHTTPCookieSecure });
        values.push(string("TRUE"));
    }
    // Foundation documents no key for HttpOnly; "HttpOnly" is the one `NSHTTPCookie.properties` itself reports for such
    // a cookie, which `cookieWithProperties` reads back (unverified against Apple's docs).
    let http_only = NSString::from_str("HttpOnly");
    if cookie.http_only {
        keys.push(&http_only);
        values.push(string("TRUE"));
    }
    match cookie.same_site {
        Some("lax") => {
            keys.push(unsafe { NSHTTPCookieSameSitePolicy });
            values.push(string(&unsafe { NSHTTPCookieSameSiteLax }.to_string()));
        }
        Some("strict") => {
            keys.push(unsafe { NSHTTPCookieSameSitePolicy });
            values.push(string(&unsafe { NSHTTPCookieSameSiteStrict }.to_string()));
        }
        _ => {}
    }
    let values: Vec<&AnyObject> = values.iter().map(|value| &**value).collect();
    let properties = NSDictionary::<NSString, AnyObject>::from_slices(&keys, &values);
    // SAFETY: a dictionary of the cookie's properties by Foundation's keys.
    unsafe { NSHTTPCookie::cookieWithProperties(&properties) }
}

struct WebKit;

impl Maker for WebKit {
    fn make(&self, profile: &Profile, keep_data: bool, scripts: bool, url: Option<&str>, told: Tell) -> Box<dyn Native> {
        let mtm = MainThreadMarker::new().expect("web views are made on the main thread");
        let shared = shared(profile, keep_data, mtm);
        // SAFETY: WebKit's objects, made and used on the main thread.
        let view = unsafe {
            let configuration = WKWebViewConfiguration::new(mtm);
            configuration.setWebsiteDataStore(&shared.store);
            if let Some(extensions) = &shared.extensions {
                configuration.setWebExtensionController(Some(extensions));
            }
            // In the page's own world, the only one that sees its `console` and `fetch`; it posts what it sees, and what
            // comes from a site signed in with imported cookies is dropped as it arrives (`heard`). It names no such site.
            if scripts {
                let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                    WKUserScript::alloc(mtm),
                    &NSString::from_str(LOGS),
                    WKUserScriptInjectionTime::AtDocumentStart,
                    true,
                    &WKContentWorld::pageWorld(mtm),
                );
                configuration.userContentController().addUserScript(&script);
                // Every frame tells Wings, in a world of its own, once it holds a secret field (`guard`).
                let watch = WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                    WKUserScript::alloc(mtm),
                    &NSString::from_str(guard::WATCH),
                    WKUserScriptInjectionTime::AtDocumentEnd,
                    false,
                    &WKContentWorld::worldWithName(&NSString::from_str(guard::WORLD), mtm),
                );
                configuration.userContentController().addUserScript(&watch);
            }
            // `WebView` is a `WKWebView` that adds nothing but a rule for taking the keyboard.
            let view: Retained<WebView> = msg_send![WebView::alloc(mtm), initWithFrame: NSRect::ZERO, configuration: &*configuration];
            let view = view.into_super();
            // So a plugin's author can look at a page with Safari's Web Inspector, in a build of their own only: a release
            // build's pages, signed in as they may be, aren't open to it.
            view.setInspectable(cfg!(debug_assertions));
            view.setHidden(true);
            view
        };
        let observer = Observer::new(told.clone(), profile.dir.clone(), mtm);
        // A tab to the extensions, whose buttons the page hears.
        if let (Some(delegate), Some(controller)) = (&shared.tabs, &shared.extensions) {
            let tab = actions::open(delegate, controller, &view, &observer, &profile.plugin);
            observer.ivars().tab.replace(Some(tab.clone()));
            actions::refresh(&tab);
        }
        if scripts {
            // SAFETY: the configuration's controller holds the handler until the view goes (`Drop` takes it away).
            unsafe {
                let contents = view.configuration().userContentController();
                contents.addScriptMessageHandler_contentWorld_name(ProtocolObject::from_ref(&*observer), &WKContentWorld::pageWorld(mtm), &NSString::from_str(LOG_HANDLER));
                // Only Wings' own world reaches it: the page can't say it holds no secret field, nor reach it at all.
                contents.addScriptMessageHandler_contentWorld_name(
                    ProtocolObject::from_ref(&*observer),
                    &WKContentWorld::worldWithName(&NSString::from_str(guard::WORLD), mtm),
                    &NSString::from_str(guard::HANDLER),
                );
            };
        }
        // SAFETY: the observer outlives the observing: `Drop` removes it before either goes.
        unsafe {
            for key in OBSERVED {
                view.addObserver_forKeyPath_options_context(
                    &observer,
                    &NSString::from_str(key),
                    NSKeyValueObservingOptions::empty(),
                    std::ptr::null_mut(),
                );
            }
            view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*observer)));
            view.setUIDelegate(Some(ProtocolObject::from_ref(&*observer)));
        }
        let (shots, web_dir) = (profile.dir.join("screenshots"), profile.dir.clone());
        let mut native = WebPage { view, observer, told, shots, web_dir, scripts };
        if let Some(url) = url {
            native.go(&WebGo::Load(url.to_owned()), 0);
        }
        Box::new(native)
    }
}

struct WebPage {
    view: Retained<WKWebView>,
    observer: Retained<Observer>,
    told: Tell,
    /// Where its snapshots are saved, in its plugin's web folder, and that folder.
    shots: std::path::PathBuf,
    web_dir: std::path::PathBuf,
    /// Its plugin may read and act on its pages (`webScripts`).
    scripts: bool,
}

/// The name the page's log script posts to.
const LOG_HANDLER: &str = "wingsLog";
/// The most console messages and requests kept of each page.
const LOGS_KEPT: usize = 200;

/// Sees the page's console messages, uncaught errors and `fetch`/`XMLHttpRequest` calls (method, address, status),
/// never a body or a header, and posts each to Wings. It can't see the page's own loads (navigations, pictures, styles,
/// scripts), a service worker's requests or web sockets.
const LOGS: &str = r#"(() => {
  const post = (m) => { try { webkit.messageHandlers.wingsLog.postMessage(m); } catch (_) {} };
  const text = (a) => a.map((x) => { try { return typeof x === "string" ? x : JSON.stringify(x); } catch (_) { return String(x); } }).join(" ").slice(0, 2000);
  for (const level of ["log", "info", "warn", "error", "debug"]) {
    const f = console[level];
    console[level] = function (...a) { post({ kind: "console", level, text: text(a) }); return f.apply(this, a); };
  }
  addEventListener("error", (e) => post({ kind: "console", level: "error", text: String(e.message).slice(0, 2000) }));
  addEventListener("unhandledrejection", (e) => post({ kind: "console", level: "error", text: ("Unhandled rejection: " + String(e.reason)).slice(0, 2000) }));
  const fetch0 = window.fetch;
  window.fetch = async function (input, init) {
    const method = String((init && init.method) || (input && input.method) || "GET");
    const url = String((input && input.url) || input).slice(0, 2000);
    try { const r = await fetch0.apply(this, arguments); post({ kind: "network", method, url, status: r.status }); return r; }
    catch (e) { post({ kind: "network", method, url, status: 0, error: String(e) }); throw e; }
  };
  const open0 = XMLHttpRequest.prototype.open, send0 = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (m, u) { this.__wings = { method: String(m), url: String(u).slice(0, 2000) }; return open0.apply(this, arguments); };
  XMLHttpRequest.prototype.send = function () { this.addEventListener("loadend", () => this.__wings && post({ kind: "network", ...this.__wings, status: this.status })); return send0.apply(this, arguments); };
})();"#;

impl Native for WebPage {
    fn place(&mut self, window: &Window, frame: Option<Bounds<Pixels>>) {
        let Some(frame) = frame else {
            if let Some(gpui) = gpui_view(window) {
                give_keys_away(&self.view, &gpui);
            }
            self.view.setHidden(true);
            return;
        };
        let Some(gpui) = gpui_view(window) else { return };
        // SAFETY: AppKit's views, on the main thread.
        unsafe {
            let Some(content) = gpui.superview() else { return };
            if self.view.superview().as_deref() != Some(&*content) {
                attach(&self.view, &content, &gpui);
            }
            // GPUI counts from the window's top; an unflipped view counts from its bottom.
            let height = content.bounds().size.height;
            let (x, y) = (f64::from(f32::from(frame.origin.x)), f64::from(f32::from(frame.origin.y)));
            let (w, h) = (f64::from(f32::from(frame.size.width)), f64::from(f32::from(frame.size.height)));
            let y = if content.isFlipped() { y } else { height - y - h };
            self.view.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)));
        }
        // The page that comes on screen is the one in front, to the extensions.
        if self.view.isHidden()
            && let Some(tab) = self.observer.ivars().tab.borrow().as_ref()
        {
            actions::activate(tab);
        }
        self.view.setHidden(false);
    }

    fn go(&mut self, go: &WebGo, token: u64) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        // The document it would act on decides, as `gate` says for every command: the page committed, while no other is
        // on its way, or for a dialog its own frame, which counts as imported when it isn't known.
        // SAFETY: the web view's address and state, on the main thread.
        let (url, loading) = unsafe { (self.view.URL().and_then(|url| url.absoluteString()).map(|url| url.to_string()).unwrap_or_default(), self.view.isLoading()) };
        let committed = self.observer.ivars().committed.borrow().clone();
        let (imported, settled) = match go {
            // The page's own site and the dialog's frame's origin, either imported or unknown.
            WebGo::Dialog { .. } => {
                let frame = self.observer.ivars().dialog_frame.borrow().clone();
                let frame = frame.is_none_or(|host| super::holds_imported_host(&self.web_dir, &host));
                (frame || super::holds_imported(&self.web_dir, &committed), true)
            }
            // A page moving about within itself (`pushState`, a `#`) keeps its origin, and is the same page to act on.
            _ => (super::holds_imported(&self.web_dir, &committed), !loading && super::origin(&committed) == super::origin(&url)),
        };
        if let Err(why) = super::gate(go, self.scripts, imported, settled) {
            return self.told.tell(Heard::Result { token, result: Err(why.into()) });
        }
        // SAFETY: the web view's own calls, on the main thread.
        unsafe {
            match go {
                WebGo::Load(url) => {
                    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
                        self.view.loadRequest(&NSURLRequest::requestWithURL(&url));
                    }
                }
                WebGo::Back => drop(self.view.goBack()),
                WebGo::Forward => drop(self.view.goForward()),
                WebGo::Reload => drop(self.view.reload()),
                WebGo::Stop => self.view.stopLoading(),
                // The plugin's own world: its helper, which reads no secret field's value.
                WebGo::Eval { script, page: false } => {
                    let world = WKContentWorld::worldWithName(&NSString::from_str("wings-plugin"), mtm);
                    eval(&self.view, script, &world, &self.told, &self.observer, token);
                }
                // The page's own world, where an agent's script runs: only on a page with no password, card or one-time
                // code field (`guard`), checked right before, and again by the script itself as it starts.
                WebGo::Eval { script, page: true } => {
                    let (view, told, observer, script) = (self.view.clone(), self.told.clone(), self.observer.clone(), guard::checked_again(script));
                    let sent = observer.ivars().generation.get();
                    guard::check(&self.view, mtm, move |found, took| {
                        if found.as_ref().is_ok_and(|found| found["secret"] == true) {
                            observer.ivars().seen.note(sent, false);
                        }
                        let seen = observer.ivars().seen.secret(sent);
                        let allowed = if observer.ivars().generation.get() != sent { Err(MOVED_ON) } else { guard::may_run(&found, took, seen) };
                        match allowed {
                            Ok(()) => eval(&view, &script, &WKContentWorld::pageWorld(mtm), &told, &observer, token),
                            Err(why) => told.tell(Heard::Result { token, result: Err(why.into()) }),
                        }
                    });
                }
                WebGo::Dialog { accept, text } => {
                    let answered = self.observer.answer(*accept, text.as_deref());
                    self.observer.tell(&self.view);
                    let result = if answered { Ok(serde_json::Value::Null) } else { Err("The page shows no dialog".into()) };
                    self.told.tell(Heard::Result { token, result });
                }
                WebGo::Extension(id) => {
                    let tab = self.observer.ivars().tab.borrow().clone();
                    let done = tab.map_or(Err(actions::NOT_ON.to_owned()), |tab| actions::perform(&tab, &self.view, id));
                    self.told.tell(Heard::Result { token, result: done.map(|()| serde_json::Value::Null) });
                }
                WebGo::Logs => {
                    let logs = self.observer.ivars().logs.borrow();
                    let of = |kind: &str| logs.iter().filter(|line| line["kind"] == kind).cloned().collect::<Vec<_>>();
                    let result = Ok(serde_json::json!({ "console": of("console"), "network": of("network") }));
                    self.told.tell(Heard::Result { token, result });
                }
                WebGo::Snapshot => {
                    let (told, shots, observer, web_dir) = (self.told.clone(), self.shots.clone(), self.observer.clone(), self.web_dir.clone());
                    let sent = observer.ivars().generation.get();
                    // Its secret fields are painted over where they were just before and just after it was taken.
                    let (view, wide, zoom) = (self.view.clone(), self.view.bounds().size.width, self.view.pageZoom());
                    guard::check(&self.view, mtm, move |before, _| {
                        let before = match guard::rects(&before, observer.ivars().seen.foreign(sent)) {
                            Ok(before) => before,
                            Err(why) => return told.tell(Heard::Result { token, result: Err(why.into()) }),
                        };
                        let (told, shots, observer, web_dir, again) = (told.clone(), shots.clone(), observer.clone(), web_dir.clone(), view.clone());
                        let done = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                            // As a script's answer: none of a page that came while it was taken, or one signed in with imports.
                            let moved = observer.ivars().generation.get() != sent;
                            let imported = super::holds_imported(&web_dir, &observer.ivars().committed.borrow());
                            let picture = match (image.as_ref(), error.as_ref()) {
                                _ if moved => Err(MOVED_ON.to_owned()),
                                _ if imported => Err(super::IMPORTED.to_owned()),
                                (Some(image), _) => png_of(image),
                                (None, Some(error)) => Err(error.localizedDescription().to_string()),
                                (None, None) => Err("WebKit made no picture".into()),
                            };
                            let (told, shots, before, observer) = (told.clone(), shots.clone(), before.clone(), observer.clone());
                            guard::check(&again, mtm, move |after, _| {
                                let foreign = observer.ivars().seen.foreign(sent);
                                let rects = guard::rects(&after, foreign).map(|after| [before.clone(), after].concat()).map_err(str::to_owned);
                                // Painted, scaled and written off the main thread: a big picture took the window's frames with it.
                                let (told, shots, picture) = (told.clone(), shots.clone(), picture.clone());
                                std::thread::spawn(move || {
                                    let result = rects
                                        .and_then(|rects| picture.and_then(|png| guard::masked(&png, &rects, wide, zoom)))
                                        .and_then(|png| super::snapshot::save(&shots, &png));
                                    told.tell(Heard::Result { token, result });
                                });
                            });
                        });
                        // The part of the page that shows: WebKit takes no more than the view's bounds.
                        view.takeSnapshotWithConfiguration_completionHandler(None, &done);
                    });
                }
            }
        }
    }
}

/// Runs `script` in `world` of `view`'s main frame and tells its answer as the command `token`'s. The script is the
/// function's whole body, with no wrapper of ours to break out of, and nothing it says of where it ran is believed: what
/// it returns is read as WebKit hands it over, and thrown away when a new page came on the main frame while it ran
/// (`generation`).
fn eval(view: &WKWebView, script: &str, world: &WKContentWorld, told: &Tell, observer: &Retained<Observer>, token: u64) {
    let (told, observer) = (told.clone(), observer.clone());
    let sent = observer.ivars().generation.get();
    let done = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        // SAFETY: WebKit hands one of the two, alive for the call, on the main thread.
        let result = match unsafe { (value.as_ref(), error.as_ref()) } {
            _ if observer.ivars().generation.get() != sent => Err(MOVED_ON.to_owned()),
            (_, Some(error)) => Err(error.localizedDescription().to_string()),
            (Some(value), None) => json_of(value),
            (None, None) => Ok(serde_json::Value::Null),
        };
        told.tell(Heard::Result { token, result });
    });
    // SAFETY: the web view's own call, on the main thread, with a block WebKit calls once.
    unsafe { view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(&NSString::from_str(script), None, None, world, Some(&done)) };
}

/// Why a script's answer was thrown away.
const MOVED_ON: &str = "The page moved on while the script ran, so its answer was dropped: try again";

/// What a script returned, as JSON, read by Foundation from what WebKit made of it. One that isn't JSON (a date, a node)
/// is an error rather than an exception, which would end the app: Foundation checks it first, inside an array, so a
/// single string or number is checked too.
fn json_of(value: &AnyObject) -> Result<serde_json::Value, String> {
    let wrapped = NSArray::from_slice(&[value]);
    // SAFETY: an array of what WebKit returned, checked before it is written.
    unsafe {
        if !NSJSONSerialization::isValidJSONObject(&wrapped) {
            return Err("The script returned something that isn't JSON: return plain values, arrays and objects".into());
        }
        let data = NSJSONSerialization::dataWithJSONObject_options_error(&wrapped, NSJSONWritingOptions::empty())
            .map_err(|error| error.localizedDescription().to_string())?;
        let array: serde_json::Value = serde_json::from_slice(&data.to_vec()).map_err(|error| error.to_string())?;
        Ok(array[0].clone())
    }
}

/// The PNG of `image`, by way of its bitmap.
fn png_of(image: &NSImage) -> Result<Vec<u8>, String> {
    // SAFETY: AppKit's conversions of an image WebKit made, on the main thread.
    unsafe {
        let tiff = image.TIFFRepresentation().ok_or("The picture has no bitmap")?;
        let bitmap = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("The picture has no bitmap")?;
        let png = bitmap
            .representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
            .ok_or("The picture couldn't be made a PNG")?;
        Ok(png.to_vec())
    }
}

impl Drop for WebPage {
    fn drop(&mut self) {
        // WebKit waits on a dialog's answer, so one still open is answered as Cancel.
        self.observer.answer(false, None);
        // SAFETY: the observer added in `make`, removed once.
        unsafe {
            self.view.configuration().userContentController().removeAllScriptMessageHandlers();
            for key in OBSERVED {
                self.view.removeObserver_forKeyPath(&self.observer, &NSString::from_str(key));
            }
            self.view.setNavigationDelegate(None);
            self.view.setUIDelegate(None);
            self.view.stopLoading();
        }
        if let Some(tab) = self.observer.ivars().tab.take() {
            actions::close(&tab);
        }
        // GPUI's view is the window's other view, under the web views.
        // SAFETY: AppKit's views, on the main thread.
        if let Some(gpui) = unsafe { self.view.superview() }.as_deref().and_then(gpui_among) {
            give_keys_away(&self.view, &gpui);
        }
        self.view.removeFromSuperview();
    }
}

/// Puts `view` in the window's `content` view, over GPUI's view `gpui`.
pub unsafe fn attach(view: &WKWebView, content: &NSView, gpui: &NSView) {
    content.addSubview_positioned_relativeTo(view, NSWindowOrderingMode::Above, Some(gpui));
}

/// GPUI's view of `window`, which the web views go over.
fn gpui_view(window: &Window) -> Option<Retained<NSView>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
    // SAFETY: GPUI's window view, alive while its window is.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    Some(view.retain())
}

struct Ivars {
    /// The secret fields its document showed (`guard`).
    seen: guard::Seen,
    told: Tell,
    told_last: RefCell<Option<State>>,
    /// Why the last page failed, until another starts.
    error: RefCell<Option<String>>,
    /// Its plugin's web folder, which names the sites signed in with imported cookies.
    web_dir: std::path::PathBuf,
    /// The address of the document the view shows, as WebKit committed it, which commands act on.
    committed: RefCell<String>,
    /// The host of the origin of the frame whose dialog waits, none when it has none.
    dialog_frame: RefCell<Option<String>>,
    /// Counts the main frame's new pages, so a script's answer that a new page overtook is thrown away.
    generation: std::cell::Cell<u64>,
    /// The main frame's navigation under way started from or passed through a site signed in with imported cookies.
    chain_tainted: std::cell::Cell<bool>,
    /// The dialogs of the page now: its generation, how many it showed, and whether the person stopped them.
    dialogs: std::cell::Cell<(u64, u32, bool)>,
    /// Wings' own sheet showing the page's dialog, and its text field for a prompt.
    sheet: RefCell<Option<(Retained<NSAlert>, Option<Retained<NSTextField>>)>>,
    /// The page's dialog that waits for an answer, and what it shows.
    pending: RefCell<Option<(Pending, super::views::Dialog)>>,
    /// The page's recent console messages and requests (`LOGS`), the newest last.
    logs: RefCell<std::collections::VecDeque<serde_json::Value>>,
    /// The page as a tab to its extensions, and their buttons for it (`actions`).
    tab: RefCell<Option<Retained<actions::Tab>>>,
    extensions: RefCell<Vec<super::views::Extension>>,
}

/// How a page's dialog is answered.
enum Pending {
    Alert(RcBlock<dyn Fn()>),
    Confirm(RcBlock<dyn Fn(Bool)>),
    Prompt(RcBlock<dyn Fn(*mut NSString)>),
}

define_class!(
    // SAFETY: NSObject has no rules for subclasses, and this has no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Observer;

    impl Observer {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe(
            &self,
            key: Option<&NSString>,
            object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
            if let (Some(tab), Some(key)) = (self.ivars().tab.borrow().as_ref(), key) {
                actions::changed(tab, &key.to_string());
            }
            if let Some(view) = object.and_then(|object| object.downcast_ref::<WKWebView>()) {
                // A new load clears the last one's error.
                if unsafe { view.isLoading() } {
                    self.ivars().error.replace(None);
                }
                self.tell(view);
            }
        }
    }

    unsafe impl NSObjectProtocol for Observer {}

    unsafe impl WKNavigationDelegate for Observer {
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn failed_to_start(&self, view: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.failed(view, error);
        }

        /// Only to addresses a web view may go to (`may_navigate`): a link to `file:` or another app's scheme is dropped.
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide(&self, _view: &WKWebView, action: &WKNavigationAction, decide: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>) {
            // SAFETY: the action's request and frame, on the main thread.
            let (url, main_frame) = unsafe {
                let url = action.request().URL().and_then(|url| url.absoluteString()).map(|url| url.to_string()).unwrap_or_default();
                // A new window's navigation has no target frame: it loads in this view's page (`new_window`).
                (url, action.targetFrame().is_none_or(|frame| frame.isMainFrame()))
            };
            let allowed = super::may_navigate(&url, main_frame);
            // A page reached from a site signed in with imported cookies, or through one (a sign-in's redirects), may be
            // signed in by it: it counts as imported once it commits (`committed`).
            if allowed && main_frame && super::carries_imported(&self.ivars().web_dir, &self.ivars().committed.borrow(), &url) {
                self.ivars().chain_tainted.set(true);
            }
            let policy = if allowed { WKNavigationActionPolicy::Allow } else { WKNavigationActionPolicy::Cancel };
            decide.call((policy,));
        }

        /// The view shows a new document: commands act on it from now.
        #[unsafe(method(webView:didCommitNavigation:))]
        fn committed(&self, view: &WKWebView, _navigation: Option<&WKNavigation>) {
            // SAFETY: the view's address, on the main thread.
            let url = unsafe { view.URL() }.and_then(|url| url.absoluteString()).map(|url| url.to_string()).unwrap_or_default();
            if self.ivars().chain_tainted.replace(false)
                && let Some(host) = super::origin(&url).and_then(|origin| origin.split_once("://").map(|(_, host)| host.split(':').next().unwrap_or_default().to_owned()))
            {
                // Kept for this run even when it can't be written down (`note_imported`).
                let _ = super::note_imported(&self.ivars().web_dir, [host]);
            }
            self.ivars().committed.replace(url);
            // A new page starts with no logs, and a script or a picture of the last one is thrown away. Only here: a jump to
            // a `#` in the same page is no new page.
            self.ivars().logs.borrow_mut().clear();
            self.ivars().generation.set(self.ivars().generation.get() + 1);
            // A page on a site signed in with imported cookies, or no longer on one, shows other buttons (`views::shown`).
            let tab = self.ivars().tab.borrow().clone();
            if let Some(tab) = tab {
                actions::refresh(&tab);
            }
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn failed_to_finish(&self, view: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.failed(view, error);
        }
    }

    unsafe impl WKScriptMessageHandler for Observer {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn heard(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            // A frame holds a secret field (`guard::WATCH`): kept for the rest of the main frame's document, and apart
            // when it is a frame in the page, which may be of another origin, so the frames the check can't look into
            // are painted over in a picture.
            // SAFETY: the message's name and frame, on the main thread.
            if unsafe { message.name() }.to_string() == guard::HANDLER {
                let in_frame = !unsafe { message.frameInfo().isMainFrame() };
                self.ivars().seen.note(self.ivars().generation.get(), in_frame);
                return;
            }
            // SAFETY: the message's frame's origin and body, on the main thread: what the page posted, its word only.
            let (host, body) = unsafe { (message.frameInfo().securityOrigin().host().to_string(), message.body()) };
            // Nothing from a site signed in with imported cookies, or a frame with no host of its own.
            if super::holds_imported_host(&self.ivars().web_dir, &host) {
                return;
            }
            let Some(body) = body.downcast_ref::<NSDictionary>() else { return };
            let field = |key: &str| body.objectForKey(&NSString::from_str(key));
            let text = |key: &str| field(key).and_then(|value| value.downcast_ref::<NSString>().map(|text| text.to_string()));
            let number = |key: &str| field(key).and_then(|value| value.downcast_ref::<NSNumber>().map(|number| number.as_i64()));
            let line = match text("kind").as_deref() {
                Some("console") => serde_json::json!({ "kind": "console", "level": text("level"), "text": text("text") }),
                Some("network") => serde_json::json!({ "kind": "network", "method": text("method"), "url": text("url"), "status": number("status"), "error": text("error") }),
                _ => return,
            };
            let mut logs = self.ivars().logs.borrow_mut();
            logs.push_back(line);
            while logs.len() > LOGS_KEPT * 2 {
                logs.pop_front();
            }
        }
    }

    unsafe impl WKUIDelegate for Observer {
        #[unsafe(method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn alert(&self, view: &WKWebView, message: &NSString, frame: &WKFrameInfo, done: &block2::DynBlock<dyn Fn()>) {
            self.wait_on(view, Pending::Alert(done.copy()), "alert", message, None, frame);
        }

        #[unsafe(method(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn confirm(&self, view: &WKWebView, message: &NSString, frame: &WKFrameInfo, done: &block2::DynBlock<dyn Fn(Bool)>) {
            self.wait_on(view, Pending::Confirm(done.copy()), "confirm", message, None, frame);
        }

        #[unsafe(method(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:))]
        fn prompt(&self, view: &WKWebView, message: &NSString, default: Option<&NSString>, frame: &WKFrameInfo, done: &block2::DynBlock<dyn Fn(*mut NSString)>) {
            self.wait_on(view, Pending::Prompt(done.copy()), "prompt", message, default, frame);
        }

        /// A page's file input: the system's open panel, for the person to pick. Nothing else picks files for a page.
        #[unsafe(method(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:))]
        fn open_panel(&self, _view: &WKWebView, parameters: &WKOpenPanelParameters, _frame: &WKFrameInfo, done: &block2::DynBlock<dyn Fn(*mut NSArray<NSURL>)>) {
            let Some(mtm) = MainThreadMarker::new() else { return done.call((std::ptr::null_mut(),)) };
            let panel = NSOpenPanel::openPanel(mtm);
            // SAFETY: the page's own wishes for the panel, on the main thread.
            unsafe {
                panel.setAllowsMultipleSelection(parameters.allowsMultipleSelection());
                panel.setCanChooseDirectories(parameters.allowsDirectories());
            }
            panel.setCanChooseFiles(true);
            let (done, chosen) = (done.copy(), panel.clone());
            let picked = RcBlock::new(move |response: NSModalResponse| {
                // NSModalResponseOK is 1.
                let urls = (response == 1).then(|| chosen.URLs());
                done.call((urls.map_or(std::ptr::null_mut(), |urls| Retained::as_ptr(&urls).cast_mut()),));
            });
            panel.beginWithCompletionHandler(&picked);
        }

        /// A link that opens a new window (`target="_blank"`, `window.open`) loads in this view: a page has one view.
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn new_window(
            &self,
            view: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            // SAFETY: the action's request, loaded in the view that asked, on the main thread; only to an address a web view
            // may load, as `decide` checks the navigation that follows too.
            unsafe {
                let request = action.request();
                let url = request.URL().and_then(|url| url.absoluteString()).map(|url| url.to_string()).unwrap_or_default();
                if super::may_navigate(&url, true) {
                    view.loadRequest(&request);
                }
            }
            None
        }
    }
);

impl Observer {
    fn new(told: Tell, web_dir: std::path::PathBuf, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            told,
            told_last: RefCell::new(None),
            error: RefCell::new(None),
            pending: RefCell::new(None),
            logs: RefCell::default(),
            web_dir,
            sheet: RefCell::new(None),
            committed: RefCell::default(),
            dialog_frame: RefCell::new(None),
            generation: std::cell::Cell::new(0),
            chain_tainted: std::cell::Cell::new(false),
            dialogs: std::cell::Cell::new((0, 0, false)),
            tab: RefCell::new(None),
            extensions: RefCell::default(),
            seen: guard::Seen::default(),
        });
        // SAFETY: NSObject's `init`.
        unsafe { msg_send![super(this), init] }
    }

    /// The page waits on a dialog: Wings shows it in a sheet of its own on the window, naming the site, which the person
    /// answers; its plugin hears of it (`State::dialog`) and, where `gate` lets it, may answer it too (`dialog`).
    fn wait_on(&self, view: &WKWebView, pending: Pending, kind: &str, message: &NSString, default: Option<&NSString>, frame: &WKFrameInfo) {
        // A page that asks again before the last was answered: the last is cancelled, as WebKit needs each answered.
        self.answer(false, None);
        // The frame's origin, which is where it really is: a frame's request can say `about:blank` for a page on any site.
        // SAFETY: the frame's security origin, on the main thread.
        let host = unsafe { frame.securityOrigin().host() }.to_string();
        let site = host.clone();
        let frame_url = Some(host).filter(|host| !host.is_empty());
        let shown = (site.clone(), message.to_string().chars().take(4000).collect::<String>(), default.map(|text| text.to_string()));
        // The plugin hears only that there is one when it comes from a site signed in with imported cookies, or a frame
        // with no host.
        let private = frame_url.as_ref().is_none_or(|host| super::holds_imported_host(&self.ivars().web_dir, host));
        let dialog = match private {
            true => super::views::Dialog { kind: kind.into(), ..Default::default() },
            false => super::views::Dialog { kind: kind.into(), message: shown.1.clone(), default_text: shown.2.clone(), site },
        };
        self.ivars().dialog_frame.replace(frame_url);
        let prompt = matches!(pending, Pending::Prompt(_));
        self.ivars().pending.replace(Some((pending, dialog)));
        // A page whose dialogs the person stopped, or one not on screen (a tab behind another), gets Cancel at once:
        // a sheet would hold the whole window for a page nobody looks at, and `while (true) alert()` would never end.
        let (generation, count, stopped) = self.ivars().dialogs.get();
        let count = if generation == self.ivars().generation.get() { count + 1 } else { 1 };
        let stopped = stopped && generation == self.ivars().generation.get();
        self.ivars().dialogs.set((self.ivars().generation.get(), count, stopped));
        if stopped || view.isHidden() || view.window().is_none() {
            self.resolve(false, None);
        } else {
            self.show_sheet(view, kind, shown, prompt, count > 1);
        }
        self.tell(view);
    }

    fn show_sheet(&self, view: &WKWebView, kind: &str, (site, message, default): (String, String, Option<String>), prompt: bool, offer_stop: bool) {
        let (Some(mtm), Some(window)) = (MainThreadMarker::new(), view.window()) else { return };
        let alert = NSAlert::new(mtm);
        let site = if site.is_empty() { "This page".to_owned() } else { site };
        alert.setMessageText(&NSString::from_str(&format!("{site} says")));
        alert.setInformativeText(&NSString::from_str(&message));
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        if kind != "alert" {
            alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        }
        let field = prompt.then(|| {
            let field = NSTextField::textFieldWithString(&NSString::from_str(default.as_deref().unwrap_or_default()), mtm);
            field.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(260.0, 24.0)));
            alert.setAccessoryView(Some(&field));
            field
        });
        // From a page's second dialog on, the person can stop its dialogs for as long as it stays.
        if offer_stop {
            alert.setShowsSuppressionButton(true);
            if let Some(stop) = alert.suppressionButton() {
                stop.setTitle(&NSString::from_str("Stop dialogs from this page"));
            }
        }
        let (this, typed, page, asked) = (self.retain(), field.clone(), view.retain(), alert.clone());
        let answered = RcBlock::new(move |response: NSModalResponse| {
            this.ivars().sheet.replace(None);
            if asked.suppressionButton().is_some_and(|stop| stop.state() == 1) {
                let (generation, count, _) = this.ivars().dialogs.get();
                this.ivars().dialogs.set((generation, count, true));
            }
            // NSAlertFirstButtonReturn, OK, is 1000.
            let text = typed.as_ref().map(|field| field.stringValue().to_string());
            this.resolve(response == 1000, text.as_deref());
            this.tell(&page);
        });
        self.ivars().sheet.replace(Some((alert.clone(), field)));
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&answered));
    }

    /// Answers the page's dialog, if it shows one, as OK or Cancel: whether it did. Wings' sheet goes with the same
    /// answer, which then reaches the page.
    fn answer(&self, accept: bool, text: Option<&str>) -> bool {
        if self.ivars().pending.borrow().is_none() {
            return false;
        }
        let sheet = self.ivars().sheet.borrow().clone();
        match sheet.and_then(|(alert, field)| Some((alert.window().sheetParent()?, alert, field))) {
            Some((parent, alert, field)) => {
                if let (Some(field), Some(text)) = (field, text) {
                    field.setStringValue(&NSString::from_str(text));
                }
                parent.endSheet_returnCode(&alert.window(), if accept { 1000 } else { 1001 });
            }
            None => {
                self.ivars().sheet.replace(None);
                self.resolve(accept, text);
            }
        }
        true
    }

    /// Hands WebKit the answer to the page's dialog, and tells the page it has none.
    fn resolve(&self, accept: bool, text: Option<&str>) {
        let Some((pending, dialog)) = self.ivars().pending.take() else { return };
        match pending {
            Pending::Alert(done) => done.call(()),
            Pending::Confirm(done) => done.call((Bool::new(accept),)),
            Pending::Prompt(done) => {
                let answer = accept.then(|| NSString::from_str(text.or(dialog.default_text.as_deref()).unwrap_or_default()));
                done.call((answer.as_ref().map_or(std::ptr::null_mut(), |answer| Retained::as_ptr(answer).cast_mut()),));
            }
        }
    }

    fn failed(&self, view: &WKWebView, error: &NSError) {
        // A load stopped by another (a click before the page finished) isn't a failure the person needs to see:
        // NSURLErrorCancelled.
        if error.code() == -999 {
            return;
        }
        self.ivars().error.replace(Some(error.localizedDescription().to_string()));
        self.tell(view);
    }

    /// Tells the page where `view` is, if that changed since it last heard.
    fn tell(&self, view: &WKWebView) {
        // SAFETY: the web view's properties, on the main thread.
        let state = unsafe {
            State {
                url: view.URL().and_then(|url| url.absoluteString()).map(|url| url.to_string()).unwrap_or_default(),
                title: view.title().map(|title| title.to_string()).unwrap_or_default(),
                loading: view.isLoading(),
                can_go_back: view.canGoBack(),
                can_go_forward: view.canGoForward(),
                error: self.ivars().error.borrow().clone(),
                dialog: self.ivars().pending.borrow().as_ref().map(|(_, dialog)| dialog.clone()),
                imported: super::holds_imported(&self.ivars().web_dir, &self.ivars().committed.borrow()),
                extensions: self.ivars().extensions.borrow().clone(),
            }
        };
        if self.ivars().told_last.borrow().as_ref() != Some(&state) {
            self.ivars().told_last.replace(Some(state.clone()));
            self.ivars().told.tell(Heard::State(state));
        }
    }
}

#[cfg(test)]
mod tests {
    /// This Mac's major and minor version, as `sw_vers` says it.
    fn this_mac() -> (isize, isize) {
        let out = std::process::Command::new("sw_vers").arg("-productVersion").output().unwrap();
        let version = String::from_utf8(out.stdout).unwrap();
        let mut parts = version.trim().split('.').map(|part| part.parse::<isize>().unwrap_or(0));
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
    }

    #[test]
    fn a_scripts_answer_is_read_as_json_and_anything_else_is_an_error_not_a_crash() {
        use objc2_foundation::{NSDate, NSDictionary, NSNumber, NSString};
        let text = NSString::from_str("Docs");
        assert_eq!(super::json_of(&text), Ok(serde_json::json!("Docs")));
        assert_eq!(super::json_of(&NSNumber::new_i64(3)), Ok(serde_json::json!(3)));
        let object = NSDictionary::from_slices(&[&*NSString::from_str("title")], &[&*NSString::from_str("Docs")]);
        assert_eq!(super::json_of(&object), Ok(serde_json::json!({ "title": "Docs" })));
        assert!(super::json_of(&NSDate::now()).unwrap_err().contains("isn't JSON"));
    }

    #[test]
    fn the_keyboard_goes_back_to_gpuis_view_not_the_vibrancy_or_backdrop_under_it() {
        use objc2::{ClassType, MainThreadMarker, msg_send, rc::Retained, runtime::{AnyClass, ClassBuilder}};
        use objc2_app_kit::{NSView, NSVisualEffectView};
        // SAFETY: plain views that never go in a window. They're never freed either: freeing GPUI's view frees the
        // window state it was never given.
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let content = NSView::new(mtm);
        content.addSubview(&NSVisualEffectView::new(mtm));
        content.addSubview(&NSView::new(mtm));
        // The tests run on GPUI's test platform, which makes no `GPUIView` class, so a plain view of that name stands in.
        let class = AnyClass::get(c"GPUIView")
            .unwrap_or_else(|| ClassBuilder::new(c"GPUIView", NSView::class()).expect("a new class").register());
        let gpui: Retained<NSView> = unsafe { msg_send![class, new] };
        content.addSubview(&gpui);
        let found = super::gpui_among(&content).expect("GPUI's view");
        assert!(std::ptr::eq(&*found, &*gpui), "{:?}", found.class().name());
        std::mem::forget((content, gpui, found));
    }

    /// An extension built for Chrome tells the browser by its own pages' user agent, and found none in WebKit's: Bitwarden's
    /// background and popup failed at `getDevice()`. With the pages' name added it is Chrome, not Safari, whose paths a
    /// Chrome build can't run. (Setting the name is WebKit's configuration, which WebKit makes only on the main thread: it
    /// ends a process that makes one off it, as every test runs.)
    #[test]
    fn an_extensions_own_pages_are_chrome_to_the_extension_and_not_safari() {
        // WebKit's own, as Bitwarden's popup read it on macOS 27.2, and as WebKit makes it with the name, after a space.
        let webkit = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
        let pages = format!("{webkit} {}", super::EXTENSION_PAGES_AGENT);
        // Bitwarden's tests, `BrowserPlatformUtilsService.isChrome` and `BrowserApi.isSafariApi`.
        let chrome = |agent: &str| agent.contains(" Chrome/");
        let safari = |agent: &str| agent.contains(" Safari/") && !agent.contains(" Chrome/") && !agent.contains(" Chromium/");
        assert!(!chrome(webkit) && !safari(webkit), "WebKit's own names no browser");
        assert!(chrome(&pages) && !safari(&pages), "{pages}");
    }

    #[test]
    fn kept_cookies_and_extensions_follow_the_macs_version_before_any_web_view_is_made() {
        let version = this_mac();
        assert_eq!(super::keeps_per_plugin(), version >= (14, 0), "{version:?}");
        assert_eq!(super::extensions_supported(), version >= (15, 4), "{version:?}");
    }
}
