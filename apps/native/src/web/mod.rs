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
pub use views::{Heard, Spec, State, Told, forget_page, slot, sync, update};
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
    /// The extension `id` in `dir` loads into the profile's web views, or unloads.
    Load { profile: Profile, id: String, dir: std::path::PathBuf },
    /// `Load`, for an extension being added: Wings asks the person first, in its own dialog, with what it can do.
    Add { profile: Profile, id: String, dir: std::path::PathBuf, ask: Ask },
    Unload { profile: Profile, id: String },
    /// Another browser's cookies go into the profile's store, kept or not: how many it took.
    Cookies { profile: Profile, keep_data: bool, cookies: Vec<wings_browser_cookies::Cookie> },
}

/// The one gate every command a plugin's page sends its web view goes through, here and in WebKit (`views::update`,
/// `mac::WebPage::go`): moving about (load, back, forward, reload, stop) always goes; anything that reads or acts on what
/// the page holds (a script, its picture, its logs, the answer to its dialog) needs the plugin's `webScripts`
/// (`scripts`) and never touches a page signed in with imported cookies (`imported`). Each command is named here, so a
/// new one has to say which it is.
pub fn gate(go: &crate::page::tree::WebGo, scripts: bool, imported: bool) -> Result<(), &'static str> {
    use crate::page::tree::WebGo;
    match go {
        WebGo::Load(_) | WebGo::Back | WebGo::Forward | WebGo::Reload | WebGo::Stop => Ok(()),
        WebGo::Eval { .. } | WebGo::Snapshot | WebGo::Logs | WebGo::Dialog { .. } => match (scripts, imported) {
            (false, _) => Err(NO_SCRIPTS),
            (true, true) => Err(IMPORTED),
            (true, false) => Ok(()),
        },
    }
}

/// Why a command that reads or acts on a page didn't run, for a plugin that may not run scripts in its pages.
pub const NO_SCRIPTS: &str = "This plugin may not read or act on the web pages it shows (permissions.webScripts)";
/// Why one didn't run on a page signed in with imported cookies.
pub const IMPORTED: &str = "This page is signed in with cookies you imported, so no plugin reads or acts on it";

/// Notes the sites whose cookies are about to be imported into `profile`, by their domains (never the cookies), so a
/// plugin's script never runs in the page's own world there (`holds_imported`). Written before the cookies go in, and
/// an import fails when it can't be: a cookie never goes where the note doesn't name it.
pub fn note_imported(profile: &Profile, domains: impl IntoIterator<Item = String>) -> Result<(), String> {
    let file = profile.dir.join("imported-sites.json");
    let mut known = match std::fs::read(&file) {
        Ok(bytes) => serde_json::from_slice::<std::collections::BTreeSet<String>>(&bytes).map_err(|error| format!("The list of imported sites is unreadable: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(error) => return Err(format!("The list of imported sites is unreadable: {error}")),
    };
    known.extend(domains.into_iter().map(|domain| domain.trim_start_matches('.').to_ascii_lowercase()));
    std::fs::create_dir_all(&profile.dir).map_err(|error| error.to_string())?;
    let json = serde_json::to_vec(&known).map_err(|error| error.to_string())?;
    // Written whole beside it and moved over, so a crash never leaves half a list.
    let next = profile.dir.join("imported-sites.json.next");
    std::fs::write(&next, json).and_then(|()| std::fs::rename(&next, &file)).map_err(|error| format!("Couldn't note the imported sites: {error}"))
}

/// Whether the page at `url` is on a site whose cookies were imported into `profile`: one signed in with them. A list
/// that is there but can't be read counts as naming every site; none at all means nothing was ever imported, as it is
/// written before any cookie goes in.
pub fn holds_imported(profile_dir: &std::path::Path, url: &str) -> bool {
    let Some(host) = url.split_once("://").and_then(|(_, rest)| rest.split(['/', '?', '#']).next()) else { return false };
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    let host = host.split(':').next().unwrap_or_default().to_ascii_lowercase();
    match imported_sites(profile_dir) {
        Some(known) => known.iter().any(|domain| host == *domain || host.ends_with(&format!(".{domain}"))),
        None => true,
    }
}

/// The domains of the sites whose cookies were imported into the profile in `profile_dir`: none when nothing was, and
/// `None` when the list is there but can't be read, which counts as every site.
pub fn imported_sites(profile_dir: &std::path::Path) -> Option<Vec<String>> {
    match std::fs::read(profile_dir.join("imported-sites.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Vec::new()),
        Err(_) => None,
    }
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
        }
        if detail.is_empty() {
            detail.push("It asks for no permissions.".into());
        }
        (format!("Add the extension \"{}\" to the Browser?", one_line(&self.name, 60)), detail.join("\n\n"))
    }
}

/// The most host patterns the dialog names; the rest are counted.
const HOSTS_SHOWN: usize = 8;

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
/// frame in it also to a picture or a document the page made itself (`data:`, `blob:`). A link to `file:`, `javascript:`
/// or another app's scheme is dropped, so a page can neither read this Mac's files nor open other apps.
pub fn may_navigate(url: &str, main_frame: bool) -> bool {
    if crate::page::tree::web_address(url).is_some() {
        return true;
    }
    let scheme = url.split_once(':').map(|(scheme, _)| scheme.to_ascii_lowercase());
    !main_frame && matches!(scheme.as_deref(), Some("about" | "data" | "blob"))
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

    #[test]
    fn the_dialog_before_an_extension_says_when_it_reaches_every_website() {
        let ask = |hosts: &[&str], permissions: &[&str]| Ask {
            name: "Blocker".into(),
            hosts: hosts.iter().map(|host| host.to_string()).collect(),
            permissions: permissions.iter().map(|permission| permission.to_string()).collect(),
        };
        let (question, detail) = ask(&["<all_urls>"], &["storage", "tabs"]).words();
        assert_eq!(question, "Add the extension \"Blocker\" to the Browser?");
        assert!(detail.contains("every website") && detail.contains("storage, tabs"), "{detail}");
        let (_, some) = ask(&["https://github.com/*"], &[]).words();
        assert_eq!(some, "It can read and change your data on: https://github.com/*.");
        for everywhere in ["*://*/*", "http://*/x", "https://*:8080/*", "ws://*/", "<all_urls>"] {
            assert!(ask(&[everywhere], &[]).words().1.contains("every website"), "{everywhere}");
        }
        // Wildcard hosts come first and are never hidden in the count.
        let many: Vec<String> = (0..12).map(|at| format!("https://site{at}.com/*")).chain(["*://*.bank.example/*".to_owned(), "https://*.mail.example/*".to_owned()]).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let (_, listed) = ask(&many, &[]).words();
        assert!(listed.starts_with("It can read and change your data on: *://*.bank.example/*, https://*.mail.example/*,"), "{listed}");
        assert!(listed.ends_with("and 6 more."), "{listed}");
        // A name can't write lines of its own into the dialog.
        let spoof = Ask { name: "Helper\"\n\nIt asks for no permissions.\u{2028}x".repeat(5), hosts: vec!["<all_urls>".into()], permissions: vec![] };
        let (question, detail) = spoof.words();
        assert!(!question.contains('\n') && !question.contains('\u{2028}') && question.chars().count() <= 100, "{question}");
        assert!(detail.starts_with("It can read and change everything on every website"));
        assert_eq!(ask(&[], &[]).words().1, "It asks for no permissions.");
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
            assert_eq!(gate(go, false, true), Ok(()), "{go:?} moves about");
        }
        for go in &reads {
            assert_eq!(gate(go, false, false), Err(NO_SCRIPTS), "{go:?}");
            assert_eq!(gate(go, true, true), Err(IMPORTED), "{go:?}");
            assert_eq!(gate(go, true, false), Ok(()), "{go:?}");
        }
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
    }

    #[test]
    fn a_site_signed_in_with_imported_cookies_is_known_by_its_domain() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::of("browser", "s", &dir.path().join("plugin-storage"));
        assert!(!holds_imported(&profile.dir, "https://github.com/"));
        note_imported(&profile, [".github.com".to_owned(), "Example.org".to_owned()]).unwrap();
        for url in ["https://github.com/", "https://gist.github.com/x", "http://user@example.org:8080/a"] {
            assert!(holds_imported(&profile.dir, url), "{url}");
        }
        for url in ["https://notgithub.com/", "https://github.com.evil.dev/", "about:blank"] {
            assert!(!holds_imported(&profile.dir, url), "{url}");
        }
        // A list it can't read names every site, and no import goes in past it.
        std::fs::write(profile.dir.join("imported-sites.json"), b"{not json").unwrap();
        assert!(holds_imported(&profile.dir, "https://anything.example/"));
        assert!(note_imported(&profile, ["a.com".to_owned()]).is_err());
    }

    #[test]
    fn cookies_are_kept_only_in_a_store_of_the_plugins_own() {
        assert!(keeps_refusal(true, false).is_some_and(|why| why.contains("macOS 14")));
        assert_eq!(keeps_refusal(false, false), None, "a store kept only while Wings runs is the plugin's own");
        assert_eq!(keeps_refusal(true, true), None);
    }
}
