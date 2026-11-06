//! Shared native Wings modules, independent of the UI runtime.

pub mod diff;
pub mod host;
pub mod page;
pub mod plugin;
pub mod projects;
pub mod saved;
pub mod secrets;
#[cfg(unix)]
pub mod terminal_server;
pub mod theme;
pub mod updater;

#[cfg(feature = "ui")]
pub mod actions;
#[cfg(feature = "ui")]
pub mod agents;
#[cfg(feature = "ui")]
pub mod icons;
#[cfg(feature = "ui")]
pub mod installed;
#[cfg(feature = "ui")]
pub mod layout;
#[cfg(feature = "ui")]
pub mod markdown;
#[cfg(feature = "ui")]
pub mod plugin_view;
#[cfg(feature = "ui")]
pub mod ports;
#[cfg(feature = "ui")]
pub mod terminal_view;
#[cfg(feature = "ui")]
pub mod text_field;
#[cfg(feature = "ui")]
pub mod transcripts;
#[cfg(feature = "ui")]
pub mod ui;
#[cfg(feature = "ui")]
pub mod workspace;
