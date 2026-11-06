//! The rules plugins run under, shared by the apps: manifests and how a plugin folder is checked, what each permission
//! lets a plugin do, the exec runner and installing plugins. Plugins never call the host directly: the UI relays their
//! requests, and everything that touches the machine is checked here against the permissions the manifest declares.

use std::{fmt::Arguments, sync::OnceLock};

mod cli_rules;
pub mod exec;
pub mod fetch;
pub mod loading;
pub mod manifest;
pub mod panes;
pub mod permissions;
pub mod post;
pub mod store;
pub mod transcript;

#[cfg(test)]
mod shipped;
#[cfg(test)]
mod test_support;

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

static WINGS_VERSION: OnceLock<&'static str> = OnceLock::new();

/// Tells this crate which Wings it runs in, the version a manifest's `minWingsVersion` is checked against: the app's own
/// `env!("CARGO_PKG_VERSION")`. Only the first call counts. Loading a plugin that names a `minWingsVersion` panics until it is set.
pub fn set_wings_version(version: &'static str) {
    let _ = WINGS_VERSION.set(version);
}

fn wings_version() -> &'static str {
    WINGS_VERSION.get().expect("wings_plugins::set_wings_version wasn't called")
}
