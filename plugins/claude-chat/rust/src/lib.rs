//! Claude chat (experimental): Claude Code as a chat in a tab instead of a terminal. The crate builds three frames. The
//! main (`--features main`, `main_frame.rs`) puts a Chat badge on Claude's terminals, whose button moves the session
//! into a chat tab. The chat page (`--features chat`, `chat.rs` and `view.rs`) runs Claude Code headless and draws the
//! session from what it prints (`session.rs`). The settings page (`--features settings`, `settings_page.rs`) is in Wings
//! Settings and sets what every chat reads (`prefs.rs`).

// Each frame uses its own part of the crate.
#![allow(dead_code)]

/// What a chat tab tells main when a turn ended, so the sidebar's Chats list reads the session again.
#[cfg(any(feature = "main", feature = "chat", test))]
pub const SESSION_SAVED: &str = "session-saved";

#[cfg(feature = "chat")]
mod chat;
#[cfg(any(feature = "main", all(test, not(feature = "chat"))))]
mod main_frame;
#[cfg(any(feature = "chat", test))]
mod mentions;
#[cfg(feature = "chat")]
mod past_view;
mod plugins;
#[cfg(feature = "chat")]
mod plugins_view;
mod prefs;
mod session;
#[cfg(feature = "settings")]
mod settings_page;
mod usage;
#[cfg(feature = "chat")]
mod usage_view;
#[cfg(feature = "chat")]
mod view;
