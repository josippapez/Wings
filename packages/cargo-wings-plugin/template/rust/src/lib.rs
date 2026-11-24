//! Hello Wings: a badge on each terminal pane with the folder it is in, a sidebar that lists them, and a tool that
//! tells Claude.
//!
//! The crate builds two frames. The main script (`--features main`, `main_frame.rs`) hears the panes, sets the badges
//! and answers the tool. The sidebar page (`--features sidebar`, `sidebar.rs`) draws what main tells it.

#[cfg(feature = "main")]
mod main_frame;
#[cfg(feature = "sidebar")]
mod sidebar;

/// What main tells the sidebar: `[{ paneId, folder, cwd }]`, under this name. The sidebar asks for it with `WANT`.
pub const PANES: &str = "panes";
pub const WANT: &str = "panes:want";

/// The last part of a path: `/Users/me/code/app/` is `app`.
pub fn folder_name(cwd: &str) -> &str {
    cwd.trim_end_matches('/').rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or(cwd)
}

#[cfg(test)]
mod tests {
    use super::folder_name;

    #[test]
    fn the_folder_is_the_last_part_of_the_path() {
        assert_eq!(folder_name("/Users/me/code/app"), "app");
        assert_eq!(folder_name("/Users/me/code/app/"), "app");
        assert_eq!(folder_name("/"), "/");
    }
}
