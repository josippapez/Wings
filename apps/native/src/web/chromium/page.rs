//! One Chromium web view: a frameless Chrome-style window over the page's place in the Wings window (a child window
//! of it, `wings_chromium::window`), its browser, and what it tells its page. It is made, placed and hidden by the same
//! code as a WebKit view (`views::place`), and every command goes through the same gate (`web::gate`); a plugin's
//! scripts, pictures and logs then run over the page's own DevTools channel (`agent`).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::{Rc, Weak},
};

use cef::*;
use gpui::{Bounds, Pixels, Window};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSAlert, NSEvent, NSModalResponse, NSSecureTextField, NSTextField, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use wings_chromium::window as frameless;

use super::{agent, devtools::Incoming, focus, keys, runtime};
use crate::{
    page::tree::WebGo,
    web::{
        Profile, downloads, prompts,
        views::{self, Dialog, Heard, Maker, Native, Overlaid, State, Tell},
    },
};

/// What a page that crashed says until it reloads.
pub const CRASHED: &str = "This page crashed. Reload to open it again.";

pub struct Chromium;

impl Maker for Chromium {
    fn make(&self, profile: &Profile, keep_data: bool, scripts: bool, url: Option<&str>, told: Tell) -> Box<dyn Native> {
        let page = Shared::new(told, profile, keep_data, scripts, url, None);
        // Its loading state shows while CEF starts.
        page.tell();
        let weak = Rc::downgrade(&page);
        runtime::ensure(profile, move |ready| {
            let Some(page) = weak.upgrade().filter(|page| !page.closed.get()) else { return };
            match ready {
                Ok(()) => page.open(),
                Err(why) => {
                    page.now.borrow_mut().loading = false;
                    page.now.borrow_mut().error = Some(format!("Chromium couldn't start: {why}"));
                    page.tell();
                }
            }
        });
        Box::new(Page(page))
    }
}

thread_local! {
    /// Every page alive, to close them all as Wings quits.
    static PAGES: RefCell<Vec<Weak<Shared>>> = RefCell::default();
    /// The contexts, by profile and whether they keep their data, whose sites' storage a Clear data couldn't reach as
    /// no page of theirs was open: their next page clears it before it loads (`clear_first`).
    static CLEAR_NEXT: RefCell<Vec<(Profile, bool)>> = RefCell::default();
}

/// What `Storage.clearDataForOrigin` takes to clear every site's storage of a page's context: no origin, all kinds
/// (cookies, local and session storage, IndexedDB, service workers, cache storage, file systems), as Chromium 156's
/// `StoragePartitionImpl::ClearData` reads an opaque origin as every one.
fn clear_all() -> serde_json::Value {
    serde_json::json!({ "origin": "", "storageTypes": "all" })
}

/// Closes every page's window, as Wings quits.
pub fn close_all() {
    let pages: Vec<Rc<Shared>> = PAGES.with_borrow_mut(|pages| pages.drain(..).filter_map(|page| page.upgrade()).collect());
    for page in pages {
        page.close();
    }
}

/// Removes everything Chromium keeps for `profile`, as WebKit's Clear data does of its stores: cookies, the cache, every
/// site's storage and service workers, and its extensions' data. Its folder goes while no context of Chromium's holds it
/// open (Chromium isn't running, or not for it); the contexts CEF holds are cleared through Chromium, their sites'
/// storage over one of their pages' DevTools, or before their next page loads when none is open. `done` hears when it
/// is all gone.
pub fn clear_data(profile: &Profile, done: impl FnOnce() + 'static) {
    let live = if runtime::running() { runtime::live(profile) } else { Vec::new() };
    if !live.iter().any(|(keep_data, _)| *keep_data)
        && let Some(root) = runtime::profiles_root()
    {
        let _ = std::fs::remove_dir_all(super::homes::home(&root, &runtime::dir_name(profile)));
    }
    let left = Rc::new(Cell::new(1));
    let done: Rc<RefCell<Option<Box<dyn FnOnce()>>>> = Rc::new(RefCell::new(Some(Box::new(done))));
    let counted = left.clone();
    let one_done = move || {
        counted.set(counted.get() - 1);
        if counted.get() == 0
            && let Some(done) = done.borrow_mut().take()
        {
            done();
        }
    };
    let one_done: Rc<dyn Fn()> = Rc::new(one_done);
    let pages = pages_of(profile);
    for (keep_data, context) in live {
        if let Some(cookies) = context.cookie_manager(None) {
            cookies.delete_cookies(None, None, None);
        }
        // With a callback: CEF 156 runs it unasked (`ClearHttpCacheInternal` binds `OnComplete` on it), and Wings
        // crashed as the cache was cleared with none.
        left.set(left.get() + 1);
        context.clear_http_cache(Some(&mut CacheCleared::new(one_done.clone())));
        match pages.iter().find(|page| page.keep_data == keep_data).and_then(|page| page.browser.borrow().clone()) {
            Some(browser) => {
                left.set(left.get() + 1);
                let one_done = one_done.clone();
                super::devtools::once(&browser, "Storage.clearDataForOrigin", clear_all(), move |cleared| {
                    if let Err(why) = cleared {
                        eprintln!("[chromium] a page's sites' storage wasn't cleared: {why}");
                    }
                    one_done();
                });
            }
            None => CLEAR_NEXT.with_borrow_mut(|next| next.push((profile.clone(), keep_data))),
        }
    }
    if runtime::global().as_ref() == Some(profile) {
        left.set(left.get() + 1);
        let one_done = one_done.clone();
        super::extensions::forget_data(profile, move || one_done());
    }
    one_done();
}

/// Removes the cookies and storage of `site` from every context of `profile` Chromium holds, and nothing of another
/// site: its cookies through the cookie manager, the rest (local storage, IndexedDB, service workers, caches) over
/// one of the context's pages' DevTools, as `clear_data` does. `done` hears when they are gone.
pub fn clear_site(profile: &Profile, site: &super::super::engine::site::Site, done: impl FnOnce() + 'static) {
    let live = if runtime::running() { runtime::live(profile) } else { Vec::new() };
    let left = Rc::new(Cell::new(1));
    let done: Rc<RefCell<Option<Box<dyn FnOnce()>>>> = Rc::new(RefCell::new(Some(Box::new(done))));
    let counted = left.clone();
    let one_done: Rc<dyn Fn()> = Rc::new(move || {
        counted.set(counted.get() - 1);
        if counted.get() == 0
            && let Some(done) = done.borrow_mut().take()
        {
            done();
        }
    });
    let pages = pages_of(profile);
    for (keep_data, context) in live {
        if let Some(cookies) = context.cookie_manager(None) {
            cookies.delete_cookies(Some(&CefString::from(site.url.as_str())), None, None);
        }
        let Some(browser) = pages.iter().find(|page| page.keep_data == keep_data).and_then(|page| page.browser.borrow().clone()) else { continue };
        left.set(left.get() + 1);
        let one_done = one_done.clone();
        super::devtools::once(&browser, "Storage.clearDataForOrigin", serde_json::json!({ "origin": site.origin, "storageTypes": "all" }), move |cleared| {
            if let Err(why) = cleared {
                eprintln!("[chromium] a site's storage wasn't cleared: {why}");
            }
            one_done();
        });
    }
    one_done();
}

/// The first page alive, for a debug build's checks (`debug`).
#[cfg(debug_assertions)]
pub fn first() -> Option<Rc<Shared>> {
    PAGES.with_borrow(|pages| pages.iter().filter_map(Weak::upgrade).find(|page| !page.closed.get()))
}

/// The page `page` is, as the list of pages holds it.
pub fn weak_of(page: &Shared) -> Weak<Shared> {
    PAGES.with_borrow(|pages| pages.iter().find(|kept| std::ptr::eq(kept.as_ptr(), page)).cloned()).unwrap_or_default()
}

/// Every page of `profile` alive now.
pub fn pages_of(profile: &Profile) -> Vec<Rc<Shared>> {
    PAGES.with_borrow_mut(|pages| {
        pages.retain(|page| page.strong_count() > 0);
        pages.iter().filter_map(Weak::upgrade).filter(|page| page.profile == *profile && !page.closed.get()).collect()
    })
}

/// Where the page is, as it last heard.
#[derive(Default, Clone)]
struct Now {
    url: String,
    title: String,
    loading: bool,
    back: bool,
    forward: bool,
    error: Option<String>,
}

pub struct Shared {
    pub(super) told: Tell,
    pub(super) profile: Profile,
    pub(super) keep_data: bool,
    scripts: bool,
    pub(super) web_dir: PathBuf,
    /// Where it opens once CEF is up, which a `load` before that changes.
    start: RefCell<String>,
    now: RefCell<Now>,
    told_last: RefCell<Option<State>>,
    /// The address of the main frame's document, as CEF committed it, which commands act on.
    pub(super) committed: RefCell<String>,
    /// Counts the main frame's new documents.
    pub(super) generation: Cell<u64>,
    /// The main frame's navigation under way started from or passed through a site signed in with imported cookies.
    chain_tainted: Cell<bool>,
    /// Its plugin's scripts, pictures and logs.
    pub(super) agent: agent::Agent,
    /// Its browser exists and, for a plugin that may run scripts in its pages, its DevTools channel is readied: until
    /// then it shows a blank page and keeps the commands sent, so nothing loads before the guard watches it.
    ready: Cell<bool>,
    /// It opened blank, as its context's sites' storage is cleared first (`CLEAR_NEXT`).
    clear_first: Cell<bool>,
    pub(super) browser: RefCell<Option<Browser>>,
    window: RefCell<Option<cef::Window>>,
    pub(super) ns: RefCell<Option<Retained<NSWindow>>>,
    /// The Wings window it shows over.
    pub(super) parent: RefCell<Option<Retained<NSWindow>>>,
    /// Where it shows, in the Wings window's points from its top left, or hidden.
    want: Cell<Option<Bounds<Pixels>>>,
    /// What of the Wings window lies over it (a menu, a popover, a dialog's backdrop), which shows through it or dims it
    /// (`Native::overlaid`).
    over: RefCell<Overlaid>,
    /// What dims it under a dialog's backdrop, once one has.
    dim: RefCell<Option<Retained<NSView>>>,
    attached: Cell<bool>,
    /// It is hidden but kept rendering: attached at its frame, clear and deaf to the mouse (`Native::keep_live`).
    live: Cell<bool>,
    closed: Cell<bool>,
    /// Commands sent before it was ready.
    pending: RefCell<Vec<(WebGo, u64)>>,
    /// The page's `alert`, `confirm` or `prompt` waiting for an answer, what it shows, and Wings' sheet showing it.
    dialog: RefCell<Option<(JsdialogCallback, JsdialogType, Dialog)>>,
    sheet: RefCell<Option<(Retained<NSAlert>, Option<Retained<NSTextField>>)>>,
    /// The site's certificate Chromium refused and waits on the person for (`on_certificate_error`), what the notice says,
    /// and the address the main frame is going to, which tells a main page's certificate from a picture's.
    certificate: RefCell<Option<(Callback, prompts::Notice)>>,
    navigating: RefCell<String>,
    /// A server's request for a name and a password, waiting on Wings' sheet: how to answer it, the site, and the sheet.
    /// What is typed goes from the sheet to `AuthCallback` and nowhere else.
    auth: RefCell<Option<(AuthCallback, String)>>,
    auth_sheet: RefCell<Option<Retained<NSAlert>>>,
    /// The page's zoom in percent, kept for this view and put back as each document loads.
    zoom: Cell<u32>,
    /// What its search last found: the text, and how many matches and which is current.
    find: RefCell<Option<(String, Option<u32>, u32)>>,
    /// The page's dialogs now: its generation, how many it showed, and whether the person stopped them.
    dialogs: Cell<(u64, u32, bool)>,
    /// The key code of a press handed to Wings, whose key-up and characters the page doesn't get either.
    forwarded: Cell<Option<i32>>,
    /// It was made for a window another page opened: CEF made its browser and loads its first page, which keeps the
    /// opener, and its opener's browser and the popup's number name it until then (`on_before_popup_aborted`).
    popup: Option<(i32, i32)>,
    /// What downloads its documents may start, the addresses CEF said you started one from (`can_download`), and its
    /// downloads under way: Wings' id by CEF's, and how to stop each.
    gate: RefCell<downloads::Gate>,
    asked: RefCell<Vec<String>>,
    downloads: RefCell<HashMap<u32, u64>>,
    stops: RefCell<HashMap<u32, DownloadItemCallback>>,
}

struct Page(Rc<Shared>);

impl Native for Page {
    fn place(&mut self, window: &Window, frame: Option<Bounds<Pixels>>) {
        if let Some(parent) = ns_window_of(window) {
            self.0.parent.replace(Some(parent));
        }
        self.0.want.set(frame);
        self.0.live.set(false);
        self.0.apply_place();
    }

    fn keep_live(&mut self, window: &Window, frame: Bounds<Pixels>) {
        if let Some(parent) = ns_window_of(window) {
            self.0.parent.replace(Some(parent));
        }
        self.0.want.set(Some(frame));
        self.0.live.set(true);
        self.0.apply_place();
    }

    fn overlaid(&mut self, over: &Overlaid) -> bool {
        self.0.over.replace(over.clone());
        self.0.apply_over();
        true
    }

    fn go(&mut self, go: &WebGo, token: u64) {
        self.0.go(go, token);
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        self.0.close();
        runtime::gone();
    }
}

impl Shared {
    /// A page of `profile` at `url`, kept in the list of pages; `popup` names the opener's browser and the popup's
    /// number for one made for a window a page opened.
    fn new(told: Tell, profile: &Profile, keep_data: bool, scripts: bool, url: Option<&str>, popup: Option<(i32, i32)>) -> Rc<Self> {
        let page = Rc::new(Shared {
            told,
            profile: profile.clone(),
            keep_data,
            scripts,
            web_dir: profile.dir.clone(),
            start: RefCell::new(url.unwrap_or("about:blank").to_owned()),
            now: RefCell::new(Now { url: url.unwrap_or_default().to_owned(), loading: true, ..Now::default() }),
            told_last: RefCell::new(None),
            committed: RefCell::default(),
            generation: Cell::new(0),
            chain_tainted: Cell::new(false),
            agent: agent::Agent::default(),
            ready: Cell::new(false),
            clear_first: Cell::new(false),
            browser: RefCell::new(None),
            window: RefCell::new(None),
            ns: RefCell::new(None),
            parent: RefCell::new(None),
            want: Cell::new(None),
            over: RefCell::default(),
            dim: RefCell::new(None),
            attached: Cell::new(false),
            live: Cell::new(false),
            closed: Cell::new(false),
            pending: RefCell::default(),
            dialog: RefCell::new(None),
            sheet: RefCell::new(None),
            certificate: RefCell::new(None),
            navigating: RefCell::default(),
            auth: RefCell::new(None),
            auth_sheet: RefCell::new(None),
            zoom: Cell::new(100),
            find: RefCell::new(None),
            dialogs: Cell::new((0, 0, false)),
            forwarded: Cell::new(None),
            popup,
            gate: RefCell::default(),
            asked: RefCell::default(),
            downloads: RefCell::default(),
            stops: RefCell::default(),
        });
        runtime::opened();
        PAGES.with_borrow_mut(|pages| pages.push(Rc::downgrade(&page)));
        page
    }

    /// Whether it was made for a window another page opened.
    pub(super) fn is_popup(&self) -> bool {
        self.popup.is_some()
    }

    /// Whether what CEF says of its browser is told: not the blank page an opened one shows until it is ready (`ready`),
    /// but all of a popup's, whose first page is the one it opened with.
    fn telling(&self) -> bool {
        self.ready.get() || self.popup.is_some()
    }

    /// A window this page opened at `url` with your click: a page made for it, whose browser CEF makes with its opener
    /// (`client`), and which waits for this page's plugin to show it as a tab (`views::popup`).
    fn opened_popup(&self, popup_id: i32, url: &str, background: bool) -> Rc<Shared> {
        let opener = self.browser.borrow().as_ref().map_or(0, Browser::identifier);
        let tell = self.told.for_popup();
        let popup = Shared::new(tell.clone(), &self.profile, self.keep_data, self.scripts, (!url.is_empty()).then_some(url), Some((opener, popup_id)));
        popup.parent.replace(self.parent.borrow().clone());
        views::popup(&self.told, &self.profile, self.keep_data, crate::web::engine::Kind::Chromium, Box::new(Page(popup.clone())), tell, url, background);
        popup
    }

    /// CEF made the browser view of a window a page opened: when it is this popup's, it goes in a window of Wings' own,
    /// placed as any page's is.
    fn host(self: &Rc<Self>, view: &BrowserView) {
        let bounds = self.screen_rect().map_or(Rect { x: -10_000, y: -10_000, width: 800, height: 600 }, cef_rect);
        let (weak, activated) = (Rc::downgrade(self), Rc::downgrade(self));
        frameless::host(
            Some(view.clone()),
            bounds,
            Rc::new(move |window, _view| {
                if let Some(page) = weak.upgrade() {
                    page.created(window);
                }
            }),
            Rc::new(move |active| {
                if let Some(page) = activated.upgrade() {
                    focus::page_active(&page, active);
                }
            }),
        );
    }

    /// Opens its window, once CEF is up.
    fn open(self: &Rc<Self>) {
        // A plugin that may run scripts in its pages gets the address once its channel is readied (`ready`), and a page
        // whose context's storage a Clear data left to it once that's cleared.
        let waiting = CLEAR_NEXT.with_borrow_mut(|next| {
            let at = next.iter().position(|(profile, keep_data)| *profile == self.profile && *keep_data == self.keep_data);
            at.map(|at| next.remove(at)).is_some()
        });
        self.clear_first.set(waiting);
        let url = if self.scripts || waiting { "about:blank".to_owned() } else { self.start.borrow().clone() };
        let client = PageClient::new(Rc::downgrade(self));
        let context = runtime::context(&self.profile, self.keep_data);
        let bounds = self.screen_rect().map_or(Rect { x: -10_000, y: -10_000, width: 800, height: 600 }, cef_rect);
        let weak = Rc::downgrade(self);
        let activated = Rc::downgrade(self);
        frameless::open(
            &url,
            bounds,
            Some(client),
            context,
            Some(frameless::PAGE_BACKGROUND),
            Some(Rc::new(host_popup) as frameless::OnPopup),
            Rc::new(move |window, _view| {
                if let Some(page) = weak.upgrade() {
                    page.created(window);
                }
            }),
            Rc::new(move |active| {
                if let Some(page) = activated.upgrade() {
                    focus::page_active(&page, active);
                }
            }),
        );
    }

    /// Its window exists, hidden: it shows where the page's place is, without taking the keyboard.
    fn created(&self, window: &mut cef::Window) {
        if self.closed.get() {
            window.close();
            return;
        }
        let Some(ns) = frameless::ns_window(window) else { return };
        frameless::never_main(&ns);
        frameless::never_fullscreen(&ns);
        focus::follow_mouse();
        let parent = self.parent.borrow().clone();
        if let Some(parent) = &parent {
            frameless::count_children_as_key(parent);
        }
        let parent_was_key = parent.as_ref().is_some_and(|parent| parent.isKeyWindow());
        // CEF draws only a window it shows; showing it makes it key.
        window.show();
        frameless::as_page(&ns);
        self.window.replace(Some(window.clone()));
        self.ns.replace(Some(ns));
        self.attached.set(false);
        self.apply_place();
        if let (true, Some(parent)) = (parent_was_key, parent) {
            parent.makeKeyWindow();
        }
    }

    /// Shows its window at its place over the Wings window, or hides it.
    fn apply_place(&self) {
        let (Some(ns), Some(parent)) = (self.ns.borrow().clone(), self.parent.borrow().clone()) else { return };
        match self.screen_rect() {
            Some(rect) if self.attached.get() => ns.setFrame_display(rect, true),
            Some(rect) => {
                frameless::attach(&ns, &parent, rect);
                self.attached.set(true);
            }
            None if self.attached.replace(false) => frameless::detach(&ns, &parent),
            None => ns.orderOut(None),
        }
        self.apply_over();
    }

    /// Cuts what lies over it out of its window, which is clear there: the menu or popover the Wings window draws under it
    /// shows, and a click there goes to it, as a press on a window's clear part goes to the one under it. Hiding the whole
    /// page for a menu left it black under the menu. Under a dialog's backdrop it is dimmed as the window is, and the
    /// clicks and keys go to the dialog.
    fn apply_over(&self) {
        use objc2_core_foundation::{CGPoint, CGRect, CGSize};
        use objc2_core_graphics::CGMutablePath;
        use objc2_quartz_core::{CAShapeLayer, kCAFillRuleEvenOdd};

        let Some(ns) = self.ns.borrow().clone() else { return };
        let Some(content) = ns.contentView() else { return };
        let over = self.over.borrow();
        // Chromium stops rendering a window that is ordered out, so a live one stays attached and wholly masked instead:
        // Chromium sets the window's own opacity as it shows it, which showed a live page whole over Wings at launch.
        let live = self.live.get();
        ns.setIgnoresMouseEvents(over.dimmed || live);
        if (over.dimmed || live)
            && ns.isKeyWindow()
            && let Some(parent) = self.parent.borrow().as_ref()
        {
            parent.makeKeyWindow();
        }
        self.apply_dim(&content, over.dimmed && !live);
        let size = content.bounds().size;
        let rect = |x: f64, y: f64, w: f64, h: f64| CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
        // SAFETY: AppKit's and Core Animation's objects, on the main thread; no transform for the path.
        unsafe {
            let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![&*content, layer];
            if layer.is_null() {
                return;
            }
            if over.holes.is_empty() && !live {
                let _: () = objc2::msg_send![layer, setMask: std::ptr::null::<objc2::runtime::AnyObject>()];
                return;
            }
            ns.setOpaque(false);
            ns.setBackgroundColor(Some(&objc2_app_kit::NSColor::clearColor()));
            if live {
                // An empty mask shows nothing of it.
                let _: () = objc2::msg_send![layer, setMask: &*objc2_quartz_core::CALayer::new()];
                return;
            }
            let path = CGMutablePath::new();
            CGMutablePath::add_rect(Some(&path), std::ptr::null(), rect(0.0, 0.0, size.width, size.height));
            let flipped = content.isFlipped();
            for hole in over.holes.iter() {
                let (x, y) = (f64::from(f32::from(hole.at.origin.x)), f64::from(f32::from(hole.at.origin.y)));
                let (w, h) = (f64::from(f32::from(hole.at.size.width)), f64::from(f32::from(hole.at.size.height)));
                // From the top left, as the hole is given, to the layer's own.
                let at = |x: f64, y: f64| (x, if flipped { y } else { size.height - y });
                let corners = [(x + w, y), (x + w, y + h), (x, y + h), (x, y)];
                let round = |corner: usize| f64::from(hole.corners[(corner + 1) % 4]);
                let (start_x, start_y) = at(x + f64::from(hole.corners[0]), y);
                CGMutablePath::move_to_point(Some(&path), std::ptr::null(), start_x, start_y);
                // Round each corner toward the next one, top right first.
                for (index, &(corner_x, corner_y)) in corners.iter().enumerate() {
                    let (cx, cy) = at(corner_x, corner_y);
                    if round(index) > 0.0 {
                        let (next_x, next_y) = corners[(index + 1) % 4];
                        let (nx, ny) = at(next_x, next_y);
                        CGMutablePath::add_arc_to_point(Some(&path), std::ptr::null(), cx, cy, nx, ny, round(index));
                    } else {
                        CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), cx, cy);
                    }
                }
                CGMutablePath::close_subpath(Some(&path));
            }
            let mask = CAShapeLayer::new();
            mask.setFrame(rect(0.0, 0.0, size.width, size.height));
            mask.setFillRule(kCAFillRuleEvenOdd);
            mask.setPath(Some(&path));
            let _: () = objc2::msg_send![layer, setMask: &*mask];
        }
    }

    /// Dims the page under a dialog's backdrop as the backdrop dims the rest of the window, or stops.
    fn apply_dim(&self, content: &NSView, dimmed: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let mut dim = self.dim.borrow_mut();
        if !dimmed {
            if let Some(view) = dim.as_ref() {
                view.setHidden(true);
            }
            return;
        }
        let view = dim.get_or_insert_with(|| {
            let view = NSView::initWithFrame(<NSView as objc2::MainThreadOnly>::alloc(mtm), content.bounds());
            view.setWantsLayer(true);
            view.setAutoresizingMask(
                objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
                    | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            let backdrop = gpui::Rgba::from(crate::ui::backdrop_color());
            let color = objc2_core_graphics::CGColor::new_srgb(
                f64::from(backdrop.r),
                f64::from(backdrop.g),
                f64::from(backdrop.b),
                f64::from(backdrop.a),
            );
            // SAFETY: the view's own layer, made by `setWantsLayer`, on the main thread.
            let layer: Option<Retained<objc2_quartz_core::CALayer>> = unsafe { objc2::msg_send![&*view, layer] };
            if let Some(layer) = layer {
                layer.setBackgroundColor(Some(&color));
            }
            view
        });
        view.setFrame(content.bounds());
        view.setHidden(false);
        // Over the page's own views, which Chromium may have added since.
        content.addSubview_positioned_relativeTo(view, objc2_app_kit::NSWindowOrderingMode::Above, None);
    }

    /// Where it shows on screen, from its place in the Wings window.
    fn screen_rect(&self) -> Option<NSRect> {
        let frame = self.want.get()?;
        let parent = self.parent.borrow().clone()?;
        let content = parent.contentView()?;
        let height = content.bounds().size.height;
        let (x, y) = (f64::from(f32::from(frame.origin.x)), f64::from(f32::from(frame.origin.y)));
        let (w, h) = (f64::from(f32::from(frame.size.width)), f64::from(f32::from(frame.size.height)));
        let y = if content.isFlipped() { y } else { height - y - h };
        let in_window = content.convertRect_toView(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)), None);
        Some(parent.convertRectToScreen(in_window))
    }

    /// Closes its window and browser, and answers a dialog still open as Cancel.
    pub(super) fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        self.answer(false, None);
        self.resolve_certificate(prompts::Choice::Back);
        self.resolve_auth(None);
        self.agent.page_closed();
        if let (Some(ns), Some(parent)) = (self.ns.borrow().clone(), self.parent.borrow().clone()) {
            frameless::detach(&ns, &parent);
        }
        if let Some(window) = self.window.take() {
            window.close();
        }
        super::extensions::page_closed(self);
    }

    fn go(&self, go: &WebGo, token: u64) {
        if !self.ready.get() {
            // Before it is ready a load only changes where it opens.
            if let WebGo::Load(url) = go {
                self.start.replace(url.clone());
                return;
            }
            self.pending.borrow_mut().push((go.clone(), token));
            return;
        }
        let committed = self.committed.borrow().clone();
        let now = self.now.borrow().clone();
        let (imported, settled) = match go {
            WebGo::Dialog { .. } => {
                let site = self.dialog.borrow().as_ref().map(|(_, _, dialog)| dialog.site.clone()).unwrap_or_default();
                (site.is_empty() || crate::web::holds_imported_host(&self.web_dir, &site) || crate::web::holds_imported(&self.web_dir, &committed), true)
            }
            _ => (crate::web::holds_imported(&self.web_dir, &committed), !now.loading && crate::web::origin(&committed) == crate::web::origin(&now.url)),
        };
        if let Err(why) = crate::web::gate(go, self.scripts, imported, settled) {
            return self.told.tell(Heard::Result { token, result: Err(why.into()) });
        }
        let Some(browser) = self.browser.borrow().clone() else { return };
        match go {
            WebGo::Load(url) => {
                if let Some(frame) = browser.main_frame() {
                    frame.load_url(Some(&CefString::from(url.as_str())));
                }
            }
            WebGo::Back => browser.go_back(),
            WebGo::Forward => browser.go_forward(),
            WebGo::Reload => {
                if self.now.borrow().error.as_deref() == Some(CRASHED) {
                    self.now.borrow_mut().error = None;
                }
                browser.reload()
            }
            WebGo::Stop => browser.stop_load(),
            WebGo::Logs => self.told.tell(Heard::Result { token, result: agent::logs(self) }),
            WebGo::Dialog { accept, text } => {
                let answered = self.answer(*accept, text.as_deref());
                let result = if answered { Ok(serde_json::Value::Null) } else { Err("The page shows no dialog".into()) };
                self.told.tell(Heard::Result { token, result });
            }
            WebGo::Extension(id) => {
                let result = super::extensions::click(self, id);
                self.told.tell(Heard::Result { token, result: result.map(|()| serde_json::Value::Null) });
            }
            WebGo::Eval { script, page } => agent::eval(self, script, *page, token),
            WebGo::Snapshot => agent::screenshot(self, token),
            WebGo::Upload { reference, selector, files } => agent::upload(self, reference.as_deref(), selector.as_deref(), files, token),
            WebGo::ScreenshotDiff { baseline, threshold, clip, current, diff } => agent::screenshot_diff(self, baseline, *threshold, *clip, current.as_deref(), diff.as_deref(), token),
            WebGo::Record(record) => agent::record(self, record, token),
            WebGo::Input(events) => agent::input(self, events, token),
            WebGo::Network(wanted) => agent::network(self, wanted, token),
            // Done by `views::update`, whichever engine wrote the file.
            WebGo::Download { .. } => {}
            WebGo::Certificate { proceed } => {
                let waiting = self.certificate.borrow().as_ref().map(|(_, notice)| notice.clone());
                let choice = if *proceed { prompts::Choice::Proceed } else { prompts::Choice::Back };
                let result = match prompts::decide(&self.web_dir, waiting.as_ref(), choice) {
                    Some(choice) => {
                        self.resolve_certificate(choice);
                        Ok(serde_json::Value::Null)
                    }
                    None => Err("The page waits on no certificate choice".into()),
                };
                self.told.tell(Heard::Result { token, result });
            }
            WebGo::Find { text, forward, again } => {
                if let Some(host) = browser.host() {
                    self.find.replace(Some((text.clone(), None, 0)));
                    host.find(Some(&CefString::from(text.as_str())), i32::from(*forward), 0, i32::from(*again));
                    self.tell();
                }
            }
            WebGo::StopFind => {
                if let Some(host) = browser.host() {
                    host.stop_finding(1);
                }
                self.find.replace(None);
                self.tell();
            }
            WebGo::Zoom(zoom) => {
                self.zoom.set(prompts::zoom_after(self.zoom.get(), zoom.step()));
                self.apply_zoom();
                self.tell();
            }
            WebGo::Inspect => {
                if let Some(host) = browser.host() {
                    // In a window of its own, with a client of its own that `StrayClient`'s rules don't close.
                    host.show_dev_tools(None, Some(&mut runtime::inspector()), None, None);
                }
            }
        }
    }

    /// Puts the view's zoom on the page: Chromium keeps its own per site, so it is set again as each document loads.
    fn apply_zoom(&self) {
        if let Some(host) = self.browser.borrow().as_ref().and_then(Browser::host) {
            host.set_zoom_level(prompts::chromium_level(self.zoom.get()));
        }
    }

    /// Carries out the person's choice for the certificate notice that waits: Chromium goes on with the site or gives up.
    fn resolve_certificate(&self, choice: prompts::Choice) {
        let Some((callback, _)) = self.certificate.take() else { return };
        match choice {
            prompts::Choice::Proceed => callback.cont(),
            prompts::Choice::Back => callback.cancel(),
        }
        self.tell();
    }

    /// The server asks for a name and a password: Wings' sheet asks the person, naming the site and the realm, and gives
    /// the answer to Chromium only. A page not on screen, or one asking again, is cancelled.
    fn wait_for_auth(&self, callback: AuthCallback, site: String, realm: &str, proxy: bool) {
        self.resolve_auth(None);
        let (Some(mtm), Some(window)) = (MainThreadMarker::new(), self.parent.borrow().clone()) else { return callback.cancel() };
        if self.want.get().is_none() {
            return callback.cancel();
        }
        let (title, message) = prompts::auth_words(&site, realm, proxy, false);
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&title));
        alert.setInformativeText(&NSString::from_str(&message));
        alert.addButtonWithTitle(&NSString::from_str("Sign In"));
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        let (name, password) = {
            let frame = |y: f64| NSRect::new(NSPoint::new(0.0, y), NSSize::new(260.0, 24.0));
            let name = NSTextField::textFieldWithString(&NSString::new(), mtm);
            name.setFrame(frame(30.0));
            name.setPlaceholderString(Some(&NSString::from_str("Name")));
            let password = NSSecureTextField::initWithFrame(<NSSecureTextField as objc2::MainThreadOnly>::alloc(mtm), frame(0.0));
            password.setPlaceholderString(Some(&NSString::from_str("Password")));
            (name, password)
        };
        let fields = NSView::initWithFrame(<NSView as objc2::MainThreadOnly>::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(260.0, 54.0)));
        fields.addSubview(&name);
        fields.addSubview(&password);
        alert.setAccessoryView(Some(&fields));
        alert.window().setInitialFirstResponder(Some(&name));
        // The page's window is over the sheet's place: it hides while the sheet shows.
        if let (Some(ns), Some(parent)) = (self.ns.borrow().clone(), self.parent.borrow().clone())
            && self.attached.replace(false)
        {
            frameless::detach(&ns, &parent);
        }
        self.auth.replace(Some((callback, site)));
        let page = weak_of(self);
        let answered = block2::RcBlock::new(move |response: NSModalResponse| {
            let Some(page) = page.upgrade() else { return };
            page.auth_sheet.replace(None);
            // NSAlertFirstButtonReturn, Sign In, is 1000.
            let given = (response == 1000).then(|| prompts::credentials(&name.stringValue().to_string(), &password.stringValue().to_string())).flatten();
            password.setStringValue(&NSString::new());
            page.resolve_auth(given);
            page.apply_place();
            page.tell();
        });
        self.auth_sheet.replace(Some(alert.clone()));
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&answered));
        self.tell();
    }

    /// Gives Chromium the name and password the person typed, or cancels the request for none.
    fn resolve_auth(&self, given: Option<(String, String)>) {
        let Some((callback, _)) = self.auth.take() else { return };
        match given {
            Some((name, password)) => callback.cont(Some(&CefString::from(name.as_str())), Some(&CefString::from(password.as_str()))),
            None => callback.cancel(),
        }
        if let Some(alert) = self.auth_sheet.take()
            && let Some(parent) = alert.window().sheetParent()
        {
            parent.endSheet_returnCode(&alert.window(), 1001);
        }
        self.tell();
    }

    /// The address it shows now.
    pub(super) fn url_now(&self) -> String {
        self.now.borrow().url.clone()
    }

    /// What a debug build's checks read of where it is (`debug`).
    #[cfg(debug_assertions)]
    pub(super) fn describe(&self) -> String {
        let now = self.now.borrow();
        format!("ready {}, loading {}, at {}, committed {}, generation {}; {}", self.ready.get(), now.loading, now.url, self.committed.borrow(), self.generation.get(), self.agent.describe())
    }

    /// Tells the page where it is, if that changed since it last heard.
    pub(super) fn tell(&self) {
        let now = self.now.borrow().clone();
        let imported = crate::web::holds_imported(&self.web_dir, &self.committed.borrow());
        let extensions = if self.keep_data { super::extensions::buttons(&self.profile, imported, &now.url) } else { Vec::new() };
        let state = State {
            url: now.url,
            title: now.title,
            loading: now.loading,
            can_go_back: now.back,
            can_go_forward: now.forward,
            error: now.error,
            dialog: self.dialog.borrow().as_ref().map(|(_, _, dialog)| dialog.clone()),
            imported,
            extensions,
            engine: "chromium",
            certificate: self.certificate.borrow().as_ref().map(|(_, notice)| views::Certificate { site: notice.site.clone(), reason: notice.reason.clone() }),
            auth: self.auth.borrow().as_ref().map(|(_, site)| views::Auth { site: site.clone() }),
            find: self.find.borrow().as_ref().map(|(_, matches, current)| {
                let (matches, current, found) = prompts::shown_find(*matches, *current, imported);
                views::Found { matches, current, found }
            }),
            zoom: self.zoom.get(),
        };
        if self.told_last.borrow().as_ref() != Some(&state) {
            self.told_last.replace(Some(state.clone()));
            self.told.tell(Heard::State(state));
        }
    }

    /// Its browser exists: for a plugin that may run scripts in its pages its DevTools channel is readied, then it is
    /// ready (`readied`).
    fn browser_created(&self, browser: &Browser) {
        self.browser.replace(Some(browser.clone()));
        if self.clear_first.get() {
            let (page, again) = (weak_of(self), browser.clone());
            return super::devtools::once(browser, "Storage.clearDataForOrigin", clear_all(), move |cleared| {
                if let Err(why) = cleared {
                    eprintln!("[chromium] a page's sites' storage wasn't cleared: {why}");
                }
                if let Some(page) = page.upgrade() {
                    page.go_on(&again);
                }
            });
        }
        // CEF makes a popup's browser while its opener waits on `window.open`, when its DevTools channel takes nothing
        // yet: it is opened once that is over.
        if self.popup.is_some() {
            let (page, again) = (weak_of(self), browser.clone());
            return super::next_turn(move || {
                if let Some(page) = page.upgrade().filter(|page| !page.closed.get()) {
                    page.go_on(&again);
                }
            });
        }
        self.go_on(browser);
    }

    /// Its browser is ready to go on: to its channel first, for a plugin that may run scripts in its pages.
    fn go_on(&self, browser: &Browser) {
        if !self.scripts {
            // A window a page opens waits for a debugger before it runs (`popup_waits`), and nothing else lets it go.
            if self.popup.is_some() {
                let (method, params) = agent::popup_waits();
                super::devtools::once(browser, method, params, |_| {});
            }
            return self.readied(Ok(()));
        }
        let page = weak_of(self);
        agent::open(self, browser, move |result| {
            // On a turn of its own: Chromium may answer the last step while it is being sent.
            super::next_turn(move || {
                if let Some(page) = page.upgrade() {
                    page.readied(result);
                }
            });
        });
    }

    /// It is ready: it goes where it was asked to, and the commands sent before run now. A channel that couldn't be
    /// readied leaves the page to show, and nothing reads or acts on it (`agent::Agent::guarded`).
    fn readied(&self, result: Result<(), String>) {
        if self.closed.get() || self.ready.replace(true) {
            return;
        }
        if let Err(why) = result {
            eprintln!("[chromium] a page's DevTools channel isn't readied, so nothing reads or acts on it: {why}");
        }
        if (self.scripts || self.clear_first.get()) && self.popup.is_none() {
            let start = self.start.borrow().clone();
            match self.browser.borrow().as_ref().and_then(Browser::main_frame) {
                Some(frame) if start != "about:blank" => frame.load_url(Some(&CefString::from(start.as_str()))),
                _ => self.now.borrow_mut().loading = false,
            }
            self.tell();
        }
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for (go, token) in pending {
            self.go(&go, token);
        }
    }

    /// What its DevTools channel heard.
    pub(super) fn devtools(&self, incoming: Incoming) {
        agent::heard(self, incoming);
    }

    /// The main frame committed a new document at `url`.
    fn committed(&self, url: String) {
        if self.chain_tainted.replace(false)
            && let Some(host) = crate::web::origin(&url).and_then(|origin| origin.split_once("://").map(|(_, host)| host.split(':').next().unwrap_or_default().to_owned()))
        {
            let _ = crate::web::note_imported(&self.web_dir, [host]);
        }
        self.committed.replace(url);
        self.agent.clear_logs();
        self.generation.set(self.generation.get() + 1);
    }

    /// The page waits on a dialog: Wings shows it in a sheet of its own on the window, naming the site, which the person
    /// answers; its plugin hears of it (`State::dialog`) and, where `gate` lets it, may answer it too.
    fn wait_on(&self, callback: JsdialogCallback, kind: JsdialogType, message: String, default: Option<String>, origin: &str) {
        self.answer(false, None);
        let host = crate::web::origin(origin).and_then(|origin| origin.split_once("://").map(|(_, host)| host.split(':').next().unwrap_or_default().to_owned())).unwrap_or_default();
        let name = if kind == JsdialogType::CONFIRM { "confirm" } else if kind == JsdialogType::PROMPT { "prompt" } else { "alert" };
        let message: String = message.chars().take(4000).collect();
        let private = host.is_empty() || crate::web::holds_imported_host(&self.web_dir, &host);
        let dialog = match private {
            true => Dialog { kind: name.into(), ..Default::default() },
            false => Dialog { kind: name.into(), message: message.clone(), default_text: default.clone(), site: host.clone() },
        };
        self.dialog.replace(Some((callback, kind, dialog)));
        // A page whose dialogs the person stopped, or one not on screen, gets Cancel at once.
        let (generation, count, stopped) = self.dialogs.get();
        let count = if generation == self.generation.get() { count + 1 } else { 1 };
        let stopped = stopped && generation == self.generation.get();
        self.dialogs.set((self.generation.get(), count, stopped));
        if stopped || self.want.get().is_none() || self.parent.borrow().is_none() {
            self.resolve(false, None);
        } else {
            self.show_sheet(name, &host, &message, default.as_deref(), count > 1);
        }
        self.tell();
    }

    fn show_sheet(&self, kind: &str, site: &str, message: &str, default: Option<&str>, offer_stop: bool) {
        let (Some(mtm), Some(window)) = (MainThreadMarker::new(), self.parent.borrow().clone()) else { return };
        let alert = NSAlert::new(mtm);
        let site = if site.is_empty() { "This page" } else { site };
        alert.setMessageText(&NSString::from_str(&format!("{site} says")));
        alert.setInformativeText(&NSString::from_str(message));
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        if kind != "alert" {
            alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        }
        let field = (kind == "prompt").then(|| {
            let field = NSTextField::textFieldWithString(&NSString::from_str(default.unwrap_or_default()), mtm);
            field.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(260.0, 24.0)));
            alert.setAccessoryView(Some(&field));
            field
        });
        if offer_stop {
            alert.setShowsSuppressionButton(true);
            if let Some(stop) = alert.suppressionButton() {
                stop.setTitle(&NSString::from_str("Stop dialogs from this page"));
            }
        }
        // The page's window is over the sheet's place: it hides while the sheet shows.
        if let (Some(ns), Some(parent)) = (self.ns.borrow().clone(), self.parent.borrow().clone())
            && self.attached.replace(false)
        {
            frameless::detach(&ns, &parent);
        }
        let page = weak_of(self);
        let (typed, asked) = (field.clone(), alert.clone());
        let answered = block2::RcBlock::new(move |response: NSModalResponse| {
            let Some(page) = page.upgrade() else { return };
            page.sheet.replace(None);
            if asked.suppressionButton().is_some_and(|stop| stop.state() == 1) {
                let (generation, count, _) = page.dialogs.get();
                page.dialogs.set((generation, count, true));
            }
            let text = typed.as_ref().map(|field| field.stringValue().to_string());
            page.resolve(response == 1000, text.as_deref());
            page.apply_place();
            page.tell();
        });
        self.sheet.replace(Some((alert.clone(), field)));
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&answered));
    }

    /// Answers the page's dialog, if it shows one, as OK or Cancel: whether it did.
    fn answer(&self, accept: bool, text: Option<&str>) -> bool {
        if self.dialog.borrow().is_none() {
            return false;
        }
        let sheet = self.sheet.borrow().clone();
        match sheet.and_then(|(alert, field)| Some((alert.window().sheetParent()?, alert, field))) {
            Some((parent, alert, field)) => {
                if let (Some(field), Some(text)) = (field, text) {
                    field.setStringValue(&NSString::from_str(text));
                }
                parent.endSheet_returnCode(&alert.window(), if accept { 1000 } else { 1001 });
            }
            None => {
                self.sheet.replace(None);
                self.resolve(accept, text);
            }
        }
        true
    }

    /// Hands CEF the answer to the page's dialog, and tells the page it has none.
    fn resolve(&self, accept: bool, text: Option<&str>) {
        let Some((callback, kind, dialog)) = self.dialog.take() else { return };
        let answer = (kind == JsdialogType::PROMPT && accept).then(|| CefString::from(text.or(dialog.default_text.as_deref()).unwrap_or_default()));
        callback.cont(i32::from(accept || kind == JsdialogType::ALERT), answer.as_ref());
    }

    fn browser_is(&self, browser: Option<&mut Browser>) -> bool {
        let mine = self.browser.borrow().as_ref().map(Browser::identifier);
        browser.is_some_and(|browser| mine.is_none_or(|mine| mine == browser.identifier()))
    }
}

/// CEF made the browser view of a window a page opened (`frameless::OnPopup`): the popup page made for it puts it in a
/// window of its own. CEF calls this before the popup's `on_after_created` (measured in 158), so the page is the oldest
/// popup page with neither a browser nor a window yet, as CEF makes popups in the order they are asked for. False for
/// one no page made, which CEF opens in a window of its own.
fn host_popup(view: &BrowserView) -> bool {
    let page = PAGES.with_borrow(|pages| {
        pages.iter().filter_map(Weak::upgrade).find(|page| page.popup.is_some() && !page.closed.get() && page.browser.borrow().is_none() && page.window.borrow().is_none())
    });
    page.inspect(|page| page.host(view)).is_some()
}

/// The Wings window `window` is, in AppKit.
fn ns_window_of(window: &Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
    // SAFETY: GPUI's window view, alive while its window is.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    view.window()
}

/// AppKit's screen rect (bottom-left origin) as CEF Views takes it (top-left origin of the main screen).
fn cef_rect(frame: NSRect) -> Rect {
    let screen_height = MainThreadMarker::new()
        .and_then(|mtm| objc2_app_kit::NSScreen::screens(mtm).firstObject())
        .map_or(0.0, |screen| screen.frame().size.height);
    Rect {
        x: frame.origin.x as i32,
        y: (screen_height - frame.origin.y - frame.size.height) as i32,
        width: frame.size.width as i32,
        height: frame.size.height as i32,
    }
}

fn text(value: Option<&CefString>) -> String {
    value.map(CefString::to_string).unwrap_or_default()
}

wrap_completion_callback! {
    struct CacheCleared {
        done: Rc<dyn Fn()>,
    }

    impl CompletionCallback {
        fn on_complete(&self) {
            (self.done)();
        }
    }
}

wrap_client! {
    struct PageClient {
        page: Weak<Shared>,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(PageLifeSpan::new(self.page.clone()))
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(PageLoad::new(self.page.clone()))
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(PageDisplay::new(self.page.clone()))
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(PageRequests::new(self.page.clone()))
        }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(PageKeys::new(self.page.clone()))
        }
        fn find_handler(&self) -> Option<FindHandler> {
            Some(PageFind::new(self.page.clone()))
        }
        fn jsdialog_handler(&self) -> Option<JsdialogHandler> {
            Some(PageDialogs::new(self.page.clone()))
        }
        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(Refuse::new())
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(PageDownloads::new(self.page.clone()))
        }
    }
}

wrap_life_span_handler! {
    struct PageLifeSpan {
        page: Weak<Shared>,
    }

    impl LifeSpanHandler {
        /// A link that opens a new window (`target="_blank"`, `window.open`) opens as a tab of its own: CEF makes its
        /// browser, with its opener, so `window.opener` and `postMessage` back work as a sign-in in a window needs, and
        /// Wings puts it in a window of its own (`host_popup`). Only one your click led to, as Chrome's blocker does; the
        /// page hears of one it opened by itself. A page that doesn't show such tabs loads one you clicked in itself.
        fn on_before_popup(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let url = text(target_url);
            // A window opened with no address is blank until its opener sends it somewhere.
            let Some(opener) = self.page.upgrade().filter(|_| url.is_empty() || crate::web::may_navigate(&url, true)) else { return 1 };
            if !opener.told.popups() {
                if user_gesture != 0
                    && !url.is_empty()
                    && let Some(frame) = browser.and_then(|browser| browser.main_frame())
                {
                    frame.load_url(Some(&CefString::from(url.as_str())));
                }
                return 1;
            }
            if user_gesture == 0 {
                opener.told.tell(Heard::Blocked { url });
                return 1;
            }
            let Some(client) = client else { return 1 };
            let popup = opener.opened_popup(popup_id, &url, target_disposition == WindowOpenDisposition::NEW_BACKGROUND_TAB);
            *client = Some(PageClient::new(Rc::downgrade(&popup)));
            0
        }

        /// CEF didn't make the window after all: its tab closes.
        fn on_before_popup_aborted(&self, browser: Option<&mut Browser>, popup_id: ::std::os::raw::c_int) {
            let Some(opener) = browser.map(|browser| browser.identifier()) else { return };
            let popup = PAGES.with_borrow(|pages| pages.iter().filter_map(Weak::upgrade).find(|page| page.popup == Some((opener, popup_id))));
            if let Some(popup) = popup {
                popup.told.tell(Heard::Closed);
            }
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser else { return };
            runtime::created(browser);
            if let Some(page) = self.page.upgrade() {
                page.browser_created(browser);
            }
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser else { return };
            runtime::closed(browser);
            // Closing while Wings didn't close it: the page closed its own window (`window.close()`), and its tab closes.
            if let Some(page) = self.page.upgrade().filter(|page| !page.closed.get() && page.browser_is(Some(browser))) {
                page.told.tell(Heard::Closed);
            }
        }
    }
}

wrap_load_handler! {
    struct PageLoad {
        page: Weak<Shared>,
    }

    impl LoadHandler {
        fn on_loading_state_change(&self, browser: Option<&mut Browser>, is_loading: ::std::os::raw::c_int, can_go_back: ::std::os::raw::c_int, can_go_forward: ::std::os::raw::c_int) {
            // The blank page it shows until it is ready says nothing.
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser) && page.telling()) else { return };
            {
                let mut now = page.now.borrow_mut();
                now.loading = is_loading != 0;
                now.back = can_go_back != 0;
                now.forward = can_go_forward != 0;
                // A new load clears the last one's error.
                if now.loading {
                    now.error = None;
                }
            }
            page.tell();
        }

        /// The main frame committed a new document: commands act on it from now.
        fn on_load_start(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, _transition_type: TransitionType) {
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser)) else { return };
            if let Some(frame) = frame.filter(|frame| frame.is_main() != 0) {
                page.committed(CefString::from(&frame.url()).to_string());
                page.find.replace(None);
                if page.zoom.get() != 100 {
                    page.apply_zoom();
                }
                page.tell();
            }
        }

        fn on_load_error(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, error_code: Errorcode, error_text: Option<&CefString>, _failed_url: Option<&CefString>) {
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser) && page.telling()) else { return };
            // A load stopped by another (a click before the page finished) isn't a failure the person needs to see.
            if frame.is_none_or(|frame| frame.is_main() == 0) || error_code == Errorcode::ABORTED {
                return;
            }
            page.now.borrow_mut().error = Some(load_error(&text(error_text)));
            page.tell();
        }
    }
}

/// The `ERR_CERT_...` name in what Debug writes of a Chromium error code.
fn error_name(debug: &str) -> &str {
    let at = debug.find("ERR_").unwrap_or(debug.len());
    let name = &debug[at..];
    &name[..name.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(name.len())]
}

/// What the page says of a load that failed, from Chromium's error name (`net::ERR_NAME_NOT_RESOLVED`).
pub fn load_error(name: &str) -> String {
    let name = name.trim_start_matches("net::");
    let why = match name {
        "ERR_NAME_NOT_RESOLVED" => "A server with that name couldn't be found",
        "ERR_INTERNET_DISCONNECTED" => "This Mac isn't connected to the internet",
        "ERR_CONNECTION_REFUSED" => "The server refused the connection",
        "ERR_CONNECTION_TIMED_OUT" | "ERR_TIMED_OUT" => "The server took too long to answer",
        name if name.starts_with("ERR_CERT_") => "The site's certificate isn't valid",
        _ => "The page couldn't be opened",
    };
    if name.is_empty() { why.to_owned() } else { format!("{why} ({name})") }
}

wrap_display_handler! {
    struct PageDisplay {
        page: Weak<Shared>,
    }

    impl DisplayHandler {
        fn on_address_change(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser) && page.telling()) else { return };
            if frame.is_some_and(|frame| frame.is_main() != 0) {
                page.now.borrow_mut().url = text(url);
                page.tell();
                super::extensions::page_moved(&page);
            }
        }

        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser) && page.telling()) else { return };
            page.now.borrow_mut().title = text(title);
            page.tell();
        }
    }
}

wrap_request_handler! {
    struct PageRequests {
        page: Weak<Shared>,
    }

    impl RequestHandler {
        /// Only to addresses a web view may go to (`may_navigate`): a link to `file:` or another app's scheme is dropped.
        fn on_before_browse(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, _user_gesture: ::std::os::raw::c_int, _is_redirect: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            let url = request.map(|request| CefString::from(&request.url()).to_string()).unwrap_or_default();
            let main_frame = frame.is_none_or(|frame| frame.is_main() != 0);
            let allowed = crate::web::may_navigate(&url, main_frame);
            let mut browser = browser;
            if allowed
                && main_frame
                && let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser.as_deref_mut()))
            {
                // A new address gives up the certificate choice the last one waited on, and says where the main frame goes.
                page.resolve_certificate(prompts::Choice::Back);
                page.navigating.replace(prompts::site_of(&url).unwrap_or_default());
            }
            if let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser))
                && allowed
                && main_frame
                && crate::web::carries_imported(&page.web_dir, &page.committed.borrow(), &url)
            {
                page.chain_tainted.set(true);
            }
            i32::from(!allowed)
        }

        /// A site whose certificate Chromium refuses: one the person chose to open goes on; the main page asks the person in the
        /// plugin's notice (`State::certificate`), which waits; a picture or a frame from such a site is refused.
        fn on_certificate_error(&self, browser: Option<&mut Browser>, cert_error: Errorcode, request_url: Option<&CefString>, _ssl_info: Option<&mut Sslinfo>, callback: Option<&mut Callback>) -> ::std::os::raw::c_int {
            let (Some(page), Some(callback)) = (self.page.upgrade().filter(|page| page.browser_is(browser)), callback) else { return 0 };
            let url = text(request_url);
            let Some(site) = prompts::site_of(&url) else { return 0 };
            if prompts::trusted(&page.web_dir, &site) {
                callback.cont();
                return 1;
            }
            if *page.navigating.borrow() != site {
                return 0;
            }
            let reason = prompts::chromium_reason(error_name(&format!("{cert_error:?}")));
            page.resolve_certificate(prompts::Choice::Back);
            if let Some(notice) = prompts::notice(&page.web_dir, &url, reason) {
                page.certificate.replace(Some((callback.clone(), notice)));
                page.tell();
                return 1;
            }
            0
        }

        /// A server's basic or digest sign-in asks the person in Wings' sheet; other schemes are refused as before.
        fn auth_credentials(&self, browser: Option<&mut Browser>, _origin_url: Option<&CefString>, is_proxy: ::std::os::raw::c_int, host: Option<&CefString>, port: ::std::os::raw::c_int, realm: Option<&CefString>, scheme: Option<&CefString>, callback: Option<&mut AuthCallback>) -> ::std::os::raw::c_int {
            // Chromium asks on its IO thread (measured in 158), where nothing of the page may be touched: the request goes to
            // the UI thread with the browser's id, which finds the page there (`AskAuth`).
            let (Some(browser), Some(callback)) = (browser, callback) else { return 0 };
            if !prompts::asks_for_a_password(&text(scheme)) {
                return 0;
            }
            let mut task = AskAuth::new(browser.identifier(), callback.clone(), prompts::auth_site(&text(host), port), text(realm), is_proxy != 0);
            post_task(ThreadId::UI, Some(&mut task))
        }

        /// A site asking for a client certificate gets none, with no Chrome dialog to pick one, as WebKit in Wings sends
        /// none (it has no handler for the challenge).
        fn on_select_client_certificate(&self, _browser: Option<&mut Browser>, _is_proxy: ::std::os::raw::c_int, _host: Option<&CefString>, _port: ::std::os::raw::c_int, _certificates: Option<&[Option<X509Certificate>]>, callback: Option<&mut SelectClientCertificateCallback>) -> ::std::os::raw::c_int {
            if let Some(callback) = callback {
                callback.select(None);
            }
            1
        }

        /// A link opened in a new tab (Cmd-click, a middle click) opens in a tab of its own, behind this one for a
        /// background tab; a page that doesn't show such tabs loads it in itself.
        fn on_open_urlfrom_tab(&self, browser: Option<&mut Browser>, _frame: Option<&mut Frame>, target_url: Option<&CefString>, target_disposition: WindowOpenDisposition, user_gesture: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            let url = text(target_url);
            if !crate::web::may_navigate(&url, true) {
                return 1;
            }
            match self.page.upgrade().filter(|page| page.told.popups()) {
                // One a script asked for, as with a made-up Cmd-click, is no tab of yours.
                Some(page) if user_gesture == 0 => page.told.tell(Heard::Blocked { url }),
                Some(page) => page.told.tell(Heard::Popup { popup: None, url, background: target_disposition == WindowOpenDisposition::NEW_BACKGROUND_TAB }),
                None => {
                    if user_gesture != 0
                        && let Some(frame) = browser.and_then(|browser| browser.main_frame())
                    {
                        frame.load_url(Some(&CefString::from(url.as_str())));
                    }
                }
            }
            1
        }

        fn on_render_process_terminated(&self, browser: Option<&mut Browser>, _status: TerminationStatus, _error_code: ::std::os::raw::c_int, _error_string: Option<&CefString>) {
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser)) else { return };
            {
                let mut now = page.now.borrow_mut();
                now.loading = false;
                now.error = Some(CRASHED.into());
            }
            page.tell();
        }
    }
}

// A server's sign-in request, carried from Chromium's IO thread to the UI thread: the page of the browser `browser` asks
// the person in Wings' sheet, or the request is cancelled when that page is gone.
wrap_task! {
    struct AskAuth {
        browser: i32,
        callback: AuthCallback,
        site: String,
        realm: String,
        proxy: bool,
    }

    impl Task {
        fn execute(&self) {
            let page = PAGES.with_borrow(|pages| {
                pages.iter().filter_map(Weak::upgrade).find(|page| page.browser.borrow().as_ref().map(Browser::identifier) == Some(self.browser))
            });
            match page {
                Some(page) => page.wait_for_auth(self.callback.clone(), self.site.clone(), &self.realm, self.proxy),
                None => self.callback.cancel(),
            }
        }
    }
}

wrap_find_handler! {
    struct PageFind {
        page: Weak<Shared>,
    }

    impl FindHandler {
        /// How many matches the search has and which is current, as Chromium counts them.
        fn on_find_result(&self, browser: Option<&mut Browser>, _identifier: ::std::os::raw::c_int, count: ::std::os::raw::c_int, _selection_rect: Option<&Rect>, active_match_ordinal: ::std::os::raw::c_int, _final_update: ::std::os::raw::c_int) {
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser)) else { return };
            if let Some((_, matches, current)) = page.find.borrow_mut().as_mut() {
                // A count of -1 is an update that has none yet.
                if count >= 0 {
                    *matches = Some(count as u32);
                }
                *current = active_match_ordinal.max(0) as u32;
            }
            page.tell();
        }
    }
}

wrap_keyboard_handler! {
    struct PageKeys {
        page: Weak<Shared>,
    }

    impl KeyboardHandler {
        /// Wings' own shortcuts pressed in the page go to Wings (`keys`), before Chrome sees them.
        fn on_pre_key_event(&self, _browser: Option<&mut Browser>, event: Option<&KeyEvent>, os_event: *mut u8, _is_keyboard_shortcut: Option<&mut ::std::os::raw::c_int>) -> ::std::os::raw::c_int {
            let (Some(page), Some(event)) = (self.page.upgrade(), event) else { return 0 };
            let down = event.type_ == KeyEventType::RAWKEYDOWN || event.type_ == KeyEventType::KEYDOWN;
            if !down {
                // The rest of a press that went to Wings.
                let forwarded = page.forwarded.get() == Some(event.native_key_code);
                if event.type_ == KeyEventType::KEYUP && forwarded {
                    page.forwarded.set(None);
                }
                return i32::from(forwarded);
            }
            // SAFETY: CEF hands the NSEvent it is handling, alive for the call.
            let Some(ns_event) = (unsafe { os_event.cast::<NSEvent>().as_ref() }) else { return 0 };
            let Some(press) = focus::press_of(ns_event) else { return 0 };
            if !keys::goes_to_wings_now(&press, page.told.page_id()) {
                return 0;
            }
            page.forwarded.set(Some(event.native_key_code));
            crate::web::key_from(page.told.page_id());
            if let Some(parent) = page.parent.borrow().clone() {
                focus::to_wings(ns_event, &parent);
            }
            1
        }
    }
}

wrap_jsdialog_handler! {
    struct PageDialogs {
        page: Weak<Shared>,
    }

    impl JsdialogHandler {
        fn on_jsdialog(&self, browser: Option<&mut Browser>, origin_url: Option<&CefString>, dialog_type: JsdialogType, message_text: Option<&CefString>, default_prompt_text: Option<&CefString>, callback: Option<&mut JsdialogCallback>, _suppress_message: Option<&mut ::std::os::raw::c_int>) -> ::std::os::raw::c_int {
            let (Some(page), Some(callback)) = (self.page.upgrade().filter(|page| page.browser_is(browser)), callback) else { return 0 };
            let default = default_prompt_text.map(CefString::to_string);
            page.wait_on(callback.clone(), dialog_type, text(message_text), default, &text(origin_url));
            1
        }

        /// Leaving a page that asks first leaves, as in WebKit, which shows no such dialog.
        fn on_before_unload_dialog(&self, _browser: Option<&mut Browser>, _message_text: Option<&CefString>, _is_reload: ::std::os::raw::c_int, callback: Option<&mut JsdialogCallback>) -> ::std::os::raw::c_int {
            if let Some(callback) = callback {
                callback.cont(1, None);
            }
            1
        }
    }
}

// The camera, microphone, location and notifications: WebKit in Wings has none of them (the app declares no use of the
// camera, microphone or location, and WebKit has no web notifications in an app), so Chromium refuses them too.
wrap_permission_handler! {
    pub(super) struct Refuse {}

    impl PermissionHandler {
        fn on_request_media_access_permission(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _requesting_origin: Option<&CefString>, _requested_permissions: u32, callback: Option<&mut MediaAccessCallback>) -> ::std::os::raw::c_int {
            if let Some(callback) = callback {
                callback.cancel();
            }
            1
        }

        fn on_show_permission_prompt(&self, _browser: Option<&mut Browser>, _prompt_id: u64, _requesting_origin: Option<&CefString>, _requested_permissions: u32, callback: Option<&mut PermissionPromptCallback>) -> ::std::os::raw::c_int {
            if let Some(callback) = callback {
                callback.cont(PermissionRequestResult::DENY);
            }
            1
        }
    }
}

// A page whose node hears its downloads (`onDownload`) gets them into ~/Downloads (`downloads`), each its document may
// start (`downloads::Gate`); any other downloads nothing. Returning 0 from `on_before_download` would hand it to Chrome's
// own downloads, so every way out returns 1: one never continued is canceled.
wrap_download_handler! {
    struct PageDownloads {
        page: Weak<Shared>,
    }

    impl DownloadHandler {
        /// CEF asks this only of one your click started (a link to an attachment, Alt-click): it counts as yours.
        fn can_download(&self, browser: Option<&mut Browser>, url: Option<&CefString>, _request_method: Option<&CefString>) -> ::std::os::raw::c_int {
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser) && page.told.downloads()) else { return 0 };
            let mut asked = page.asked.borrow_mut();
            asked.push(text(url));
            let extra = asked.len().saturating_sub(8);
            asked.drain(..extra);
            1
        }

        fn on_before_download(&self, browser: Option<&mut Browser>, download_item: Option<&mut DownloadItem>, suggested_name: Option<&CefString>, callback: Option<&mut BeforeDownloadCallback>) -> ::std::os::raw::c_int {
            let (Some(page), Some(item), Some(callback)) = (self.page.upgrade().filter(|page| page.browser_is(browser) && page.told.downloads()), download_item, callback) else { return 1 };
            let url = CefString::from(&item.url()).to_string();
            let user = {
                let mut asked = page.asked.borrow_mut();
                asked.iter().position(|one| *one == url).map(|at| asked.remove(at)).is_some()
            };
            if !page.gate.borrow_mut().allow(page.generation.get(), user) {
                downloads::blocked(&page.told, &url, &text(suggested_name));
                return 1;
            }
            let (weak, cef_id) = (Rc::downgrade(&page), item.id());
            let stop: Rc<dyn Fn()> = Rc::new(move || {
                if let Some(stop) = weak.upgrade().and_then(|page| page.stops.borrow_mut().remove(&cef_id)) {
                    stop.cancel();
                }
            });
            let Some((id, path)) = downloads::begin(&page.told, &page.profile, &url, &page.url_now(), &text(suggested_name), stop) else { return 1 };
            page.downloads.borrow_mut().insert(cef_id, id);
            callback.cont(Some(&CefString::from(path.to_string_lossy().as_ref())), 0);
            1
        }

        fn on_download_updated(&self, browser: Option<&mut Browser>, download_item: Option<&mut DownloadItem>, callback: Option<&mut DownloadItemCallback>) {
            let (Some(page), Some(item)) = (self.page.upgrade().filter(|page| page.browser_is(browser)), download_item) else { return };
            let cef_id = item.id();
            let Some(id) = page.downloads.borrow().get(&cef_id).copied() else { return };
            let ended = item.is_complete() != 0 || item.is_canceled() != 0 || item.is_interrupted() != 0;
            if ended {
                page.downloads.borrow_mut().remove(&cef_id);
                page.stops.borrow_mut().remove(&cef_id);
            } else if let Some(callback) = callback {
                page.stops.borrow_mut().insert(cef_id, callback.clone());
            }
            if item.is_complete() != 0 {
                let path = CefString::from(&item.full_path()).to_string();
                downloads::finished(id, (!path.is_empty()).then(|| PathBuf::from(path)));
            } else if item.is_canceled() != 0 {
                downloads::canceled(id);
            } else if item.is_interrupted() != 0 {
                downloads::failed(id, &interrupted(&format!("{:?}", item.interrupt_reason())));
            } else {
                let total = item.total_bytes();
                downloads::progress(id, item.received_bytes().max(0) as u64, (total > 0).then_some(total as u64));
            }
        }
    }
}

/// What a page says of a download Chromium stopped, from the reason's name (`CEF_DOWNLOAD_INTERRUPT_REASON_NETWORK_FAILED`).
fn interrupted(reason: &str) -> String {
    let name = reason.rsplit("CEF_DOWNLOAD_INTERRUPT_REASON_").next().unwrap_or_default().trim_end_matches(')');
    if name.is_empty() || name == reason { "The download stopped".into() } else { format!("The download stopped ({name})") }
}

// A window Chrome opened by itself, or an extension's popup, downloads nothing.
wrap_download_handler! {
    pub(super) struct NoDownloads {}

    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _request_method: Option<&CefString>) -> ::std::os::raw::c_int {
            0
        }

        /// One that started by itself, which `can_download` isn't asked of: 0 would hand it to Chrome's own downloads.
        fn on_before_download(&self, _browser: Option<&mut Browser>, _download_item: Option<&mut DownloadItem>, _suggested_name: Option<&CefString>, _callback: Option<&mut BeforeDownloadCallback>) -> ::std::os::raw::c_int {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_stopped_download_says_why_with_chromiums_name_for_it() {
        assert_eq!(super::interrupted("DownloadInterruptReason(CEF_DOWNLOAD_INTERRUPT_REASON_NETWORK_FAILED)"), "The download stopped (NETWORK_FAILED)");
        assert_eq!(super::interrupted("something else"), "The download stopped");
    }

    #[test]
    fn a_certificate_errors_name_is_read_from_its_debug_text() {
        assert_eq!(super::error_name("Errorcode(ERR_CERT_AUTHORITY_INVALID)"), "ERR_CERT_AUTHORITY_INVALID");
        assert_eq!(super::error_name("ERR_CERT_DATE_INVALID"), "ERR_CERT_DATE_INVALID");
        assert_eq!(super::error_name("something"), "");
    }

    #[test]
    fn a_failed_load_says_why_in_words_with_chromiums_name_for_it() {
        assert_eq!(super::load_error("net::ERR_NAME_NOT_RESOLVED"), "A server with that name couldn't be found (ERR_NAME_NOT_RESOLVED)");
        assert_eq!(super::load_error("net::ERR_CERT_DATE_INVALID"), "The site's certificate isn't valid (ERR_CERT_DATE_INVALID)");
        assert_eq!(super::load_error(""), "The page couldn't be opened");
    }

    use std::sync::atomic::{AtomicUsize, Ordering};

    use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send, rc::Retained, runtime::{AnyObject, NSObject}};
    use objc2_foundation::NSNotification;

    /// What a stand-in window heard: AppKit asked to toggle fullscreen, and its delegate told it stayed windowed.
    static TOGGLED: AtomicUsize = AtomicUsize::new(0);
    static WINDOWED: AtomicUsize = AtomicUsize::new(0);

    define_class!(
        // SAFETY: NSObject has no rules for subclasses, and this has no ivars.
        #[unsafe(super(NSObject))]
        struct FakeDelegate;

        impl FakeDelegate {
            #[unsafe(method(windowDidExitFullScreen:))]
            fn exited(&self, _notification: &NSNotification) {
                WINDOWED.fetch_add(1, Ordering::SeqCst);
            }
        }
    );

    define_class!(
        // SAFETY: NSObject has no rules for subclasses; its ivar is its delegate, kept alive with it.
        #[unsafe(super(NSObject))]
        #[ivars = Retained<FakeDelegate>]
        struct FakeWindow;

        impl FakeWindow {
            /// AppKit's own toggle, which a page's window ignored without a word.
            #[unsafe(method(toggleFullScreen:))]
            fn toggle(&self, _sender: *mut AnyObject) {
                TOGGLED.fetch_add(1, Ordering::SeqCst);
            }

            #[unsafe(method(delegate))]
            fn delegate(&self) -> *mut AnyObject {
                Retained::as_ptr(self.ivars()) as *mut AnyObject
            }
        }
    );

    fn fake_window() -> Retained<FakeWindow> {
        let delegate: Retained<FakeDelegate> = unsafe { msg_send![super(FakeDelegate::alloc().set_ivars(())), init] };
        unsafe { msg_send![super(FakeWindow::alloc().set_ivars(delegate)), init] }
    }

    /// Chrome's request of fullscreen for a page's window never reaches AppKit, which ignored it and so left Chrome in
    /// its transition for ever, putting off the window's close until Wings crashed quitting: Chrome hears at once that
    /// the window stays windowed. A window not kept windowed toggles as AppKit does.
    #[test]
    fn a_pages_window_never_goes_fullscreen_and_chrome_hears_it_stayed_windowed() {
        let (page, other) = (fake_window(), fake_window());
        let kept = wings_chromium::window::keep_windowed(&page);
        assert!(kept.ends_with("kept windowed"), "{kept}");
        let _: () = unsafe { msg_send![&*page, toggleFullScreen: std::ptr::null_mut::<AnyObject>()] };
        assert_eq!((TOGGLED.load(Ordering::SeqCst), WINDOWED.load(Ordering::SeqCst)), (0, 1), "AppKit isn't asked; Chrome hears");
        let _: () = unsafe { msg_send![&*other, toggleFullScreen: std::ptr::null_mut::<AnyObject>()] };
        assert_eq!((TOGGLED.load(Ordering::SeqCst), WINDOWED.load(Ordering::SeqCst)), (1, 1), "another window toggles as before");
    }
}
