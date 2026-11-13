//! A lightweight issue tracker for each project, for you and for Claude, shown as Issues: a key like `WIN-12`, a type, a
//! Markdown spec, a status, a priority, links, parents, related and blocking issues, the agent sessions that worked on it
//! and an activity log.
//!
//! The crate builds five frames. The main script (`--features main`, `main_frame.rs`) answers Claude's tools and hooks, writes
//! every change and fills the project sections. The board tab (`board`), the issue tab (`issue`), the Issues block (`block`)
//! and the settings page (`settings`) read the issues from storage themselves and ask main to change them.

// Each frame uses its own part of the crate.
#![allow(dead_code)]

mod actor;
mod add_issue;
mod ask;
#[cfg(feature = "block")]
mod block_page;
#[cfg(feature = "board")]
mod board_page;
mod claude_hooks;
mod commands;
mod delete_confirm;
mod domain;
mod feed;
mod file_diff;
mod format;
mod home;
#[cfg(feature = "issue")]
mod issue_page;
mod journal;
mod kv;
mod local_only;
mod log;
#[cfg(feature = "main")]
mod main_frame;
mod markdown;
mod ops;
mod orderings;
mod parts;
mod pr_check;
mod pr_state;
mod relations;
mod search;
mod session_start;
mod settings;
#[cfg(feature = "settings")]
mod settings_page;
mod sidebar;
mod storage;
#[cfg(test)]
mod testing;
mod tool_defs;
mod writer;
