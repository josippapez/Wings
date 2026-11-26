//! The extensions a profile added (`web::extensions`, the one list, each checked file by file before it loads) under
//! Chromium: loaded into CEF's global context over the browser target's DevTools pipe (`Extensions.loadUnpacked`),
//! uninstalled the same way when turned off or removed (Chromium deletes what it stored then), shown as buttons in the
//! page's toolbar, and a button's popup opened in a small frameless window under it.
//!
//! Chromium forgets an extension loaded this way at its next start (`ExtensionPrefs::CleanUpCdpInstalledExtensions`),
//! so each run loads only what passes Wings' check that run.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

use cef::*;
use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Value, json};
use wings_chromium::window as frameless;

use super::{page::Shared, runtime};
use crate::web::{Profile, extensions, views};

/// Why a click on an extension's button did nothing.
pub const NOT_ON: &str = "That extension isn't on in this browser";
const NO_POPUP: &str = "That extension has no popup";
/// A popup's size: Chrome's largest, which Bitwarden's fills.
const POPUP: NSSize = NSSize { width: 380.0, height: 600.0 };
/// How long after its popup lost the keyboard a click on the same button closes it rather than opening it again: the
/// press on the button is what took the keyboard away.
const TOGGLE_WITHIN: Duration = Duration::from_millis(800);

/// An extension loaded into Chromium.
#[derive(Clone, Debug)]
struct Loaded {
    /// Wings' id for it (the list's), which the page's buttons and commands use.
    id: String,
    /// Chromium's id for it.
    chrome: String,
    name: String,
    /// Its popup page, from its manifest's `action` (or `browser_action`), if it has one.
    popup: Option<String>,
    /// Whether its manifest gives it a button at all.
    button: bool,
    icon: Option<String>,
}

type Answered = Box<dyn FnOnce(Result<Value, String>)>;

thread_local! {
    static LOADED: RefCell<Vec<Loaded>> = RefCell::default();
    static ASKED: RefCell<HashMap<u64, Answered>> = RefCell::default();
    static NEXT: Cell<u64> = const { Cell::new(1) };
    static POPUP_OPEN: RefCell<Option<Popup>> = const { RefCell::new(None) };
    /// The extension whose popup lost the keyboard last, and when.
    static CLOSED: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

/// Asks the browser target `method` with `params`, and `done` hears its answer.
fn ask(method: &str, params: Value, done: impl FnOnce(Result<Value, String>) + 'static) {
    let id = NEXT.replace(NEXT.get() + 1);
    let message = json!({ "id": id, "method": method, "params": params }).to_string();
    ASKED.with_borrow_mut(|asked| asked.insert(id, Box::new(done)));
    if !runtime::send(message.as_bytes()) {
        if let Some(done) = ASKED.with_borrow_mut(|asked| asked.remove(&id)) {
            done(Err("Chromium can't load extensions in this run of Wings".into()));
        }
    }
}

/// A message from the browser target: the answer to a request of `ask`'s, or an event, which nothing here follows.
pub fn heard(message: &[u8]) {
    let Ok(message) = serde_json::from_slice::<Value>(message) else { return };
    let Some(id) = message["id"].as_u64() else { return };
    let Some(done) = ASKED.with_borrow_mut(|asked| asked.remove(&id)) else { return };
    done(match message.get("error") {
        Some(error) => Err(error["message"].as_str().unwrap_or("Chromium refused").to_owned()),
        None => Ok(message["result"].clone()),
    });
}

/// CEF's global context is up: its profile's extensions load, each checked first.
pub fn started() {
    load_checked();
}

/// Loads the global profile's extensions that are on and pass `extensions::verify`, checked off the main thread (hashing
/// Bitwarden's 92 MB takes a second in a debug build, and the Keychain may wait for the person).
fn load_checked() {
    let Some(profile) = runtime::global() else { return };
    std::thread::spawn(move || {
        extensions::sweep_staging(&profile);
        let verified = extensions::loadable(&profile, &|| extensions::key(&profile));
        dispatch2::DispatchQueue::main().exec_async(move || {
            for extension in verified {
                load(&profile, &extension, |result| {
                    if let Err(why) = result {
                        eprintln!("[chromium] an extension didn't load: {why}");
                    }
                });
            }
        });
    });
}

/// Loads `extension`, which passed `extensions::verify`, into Chromium when it runs for `profile`: `done` hears whether it
/// did. Nothing when Chromium isn't up for this profile: it loads its extensions as it starts.
pub fn load(profile: &Profile, extension: &extensions::Verified, done: impl FnOnce(Result<(), String>) + 'static) {
    if runtime::global().as_ref() != Some(profile) {
        return done(Ok(()));
    }
    if LOADED.with_borrow(|loaded| loaded.iter().any(|loaded| loaded.id == extension.id)) {
        return done(Ok(()));
    }
    let added = extensions::list(profile).into_iter().find(|added| added.id == extension.id);
    let manifest = manifest_of(&extension.dir);
    let icon = added.as_ref().and_then(|added| extensions::icon_path(profile, added)).and_then(|path| icon(&profile.plugin, &path));
    let (id, name, plugin) = (extension.id.clone(), added.map(|added| added.name).unwrap_or_default(), profile.plugin.clone());
    let dir = match chromium_copy(extension) {
        Ok(dir) => dir.to_string_lossy().into_owned(),
        Err(why) => return done(Err(why)),
    };
    ask("Extensions.loadUnpacked", json!({ "path": dir }), move |answer| match answer {
        Ok(result) => {
            let Some(chrome) = result["id"].as_str().map(str::to_owned) else { return done(Err("Chromium didn't say which extension it loaded".into())) };
            let (button, popup) = action_of(&manifest);
            LOADED.with_borrow_mut(|loaded| loaded.push(Loaded { id, chrome, name, popup, button, icon }));
            refresh(&plugin);
            done(Ok(()))
        }
        Err(why) => done(Err(why)),
    });
}

/// Uninstalls the extension `id` from Chromium, which deletes what it stored there: turned off or removed.
pub fn unload(profile: &Profile, id: &str, done: impl FnOnce() + 'static) {
    let Some(loaded) = LOADED.with_borrow_mut(|loaded| {
        let at = loaded.iter().position(|loaded| loaded.id == id)?;
        Some(loaded.remove(at))
    }) else {
        return done();
    };
    if POPUP_OPEN.with_borrow(|popup| popup.as_ref().is_some_and(|popup| popup.id == id)) {
        close_popup();
    }
    let plugin = profile.plugin.clone();
    if let Some(copy) = copy_of(id) {
        let _ = std::fs::remove_dir_all(copy);
    }
    ask("Extensions.uninstall", json!({ "id": loaded.chrome }), move |answer| {
        if let Err(why) = answer {
            eprintln!("[chromium] couldn't uninstall an extension: {why}");
        }
        refresh(&plugin);
        done()
    });
}

/// The buttons of `profile`'s extensions for its pages, by name (`views::shown`).
pub fn buttons(profile: &Profile, imported: bool) -> Vec<views::Extension> {
    if runtime::global().as_ref() != Some(profile) {
        return Vec::new();
    }
    let found = LOADED.with_borrow(|loaded| {
        loaded
            .iter()
            .filter(|loaded| loaded.button)
            .map(|loaded| views::Extension {
                id: loaded.id.clone(),
                name: loaded.name.clone(),
                label: loaded.name.clone(),
                icon: loaded.icon.clone(),
                // debt: Chromium's badge text (`action.setBadgeText`) has no event to follow; reading it needs the
                // extension's worker over the DevTools pipe on every change, so the buttons show none yet.
                badge: String::new(),
                enabled: true,
            })
            .collect()
    });
    crate::web::shown(found, imported)
}

/// Tells every page of the plugin `plugin` its buttons again.
fn refresh(plugin: &str) {
    if let Some(profile) = runtime::global().filter(|profile| profile.plugin == plugin) {
        for page in super::page::pages_of(&profile) {
            page.tell();
        }
    }
}

/// Where Chromium's copy of the extension `id` goes.
fn copy_of(id: &str) -> Option<std::path::PathBuf> {
    let safe = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric());
    safe.then(|| Some(runtime::profiles_root()?.join("extensions").join(id))).flatten()
}

/// Chromium's copy of `extension`'s checked folder, made right after the check, as WebKit reads that folder in place
/// then: a clone (APFS copies no data) without Wings' WebKit shim, whose `__wings_` files Chromium refuses ("Filenames
/// starting with "_" are reserved"), without the Web Store's `_metadata`, and without `nativeMessaging`, which Wings'
/// consent dialog says it never allows.
fn chromium_copy(extension: &extensions::Verified) -> Result<std::path::PathBuf, String> {
    let copy = copy_of(&extension.id).ok_or("No folder for Chromium's copy")?;
    let _ = std::fs::remove_dir_all(&copy);
    std::fs::create_dir_all(copy.parent().unwrap_or(&copy)).map_err(|error| error.to_string())?;
    let cloned = std::process::Command::new("/bin/cp").arg("-cR").arg(&extension.dir).arg(&copy).status();
    if !cloned.is_ok_and(|status| status.success()) {
        let _ = std::fs::remove_dir_all(&copy);
        return Err("Couldn't copy the extension for Chromium".into());
    }
    let readied = wings_web_extensions::unshim_service_worker(&copy).map_err(|error| error.to_string()).and_then(|_| {
        let _ = std::fs::remove_dir_all(copy.join("_metadata"));
        without_native_messaging(&copy.join("manifest.json"))?;
        match action_of(&manifest_of(&copy)).1 {
            Some(popup) => with_popup_script(&copy, &popup),
            None => Ok(()),
        }
    });
    if let Err(why) = readied {
        let _ = std::fs::remove_dir_all(&copy);
        return Err(why);
    }
    Ok(copy)
}

/// Takes `nativeMessaging` out of the manifest at `file`, from what it asks for now and what it may ask for later.
fn without_native_messaging(file: &std::path::Path) -> Result<(), String> {
    let mut manifest: Value = std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).ok_or("The extension's manifest can't be read")?;
    let mut changed = false;
    for key in ["permissions", "optional_permissions"] {
        if let Some(list) = manifest.get_mut(key).and_then(Value::as_array_mut) {
            let before = list.len();
            list.retain(|permission| permission != "nativeMessaging");
            changed |= list.len() != before;
        }
    }
    if changed {
        std::fs::write(file, serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// What the manifest at `dir` says.
fn manifest_of(dir: &std::path::Path) -> Value {
    std::fs::read(dir.join("manifest.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Whether a manifest gives its extension a button, and the button's popup page.
fn action_of(manifest: &Value) -> (bool, Option<String>) {
    let action = ["action", "browser_action", "page_action"].iter().find_map(|key| manifest.get(*key).filter(|action| action.is_object()));
    let popup = action.and_then(|action| action["default_popup"].as_str()).filter(|popup| !popup.is_empty()).map(|popup| popup.trim_start_matches('/').to_owned());
    (action.is_some(), popup)
}

/// The address of `popup` in the extension `chrome`, when the path stays inside it, for the page at `page_url`
/// (`POPUP_SCRIPT` reads it).
fn popup_url(chrome: &str, popup: &str, page_url: &str) -> Option<String> {
    let path = std::path::Path::new(popup);
    let page: String = page_url
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect();
    path.components()
        .all(|part| matches!(part, std::path::Component::Normal(_)))
        .then(|| format!("chrome-extension://{chrome}/{popup}?wings-page={page}"))
}

/// Registers the icon at `path` among `plugin`'s images, under an id made of its bytes.
fn icon(plugin: &str, path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    let id = format!("ext-btn-{}", Sha256::digest(&bytes).iter().take(12).map(|byte| format!("{byte:02x}")).collect::<String>());
    crate::page::images::register_bytes(plugin, &id, bytes).ok()?;
    Some(id)
}

/// An extension's popup open now, over the page whose button opened it.
struct Popup {
    id: String,
    page: Weak<Shared>,
    window: Option<cef::Window>,
    ns: Option<Retained<NSWindow>>,
    parent: Retained<NSWindow>,
}

/// Whether a click of the button of `id` comes right after its own popup lost the keyboard (`closed`), as a press on
/// the button while the popup shows does: the click closes it, as in Chrome, rather than opening it again.
fn toggles_off(closed: Option<&(String, Instant)>, id: &str, now: Instant) -> bool {
    closed.is_some_and(|(closed, at)| closed == id && now.saturating_duration_since(*at) < TOGGLE_WITHIN)
}

/// A click of your own on the button of the extension `id` in `page`'s toolbar: its popup opens under the button, or
/// closes when it shows.
pub fn click(page: &Shared, id: &str) -> Result<(), String> {
    let open_here = POPUP_OPEN.with_borrow(|popup| popup.as_ref().map(|popup| popup.id == id && std::ptr::eq(popup.page.as_ptr(), page)));
    if let Some(same) = open_here {
        close_popup();
        if same {
            return Ok(());
        }
    }
    if toggles_off(CLOSED.with_borrow(|closed| closed.clone()).as_ref(), id, Instant::now()) {
        CLOSED.with_borrow_mut(|closed| *closed = None);
        return Ok(());
    }
    let loaded = LOADED.with_borrow(|loaded| loaded.iter().find(|loaded| loaded.id == id).cloned()).ok_or(NOT_ON)?;
    let url = loaded.popup.as_deref().and_then(|popup| popup_url(&loaded.chrome, popup, &page.url_now())).ok_or(NO_POPUP)?;
    let (Some(parent), Some(page_ns)) = (page.parent.borrow().clone(), page.ns.borrow().clone()) else { return Err("The page isn't on screen".into()) };
    let frame = anchor(page_ns.frame(), NSEvent::mouseLocation());
    POPUP_OPEN.with_borrow_mut(|popup| {
        *popup = Some(Popup { id: id.to_owned(), page: super::page::weak_of(page), window: None, ns: None, parent: parent.clone() })
    });
    runtime::opened();
    let id = id.to_owned();
    frameless::open(
        &url,
        cef_rect(frame),
        Some(PopupClient::new()),
        None,
        Rc::new(move |window, _view| popup_created(window, frame)),
        Rc::new(move |active| {
            // A click anywhere else takes the keyboard from it: it closes, as in Chrome.
            if !active && POPUP_OPEN.with_borrow(|popup| popup.as_ref().is_some_and(|popup| popup.id == id && popup.window.is_some())) {
                CLOSED.with_borrow_mut(|closed| *closed = Some((id.clone(), Instant::now())));
                close_popup();
            }
        }),
    );
    Ok(())
}

/// Where a popup goes: its top just under the page's top edge (the toolbar's foot), centred under the pointer that
/// pressed the button and kept inside the page, in screen points.
fn anchor(page: NSRect, pointer: NSPoint) -> NSRect {
    let top = page.origin.y + page.size.height - 4.0;
    let low = page.origin.x + 8.0;
    let high = (page.origin.x + page.size.width - POPUP.width - 8.0).max(low);
    let x = (pointer.x - POPUP.width / 2.0).clamp(low, high);
    let height = POPUP.height.min(page.size.height - 12.0).max(120.0);
    NSRect::new(NSPoint::new(x, top - height), NSSize::new(POPUP.width, height))
}

fn popup_created(window: &mut cef::Window, frame: NSRect) {
    let Some(ns) = frameless::ns_window(window) else { return };
    let parent = POPUP_OPEN.with_borrow(|popup| popup.as_ref().map(|popup| popup.parent.clone()));
    let Some(parent) = parent else {
        // Closed before it opened.
        window.close();
        return;
    };
    frameless::never_main(&ns);
    window.show();
    frameless::as_popup(&ns, 10.0);
    frameless::attach(&ns, &parent, frame);
    // What you type goes into it at once, as its field asks for the keyboard as it opens.
    ns.makeKeyWindow();
    if let Some(view) = window.child_view_at(0) {
        view.request_focus();
    }
    super::focus::resync(&parent);
    POPUP_OPEN.with_borrow_mut(|popup| {
        if let Some(popup) = popup.as_mut() {
            popup.window = Some(window.clone());
            popup.ns = Some(ns);
        }
    });
}

/// The first extension loaded, for a debug build's checks.
#[cfg(debug_assertions)]
pub fn first_id() -> Option<String> {
    LOADED.with_borrow(|loaded| loaded.iter().find(|loaded| loaded.popup.is_some()).map(|loaded| loaded.id.clone()))
}

/// Whether a popup shows now.
#[cfg(debug_assertions)]
pub fn popup_open() -> bool {
    POPUP_OPEN.with_borrow(|popup| popup.as_ref().is_some_and(|popup| popup.window.is_some()))
}

/// Closes the popup if one shows, and gives the keyboard back to the Wings window under it.
pub fn close_popup() {
    let Some(popup) = POPUP_OPEN.with_borrow_mut(Option::take) else { return };
    if let Some(ns) = &popup.ns {
        frameless::detach(ns, &popup.parent);
    }
    if let Some(window) = popup.window {
        window.close();
    }
    let app_active = objc2::MainThreadMarker::new().is_some_and(|mtm| objc2_app_kit::NSApplication::sharedApplication(mtm).isActive());
    if app_active && popup.parent.isVisible() {
        popup.parent.makeKeyWindow();
    }
    super::focus::resync(&popup.parent);
    runtime::gone();
}

/// The page `page` closes: so does its popup.
pub fn page_closed(page: &Shared) {
    if POPUP_OPEN.with_borrow(|popup| popup.as_ref().is_some_and(|popup| std::ptr::eq(popup.page.as_ptr(), page))) {
        close_popup();
    }
}

/// The page went to another address: its popup, made for the last one, closes.
pub fn page_moved(page: &Shared) {
    page_closed(page);
}

/// What runs first in an extension's popup under Chromium, from Chromium's copy of the extension (`with_popup_script`).
/// Chromium shows the popup in a window of its own, so `tabs.query({ currentWindow: true })` and a query of
/// `WINDOW_ID_CURRENT` found the popup itself, and Bitwarden filled nothing in the page: such a query asks for the
/// window of the tab showing the page the button was clicked on, whose address the popup's own address carries.
pub const POPUP_SCRIPT: &str = r#"(() => {
  const named = (location.search.match(/[?&]wings-page=([^&]*)/) || [])[1];
  const tabs = globalThis.chrome && chrome.tabs;
  if (!named || !tabs || typeof tabs.query !== "function") return;
  const pageUrl = decodeURIComponent(named);
  const query = tabs.query.bind(tabs);
  const current = chrome.windows && chrome.windows.WINDOW_ID_CURRENT !== undefined ? chrome.windows.WINDOW_ID_CURRENT : -2;
  let found;
  const pageWindow = () => found || (found = query({}).then((all) => {
    const tab = all.find((tab) => tab.url === pageUrl);
    return tab ? tab.windowId : null;
  }));
  tabs.query = function (info, callback) {
    if (!info || !(info.currentWindow || info.lastFocusedWindow || info.windowId === current)) return query(info, callback);
    const asked = pageWindow().then((id) => {
      if (id === null) return query(info);
      const rest = Object.assign({}, info);
      delete rest.currentWindow;
      delete rest.lastFocusedWindow;
      rest.windowId = id;
      return query(rest);
    });
    if (typeof callback === "function") { asked.then(callback); return undefined; }
    return asked;
  };
})();
"#;

/// The file Chromium's copy of an extension holds `POPUP_SCRIPT` in: no leading `_`, which Chromium refuses.
const POPUP_SCRIPT_FILE: &str = "wings-popup-tabs.js";

/// Puts `POPUP_SCRIPT` first in the popup page `popup` of the extension copy in `dir`.
fn with_popup_script(dir: &std::path::Path, popup: &str) -> Result<(), String> {
    // The manifest is the extension's to write: a `default_popup` of "../.." or a link would have Wings rewrite a file
    // outside the copy.
    const OUTSIDE: &str = "The extension's popup isn't inside the extension";
    if !std::path::Path::new(popup).components().all(|part| matches!(part, std::path::Component::Normal(_))) {
        return Err(OUTSIDE.into());
    }
    let page = dir.join(popup);
    if std::fs::symlink_metadata(&page).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(OUTSIDE.into());
    }
    let html = std::fs::read_to_string(&page).map_err(|error| format!("The extension's popup can't be read: {error}"))?;
    std::fs::write(dir.join(POPUP_SCRIPT_FILE), POPUP_SCRIPT).map_err(|error| error.to_string())?;
    std::fs::write(&page, first_script(&html, &format!("/{POPUP_SCRIPT_FILE}"))).map_err(|error| error.to_string())
}

/// `html` with a script from `src` before anything else it runs: right after its `<head>`, or first.
fn first_script(html: &str, src: &str) -> String {
    let tag = format!("<script src=\"{src}\"></script>");
    let head = html.to_ascii_lowercase().find("<head").and_then(|at| html[at..].find('>').map(|end| at + end + 1));
    match head {
        Some(at) => format!("{}{tag}{}", &html[..at], &html[at..]),
        None => format!("{tag}{html}"),
    }
}

fn cef_rect(frame: NSRect) -> Rect {
    let screen_height = objc2::MainThreadMarker::new()
        .and_then(|mtm| objc2_app_kit::NSScreen::screens(mtm).firstObject())
        .map_or(0.0, |screen| screen.frame().size.height);
    Rect {
        x: frame.origin.x as i32,
        y: (screen_height - frame.origin.y - frame.size.height) as i32,
        width: frame.size.width as i32,
        height: frame.size.height as i32,
    }
}

wrap_client! {
    struct PopupClient {}

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(PopupLifeSpan::new())
        }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(PopupKeys::new())
        }
    }
}

wrap_life_span_handler! {
    struct PopupLifeSpan {}

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                runtime::created(browser);
            }
        }

        /// A link the popup opens goes nowhere: no Chrome window opens.
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
                runtime::closed(browser);
            }
        }
    }
}

wrap_keyboard_handler! {
    struct PopupKeys {}

    impl KeyboardHandler {
        /// Escape closes it, as in Chrome.
        fn on_pre_key_event(&self, _browser: Option<&mut Browser>, event: Option<&KeyEvent>, _os_event: *mut u8, _is_keyboard_shortcut: Option<&mut ::std::os::raw::c_int>) -> ::std::os::raw::c_int {
            let Some(event) = event else { return 0 };
            if event.type_ == KeyEventType::RAWKEYDOWN && event.native_key_code == 53 {
                super::next_turn(close_popup);
                return 1;
            }
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use serde_json::json;

    use super::*;

    /// A press on the button while its popup shows takes the keyboard from the popup, which closes it: the click that
    /// follows must not open it again. Another extension's button, or a click long after, opens its popup.
    #[test]
    fn a_click_right_after_its_popup_lost_the_keyboard_closes_it_and_nothing_else_does() {
        let now = Instant::now();
        let closed = ("bitwarden".to_owned(), now - Duration::from_millis(150));
        assert!(toggles_off(Some(&closed), "bitwarden", now));
        assert!(!toggles_off(Some(&closed), "ublock", now), "another extension's button");
        assert!(!toggles_off(Some(&closed), "bitwarden", now + Duration::from_secs(2)), "long after");
        assert!(!toggles_off(None, "bitwarden", now));
    }

    /// Wings' consent dialog says it never lets an extension talk to an app on this Mac: Chromium's copy doesn't ask.
    #[test]
    fn chromiums_copy_of_an_extension_asks_for_no_native_messaging() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("manifest.json");
        std::fs::write(&file, r#"{"name":"x","permissions":["storage","nativeMessaging"],"optional_permissions":["nativeMessaging","tabs"]}"#).unwrap();
        without_native_messaging(&file).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(manifest["permissions"], json!(["storage"]));
        assert_eq!(manifest["optional_permissions"], json!(["tabs"]));
    }

    #[test]
    fn a_button_and_its_popup_come_from_the_manifest_and_a_popup_path_never_leaves_the_extension() {
        assert_eq!(action_of(&json!({ "action": { "default_popup": "popup/index.html" } })), (true, Some("popup/index.html".into())));
        assert_eq!(action_of(&json!({ "browser_action": { "default_popup": "/p.html" } })), (true, Some("p.html".into())));
        assert_eq!(action_of(&json!({ "action": {} })), (true, None));
        assert_eq!(action_of(&json!({ "name": "no button" })), (false, None));
        assert_eq!(
            popup_url("abc", "popup/index.html", "https://a.com/x?y=1").as_deref(),
            Some("chrome-extension://abc/popup/index.html?wings-page=https%3A%2F%2Fa.com%2Fx%3Fy%3D1")
        );
        assert_eq!(popup_url("abc", "../../etc/x", "https://a.com/"), None);
    }

    /// A manifest naming a popup outside the extension, by "..", an absolute path or a link, leaves that file as it was.
    #[test]
    fn a_popup_outside_the_extension_is_refused_and_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let copy = root.path().join("copy");
        std::fs::create_dir_all(copy.join("popup")).unwrap();
        let outside = root.path().join("outside.html");
        std::fs::write(&outside, "<head></head>").unwrap();
        std::os::unix::fs::symlink(&outside, copy.join("popup/linked.html")).unwrap();
        for popup in ["../outside.html", outside.to_str().unwrap(), "popup/linked.html"] {
            assert!(with_popup_script(&copy, popup).is_err(), "{popup}");
        }
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "<head></head>");
        std::fs::write(copy.join("popup/index.html"), "<head></head>").unwrap();
        with_popup_script(&copy, "popup/index.html").unwrap();
        assert!(std::fs::read_to_string(copy.join("popup/index.html")).unwrap().contains(POPUP_SCRIPT_FILE));
    }

    /// The popup's own script goes first, before the extension's, wherever its page's head is.
    #[test]
    fn the_popups_first_script_comes_before_the_extensions_own() {
        let html = r#"<!doctype html><html><HEAD lang="en"><script src="main.js"></script></HEAD></html>"#;
        assert_eq!(first_script(html, "/w.js"), r#"<!doctype html><html><HEAD lang="en"><script src="/w.js"></script><script src="main.js"></script></HEAD></html>"#);
        assert_eq!(first_script("<body>x</body>", "/w.js"), r#"<script src="/w.js"></script><body>x</body>"#);
    }

    /// In the popup, a query of the current window's tabs (or of `WINDOW_ID_CURRENT`) asks for the page's window, found by
    /// the page's address; any other query is the extension's own. Run in JavaScriptCore with stand-ins for `chrome`.
    #[test]
    fn a_popups_query_of_its_current_window_finds_the_pages_tab() {
        let fake = r#"
            const asked = [];
            const location = { search: "?wings-page=http%3A%2F%2F127.0.0.1%3A8765%2Flogin" };
            const all = [{ url: "http://127.0.0.1:8765/login", windowId: 7, active: true }, { url: "chrome-extension://x/popup/index.html", windowId: 9, active: true }];
            globalThis.chrome = { windows: { WINDOW_ID_CURRENT: -2 }, tabs: { query(info) { asked.push(JSON.stringify(info)); return Promise.resolve(info.windowId === undefined || info.windowId === 7 ? all.filter((tab) => info.windowId === undefined || tab.windowId === info.windowId) : []); } } };
        "#;
        let run = r#"
            const out = [];
            chrome.tabs.query({ active: true, currentWindow: true }).then((tabs) => out.push(tabs.map((tab) => tab.url).join()));
            chrome.tabs.query({ active: true, windowId: -2 }).then((tabs) => out.push(tabs.length));
            chrome.tabs.query({ url: "https://*/*" });
            function run() { return JSON.stringify({ out, asked }); }
        "#;
        let script = format!("{fake}
{POPUP_SCRIPT}
{run}");
        let mut child = std::process::Command::new("osascript")
            .args(["-l", "JavaScript", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), script.as_bytes()).unwrap();
        let out: serde_json::Value = serde_json::from_slice(&child.wait_with_output().unwrap().stdout).unwrap();
        assert_eq!(out["out"], json!(["http://127.0.0.1:8765/login", 1]), "{out}");
        assert!(out["asked"].as_array().unwrap().iter().any(|asked| asked == r#"{"active":true,"windowId":7}"#), "{out}");
        assert!(out["asked"].as_array().unwrap().iter().any(|asked| asked == r#"{"url":"https://*/*"}"#), "another query is the extension's own: {out}");
    }

    /// The popup opens under the pointer that pressed its button, at the page's top, and never past the page's sides.
    #[test]
    fn a_popup_opens_under_the_button_inside_the_page() {
        let page = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(1000.0, 700.0));
        let under = anchor(page, NSPoint::new(900.0, 820.0));
        assert_eq!((under.origin.x, under.origin.y + under.size.height), (710.0, 796.0));
        let at_edge = anchor(page, NSPoint::new(1095.0, 820.0));
        assert_eq!(at_edge.origin.x + at_edge.size.width, 1092.0, "kept inside the page's right edge");
        let small = anchor(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(500.0, 300.0)), NSPoint::new(250.0, 300.0));
        assert!(small.size.height <= 288.0 && small.origin.y >= 0.0, "{small:?}");
    }
}
