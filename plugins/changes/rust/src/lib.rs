//! Shows what changed in each pane's git repo: a badge with the counts, a sidebar with the files as a tree and the
//! commit history, the commit graph, and the diff viewer for one file, all of them or one commit. Everything is local
//! git, about 25 ms a call.
//!
//! The crate builds three frames. The main script (`--features main`, `main_frame.rs`) sets the badges, answers
//! Claude's tools and opens the diff viewer, which only a main can. The sidebar (`--features sidebar`, `sidebar.rs`)
//! and the graph view (`--features graph`, `graph.rs`) run git themselves and ask main to open a diff.

// Each frame uses its own part of the crate.
#![allow(dead_code)]

mod batch;
mod changes;
mod dates;
#[cfg(any(feature = "main", test))]
mod diff;
#[cfg(feature = "graph")]
mod graph;
#[cfg(any(feature = "graph", feature = "sidebar", test))]
mod graph_log;
mod history;
mod layout;
mod log;
#[cfg(any(feature = "sidebar", test))]
mod mode;
#[cfg(feature = "main")]
mod main_frame;
#[cfg(feature = "sidebar")]
mod sidebar;

/// How often the badges and an open sidebar check again, in ms, while Wings is in front.
pub const POLL_MS: u32 = 15_000;

/// What a page tells main, each with an `id` main answers under with `DONE`: open the diff viewer on one changed file
/// or on all of them (`path` null), or on one commit, or open the graph view on a repo.
pub const OPEN: &str = "open";
pub const COMMIT: &str = "commit";
pub const GRAPH: &str = "graph";
/// What main tells back: `{ id, error }`, the error null when it opened.
pub const DONE: &str = "done";
/// The graph view asks which repo it shows, and main tells it: `{ root, name }`, both null when no pane is in a repo.
pub const GRAPH_WANT: &str = "graph-want";
pub const GRAPH_REPO: &str = "graph-repo";
