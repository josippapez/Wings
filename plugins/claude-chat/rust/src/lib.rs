//! Claude chat (experimental): Claude Code as a chat in a tab instead of a terminal. The crate builds two frames. The
//! main (`--features main`, `main_frame.rs`) puts a Chat badge on Claude's terminals, whose button moves the session
//! into a chat tab. The chat page (`--features chat`, `chat.rs` and `view.rs`) runs Claude Code headless and draws the
//! session from what it prints (`session.rs`).

// Each frame uses its own part of the crate.
#![allow(dead_code)]

/// What a chat tab tells main when a turn ended, so the sidebar's Chats list reads the session again.
#[cfg(any(feature = "main", feature = "chat", test))]
pub const SESSION_SAVED: &str = "session-saved";

#[cfg(feature = "chat")]
mod chat;
#[cfg(any(feature = "main", test))]
mod main_frame;
#[cfg(any(feature = "chat", test))]
mod mentions;
mod session;
#[cfg(feature = "chat")]
mod view;
