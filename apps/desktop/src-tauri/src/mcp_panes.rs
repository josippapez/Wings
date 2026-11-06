//! Wings' built-in MCP tools about the panes themselves (`list_sessions`, `read_pane`, `open_pane` and the rest). The
//! code lives in the shared `wings-connect` crate, which the native app uses too; this puts it back under the name the
//! rest of the app uses.

pub use wings_connect::panes::*;
