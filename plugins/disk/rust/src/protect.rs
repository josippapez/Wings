//! What the Collector refuses, so a click can't queue a whole disk, a system folder or a home folder for the Trash. The
//! folders that are only refused themselves have contents that may be collected: `/Library/Caches` is fine, `/Library` isn't.

/// These folders, and everything inside them.
const SYSTEM: [&str; 6] = ["/system", "/bin", "/sbin", "/usr", "/private/var/db", "/library/apple"];
/// Inside `/usr`, this one is for the user's own programs.
const USER_PROGRAMS: &str = "/usr/local/";
/// These folders themselves. `/var`, `/etc` and `/tmp` are `/private/...`: see `canonical`.
const FOLDERS: [&str; 12] = ["/applications", "/library", "/users", "/private", "/private/var", "/private/etc", "/private/tmp", "/opt", "/cores", "/volumes", "/dev", "/home"];
const HOME_FOLDERS: [&str; 9] = ["library", "desktop", "documents", "downloads", "applications", "movies", "music", "pictures", "public"];

/// `path` without repeated and trailing slashes, in lower case: macOS volumes ignore case.
fn simplify(path: &str) -> String {
    format!("/{}", path.to_lowercase().split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("/"))
}

/// `/var`, `/etc` and `/tmp` are links into `/private`, and a path through them is the same folder.
fn canonical(path: String) -> String {
    let linked = ["/var", "/etc", "/tmp"].iter().any(|link| path == *link || path.starts_with(&format!("{link}/")));
    if linked { format!("/private{path}") } else { path }
}

fn inside(path: &str, folder: &str) -> bool {
    path == folder || path.starts_with(&format!("{folder}/"))
}

/// Why `path` may not go in the Collector, as a sentence that starts with it, or none when it may. `mounts` are the mount
/// points of the disks, which are refused like `/`. A path that isn't absolute is refused too.
pub fn protected_reason(path: &str, mounts: &[String]) -> Option<String> {
    if !path.starts_with('/') {
        return Some(format!("{path} isn't a full path."));
    }
    // The system takes `..` after following a link, so `~/x/link/../Desktop` can be the real Desktop while it reads as
    // `~/x/Desktop`. Scans name real paths, which have neither.
    if path.split('/').any(|part| part == "." || part == "..") {
        return Some(format!("{path} has . or .. in it: use its real path."));
    }
    let p = canonical(simplify(path));
    if p == "/" || mounts.iter().any(|mount| canonical(simplify(mount)) == p) {
        return Some(format!("{path} is a whole disk."));
    }
    if SYSTEM.iter().any(|folder| inside(&p, folder)) && !p.starts_with(USER_PROGRAMS) || FOLDERS.contains(&p.as_str()) {
        return Some(format!("{path} is a system folder."));
    }
    let parts: Vec<&str> = p[1..].split('/').collect();
    let home = matches!(parts.first(), Some(&"users" | &"home"));
    if home && parts.len() == 2 {
        return Some(format!("{path} is a home folder."));
    }
    if home && parts.len() == 3 && HOME_FOLDERS.contains(&parts[2]) {
        return Some(format!("{path} is one of the standard folders of a home folder."));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mounts() -> Vec<String> {
        vec!["/".into(), "/Volumes/encrypted".into()]
    }

    #[test]
    fn disks_system_folders_home_folders_and_relative_paths_are_refused_each_with_a_sentence_that_starts_with_the_path() {
        assert_eq!(protected_reason("/Library", &mounts()).as_deref(), Some("/Library is a system folder."));
        for path in ["/", "/Volumes/encrypted", "/Volumes/encrypted/", "/System", "/System/Library/Fonts", "/usr", "/usr/bin/env", "/usr/local", "/bin/sh", "/private/var/db/x", "/Library/Apple/y"] {
            assert!(protected_reason(path, &mounts()).is_some_and(|reason| reason.starts_with(path)), "{path}");
        }
        for path in [
            "/Applications", "/Users", "/private", "/private/var", "/var", "/etc", "/tmp", "/opt", "/cores", "/Volumes", "/dev", "/home", "/Users/ada", "/home/ada", "/Users/ada/Library",
            "/Users/ada/Downloads", "/Users/ada/Public", "relative/path", "~/Downloads",
        ] {
            assert!(protected_reason(path, &mounts()).is_some(), "{path}");
        }
        // `..`, repeated slashes, a trailing slash, case and the links into /private don't get a path past it.
        for path in ["/Users/ada/Documents/../Downloads/..", "//Library//", "/library", "/Users/ada/library/", "/var/db/dslocal", "/tmp/../etc"] {
            assert!(protected_reason(path, &mounts()).is_some(), "{path}");
        }
        // Read as text this is ~/Code/Desktop, but through a link `..` can land in the real home folder.
        assert_eq!(protected_reason("/Users/ada/Code/link/../Desktop", &mounts()).as_deref(), Some("/Users/ada/Code/link/../Desktop has . or .. in it: use its real path."));
        assert!(protected_reason("/Users/ada/Code/./x", &mounts()).is_some());
    }

    #[test]
    fn what_is_inside_a_folder_that_is_only_refused_itself_and_inside_usr_local_may_be_collected() {
        for path in [
            "/Users/ada/Downloads/old.zip", "/Users/ada/Library/Caches", "/Users/ada/Desktop/Screenshot.png", "/Library/Developer", "/Library/Caches/com.example", "/Applications/Xcode.app",
            "/usr/local/Cellar", "/private/var/folders/zz", "/var/folders", "/opt/homebrew", "/Volumes/encrypted/Backups", "/Users/ada/Code/.git",
        ] {
            assert_eq!(protected_reason(path, &mounts()), None, "{path}");
        }
    }
}
