//! `reveal`: scroll the active item of a list into view, by a named strategy. One word for gpui's two scroll handles'
//! `scroll_to_item`s, which differ in name and in what they take.
//!
//! - `Nearest`: only as far as it takes to show the item, and not at all when it already shows.
//! - `Top`: the item at the top of the viewport.
//! - `Center`: the item in the middle (as near it as the list's ends allow).

use gpui::{ScrollHandle, ScrollStrategy, UniformListScrollHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reveal {
    Nearest,
    Top,
    Center,
}

/// A scroll handle that can reveal the item at an index.
pub trait RevealExt {
    /// Reveals item `index` on the next draw.
    fn reveal(&self, index: usize, how: Reveal);
}

/// A list of virtualised rows. All three strategies map to gpui's own.
impl RevealExt for UniformListScrollHandle {
    fn reveal(&self, index: usize, how: Reveal) {
        self.scroll_to_item(
            index,
            match how {
                Reveal::Nearest => ScrollStrategy::Nearest,
                Reveal::Top => ScrollStrategy::Top,
                Reveal::Center => ScrollStrategy::Center,
            },
        );
    }
}

/// A plain scroll container, whose handle gpui gives `Nearest` (`scroll_to_item`) and `Top` (`scroll_to_top_of_item`)
/// and no centring: `Center` reveals as `Nearest` here.
impl RevealExt for ScrollHandle {
    fn reveal(&self, index: usize, how: Reveal) {
        match how {
            Reveal::Top => self.scroll_to_top_of_item(index),
            Reveal::Nearest | Reveal::Center => self.scroll_to_item(index),
        }
    }
}
