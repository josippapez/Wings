//! Where the native app keeps its data, decided here and nowhere else.
//!
//! Two builds of the app exist. The side-by-side one (`Wings-native.app`, bundle id `dev.wings.native`) runs next to
//! the desktop app and keeps its own state in `Wings Native`, reading the desktop's plugins, plugin storage and project
//! pages from `dev.wings.app`. The replacement (`Wings.app`, bundle id `dev.wings.app`, built with `--features switch`
//! by `WINGS_SWITCH=1 sh scripts/build-mac-app.sh`) takes the desktop's place and keeps everything in the desktop's own
//! folder, after `migrate` has brought over what the side-by-side app kept.
//!
//! In the desktop's folder the native state goes to `native-workspace.json`, never to `workspace.json`: the desktop's
//! file stays as the desktop last wrote it, so reinstalling the desktop app gets back its tabs exactly as they were.

use std::path::{Path, PathBuf};

pub mod desktop;
pub mod migrate;

/// This build replaces the desktop app and runs on its data folder.
pub const REPLACEMENT: bool = cfg!(feature = "switch");

/// The app's name in its window title and menus.
pub const APP_NAME: &str = if REPLACEMENT { "Wings" } else { "Wings Native" };

/// The native state's file name in the replacement's folder, beside the desktop's own `workspace.json`.
pub const REPLACEMENT_STATE: &str = "native-workspace.json";

/// The platform's folder for app data: `~/Library/Application Support` on macOS, `%APPDATA%` on Windows, and
/// `$XDG_DATA_HOME` or `~/.local/share` elsewhere.
fn app_support() -> Option<PathBuf> {
    let env = |key| {
        std::env::var_os(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "macos") {
        Some(env("HOME")?.join("Library/Application Support"))
    } else if cfg!(windows) {
        env("APPDATA")
    } else {
        env("XDG_DATA_HOME").or_else(|| Some(env("HOME")?.join(".local/share")))
    }
}

/// The desktop app's data folder, which Tauri names after its bundle id (`wings_connect::cli::IDENTIFIER`, on Unix only).
pub fn desktop_dir() -> Option<PathBuf> {
    Some(app_support()?.join("dev.wings.app"))
}

/// The side-by-side app's own folder.
pub fn side_by_side_dir() -> Option<PathBuf> {
    let name = if cfg!(any(target_os = "macos", windows)) {
        "Wings Native"
    } else {
        "wings-native"
    };
    Some(app_support()?.join(name))
}

/// The state file in `dir`, for the build `replacement` says.
pub fn state_file_in(dir: &Path, replacement: bool) -> PathBuf {
    dir.join(if replacement {
        REPLACEMENT_STATE
    } else {
        "workspace.json"
    })
}

/// This build's workspace file. Its spaces, scrollback, terminal server and connection markers sit beside it.
pub fn state_file() -> Option<PathBuf> {
    let dir = if REPLACEMENT {
        desktop_dir()?
    } else {
        side_by_side_dir()?
    };
    Some(state_file_in(&dir, REPLACEMENT))
}

#[cfg(test)]
mod tests;
