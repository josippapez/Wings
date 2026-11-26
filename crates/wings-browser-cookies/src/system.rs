//! Everything that touches this Mac: finding the browsers' profiles, copying a cookie database, running the system
//! `sqlite3`, and the Keychain read.

use crate::chromium;
use crate::crypto::{derive_key, Key};
use crate::firefox;
use crate::safari;
use crate::{unix_now, Browser, Read, ReadError, Source};
use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
use std::fs;
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use zeroize::Zeroizing;

const SQLITE3: &str = "/usr/bin/sqlite3";

/// OSStatus codes from Security/SecBase.h.
const ERR_SEC_USER_CANCELED: i32 = -128;
const ERR_SEC_AUTH_FAILED: i32 = -25293;
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
const ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25308;

/// A Chromium browser's folder under `~/Library/Application Support`, and the service of its Keychain item.
/// Chrome, Brave, Edge, Chromium and Vivaldi: yt-dlp cookies.py and Chromium's keychain_password_mac.mm. Arc:
/// published write-ups (a PR in nottelabs/notte-cli), not tested here.
const CHROMIUM_FAMILY: [(Browser, &str, &str); 6] = [
    (Browser::Chrome, "Google/Chrome", "Chrome Safe Storage"),
    (Browser::Arc, "Arc/User Data", "Arc Safe Storage"),
    (Browser::Brave, "BraveSoftware/Brave-Browser", "Brave Safe Storage"),
    (Browser::Edge, "Microsoft Edge", "Microsoft Edge Safe Storage"),
    (Browser::Chromium, "Chromium", "Chromium Safe Storage"),
    (Browser::Vivaldi, "Vivaldi", "Vivaldi Safe Storage"),
];

fn app_support() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support"))
}

fn map_io(error: io::Error) -> ReadError {
    match error.kind() {
        // macOS answers EPERM for a protected folder until the app has Full Disk Access.
        io::ErrorKind::PermissionDenied => ReadError::NeedsFullDiskAccess,
        io::ErrorKind::NotFound => ReadError::NotFound,
        _ => ReadError::Unreadable(error.to_string()),
    }
}

// ---- detect ----

pub(crate) fn detect() -> Vec<Source> {
    let mut sources = Vec::new();
    if let Some(support) = app_support() {
        for (browser, folder, _) in CHROMIUM_FAMILY {
            sources.extend(chromium_profiles(browser, &support.join(folder)));
        }
        sources.extend(firefox_profiles(&support.join("Firefox")));
    }
    sources.extend(safari_source());
    sources
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// `profile.info_cache` of `Local State`: the folder name to the name the user sees (Chromium's
/// ProfileAttributesEntry::kNameKey is "name").
fn profile_names(local_state: &Path) -> Vec<(String, String)> {
    let Ok(text) = fs::read_to_string(local_state) else { return Vec::new() };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    json.pointer("/profile/info_cache")
        .and_then(|v| v.as_object())
        .map(|cache| {
            cache
                .iter()
                .filter_map(|(dir, info)| Some((dir.clone(), info.get("name")?.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

fn chromium_profiles(browser: Browser, root: &Path) -> Vec<Source> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let names = profile_names(&root.join("Local State"));
    let mut dirs: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n == "Default" || n.starts_with("Profile "))
        .collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|dir| {
            // Newer builds keep the store in Network/, older ones next to the profile; take the fresher.
            let store = [root.join(&dir).join("Network/Cookies"), root.join(&dir).join("Cookies")]
                .into_iter()
                .filter(|p| p.is_file())
                .max_by_key(|p| modified(p))?;
            let profile = names.iter().find(|(d, _)| *d == dir).map_or_else(|| dir.clone(), |(_, n)| n.clone());
            Some(Source {
                id: format!("{}:{dir}", browser.slug()),
                browser,
                browser_name: browser.name().to_owned(),
                profile,
                path: store,
            })
        })
        .collect()
}

fn firefox_profiles(root: &Path) -> Vec<Source> {
    let ini = root.join("profiles.ini");
    let text = match fs::read_to_string(&ini) {
        Ok(text) => text,
        // The folder is there but macOS hides it: offer Firefox so the import can ask for Full Disk Access.
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return vec![Source {
                id: "firefox:".to_owned(),
                browser: Browser::Firefox,
                browser_name: Browser::Firefox.name().to_owned(),
                profile: "Default".to_owned(),
                path: ini,
            }]
        }
        Err(_) => return Vec::new(),
    };
    firefox::parse_profiles_ini(&text)
        .into_iter()
        .filter_map(|p| {
            let dir = if p.relative { root.join(&p.path) } else { PathBuf::from(&p.path) };
            let store = dir.join("cookies.sqlite");
            store.is_file().then(|| Source {
                id: format!("firefox:{}", p.path),
                browser: Browser::Firefox,
                browser_name: Browser::Firefox.name().to_owned(),
                profile: p.name,
                path: store,
            })
        })
        .collect()
}

/// Safari's folder is protected, so a stat that is refused means "there, but not readable yet".
fn safari_source() -> Option<Source> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let candidates = [
        home.join("Library/Containers/com.apple.Safari/Data/Library/Cookies/Cookies.binarycookies"),
        home.join("Library/Cookies/Cookies.binarycookies"),
    ];
    let path = candidates
        .iter()
        .find(|p| match fs::metadata(p) {
            Ok(_) => true,
            Err(e) => e.kind() == io::ErrorKind::PermissionDenied,
        })?
        .clone();
    Some(Source {
        id: "safari:default".to_owned(),
        browser: Browser::Safari,
        browser_name: Browser::Safari.name().to_owned(),
        profile: "Default".to_owned(),
        path,
    })
}

// ---- read ----

pub(crate) fn read(source: &Source) -> Result<Read, ReadError> {
    match source.browser {
        Browser::Safari => read_safari(&source.path),
        Browser::Firefox => read_firefox(source),
        browser => {
            let service = CHROMIUM_FAMILY
                .iter()
                .find(|(b, _, _)| *b == browser)
                .map(|(_, _, service)| *service)
                .ok_or(ReadError::NotFound)?;
            let db = TempDb::copy(&source.path)?;
            let meta = chromium::meta_version_from_json(&db.query(chromium::META_SQL)?);
            let rows = chromium::rows_from_json(&db.query(chromium::ROWS_SQL)?).map_err(ReadError::Unreadable)?;
            chromium::cookies_from_rows(rows, meta, &mut || keychain_key(service), unix_now())
        }
    }
}

fn read_safari(path: &Path) -> Result<Read, ReadError> {
    let data = Zeroizing::new(fs::read(path).map_err(map_io)?);
    let (cookies, skipped) = safari::parse(&data, unix_now())
        .map_err(|_| ReadError::Unreadable("Safari's cookie file is not in the expected format".to_owned()))?;
    Ok(Read { cookies, skipped })
}

fn read_firefox(source: &Source) -> Result<Read, ReadError> {
    if source.path.file_name().is_some_and(|n| n == "profiles.ini") {
        // The placeholder from `detect`: the profile list was unreadable then.
        return match fs::read_to_string(&source.path) {
            Err(e) => Err(map_io(e)),
            Ok(_) => Err(ReadError::Unreadable("Firefox's profiles can now be read: refresh the list".to_owned())),
        };
    }
    let db = TempDb::copy(&source.path)?;
    let version = firefox::schema_version_from_json(&db.query(firefox::VERSION_SQL)?);
    let (cookies, skipped) =
        firefox::cookies_from_json(&db.query(firefox::ROWS_SQL)?, version, unix_now()).map_err(ReadError::Unreadable)?;
    Ok(Read { cookies, skipped })
}

/// The browser's Safe Storage password from the login Keychain, turned into the AES key. Read in this process, so the
/// system prompt names Wings and the item, and "Always Allow" covers only Wings. Looked up by service alone: the
/// account name differs per browser and Arc's is not documented.
fn keychain_key(service: &str) -> Result<Key, ReadError> {
    let found = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(service)
        .load_data(true)
        .limit(Limit::Max(1))
        .search();
    match found {
        Ok(items) => items
            .into_iter()
            .find_map(|item| match item {
                SearchResult::Data(bytes) => Some(Zeroizing::new(bytes)),
                _ => None,
            })
            .map(|password| derive_key(&password))
            .ok_or_else(|| ReadError::Unreadable(format!("the Keychain returned no password for \"{service}\""))),
        Err(e) => Err(match e.code() {
            ERR_SEC_USER_CANCELED | ERR_SEC_AUTH_FAILED | ERR_SEC_INTERACTION_NOT_ALLOWED => ReadError::KeychainDenied,
            ERR_SEC_ITEM_NOT_FOUND => ReadError::Unreadable(format!("\"{service}\" is not in the Keychain")),
            code => ReadError::Unreadable(format!("Keychain error {code}")),
        }),
    }
}

// ---- private database copy ----

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A private copy of a cookie database in a fresh 0700 folder, removed when this is dropped, also on an error path.
/// The browser keeps the original locked and its newest rows in the `-wal` file, so both are copied.
struct TempDb {
    dir: PathBuf,
    db: PathBuf,
}

impl TempDb {
    fn copy(source: &Path) -> Result<Self, ReadError> {
        Self::copy_into(&std::env::temp_dir(), source)
    }

    fn copy_into(parent: &Path, source: &Path) -> Result<Self, ReadError> {
        // A copy a crash left behind (its process's guard never ran) goes now: it holds another browser's cookies.
        let ours = format!("wings-cookies-{}-", std::process::id());
        if let Ok(entries) = fs::read_dir(parent) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("wings-cookies-") && !name.starts_with("wings-cookies-test-") && !name.starts_with(&ours) {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let dir = parent.join(format!(
            "wings-cookies-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&dir).map_err(map_io)?;
        let this = Self { db: dir.join("cookies.db"), dir };
        fs::copy(source, &this.db).map_err(map_io)?;
        for suffix in ["-wal", "-journal"] {
            let mut from = source.as_os_str().to_owned();
            from.push(suffix);
            let mut to = this.db.as_os_str().to_owned();
            to.push(suffix);
            match fs::copy(&from, &to) {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(map_io(e)),
            }
        }
        Ok(this)
    }

    /// The rows of `sql` as sqlite3's JSON. The copy is ours, so it is opened normally rather than `-readonly`: a
    /// WAL database needs to write its `-shm` file next to it to be read.
    fn query(&self, sql: &str) -> Result<Zeroizing<String>, ReadError> {
        let output = Command::new(SQLITE3)
            .arg("-json")
            .arg(&self.db)
            .arg(sql)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| ReadError::Unreadable(format!("could not run sqlite3: {e}")))?;
        let stdout = Zeroizing::new(output.stdout);
        if !output.status.success() {
            let why = String::from_utf8_lossy(&output.stderr);
            return Err(ReadError::Unreadable(why.lines().next().unwrap_or("sqlite3 failed").to_owned()));
        }
        String::from_utf8(stdout.to_vec())
            .map(Zeroizing::new)
            .map_err(|_| ReadError::Unreadable("sqlite3 printed text that is not UTF-8".to_owned()))
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chromium::tests::{hex, sealed_v24};
    use crate::crypto::derive_key;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wings-cookies-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sqlite(db: &Path, sql: &str) {
        let status = Command::new(SQLITE3).arg(db).arg(sql).status().unwrap();
        assert!(status.success());
    }

    /// A Chromium v24 store built with sqlite3, read through the real copy and query path. The key is injected
    /// by calling `cookies_from_rows` the way `read` does, so the Keychain is never touched.
    #[test]
    fn chromium_store_round_trip_and_copy_is_removed() {
        let dir = scratch("chromium");
        let db = dir.join("Cookies");
        let key = derive_key(b"fixture password");
        let enc = hex(&sealed_v24(&key, ".example.com", "s3cret"));
        sqlite(
            &db,
            &format!(
                "CREATE TABLE meta(key LONGVARCHAR NOT NULL UNIQUE PRIMARY KEY, value LONGVARCHAR);\
                 INSERT INTO meta VALUES('version','24');\
                 CREATE TABLE cookies(host_key TEXT, top_frame_site_key TEXT, name TEXT, value TEXT, \
                   encrypted_value BLOB, path TEXT, expires_utc INTEGER, has_expires INTEGER, is_secure INTEGER, \
                   is_httponly INTEGER, samesite INTEGER);\
                 INSERT INTO cookies VALUES('.example.com','','sid','',x'{enc}','/',13400000000000000,1,1,1,1);\
                 INSERT INTO cookies VALUES('.example.com','https://top.test','part','',x'',  '/',0,0,0,0,-1);"
            ),
        );
        let tmp_dir;
        {
            let copy = TempDb::copy(&db).unwrap();
            tmp_dir = copy.dir.clone();
            let mode = std::os::unix::fs::PermissionsExt::mode(&fs::metadata(&tmp_dir).unwrap().permissions());
            assert_eq!(mode & 0o777, 0o700);
            let meta = chromium::meta_version_from_json(&copy.query(chromium::META_SQL).unwrap());
            let rows = chromium::rows_from_json(&copy.query(chromium::ROWS_SQL).unwrap()).unwrap();
            let read = chromium::cookies_from_rows(rows, meta, &mut || Ok(derive_key(b"fixture password")), 1.0).unwrap();
            // The partitioned row is left out; the encrypted one is decrypted.
            assert_eq!(read.cookies.len(), 1);
            assert_eq!(read.cookies[0].value, "s3cret");
            assert_eq!(read.cookies[0].same_site, Some(crate::SameSite::Lax));
        }
        assert!(!tmp_dir.exists(), "the private copy must be removed");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_copy_leaves_nothing_behind() {
        let parent = scratch("failed-copy");
        // A copy a crashed run of another process left is swept away by the next one.
        let left = parent.join("wings-cookies-1-12-0");
        fs::create_dir_all(&left).unwrap();
        fs::write(left.join("cookies.db"), b"secret").unwrap();
        let err = TempDb::copy_into(&parent, Path::new("/nonexistent/Cookies")).err();
        assert!(!left.exists(), "a crashed run's copy is gone");
        assert_eq!(err, Some(ReadError::NotFound));
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn firefox_store_with_wal_round_trip() {
        let dir = scratch("firefox");
        let db = dir.join("cookies.sqlite");
        // Firefox keeps its store in WAL mode, so the query must work on a WAL database.
        sqlite(
            &db,
            "PRAGMA journal_mode=WAL; PRAGMA user_version=17;\
             CREATE TABLE moz_cookies(id INTEGER PRIMARY KEY, originAttributes TEXT NOT NULL DEFAULT '', name TEXT, value TEXT, \
               host TEXT, path TEXT, expiry INTEGER, isSecure INTEGER, isHttpOnly INTEGER, sameSite INTEGER);\
             INSERT INTO moz_cookies(name,value,host,path,expiry,isSecure,isHttpOnly,sameSite) VALUES('a','1','.f.com','/',4102444800000,1,0,256);\
             INSERT INTO moz_cookies(originAttributes,name,value,host,path,expiry,isSecure,isHttpOnly,sameSite) VALUES('^userContextId=2','c','3','.f.com','/',4102444800000,0,0,0);",
        );
        let copy = TempDb::copy(&db).unwrap();
        let version = firefox::schema_version_from_json(&copy.query(firefox::VERSION_SQL).unwrap());
        assert_eq!(version, 17);
        let (cookies, _) = firefox::cookies_from_json(&copy.query(firefox::ROWS_SQL).unwrap(), version, 1.0).unwrap();
        assert_eq!(cookies.len(), 1, "container cookies stay out");
        assert_eq!(cookies[0].expires, Some(4_102_444_800.0));
        drop(copy);
        let _ = fs::remove_dir_all(dir);
    }
}
