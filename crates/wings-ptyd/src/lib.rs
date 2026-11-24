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

/// The server's own version, apart from the app's, in its `welcome`. Bump it when what the server runs changes (`host`,
/// `output`, `proto`), so an app can tell an older server that still runs from before an update, and leave it to finish
/// its panes: a server that keeps its version keeps running its old code through every update.
pub const SERVER_VERSION: u32 = 9;

/// The id a pane's shell gets in `WINGS_PANE_ID`, from the number the server gave it.
pub fn pane_id(number: u32) -> String {
    format!("p{number}")
}

#[cfg(test)]
mod tests {
    /// What the server runs, as `SERVER_VERSION` ships it. A change to it without a new version stays out of every
    /// running server: 0.2.90's process group for jobs did, as the version stayed 8.
    #[test]
    fn a_change_to_what_the_server_runs_comes_with_a_new_server_version() {
        let sources = [
            include_str!("host.rs"),
            include_str!("host/job.rs"),
            include_str!("output.rs"),
            include_str!("proto.rs"),
        ];
        let hash = sources.iter().flat_map(|source| source.bytes()).fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
        assert_eq!(
            (super::SERVER_VERSION, format!("{hash:016x}")),
            (9, "639cfc1e21d52422".to_owned()),
            "the server's code changed: bump SERVER_VERSION, then put the new version and hash here"
        );
    }
}
