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
    runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol, ProtocolObject},
    sel,
};
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSView, NSWindowOrderingMode};
use objc2_foundation::{
    NSDate, NSDictionary, NSError, NSHTTPCookie, NSHTTPCookieDomain, NSHTTPCookieExpires, NSHTTPCookieName,
    NSHTTPCookiePath, NSHTTPCookieSameSitePolicy, NSHTTPCookieSameSiteLax, NSHTTPCookieSameSiteStrict,
    NSHTTPCookieSecure, NSHTTPCookieValue, NSKeyValueChangeKey, NSKeyValueObservingOptions,
    NSObjectNSKeyValueObserverRegistration, NSPoint, NSRect, NSSize, NSString, NSURL, NSURLRequest, NSUUID,
};
use objc2_web_kit::{
    WKContentWorld, WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKUIDelegate, WKWebExtension, WKWebExtensionContext,
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

/// Whether this Mac has WebKit's extensions, which came with macOS 15.4.
pub fn extensions_supported() -> bool {
    AnyClass::get(c"WKWebExtensionController").is_some()
}

/// Whether WebKit keeps a store for each plugin (`dataStoreForIdentifier:`, macOS 14).
pub fn keeps_per_plugin() -> bool {
    AnyClass::get(c"WKWebsiteDataStore").is_some_and(|class| class.responds_to(sel!(dataStoreForIdentifier:)))
}

fn shared(profile: &Profile, keep_data: bool, mtm: MainThreadMarker) -> Rc<Shared> {
    if let Some(shared) = SHARED.with_borrow(|all| all.get(&(profile.clone(), keep_data)).cloned()) {
        return shared;
    }
    let uuid = NSUUID::from_bytes(profile.uuid());
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
    fn make(&self, profile: &Profile, keep_data: bool, url: Option<&str>, told: Rc<dyn Fn(Heard)>) -> Box<dyn Native> {
        let mtm = MainThreadMarker::new().expect("web views are made on the main thread");
        let shared = shared(profile, keep_data, mtm);
        // SAFETY: WebKit's objects, made and used on the main thread.
        let view = unsafe {
            let configuration = WKWebViewConfiguration::new(mtm);
            configuration.setWebsiteDataStore(&shared.store);
            if let Some(extensions) = &shared.extensions {
                configuration.setWebExtensionController(Some(extensions));
            }
            let view = WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), NSRect::ZERO, &configuration);
            // So a plugin author can look at a page with Safari's Web Inspector, as in any app with a web view.
            view.setInspectable(true);
            view.setHidden(true);
            view
        };
        let observer = Observer::new(told.clone(), mtm);
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
        let mut native = WebPage { view, observer, told, shots, web_dir };
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
}

/// Why a script didn't run in a page's own world.
const IMPORTED: &str = "This page is signed in with cookies you imported, so a plugin's script runs there only in its own world";

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
                // A script in the page's own world acts as the page: where the person is signed in with cookies they
                // imported, only the plugin's world, which sees the document but not the page's scripts or their state.
                WebGo::Eval { page: true, .. } if self.view.URL().and_then(|url| url.absoluteString()).is_some_and(|url| super::holds_imported(&self.web_dir, &url.to_string())) => {
                    (self.told)(Heard::Result { token, result: Err(IMPORTED.into()) });
                }
                WebGo::Eval { script, page } => {
                    // The value goes back as JSON text, which crosses from JavaScript as a plain string.
                    let body = format!("return JSON.stringify(await (async () => {{\n{script}\n}})() ?? null);");
                    let world = if *page { WKContentWorld::pageWorld(mtm) } else { WKContentWorld::worldWithName(&NSString::from_str("wings-plugin"), mtm) };
                    let told = self.told.clone();
                    let done = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                        let result = match (value.as_ref().and_then(|value| value.downcast_ref::<NSString>()), error.as_ref()) {
                            (Some(json), None) => serde_json::from_str(&json.to_string()).map_err(|error| error.to_string()),
                            (_, Some(error)) => Err(error.localizedDescription().to_string()),
                            (None, None) => Ok(serde_json::Value::Null),
                        };
                        told(Heard::Result { token, result });
                    });
                    self.view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(&NSString::from_str(&body), None, None, &world, Some(&done));
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
        // SAFETY: the observer added in `make`, removed once.
        unsafe {
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
            let policy = if super::may_navigate(&url, main_frame) { WKNavigationActionPolicy::Allow } else { WKNavigationActionPolicy::Cancel };
            decide.call((policy,));
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn failed_to_finish(&self, view: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.failed(view, error);
        }
    }

    unsafe impl WKUIDelegate for Observer {
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
    fn new(told: Rc<dyn Fn(Heard)>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { told, told_last: RefCell::new(None), error: RefCell::new(None) });
        // SAFETY: NSObject's `init`.
        unsafe { msg_send![super(this), init] }
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
            }
        };
        if self.ivars().told_last.borrow().as_ref() != Some(&state) {
            self.ivars().told_last.replace(Some(state.clone()));
            (self.ivars().told)(Heard::State(state));
        }
    }
}
