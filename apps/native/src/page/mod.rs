//! Plugin pages, drawn with the native UI kit.
//!
//! A page is a WebAssembly module. It runs in its own thread (`runtime`), which sends a tree each time the page
//! renders (`tree`), and `PageView` draws that tree and sends clicks back by handler id. Like Ink or React Native, the
//! page never touches a DOM.
//!
//! A plugin is a main and its pages (`plugin`): each runs in its own thread, and they talk through
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
