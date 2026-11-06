//! A new agent in its own git worktree. The code lives in the shared `wings-spaces` crate, so every app makes
//! worktrees the same way; this puts it back under the name the rest of the app uses.

pub use wings_spaces::worktree::*;
