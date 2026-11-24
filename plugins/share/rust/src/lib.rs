//! Share: puts a program that listens on a local port at a public https link with OpenTunnel (`opentunnel route add`),
//! for Claude through its `share_port`, `list_shares` and `stop_share` tools, and for the user through a badge on the
//! pane the program runs in. Every link this plugin made is removed when its port closes, since OpenTunnel's service
//! keeps routes after Wings quits.
//!
//! The crate builds two frames. The main script (`--features main`) runs `opentunnel`, answers the tools, asks the user
//! to allow each port, keeps the badges and takes the links down. The modal (`--features share`, `share_page.rs`) shows
//! what waits for an answer and what is shared, with Copy link, Open and Stop.

// Each frame uses its own part of the crate.
#![allow(dead_code)]

#[cfg(feature = "main")]
mod approvals;
#[cfg(feature = "main")]
mod main_frame;
#[cfg(feature = "share")]
mod share_page;
mod shares;
#[cfg(feature = "main")]
mod tools;
mod tunnel;

/// A page tells main what it wants: `{ id, op, ... }`, which main answers with `DONE`.
pub const ASK: &str = "ask";
/// What main tells back: `{ id, error }`, the error null when it worked.
pub const DONE: &str = "done";
/// A page asks main for `STATE`.
pub const WANT: &str = "want";
/// What a page can't read itself: `{ approvals, shares }`.
pub const STATE: &str = "state";
