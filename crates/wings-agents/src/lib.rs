//! Finds the agent CLIs (Claude Code, opencode, Codex) running in terminal panes and what they are doing.

use std::{fmt::Arguments, sync::OnceLock};

pub mod agent_activity;
pub mod ai;
pub mod detect;
pub mod history;
pub mod opencode;
pub mod program_status;

/// The `space_id` of a pane that belongs to no project (the quick terminal): it starts in the home folder, and no
/// agent is detected in it, since the Agents list is grouped by project.
pub const NO_PROJECT: &str = "";

static LOGGER: OnceLock<fn(Arguments)> = OnceLock::new();

/// Sends this crate's log lines to `logger`. Until it is set they go to stderr. Only the first call counts.
pub fn set_logger(logger: fn(Arguments)) {
    let _ = LOGGER.set(logger);
}

fn log(message: Arguments) {
    match LOGGER.get() {
        Some(logger) => logger(message),
        None => eprintln!("{message}"),
    }
}

macro_rules! log_line {
    ($($arg:tt)*) => { $crate::log(format_args!($($arg)*)) };
}
pub(crate) use log_line;
