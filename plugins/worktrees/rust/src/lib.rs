//! Lists the git worktrees of your projects with what each one is, and removes the ones you no longer need after you
//! confirm. It can also look across the whole Mac for repos outside your projects that have worktrees.
//!
//! The crate builds two frames: the sidebar page (`--features sidebar`, `sidebar.rs`), which does all of it, and the
//! main script (`--features main`), which a plugin has to have and which does nothing here.

// The main frame uses none of the crate.
#![allow(dead_code)]

mod elsewhere;
mod git;
#[cfg(feature = "sidebar")]
mod sidebar;
mod worktrees;

#[cfg(feature = "main")]
#[unsafe(no_mangle)]
pub fn start() {}
