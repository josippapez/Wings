//! The folders the person let a Browser profile's agents use for files, besides the temporary folders and the ones the
//! environment names (`policy::Files::of` reads them). They are the app's own record, one file in the data folder
//! beside `plugin-web` and `plugin-storage`, never in a plugin's storage or web folder: a plugin can write
//! there, so it could widen its own file access. A folder goes in only through the system's picker, right after the
//! person's click (`webFolders` `add`), and has to pass what a root passes (`policy::check_folder`).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use super::Profile;

const FILE: &str = "web-file-folders.json";

/// The record of the profile whose web folder is `web` (`<data>/plugin-web/<plugin>.<scope>`): its file and its key.
pub fn record_of(web: &Path) -> Option<(PathBuf, String)> {
    let key = web.file_name()?.to_str()?.to_owned();
    Some((web.parent()?.parent()?.join(FILE), key))
}

fn read(file: &Path) -> BTreeMap<String, Vec<PathBuf>> {
    std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Written whole beside the file and renamed over it, so a crash leaves the old list or the new, never half.
fn write(file: &Path, all: &BTreeMap<String, Vec<PathBuf>>) -> Result<(), String> {
    let part = file.with_extension("json.part");
    let bytes = serde_json::to_vec_pretty(all).map_err(|error| error.to_string())?;
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|error| format!("Couldn't save the folders: {error}"))?;
    }
    std::fs::write(&part, bytes).map_err(|error| format!("Couldn't save the folders: {error}"))?;
    std::fs::rename(&part, file).map_err(|error| {
        let _ = std::fs::remove_file(&part);
        format!("Couldn't save the folders: {error}")
    })
}

/// The folders listed for the profile whose web folder is `web`, as they were added.
pub fn listed(web: &Path) -> Vec<PathBuf> {
    record_of(web).and_then(|(file, key)| read(&file).remove(&key)).unwrap_or_default()
}

/// The folders listed for `profile`.
pub fn list(profile: &Profile) -> Vec<PathBuf> {
    listed(&profile.dir)
}

/// Lists `path` for `profile` after it passes the checks of a root: the list now. Adding a folder that is listed is a
/// no-op.
pub fn add(profile: &Profile, path: &Path) -> Result<Vec<PathBuf>, String> {
    let (file, key) = record_of(&profile.dir).ok_or("Wings has no data folder")?;
    let real = super::policy::check_folder(path)?;
    let mut all = read(&file);
    let list = all.entry(key).or_default();
    if !list.contains(&real) {
        list.push(real);
    }
    let now = list.clone();
    write(&file, &all)?;
    Ok(now)
}

/// Takes `path` off the list of `profile`: the list now.
pub fn remove(profile: &Profile, path: &Path) -> Result<Vec<PathBuf>, String> {
    let (file, key) = record_of(&profile.dir).ok_or("Wings has no data folder")?;
    let mut all = read(&file);
    let list = all.entry(key.clone()).or_default();
    list.retain(|listed| listed != path);
    let now = list.clone();
    if now.is_empty() {
        all.remove(&key);
    }
    write(&file, &all)?;
    Ok(now)
}
