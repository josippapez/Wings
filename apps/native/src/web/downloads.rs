//! Files a page's web views download, for a view whose node hears them (`onDownload`): each goes into ~/Downloads under a
//! name no file there has, is marked as coming from the web (the quarantine Finder and Gatekeeper read before anything in
//! it runs), and is never opened for the page. The page hears how each goes (`Report`), and opens one in Finder or its
//! app only on your click (`act`, `web::needs_click`).

use std::path::{Path, PathBuf};

use serde::Serialize;

/// What a page hears of one download (`Heard::Download`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: u64,
    pub url: String,
    /// The file's name in the folder, and its whole path: empty for one that was blocked.
    pub name: String,
    pub path: String,
    /// `progress`, `done`, `failed`, `canceled` or `blocked`.
    pub state: &'static str,
    pub received: u64,
    /// None while the server hasn't said how big it is.
    pub total: Option<u64>,
    pub error: Option<String>,
}

/// Why a download didn't start: the page had already started one by itself, or as many as a document may.
pub const BLOCKED: &str = "The page tried to download more files by itself. Click its link to download it.";

/// The most bytes of a file's name, which macOS allows.
const NAME_MOST: usize = 255;

/// Characters that change the order text shows in, which a name could use to show "invoice.pdf" for an app's
/// "invoice\u{202E}fdp.app".
const REORDERING: [char; 12] = ['\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'];

/// The name a download is saved under, from the one the site or the engine suggests, or else the last part of its address:
/// one plain name, never a path into another folder, a hidden file, or a name whose characters reorder how it reads, cut
/// to 255 bytes with its extension kept. "download" when neither has one.
pub fn file_name(suggested: &str, url: &str) -> String {
    let from_url = url.split(['?', '#']).next().unwrap_or_default().rsplit('/').next().unwrap_or_default();
    for candidate in [suggested, from_url] {
        let last = candidate.rsplit(['/', '\\']).next().unwrap_or_default();
        let plain: String = last
            .chars()
            .filter(|c| !c.is_control() && !REORDERING.contains(c))
            .map(|c| if c == ':' { '-' } else { c })
            .collect();
        let plain = plain.trim().trim_start_matches('.').trim_end_matches(['.', ' ']).trim();
        if !plain.is_empty() {
            return fit(plain);
        }
    }
    "download".into()
}

/// `name` cut to `NAME_MOST` bytes at a character's edge, keeping a short extension.
fn fit(name: &str) -> String {
    if name.len() <= NAME_MOST {
        return name.to_owned();
    }
    let (stem, extension) = split_extension(name);
    let room = NAME_MOST.saturating_sub(extension.len());
    let mut end = room.min(stem.len());
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{extension}", &stem[..end])
}

/// A name's stem and its extension with the dot (`report`, `.pdf`), the extension empty for none or one too long to be one.
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(at) if at > 0 && name.len() - at <= 16 => (&name[..at], &name[at..]),
        _ => (name, ""),
    }
}

/// `name` in `folder`, else `name (1)`, `name (2)`… before its extension: the first no file has (a link that points
/// nowhere counts as one) and `taken` doesn't hold, which is a download under way that may not have written its file yet.
/// A file is never written over.
pub fn unique_path(folder: &Path, name: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let (stem, extension) = split_extension(name);
    let free = |path: &Path| !taken(path) && std::fs::symlink_metadata(path).is_err();
    for count in 0..10_000u32 {
        let path = folder.join(if count == 0 { name.to_owned() } else { fit(&format!("{stem} ({count}){extension}")) });
        if free(&path) {
            return path;
        }
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    folder.join(fit(&format!("{stem} {stamp}{extension}")))
}

/// The most downloads one document may start, clicked or not: more is a page spamming files.
const MOST_PER_DOCUMENT: u32 = 10;

/// What downloads a view's document may start: one it starts by itself (no click or key of yours led to it), as Chrome
/// lets the first through and asks before more, and at most `MOST_PER_DOCUMENT` in all. The one by itself is the view's,
/// not each document's, so a page that loads itself again doesn't get another: a download you start gives it one again.
#[derive(Debug, Default)]
pub struct Gate {
    document: u64,
    automatic: u32,
    all: u32,
}

impl Gate {
    /// Whether a download the view's document `document` (its count of new documents) starts may go, `user` when your
    /// click or key started it.
    pub fn allow(&mut self, document: u64, user: bool) -> bool {
        if self.document != document {
            *self = Self { document, all: 0, ..*self };
        }
        if self.all >= MOST_PER_DOCUMENT || (!user && self.automatic >= 1) {
            return false;
        }
        self.all += 1;
        self.automatic = if user { 0 } else { self.automatic + 1 };
        true
    }
}

/// How often a page hears a download's progress: its state changes (done, failed) always come at once, its bytes at most
/// this often, so a fast download doesn't draw the page again for every chunk.
pub const PROGRESS_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

/// The folder downloads go into: Downloads in the home folder.
pub fn folder() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Downloads"))
}

#[cfg(feature = "ui")]
pub use registry::{act, begin, blocked, canceled, failed, finished, progress};

#[cfg(feature = "ui")]
mod registry {
    //! The downloads of this run, by their id: where each goes, how it went, whose page hears of it, and how to stop it.

    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
        path::PathBuf,
        rc::Rc,
        time::Instant,
    };

    use super::{Report, folder, unique_path};
    use crate::{
        page::tree::WebDownload,
        web::{
            Profile,
            views::{Heard, Tell},
        },
    };

    struct Entry {
        profile: Profile,
        tell: Tell,
        report: Report,
        /// The page that started it, for the quarantine's record of where it came from.
        page: String,
        told_at: Option<Instant>,
        cancel: Option<Rc<dyn Fn()>>,
    }

    /// The most downloads kept for `act`; the oldest that ended go first.
    const KEPT: usize = 200;

    thread_local! {
        static DOWNLOADS: RefCell<BTreeMap<u64, Entry>> = RefCell::default();
        static NEXT: Cell<u64> = const { Cell::new(1) };
    }

    fn next_id() -> u64 {
        NEXT.with(|next| next.replace(next.get() + 1))
    }

    fn keep(id: u64, entry: Entry) {
        DOWNLOADS.with_borrow_mut(|all| {
            all.insert(id, entry);
            while all.len() > KEPT {
                let Some(old) = all.iter().find(|(_, entry)| entry.report.state != "progress").map(|(id, _)| *id) else { break };
                all.remove(&old);
            }
        });
    }

    /// Tells the download's page where it is now, unless it is only more bytes told less than `PROGRESS_EVERY` ago.
    fn tell(id: u64, change: impl FnOnce(&mut Report)) {
        let told = DOWNLOADS.with_borrow_mut(|all| {
            let entry = all.get_mut(&id)?;
            let was = entry.report.state;
            change(&mut entry.report);
            let now = Instant::now();
            let due = entry.report.state != was || entry.report.state != "progress" || entry.told_at.is_none_or(|at| now - at >= super::PROGRESS_EVERY);
            due.then(|| {
                entry.told_at = Some(now);
                (entry.tell.clone(), entry.report.clone())
            })
        });
        if let Some((tell, report)) = told {
            tell.tell(Heard::Download(report));
        }
    }

    /// A download the view of `tell` starts from `url`, on the page at `page`, which the engine suggests naming
    /// `suggested`: its id and the path to write it to, which no file has, in ~/Downloads (made if it isn't there).
    /// `cancel` stops it. None when there is no home folder to put it in.
    pub fn begin(tell: &Tell, profile: &Profile, url: &str, page: &str, suggested: &str, cancel: Rc<dyn Fn()>) -> Option<(u64, PathBuf)> {
        let folder = folder()?;
        std::fs::create_dir_all(&folder).ok()?;
        let name = super::file_name(suggested, url);
        let path = DOWNLOADS.with_borrow(|all| {
            unique_path(&folder, &name, |path| all.values().any(|entry| entry.report.state == "progress" && std::path::Path::new(&entry.report.path) == path))
        });
        let id = next_id();
        let report = Report {
            id,
            url: url.to_owned(),
            name: path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            path: path.to_string_lossy().into_owned(),
            state: "progress",
            received: 0,
            total: None,
            error: None,
        };
        keep(id, Entry { profile: profile.clone(), tell: tell.clone(), report, page: page.to_owned(), told_at: None, cancel: Some(cancel) });
        tell_now(id);
        Some((id, path))
    }

    fn tell_now(id: u64) {
        let told = DOWNLOADS.with_borrow_mut(|all| {
            let entry = all.get_mut(&id)?;
            entry.told_at = Some(Instant::now());
            Some((entry.tell.clone(), entry.report.clone()))
        });
        if let Some((tell, report)) = told {
            tell.tell(Heard::Download(report));
        }
    }

    /// A download the page started that didn't go (`Gate`): the page hears it was blocked.
    pub fn blocked(tell: &Tell, url: &str, suggested: &str) {
        let report = Report {
            id: next_id(),
            url: url.to_owned(),
            name: super::file_name(suggested, url),
            path: String::new(),
            state: "blocked",
            received: 0,
            total: None,
            error: Some(super::BLOCKED.into()),
        };
        tell.tell(Heard::Download(report));
    }

    /// `received` bytes of `total` (none while unknown) are written.
    pub fn progress(id: u64, received: u64, total: Option<u64>) {
        tell(id, |report| {
            report.received = received;
            report.total = total.or(report.total);
        });
    }

    /// It is all written, at `path` when the engine put it somewhere else than it was told: it is marked as from the web.
    pub fn finished(id: u64, path: Option<PathBuf>) {
        let marked = DOWNLOADS.with_borrow_mut(|all| {
            let entry = all.get_mut(&id)?;
            if let Some(path) = path {
                entry.report.name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                entry.report.path = path.to_string_lossy().into_owned();
            }
            entry.cancel = None;
            Some((PathBuf::from(&entry.report.path), entry.report.url.clone(), entry.page.clone()))
        });
        let Some((path, url, page)) = marked else { return };
        // Unmarked, macOS wouldn't check it before it opens: it doesn't stay.
        if let Err(why) = quarantine(&path, &url, &page) {
            eprintln!("[downloads] couldn't mark a download as from the web: {why}");
            return ended(id, "failed", Some(format!("Wings couldn't mark the file as downloaded from the web, so it was removed ({why})")));
        }
        tell(id, |report| {
            report.state = "done";
            report.total = Some(report.total.unwrap_or(report.received).max(report.received));
            report.received = report.total.unwrap_or(report.received);
        });
    }

    /// It stopped with `why`: what it wrote goes.
    pub fn failed(id: u64, why: &str) {
        ended(id, "failed", Some(why.to_owned()));
    }

    /// It was stopped, by you or the page going: what it wrote goes.
    pub fn canceled(id: u64) {
        ended(id, "canceled", None);
    }

    fn ended(id: u64, state: &'static str, error: Option<String>) {
        let path = DOWNLOADS.with_borrow_mut(|all| {
            let entry = all.get_mut(&id).filter(|entry| entry.report.state == "progress")?;
            entry.cancel = None;
            Some(PathBuf::from(&entry.report.path))
        });
        let Some(path) = path else { return };
        // Only a path chosen for it as one no file had (`unique_path`), so nothing else is removed.
        let _ = std::fs::remove_file(&path);
        tell(id, |report| {
            report.state = state;
            report.error = error;
        });
    }

    /// Does `act` to the download `id` of `profile`'s pages: shows it in Finder, opens it in its app once it is done, or
    /// stops it while it goes.
    pub fn act(profile: &Profile, id: u64, act: WebDownload) -> Result<serde_json::Value, String> {
        let (path, state, cancel) = DOWNLOADS
            .with_borrow(|all| {
                let entry = all.get(&id).filter(|entry| entry.profile == *profile)?;
                Some((PathBuf::from(&entry.report.path), entry.report.state, entry.cancel.clone()))
            })
            .ok_or("There is no such download")?;
        match act {
            WebDownload::Cancel => {
                let cancel = cancel.filter(|_| state == "progress").ok_or("It isn't downloading")?;
                cancel();
                canceled(id);
            }
            WebDownload::Reveal | WebDownload::Open => {
                if act == WebDownload::Open && state != "done" {
                    return Err("It hasn't finished downloading".into());
                }
                if path.as_os_str().is_empty() || std::fs::symlink_metadata(&path).is_err() {
                    return Err("The file isn't in Downloads any more".into());
                }
                show(&path, act == WebDownload::Open)?;
            }
        }
        Ok(serde_json::Value::Null)
    }

    /// Marks the file at `path` as downloaded from `url` on the page at `page` (`NSURLQuarantinePropertiesKey`), as a
    /// browser does: Gatekeeper then checks an app or a script in it before it runs, and Finder says where it came from.
    #[cfg(target_os = "macos")]
    pub(super) fn quarantine(path: &std::path::Path, url: &str, page: &str) -> Result<(), String> {
        use objc2::{Message, rc::Retained, runtime::AnyObject};
        use objc2_core_foundation::CFString;
        use objc2_core_services::{kLSQuarantineAgentNameKey, kLSQuarantineDataURLKey, kLSQuarantineOriginURLKey, kLSQuarantineTypeKey, kLSQuarantineTypeWebDownload};
        use objc2_foundation::{NSDictionary, NSString, NSURL, NSURLQuarantinePropertiesKey};

        // A CFString is an NSString (toll-free bridged).
        let ns = |text: &CFString| -> &NSString { unsafe { &*std::ptr::from_ref(text).cast::<NSString>() } };
        let web = |url: &str| (url.starts_with("http://") || url.starts_with("https://")).then(|| NSURL::URLWithString(&NSString::from_str(url))).flatten();
        // SAFETY: LaunchServices' constant keys, which live as long as the process.
        let (agent_key, type_key, web_download, data_key, origin_key) =
            unsafe { (ns(kLSQuarantineAgentNameKey), ns(kLSQuarantineTypeKey), ns(kLSQuarantineTypeWebDownload), ns(kLSQuarantineDataURLKey), ns(kLSQuarantineOriginURLKey)) };
        let mut keys = vec![agent_key, type_key];
        let mut values: Vec<Retained<AnyObject>> = vec![Retained::into_super(Retained::into_super(NSString::from_str("Wings"))), Retained::into_super(Retained::into_super(web_download.retain()))];
        if let Some(data) = web(url) {
            keys.push(data_key);
            values.push(Retained::into_super(Retained::into_super(data)));
        }
        if let Some(origin) = web(page) {
            keys.push(origin_key);
            values.push(Retained::into_super(Retained::into_super(origin)));
        }
        let values: Vec<&AnyObject> = values.iter().map(|value| &**value).collect();
        let properties = NSDictionary::<NSString, AnyObject>::from_slices(&keys, &values);
        let file = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        // SAFETY: a file URL, and the dictionary of quarantine properties the key takes.
        unsafe { file.setResourceValue_forKey_error(Some(&properties), NSURLQuarantinePropertiesKey) }.map_err(|error| error.localizedDescription().to_string())
    }

    #[cfg(not(target_os = "macos"))]
    fn quarantine(_path: &std::path::Path, _url: &str, _page: &str) -> Result<(), String> {
        Ok(())
    }

    /// Shows the file at `path` in Finder, or opens it in its app.
    #[cfg(target_os = "macos")]
    fn show(path: &std::path::Path, open: bool) -> Result<(), String> {
        use objc2_app_kit::NSWorkspace;
        use objc2_foundation::{NSArray, NSString, NSURL};
        let file = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let workspace = NSWorkspace::sharedWorkspace();
        if open {
            return if workspace.openURL(&file) { Ok(()) } else { Err("macOS couldn't open it".into()) };
        }
        workspace.activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[file]));
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn show(_path: &std::path::Path, _open: bool) -> Result<(), String> {
        Err("Only on macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A site names the file: only the name stays, never a folder, a hidden file or characters that hide its extension.
    #[test]
    fn a_downloads_name_is_one_plain_name_whatever_the_site_says() {
        assert_eq!(file_name("report.pdf", "https://a.com/x"), "report.pdf");
        assert_eq!(file_name("../../.ssh/authorized_keys", "https://a.com/x"), "authorized_keys");
        assert_eq!(file_name("..\\..\\evil.bat", ""), "evil.bat");
        assert_eq!(file_name(".zshrc", ""), "zshrc", "never a hidden file");
        assert_eq!(file_name("invoice\u{202E}fdp.app", ""), "invoicefdp.app", "nothing that reorders how it reads");
        assert_eq!(file_name("a\nb\u{0}c:d.txt", ""), "abc-d.txt");
        assert_eq!(file_name("  ... ", "https://a.com/files/data.csv?x=1#y"), "data.csv", "from the address when it names none");
        assert_eq!(file_name("", "https://a.com/"), "download");
        let long = format!("{}.pdf", "é".repeat(300));
        let cut = file_name(&long, "");
        assert!(cut.len() <= 255 && cut.ends_with(".pdf"), "{}", cut.len());
    }

    /// Never over a file that is there, a link included, nor over one a download under way will write.
    #[test]
    fn a_download_never_takes_a_name_a_file_or_another_download_has() {
        let dir = tempfile::tempdir().unwrap();
        let none = |_: &Path| false;
        assert_eq!(unique_path(dir.path(), "a.pdf", none), dir.path().join("a.pdf"));
        std::fs::write(dir.path().join("a.pdf"), b"mine").unwrap();
        assert_eq!(unique_path(dir.path(), "a.pdf", none), dir.path().join("a (1).pdf"));
        std::os::unix::fs::symlink(dir.path().join("nowhere"), dir.path().join("a (1).pdf")).unwrap();
        let under_way = dir.path().join("a (2).pdf");
        assert_eq!(unique_path(dir.path(), "a.pdf", |path| path == under_way), dir.path().join("a (3).pdf"));
        assert_eq!(std::fs::read(dir.path().join("a.pdf")).unwrap(), b"mine");
        std::fs::write(dir.path().join("archive"), b"").unwrap();
        assert_eq!(unique_path(dir.path(), "archive", none), dir.path().join("archive (1)"), "a name with no extension");
    }

    /// A finished download is marked as from the web (the quarantine Gatekeeper reads before anything in it runs), by Wings,
    /// as LaunchServices reads the mark back.
    #[cfg(all(feature = "ui", target_os = "macos"))]
    #[test]
    fn a_finished_download_is_marked_as_from_the_web_by_wings() {
        use objc2::runtime::AnyObject;
        use objc2_foundation::{NSDictionary, NSString, NSURL, NSURLQuarantinePropertiesKey};
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("report.pdf");
        std::fs::write(&file, b"%PDF").unwrap();
        let mark = || {
            let out = std::process::Command::new("xattr").args(["-p", "com.apple.quarantine"]).arg(&file).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        assert_eq!(mark(), "", "none before");
        registry::quarantine(&file, "https://app.example/files/report.pdf", "https://app.example/reports").unwrap();
        assert!(!mark().is_empty(), "the quarantine is on the file");
        let url = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
        let mut value: Option<objc2::rc::Retained<AnyObject>> = None;
        // SAFETY: the file's quarantine properties, a dictionary when it has them.
        unsafe { url.getResourceValue_forKey_error(&mut value, NSURLQuarantinePropertiesKey) }.unwrap();
        let properties = value.and_then(|value| value.downcast::<NSDictionary>().ok()).expect("its quarantine properties");
        // SAFETY: a description is always an NSString.
        let said = unsafe { objc2::rc::Retained::cast_unchecked::<NSString>(objc2::runtime::NSObjectProtocol::description(&*properties)) }.to_string();
        for wanted in ["LSQuarantineType = LSQuarantineTypeWebDownload", "LSQuarantineAgentName = Wings"] {
            assert!(said.contains(wanted), "{wanted} isn't in {said}");
        }
    }

    /// A page gets its first download by itself, as Chrome gives it, then only those you start, up to a document's most;
    /// loading itself again doesn't give it another.
    #[test]
    fn a_page_downloads_once_by_itself_and_never_spams_files() {
        let mut gate = Gate::default();
        assert!(gate.allow(1, false), "the first one it starts by itself");
        assert!(!gate.allow(1, false), "not a second");
        assert!(!gate.allow(2, false), "nor in a document the page loaded itself");
        assert!(gate.allow(2, true), "one you clicked");
        assert!(gate.allow(2, false), "which gives it one by itself again");
        assert!(!gate.allow(3, false));
        let mut gate = Gate::default();
        let clicked = (0..30).filter(|_| gate.allow(7, true)).count();
        assert_eq!(clicked, MOST_PER_DOCUMENT as usize, "clicked or not, a document has a most");
    }
}
