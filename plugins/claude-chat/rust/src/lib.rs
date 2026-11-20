//! Claude chat (experimental): Claude Code as a chat in a tab instead of a terminal. The crate builds three frames. The
//! main (`--features main`, `main_frame.rs`) puts a Chat badge on Claude's terminals, whose button moves the session
//! into a chat tab. The chat page (`--features chat`, `chat.rs` and `view.rs`) runs Claude Code headless and draws the
//! session from what it prints (`session.rs`). The subagent sidebar (`--features subagent`, which is the chat's code with
//! `subagent_sidebar.rs` as its start) is Wings' right sidebar for one subagent, and the pane sidebar (`--features pane`,
//! `pane_sidebar.rs`) is that sidebar for one pane a Claude Code plugin opened (`mod_pane.rs`). The session sidebar
//! (`--features session`, `session_sidebar.rs`) shows what the chat in front has on the go (`session_side.rs`). The settings page (`--features settings`,
//! `settings_page.rs`) is in Wings Settings and sets what every chat reads (`prefs.rs`).

// Each frame uses its own part of the crate.
#![allow(dead_code)]

/// What a chat tab tells main when a turn ended, so the sidebar's Chats list reads the session again.
#[cfg(any(feature = "main", feature = "chat", test))]
pub const SESSION_SAVED: &str = "session-saved";

mod actions;
#[cfg(feature = "chat")]
mod actions_view;
#[cfg(feature = "chat")]
mod branch;
#[cfg(feature = "chat")]
mod chat;
#[cfg(any(feature = "chat", test))]
mod code_view;
mod composer;
mod commands;
#[cfg(any(feature = "chat", test))]
mod copy;
mod diff_preview;
#[cfg(feature = "chat")]
mod diff_view;
mod edits;
#[cfg(feature = "chat")]
mod edits_view;
mod elicitation;
#[cfg(feature = "chat")]
mod elicitation_view;
mod folds;
#[cfg(any(feature = "chat", test))]
mod file_links;
#[cfg(any(feature = "chat", test))]
mod fork;
#[cfg(feature = "chat")]
mod fork_view;
#[cfg(any(feature = "main", all(test, not(feature = "chat"))))]
mod main_frame;
mod images;
#[cfg(feature = "chat")]
mod issues;
#[cfg(any(feature = "chat", test))]
mod mentions;
mod notices;
#[cfg(feature = "chat")]
mod pictures_view;
mod plan_limit;
mod plugins;
#[cfg(feature = "chat")]
mod plugins_view;
mod prefs;
#[cfg(feature = "chat")]
mod rail;
#[cfg(any(feature = "chat", test))]
mod reattach;
#[cfg(feature = "chat")]
mod rewind_view;
mod mods;
#[cfg(feature = "chat")]
mod mods_view;
#[cfg(feature = "chat")]
mod mod_pane;
#[cfg(feature = "pane")]
mod pane_sidebar;
mod refs;
mod remote;
#[cfg(feature = "chat")]
mod refs_view;
mod session;
#[cfg(feature = "chat")]
mod session_side;
#[cfg(feature = "session")]
mod session_sidebar;
#[cfg(feature = "settings")]
mod settings_page;
mod setup;
#[cfg(feature = "chat")]
mod setup_view;
#[cfg(feature = "chat")]
mod status_view;
#[cfg(feature = "chat")]
mod subagent_relay;
#[cfg(feature = "subagent")]
mod subagent_sidebar;
#[cfg(feature = "chat")]
mod subagent_tab;
#[cfg(feature = "chat")]
mod subagent_view;
mod subagents;
#[cfg(feature = "chat")]
mod tasks_view;
mod usage;
#[cfg(feature = "chat")]
mod usage_view;
#[cfg(feature = "chat")]
mod view;
#[cfg(feature = "chat")]
mod worktree;
