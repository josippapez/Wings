//! What Claude Code tells its status line, passed on by `wings statusline`, for plugins to read with
//! `wings.statusline()`. The code lives in the shared `wings-connect` crate, which the native app uses too; this puts it
//! back under the name the rest of the app uses.

pub use wings_connect::statusline::*;
