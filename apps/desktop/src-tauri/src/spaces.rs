//! Spaces are the projects in the sidebar, persisted as JSON in the app data folder. The store, the worktree setup and
//! the git helpers live in the shared `wings-spaces` crate, which the native app uses with a list of its own; this puts
//! them back under the names the rest of the app uses.

pub use wings_spaces::spaces::*;
