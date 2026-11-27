//! One Chromium web view: a frameless Chrome-style window over the page's place in the Wings window (a child window
//! of it, `wings_chromium::window`), its browser, and what it tells its page. It is made, placed and hidden by the same
//! code as a WebKit view (`views::place`), and every command goes through the same gate (`web::gate`); a plugin's
//! scripts, pictures and logs then run over the page's own DevTools channel (`agent`).

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::{Rc, Weak},
};

use cef::*;
use gpui::{Bounds, Pixels, Window};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSAlert, NSEvent, NSModalResponse, NSTextField, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use wings_chromium::window as frameless;

use super::{agent, devtools::Incoming, focus, keys, runtime};
use crate::{
    page::tree::WebGo,
    web::{
        Profile,
        views::{Dialog, Heard, Maker, Native, State, Tell},
    },
};

/// What a page that crashed says until it reloads.
pub const CRASHED: &str = "This page crashed. Reload to open it again.";

pub struct Chromium;

impl Maker for Chromium {
    fn make(&self, profile: &Profile, keep_data: bool, scripts: bool, url: Option<&str>, told: Tell) -> Box<dyn Native> {
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
            browser: RefCell::new(None),
            window: RefCell::new(None),
            ns: RefCell::new(None),
            parent: RefCell::new(None),
            want: Cell::new(None),
            attached: Cell::new(false),
            closed: Cell::new(false),
            pending: RefCell::default(),
            dialog: RefCell::new(None),
            sheet: RefCell::new(None),
            dialogs: Cell::new((0, 0, false)),
            forwarded: Cell::new(None),
        });
        // Its loading state shows while CEF starts.
        page.tell();
        runtime::opened();
        PAGES.with_borrow_mut(|pages| pages.push(Rc::downgrade(&page)));
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
}

/// Closes every page's window, as Wings quits.
pub fn close_all() {
    let pages: Vec<Rc<Shared>> = PAGES.with_borrow_mut(|pages| pages.drain(..).filter_map(|page| page.upgrade()).collect());
    for page in pages {
        page.close();
    }
}

/// Removes every cookie and the cache Chromium keeps for `profile`: in the contexts open now while CEF runs, else its
/// folder on disk, which holds the rest of its site data too.
pub fn clear_data(profile: &Profile) {
    if !runtime::running() {
        if let Some(root) = crate::web::engine::profiles() {
            let _ = std::fs::remove_dir_all(root.join(runtime::dir_name(profile)));
        }
        return;
    }
    // debt: while CEF runs, a profile's local storage and IndexedDB stay until it quits; they'd need the DevTools
    // protocol's `Storage.clearDataForOrigin` for each origin it holds.
    for keep_data in [true, false] {
        let context = runtime::context(profile, keep_data).or_else(request_context_get_global_context);
        if let Some(context) = context {
            if let Some(cookies) = context.cookie_manager(None) {
                cookies.delete_cookies(None, None, None);
            }
            context.clear_http_cache(None);
        }
    }
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
    pub(super) browser: RefCell<Option<Browser>>,
    window: RefCell<Option<cef::Window>>,
    pub(super) ns: RefCell<Option<Retained<NSWindow>>>,
    /// The Wings window it shows over.
    pub(super) parent: RefCell<Option<Retained<NSWindow>>>,
    /// Where it shows, in the Wings window's points from its top left, or hidden.
    want: Cell<Option<Bounds<Pixels>>>,
    attached: Cell<bool>,
    closed: Cell<bool>,
    /// Commands sent before it was ready.
    pending: RefCell<Vec<(WebGo, u64)>>,
    /// The page's `alert`, `confirm` or `prompt` waiting for an answer, what it shows, and Wings' sheet showing it.
    dialog: RefCell<Option<(JsdialogCallback, JsdialogType, Dialog)>>,
    sheet: RefCell<Option<(Retained<NSAlert>, Option<Retained<NSTextField>>)>>,
    /// The page's dialogs now: its generation, how many it showed, and whether the person stopped them.
    dialogs: Cell<(u64, u32, bool)>,
    /// The key code of a press handed to Wings, whose key-up and characters the page doesn't get either.
    forwarded: Cell<Option<i32>>,
}

struct Page(Rc<Shared>);

impl Native for Page {
    fn place(&mut self, window: &Window, frame: Option<Bounds<Pixels>>) {
        if let Some(parent) = ns_window_of(window) {
            self.0.parent.replace(Some(parent));
        }
        self.0.want.set(frame);
        self.0.apply_place();
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
    /// Opens its window, once CEF is up.
    fn open(self: &Rc<Self>) {
        // A plugin that may run scripts in its pages gets the address once its channel is readied (`ready`).
        let url = if self.scripts { "about:blank".to_owned() } else { self.start.borrow().clone() };
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
        }
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
        if !self.scripts {
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
        if self.scripts {
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
        fn jsdialog_handler(&self) -> Option<JsdialogHandler> {
            Some(PageDialogs::new(self.page.clone()))
        }
        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(Refuse::new())
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(NoDownloads::new())
        }
    }
}

wrap_life_span_handler! {
    struct PageLifeSpan {
        page: Weak<Shared>,
    }

    impl LifeSpanHandler {
        /// A link that opens a new window (`target="_blank"`, `window.open`) loads in this page, as in WebKit: a page has
        /// one view, and no Chrome window opens.
        fn on_before_popup(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
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
            let url = text(target_url);
            if crate::web::may_navigate(&url, true)
                && let Some(frame) = browser.and_then(|browser| browser.main_frame())
            {
                frame.load_url(Some(&CefString::from(url.as_str())));
            }
            1
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser else { return };
            runtime::created(browser);
            if let Some(page) = self.page.upgrade() {
                page.browser_created(browser);
            }
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                runtime::closed(browser);
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
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser) && page.ready.get()) else { return };
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
                page.tell();
            }
        }

        fn on_load_error(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, error_code: Errorcode, error_text: Option<&CefString>, _failed_url: Option<&CefString>) {
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser) && page.ready.get()) else { return };
            // A load stopped by another (a click before the page finished) isn't a failure the person needs to see.
            if frame.is_none_or(|frame| frame.is_main() == 0) || error_code == Errorcode::ABORTED {
                return;
            }
            page.now.borrow_mut().error = Some(load_error(&text(error_text)));
            page.tell();
        }
    }
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
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser) && page.ready.get()) else { return };
            if frame.is_some_and(|frame| frame.is_main() != 0) {
                page.now.borrow_mut().url = text(url);
                page.tell();
                super::extensions::page_moved(&page);
            }
        }

        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser) && page.ready.get()) else { return };
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
            if let Some(page) = self.page.upgrade().filter(|page| page.browser_is(browser))
                && allowed
                && main_frame
                && crate::web::carries_imported(&page.web_dir, &page.committed.borrow(), &url)
            {
                page.chain_tainted.set(true);
            }
            i32::from(!allowed)
        }

        /// A site asking for a client certificate gets none, with no Chrome dialog to pick one, as WebKit in Wings sends
        /// none (it has no handler for the challenge).
        fn on_select_client_certificate(&self, _browser: Option<&mut Browser>, _is_proxy: ::std::os::raw::c_int, _host: Option<&CefString>, _port: ::std::os::raw::c_int, _certificates: Option<&[Option<X509Certificate>]>, callback: Option<&mut SelectClientCertificateCallback>) -> ::std::os::raw::c_int {
            if let Some(callback) = callback {
                callback.select(None);
            }
            1
        }

        /// A link opened in a new tab (Cmd-click, a middle click) loads in this page, as in WebKit.
        fn on_open_urlfrom_tab(&self, browser: Option<&mut Browser>, _frame: Option<&mut Frame>, target_url: Option<&CefString>, _target_disposition: WindowOpenDisposition, _user_gesture: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            let url = text(target_url);
            if crate::web::may_navigate(&url, true)
                && let Some(frame) = browser.and_then(|browser| browser.main_frame())
            {
                frame.load_url(Some(&CefString::from(url.as_str())));
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
            if !keys::goes_to_wings_now(&press) {
                return 0;
            }
            page.forwarded.set(Some(event.native_key_code));
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

// WebKit in Wings downloads nothing (its navigation delegate takes no download), and neither does Chromium.
wrap_download_handler! {
    pub(super) struct NoDownloads {}

    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _request_method: Option<&CefString>) -> ::std::os::raw::c_int {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_failed_load_says_why_in_words_with_chromiums_name_for_it() {
        assert_eq!(super::load_error("net::ERR_NAME_NOT_RESOLVED"), "A server with that name couldn't be found (ERR_NAME_NOT_RESOLVED)");
        assert_eq!(super::load_error("net::ERR_CERT_DATE_INVALID"), "The site's certificate isn't valid (ERR_CERT_DATE_INVALID)");
        assert_eq!(super::load_error(""), "The page couldn't be opened");
    }
}
