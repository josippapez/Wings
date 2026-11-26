//! What main and the pages say to each other, in both frames.

/// A page tells main about itself under this name, and main asks every page to again under `WHO`.
pub const ANNOUNCE: &str = "browser:page";
pub const WHO: &str = "browser:who";

/// The name a page answers main's asks under: one for each page, so only that page hears them.
pub fn cmd_name(id: &str) -> String {
    format!("browser:cmd:{id}")
}

/// How often a wait looks again: the page's state changes by itself, a script has to be asked.
pub const WAIT_POLL_MS: u32 = 250;
