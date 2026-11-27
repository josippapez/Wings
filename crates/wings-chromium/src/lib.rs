//! Chromium for the Browser tab, as an engine Wings downloads instead of bundling.
//!
//! - [`engine`]: the downloaded folder (framework and helper apps) and the CEF settings that point at it.
//! - [`early_zone`]: the malloc zone the host registers first, so Chromium's allocator can take over malloc safely.
//! - [`app_protocol`]: the `NSApplication` methods CEF needs, added at run time to a class the host owns.
//! - [`pump`]: CEF's message loop work, run from the host's own run loop.
//! - [`pipe`]: the DevTools protocol with the browser target, over a pipe only this process holds.
//! - [`window`]: a frameless Chrome-style window with one browser, and gluing it over a host window.
#![cfg(target_os = "macos")]

pub mod app_protocol;
pub mod early_zone;
pub mod engine;
mod objc_runtime;
pub mod pipe;
pub mod pump;
pub mod sandbox_bundle;
pub mod window;

pub use cef;
