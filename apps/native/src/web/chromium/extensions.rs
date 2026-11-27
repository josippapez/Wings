//! The extensions a profile added (`web::extensions`, the one list, each checked file by file before it loads) under
//! Chromium: loaded into CEF's global context over the browser target's DevTools pipe (`Extensions.loadUnpacked`),
//! uninstalled the same way when turned off or removed (Chromium deletes what it stored then), shown as buttons in the
//! page's toolbar, and a button's popup opened in a small frameless window under it.
//!
//! The DevTools protocol has no badges, so each loaded extension's worker is followed over the pipe as it starts
//! (`Target.setAutoAttach`), and Wings' hook in it (`BADGE_HOOK`) reads its badges once, then again after every
//! `action.setBadgeText`, and tells them to a binding only that session has: no polling. A followed worker stays running
//! while Chromium does, as one with DevTools open does; workers of sites and of other extensions run on untouched.
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
    /// The sites its copy may reach (`ready_copy`): only on a page one of them covers does its popup hear the page's
    /// address.
    hosts: Vec<String>,
}

type Answered = Box<dyn FnOnce(Result<Value, String>)>;

thread_local! {
    static LOADED: RefCell<Vec<Loaded>> = RefCell::default();
    static ASKED: RefCell<HashMap<u64, Answered>> = RefCell::default();
    static NEXT: Cell<u64> = const { Cell::new(1) };
    static POPUP_OPEN: RefCell<Option<Popup>> = const { RefCell::new(None) };
    /// The badges each extension set, by its Chromium id (`BADGE_HOOK`).
    static BADGES: RefCell<HashMap<String, Badges>> = RefCell::default();
    /// The extension workers followed, by their session on the browser target, with their extension's Chromium id.
    static WORKERS: RefCell<HashMap<String, String>> = RefCell::default();
    /// The extension whose popup lost the keyboard last, and when.
    static CLOSED: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

/// Asks the browser target `method` with `params`, and `done` hears its answer.
fn ask(method: &str, params: Value, done: impl FnOnce(Result<Value, String>) + 'static) {
    ask_in(None, method, params, done);
}

/// `ask`, of the target attached as `session` (an extension's worker), or of the browser target for none.
fn ask_in(session: Option<&str>, method: &str, params: Value, done: impl FnOnce(Result<Value, String>) + 'static) {
    let id = NEXT.replace(NEXT.get() + 1);
    let message = super::devtools::request(id, method, params, session);
    ASKED.with_borrow_mut(|asked| asked.insert(id, Box::new(done)));
    if !runtime::send(message.as_bytes()) {
        if let Some(done) = ASKED.with_borrow_mut(|asked| asked.remove(&id)) {
            done(Err("Chromium can't load extensions in this run of Wings".into()));
        }
    }
}

/// The events of the browser target and its workers that `heard` follows.
const FOLLOWED: [&str; 3] = ["Target.attachedToTarget", "Target.detachedFromTarget", "Runtime.bindingCalled"];

/// Whether `heard` reads `message`, by its first bytes, on the pipe's own thread: a followed worker's console sends an
/// event for each line, which never needs the main thread.
pub fn follows(message: &[u8]) -> bool {
    super::devtools::event_name(message).is_none_or(|name| FOLLOWED.iter().any(|followed| followed.as_bytes() == name))
}

/// A message from the browser target: the answer to a request of `ask`'s, or an event: a worker starting or gone, or
/// what a followed worker's badges are now.
pub fn heard(message: &[u8]) {
    let Ok(message) = serde_json::from_slice::<Value>(message) else { return };
    let Some(id) = message["id"].as_u64() else {
        let params = &message["params"];
        match message["method"].as_str() {
            Some("Target.attachedToTarget") => worker_started(params),
            Some("Target.detachedFromTarget") => {
                if let Some(chrome) = params["sessionId"].as_str().and_then(|session| WORKERS.with_borrow_mut(|workers| workers.remove(session))) {
                    eprintln!("[chromium] no longer following an extension's worker: {chrome}");
                }
            }
            Some("Runtime.bindingCalled") if params["name"] == BADGE_BINDING => {
                if let (Some(session), Some(payload)) = (message["sessionId"].as_str(), params["payload"].as_str()) {
                    badges_heard(session, payload);
                }
            }
            _ => {}
        }
        return;
    };
    let Some(done) = ASKED.with_borrow_mut(|asked| asked.remove(&id)) else { return };
    done(match message.get("error") {
        Some(error) => Err(error["message"].as_str().unwrap_or("Chromium refused").to_owned()),
        None => Ok(message["result"].clone()),
    });
}

/// CEF's global context is up: its profile's extensions load, each checked first, and every worker that starts waits
/// to be looked at (`worker_started`).
pub fn started() {
    load_checked();
    let workers = json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true, "filter": [{ "type": "service_worker" }, { "exclude": true }] });
    ask("Target.setAutoAttach", workers, |answer| {
        if let Err(why) = answer {
            eprintln!("[chromium] extensions' workers aren't followed, so their buttons show no badges: {why}");
        }
    });
}

/// The binding the badge hook tells, only in the sessions of followed workers.
const BADGE_BINDING: &str = "wingsBadge";

/// Run in a followed extension's worker: reads its badges, for every page and for each tab whose address it can see, and
/// again after each `setBadgeText` it calls, and tells them to the binding.
pub const BADGE_HOOK: &str = r#"(() => {
  const action = globalThis.chrome && chrome.action;
  const tell = globalThis.wingsBadge;
  if (!action || typeof action.setBadgeText !== "function" || typeof tell !== "function" || action.setBadgeText.wings) return;
  const read = () =>
    Promise.all([action.getBadgeText({}), chrome.tabs.query({})])
      .then(([all, tabs]) => Promise.all(tabs.filter((tab) => tab.url).map((tab) => action.getBadgeText({ tabId: tab.id }).then((text) => [tab.url, text])))
        .then((pages) => tell(JSON.stringify({ all, pages }))))
      .catch(() => {});
  const set = action.setBadgeText;
  const wrapped = function (...args) { const done = set.apply(this, args); read(); return done; };
  wrapped.wings = true;
  action.setBadgeText = wrapped;
  read();
})();"#;

/// The badges of one extension: for every page, and for each page's address that has one of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Badges {
    pub all: String,
    pub pages: HashMap<String, String>,
}

/// The most pages' badges kept of one extension: what it tells is its own, not Wings'.
const BADGED_PAGES_MOST: usize = 200;

impl Badges {
    /// What the hook told, or none for what isn't that.
    pub fn read(payload: &str) -> Option<Self> {
        let told: Value = serde_json::from_str(payload).ok()?;
        let pages = told["pages"].as_array()?.iter().take(BADGED_PAGES_MOST).filter_map(|page| Some((page[0].as_str()?.to_owned(), page[1].as_str()?.to_owned()))).collect();
        Some(Self { all: told["all"].as_str().unwrap_or_default().to_owned(), pages })
    }

    /// The badge on the button of the page at `url`.
    pub fn of(&self, url: &str) -> &str {
        self.pages.get(url).unwrap_or(&self.all)
    }
}

/// A worker started, waiting: one of an extension the person added is followed, any other runs on alone.
fn worker_started(params: &Value) {
    let Some(session) = params["sessionId"].as_str() else { return };
    let url = params["targetInfo"]["url"].as_str().unwrap_or_default();
    let chrome = url.strip_prefix("chrome-extension://").and_then(|rest| rest.split('/').next()).unwrap_or_default();
    let added = !chrome.is_empty() && LOADED.with_borrow(|loaded| loaded.iter().any(|loaded| loaded.chrome == chrome));
    ask_in(Some(session), "Runtime.runIfWaitingForDebugger", json!({}), |_| {});
    if !added {
        return ask("Target.detachFromTarget", json!({ "sessionId": session }), |_| {});
    }
    if WORKERS.with_borrow_mut(|workers| workers.insert(session.to_owned(), chrome.to_owned())).is_some() {
        return;
    }
    // After it runs, so its `chrome.action` is there.
    ask_in(Some(session), "Runtime.enable", json!({}), |_| {});
    ask_in(Some(session), "Runtime.addBinding", json!({ "name": BADGE_BINDING }), |_| {});
    ask_in(Some(session), "Runtime.evaluate", json!({ "expression": BADGE_HOOK }), |answer| {
        if let Err(why) = answer {
            eprintln!("[chromium] an extension's badges can't be read: {why}");
        }
    });
}

/// A worker that started as its extension loaded, before Chromium said which it was, is followed now.
fn follow_running(chrome: &str) {
    let prefix = format!("chrome-extension://{chrome}/");
    ask("Target.getTargets", json!({}), move |answer| {
        let Ok(answer) = answer else { return };
        for target in answer["targetInfos"].as_array().into_iter().flatten() {
            let running = target["type"] == "service_worker" && target["url"].as_str().is_some_and(|url| url.starts_with(&prefix)) && target["attached"] != true;
            if let (true, Some(id)) = (running, target["targetId"].as_str()) {
                // Its `Target.attachedToTarget` follows it.
                ask("Target.attachToTarget", json!({ "targetId": id, "flatten": true }), |_| {});
            }
        }
    });
}

/// A followed worker told its extension's badges: the pages show them.
fn badges_heard(session: &str, payload: &str) {
    let (Some(chrome), Some(badges)) = (WORKERS.with_borrow(|workers| workers.get(session).cloned()), Badges::read(payload)) else { return };
    let changed = BADGES.with_borrow_mut(|all| all.insert(chrome, badges.clone()).is_none_or(|was| was != badges));
    if let (true, Some(profile)) = (changed, runtime::global()) {
        refresh(&profile.plugin);
    }
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
    let (dir, hosts) = match chromium_copy(extension) {
        Ok((dir, hosts)) => (dir.to_string_lossy().into_owned(), hosts),
        Err(why) => return done(Err(why)),
    };
    ask("Extensions.loadUnpacked", json!({ "path": &dir }), move |answer| match answer {
        Ok(result) => {
            let Some(chrome) = result["id"].as_str().map(str::to_owned) else { return done(Err("Chromium didn't say which extension it loaded".into())) };
            #[cfg(debug_assertions)]
            eprintln!("[chromium] loaded {chrome}; Wings works out {:?}", chromium_id(&manifest_of(std::path::Path::new(&dir)), std::path::Path::new(&dir)));
            let (button, popup) = action_of(&manifest);
            follow_running(&chrome);
            LOADED.with_borrow_mut(|loaded| loaded.push(Loaded { id, chrome, name, popup, button, icon, hosts }));
            refresh(&plugin);
            done(Ok(()))
        }
        Err(why) => done(Err(why)),
    });
}

/// Takes the extension `id` out of Chromium, turned off or removed: its copy goes, and what it stored there. One loaded
/// now is uninstalled over the pipe, which deletes what it stored in its profile; whether it is or not (Chromium not
/// running, or running for another profile), its own folders of storage go from every profile Chromium keeps
/// (`forget_storage`): Chromium forgets an extension loaded this way at its next start, its preferences only, and left
/// Bitwarden's vault and session there.
pub fn unload(profile: &Profile, id: &str, done: impl FnOnce() + 'static) {
    let loaded = LOADED.with_borrow_mut(|loaded| {
        let at = loaded.iter().position(|loaded| loaded.id == id)?;
        Some(loaded.remove(at))
    });
    if POPUP_OPEN.with_borrow(|popup| popup.as_ref().is_some_and(|popup| popup.id == id)) {
        close_popup();
    }
    let copy = copy_of(id);
    let chrome = loaded.as_ref().map(|loaded| loaded.chrome.clone()).or_else(|| {
        let copy = copy.as_deref()?;
        let manifest = Some(manifest_of(copy)).filter(|manifest| manifest.get("key").is_some()).or_else(|| Some(manifest_of(&extensions::folder(profile, id)?)))?;
        chromium_id(&manifest, copy)
    });
    if let Some(copy) = &copy {
        let _ = std::fs::remove_dir_all(copy);
    }
    let Some(loaded) = loaded else {
        if let (Some(chrome), Some(root)) = (chrome, runtime::profiles_root()) {
            forget_storage(&root, &chrome);
        }
        return done();
    };
    let plugin = profile.plugin.clone();
    BADGES.with_borrow_mut(|badges| badges.remove(&loaded.chrome));
    ask("Extensions.uninstall", json!({ "id": loaded.chrome }), move |answer| {
        if let Err(why) = answer {
            eprintln!("[chromium] couldn't uninstall an extension: {why}");
        }
        if let Some(root) = runtime::profiles_root() {
            forget_storage(&root, &loaded.chrome);
        }
        refresh(&plugin);
        done()
    });
}

/// The folders of its own Chromium 156 keeps an extension's storage in, in a profile: `chrome.storage`'s three areas,
/// its rules for `declarativeNetRequest` (`extensions/common/constants.h`,
/// `declarative_net_request/file_backed_ruleset_source.cc`), by its id, and its IndexedDB, by its origin
/// (`content/browser/indexed_db/file_path_util.cc`: `chrome-extension_<id>_0`). What it keeps in databases all
/// extensions and sites share (`Extension State`, `Local Storage`, `Service Worker`) only Chromium can take out, as it
/// does as it uninstalls one loaded.
fn storage_of(chrome: &str) -> [String; 7] {
    let indexed = format!("chrome-extension_{chrome}_0");
    [
        format!("Local Extension Settings/{chrome}"),
        format!("Sync Extension Settings/{chrome}"),
        format!("Managed Extension Settings/{chrome}"),
        format!("DNR Extension Rules/{chrome}"),
        format!("IndexedDB/{indexed}.indexeddb.leveldb"),
        format!("IndexedDB/{indexed}.indexeddb.blob"),
        format!("IndexedDB/{indexed}"),
    ]
}

/// Deletes what the extension `chrome` stored in its own folders (`storage_of`) in every profile under `root`, Chromium's
/// profiles' folder: the global context keeps its data in `Default` whichever profile started Chromium.
fn forget_storage(root: &std::path::Path, chrome: &str) {
    if chrome.len() != 32 || !chrome.bytes().all(|byte| (b'a'..=b'p').contains(&byte)) {
        return;
    }
    let Ok(profiles) = std::fs::read_dir(root) else { return };
    for profile in profiles.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()) && entry.file_name() != "extensions") {
        for folder in storage_of(chrome) {
            let at = profile.path().join(folder);
            let _ = if at.is_dir() { std::fs::remove_dir_all(&at) } else { std::fs::remove_file(&at) };
        }
    }
}

/// The id Chromium gives an extension with `manifest` loaded from the folder `copy`: from its manifest's `key` when it has
/// one, the SHA-256 of the key's bytes (`Extension::ParsePEMKeyBytes`, base64 strictly, or PEM), else of the folder's
/// path as Chromium resolves it (`crx_file::id_util::GenerateIdForPath`), its first 16 bytes written in the letters a to
/// p (`GenerateIdFromHash`). None for a key Chromium refuses.
fn chromium_id(manifest: &Value, copy: &std::path::Path) -> Option<String> {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let hashed = match manifest.get("key") {
        Some(key) => {
            let key = key.as_str().filter(|key| !key.is_empty() && key.len() <= 100 * 1024)?;
            let body = match key.starts_with("-----BEGIN") {
                true => {
                    let collapsed = key.split_whitespace().collect::<Vec<_>>().join(" ");
                    let start = collapsed.find("KEY-----")? + "KEY-----".len();
                    let end = collapsed.rfind("-----END")?;
                    collapsed.get(start..end)?.trim().replace(' ', "")
                }
                false => key.to_owned(),
            };
            Sha256::digest(base64::engine::general_purpose::STANDARD.decode(body).ok()?)
        }
        None => Sha256::digest(copy.as_os_str().as_encoded_bytes()),
    };
    Some(hashed.iter().take(16).flat_map(|byte| [byte >> 4, byte & 0xf]).map(|nibble| char::from(b'a' + nibble)).collect())
}

/// The buttons of `profile`'s extensions for its page at `url`, by name (`views::shown`).
pub fn buttons(profile: &Profile, imported: bool, url: &str) -> Vec<views::Extension> {
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
                badge: BADGES.with_borrow(|badges| badges.get(&loaded.chrome).map(|badges| badges.of(url).to_owned())).unwrap_or_default(),
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
/// then: a clone (APFS copies no data), readied for Chromium (`ready_copy`). Answers it and the sites it may reach.
fn chromium_copy(extension: &extensions::Verified) -> Result<(std::path::PathBuf, Vec<String>), String> {
    let copy = copy_of(&extension.id).ok_or("No folder for Chromium's copy")?;
    let _ = std::fs::remove_dir_all(&copy);
    std::fs::create_dir_all(copy.parent().unwrap_or(&copy)).map_err(|error| error.to_string())?;
    let cloned = std::process::Command::new("/bin/cp").arg("-cR").arg(&extension.dir).arg(&copy).status();
    if !cloned.is_ok_and(|status| status.success()) {
        let _ = std::fs::remove_dir_all(&copy);
        return Err("Couldn't copy the extension for Chromium".into());
    }
    match ready_copy(&copy, &extension.permissions, &extension.hosts) {
        Ok(hosts) => Ok((copy, hosts)),
        Err(why) => {
            let _ = std::fs::remove_dir_all(&copy);
            Err(why)
        }
    }
}

/// Readies the copy of an extension in `copy`, which the person allowed `permissions` and `hosts` of: without Wings'
/// WebKit shim, whose `__wings_` files Chromium refuses ("Filenames starting with "_" are reserved"), without the Web
/// Store's `_metadata`, and with a manifest that asks for no more than the person allowed (`granted`, `narrow_manifest`):
/// Chromium grants whatever a manifest asks for, with no prompt, and a manifest's optional permissions and sites on its
/// `permissions.request`, which Wings' dialog never showed. Answers the sites the copy may reach.
fn ready_copy(copy: &std::path::Path, permissions: &[String], hosts: &[String]) -> Result<Vec<String>, String> {
    wings_web_extensions::unshim_service_worker(copy).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_dir_all(copy.join("_metadata"));
    let reached = wings_web_extensions::narrow_manifest(copy, &|name| granted(name, permissions), hosts).map_err(|error| error.to_string())?;
    if let Some(popup) = action_of(&manifest_of(copy)).1 {
        with_popup_script(copy, &popup)?;
    }
    Ok(reached)
}

/// The permissions Chromium's copy of an extension may keep, each reviewed: those WebKit has for an extension (its
/// `supportedPermissions`, `nativeMessaging` aside, which Wings' dialog says it never allows), and a few of Chromium's
/// own that stay within what the extension does by itself (`idle`, `offscreen`, `clipboardRead`, `notifications`,
/// `sidePanel`). Any other, a new one too, isn't granted: Chromium grants whatever a manifest asks for, and some reach
/// past the extension's sites (the debugger, a proxy, other extensions, capture of the screen).
const GRANTED: [&str; 20] = [
    "activeTab",
    "alarms",
    "clipboardRead",
    "clipboardWrite",
    "contextMenus",
    "cookies",
    "declarativeNetRequest",
    "declarativeNetRequestFeedback",
    "declarativeNetRequestWithHostAccess",
    "idle",
    "menus",
    "notifications",
    "offscreen",
    "scripting",
    "sidePanel",
    "storage",
    "tabs",
    "unlimitedStorage",
    "webNavigation",
    "webRequest",
];

/// Whether Chromium's copy of an extension keeps the permission `name`: one on the reviewed list (`GRANTED`) that Wings'
/// dialog showed as the person allowed it (`allowed`). WebKit grants by its own rule (`mac::grant`).
fn granted(name: &str, allowed: &[String]) -> bool {
    GRANTED.contains(&name) && allowed.iter().any(|allowed| allowed == name)
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

/// The address of `popup` in the extension `chrome`, when the path stays inside it, naming the page at `page_url` when
/// there is one (`POPUP_SCRIPT` reads it).
fn popup_url(chrome: &str, popup: &str, page_url: Option<&str>) -> Option<String> {
    let path = std::path::Path::new(popup);
    let page: Option<String> = page_url.map(|page_url| {
        page_url
            .bytes()
            .map(|byte| match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (byte as char).to_string(),
                _ => format!("%{byte:02X}"),
            })
            .collect()
    });
    path.components().all(|part| matches!(part, std::path::Component::Normal(_))).then(|| match page {
        Some(page) => format!("chrome-extension://{chrome}/{popup}?wings-page={page}"),
        None => format!("chrome-extension://{chrome}/{popup}"),
    })
}

/// The page at `url` as its popup is told of it: only when one of the sites the extension may reach (`hosts`) covers it.
/// One it can't reach is a page Chromium wouldn't show it either.
fn seen_page<'a>(hosts: &[String], url: &'a str) -> Option<&'a str> {
    hosts.iter().any(|host| wings_web_extensions::pattern_reaches(host, url)).then_some(url)
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
    let page_url = page.url_now();
    let url = loaded.popup.as_deref().and_then(|popup| popup_url(&loaded.chrome, popup, seen_page(&loaded.hosts, &page_url))).ok_or(NO_POPUP)?;
    let (Some(parent), Some(page_ns)) = (page.parent.borrow().clone(), page.ns.borrow().clone()) else { return Err("The page isn't on screen".into()) };
    let visible = page_ns.screen().map_or_else(|| page_ns.frame(), |screen| screen.visibleFrame());
    let frame = anchor(page_ns.frame(), NSEvent::mouseLocation(), visible);
    POPUP_OPEN.with_borrow_mut(|popup| {
        *popup = Some(Popup { id: id.to_owned(), page: super::page::weak_of(page), window: None, ns: None, parent: parent.clone() })
    });
    runtime::opened();
    let id = id.to_owned();
    frameless::open(
        &url,
        cef_rect(frame),
        Some(PopupClient::new(loaded.chrome.clone())),
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

/// Escape pressed while a popup has the keyboard closes it, whatever its page does with the key, as Chrome's does: the
/// popup's own key handler (`PopupKeys`) didn't hear Escape typed in Bitwarden's email field. Installed once, the
/// first time a popup opens.
fn escape_closes_popup() {
    thread_local!(static ON: Cell<bool> = const { Cell::new(false) });
    if ON.replace(true) {
        return;
    }
    let block = block2::RcBlock::new(|event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit hands the event it is about to send, alive for this call, on the main thread.
        let event_ref = unsafe { event.as_ref() };
        let in_popup = objc2::MainThreadMarker::new().and_then(|mtm| event_ref.window(mtm)).is_some_and(|window| {
            POPUP_OPEN.with_borrow(|popup| popup.as_ref().and_then(|popup| popup.ns.as_ref()).is_some_and(|ns| std::ptr::eq(&**ns, &*window)))
        });
        if in_popup && event_ref.keyCode() == ESCAPE {
            super::next_turn(close_popup);
            return std::ptr::null_mut();
        }
        event.as_ptr()
    });
    // SAFETY: the block returns the event it was given, or none for the one it took. The monitor lasts as long as the
    // app, so it is never removed.
    std::mem::forget(unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(objc2_app_kit::NSEventMask::KeyDown, &block) });
}

/// The key code of Escape.
const ESCAPE: u16 = 53;

/// Where a popup goes: its top just under the page's top edge (the toolbar's foot), centred under the pointer that
/// pressed the button and kept inside the page's sides, in screen points. It may reach below the page, down to the
/// bottom of the screen's visible area (`visible`), as Chrome's does: a short window cut Bitwarden's buttons off.
fn anchor(page: NSRect, pointer: NSPoint, visible: NSRect) -> NSRect {
    let top = page.origin.y + page.size.height - 4.0;
    let low = page.origin.x + 8.0;
    let high = (page.origin.x + page.size.width - POPUP.width - 8.0).max(low);
    let x = (pointer.x - POPUP.width / 2.0).clamp(low, high);
    let height = POPUP.height.min(top - visible.origin.y - 8.0).max(120.0);
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
    frameless::never_fullscreen(&ns);
    window.show();
    frameless::as_popup(&ns, 10.0);
    frameless::attach(&ns, &parent, frame);
    // It opens under the pointer, which then never moves in Wings' window: the button's tooltip went with it.
    super::focus::pointer_left(&parent);
    escape_closes_popup();
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

/// The popup's window, while one shows, for a debug build's checks.
#[cfg(debug_assertions)]
pub fn popup_window() -> Option<Retained<NSWindow>> {
    POPUP_OPEN.with_borrow(|popup| popup.as_ref().and_then(|popup| popup.ns.clone()))
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

// The popup of the extension `chrome`: it refuses what a page refuses (permissions, downloads), and shows only its own
// extension's pages.
wrap_client! {
    struct PopupClient {
        chrome: String,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(PopupLifeSpan::new())
        }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(PopupKeys::new())
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(PopupRequests::new(self.chrome.clone()))
        }
        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(super::page::Refuse::new())
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(super::page::NoDownloads::new())
        }
    }
}

/// Whether the popup of the extension `chrome` may go to `url`: the popup itself only to a page of that extension, a
/// frame in it where a page's frame may go (`may_navigate`). A popup that went anywhere else would be a page of any
/// site in a window with no address, over Wings.
fn popup_may_go(chrome: &str, url: &str, main_frame: bool) -> bool {
    if !main_frame {
        return crate::web::may_navigate(url, false);
    }
    url.strip_prefix("chrome-extension://").and_then(|rest| rest.split_once('/')).is_some_and(|(id, _)| id == chrome)
}

wrap_request_handler! {
    struct PopupRequests {
        chrome: String,
    }

    impl RequestHandler {
        fn on_before_browse(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, _user_gesture: ::std::os::raw::c_int, _is_redirect: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            let url = request.map(|request| CefString::from(&request.url()).to_string()).unwrap_or_default();
            let main_frame = frame.is_none_or(|frame| frame.is_main() != 0);
            i32::from(!popup_may_go(&self.chrome, &url, main_frame))
        }

        /// A link it opens in a new tab goes nowhere, as one in a new window does (`on_before_popup`).
        fn on_open_urlfrom_tab(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _target_url: Option<&CefString>, _target_disposition: WindowOpenDisposition, _user_gesture: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            1
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
            if event.type_ == KeyEventType::RAWKEYDOWN && event.native_key_code == i32::from(ESCAPE) {
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

    /// Chromium's copy asks for what the person allowed in Wings' dialog: of Bitwarden's manifest, the permissions the
    /// dialog showed but those it doesn't explain, the sites allowed, its content scripts there, nothing optional, and
    /// never native messaging. Its popup still gets Wings' first script.
    #[test]
    fn chromiums_copy_of_an_extension_asks_for_only_what_wings_dialog_allowed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("popup")).unwrap();
        std::fs::write(dir.path().join("popup/index.html"), "<head></head>").unwrap();
        let manifest = json!({
            "manifest_version": 3, "name": "Bitwarden", "version": "1",
            "permissions": ["activeTab", "alarms", "clipboardRead", "idle", "offscreen", "scripting", "storage", "tabs", "debugger", "nativeMessaging", "proxy", "cookies"],
            "optional_permissions": ["nativeMessaging", "privacy"], "optional_host_permissions": ["<all_urls>"],
            "host_permissions": ["https://*/*", "http://*/*"],
            "content_scripts": [{ "matches": ["*://*/*", "file:///*"], "js": ["content.js"] }],
            "action": { "default_popup": "popup/index.html" },
        });
        std::fs::write(dir.path().join("manifest.json"), manifest.to_string()).unwrap();
        let allowed: Vec<String> = ["activeTab", "alarms", "clipboardRead", "idle", "offscreen", "scripting", "storage", "tabs", "debugger", "nativeMessaging", "proxy"].map(String::from).to_vec();
        let hosts: Vec<String> = ["https://*/*", "http://*/*", "*://*/*"].map(String::from).to_vec();
        let reached = ready_copy(dir.path(), &allowed, &hosts).unwrap();
        let copy: Value = serde_json::from_slice(&std::fs::read(dir.path().join("manifest.json")).unwrap()).unwrap();
        assert_eq!(copy["permissions"], json!(["activeTab", "alarms", "clipboardRead", "idle", "offscreen", "scripting", "storage", "tabs"]), "cookies wasn't shown");
        assert_eq!(copy["host_permissions"], json!(["https://*/*", "http://*/*"]));
        assert_eq!(copy["content_scripts"], json!([{ "matches": ["*://*/*"], "js": ["content.js"] }]), "not on this Mac's files, which weren't allowed");
        assert!(copy.get("optional_permissions").is_none() && copy.get("optional_host_permissions").is_none());
        assert_eq!(reached, ["https://*/*", "http://*/*", "*://*/*"]);
        assert!(std::fs::read_to_string(dir.path().join("popup/index.html")).unwrap().contains(POPUP_SCRIPT_FILE));
    }

    /// Of what the dialog showed, only a permission on the reviewed list is granted: Bitwarden's set, but not a name
    /// nobody reviewed (a new one too), not one that reaches past the extension's sites, and none the dialog didn't show.
    #[test]
    fn chromium_keeps_only_reviewed_permissions_the_dialog_showed() {
        let bitwarden = [
            "activeTab", "alarms", "clipboardRead", "clipboardWrite", "contextMenus", "idle", "notifications", "offscreen", "scripting", "sidePanel", "storage",
            "tabs", "unlimitedStorage", "webNavigation", "webRequest",
        ];
        let shown: Vec<String> = bitwarden.iter().map(|name| name.to_string()).chain(["webRequestAuthProvider".into(), "someNewPermission".into()]).collect();
        let kept: Vec<&str> = shown.iter().map(String::as_str).filter(|name| granted(name, &shown)).collect();
        assert_eq!(kept, bitwarden, "a name nobody reviewed isn't granted");
        let risky = [
            "debugger", "proxy", "management", "desktopCapture", "tabCapture", "pageCapture", "browsingData", "privacy", "contentSettings", "downloads",
            "downloads.open", "nativeMessaging", "webAuthenticationProxy", "userScripts", "geolocation", "system.storage", "webRequestBlocking", "vpnProvider",
        ];
        let shown: Vec<String> = risky.map(String::from).to_vec();
        for name in risky {
            assert!(!GRANTED.contains(&name) && !granted(name, &shown), "{name}");
        }
        assert!(!granted("cookies", &[]), "not shown");
    }

    /// Chromium's id of an extension comes from its manifest's key, Bitwarden's from the Web Store's, or else from the
    /// path of the folder it loads from.
    #[test]
    fn chromiums_id_of_an_extension_comes_from_its_key_or_its_folder() {
        let bitwarden = json!({ "key": "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAmqKbvreshyXRuN2gikeR1idqR6KL0Di89JZcMyD4bjJRZVmQO7aznSGSALIHzSAUGYocUYBNDOP5QAhImxXyQ1qG8+goXs93v9GzrNJETdVuCEhqBggC4/DFabryJZDiKvZ2Jl0DM7MsWdoybZPwrj70V3aJ/nVNOMkf868scNTMliwitCqqjT5baTANsG0DkZWQExD4lSXzSZHH9MEO8q0iZ7RRlNuGRBAkZgNV8FwZRsPKm/rwQ9dy3VpgLcmLp5GiMt+kAEncqKAkuRYnhVXXBsKqIyYTMjHSLkLnpfFySyOPLBdS617i/PGNiP/MT6Xy6z//v5NozUgaAZ4gJQIDAQAB" });
        assert_eq!(chromium_id(&bitwarden, std::path::Path::new("/anywhere")).as_deref(), Some("nngceckbapebfimnlniiiahkandclblb"));
        let at = |path: &str| chromium_id(&json!({ "name": "x" }), std::path::Path::new(path)).unwrap();
        assert_eq!(at("/a/extensions/1"), at("/a/extensions/1"));
        assert_ne!(at("/a/extensions/1"), at("/a/extensions/2"));
        assert!(at("/a/extensions/1").len() == 32 && at("/a/extensions/1").bytes().all(|byte| (b'a'..=b'p').contains(&byte)));
        assert_eq!(chromium_id(&json!({ "key": "not base64!" }), std::path::Path::new("/a")), None, "a key Chromium refuses");
    }

    /// Its own folders of storage go from every profile (Chromium's global context keeps its data in `Default`); another
    /// extension's, the databases they share, and anything named by something that isn't an id stay.
    #[test]
    fn an_extensions_storage_goes_from_every_profile_and_nothing_else_does() {
        let root = tempfile::tempdir().unwrap();
        let (bitwarden, other) = ("nngceckbapebfimnlniiiahkandclblb", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        for profile in ["Default", "browser.s"] {
            for folder in storage_of(bitwarden).iter().chain(storage_of(other).iter()) {
                std::fs::create_dir_all(root.path().join(profile).join(folder)).unwrap();
            }
            std::fs::create_dir_all(root.path().join(profile).join("Local Storage/leveldb")).unwrap();
        }
        forget_storage(root.path(), bitwarden);
        for profile in ["Default", "browser.s"] {
            for folder in storage_of(bitwarden) {
                assert!(!root.path().join(profile).join(&folder).exists(), "{profile}/{folder}");
            }
            for folder in storage_of(other) {
                assert!(root.path().join(profile).join(&folder).exists(), "{profile}/{folder}");
            }
            assert!(root.path().join(profile).join("Local Storage/leveldb").exists());
        }
        forget_storage(root.path(), "../../Default");
        assert!(root.path().join("Default/Local Storage/leveldb").exists());
    }

    /// Its popup hears the page's address only on a page the extension may reach; on any other it opens without it.
    #[test]
    fn a_popup_is_told_the_page_only_where_the_extension_reaches() {
        let hosts = vec!["http://127.0.0.1/*".to_owned()];
        assert_eq!(seen_page(&hosts, "http://127.0.0.1:8765/login"), Some("http://127.0.0.1:8765/login"));
        assert_eq!(seen_page(&hosts, "https://bank.example/account"), None);
        assert_eq!(seen_page(&[], "http://127.0.0.1:8765/login"), None);
        assert_eq!(popup_url("abc", "popup/index.html", None).as_deref(), Some("chrome-extension://abc/popup/index.html"));
    }

    /// The popup shows only its own extension's pages; a frame in it goes where a page's frame may.
    #[test]
    fn a_popup_shows_only_its_own_extensions_pages() {
        let own = "nngceckbapebfimnlniiiahkandclblb";
        assert!(popup_may_go(own, &format!("chrome-extension://{own}/popup/index.html#/login"), true));
        for elsewhere in ["https://evil.example/", "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/popup.html", "file:///etc/passwd", "data:text/html,hi", "about:blank"] {
            assert!(!popup_may_go(own, elsewhere, true), "{elsewhere}");
        }
        assert!(popup_may_go(own, "https://vault.bitwarden.com/connector.html", false), "a frame of a site");
        assert!(!popup_may_go(own, "file:///etc/passwd", false));
    }

    #[test]
    fn a_button_and_its_popup_come_from_the_manifest_and_a_popup_path_never_leaves_the_extension() {
        assert_eq!(action_of(&json!({ "action": { "default_popup": "popup/index.html" } })), (true, Some("popup/index.html".into())));
        assert_eq!(action_of(&json!({ "browser_action": { "default_popup": "/p.html" } })), (true, Some("p.html".into())));
        assert_eq!(action_of(&json!({ "action": {} })), (true, None));
        assert_eq!(action_of(&json!({ "name": "no button" })), (false, None));
        assert_eq!(
            popup_url("abc", "popup/index.html", Some("https://a.com/x?y=1")).as_deref(),
            Some("chrome-extension://abc/popup/index.html?wings-page=https%3A%2F%2Fa.com%2Fx%3Fy%3D1")
        );
        assert_eq!(popup_url("abc", "../../etc/x", Some("https://a.com/")), None);
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

    /// A popup told of no page (one the extension can't reach) leaves every query as the extension asked it, and
    /// throws nothing: it sees what Chromium shows it, as without Wings.
    #[test]
    fn a_popup_told_of_no_page_queries_as_the_extension_asked() {
        let fake = r#"
            const asked = [];
            const location = { search: "" };
            globalThis.chrome = { windows: { WINDOW_ID_CURRENT: -2 }, tabs: { query(info) { asked.push(JSON.stringify(info)); return Promise.resolve([]); } } };
            const original = chrome.tabs.query;
        "#;
        let run = r#"
            chrome.tabs.query({ active: true, currentWindow: true });
            function run() { return JSON.stringify({ asked, same: chrome.tabs.query === original }); }
        "#;
        let script = format!("{fake}\n{POPUP_SCRIPT}\n{run}");
        let mut child = std::process::Command::new("osascript")
            .args(["-l", "JavaScript", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), script.as_bytes()).unwrap();
        let out: serde_json::Value = serde_json::from_slice(&child.wait_with_output().unwrap().stdout).unwrap();
        assert_eq!(out, json!({ "asked": [r#"{"active":true,"currentWindow":true}"#], "same": true }));
    }

    /// What the hook tells is the extension's badge for every page and for each page it set one on, read by the page's
    /// address, at most `BADGED_PAGES_MOST` of them; anything else is no badge.
    #[test]
    fn an_extensions_badges_are_read_for_each_page_by_its_address() {
        let badges = Badges::read(r#"{"all":"","pages":[["http://127.0.0.1:8765/login","3"],["https://a.com/",""],[1,"x"]]}"#).unwrap();
        assert_eq!((badges.of("http://127.0.0.1:8765/login"), badges.of("https://a.com/"), badges.of("https://b.com/")), ("3", "", ""));
        let everywhere = Badges::read(r#"{"all":"!","pages":[]}"#).unwrap();
        assert_eq!(everywhere.of("https://b.com/"), "!");
        let many: Vec<Value> = (0..500).map(|at| json!([format!("https://s{at}.com/"), "1"])).collect();
        assert_eq!(Badges::read(&json!({ "all": "", "pages": many }).to_string()).unwrap().pages.len(), BADGED_PAGES_MOST);
        assert_eq!(Badges::read("not json"), None);
        assert_eq!(Badges::read(r#"{"all":"1"}"#), None);
    }

    /// The hook wraps `setBadgeText` once, reads badges and nothing else, and sends nothing anywhere but its binding.
    #[test]
    fn the_badge_hook_reads_badges_and_tells_only_its_binding() {
        assert!(BADGE_HOOK.contains("action.setBadgeText.wings") && BADGE_HOOK.contains(&format!("globalThis.{BADGE_BINDING}")));
        for sends in ["fetch(", "XMLHttpRequest", "WebSocket", "sendMessage", "eval(", "storage"] {
            assert!(!BADGE_HOOK.contains(sends), "{sends}");
        }
    }

    /// Only answers and the events `heard` follows cross to the main thread: a followed worker's console lines don't.
    #[test]
    fn only_answers_and_followed_events_leave_the_pipes_thread() {
        assert!(follows(br#"{"id":3,"result":{}}"#));
        assert!(follows(br#"{"method":"Target.attachedToTarget","params":{}}"#));
        assert!(follows(br#"{"method":"Runtime.bindingCalled","params":{},"sessionId":"S"}"#));
        assert!(!follows(br#"{"method":"Runtime.consoleAPICalled","params":{},"sessionId":"S"}"#));
        assert!(!follows(br#"{"method":"Runtime.executionContextCreated","params":{},"sessionId":"S"}"#));
    }

    /// The popup opens under the pointer that pressed its button, at the page's top, and never past the page's sides.
    #[test]
    fn a_popup_opens_under_the_button_inside_the_page() {
        let screen = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1512.0, 944.0));
        let page = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(1000.0, 700.0));
        let under = anchor(page, NSPoint::new(900.0, 820.0), screen);
        assert_eq!((under.origin.x, under.origin.y + under.size.height, under.size.height), (710.0, 796.0, 600.0));
        let at_edge = anchor(page, NSPoint::new(1095.0, 820.0), screen);
        assert_eq!(at_edge.origin.x + at_edge.size.width, 1092.0, "kept inside the page's right edge");
    }

    /// In a short window the popup reaches below the page, whole, down to the screen's visible bottom: Bitwarden's
    /// buttons were cut off at the page's foot. Only a screen too short for it cuts it.
    #[test]
    fn a_popup_in_a_short_window_reaches_below_the_page_but_not_off_the_screen() {
        let visible = NSRect::new(NSPoint::new(0.0, 40.0), NSSize::new(1512.0, 880.0));
        let short = NSRect::new(NSPoint::new(200.0, 500.0), NSSize::new(900.0, 300.0));
        let popup = anchor(short, NSPoint::new(600.0, 820.0), visible);
        assert_eq!((popup.size.height, popup.origin.y + popup.size.height), (600.0, 796.0), "whole, below the page's foot at 500");
        let low = NSRect::new(NSPoint::new(200.0, 60.0), NSSize::new(900.0, 300.0));
        let popup = anchor(low, NSPoint::new(600.0, 380.0), visible);
        assert_eq!(popup.origin.y, 48.0, "stops at the visible bottom");
    }
}
