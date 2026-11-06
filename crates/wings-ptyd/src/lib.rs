//! The Wings terminal server: a process apart from the app that owns the terminals, so their programs keep running
//! while the app quits, restarts, crashes or updates. `host` is the server, `proto` how apps talk to it, `output` what
//! it keeps of each pane, and `client` how an app reaches one pane. On macOS, `launchd` starts it apart from the app and
//! `responsibility` makes it the owner of its panes' privacy grants.

#[cfg(unix)]
pub mod client;
#[cfg(unix)]
pub mod host;
#[cfg(target_os = "macos")]
pub mod launchd;
pub mod output;
pub mod proto;
#[cfg(target_os = "macos")]
pub mod responsibility;

/// The server's own version, apart from the app's, in its `welcome`. Bump it when `host.rs` changes, so an app can
/// tell an older server that still runs from before an update, and leave it to finish its panes.
pub const SERVER_VERSION: u32 = 3;

/// The id a pane's shell gets in `WINGS_PANE_ID`, from the number the server gave it.
pub fn pane_id(number: u32) -> String {
    format!("p{number}")
}
