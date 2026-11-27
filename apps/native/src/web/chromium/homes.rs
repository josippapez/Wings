//! Where each profile's Chromium data lives, so that no two profiles ever share any. Chrome-style CEF keeps its global
//! context in `Default` under the profiles' root whatever `cache_path` names, and only that context's profile can have
//! extensions (the DevTools protocol loads them into it). The profile that starts Chromium in a run is the global one;
//! every other keeps its data in a folder of its own, named by `runtime::dir_name`, which its pages' context opens. So
//! before CEF starts, the starting profile's folder becomes `Default`, and the profile `Default` held goes back to its
//! own folder; `Default.owner` names whose `Default` is.

use std::path::{Path, PathBuf};

const DEFAULT: &str = "Default";
const OWNER: &str = "Default.owner";

/// The profile whose data `Default` holds, if one is noted.
fn owner(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join(OWNER)).ok().map(|owner| owner.trim().to_owned()).filter(|owner| !owner.is_empty())
}

/// The folder holding the data of the profile `name` (`runtime::dir_name`) under the profiles' root `root`.
pub fn home(root: &Path, name: &str) -> PathBuf {
    match owner(root).as_deref() == Some(name) {
        true => root.join(DEFAULT),
        false => root.join(name),
    }
}

/// Makes `Default` hold the data of the profile `name`, which starts Chromium: the profile it held goes back to its own
/// folder first. Each step is a rename, in an order a run stopped between two of them picks up from. A `Default` from
/// before owners were noted counts as the starting profile's. Fails rather than put two profiles' data in one folder.
pub fn make_global(root: &Path, name: &str) -> Result<(), String> {
    let default = root.join(DEFAULT);
    let held = owner(root);
    if held.as_deref() != Some(name) {
        if let Some(held) = held.filter(|_| default.exists()) {
            // A folder left empty (CEF made one for a context) is replaced; one with data stops it.
            std::fs::rename(&default, root.join(&held)).map_err(|error| format!("Chromium's data of {held} can't go back to its folder: {error}"))?;
        }
        std::fs::write(root.join(OWNER), name).map_err(|error| error.to_string())?;
    }
    let own = root.join(name);
    if !default.exists() && own.exists() {
        std::fs::rename(&own, &default).map_err(|error| format!("Chromium's data of {name} can't be opened: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holds(folder: &Path, what: &str) -> bool {
        std::fs::read_to_string(folder.join("Cookies")).is_ok_and(|read| read == what)
    }

    fn with(root: &Path, folder: &str, what: &str) {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::write(root.join(folder).join("Cookies"), what).unwrap();
    }

    /// Whichever profile starts Chromium, its own data is in `Default` and every other profile's in its own folder:
    /// the Browser's cookies never show in another plugin's pages, nor theirs in the Browser's.
    #[test]
    fn the_profile_starting_chromium_gets_default_and_every_other_keeps_its_own() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        make_global(root, "browser.a").unwrap();
        assert!(!root.join(DEFAULT).exists(), "nothing yet: CEF makes it");
        with(root, DEFAULT, "browser's");
        with(root, "notes.b", "notes'");
        assert_eq!((home(root, "browser.a"), home(root, "notes.b")), (root.join(DEFAULT), root.join("notes.b")));

        make_global(root, "notes.b").unwrap();
        assert!(holds(&root.join(DEFAULT), "notes'") && holds(&root.join("browser.a"), "browser's"));
        assert_eq!((home(root, "browser.a"), home(root, "notes.b")), (root.join("browser.a"), root.join(DEFAULT)));

        make_global(root, "browser.a").unwrap();
        assert!(holds(&root.join(DEFAULT), "browser's") && holds(&root.join("notes.b"), "notes'"));
        make_global(root, "browser.a").unwrap();
        assert!(holds(&root.join(DEFAULT), "browser's"), "the same profile again moves nothing");
    }

    /// A start stopped between two renames leaves no profile's data in another's place: the next start picks up.
    #[test]
    fn a_start_stopped_half_way_is_picked_up() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        // Stopped after `Default` went back to its owner's folder: noted for the old owner, no `Default`.
        with(root, "browser.a", "browser's");
        with(root, "notes.b", "notes'");
        std::fs::write(root.join(OWNER), "browser.a").unwrap();
        make_global(root, "notes.b").unwrap();
        assert!(holds(&root.join(DEFAULT), "notes'") && holds(&root.join("browser.a"), "browser's"));
        // Stopped after noting the new owner, before its folder became `Default`.
        std::fs::rename(root.join(DEFAULT), root.join("notes.b")).unwrap();
        make_global(root, "notes.b").unwrap();
        assert!(holds(&root.join(DEFAULT), "notes'"));
    }

    /// A `Default` from before owners were noted stays the starting profile's, an empty folder CEF made for a context
    /// is no obstacle, and a folder of data in the way stops the start rather than mixing two profiles.
    #[test]
    fn an_old_default_is_kept_and_data_in_the_way_stops_the_start() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        with(root, DEFAULT, "browser's");
        std::fs::create_dir_all(root.join("browser.a")).unwrap();
        make_global(root, "browser.a").unwrap();
        assert!(holds(&root.join(DEFAULT), "browser's"));
        make_global(root, "notes.b").unwrap();
        assert!(holds(&root.join("browser.a"), "browser's"), "into the empty folder CEF had made");
        with(root, DEFAULT, "notes'");
        with(root, "notes.b", "stray");
        assert!(make_global(root, "browser.a").is_err());
        assert!(holds(&root.join(DEFAULT), "notes'") && holds(&root.join("notes.b"), "stray") && holds(&root.join("browser.a"), "browser's"));
    }
}
