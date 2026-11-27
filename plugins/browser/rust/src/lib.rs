//! A web browser in a tab, drawn by WebKit: an address bar that searches, history suggestions, cookies kept between runs,
//! sign-in copied from another browser on this Mac and Safari web extensions.
//!
//! The crate builds three frames. The main script (`--features main`, `main_frame.rs`) has nothing to do: the plugin
//! needs one to load. Each browser tab (`--features browser`, `browser_page.rs`) and the settings page
//! (`--features settings`, `settings_page.rs`) read what is kept (history, settings) from storage themselves.

// Each frame uses its own part of the crate.
#![allow(dead_code)]

mod address;
mod agent_demo;
mod agent_js;
#[cfg(feature = "browser")]
mod agent_page;
mod agent_webmcp;
#[cfg(feature = "main")]
mod agent_tools;
#[cfg(feature = "browser")]
mod browser_page;
#[cfg(feature = "settings")]
mod engine_settings;
mod extension;
mod history;
#[cfg(feature = "main")]
mod main_frame;
mod settings;
#[cfg(feature = "settings")]
mod settings_page;
mod protocol;
mod start_page;
#[cfg(feature = "browser")]
mod tabs;
mod wait;
