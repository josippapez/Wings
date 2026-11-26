//! The browser extensions a plugin's web views load: each unpacked in a folder of its own under the plugin's web folder,
//! and a list of them (`extensions.json`) with what each asked for and whether it is on. One the person picks, takes from
//! another browser or a store page is first staged, unpacked beside them, so its page can show what it asks for before
//! the person adds it.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use wings_web_extensions::Info;

use super::Profile;

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
}

/// An extension to load: its id and folder.
pub struct Loadable {
    pub id: String,
    pub dir: PathBuf,
}

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

/// The extensions that are on, with their folders.
pub fn enabled(profile: &Profile) -> Vec<Loadable> {
    list(profile)
        .into_iter()
        .filter(|added| added.enabled && added.allowed)
        .map(|added| Loadable { dir: installed_dir(profile, &added.id), id: added.id })
        .collect()
}

/// Stages the extension in the folder `from` (an unpacked one, or one of another browser's), or the package (a ZIP,
/// CRX or XPI) at the file `from`.
pub fn stage_path(profile: &Profile, from: &Path) -> Result<Staged, String> {
    let stage = new_id();
    let into = staged_dir(profile, &stage);
    let info = if from.is_dir() {
        wings_web_extensions::copy_folder(from, &into)
    } else {
        let bytes = fs::read(from).map_err(|error| format!("Couldn't read {}: {error}", from.display()))?;
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
    let stage = new_id();
    let into = staged_dir(profile, &stage);
    wings_web_extensions::unpack(bytes, &into).map(|info| Staged { stage, info }).map_err(|error| error.to_string())
}

/// Readies the staged extension `stage` to be asked about: its folder moves in with the others, unlisted, so nothing
/// loads it until the person allows it (`keep`) or it goes (`forget`). What it asks for is read again from that folder,
/// not taken from the page.
pub fn prepare(profile: &Profile, stage: &str) -> Result<(Added, PathBuf), String> {
    if !valid_id(stage) {
        return Err("No extension is waiting to be added by that name".into());
    }
    let from = staged_dir(profile, stage);
    let info = wings_web_extensions::read_manifest(&from).map_err(|error| error.to_string())?;
    let id = new_id();
    let dir = installed_dir(profile, &id);
    fs::create_dir_all(profile.dir.join("extensions")).map_err(|error| error.to_string())?;
    fs::rename(&from, &dir).map_err(|error| error.to_string())?;
    let added = Added {
        icon: info.icon.and_then(|icon| icon.strip_prefix(&from).ok().map(Path::to_path_buf)),
        id,
        name: info.name,
        version: info.version,
        description: info.description,
        permissions: info.permissions,
        hosts: info.host_permissions,
        enabled: true,
        allowed: false,
    };
    Ok((added, dir))
}

/// Lists `added`, which the person allowed in Wings' dialog, on.
pub fn keep(profile: &Profile, added: &Added) -> Result<(), String> {
    let mut list = list(profile);
    list.push(Added { allowed: true, enabled: true, ..added.clone() });
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

/// Turns the extension `id` on or off: the one it was, with its folder.
pub fn set_enabled(profile: &Profile, id: &str, enabled: bool) -> Result<(Added, PathBuf), String> {
    let mut list = list(profile);
    let added = list.iter_mut().find(|added| added.id == id).ok_or("No extension by that id")?;
    // One the person never allowed is never turned on: only Wings' dialog as it is added allows it.
    if enabled && !added.allowed {
        return Err("That extension was never allowed. Add it again to see what it can do".into());
    }
    added.enabled = enabled;
    let added = added.clone();
    save(profile, &list)?;
    Ok((added, installed_dir(profile, id)))
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

    fn profile() -> (Profile, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Profile { plugin: "browser".into(), scope: "s".into(), dir: dir.path().join("web") }, dir)
    }

    fn extension(at: &Path, name: &str) {
        fs::create_dir_all(at.join("icons")).unwrap();
        let manifest = serde_json::json!({
            "manifest_version": 3, "name": name, "version": "1.2", "description": "Blocks things",
            "permissions": ["storage"], "host_permissions": ["https://*/*"], "icons": { "48": "icons/48.png" },
        });
        fs::write(at.join("manifest.json"), manifest.to_string()).unwrap();
        fs::write(at.join("icons/48.png"), b"png").unwrap();
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
        assert!(list(&profile).is_empty() && enabled(&profile).is_empty(), "nothing listed or loaded until it is allowed");
        keep(&profile, &added).unwrap();
        let added = list(&profile)[0].clone();
        assert!(dir.join("manifest.json").exists() && source.join("manifest.json").exists(), "a copy, the browser's own stays");
        assert_eq!((added.hosts.clone(), added.enabled), (vec!["https://*/*".to_owned()], true));
        assert_eq!(icon_path(&profile, &added), Some(dir.join("icons/48.png")));
        assert_eq!(enabled(&profile).len(), 1);
        set_enabled(&profile, &added.id, false).unwrap();
        assert!(enabled(&profile).is_empty() && list(&profile).len() == 1);
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
        assert!(set_enabled(&profile, &added.id, true).unwrap_err().contains("never allowed"));
        save(&profile, &[Added { allowed: false, enabled: true, ..added }]).unwrap();
        assert!(enabled(&profile).is_empty());
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
        };
        assert_eq!(icon_path(&profile, &forged), None);
        forged.icon = Some(PathBuf::from("/etc/passwd"));
        assert_eq!(icon_path(&profile, &forged), None);
    }
}
