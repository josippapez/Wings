//! Claude Code accounts: the default one and each `~/.claude-<name>` folder, and the one each project's terminals use. The
//! code lives in the shared `wings-spaces` crate, which the native app uses too; this puts it back under the name the rest
//! of the app uses.

pub use wings_spaces::claude_accounts::*;
