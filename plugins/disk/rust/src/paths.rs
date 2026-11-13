//! Paths the way main and Claude's tools meet them: with trailing slashes, or starting with `~`.

/// `path` without trailing slashes: `/Users/me/` is `/Users/me`, and `/` stays.
pub fn tidy(path: &str) -> String {
    if path.len() <= 1 {
        return path.to_owned();
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() { "/".into() } else { trimmed.to_owned() }
}

/// Whether `path` is below `folder`, and not the folder itself.
pub fn inside(path: &str, folder: &str) -> bool {
    let prefix = if folder.ends_with('/') { folder.to_owned() } else { format!("{folder}/") };
    path.starts_with(&prefix) && path.len() > folder.len()
}

/// The home folder, once a scan of `~` or of a folder under it is done: Wings turned `~` into the tree's name there, so
/// the name less what followed the `~` is the folder `~` stands for. `scans` are each scan's root and tree name. None
/// before that: nothing else in a plugin says where it is.
pub fn home_folder<'a>(scans: impl IntoIterator<Item = (&'a str, &'a str)>) -> Option<String> {
    scans.into_iter().find_map(|(root, name)| {
        if root != "~" && !root.starts_with("~/") {
            return None;
        }
        let after = &tidy(root)[1..];
        let home = name.strip_suffix(after)?;
        Some(if home.is_empty() { "/".to_owned() } else { home.to_owned() })
    })
}

/// `path` with its leading `~` made the home folder, tidied; none for a `~` path while the home folder isn't known.
pub fn expand_home(path: &str, home: Option<&str>) -> Option<String> {
    let full = tidy(path);
    if full != "~" && !full.starts_with("~/") {
        return Some(full);
    }
    home.map(|home| tidy(&format!("{home}{}", &full[1..])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_tidied_and_told_from_the_folder_above_it() {
        assert_eq!([tidy("/Users/me/"), tidy("/"), tidy("//"), tidy("~")], ["/Users/me", "/", "/", "~"]);
        assert!(inside("/Users/me/Code", "/Users/me") && inside("/x", "/"));
        assert!(!inside("/Users/me", "/Users/me") && !inside("/Users/men", "/Users/me"));
    }

    #[test]
    fn the_home_folder_is_what_a_scan_of_a_tilde_path_names_less_what_followed_the_tilde() {
        assert_eq!(home_folder([("/", "/"), ("~", "/Users/ada")]).as_deref(), Some("/Users/ada"));
        assert_eq!(home_folder([("~/Code/", "/Users/ada/Code")]).as_deref(), Some("/Users/ada"));
        assert_eq!(home_folder([("/Volumes/Backup", "/Volumes/Backup")]), None);
        assert_eq!(expand_home("~/Code/", Some("/Users/ada")).as_deref(), Some("/Users/ada/Code"));
        assert_eq!(expand_home("~", Some("/Users/ada")).as_deref(), Some("/Users/ada"));
        assert_eq!([expand_home("~/Code", None), expand_home("/tmp/", None).map(|p| p + "!")], [None, Some("/tmp!".into())]);
    }
}
