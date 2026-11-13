//! Plugin pages written in React, drawn with the native UI kit.
//!
//! A page is a bundle (`pages/*.tsx`, built by `pages/build.mjs`) of React and `wings-plugin-ui/native`. It runs
//! in its own QuickJS thread (`runtime`), which sends a tree each time React commits (`tree`), and `PageView`
//! draws that tree and sends clicks back by handler id. Like Ink or React Native, the page never touches a DOM.
//!
//! A plugin is a main script and its pages (`plugin`): each runs in its own thread, and they talk through
//! `wings.broadcast` and share `wings.storage`, as on the desktop.

pub mod log;
pub mod plugin;
pub mod runtime;
#[cfg(all(test, feature = "test-support"))]
mod tests;
pub mod tree;
#[cfg(feature = "ui")]
mod view;

#[cfg(feature = "ui")]
pub use view::PageView;
