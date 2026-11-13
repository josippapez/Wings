//! Shows the pull request for each pane's branch (GitHub and GitHub Enterprise, GitLab and self-managed GitLab, Azure
//! DevOps, Bitbucket Cloud and Codeberg), with live status, every check and why it failed, the diff and review
//! comments. Lists each project's pull requests in the sidebar and in a block, notifies about new ones, review requests
//! and mentions, opens one as a tab to read, reply to and send to an agent, and tells an agent about its PR's news.
//!
//! The crate builds four frames. The main script (`--features main`, `main_frame.rs`) sets the badges and the project
//! sections, opens the diff viewer (which only a main can) and posts the replies typed in it, answers Claude's tool
//! and watches for news. The Pull requests block (`--features pulls`), the PR tab (`--features pr`) and the settings
//! page (`--features settings`) run the forges themselves, and ask main for what only it can do (`ipc.rs`).

// Each frame uses its own part of the crate.
#![allow(dead_code)]

mod args;
mod checklist;
mod finished;
mod forge;
mod forges;
mod inbox;
mod ipc;
mod logs;
#[cfg(feature = "main")]
mod track;
#[cfg(feature = "main")]
mod main_frame;
mod news;
#[cfg(feature = "pr")]
mod pr_page;
mod prefs;
mod problem;
mod providers;
#[cfg(feature = "pulls")]
mod pulls_page;
mod pulls;
mod pushes;
mod remote;
mod runlog;
#[cfg(feature = "settings")]
mod settings_page;
mod snippet;
mod threads;
mod types;
#[cfg(any(feature = "pr", feature = "pulls"))]
mod ui;
mod vote;
mod watch;
