//! What the pages draw: the parts the PR tab and the Pull requests block share, the list of a project's pull requests,
//! the PR with its checks and threads, and a check's run.
#[cfg(feature = "pr")]
pub mod checks_view;
#[cfg(feature = "pr")]
pub mod markdown;
pub mod parts;
#[cfg(feature = "pr")]
pub mod pr_view;
pub mod pulls_list;
#[cfg(feature = "pr")]
pub mod run_view;
