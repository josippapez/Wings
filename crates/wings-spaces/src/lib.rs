//! The projects ("spaces") in a Wings sidebar, persisted as JSON, and the git worktrees made of them. The desktop and
//! the native app each keep their own list file and share this code, so a project, a worktree and a project's worktree
//! setup mean the same in both.

pub mod claude_accounts;
pub mod spaces;
pub mod worktree;
