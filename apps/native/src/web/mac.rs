//! The web views as WebKit draws them: a `WKWebView` over GPUI's view in the window's content view, at its page's place.
//! Each plugin's pages share a store of cookies and site data of its own (`WKWebsiteDataStore(forIdentifier:)`, macOS
//! 14), or one kept only while Wings runs, and on macOS 15.4 a controller of the extensions added to it
//! (`WKWebExtensionController`).

use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::c_void,
    path::Path,
    rc::Rc,
};

use block2::RcBlock;
use gpui::{Bounds, Pixels, Window};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_app_kit::{NSAlert, NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSModalResponse, NSOpenPanel, NSTextField, NSView, NSWindowOrderingMode};
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
    views::{Heard, Maker, Native, State, make_with},
};
use crate::page::tree::WebGo;

/// What the web views say they are, which a change of any of them tells the page.
const OBSERVED: [&str; 5] = ["URL", "title", "loading", "canGoBack", "canGoForward"];

/// Has WebKit draw the web views of the pages in `window` from now on.
pub fn host_natively(window: &Window) {
    make_with(window.window_handle().window_id(), Rc::new(WebKit));
}

/// What the plugins' web views share, by profile and whether they keep their data.
struct Shared {
    store: Retained<WKWebsiteDataStore>,
    /// Its extensions' controller, on a Mac that has them (15.4).
    extensions: Option<Retained<WKWebExtensionController>>,
}

thread_local! {
    static SHARED: RefCell<HashMap<(Profile, bool), Rc<Shared>>> = RefCell::default();
    /// Each profile's extensions loaded now, by their id, to unload one turned off or removed.
    static LOADED: RefCell<HashMap<(Profile, bool, String), Retained<WKWebExtensionContext>>> = RefCell::default();
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
        WKWebExtensionController::initWithConfiguration(WKWebExtensionController::alloc(mtm), &configuration)
    });
    let shared = Rc::new(Shared { store, extensions });
    SHARED.with_borrow_mut(|all| all.insert((profile.clone(), keep_data), shared.clone()));
    if shared.extensions.is_some() {
        for extension in super::extensions::enabled(profile) {
            load(profile, keep_data, &extension.id, &extension.dir, |_| {});
        }
    }
    shared
}

/// Loads the extension in `dir` as `id` into `profile`'s controller, granting what its manifest asks for, which the
/// person saw before adding it; `done` hears whether it loaded. Nothing for a profile no web view has used yet: its
/// controller loads its extensions as it is made.
pub fn load(profile: &Profile, keep_data: bool, id: &str, dir: &Path, done: impl Fn(Result<(), String>) + 'static) {
    let Some(controller) =
        SHARED.with_borrow(|all| all.get(&(profile.clone(), keep_data)).and_then(|shared| shared.extensions.clone()))
    else {
        return done(Ok(()));
    };
    let Some(mtm) = MainThreadMarker::new() else { return };
    let url = NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy()));
    let (profile, id) = (profile.clone(), id.to_owned());
    let loaded = RcBlock::new(move |extension: *mut WKWebExtension, error: *mut NSError| {
        // SAFETY: WebKit hands one of the two, alive for the call.
        let Some(extension) = (unsafe { extension.as_ref() }) else {
            let why = unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string());
            return done(Err(why.unwrap_or_else(|| "WebKit couldn't read the extension".into())));
        };
        // SAFETY: WebKit's own objects on the main thread, where its completion handlers run.
        let result = unsafe {
            let context = WKWebExtensionContext::contextForExtension(extension);
            for permission in extension.requestedPermissions().iter() {
                context.setPermissionStatus_forPermission(WKWebExtensionContextPermissionStatus::GrantedExplicitly, &permission);
            }
            for pattern in extension.requestedPermissionMatchPatterns().iter() {
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

/// Loads the extension in `dir` into each controller `profile` has now.
pub fn load_everywhere(profile: &Profile, id: &str, dir: &Path, done: impl Fn(Result<(), String>) + Clone + 'static) {
    for keep_data in [true, false] {
        load(profile, keep_data, id, dir, done.clone());
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
    let (count, done) = (made.len(), Rc::new(done));
    if count == 0 {
        return done(0);
    }
    for made in made {
        let (left, done) = (left.clone(), done.clone());
        let block = RcBlock::new(move || {
            left.set(left.get() - 1);
            if left.get() == 0 {
                done(count);
            }
        });
        // SAFETY: a cookie made just now, and a block WebKit calls once on the main thread.
        unsafe { jar.setCookie_completionHandler(&made, Some(&block)) };
    }
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
    fn make(&self, profile: &Profile, keep_data: bool, scripts: bool, url: Option<&str>, told: Rc<dyn Fn(Heard)>) -> Box<dyn Native> {
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
            }
            let view = WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), NSRect::ZERO, &configuration);
            // So a plugin author can look at a page with Safari's Web Inspector, as in any app with a web view.
            view.setInspectable(true);
            view.setHidden(true);
            view
        };
        let observer = Observer::new(told.clone(), profile.dir.clone(), mtm);
        if scripts {
            // SAFETY: the configuration's controller holds the handler until the view goes (`Drop` takes it away).
            unsafe {
                view.configuration().userContentController().addScriptMessageHandler_contentWorld_name(
                    ProtocolObject::from_ref(&*observer),
                    &WKContentWorld::pageWorld(mtm),
                    &NSString::from_str(LOG_HANDLER),
                )
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
    told: Rc<dyn Fn(Heard)>,
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
            self.view.setHidden(true);
            return;
        };
        let Some(gpui) = gpui_view(window) else { return };
        // SAFETY: AppKit's views, on the main thread.
        unsafe {
            let Some(content) = gpui.superview() else { return };
            if self.view.superview().as_deref() != Some(&*content) {
                content.addSubview_positioned_relativeTo(&self.view, NSWindowOrderingMode::Above, Some(&gpui));
            }
            // GPUI counts from the window's top; an unflipped view counts from its bottom.
            let height = content.bounds().size.height;
            let (x, y) = (f64::from(f32::from(frame.origin.x)), f64::from(f32::from(frame.origin.y)));
            let (w, h) = (f64::from(f32::from(frame.size.width)), f64::from(f32::from(frame.size.height)));
            let y = if content.isFlipped() { y } else { height - y - h };
            self.view.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)));
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
            _ => (super::holds_imported(&self.web_dir, &committed), !loading && committed == url),
        };
        if let Err(why) = super::gate(go, self.scripts, imported, settled) {
            return (self.told)(Heard::Result { token, result: Err(why.into()) });
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
                WebGo::Eval { script, page } => {
                    // The script is the function's whole body, with no wrapper of ours to break out of, and nothing it says
                    // of where it ran is believed: what it returns is read as WebKit hands it over, and thrown away when a
                    // new page came on the main frame while it ran (`generation`).
                    let world = if *page { WKContentWorld::pageWorld(mtm) } else { WKContentWorld::worldWithName(&NSString::from_str("wings-plugin"), mtm) };
                    let (told, observer) = (self.told.clone(), self.observer.clone());
                    let sent = observer.ivars().generation.get();
                    let done = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                        let result = match (value.as_ref(), error.as_ref()) {
                            _ if observer.ivars().generation.get() != sent => Err(MOVED_ON.to_owned()),
                            (_, Some(error)) => Err(error.localizedDescription().to_string()),
                            (Some(value), None) => json_of(value),
                            (None, None) => Ok(serde_json::Value::Null),
                        };
                        told(Heard::Result { token, result });
                    });
                    self.view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(&NSString::from_str(script), None, None, &world, Some(&done));
                }
                WebGo::Dialog { accept, text } => {
                    let answered = self.observer.answer(*accept, text.as_deref());
                    self.observer.tell(&self.view);
                    let result = if answered { Ok(serde_json::Value::Null) } else { Err("The page shows no dialog".into()) };
                    (self.told)(Heard::Result { token, result });
                }
                WebGo::Logs => {
                    let logs = self.observer.ivars().logs.borrow();
                    let of = |kind: &str| logs.iter().filter(|line| line["kind"] == kind).cloned().collect::<Vec<_>>();
                    let result = Ok(serde_json::json!({ "console": of("console"), "network": of("network") }));
                    (self.told)(Heard::Result { token, result });
                }
                WebGo::Snapshot => {
                    let (told, shots) = (self.told.clone(), self.shots.clone());
                    let done = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                        let result = match (image.as_ref(), error.as_ref()) {
                            (Some(image), _) => png_of(image).and_then(|png| super::snapshot::save(&shots, &png)),
                            (None, Some(error)) => Err(error.localizedDescription().to_string()),
                            (None, None) => Err("WebKit made no picture".into()),
                        };
                        told(Heard::Result { token, result });
                    });
                    // The part of the page that shows: WebKit takes no more than the view's bounds.
                    self.view.takeSnapshotWithConfiguration_completionHandler(None, &done);
                }
            }
        }
    }
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
        self.view.removeFromSuperview();
    }
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
    told: Rc<dyn Fn(Heard)>,
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
    /// Wings' own sheet showing the page's dialog, and its text field for a prompt.
    sheet: RefCell<Option<(Retained<NSAlert>, Option<Retained<NSTextField>>)>>,
    /// The page's dialog that waits for an answer, and what it shows.
    pending: RefCell<Option<(Pending, super::views::Dialog)>>,
    /// The page's recent console messages and requests (`LOGS`), the newest last.
    logs: RefCell<std::collections::VecDeque<serde_json::Value>>,
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
            _key: Option<&NSString>,
            object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
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
            // A new page starts with no logs: none of the last page's are read on this one's watch.
            if allowed && main_frame {
                self.ivars().logs.borrow_mut().clear();
                self.ivars().generation.set(self.ivars().generation.get() + 1);
            }
            let policy = if allowed { WKNavigationActionPolicy::Allow } else { WKNavigationActionPolicy::Cancel };
            decide.call((policy,));
        }

        /// The view shows a new document: commands act on it from now.
        #[unsafe(method(webView:didCommitNavigation:))]
        fn committed(&self, view: &WKWebView, _navigation: Option<&WKNavigation>) {
            // SAFETY: the view's address, on the main thread.
            let url = unsafe { view.URL() }.and_then(|url| url.absoluteString()).map(|url| url.to_string()).unwrap_or_default();
            self.ivars().committed.replace(url);
            self.ivars().generation.set(self.ivars().generation.get() + 1);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn failed_to_finish(&self, view: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.failed(view, error);
        }
    }

    unsafe impl WKScriptMessageHandler for Observer {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn heard(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            // SAFETY: the message's frame and body, on the main thread: what the page posted, which is the page's word only.
            let (frame, body) = unsafe {
                let frame = message.frameInfo().request().URL().and_then(|url| url.absoluteString()).map(|url| url.to_string());
                (frame.unwrap_or_default(), message.body())
            };
            // Nothing from a site signed in with imported cookies, a frame on one included.
            if super::holds_imported(&self.ivars().web_dir, &frame) {
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
    fn new(told: Rc<dyn Fn(Heard)>, web_dir: std::path::PathBuf, mtm: MainThreadMarker) -> Retained<Self> {
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
        self.show_sheet(view, kind, shown, prompt);
        self.tell(view);
    }

    fn show_sheet(&self, view: &WKWebView, kind: &str, (site, message, default): (String, String, Option<String>), prompt: bool) {
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
        let (this, typed, page) = (self.retain(), field.clone(), view.retain());
        let answered = RcBlock::new(move |response: NSModalResponse| {
            this.ivars().sheet.replace(None);
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
            }
        };
        if self.ivars().told_last.borrow().as_ref() != Some(&state) {
            self.ivars().told_last.replace(Some(state.clone()));
            (self.ivars().told)(Heard::State(state));
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
    fn kept_cookies_and_extensions_follow_the_macs_version_before_any_web_view_is_made() {
        let version = this_mac();
        assert_eq!(super::keeps_per_plugin(), version >= (14, 0), "{version:?}");
        assert_eq!(super::extensions_supported(), version >= (15, 4), "{version:?}");
    }
}
