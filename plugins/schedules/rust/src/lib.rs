//! Starts a coding agent (Claude Code or OpenCode) with a prompt in one of your projects by itself, every few hours or at
//! a set time on the days you pick, and watches a project's Azure DevOps, GitHub or GitLab for tickets assigned to you,
//! offering to start an agent on each in a new worktree.
//!
//! The crate builds three frames. The main script (`--features main`, `main_frame.rs`) keeps the schedules, runs the
//! timers, answers Claude's tools and makes every change. The Schedules tab (`--features schedules`,
//! `schedules_page.rs`) and the "Allow this schedule?" modal (`--features approve`, `approve_page.rs`) read what is
//! saved themselves and ask main for a change.

// Each frame uses its own part of the crate.
#![allow(dead_code)]

mod approval;
#[cfg(feature = "main")]
mod approvals;
#[cfg(feature = "approve")]
mod approve_page;
mod assigned;
#[cfg(feature = "main")]
mod forge;
mod list;
#[cfg(any(feature = "schedules", feature = "approve"))]
mod page_ask;
#[cfg(feature = "main")]
mod main_frame;
mod schedule;
#[cfg(feature = "schedules")]
mod schedules_page;
mod store;
#[cfg(any(feature = "schedules", feature = "approve"))]
mod view;
#[cfg(feature = "main")]
mod tools;
#[cfg(feature = "main")]
mod watches;
mod worktree_setup;

/// What a page asks main: `{ op, ... }` for the change it wants (`messages::ask`), which main answers, with its error when it failed.
pub const ASK: &str = "ask";
/// What a page can't read itself: `{ targets, signInCode, approvals }`, which main tells on every change and a page
/// that opens asks for (`messages::want`).
pub const STATE: &str = "state";
