//! Web pages in plugin pages: a `WebView` node (`page::tree::WebView`) is a place in the page that the system's web
//! engine draws over, as a page's `Terminal` is drawn over its place. GPUI can't draw a web page, so the engine's own view
//! (a `WKWebView` on macOS, `mac.rs`) sits over the window at the place the page left for it in its last draw, and the
//! page never draws for what the web page does.
//!
//! The engine's view is above everything GPUI draws, so it only shows while its place does: while the page is on screen
//! (`ui::place_view`), nothing of the page's own is over it (a `Dialog` or `Sheet`, `slots::uncovered_piece`) and no
//! overlay of the window that takes clicks (a menu, a popover, a dialog, `slots::blocking_covers`) lies over it. Otherwise
//! it is hidden, and it is gone with its node or its page.

pub mod extensions;
pub mod snapshot;
#[cfg(feature = "ui")]
mod views;

#[cfg(all(feature = "ui", target_os = "macos"))]
pub mod mac;

#[cfg(feature = "ui")]
pub use views::{Extension, Heard, Spec, State, Tell, Told, forget_page, shown, slot, sync, update};
#[cfg(all(test, feature = "test-support"))]
pub(crate) use views::testing;

#[cfg(all(feature = "ui", target_os = "macos"))]
pub use mac::host_natively;

/// What a plugin's call asks of WebKit, which only the window's thread may do (`page::plugin::Env::web`).
pub enum Job {
    /// Whether this Mac has extensions (macOS 15.4): `true` or `false`.
    Supported,
    /// The system's picker, for a folder or a package (ZIP, CRX or XPI): its path, or null.
    Pick,
    /// Every cookie and all site data the profile's web views kept go.
    Clear(Profile),
    /// An extension that passed `extensions::verify` loads into the profile's web views, or one unloads.
    Load { profile: Profile, extension: extensions::Verified },
    Unload { profile: Profile, id: String },
    /// Wings asks the person in its own dialog, with what an extension being added can do: `{ "allowed": true }` only on
    /// its Add (`allowed`).
    Add { ask: Ask },
    /// What the extension `id` stored goes (WebKit keeps it apart from its folder), then it unloads.
    Remove { profile: Profile, id: String },
    /// Another browser's cookies go into the profile's store, kept or not: how many it took. Unless `keep_signins`, they
    /// go as session cookies, which no store keeps past the run.
    Cookies { profile: Profile, keep_data: bool, keep_signins: bool, cookies: Vec<wings_browser_cookies::Cookie> },
}

/// The one gate every command a plugin's page sends its web view goes through, here and in WebKit (`views::update`,
/// `mac::WebPage::go`): moving about (load, back, forward, reload, stop) always goes; anything that reads or acts on what
/// the page holds (a script, its picture, its logs, the answer to its dialog) needs the plugin's `webScripts`
/// (`scripts`), never touches a page signed in with imported cookies (`imported`, of the document it would act on: the
/// one committed, or the dialog's own frame), and waits while a new page is on its way (`settled`), as it would act on
/// whichever page is there when it runs. Each command is named here, so a new one has to say which it is.
pub fn gate(go: &crate::page::tree::WebGo, scripts: bool, imported: bool, settled: bool) -> Result<(), &'static str> {
    use crate::page::tree::WebGo;
    match go {
        WebGo::Load(_) | WebGo::Back | WebGo::Forward | WebGo::Reload | WebGo::Stop => Ok(()),
        WebGo::Eval { .. } | WebGo::Snapshot | WebGo::Logs | WebGo::Dialog { .. } => match (scripts, imported, settled) {
            (false, _, _) => Err(NO_SCRIPTS),
            (true, true, _) => Err(IMPORTED),
            (true, false, false) => Err(LOADING),
            (true, false, true) => Ok(()),
        },
        // An extension's button opens its own popup, which the plugin never sees, on any page: it needs your click
        // (`needs_click`) and nothing more.
        WebGo::Extension(_) => Ok(()),
    }
}

/// Whether the command `go` runs only right after your click in the page, which it uses up, as `copyText` does: an
/// extension's button tells the extension you acted on the page (`activeTab`) and opens its popup, which takes the
/// keyboard, so neither a plugin nor an agent clicks it alone.
pub fn needs_click(go: &crate::page::tree::WebGo) -> bool {
    matches!(go, crate::page::tree::WebGo::Extension(_))
}

/// Why an extension's button didn't open without a click.
pub const NO_CLICK: &str = "A page can click an extension's button only right after you click or type in it";

/// Why one didn't run while a page loads.
pub const LOADING: &str = "The page is still loading: wait for it, then try again";

/// Why a command that reads or acts on a page didn't run, for a plugin that may not run scripts in its pages.
pub const NO_SCRIPTS: &str = "This plugin may not read or act on the web pages it shows (permissions.webScripts)";
/// Why one didn't run on a page signed in with imported cookies.
pub const IMPORTED: &str = "This page is signed in with cookies you imported, so no plugin reads or acts on it";

/// Notes the sites whose cookies are about to be imported into the profile in `profile_dir`, or that a sign-in carried
/// over from one (`taint`), by their domains (never the cookies), so nothing of the plugin's reads or acts on them
/// (`holds_imported`). Written before the cookies go in, and an import fails when it can't be: a cookie never goes where
/// the note doesn't name it. What this run knows is kept even when the file can't be written.
pub fn note_imported(profile_dir: &std::path::Path, domains: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut known = match imported_sites(profile_dir) {
        Some(known) => known,
        None => return Err("The list of imported sites is unreadable".into()),
    };
    known.extend(domains.into_iter().map(|domain| site(&domain)).filter(|domain| !domain.is_empty()));
    IMPORTED_SITES.with_borrow_mut(|cache| cache.insert(profile_dir.to_path_buf(), Some(known.clone())));
    std::fs::create_dir_all(profile_dir).map_err(|error| error.to_string())?;
    let json = serde_json::to_vec(&known).map_err(|error| error.to_string())?;
    // Written whole beside it and moved over, so a crash never leaves half a list.
    let (file, next) = (profile_dir.join("imported-sites.json"), profile_dir.join("imported-sites.json.next"));
    std::fs::write(&next, json).and_then(|()| std::fs::rename(&next, &file)).map_err(|error| format!("Couldn't note the imported sites: {error}"))
}

/// Whether a main-frame navigation from the page at `from` to `to` (or a redirect's hop to it) starts from or passes
/// through a site signed in with imported cookies, so where it ends may be signed in by it (a "Continue with" sign-in
/// coming back).
pub fn carries_imported(profile_dir: &std::path::Path, from: &str, to: &str) -> bool {
    holds_imported(profile_dir, from) || holds_imported(profile_dir, to)
}

/// A domain or host as the list keeps it: lower case, with no leading dot (a cookie's) or trailing one (`github.com.`).
fn site(domain: &str) -> String {
    domain.trim_start_matches('.').trim_end_matches('.').to_ascii_lowercase()
}

/// Whether the page at `url` is on a site whose cookies were imported into `profile`: one signed in with them. A list
/// that is there but can't be read counts as naming every site; none at all means nothing was ever imported, as it is
/// written before any cookie goes in.
pub fn holds_imported(profile_dir: &std::path::Path, url: &str) -> bool {
    let Some(host) = url.split_once("://").and_then(|(_, rest)| rest.split(['/', '?', '#']).next()) else { return false };
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    holds_imported_host(profile_dir, host.split(':').next().unwrap_or_default())
}

/// `holds_imported` for a host. One with no host (a frame of `about:`, `data:`, `blob:` or a `srcdoc`, whose origin
/// says nothing of the site) counts as imported, as it may be on one.
pub fn holds_imported_host(profile_dir: &std::path::Path, host: &str) -> bool {
    let host = site(host);
    if host.is_empty() {
        return true;
    }
    match imported_sites(profile_dir) {
        Some(known) => known.iter().any(|domain| host == *domain || host.ends_with(&format!(".{domain}"))),
        None => true,
    }
}

thread_local! {
    /// Each profile's list of imported sites, read once: a page's every console line asks, on the main thread.
    static IMPORTED_SITES: std::cell::RefCell<std::collections::HashMap<std::path::PathBuf, Option<std::collections::BTreeSet<String>>>> =
        std::cell::RefCell::default();
}

/// The domains of the sites whose cookies were imported into the profile in `profile_dir`: none when nothing was, and
/// `None` when the list is there but can't be read, which counts as every site.
fn imported_sites(profile_dir: &std::path::Path) -> Option<std::collections::BTreeSet<String>> {
    IMPORTED_SITES.with_borrow_mut(|cache| {
        cache
            .entry(profile_dir.to_path_buf())
            .or_insert_with(|| match std::fs::read(profile_dir.join("imported-sites.json")) {
                Ok(bytes) => serde_json::from_slice(&bytes).ok(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Default::default()),
                Err(_) => None,
            })
            .clone()
    })
}

/// The origin of `url` (`https://host:port`), which a page keeps as it moves about within itself (`pushState`, a
/// `#`), or none.
pub fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    Some(format!("{}://{}", scheme.to_ascii_lowercase(), authority.to_ascii_lowercase()))
}

/// What an extension's add answers when the person didn't allow it.
pub const NOT_ALLOWED: &str = "The extension wasn't added: you didn't allow it";

/// Whether the person allowed an extension in Wings' dialog, from what `Job::Add` answered: only `{ "allowed": true }`,
/// which only that dialog's Add makes, with why it didn't load if it didn't. Anything else is not allowed.
pub fn allowed(answer: &Result<String, String>) -> Option<Option<String>> {
    let answer: serde_json::Value = serde_json::from_str(answer.as_ref().ok()?).ok()?;
    (answer["allowed"] == true).then(|| answer["error"].as_str().map(str::to_owned))
}

/// What Wings' dialog asks before an extension is added: it is granted what it lists, and only once it is allowed.
pub struct Ask {
    pub name: String,
    pub permissions: Vec<String>,
    pub hosts: Vec<String>,
    /// Wings put its compatibility script in front of its background worker (`extensions::Added::compat`).
    pub compat: bool,
}

impl Ask {
    /// The dialog's question and what it says under it: what it reaches first, then what else it asks for. The names come
    /// from the extension, so each is one line of printable text, cut short: a name with line breaks wrote a list of
    /// harmless permissions of its own over the real one.
    pub fn words(&self) -> (String, String) {
        let everywhere = self.hosts.iter().any(|host| every_website(host));
        let mut detail = Vec::new();
        if everywhere {
            detail.push("It can read and change everything on every website you open in the Browser.".to_owned());
        } else if !self.hosts.is_empty() {
            // The ones that reach many sites first, and never counted away under "and N more".
            let mut hosts: Vec<&String> = self.hosts.iter().collect();
            hosts.sort_by_key(|host| !broad(host));
            let broad_ones = hosts.iter().filter(|host| broad(host)).count();
            let shown_count = if broad_ones > HOSTS_SHOWN { broad_ones } else { HOSTS_SHOWN };
            let shown: Vec<String> = hosts.iter().take(shown_count).map(|host| one_line(host, 120)).collect();
            let more = self.hosts.len().saturating_sub(shown.len());
            let more = if more > 0 { format!(" and {more} more") } else { String::new() };
            detail.push(format!("It can read and change your data on: {}{more}.", shown.join(", ")));
        }
        if !self.permissions.is_empty() {
            let permissions: Vec<String> = self.permissions.iter().map(|permission| one_line(permission, 60)).collect();
            detail.push(format!("It asks for: {}.", permissions.join(", ")));
            // What the raw names that reach the most mean, one line each, in the order it asks for them.
            let meant: Vec<String> = self
                .permissions
                .iter()
                .filter_map(|permission| SENSITIVE.iter().find(|(name, _)| name == permission))
                .map(|(name, means)| format!("{name}: {means}"))
                .collect();
            if !meant.is_empty() {
                detail.push(meant.join("\n"));
            }
        }
        // Any site it reaches is reached signed in: its scripts on those pages, and its requests to them, carry the
        // cookies there, the ones imported from another browser too.
        if !self.hosts.is_empty() {
            detail.push("On those sites it acts as you, with the sign-ins you imported from another browser too.".into());
        }
        if detail.is_empty() {
            detail.push("It asks for no permissions.".into());
        }
        if self.compat {
            detail.push(COMPAT_ADDED.into());
        }
        (format!("Add the extension \"{}\" to the Browser?", one_line(&self.name, 60)), detail.join("\n\n"))
    }
}

/// What the dialog says of an extension Wings put its compatibility script in front of (`COMPAT`).
const COMPAT_ADDED: &str = "Wings adds a compatibility file so it runs in WebKit.";

/// What Wings runs first in an extension's own pages (`mac.rs`), and in its background worker from a file Wings adds to
/// its folder as it is added (`extensions::prepare`).
pub const COMPAT: &str = include_str!("compat.js");

/// The most host patterns the dialog names; the rest are counted.
const HOSTS_SHOWN: usize = 8;

/// The permissions whose names don't say what they reach, and what they do, as the dialog says it.
const SENSITIVE: [(&str, &str); 8] = [
    ("clipboardRead", "it can read what you copy"),
    ("webRequest", "it sees the requests the pages you open make"),
    ("tabs", "it sees the address and title of every page you open"),
    ("nativeMessaging", "it asks to talk to apps on this Mac, which Wings never allows"),
    ("scripting", "it can run its code in the pages it reaches"),
    ("cookies", "it can read and change the cookies of the sites it reaches, sign-ins included"),
    ("history", "it can read and delete the pages you visited"),
    ("downloads", "it can download files and see your downloads"),
];

/// `text` as one line of printable characters, at most `most` of them.
fn one_line(text: &str, most: usize) -> String {
    let line: String = text.chars().map(|c| if c.is_control() || c == '\u{2028}' || c == '\u{2029}' { ' ' } else { c }).collect();
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(most) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}

/// Whether a match pattern reaches more than one site: any wildcard in its host, like `*.example.com`.
fn broad(pattern: &str) -> bool {
    every_website(pattern) || pattern.split_once("://").map_or(pattern, |(_, rest)| rest).split('/').next().unwrap_or_default().contains('*')
}

/// Whether a match pattern reaches every website: `<all_urls>`, or a host of `*` whatever the scheme and path.
fn every_website(pattern: &str) -> bool {
    if pattern == "<all_urls>" {
        return true;
    }
    let host = pattern.split_once("://").map_or(pattern, |(_, rest)| rest).split('/').next().unwrap_or_default();
    host == "*" || host.starts_with("*:")
}

/// Whether a web view may go to `url`: the page itself only to an address a `WebView` may load (`web_address`), and a
/// frame in it also to a picture or a document the page made itself (`data:`, `blob:`), or to a page of an extension
/// (`webkit-extension:`), which WebKit loads only when the extension lists it for pages to load: Bitwarden's menu by a
/// password field is one. A link to `file:`, `javascript:` or another app's scheme is dropped, so a page can neither read
/// this Mac's files nor open other apps.
pub fn may_navigate(url: &str, main_frame: bool) -> bool {
    if crate::page::tree::web_address(url).is_some() {
        return true;
    }
    let scheme = url.split_once(':').map(|(scheme, _)| scheme.to_ascii_lowercase());
    !main_frame && matches!(scheme.as_deref(), Some("about" | "data" | "blob" | "webkit-extension"))
}

/// Why cookies can't go into a kept store on this Mac, if they can't: only a store of the plugin's own may keep them, and
/// WebKit has those from macOS 14 (`WKWebsiteDataStore(forIdentifier:)`). Before that the only kept store is the app's
/// default one, which any web view in Wings could read, so an import or a kept cookie never goes there.
pub fn keeps_refusal(keep_data: bool, per_plugin_stores: bool) -> Option<&'static str> {
    (keep_data && !per_plugin_stores).then_some(
        "Keeping cookies for one plugin alone needs macOS 14 or newer. Turn off Keep cookies to import them for this run of Wings only.",
    )
}

/// The layer of the window the places of the pages' web views are kept in (`ui::slots`).
pub const LAYER: &str = "page-webviews";

/// What a plugin's web pages are kept under: its id and install scope, so one removed and added again starts with no
/// cookies, and the folder its extensions are kept in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Profile {
    pub plugin: String,
    pub scope: String,
    /// The plugin's web folder: its extensions, apart from its storage.
    pub dir: std::path::PathBuf,
}

impl Profile {
    /// The profile of the plugin `plugin` installed as `scope`, whose storage is kept under `storage_root`.
    pub fn of(plugin: &str, scope: &str, storage_root: &std::path::Path) -> Self {
        let base = storage_root.parent().unwrap_or(storage_root);
        Self { plugin: plugin.into(), scope: scope.into(), dir: base.join("plugin-web").join(format!("{plugin}.{scope}")) }
    }

    /// The identifier of the profile's kept store: the same for the same plugin and scope in every run, as WebKit asks
    /// a UUID of a store it finds again (`WKWebsiteDataStore(forIdentifier:)`).
    pub fn uuid(&self) -> [u8; 16] {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(format!("wings-web\0{}\0{}", self.plugin, self.scope));
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        // A version 8 (custom) UUID of RFC 9562, so it is a valid one.
        bytes[6] = (bytes[6] & 0x0f) | 0x80;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        bytes
    }
    /// `uuid` as text, `8-4-4-4-12` hex digits: `NSUUID` is made from this, as its byte constructor's argument type
    /// differs between Foundation's classes and a debug build refuses the call.
    pub fn uuid_text(&self) -> String {
        let hex: String = self.uuid().iter().map(|byte| format!("{byte:02x}")).collect();
        format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_keeps_its_store_through_runs_and_another_scope_gets_its_own() {
        let profile = |scope: &str| Profile::of("browser", scope, std::path::Path::new("/data/plugin-storage"));
        assert_eq!(profile("a").uuid(), profile("a").uuid());
        assert_ne!(profile("a").uuid(), profile("b").uuid());
        let uuid = profile("a").uuid();
        assert_eq!((uuid[6] >> 4, uuid[8] >> 6), (8, 0b10), "a version 8 UUID");
        assert_eq!(profile("a").dir, std::path::Path::new("/data/plugin-web/browser.a"));
    }

    /// Opening a Browser tab in a debug build quit the app: `NSUUID::from_bytes` failed objc2's argument type check.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_profiles_uuid_makes_an_nsuuid_of_the_same_bytes() {
        let profile = Profile::of("browser", "a", std::path::Path::new("/data/plugin-storage"));
        let made = objc2_foundation::NSUUID::from_string(&objc2_foundation::NSString::from_str(&profile.uuid_text())).expect("a valid UUID");
        assert_eq!(made.UUIDString().to_string().to_lowercase(), profile.uuid_text());
    }

    #[test]
    fn the_dialog_before_an_extension_says_when_it_reaches_every_website() {
        let ask = |hosts: &[&str], permissions: &[&str]| Ask {
            name: "Blocker".into(),
            hosts: hosts.iter().map(|host| host.to_string()).collect(),
            permissions: permissions.iter().map(|permission| permission.to_string()).collect(),
            compat: false,
        };
        let (question, detail) = ask(&["<all_urls>"], &["storage", "tabs"]).words();
        assert_eq!(question, "Add the extension \"Blocker\" to the Browser?");
        assert!(detail.contains("every website") && detail.contains("storage, tabs"), "{detail}");
        let (_, some) = ask(&["https://github.com/*"], &[]).words();
        assert_eq!(some.split("\n\n").next(), Some("It can read and change your data on: https://github.com/*."));
        for everywhere in ["*://*/*", "http://*/x", "https://*:8080/*", "ws://*/", "<all_urls>"] {
            assert!(ask(&[everywhere], &[]).words().1.contains("every website"), "{everywhere}");
        }
        // Wildcard hosts come first and are never hidden in the count.
        let many: Vec<String> = (0..12).map(|at| format!("https://site{at}.com/*")).chain(["*://*.bank.example/*".to_owned(), "https://*.mail.example/*".to_owned()]).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let (_, listed) = ask(&many, &[]).words();
        assert!(listed.starts_with("It can read and change your data on: *://*.bank.example/*, https://*.mail.example/*,"), "{listed}");
        assert!(listed.split("\n\n").next().unwrap().ends_with("and 6 more."), "{listed}");
        // A name can't write lines of its own into the dialog.
        let spoof = Ask { name: "Helper\"\n\nIt asks for no permissions.\u{2028}x".repeat(5), hosts: vec!["<all_urls>".into()], permissions: vec![], compat: false };
        let (question, detail) = spoof.words();
        assert!(!question.contains('\n') && !question.contains('\u{2028}') && question.chars().count() <= 100, "{question}");
        assert!(detail.starts_with("It can read and change everything on every website"));
        assert_eq!(ask(&[], &[]).words().1, "It asks for no permissions.");
        assert!(ask(&["https://github.com/*"], &["cookies"]).words().1.contains("sign-ins you imported"));
        assert!(!ask(&[], &["cookies"]).words().1.contains("sign-ins you imported"));
    }

    /// Any site it reaches is reached with the imported sign-ins, cookies permission or not: its content scripts and its
    /// requests carry them.
    #[test]
    fn the_dialog_says_the_imported_sign_ins_reach_any_site_it_reaches() {
        let ask = |hosts: &[&str], permissions: &[&str]| Ask {
            name: "Blocker".into(),
            hosts: hosts.iter().map(|host| host.to_string()).collect(),
            permissions: permissions.iter().map(|permission| permission.to_string()).collect(),
            compat: false,
        };
        for hosts in [&["https://github.com/*"][..], &["<all_urls>"], &["*://*.bank.example/*"]] {
            let (_, detail) = ask(hosts, &["storage"]).words();
            assert!(detail.contains("with the sign-ins you imported"), "{hosts:?}: {detail}");
        }
        assert!(!ask(&[], &["storage"]).words().1.contains("sign-ins"));
    }

    /// Only an extension Wings put its compatibility script in front of says so, on a line of its own, last.
    #[test]
    fn the_dialog_says_when_wings_adds_its_compatibility_file() {
        let ask = |compat| Ask { name: "Bitwarden".into(), hosts: vec!["https://*/*".into()], permissions: vec!["storage".into()], compat };
        assert!(ask(true).words().1.ends_with("\n\nWings adds a compatibility file so it runs in WebKit."), "{}", ask(true).words().1);
        assert!(!ask(false).words().1.contains("compatibility"));
    }

    /// The raw names that reach the most each say what they do, on a line of their own.
    #[test]
    fn the_dialog_says_what_sensitive_permissions_do() {
        let ask = Ask { name: "Helper".into(), hosts: vec![], permissions: ["storage", "tabs", "nativeMessaging", "clipboardRead"].map(String::from).to_vec(), compat: false };
        let (_, detail) = ask.words();
        assert_eq!(
            detail,
            "It asks for: storage, tabs, nativeMessaging, clipboardRead.\n\n\
             tabs: it sees the address and title of every page you open\n\
             nativeMessaging: it asks to talk to apps on this Mac, which Wings never allows\n\
             clipboardRead: it can read what you copy"
        );
        for (name, _) in SENSITIVE {
            let one = Ask { name: "x".into(), hosts: vec![], permissions: vec![name.into()], compat: false };
            assert!(one.words().1.contains(&format!("\n\n{name}: ")), "{name}");
        }
    }

    #[test]
    fn an_extension_counts_as_allowed_only_on_the_dialogs_add() {
        assert_eq!(allowed(&Ok(r#"{"allowed":true,"error":null}"#.into())), Some(None));
        assert_eq!(allowed(&Ok(r#"{"allowed":true,"error":"bad manifest"}"#.into())), Some(Some("bad manifest".into())));
        for other in [Ok("null".into()), Ok(r#"{"allowed":"yes"}"#.into()), Err(NOT_ALLOWED.into()), Err("The Wings window is closed or busy".into())] {
            assert_eq!(allowed(&other), None, "{other:?}");
        }
    }

    #[test]
    fn every_command_that_reads_or_acts_on_a_page_needs_scripts_and_a_page_without_imported_cookies() {
        use crate::page::tree::WebGo;
        let moves = [WebGo::Load("https://a.com/".into()), WebGo::Back, WebGo::Forward, WebGo::Reload, WebGo::Stop];
        let reads = [
            WebGo::Eval { script: "1".into(), page: false },
            WebGo::Eval { script: "1".into(), page: true },
            WebGo::Snapshot,
            WebGo::Logs,
            WebGo::Dialog { accept: true, text: None },
        ];
        for go in &moves {
            assert_eq!(gate(go, false, true, false), Ok(()), "{go:?} moves about");
        }
        for go in &reads {
            assert_eq!(gate(go, false, false, true), Err(NO_SCRIPTS), "{go:?}");
            assert_eq!(gate(go, true, true, true), Err(IMPORTED), "{go:?}");
            assert_eq!(gate(go, true, false, false), Err(LOADING), "{go:?} while another page is on its way");
            assert_eq!(gate(go, true, false, true), Ok(()), "{go:?}");
        }
        for go in moves.iter().chain(&reads) {
            assert!(!needs_click(go), "{go:?}");
        }
    }

    /// An extension's button reads nothing of the page for the plugin, so it opens on any page, a signed-in one too, but
    /// only on a click of yours.
    #[test]
    fn an_extensions_button_needs_a_click_and_reads_nothing_of_the_page() {
        use crate::page::tree::WebGo;
        let button = WebGo::Extension("e1".into());
        assert!(needs_click(&button));
        assert_eq!(gate(&button, false, true, false), Ok(()));
    }

    #[test]
    fn a_page_goes_only_to_web_addresses_and_its_frames_also_to_what_it_made() {
        for url in ["https://example.com/", "http://localhost:3000/", "about:blank"] {
            assert!(may_navigate(url, true), "{url}");
        }
        for url in ["file:///etc/passwd", "javascript:alert(1)", "slack://open", "x-apple.systempreferences:", "data:text/html,hi", "about:srcdoc"] {
            assert!(!may_navigate(url, true), "{url} as the page");
        }
        assert!(may_navigate("data:text/html,hi", false) && may_navigate("blob:https://a.com/1", false) && may_navigate("about:srcdoc", false));
        assert!(!may_navigate("file:///etc/passwd", false) && !may_navigate("slack://open", false));
        // An extension's page in a frame, as Bitwarden's menu by a password field is, but never as the page.
        let menu = "webkit-extension://0cc71a78-376b-4518-bfa8-2713243fd8e7/overlay/menu-button.html";
        assert!(may_navigate(menu, false) && !may_navigate(menu, true));
    }

    #[test]
    fn a_site_signed_in_with_imported_cookies_is_known_by_its_domain() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::of("browser", "s", &dir.path().join("plugin-storage"));
        assert!(!holds_imported(&profile.dir, "https://github.com/"));
        note_imported(&profile.dir, [".github.com".to_owned(), "Example.org".to_owned()]).unwrap();
        for url in ["https://github.com/", "https://gist.github.com/x", "http://user@example.org:8080/a"] {
            assert!(holds_imported(&profile.dir, url), "{url}");
        }
        for url in ["https://notgithub.com/", "https://github.com.evil.dev/", "about:blank"] {
            assert!(!holds_imported(&profile.dir, url), "{url}");
        }
        // A frame with no host of its own may be on any site.
        assert!(holds_imported_host(&profile.dir, "") && holds_imported_host(&profile.dir, "GIST.github.com"));
        assert!(!holds_imported_host(&profile.dir, "example.com"));
        // A trailing dot is the same site.
        assert!(holds_imported(&profile.dir, "https://github.com./x") && holds_imported_host(&profile.dir, "Example.org."));
        // A list it can't read names every site, and no import goes in past it.
        std::fs::write(profile.dir.join("imported-sites.json"), b"{not json").unwrap();
        IMPORTED_SITES.with_borrow_mut(|cache| cache.clear());
        assert!(holds_imported(&profile.dir, "https://anything.example/"));
        assert!(note_imported(&profile.dir, ["a.com".to_owned()]).is_err());
    }

    #[test]
    fn a_sign_in_that_passes_through_an_imported_site_taints_where_it_lands() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::of("browser", "s", &dir.path().join("plugin-storage"));
        note_imported(&profile.dir, ["github.com".to_owned()]).unwrap();
        // From a plain site to GitHub's sign-in, and from there back: both carry it.
        assert!(carries_imported(&profile.dir, "https://app.example/", "https://github.com/login/oauth"));
        assert!(carries_imported(&profile.dir, "https://github.com/login/oauth", "https://app.example/callback?code=1"));
        assert!(!carries_imported(&profile.dir, "https://app.example/", "https://app.example/next"));
        // Where it landed counts from then on.
        note_imported(&profile.dir, ["app.example".to_owned()]).unwrap();
        assert!(holds_imported(&profile.dir, "https://app.example/inbox"));
    }

    #[test]
    fn a_page_moving_within_itself_keeps_its_origin() {
        assert_eq!(origin("https://App.example.com:8443/a?b#c"), Some("https://app.example.com:8443".into()));
        assert_eq!(origin("https://app.example.com/inbox"), origin("https://app.example.com/inbox/42"));
        assert_ne!(origin("https://app.example.com/"), origin("https://evil.example/"));
        assert_eq!(origin("about:blank"), None);
    }

    #[test]
    fn cookies_are_kept_only_in_a_store_of_the_plugins_own() {
        assert!(keeps_refusal(true, false).is_some_and(|why| why.contains("macOS 14")));
        assert_eq!(keeps_refusal(false, false), None, "a store kept only while Wings runs is the plugin's own");
        assert_eq!(keeps_refusal(true, true), None);
    }
}
