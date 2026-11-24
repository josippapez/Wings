//! What the pages and the main script say to each other (`lib/messages.ts`'s channel, over `messages::tell`): a page
//! asks for what only main can do or what main already holds, and main answers. A message over what one broadcast can
//! carry goes in parts (the SDK's `messages::ask`, `answer`, `send` and `hear`). These are the names.

/// A tab's or a block's ask of main for a project's open pull requests: `{ path }`, answered with the block's rows.
pub const PULLS: &str = "pulls";
/// The PR's status, which a tab tells main it loaded (`{ url, status }`), so main keeps the pane's badge on it fresh
/// without loading it again.
pub const PR_STATUS: &str = "pr-status";
/// A tab asks for what only main can do: `{ op: "diff" | "agent" | "seen" | "commit", args, focus?, commit? }`.
pub const PR: &str = "pr";
/// A block's or a tab's button: what the pane badge's Sign in, Add token or Install does for this project's host (`{ path }`).
pub const PULLS_ACTION: &str = "pulls-action";
/// Settings saved or removed a token, which main asks again with.
pub const TOKENS_CHANGED: &str = "tokens-changed";
/// Main loaded the hosts again after a sign-in or Try again: the pages load what they show again.
pub const REFRESH: &str = "refresh";
