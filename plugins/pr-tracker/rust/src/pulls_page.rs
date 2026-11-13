//! The Pull requests block of a Project page: the project's open pull requests, yours first and then the ones waiting
//! for your review. Main owns the lists and their cache; this asks it, only while Wings is in front, and a click opens
//! the PR in a tab.

use crate::ui::pulls_list::{block_view, start_block};

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(block_view);
    start_block();
}
