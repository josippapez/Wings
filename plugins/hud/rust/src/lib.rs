//! What a terminal status line like claude-hud shows, in Wings instead: a strip under each Claude session's terminal
//! with your 5-hour and weekly limits, its context and any failed MCP server, and the HUD sidebar with the rest.
//!
//! The crate builds two frames: the main script (`--features main`, `main_frame.rs`), which follows the sessions and
//! keeps the state, and the sidebar page (`--features sidebar`, `sidebar.rs`), which draws what main tells it.

// Each frame uses its own half of the crate.
#![allow(dead_code)]

mod format;
#[cfg(feature = "main")]
mod main_frame;
mod session;
#[cfg(feature = "sidebar")]
mod sidebar;
mod view;

/// Where a gauge or a figure turns amber, then red. The strip under a pane, the limit alerts and the sidebar's gauges
/// all read these, so they can't drift apart. The context window's are claude-hud's.
pub const CONTEXT_WARN: f64 = 70.0;
pub const CONTEXT_CRITICAL: f64 = 85.0;
/// Usage limits and memory.
pub const LIMIT_WARN: f64 = 75.0;
pub const LIMIT_CRITICAL: f64 = 90.0;

/// What main tells the sidebar: the snapshot, under this name. The sidebar asks for it with `WANT` when it starts.
pub const SNAPSHOT: &str = "hud";
pub const WANT: &str = "hud:want";
