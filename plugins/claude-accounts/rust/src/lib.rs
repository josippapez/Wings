//! Choose which Claude Code account each Wings project uses, start Claude as another account, and add a new one.
//!
//! The crate builds three frames: the main script (`--features main`), which tells you in a notification that an account is
//! signed out (`signed_out.rs`), and the sidebar and its popover (`--features sidebar`, `--features popover`), which draw the same
//! cards (`views.rs`) over the same state (`state.rs`) and tell each other when an account changed.

// A frame uses only its part of the crate.
#![allow(dead_code)]

mod accounts;
mod actions;
#[cfg(feature = "popover")]
mod popover;
#[cfg(feature = "sidebar")]
mod sidebar;
#[cfg(any(feature = "main", test))]
mod signed_out;
mod state;
#[cfg(test)]
mod tests;
mod views;

#[cfg(feature = "main")]
#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    signed_out::watch();
}
